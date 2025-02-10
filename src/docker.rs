use crate::memory_kv::MemoryKV;
use bollard::container::ListContainersOptions;
use bollard::system::EventsOptions;
use bollard::Docker;
use futures_util::stream::StreamExt;
use std::collections::HashMap;
use std::env::var;
use std::error::Error;
use std::net::Ipv4Addr;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct DockerClient {
    socket: Docker,
    kv: Arc<Mutex<MemoryKV>>,
}

impl DockerClient {
    pub fn new(kv: Arc<Mutex<MemoryKV>>) -> Self {
        let socket = Docker::connect_with_unix_defaults().unwrap_or_else(|err| {
            panic!(
            "Failed to connect to Docker socket at `/var/run/docker.sock`.\nPlease ensure the Docker daemon is running and you have proper permissions.\nError: {}",
            err
            )
        });
        Self { socket, kv }
    }

    fn get_host_ip() -> Ipv4Addr {
        var("DNS_HOST_IP")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| {
                panic!("Failed to parse host IP address from environment variable `DNS_HOST_IP`.")
            })
    }

    pub async fn flush_known_hosts(&self) -> Result<(), Box<dyn Error>> {
        let mut kv = self.kv.lock().await;
        kv.clear();

        let host_ip = DockerClient::get_host_ip();

        let containers = self
            .socket
            .list_containers(Some(ListContainersOptions {
                all: true,
                filters: HashMap::from_iter(vec![("status", vec!["running"])]),
                ..Default::default()
            }))
            .await
            .unwrap_or_else(|err| panic!("Failed to list running containers.\nError: {}", err));

        for container in containers {
            let labels = match container.labels {
                Some(labels) => labels,
                None => continue,
            };

            if let Some(hostname) = labels.get("dns.hostname") {
                kv.set(hostname.clone(), host_ip);
            }
        }

        Ok(())
    }

    pub async fn watch_events(&self) {
        let mut events = self.socket.events(Some(EventsOptions {
            filters: HashMap::from_iter(vec![("type", vec!["container"])]),
            ..Default::default()
        }));

        while let Some(event) = events.next().await {
            match event {
                Ok(_) => {
                    let _ = self.flush_known_hosts().await;
                }
                Err(err) => {
                    eprintln!("Error watching Docker events: {}", err)
                }
            }
        }
    }
}
