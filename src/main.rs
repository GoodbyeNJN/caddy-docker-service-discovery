use std::{panic, sync::Arc, thread};

use env_logger::Builder;
use hickory_server::ServerFuture;
use log::{error, info, LevelFilter};
use tokio::{
    net::UdpSocket,
    signal::unix::{signal, SignalKind},
    sync::Mutex,
};

use api::{collect_registry_services, dispatch_registry_services, start_api_server};
use dns::Dns;
use docker::Docker;
use env::Env;

mod api;
mod constants;
mod dns;
mod docker;
mod env;
mod registry;

#[tokio::main]
async fn main() {
    Builder::new()
        .filter_level(LevelFilter::Info)
        .parse_env("LOG_LEVEL")
        .format_target(false)
        .format_timestamp_secs()
        .format_indent(Some(29))
        .init();

    panic::set_hook(Box::new(|info| {
        let msg1 = info.payload().downcast_ref::<&str>().copied();
        let msg2 = info.payload().downcast_ref::<String>().map(String::as_str);
        if let Some(msg) = msg1.or(msg2) {
            error!("{}", msg);
        }

        error!(
            "An unexpected error occurred.\nAt: thread: `{}`, location: `{}`",
            thread::current().name().unwrap_or("unknown"),
            if let Some(loc) = info.location() {
                loc.to_string()
            } else {
                "unknown".to_string()
            }
        );
    }));

    Env::validate().unwrap_or_else(|err| {
        panic!("{}", err);
    });

    let self_registry = Arc::new(Mutex::new(Env::self_registry()));
    let registries = Arc::new(Mutex::new(Env::registries()));

    let docker_job = {
        let self_registry = self_registry.clone();
        let registries = registries.clone();

        tokio::spawn(async move {
            let docker = Docker::new().unwrap_or_else(|err| {
                panic!("{}", err);
            });

            docker.flush_registry_services(self_registry.clone()).await;
            collect_registry_services(registries.clone()).await;
            dispatch_registry_services(self_registry.clone(), registries.clone()).await;

            docker
                .watch_events(|| async {
                    docker.flush_registry_services(self_registry.clone()).await;
                    dispatch_registry_services(self_registry.clone(), registries.clone()).await;
                })
                .await;
        })
    };

    let dns_job = {
        let self_registry = self_registry.clone();
        let registries = registries.clone();

        tokio::spawn(async move {
            let mut dns_server =
                ServerFuture::new(Dns::new(self_registry.clone(), registries.clone()));

            let addr = Env::server_listen();
            let socket = UdpSocket::bind(addr).await.unwrap_or_else(|err| {
                panic!("DNS server failed to listen on `{}`.\nError: {}", addr, err);
            });
            dns_server.register_socket(socket);

            info!("DNS server listening on: {}", addr);
            let _ = dns_server.block_until_done().await;
        })
    };

    let api_job = {
        let self_registry = self_registry.clone();
        let registries = registries.clone();

        tokio::spawn(async move {
            start_api_server(
                Env::registry_listen(),
                self_registry.clone(),
                registries.clone(),
            )
            .await
            .unwrap_or_else(|err| {
                panic!("{}", err);
            })
            .await
            .unwrap_or_else(|err| {
                panic!("Failed to start API server.\nError: {}", err);
            });
        })
    };

    let mut term_signal = signal(SignalKind::terminate()).unwrap();
    tokio::select! {
        _ = term_signal.recv() => {
            info!("SIGTERM received, shutting down...");
        },
        _ = docker_job => {
            info!("Docker client finished or encountered error.");
        },
        _ = dns_job => {
            info!("Server finished or encountered error.");
        },
        _ = api_job => {
            info!("Docker client finished or encountered error.");
        },
    };
}
