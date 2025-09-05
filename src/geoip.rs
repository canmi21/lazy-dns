/* src/geoip.rs */

use fancy_log::{LogLevel, log};
use parking_lot::RwLock;
use serde::Deserialize;
use std::env;
use std::mem;
use std::net::IpAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::time::{Duration, sleep};

const SOCKET_PATH: &str = "/tmp/lazy-mmdb.sock";

#[derive(Debug, Deserialize)]
struct CountryInfo {
    #[serde(rename = "iso_code")]
    iso_code: String,
}

#[derive(Debug, Deserialize)]
struct GeoIpResponse {
    country: CountryInfo,
}

#[derive(Debug)]
enum GeoIpStatus {
    Available(UnixStream),
    Unavailable,
}

pub struct GeoIpClient {
    status: Arc<RwLock<GeoIpStatus>>,
}

impl GeoIpClient {
    pub fn new() -> Self {
        Self {
            status: Arc::new(RwLock::new(GeoIpStatus::Unavailable)),
        }
    }

    /// Spawns a background task to connect and periodically reconnect.
    pub fn start_reconnect_task(&self) {
        let status = self.status.clone();
        tokio::spawn(async move {
            let reconnect_secs: u64 = env::var("GEOIP_RECONNECT_SECONDS")
                .unwrap_or_else(|_| "300".to_string())
                .parse()
                .unwrap_or(300);
            let reconnect_interval = Duration::from_secs(reconnect_secs);

            loop {
                log(
                    LogLevel::Info,
                    "Attempting to connect to lazy-mmdb service...",
                );
                match UnixStream::connect(SOCKET_PATH).await {
                    Ok(stream) => {
                        log(
                            LogLevel::Info,
                            "Successfully connected to lazy-mmdb. GeoIP is now available.",
                        );
                        *status.write() = GeoIpStatus::Available(stream);
                        // A long sleep. The connection will be checked on the next lookup.
                        sleep(Duration::from_secs(u64::MAX)).await;
                    }
                    Err(_) => {
                        log(
                            LogLevel::Warn,
                            &format!(
                                "lazy-mmdb service not available. Will retry in {:?}",
                                reconnect_interval
                            ),
                        );
                        *status.write() = GeoIpStatus::Unavailable;
                        sleep(reconnect_interval).await;
                    }
                }
            }
        });
    }

    /// Looks up the country code for a given IP address.
    pub async fn lookup(&self, ip: IpAddr) -> Option<String> {
        // Take the stream out of the lock, leaving Unavailable in its place.
        // The write lock is immediately released after this statement.
        let mut stream = match mem::replace(&mut *self.status.write(), GeoIpStatus::Unavailable) {
            GeoIpStatus::Available(s) => s,
            GeoIpStatus::Unavailable => return None,
        };

        let request = format!(
            "GET /lookup/country?ip={} HTTP/1.1\r\nHost: localhost\r\n\r\n",
            ip
        );

        // Perform async I/O without holding the lock.
        let mut io_successful = true;
        if stream.write_all(request.as_bytes()).await.is_err() {
            io_successful = false;
        }
        if io_successful && stream.flush().await.is_err() {
            io_successful = false;
        }

        if !io_successful {
            log(
                LogLevel::Warn,
                "Connection to lazy-mmdb lost on write. Will reconnect in background.",
            );
            return None; // Do not put the broken stream back.
        }

        let mut response_buf = [0; 1024];
        let country_code = match stream.read(&mut response_buf).await {
            Ok(n) if n > 0 => {
                let response_str = String::from_utf8_lossy(&response_buf[..n]);
                if let Some(body) = response_str.split("\r\n\r\n").nth(1) {
                    serde_json::from_str::<GeoIpResponse>(body.trim_end_matches('\0'))
                        .map(|data| data.country.iso_code)
                        .ok()
                } else {
                    None
                }
            }
            Ok(_) | Err(_) => {
                // Ok(0) means closed connection, Err is an I/O error.
                io_successful = false;
                None
            }
        };

        // If all I/O was successful, put the stream back for the next user.
        if io_successful {
            *self.status.write() = GeoIpStatus::Available(stream);
        } else {
            log(
                LogLevel::Warn,
                "Connection to lazy-mmdb lost on read. Will reconnect in background.",
            );
        }

        country_code
    }
}
