use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, LazyLock},
};

use anyhow::{Context, Result};
use bollard::{
    container::ListContainersOptions, secret::ContainerSummary, system::EventsOptions,
    Docker as DockerSocket,
};
use futures_util::stream::StreamExt;
use log::{debug, error, info};
use regex::Regex;
use tokio::sync::Mutex;

use crate::{constants::*, registry::Registry};

static CADDY_LABEL_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^caddy$|^caddy_\d+$").unwrap());
static SNIPPET_VALUE_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\(.*\)$").unwrap());
static PUBLIC_TLD_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?:https?://)?(.*)\.{}(?::\d+)?$",
        PUBLIC_SERVICE_TLD
    ))
    .unwrap()
});
static PRIVATE_TLD_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?:https?://)?(.*)\.{}(?::\d+)?$",
        PRIVATE_SERVICE_TLD
    ))
    .unwrap()
});

pub struct Docker {
    pub socket: DockerSocket,
}

impl Docker {
    pub fn new() -> Result<Self> {
        let socket = DockerSocket::connect_with_unix_defaults()
            .context("Failed to connect to Docker socket.")?;

        Ok(Self { socket })
    }

    async fn list_running_containers(&self) -> Result<Vec<ContainerSummary>> {
        self.socket
            .list_containers(Some(ListContainersOptions {
                all: true,
                filters: HashMap::from_iter(vec![("status", vec!["running"])]),
                ..Default::default()
            }))
            .await
            .context(format!("Failed to list running containers."))
    }

    fn get_container_name(container: &ContainerSummary) -> String {
        container
            .names
            .clone()
            .map_or("unknown".to_string(), |names| {
                names.get(0).map_or("unknown".to_string(), |name| {
                    name.trim_start_matches('/').to_string()
                })
            })
    }

    fn get_caddy_values(container: &ContainerSummary) -> Vec<String> {
        container
            .labels
            .clone()
            .map_or(Default::default(), |labels| {
                labels
                    .iter()
                    .filter(|(label, value)| {
                        CADDY_LABEL_REGEX.is_match(label) && !SNIPPET_VALUE_REGEX.is_match(value)
                    })
                    .map(|(_, value)| value.clone())
                    .collect()
            })
    }

    fn parse_address(address: &str) -> Vec<String> {
        let mut list = vec![];

        for address in address.split(",") {
            for address in address.trim().split(" ") {
                let address = address.trim();
                if !address.is_empty() {
                    list.push(address.to_string());
                }
            }
        }

        list
    }

    fn capture_service(address: &str, regex: &Regex) -> Option<String> {
        regex
            .captures(address)
            .map(|captures| captures.get(1).unwrap().as_str().to_string())
    }

    pub async fn flush_registry_services(&self, registry: Arc<Mutex<Registry>>) {
        let mut registry = registry.lock().await;
        registry.clear_public_services();
        registry.clear_private_services();

        let mut process_address = |address: &String| {
            if let Some(service) = Self::capture_service(address, &PUBLIC_TLD_REGEX) {
                debug!(
                    "Captured public service `{}` from address `{}`",
                    service, address
                );
                registry.add_public_service(service);
            } else if let Some(service) = Self::capture_service(address, &PRIVATE_TLD_REGEX) {
                debug!(
                    "Captured private service `{}` from address `{}`",
                    service, address
                );
                registry.add_private_service(service);
            }
        };

        let mut process_container = |container: ContainerSummary| {
            let values = Self::get_caddy_values(&container);
            debug!(
                "Found Caddy label values for container `{}`: {:?}",
                Self::get_container_name(&container),
                values
            );

            for value in values {
                for address in Self::parse_address(&value) {
                    process_address(&address);
                }
            }
        };

        info!("Flushing services for self registry.",);
        match self.list_running_containers().await {
            Ok(containers) => {
                for container in containers {
                    process_container(container);
                }

                info!(
                    "Flushed public services for self registry: {:?}",
                    registry.public_services()
                );
                info!(
                    "Flushed private services for self registry: {:?}",
                    registry.private_services()
                );
            }
            Err(err) => {
                error!("{}", err);
            }
        }
    }

    pub async fn watch_events<F, Fut>(&self, callback: F)
    where
        F: Fn() -> Fut + Send,
        Fut: Future<Output = ()> + Send,
    {
        let mut events = self.socket.events(Some(EventsOptions {
            filters: HashMap::from_iter(vec![("type", vec!["container"])]),
            ..Default::default()
        }));

        while let Some(event) = events.next().await {
            let action = event
                .map_err(|err| {
                    error!("Failed to watch Docker events.\nError: {}", err);
                    err
                })
                .map(|event| event.action)
                .ok()
                .flatten();

            if let Some(action) = action {
                if action == "start" {
                    info!("Detected container start event.");
                    callback().await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_parse_address() {
        let test_cases = vec![
            // single address
            ("192.168.1.1", vec!["192.168.1.1"]),
            // address with extra spaces
            ("  192.168.1.1  ", vec!["192.168.1.1"]),
            // comma-separated addresses without spaces
            (
                "192.168.1.1,192.168.1.2",
                vec!["192.168.1.1", "192.168.1.2"],
            ),
            // space-separated addresses
            (
                "192.168.1.1 192.168.1.2",
                vec!["192.168.1.1", "192.168.1.2"],
            ),
            // mixed comma and space separation
            (
                " 192.168.1.1, 192.168.1.2 192.168.1.3 ,192.168.1.4 ",
                vec!["192.168.1.1", "192.168.1.2", "192.168.1.3", "192.168.1.4"],
            ),
            // empty and whitespace-only segments
            ("   ,  ", Vec::<&str>::new()),
        ];

        for (input, expected) in test_cases {
            let expected: Vec<String> = expected.into_iter().map(String::from).collect();
            let result = Docker::parse_address(input);
            assert_eq!(result, expected, "Failed for input: {:?}", input);
        }
    }

    #[tokio::test]
    async fn test_capture_service() {
        // Test with PUBLIC_TLD_REGEX matches.
        let public_tests = vec![
            // Assuming PUBLIC_SERVICE_TLD is "public", for example "service.public" should capture "service"
            ("service.pub", "service"),
            ("another-service.pub:8080", "another-service"),
            ("sub.domain.pub", "sub.domain"),
            ("http://service.pub", "service"),
            ("http://another-service.pub:8080", "another-service"),
            ("http://sub.domain.pub", "sub.domain"),
        ];
        for (input, expected) in public_tests {
            let result = Docker::capture_service(input, &PUBLIC_TLD_REGEX);
            assert_eq!(
                result,
                Some(expected.to_string()),
                "Failed to capture public service from: {}",
                input
            );
        }

        // Test with PRIVATE_TLD_REGEX matches.
        let private_tests = vec![
            // Assuming PRIVATE_SERVICE_TLD is "private", for example "service.private" should capture "service"
            ("service.priv", "service"),
            ("another-service.priv:3000", "another-service"),
            ("sub.domain.priv", "sub.domain"),
            ("http://service.priv", "service"),
            ("http://another-service.priv:3000", "another-service"),
            ("http://sub.domain.priv", "sub.domain"),
        ];
        for (input, expected) in private_tests {
            let result = Docker::capture_service(input, &PRIVATE_TLD_REGEX);
            assert_eq!(
                result,
                Some(expected.to_string()),
                "Failed to capture private service from: {}",
                input
            );
        }

        // Test non-matching strings return None.
        let non_matching = vec![
            "something.pubx",
            "something.privx",
            "http://",
            "http://something.pubx",
            "http://something.privx",
            "no-tld-here",
            "service.unknown:1234",
            "127.0.0.1",
            "http://127.0.0.1",
        ];
        for input in non_matching {
            let public_result = Docker::capture_service(input, &PUBLIC_TLD_REGEX);
            let private_result = Docker::capture_service(input, &PRIVATE_TLD_REGEX);
            assert!(
                public_result.is_none() && private_result.is_none(),
                "Expected no capture from: {}",
                input
            );
        }
    }
}
