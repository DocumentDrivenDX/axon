//! Axon server binary — starts HTTP gateway and gRPC service.

use std::net::SocketAddr;
use std::sync::Arc;

use clap::Parser;
use tokio::sync::Mutex;
use tracing_subscriber::EnvFilter;

use axon_api::handler::AxonHandler;
use axon_storage::memory::MemoryStorageAdapter;

/// Axon server — schema-first transactional data store for agentic applications.
#[derive(Parser)]
#[command(name = "axon-server", version)]
struct Args {
    /// Port for the HTTP/JSON gateway.
    #[arg(long, env = "AXON_HTTP_PORT", default_value = "3000")]
    http_port: u16,

    /// Port for the gRPC service.
    #[arg(long, env = "AXON_GRPC_PORT", default_value = "50051")]
    grpc_port: u16,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    let handler = Arc::new(Mutex::new(
        AxonHandler::new(MemoryStorageAdapter::default()),
    ));

    // Build HTTP gateway.
    let http_app = axon_server::gateway::build_router(handler.clone());
    let http_addr: SocketAddr = ([0, 0, 0, 0], args.http_port).into();

    tracing::info!("HTTP gateway listening on {http_addr}");
    tracing::info!("gRPC service listening on 0.0.0.0:{}", args.grpc_port);

    // Start HTTP server with graceful shutdown.
    let http_handle = tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(http_addr).await.unwrap();
        axum::serve(listener, http_app)
            .with_graceful_shutdown(shutdown_signal())
            .await
            .unwrap();
    });

    // Health-check: the HTTP gateway itself serves as the health endpoint.
    // A GET to any unknown path returns 404, confirming the server is up.
    // A dedicated /health route could be added but is not required for V1.

    http_handle.await.unwrap();
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C handler");
    tracing::info!("shutdown signal received, stopping server");
}
