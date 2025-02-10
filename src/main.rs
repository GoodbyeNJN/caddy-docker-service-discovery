use docker::DockerClient;
use handler::Handler;
use hickory_server::server::ServerFuture;
use memory_kv::MemoryKV;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::Mutex;

mod docker;
mod handler;
mod memory_kv;

#[tokio::main]
async fn main() {
    let kv = Arc::new(Mutex::new(MemoryKV::new()));

    let docker = DockerClient::new(kv.clone());
    let _ = docker.flush_known_hosts().await;

    let handler = Handler::new(kv.clone());
    let mut server = ServerFuture::new(handler);

    let addr = "0.0.0.0:53".parse::<SocketAddr>().unwrap();
    let socket = UdpSocket::bind(addr).await.unwrap();
    server.register_socket(socket);

    let server_task = tokio::spawn(async move {
        println!("Listening on: {}", addr);
        let _ = server.block_until_done().await;
    });

    let docker_task = tokio::spawn(async move {
        docker.watch_events().await;
    });

    let mut term_signal = signal(SignalKind::terminate()).unwrap();
    tokio::select! {
        _ = term_signal.recv() => {
            println!("SIGTERM received, shutting down...");
        },
        _ = server_task => {
            println!("Server finished or encountered error.");
        },
        _ = docker_task => {
            println!("Docker client finished or encountered error.");
        },
    }
}
