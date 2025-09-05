/* src/config.rs */

use fancy_log::{LogLevel, log};
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;

const DEFAULT_CONFIG_TEMPLATE: &str = r#"
# Default TTL for all records in minutes, if not specified per domain.
default_ttl = 5

[domains]

# --- Example Records ---

# A simple domain with A and AAAA records.
[domains."test.local"]
a = ["127.0.0.1"]
aaaa = ["::1"]

# A domain with multiple A records for random selection (load balancing).
[domains."roundrobin.local"]
a = ["192.168.1.10", "192.168.1.20"]
ttl = 1

# A domain where CNAME coexists with an A record.
[domains."multi-cname.local"]
a = ["1.2.3.4"]
cname = ["alias.test.local"]

# A domain with GeoIP routing rules.
[domains."geo.local"]
# Default records for visitors from unknown locations or when GeoIP is down.
a = ["8.8.8.8"]
cname = ["default.geo.local"]

# Country-specific overrides.
[domains."geo.local".country]
US = { a = ["1.1.1.1", "1.0.0.1"], cname = ["us.geo.local"] }
CN = { a = ["114.114.114.114"], aaaa = ["2400:3200::1"] }
JP = { cname = ["jp.geo.local"] }
"#;

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub default_ttl: u32,
    #[serde(default)]
    pub domains: HashMap<String, DomainConfig>,
}

#[derive(Debug, Deserialize)]
pub struct DomainConfig {
    pub ttl: Option<u32>,
    #[serde(default)]
    pub a: Vec<String>,
    #[serde(default)]
    pub aaaa: Vec<String>,
    #[serde(default)]
    pub cname: Vec<String>,
    #[serde(default)]
    pub country: HashMap<String, Records>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Records {
    #[serde(default)]
    pub a: Vec<String>,
    #[serde(default)]
    pub aaaa: Vec<String>,
    #[serde(default)]
    pub cname: Vec<String>,
}

impl AppConfig {
    /// Loads config from path specified in .env or defaults to `~/lazy-dns/config.toml`.
    pub fn load_or_create_default() -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = env::var("CONFIG_PATH")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .expect("Could not find home directory")
                    .join("lazy-dns")
                    .join("config.toml")
            });

        if !config_path.exists() {
            log(
                LogLevel::Warn,
                &format!(
                    "Config file not found. Creating default at {:?}",
                    config_path
                ),
            );
            if let Some(parent_dir) = config_path.parent() {
                fs::create_dir_all(parent_dir)?;
            }
            fs::write(&config_path, DEFAULT_CONFIG_TEMPLATE)?;
        }

        log(
            LogLevel::Info,
            &format!("Loading config from {:?}", config_path),
        );
        let config_str = fs::read_to_string(&config_path)?;
        let config: AppConfig = toml::from_str(&config_str)?;

        if config.domains.is_empty() {
            log(
                LogLevel::Warn,
                "Config loaded, but no domains are configured.",
            );
        }

        Ok(config)
    }
}
