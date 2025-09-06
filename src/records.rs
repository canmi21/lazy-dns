/* src/records.rs */

use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize, Clone)]
pub struct SOARecord {
    pub mname: String,
    pub rname: String,
    pub refresh: Option<u32>,
    pub retry: Option<u32>,
    pub expire: Option<u32>,
    pub minimum: Option<u32>,
    #[serde(skip)]
    pub serial: u32,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MXRecord {
    pub preference: u16,
    pub exchange: String,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct RecordSet {
    #[serde(default)]
    pub a: Vec<String>,
    #[serde(default)]
    pub aaaa: Vec<String>,
    #[serde(default)]
    pub cname: Vec<String>,
    #[serde(default)]
    pub mx: Vec<MXRecord>,
    #[serde(default)]
    pub txt: Vec<String>,
    #[serde(default)]
    pub ns: Vec<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ZoneConfig {
    pub ttl: Option<u32>,
    pub soa: Option<SOARecord>,
    #[serde(default)]
    pub apex: RecordSet, // Apex records are now explicitly here
    #[serde(default)]
    pub country: HashMap<String, RecordSet>, // GeoIP for Apex
    #[serde(default, flatten)]
    pub subdomains: HashMap<String, Subdomain>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Subdomain {
    #[serde(flatten)]
    pub records: RecordSet,
    #[serde(default)]
    pub country: HashMap<String, RecordSet>,
}
