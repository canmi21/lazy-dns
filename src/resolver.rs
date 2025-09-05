/* src/resolver.rs */

use crate::config::{AppConfig, Records};
use crate::geoip::GeoIpClient;
use fancy_log::{LogLevel, log};
use hickory_proto::op::Query;
use hickory_proto::rr::rdata::{A, AAAA, CNAME};
use hickory_proto::rr::{Name, RData, Record, RecordType};
use rand::seq::SliceRandom;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;
use std::sync::Arc;

pub struct DnsResolver {
    config: Arc<AppConfig>,
    geoip: Arc<GeoIpClient>,
}

impl DnsResolver {
    pub fn new(config: Arc<AppConfig>, geoip: Arc<GeoIpClient>) -> Self {
        Self { config, geoip }
    }

    /// The main resolution logic.
    pub async fn resolve(&self, query: &Query, source_ip: IpAddr) -> Vec<Record> {
        let name_str = query.name().to_string().to_lowercase();
        // Remove trailing dot for map lookup
        let name_lookup = name_str.strip_suffix('.').unwrap_or(&name_str);

        let domain_config = match self.config.domains.get(name_lookup) {
            Some(cfg) => cfg,
            None => return vec![], // Domain not configured, return empty.
        };

        let ttl = domain_config.ttl.unwrap_or(self.config.default_ttl) * 60; // TTL in seconds
        let source_records = self.get_source_records(source_ip, domain_config).await;

        let mut records = Vec::new();
        let query_type = query.query_type();

        if query_type == RecordType::A || query_type == RecordType::ANY {
            records.extend(self.create_records(&name_str, ttl, &source_records.a, RecordType::A));
        }
        if query_type == RecordType::AAAA || query_type == RecordType::ANY {
            records.extend(self.create_records(
                &name_str,
                ttl,
                &source_records.aaaa,
                RecordType::AAAA,
            ));
        }
        if query_type == RecordType::CNAME || query_type == RecordType::ANY {
            records.extend(self.create_records(
                &name_str,
                ttl,
                &source_records.cname,
                RecordType::CNAME,
            ));
        }

        records
    }

    /// Get records based on GeoIP lookup or fall back to default.
    async fn get_source_records(
        &self,
        source_ip: IpAddr,
        domain_config: &crate::config::DomainConfig,
    ) -> Records {
        let is_private = match source_ip {
            IpAddr::V4(v4) => v4.is_private(),
            IpAddr::V6(v6) => v6.is_loopback(), // Basic private check for v6
        };

        if source_ip.is_loopback() || is_private {
            return self.get_default_records(domain_config);
        }

        if let Some(country_code) = self.geoip.lookup(source_ip).await {
            if let Some(geo_records) = domain_config.country.get(&country_code) {
                log(
                    LogLevel::Debug,
                    &format!("Found GeoIP match for {} -> {}", source_ip, country_code),
                );
                return geo_records.clone();
            }
        }

        self.get_default_records(domain_config)
    }

    fn get_default_records(&self, domain_config: &crate::config::DomainConfig) -> Records {
        Records {
            a: domain_config.a.clone(),
            aaaa: domain_config.aaaa.clone(),
            cname: domain_config.cname.clone(),
        }
    }

    /// Creates hickory-proto Records from string values, with random selection.
    fn create_records(
        &self,
        name_str: &str,
        ttl: u32,
        values: &[String],
        record_type: RecordType,
    ) -> Vec<Record> {
        if values.is_empty() {
            return vec![];
        }
        // Choose one record randomly to return, as per simple load balancing.
        let selected_value = values.choose(&mut rand::thread_rng()).unwrap();

        let name = Name::from_str(name_str).unwrap();
        let rdata = match record_type {
            RecordType::A => selected_value
                .parse::<Ipv4Addr>()
                .ok()
                .map(|ip| RData::A(A::from(ip))),
            RecordType::AAAA => selected_value
                .parse::<Ipv6Addr>()
                .ok()
                .map(|ip| RData::AAAA(AAAA::from(ip))),
            RecordType::CNAME => Name::from_str(selected_value)
                .ok()
                .map(|name| RData::CNAME(CNAME(name))),
            _ => None,
        };

        rdata.map_or(vec![], |data| vec![Record::from_rdata(name, ttl, data)])
    }
}
