use std::{env::var, error, fmt, net::SocketAddr, str::FromStr, sync::LazyLock};

use anyhow::{anyhow, Context, Result};
use hickory_server::proto::rr::Name;
use log::debug;
use reqwest::Url;

use crate::{constants::*, registry::Registry};

fn create_error_msg(key: &str, value: &str) -> String {
    format!(
        "Failed to parse environment variable `{}` with value `{}`.",
        key, value
    )
}

fn get_parsed_env<T>(key: &str, default_value: Option<&str>) -> Result<T>
where
    T: FromStr,
    <T as FromStr>::Err: fmt::Display + fmt::Debug + error::Error + Send + Sync + 'static,
{
    match var(key) {
        Ok(value) => {
            debug!(
                "Environment variable `{}` found with value `{}`.",
                key, value
            );
            value.parse::<T>().context(create_error_msg(key, &value))
        }

        Err(_) => {
            if let Some(default_value) = default_value {
                debug!(
                    "Environment variable `{}` not found. Using default value `{}`.",
                    key, default_value
                );
                Ok(default_value.parse::<T>().unwrap())
            } else {
                Err(anyhow!("Environment variable `{}` not found.", key))
            }
        }
    }
}

static SERVER_LISTEN: LazyLock<Result<SocketAddr>> =
    LazyLock::new(|| get_parsed_env(SERVER_LISTEN_ENV, Some(DEFAULT_SERVER_LISTEN)));
static REGISTRY_LISTEN: LazyLock<Result<SocketAddr>> =
    LazyLock::new(|| get_parsed_env(REGISTRY_LISTEN_ENV, Some(DEFAULT_REGISTRY_LISTEN)));
static SELF_HOSTNAME: LazyLock<Result<Name>> =
    LazyLock::new(|| get_parsed_env(REGISTRY_HOSTNAME_ENV, None));
static REGISTRY_URLS: LazyLock<Result<String>> =
    LazyLock::new(|| get_parsed_env(REGISTRY_URLS_ENV, None));

pub struct Env {}

impl Env {
    fn get_server_listen() -> Result<SocketAddr> {
        match &*SERVER_LISTEN {
            Ok(server_listen) => Ok(server_listen.clone()),
            Err(err) => Err(anyhow!("{}", err)),
        }
    }

    fn get_registry_listen() -> Result<SocketAddr> {
        match &*REGISTRY_LISTEN {
            Ok(registry_listen) => Ok(registry_listen.clone()),
            Err(err) => Err(anyhow!("{}", err)),
        }
    }

    fn get_self_registry() -> Result<Registry> {
        let hostname = match &*SELF_HOSTNAME {
            Ok(self_hostname) => Ok(self_hostname.clone()),
            Err(err) => Err(anyhow!("{}", err)),
        }?;
        let url = Url::parse(&format!("http://{}", Self::get_registry_listen()?))?;

        Ok(Registry::new(hostname, url))
    }

    fn get_registries() -> Result<Vec<Registry>> {
        let urls = match &*REGISTRY_URLS {
            Ok(urls) => Ok(urls.clone()),
            Err(err) => Err(anyhow!("{}", err)),
        }?;

        let mut registries = vec![];
        for url in urls.split(" ") {
            registries.push(url.parse()?);
        }

        Ok(registries)
    }

    pub fn validate() -> Result<()> {
        Self::get_server_listen()?;
        Self::get_registry_listen()?;
        Self::get_self_registry()?;
        Self::get_registries()?;

        Ok(())
    }

    pub fn server_listen() -> SocketAddr {
        Self::get_server_listen().unwrap()
    }

    pub fn registry_listen() -> SocketAddr {
        Self::get_registry_listen().unwrap()
    }

    pub fn self_registry() -> Registry {
        Self::get_self_registry().unwrap()
    }

    pub fn registries() -> Vec<Registry> {
        Self::get_registries().unwrap()
    }
}
