/* src/resolver.rs */

use crate::config::AppConfig;
use crate::geoip::GeoIpClient;
use crate::records::{RecordSet, ZoneConfig};
use fancy_log::{LogLevel, log};
use hickory_proto::op::Query;
use hickory_proto::rr::rdata::{self, A, AAAA, CNAME, MX, SOA, TXT};
use hickory_proto::rr::{Name, RData, Record, RecordType};
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

    pub fn config(&self) -> &Arc<AppConfig> {
        &self.config
    }

    pub async fn resolve(&self, query: &Query, source_ip: IpAddr) -> Vec<Record> {
        let q_name_str = query.name().to_string();
        let q_name_str_lower = q_name_str.to_lowercase();
        let q_name_lookup = q_name_str_lower
            .strip_suffix('.')
            .unwrap_or(&q_name_str_lower);

        let (zone_name, zone_config) = match self.find_zone(q_name_lookup) {
            Some(zone) => zone,
            None => return vec![],
        };

        let subdomain_part = q_name_lookup
            .strip_suffix(zone_name)
            .map(|s| s.strip_suffix('.').unwrap_or(s))
            .filter(|s| !s.is_empty());

        let ttl = zone_config.ttl.unwrap_or(self.config.default_ttl) * 60;
        let records = self
            .get_records_for_query(source_ip, zone_config, subdomain_part)
            .await;

        log(
            LogLevel::Debug,
            &format!("Found records for query '{}': {:?}", q_name_lookup, records),
        );

        self.build_response_records(&q_name_str, query.query_type(), ttl, &records)
    }

    fn find_zone<'a>(&'a self, query_name: &'a str) -> Option<(&'a str, &'a ZoneConfig)> {
        self.config
            .zones
            .iter()
            .filter(|(zone_name, _)| query_name.ends_with(*zone_name))
            .max_by_key(|(zone_name, _)| zone_name.len())
            .map(|(name, config)| (name.as_str(), config))
    }

    async fn get_records_for_query(
        &self,
        source_ip: IpAddr,
        zone_config: &ZoneConfig,
        subdomain: Option<&str>,
    ) -> RecordSet {
        let (default_records, geo_map) = if let Some(sub_name) = subdomain {
            if let Some(sub_config) = zone_config.subdomains.get(sub_name) {
                (&sub_config.records, &sub_config.country)
            } else {
                return RecordSet::default();
            }
        } else {
            // Use the explicit 'apex' field for apex queries
            (&zone_config.apex, &zone_config.country)
        };

        if let Some(geo_records) = self.get_geo_records(source_ip, geo_map).await {
            return geo_records;
        }

        default_records.clone()
    }

    async fn get_geo_records(
        &self,
        source_ip: IpAddr,
        geo_map: &std::collections::HashMap<String, RecordSet>,
    ) -> Option<RecordSet> {
        let is_private = matches!(source_ip, IpAddr::V4(v4) if v4.is_private());
        if source_ip.is_loopback() || is_private {
            return None;
        }

        if let Some(country_code) = self.geoip.lookup(source_ip).await {
            if let Some(records) = geo_map.get(&country_code) {
                log(
                    LogLevel::Debug,
                    &format!("Found GeoIP match for {} -> {}", source_ip, country_code),
                );
                return Some(records.clone());
            }
        }
        None
    }

    fn build_response_records(
        &self,
        q_name: &str,
        q_type: RecordType,
        ttl: u32,
        records: &RecordSet,
    ) -> Vec<Record> {
        let mut answers = Vec::new();
        let name = Name::from_str(q_name).unwrap();

        if q_type == RecordType::A || q_type == RecordType::ANY {
            answers.extend(self.create_a_records(&name, ttl, &records.a));
        }
        if q_type == RecordType::AAAA || q_type == RecordType::ANY {
            answers.extend(self.create_aaaa_records(&name, ttl, &records.aaaa));
        }
        if q_type == RecordType::CNAME || q_type == RecordType::ANY {
            answers.extend(self.create_cname_records(&name, ttl, &records.cname));
        }
        if q_type == RecordType::MX || q_type == RecordType::ANY {
            answers.extend(self.create_mx_records(&name, ttl, &records.mx));
        }
        if q_type == RecordType::TXT || q_type == RecordType::ANY {
            answers.extend(self.create_txt_records(&name, ttl, &records.txt));
        }
        if q_type == RecordType::NS || q_type == RecordType::ANY {
            answers.extend(self.create_ns_records(&name, ttl, &records.ns));
        }

        let q_name_lookup = q_name.strip_suffix('.').unwrap_or(q_name);
        if q_type == RecordType::SOA
            && self
                .find_zone(q_name_lookup)
                .map_or(false, |(zn, _)| zn == q_name_lookup)
        {
            if let Some(zone_config) = self.find_zone(q_name_lookup).map(|(_, zc)| zc) {
                if let Some(soa_rec) = self.create_soa_record(&name, ttl, zone_config) {
                    answers.push(soa_rec);
                }
            }
        }

        answers
    }

    fn create_soa_record(&self, name: &Name, ttl: u32, zone: &ZoneConfig) -> Option<Record> {
        zone.soa.as_ref().map(|soa_config| {
            let rdata = RData::SOA(SOA::new(
                Name::from_str(&soa_config.mname).unwrap(),
                Name::from_str(&soa_config.rname).unwrap(),
                soa_config.serial,
                soa_config.refresh.unwrap_or(86400) as i32,
                soa_config.retry.unwrap_or(7200) as i32,
                soa_config.expire.unwrap_or(3600000) as i32,
                soa_config.minimum.unwrap_or(300),
            ));
            Record::from_rdata(name.clone(), ttl, rdata)
        })
    }

    fn create_ns_records(&self, name: &Name, ttl: u32, values: &[String]) -> Vec<Record> {
        values
            .iter()
            .filter_map(|val| Name::from_str(val).ok())
            .map(|ns_name| Record::from_rdata(name.clone(), ttl, RData::NS(rdata::NS(ns_name))))
            .collect()
    }

    fn create_a_records(&self, name: &Name, ttl: u32, values: &[String]) -> Vec<Record> {
        values
            .iter()
            .filter_map(|val| val.parse::<Ipv4Addr>().ok())
            .map(|ip| Record::from_rdata(name.clone(), ttl, RData::A(A::from(ip))))
            .collect()
    }

    fn create_aaaa_records(&self, name: &Name, ttl: u32, values: &[String]) -> Vec<Record> {
        values
            .iter()
            .filter_map(|val| val.parse::<Ipv6Addr>().ok())
            .map(|ip| Record::from_rdata(name.clone(), ttl, RData::AAAA(AAAA::from(ip))))
            .collect()
    }

    fn create_cname_records(&self, name: &Name, ttl: u32, values: &[String]) -> Vec<Record> {
        values
            .iter()
            .filter_map(|val| Name::from_str(val).ok())
            .map(|cname| Record::from_rdata(name.clone(), ttl, RData::CNAME(CNAME(cname))))
            .collect()
    }

    fn create_mx_records(
        &self,
        name: &Name,
        ttl: u32,
        values: &[crate::records::MXRecord],
    ) -> Vec<Record> {
        values
            .iter()
            .filter_map(|val| {
                Name::from_str(&val.exchange)
                    .ok()
                    .map(|exchange| (val.preference, exchange))
            })
            .map(|(preference, exchange)| {
                Record::from_rdata(name.clone(), ttl, RData::MX(MX::new(preference, exchange)))
            })
            .collect()
    }

    fn create_txt_records(&self, name: &Name, ttl: u32, values: &[String]) -> Vec<Record> {
        values
            .iter()
            .map(|val| {
                Record::from_rdata(name.clone(), ttl, RData::TXT(TXT::new(vec![val.clone()])))
            })
            .collect()
    }
}
