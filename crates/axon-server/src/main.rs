//! Axon server binary — starts HTTP gateway, gRPC service, or MCP stdio.

use std::net::SocketAddr;
use std::sync::Arc;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use axon_api::handler::AxonHandler;
use axon_server::service::{AxonServiceImpl, AxonServiceServer};
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

    /// Disable authentication — all requests succeed as admin with actor="anonymous".
    /// Intended for local development only.
    #[arg(long, env = "AXON_NO_AUTH", default_value = "true")]
    no_auth: bool,

    /// Run MCP server over stdin/stdout instead of HTTP/gRPC.
    /// No authentication is applied for stdio connections.
    #[arg(long)]
    mcp_stdio: bool,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    // For MCP stdio mode, minimize logging to stderr so stdout is clean.
    if args.mcp_stdio {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .with_writer(std::io::stderr)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .init();
    }

    if args.mcp_stdio {
        tracing::info!("starting MCP stdio server (no auth)");
        let handler = Arc::new(std::sync::Mutex::new(AxonHandler::new(
            MemoryStorageAdapter::default(),
        )));

        // In stdio mode, collections are discovered dynamically.
        // Start with an empty list — agents use tools/list after initialization.
        if let Err(e) = axon_server::run_mcp_stdio(handler, &[]) {
            tracing::error!("MCP stdio error: {e}");
            std::process::exit(1);
        }
        return;
    }

    if args.no_auth {
        tracing::info!(
            "running in --no-auth mode: all requests succeed as admin (actor=anonymous)"
        );
    }

    // Single shared handler for both HTTP and gRPC.
    let handler = Arc::new(tokio::sync::Mutex::new(AxonHandler::new(
        MemoryStorageAdapter::default(),
    )));

    // Build HTTP gateway.
    let http_app = axon_server::gateway::build_router(handler.clone());
    let http_addr: SocketAddr = ([0, 0, 0, 0], args.http_port).into();

    // Build gRPC service sharing the same handler.
    let grpc_svc = AxonServiceImpl::from_shared(handler);
    let grpc_addr: SocketAddr = ([0, 0, 0, 0], args.grpc_port).into();

    tracing::info!("HTTP gateway listening on {http_addr}");
    tracing::info!("gRPC service listening on {grpc_addr}");

    // Start HTTP server with graceful shutdown.
    let http_handle = tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(http_addr).await.unwrap();
        axum::serve(listener, http_app)
            .with_graceful_shutdown(shutdown_signal())
            .await
            .unwrap();
    });

    // Start gRPC server with graceful shutdown.
    let grpc_handle = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(AxonServiceServer::new(grpc_svc))
            .serve_with_shutdown(grpc_addr, shutdown_signal())
            .await
            .unwrap();
    });

    // Wait for either server to finish (both shut down on CTRL+C).
    tokio::select! {
        r = http_handle => { if let Err(e) = r { tracing::error!("HTTP server error: {e}"); } }
        r = grpc_handle => { if let Err(e) = r { tracing::error!("gRPC server error: {e}"); } }
    }
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C handler");
    tracing::info!("shutdown signal received, stopping server");
}
