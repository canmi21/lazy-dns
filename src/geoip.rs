/* src/geoip.rs */

use fancy_log::{LogLevel, log};
use serde::Deserialize;
use std::env;
use std::net::IpAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::Mutex;
use tokio::time::{Duration, sleep};

fn get_socket_path() -> String {
    env::var("GEOIP_SOCKET_PATH").unwrap_or_else(|_| "/tmp/lazy-mmdb.sock".to_string())
}

#[derive(Debug, Deserialize)]
struct CountryInfo {
    #[serde(rename = "iso_code")]
    iso_code: String,
}

#[derive(Debug, Deserialize)]
struct GeoIpResponse {
    country: CountryInfo,
}

pub struct GeoIpClient {
    is_available: Arc<Mutex<bool>>,
}

impl GeoIpClient {
    pub fn new() -> Self {
        Self {
            is_available: Arc::new(Mutex::new(false)),
        }
    }

    pub fn start_reconnect_task(&self) {
        let is_available = self.is_available.clone();
        tokio::spawn(async move {
            let reconnect_secs: u64 = env::var("GEOIP_RECONNECT_SECONDS")
                .unwrap_or_else(|_| "300".to_string())
                .parse()
                .unwrap_or(300);
            let check_interval = Duration::from_secs(reconnect_secs);
            let socket_path = get_socket_path();
            let mut first_run = true;

            loop {
                if !first_run {
                    sleep(check_interval).await;
                }
                first_run = false;

                let mut current_status = is_available.lock().await;
                match UnixStream::connect(&socket_path).await {
                    Ok(_) => {
                        if !*current_status {
                            log(
                                LogLevel::Info,
                                "lazy-mmdb service is available. GeoIP enabled.",
                            );
                            *current_status = true;
                        }
                    }
                    Err(_) => {
                        if *current_status {
                            log(
                                LogLevel::Warn,
                                "lazy-mmdb service has become unavailable. GeoIP disabled.",
                            );
                            *current_status = false;
                        } else {
                            log(
                                LogLevel::Debug,
                                &format!(
                                    "lazy-mmdb still unavailable. Retrying in {:?}",
                                    check_interval
                                ),
                            );
                        }
                    }
                }
            }
        });
    }

    pub async fn lookup(&self, ip: IpAddr) -> Option<String> {
        let socket_path = get_socket_path();

        if !*self.is_available.lock().await {
            return None;
        }

        let mut stream = match UnixStream::connect(&socket_path).await {
            Ok(s) => s,
            Err(_) => {
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

        if stream.write_all(request.as_bytes()).await.is_err() || stream.flush().await.is_err() {
            return None;
        }

        let mut response_buf = [0; 1024];
        if let Ok(n) = stream.read(&mut response_buf).await {
            if let Some(body) = String::from_utf8_lossy(&response_buf[..n])
                .split("\r\n\r\n")
                .nth(1)
            {
                if let Ok(data) = serde_json::from_str::<GeoIpResponse>(body.trim_end_matches('\0'))
                {
                    return Some(data.country.iso_code);
                }
            }
        }
        None
    }
}
