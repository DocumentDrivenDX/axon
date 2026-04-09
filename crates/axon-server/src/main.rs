//! Axon server binary — starts HTTP gateway, gRPC service, or MCP stdio.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use axon_api::handler::AxonHandler;
use axon_server::service::{AxonServiceImpl, AxonServiceServer};
use axon_server::{AuthContext, Role};
use axon_storage::{MemoryStorageAdapter, PostgresStorageAdapter, SqliteStorageAdapter};

#[derive(Clone, Debug, clap::ValueEnum)]
enum StorageBackend {
    Memory,
    Sqlite,
    Postgres,
}

#[derive(Clone, Debug, clap::ValueEnum)]
enum DefaultRoleArg {
    Admin,
    Write,
    Read,
}

impl From<DefaultRoleArg> for Role {
    fn from(value: DefaultRoleArg) -> Self {
        match value {
            DefaultRoleArg::Admin => Role::Admin,
            DefaultRoleArg::Write => Role::Write,
            DefaultRoleArg::Read => Role::Read,
        }
    }
}

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

    /// Path to the local tailscaled socket for LocalAPI whois lookups.
    #[arg(
        long,
        env = "AXON_TAILSCALE_SOCKET",
        default_value = "/run/tailscale/tailscaled.sock"
    )]
    tailscale_socket: PathBuf,

    /// Default role assigned to Tailscale nodes without a recognized ACL tag.
    #[arg(
        long,
        env = "AXON_TAILSCALE_DEFAULT_ROLE",
        value_enum,
        default_value = "read"
    )]
    tailscale_default_role: DefaultRoleArg,

    /// TTL in seconds for cached Tailscale whois identity lookups.
    #[arg(long, env = "AXON_AUTH_CACHE_TTL_SECS", default_value = "60")]
    auth_cache_ttl_secs: u64,

    /// Run MCP server over stdin/stdout instead of HTTP/gRPC.
    /// No authentication is applied for stdio connections.
    #[arg(long)]
    mcp_stdio: bool,

    /// Backing storage adapter.
    #[arg(long, env = "AXON_STORAGE", value_enum, default_value = "sqlite")]
    storage: StorageBackend,

    /// SQLite database path when `--storage=sqlite`.
    #[arg(long, env = "AXON_SQLITE_PATH", default_value = "axon-server.db")]
    sqlite_path: String,

    /// PostgreSQL DSN when `--storage=postgres`.
    #[arg(long, env = "AXON_POSTGRES_DSN")]
    postgres_dsn: Option<String>,

    /// Serve built admin UI assets from this directory under the `/ui` path prefix.
    #[arg(long, env = "AXON_UI_DIR")]
    ui_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    if let Err(error) = run(args).await {
        tracing::error!("{error}");
        std::process::exit(1);
    }
}

async fn run(args: Args) -> Result<(), String> {
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

    if args.no_auth {
        tracing::info!(
            "running in --no-auth mode: all requests succeed as admin (actor=anonymous)"
        );
    }

    match args.storage {
        StorageBackend::Memory => {
            run_with_storage(MemoryStorageAdapter::default(), &args, "memory").await
        }
        StorageBackend::Sqlite => {
            let storage = SqliteStorageAdapter::open(&args.sqlite_path)
                .map_err(|error| format!("failed to open SQLite backing store: {error}"))?;
            run_with_storage(storage, &args, format!("sqlite:{}", args.sqlite_path)).await
        }
        StorageBackend::Postgres => {
            let dsn = args
                .postgres_dsn
                .as_deref()
                .ok_or_else(|| "--postgres-dsn is required when --storage=postgres".to_string())?;
            let storage = PostgresStorageAdapter::connect(dsn)
                .map_err(|error| format!("failed to connect PostgreSQL backing store: {error}"))?;
            run_with_storage(storage, &args, "postgres").await
        }
    }
}

async fn run_with_storage<S>(
    storage: S,
    args: &Args,
    backend: impl Into<String>,
) -> Result<(), String>
where
    S: axon_storage::adapter::StorageAdapter + 'static,
{
    let backend = backend.into();

    if args.mcp_stdio {
        tracing::info!("starting MCP stdio server with backend {backend}");
        let handler = Arc::new(std::sync::Mutex::new(AxonHandler::new(storage)));
        return axon_server::run_mcp_stdio(handler, &[]).map_err(|error| error.to_string());
    }

    let auth = if args.no_auth {
        AuthContext::no_auth()
    } else {
        AuthContext::tailscale(
            args.tailscale_default_role.clone().into(),
            args.tailscale_socket.clone(),
            Duration::from_secs(args.auth_cache_ttl_secs),
        )
    };

    auth.verify().await.map_err(|error| {
        format!(
            "failed to initialize auth via {}: {error}",
            args.tailscale_socket.display()
        )
    })?;

    let handler = Arc::new(tokio::sync::Mutex::new(AxonHandler::new(storage)));
    let http_app = axon_server::gateway::build_router_with_auth(
        handler.clone(),
        backend.clone(),
        args.ui_dir.clone(),
        auth.clone(),
    );
    let http_addr: SocketAddr = ([0, 0, 0, 0], args.http_port).into();

    let grpc_svc = AxonServiceImpl::from_shared_with_auth(handler, auth);
    let grpc_addr: SocketAddr = ([0, 0, 0, 0], args.grpc_port).into();

    tracing::info!("HTTP gateway listening on {http_addr}");
    tracing::info!("gRPC service listening on {grpc_addr}");

    let http_handle = tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(http_addr)
            .await
            .map_err(|error| format!("failed to bind HTTP listener: {error}"))?;
        axum::serve(
            listener,
            http_app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|error| format!("HTTP server error: {error}"))
    });

    let grpc_handle = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(AxonServiceServer::new(grpc_svc))
            .serve_with_shutdown(grpc_addr, shutdown_signal())
            .await
            .map_err(|error| format!("gRPC server error: {error}"))
    });

    tokio::select! {
        result = http_handle => {
            result.map_err(|error| format!("HTTP task join error: {error}"))??;
        }
        result = grpc_handle => {
            result.map_err(|error| format!("gRPC task join error: {error}"))??;
        }
    }
    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C handler");
    tracing::info!("shutdown signal received, stopping server");
}
