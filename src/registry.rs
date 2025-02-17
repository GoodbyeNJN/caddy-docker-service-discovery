use std::{collections::HashSet, str::FromStr};

use anyhow::{anyhow, Context, Error, Result};
use hickory_server::proto::rr::{Name, RData};
use reqwest::Url;
use serde::{
    de::{self},
    Deserialize, Deserializer, Serialize, Serializer,
};

use crate::dns::Dns;

fn serialize_hostname<S>(hostname: &Name, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    hostname.to_string().serialize(serializer)
}

fn deserialize_hostname<'de, D>(deserializer: D) -> Result<Name, D::Error>
where
    D: Deserializer<'de>,
{
    let hostname = String::deserialize(deserializer)?;
    hostname.parse().map_err(de::Error::custom)
}

fn serialize_url<S>(url: &Url, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    url.as_str().serialize(serializer)
}

fn deserialize_url<'de, D>(deserializer: D) -> Result<Url, D::Error>
where
    D: Deserializer<'de>,
{
    let url = String::deserialize(deserializer)?;
    Url::parse(&url).map_err(de::Error::custom)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Registry {
    #[serde(
        serialize_with = "serialize_hostname",
        deserialize_with = "deserialize_hostname"
    )]
    hostname: Name,

    #[serde(serialize_with = "serialize_url", deserialize_with = "deserialize_url")]
    url: Url,

    public_services: HashSet<String>,

    private_services: HashSet<String>,
}

impl Registry {
    pub fn new(hostname: Name, url: Url) -> Self {
        Self {
            hostname,
            url,
            public_services: Default::default(),
            private_services: Default::default(),
        }
    }

    pub fn hostname(&self) -> &Name {
        &self.hostname
    }

    pub fn url(&self) -> &Url {
        &self.url
    }

    pub fn public_services(&self) -> &HashSet<String> {
        &self.public_services
    }

    pub fn private_services(&self) -> &HashSet<String> {
        &self.private_services
    }

    pub fn has_public_service(&self, service: &str) -> bool {
        self.public_services.contains(service)
    }

    pub fn has_private_service(&self, service: &str) -> bool {
        self.private_services.contains(service)
    }

    pub fn add_public_service(&mut self, service: String) {
        self.public_services.insert(service);
    }

    pub fn add_private_service(&mut self, service: String) {
        self.private_services.insert(service);
    }

    pub fn clear_public_services(&mut self) {
        self.public_services.clear();
    }

    pub fn clear_private_services(&mut self) {
        self.private_services.clear();
    }

    pub fn flush_public_services(&mut self, services: HashSet<String>) {
        self.public_services = services;
    }
}

impl FromStr for Registry {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let url = Url::parse(s).context(format!("Failed to parse URL `{}`.", s))?;
        let hostname = url
            .host_str()
            .ok_or(anyhow!("No hostname found in URL `{}`.", s))?;
        let hostname = hostname
            .parse()
            .context(format!("Failed to parse hostname `{}`.", hostname))?;

        Ok(Self::new(hostname, url))
    }
}

impl TryInto<RData> for Registry {
    type Error = Error;

    fn try_into(self) -> Result<RData> {
        let data = Dns::query_upstream(&self.hostname.to_string());
        if let Some(data) = data {
            Ok(data)
        } else {
            Err(anyhow!(
                "No IPv4 address found for hostname `{}`.",
                self.hostname
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::net::Ipv4Addr;

    use hickory_server::proto::rr::{rdata::A, RecordData};

    #[test]
    fn test_registry_from_str() {
        let registry = Registry::from_str("http://localhost:8080").unwrap();
        assert_eq!(registry.hostname(), &Name::from_str("localhost").unwrap());
        assert_eq!(
            registry.url(),
            &Url::parse("http://localhost:8080").unwrap()
        );
    }

    #[test]
    fn test_registry_try_into_record() {
        let registry = Registry::from_str("http://localhost:8080").unwrap();
        let data: RData = registry.try_into().unwrap();
        assert_eq!(data, A(Ipv4Addr::new(127, 0, 0, 1)).into_rdata());
    }
}
