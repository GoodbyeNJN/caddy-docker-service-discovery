use std::{collections::HashSet, net::SocketAddr, sync::Arc};

use actix_web::{
    dev::Server,
    get,
    middleware::Logger,
    put,
    web::{Data, Path},
    App, HttpResponse, HttpServer, Responder,
};
use anyhow::{Context, Result};
use log::{error, info};
use reqwest::Url;
use serde_json::{from_str, to_string};
use tokio::sync::Mutex;

use crate::registry::Registry;

struct State {
    pub self_registry: Arc<Mutex<Registry>>,
    pub registries: Arc<Mutex<Vec<Registry>>>,
}

#[get("/health")]
async fn health() -> impl Responder {
    HttpResponse::Ok().body("OK")
}

#[get("/api/self/services")]
async fn get_self_services(data: Data<State>) -> impl Responder {
    let self_registry = &*data.self_registry.lock().await;

    HttpResponse::Ok().json(self_registry.public_services())
}

#[get("/api/{registry_hostname}/services")]
async fn get_registry_services(path: Path<String>, data: Data<State>) -> impl Responder {
    let registries = data.registries.lock().await;
    let registry_hostname = path.into_inner();

    let registry = registries
        .iter()
        .find(|registry| registry.hostname().to_string() == registry_hostname);
    if let Some(registry) = registry {
        HttpResponse::Ok().json(registry.public_services())
    } else {
        HttpResponse::Ok().body("null")
    }
}

#[put("/api/{registry_hostname}/services")]
async fn put_registry_services(
    path: Path<String>,
    services: String,
    data: Data<State>,
) -> impl Responder {
    let mut registries = data.registries.lock().await;
    let registry_hostname = path.into_inner();

    let registry = registries
        .iter_mut()
        .find(|registry| registry.hostname().to_string() == registry_hostname);
    if let Some(registry) = registry {
        match from_str::<HashSet<String>>(&services) {
            Ok(services) => {
                registry.flush_public_services(services);

                HttpResponse::Ok().body("Success")
            }
            Err(_) => HttpResponse::Ok().body("Invalid services"),
        }
    } else {
        match registry_hostname.parse() {
            Ok(registry) => {
                registries.push(registry);
                HttpResponse::Ok().body("Success")
            }
            Err(_) => HttpResponse::Ok().body("Invalid registry"),
        }
    }
}

pub async fn start_api_server(
    addr: SocketAddr,
    self_registry: Arc<Mutex<Registry>>,
    registries: Arc<Mutex<Vec<Registry>>>,
) -> Result<Server> {
    let data = Data::new(State {
        self_registry,
        registries,
    });

    let server = HttpServer::new(move || {
        App::new()
            .app_data(data.clone())
            .wrap(Logger::default())
            .service(health)
            .service(get_self_services)
            .service(get_registry_services)
            .service(put_registry_services)
    })
    .bind(addr)
    .context(format!("Failed to bind API server to `{}`.", addr))?
    .run();

    Ok(server)
}

async fn get(url: Url) -> Result<String> {
    reqwest::get(url.clone())
        .await
        .context(format!("Failed to get `{}`.", url))?
        .text()
        .await
        .context(format!("Failed to read response from `{}`.", url))
}

async fn put(url: Url, body: String) -> Result<String> {
    reqwest::Client::new()
        .put(url.clone())
        .body(body)
        .send()
        .await
        .context(format!("Failed to put `{}`.", url))?
        .text()
        .await
        .context(format!("Failed to read response from `{}`.", url))
}

pub async fn collect_registry_services(registries: Arc<Mutex<Vec<Registry>>>) {
    let mut registries = registries.lock().await;

    for registry in registries.iter_mut() {
        let mut url = registry.url().clone();
        url.set_path("/api/self/services");

        info!("Collecting public services from `{}`.", registry.hostname());
        match get(url).await {
            Ok(response) => match from_str::<HashSet<String>>(&response) {
                Ok(services) => {
                    registry.flush_public_services(services);
                    info!(
                        "Collected public services from `{}`: {:?}.",
                        registry.hostname(),
                        registry.public_services()
                    );
                }
                Err(_) => {
                    error!(
                        "Failed to parse public services from `{}`.\nResponse: {}",
                        registry.hostname(),
                        response
                    );
                }
            },

            Err(err) => {
                error!(
                    "Failed to fetch public services from `{}`.\nError: {}",
                    registry.hostname(),
                    err
                );
            }
        }
    }
}

pub async fn dispatch_registry_services(
    self_registry: Arc<Mutex<Registry>>,
    registries: Arc<Mutex<Vec<Registry>>>,
) {
    let self_registry = self_registry.lock().await;
    let registries = &*registries.lock().await;

    for registry in registries {
        let mut url = registry.url().clone();
        url.set_path(&format!("/api/{}/services", self_registry.hostname()));

        info!("Dispatching public services to `{}`.", registry.hostname());
        match put(url, to_string(&self_registry.public_services()).unwrap()).await {
            Ok(_) => {
                info!(
                    "Dispatched public services to `{}`: {:?}.",
                    registry.hostname(),
                    self_registry.public_services()
                );
            }
            Err(err) => {
                error!(
                    "Failed to dispatch public services to `{}`.\nError: {}",
                    registry.hostname(),
                    err
                );
            }
        }
    }
}
