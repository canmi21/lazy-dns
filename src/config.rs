/* src/config.rs */

use crate::records::ZoneConfig;
use chrono::{DateTime, Utc};
use fancy_log::{LogLevel, log};
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::SystemTime;

const DEFAULT_MAIN_CONFIG: &str = r#"
default_ttl = 5

[zones]
"example.com" = "example.com.zone.toml"
"#;

const DEFAULT_ZONE_FILE: &str = r#"
# SOA & NS records are optional, typically only for the zone apex.
[soa]
mname = "ns1.example.com."
rname = "admin.example.com."
# serial is auto-generated

# --- Apex / Root Domain Records (@) ---
[apex]
ns = ["ns1.example.com.", "ns2.example.com."]
a = ["192.0.2.1"]
aaaa = ["::1"]
txt = ["v=spf1 mx -all"]
mx = [
    { preference = 10, exchange = "mail.example.com." },
]

# GeoIP overrides for the apex domain.
[country]
US = { a = ["1.1.1.1", "1.0.0.1"] }
CN = { a = ["114.114.114.114"], aaaa = ["2400:3200::1"] }

# --- Subdomain Records ---
[www]
a = ["192.0.2.2"]
cname = ["alias.example.com."]

[www.country]
US = { a = ["2.2.2.2"] }
CN = { a = ["223.5.5.5"] }
"#;

#[derive(Debug, Deserialize)]
struct MainConfig {
    default_ttl: u32,
    #[serde(default)]
    zones: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnconfiguredPolicy {
    Drop,
    Refused,
    NxDomain,
}

impl FromStr for UnconfiguredPolicy {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "DROP" => Ok(Self::Drop),
            "REFUSED" => Ok(Self::Refused),
            "NXDOMAIN" => Ok(Self::NxDomain),
            _ => Err(()),
        }
    }
}

pub struct AppConfig {
    pub default_ttl: u32,
    pub zones: HashMap<String, ZoneConfig>,
    pub unconfigured_policy: UnconfiguredPolicy,
}

impl AppConfig {
    pub fn load_from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let base_path = env::var("CONFIG_PATH")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .expect("Could not find home directory")
                    .join("lazy-dns")
            });

        if !base_path.exists() {
            fs::create_dir_all(&base_path)?;
        }

        let main_config_path = base_path.join("config.toml");

        if !main_config_path.exists() {
            log(
                LogLevel::Warn,
                &format!(
                    "Main config not found. Creating default at {:?}",
                    main_config_path
                ),
            );
            fs::write(&main_config_path, DEFAULT_MAIN_CONFIG)?;
            let example_zone_path = base_path.join("example.com.zone.toml");
            fs::write(&example_zone_path, DEFAULT_ZONE_FILE)?;
        }

        log(
            LogLevel::Info,
            &format!("Loading main config from {:?}", main_config_path),
        );
        let main_config_str = fs::read_to_string(&main_config_path)?;
        let main_config: MainConfig = toml::from_str(&main_config_str)?;

        let mut loaded_zones = HashMap::new();
        for (domain, file_name) in main_config.zones {
            let zone_path = base_path.join(file_name);
            match load_zone_file(&zone_path) {
                Ok(zone_config) => {
                    log(
                        LogLevel::Info,
                        &format!("Loaded zone for '{}' from {:?}", domain, zone_path),
                    );
                    if !zone_config.apex.ns.is_empty() && zone_config.soa.is_none() {
                        log(
                            LogLevel::Error,
                            &format!("Zone '{}' has NS records but no SOA record.", domain),
                        );
                        continue;
                    }
                    loaded_zones.insert(domain, zone_config);
                }
                Err(e) => {
                    log(
                        LogLevel::Error,
                        &format!("Failed to load zone file {:?}: {}", zone_path, e),
                    );
                }
            }
        }

        if loaded_zones.is_empty() {
            log(
                LogLevel::Warn,
                "Config loaded, but no zones are configured or loaded successfully.",
            );
        }

        let unconfigured_policy = env::var("UNCONFIGURED_DOMAIN_POLICY")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(UnconfiguredPolicy::NxDomain);

        log(
            LogLevel::Info,
            &format!(
                "Unconfigured domain policy set to: {:?}",
                unconfigured_policy
            ),
        );

        Ok(AppConfig {
            default_ttl: main_config.default_ttl,
            zones: loaded_zones,
            unconfigured_policy,
        })
    }
}

fn load_zone_file(path: &Path) -> Result<ZoneConfig, Box<dyn std::error::Error>> {
    let metadata = fs::metadata(path)?;
    let modified_time = metadata.modified()?;
    let serial = generate_serial(modified_time);

    let content = fs::read_to_string(path)?;
    let mut zone: ZoneConfig = toml::from_str(&content)?;

    if let Some(soa) = &mut zone.soa {
        soa.serial = serial;
    }

    Ok(zone)
}

fn generate_serial(mod_time: SystemTime) -> u32 {
    let datetime: DateTime<Utc> = mod_time.into();
    let serial_str = datetime.format("%Y%m%d%H").to_string();
    serial_str.parse().unwrap_or_else(|_| {
        mod_time
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs() as u32
    })
}
