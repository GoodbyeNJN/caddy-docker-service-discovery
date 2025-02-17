use std::{net::IpAddr, sync::Arc};

use async_trait::async_trait;
use dns_lookup::lookup_host;
use hickory_server::{
    authority::MessageResponseBuilder,
    proto::{
        op::{Header, ResponseCode},
        rr::{rdata::A, RData, Record, RecordData},
    },
    server::{Request, RequestHandler, ResponseHandler, ResponseInfo},
};
use log::{debug, error, info};
use tokio::sync::Mutex;

use crate::{
    constants::{PRIVATE_SERVICE_TLD, PUBLIC_SERVICE_TLD},
    registry::Registry,
};

pub struct Dns {
    self_registry: Arc<Mutex<Registry>>,
    registries: Arc<Mutex<Vec<Registry>>>,
}

impl Dns {
    pub fn new(self_registry: Arc<Mutex<Registry>>, registries: Arc<Mutex<Vec<Registry>>>) -> Self {
        Self {
            self_registry,
            registries,
        }
    }

    pub fn query_upstream(name: &str) -> Option<RData> {
        lookup_host(name)
            .ok()?
            .into_iter()
            .find_map(|addr| match addr {
                IpAddr::V4(ip) => Some(ip),
                _ => None,
            })
            .map(|ip| A(ip).into_rdata())
    }

    async fn query_self_registry(&self, service: &str) -> Option<RData> {
        let self_registry = (&*self.self_registry.lock().await).clone();

        if self_registry.has_public_service(service) || self_registry.has_private_service(service) {
            if self_registry.has_public_service(service) {
                debug!("Found public service `{}` in self registry", service);
            } else {
                debug!("Found private service `{}` in self registry", service);
            }

            self_registry
                .try_into()
                .map_err(|err| error!("{}", err))
                .ok()
        } else {
            debug!("Service `{}` not found in self registry", service);

            None
        }
    }

    async fn query_registries(&self, service: &str) -> Option<RData> {
        let registries = (&*self.registries.lock().await).clone();

        for registry in registries.iter() {
            if registry.has_public_service(service) {
                debug!(
                    "Found public service `{}` in registry `{}`",
                    service,
                    registry.hostname()
                );

                return registry
                    .clone()
                    .try_into()
                    .map_err(|err| error!("{}", err))
                    .ok();
            }
        }

        debug!("Service `{}` not found in any registry", service);

        None
    }
}

#[async_trait]
impl RequestHandler for Dns {
    async fn handle_request<R: ResponseHandler>(
        &self,
        request: &Request,
        mut response_handle: R,
    ) -> ResponseInfo {
        let name = request.query().name();
        debug!("Received DNS query for name: `{}`", name);
        let service = name
            .to_string()
            .trim_end_matches('.')
            .trim_end_matches(&format!(".{}", PUBLIC_SERVICE_TLD))
            .trim_end_matches(&format!(".{}", PRIVATE_SERVICE_TLD))
            .to_string();
        debug!("Extracted service name: `{}`", service);

        let mut header = Header::response_from_request(request.header());
        header.set_authoritative(true);
        header.set_recursion_available(true);

        let builder = MessageResponseBuilder::from_message_request(request);

        let data = self
            .query_self_registry(&service)
            .await
            .or(self.query_registries(&service).await)
            .or(Self::query_upstream(&name.to_string()));
        let result = match data {
            Some(data) => {
                info!("Responding with A record for `{}`: `{}`", name, data);

                let records = vec![Record::from_rdata(name.into(), 0, data)];
                let response = builder.build(header, records.iter(), &[], &[], &[]);

                response_handle.send_response(response).await
            }

            None => {
                info!("No A record found for `{}`", name);

                header.set_response_code(ResponseCode::NXDomain);
                let response = builder.build_no_records(header);

                response_handle.send_response(response).await
            }
        };

        result.unwrap_or_else(|err| {
            error!("Failed to send response: {}", err);

            let mut header = Header::new();
            header.set_response_code(ResponseCode::ServFail);
            header.into()
        })
    }
}
