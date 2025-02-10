use crate::memory_kv::MemoryKV;
use async_trait::async_trait;
use hickory_server::authority::MessageResponseBuilder;
use hickory_server::proto::op::{Header, ResponseCode};
use hickory_server::proto::rr::{rdata, RData, Record};
use hickory_server::server::{Request, RequestHandler, ResponseHandler, ResponseInfo};
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct Handler {
    kv: Arc<Mutex<MemoryKV>>,
}

impl Handler {
    pub fn new(kv: Arc<Mutex<MemoryKV>>) -> Self {
        Self { kv }
    }

    fn create_header(&self, request: &Request) -> Header {
        let mut header = Header::response_from_request(request.header());
        header.set_authoritative(true);
        header.set_recursion_available(true);

        header
    }

    async fn get_matched_records(&self, request: &Request) -> Option<Vec<Record>> {
        let name = request.query().name();

        self.kv
            .lock()
            .await
            .get(&name.to_string())
            .cloned()
            .map(|addr| {
                let record = Record::from_rdata(name.into(), 60, RData::A(rdata::A(addr)));
                vec![record]
            })
    }
}

#[async_trait]
impl RequestHandler for Handler {
    async fn handle_request<R: ResponseHandler>(
        &self,
        request: &Request,
        mut response_handle: R,
    ) -> ResponseInfo {
        let builder = MessageResponseBuilder::from_message_request(request);

        let records = self.get_matched_records(request).await;
        let result = if let Some(records) = records {
            let header = self.create_header(request);
            let answers = records.iter();
            let response = builder.build(header, answers, &[], &[], &[]);

            response_handle.send_response(response).await
        } else {
            let header = {
                let mut header = self.create_header(request);
                header.set_response_code(ResponseCode::NXDomain);
                header
            };
            let response = builder.build_no_records(header);

            response_handle.send_response(response).await
        };

        match result {
            Ok(res) => res,
            Err(err) => {
                eprintln!("Failed to send response: {:?}", err);

                let mut header = Header::new();
                header.set_response_code(ResponseCode::ServFail);
                header.into()
            }
        }
    }
}
