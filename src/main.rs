/* src/main.rs */

mod config;
mod dns_server;
mod geoip;
mod records;
mod resolver;

use crate::config::AppConfig;
use crate::geoip::GeoIpClient;
use crate::resolver::DnsResolver;
use dotenvy::dotenv;
use fancy_log::{LogLevel, log, set_log_level};
use lazy_motd::lazy_motd;
use std::env;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // --- Initialization ---
    dotenv().ok();
    let level = env::var("LOG_LEVEL")
        .unwrap_or_else(|_| "info".to_string())
        .to_lowercase();
    let log_level = match level.as_str() {
        "debug" => LogLevel::Debug,
        "warn" => LogLevel::Warn,
        "error" => LogLevel::Error,
        _ => LogLevel::Info,
    };
    set_log_level(log_level);
    lazy_motd!();

    // --- Load Config ---
    let config = match AppConfig::load_from_env() {
        Ok(cfg) => Arc::new(cfg),
        Err(e) => {
            log(LogLevel::Error, &format!("Failed to load config: {}", e));
            return Err(e);
        }
    };

    // --- Initialize Services ---
    let geoip_client = Arc::new(GeoIpClient::new());
    geoip_client.start_reconnect_task(); // Start background reconnection task

    let resolver = Arc::new(DnsResolver::new(config.clone(), geoip_client));

    // --- Start DNS Server ---
    let port = env::var("BIND_PORT").unwrap_or_else(|_| "53".to_string());
    let bind_addr = format!("0.0.0.0:{}", port);

    log(
        LogLevel::Info,
        &format!("Lazy DNS server starting on {}", bind_addr),
    );

    dns_server::run_server(&bind_addr, resolver).await?;

    Ok(())
}
