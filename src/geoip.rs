/* src/geoip.rs */

use fancy_log::{LogLevel, log};
use serde::Deserialize;
use std::env;
use std::net::IpAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::Mutex; // Using tokio's Mutex for async code
use tokio::time::{Duration, sleep};

const SOCKET_PATH: &str = "/tmp/lazy-mmdb/lazy-mmdb.sock";

#[derive(Debug, Deserialize)]
struct CountryInfo {
    #[serde(rename = "iso_code")]
    iso_code: String,
}

#[derive(Debug, Deserialize)]
struct GeoIpResponse {
    country: CountryInfo,
}

// We no longer store the stream, just a boolean flag indicating availability.
pub struct GeoIpClient {
    is_available: Arc<Mutex<bool>>,
}

impl GeoIpClient {
    pub fn new() -> Self {
        Self {
            is_available: Arc::new(Mutex::new(false)),
        }
    }

    /// Spawns a background task to periodically check for service availability.
    pub fn start_reconnect_task(&self) {
        let is_available = self.is_available.clone();
        tokio::spawn(async move {
            let reconnect_secs: u64 = env::var("GEOIP_RECONNECT_SECONDS")
                .unwrap_or_else(|_| "300".to_string())
                .parse()
                .unwrap_or(300);
            let check_interval = Duration::from_secs(reconnect_secs);

            // Run an initial check immediately.
            let mut initial_check = true;

            loop {
                if !initial_check {
                    sleep(check_interval).await;
                }
                initial_check = false;

                // Check if the socket file exists and we can connect.
                if UnixStream::connect(SOCKET_PATH).await.is_ok() {
                    let mut avail = is_available.lock().await;
                    if !*avail {
                        log(
                            LogLevel::Info,
                            "lazy-mmdb service is available. GeoIP enabled.",
                        );
                        *avail = true;
                    }
                } else {
                    let mut avail = is_available.lock().await;
                    if *avail {
                        log(
                            LogLevel::Warn,
                            "lazy-mmdb service has become unavailable. GeoIP disabled.",
                        );
                        *avail = false;
                    } else {
                        log(
                            LogLevel::Warn,
                            &format!(
                                "lazy-mmdb still unavailable. Retrying in {:?}",
                                check_interval
                            ),
                        );
                    }
                }
            }
        });
    }

    /// Looks up the country code for a given IP address by creating a new connection each time.
    pub async fn lookup(&self, ip: IpAddr) -> Option<String> {
        // Quick check: if the service is marked as unavailable, don't even try to connect.
        if !*self.is_available.lock().await {
            return None;
        }

        // Create a new connection for every lookup.
        let mut stream = match UnixStream::connect(SOCKET_PATH).await {
            Ok(s) => s,
            Err(_) => {
                // If connection fails, the service likely went down. Mark it as unavailable.
                let mut avail = self.is_available.lock().await;
                if *avail {
                    log(
                        LogLevel::Warn,
                        "Failed to connect to lazy-mmdb for lookup. Marking as unavailable.",
                    );
                    *avail = false;
                }
                return None;
            }
        };

        let request = format!(
            "GET /lookup/country?ip={} HTTP/1.1\r\nHost: localhost\r\n\r\n",
            ip
        );

        // This is a one-shot connection, so if any I/O fails, we just abandon it.
        if stream.write_all(request.as_bytes()).await.is_err() || stream.flush().await.is_err() {
            return None;
        }

        let mut response_buf = [0; 1024];
        if let Ok(n) = stream.read(&mut response_buf).await {
            let response_str = String::from_utf8_lossy(&response_buf[..n]);
            if let Some(body) = response_str.split("\r\n\r\n").nth(1) {
                if let Ok(data) = serde_json::from_str::<GeoIpResponse>(body.trim_end_matches('\0'))
                {
                    // Success!
                    return Some(data.country.iso_code);
                }
            }
        }

        None // Return None on read error or parse error.
    }
}
