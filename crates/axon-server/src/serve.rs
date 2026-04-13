//! Server startup logic — starts HTTP gateway, gRPC service, or MCP stdio.
//!
//! Extracted from the former `main.rs` binary so that `axon-cli` (or any
//! other binary) can invoke [`serve`] as a library call.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use crate::service::{AxonServiceImpl, AxonServiceServer};
use crate::{AuthContext, Role};
use axon_api::handler::AxonHandler;
use axon_storage::{provision_postgres_database, PostgresStorageAdapter, SqliteStorageAdapter};

#[derive(Clone, Debug, clap::ValueEnum)]
pub enum StorageBackend {
    Memory,
    Sqlite,
    Postgres,
}

/// CLI-compatible role selection for `--tailscale-default-role` and `--guest-role`.
///
/// Converted to [`Role`] via `From<DefaultRoleArg>`.
#[derive(Clone, Debug, clap::ValueEnum)]
pub enum DefaultRoleArg {
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
#[command(name = "axon-serve", version)]
pub struct ServeArgs {
    /// Port for the HTTP/JSON gateway.
    #[arg(long, env = "AXON_HTTP_PORT", default_value = "4170")]
    pub http_port: u16,

    /// Port for the gRPC service. When omitted, gRPC is not started.
    #[arg(long, env = "AXON_GRPC_PORT")]
    pub grpc_port: Option<u16>,

    /// Disable authentication — all requests succeed as admin with actor="anonymous".
    /// Intended for local development only.
    #[arg(
        long,
        env = "AXON_NO_AUTH",
        num_args = 0..=1,
        default_missing_value = "true",
        default_value = "false",
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    pub no_auth: bool,

    /// Path to the local tailscaled socket for LocalAPI whois lookups.
    #[arg(
        long,
        env = "AXON_TAILSCALE_SOCKET",
        default_value = "/run/tailscale/tailscaled.sock"
    )]
    pub tailscale_socket: PathBuf,

    /// Default role assigned to Tailscale nodes without a recognized ACL tag.
    #[arg(
        long,
        env = "AXON_TAILSCALE_DEFAULT_ROLE",
        value_enum,
        default_value = "read"
    )]
    pub tailscale_default_role: DefaultRoleArg,

    /// Enable guest mode: unauthenticated requests get the specified role
    /// instead of being rejected. This is opt-in; the default role is `read`.
    /// When set, Tailscale auth is not required. Mutually exclusive with `--no-auth`.
    #[arg(long, env = "AXON_GUEST_ROLE", value_enum)]
    pub guest_role: Option<DefaultRoleArg>,

    /// TTL in seconds for cached Tailscale whois identity lookups.
    #[arg(long, env = "AXON_AUTH_CACHE_TTL_SECS", default_value = "60")]
    pub auth_cache_ttl_secs: u64,

    /// Run MCP server over stdin/stdout instead of HTTP/gRPC.
    /// No authentication is applied for stdio connections.
    #[arg(long)]
    pub mcp_stdio: bool,

    /// Backing storage adapter.
    #[arg(long, env = "AXON_STORAGE", value_enum, default_value = "sqlite")]
    pub storage: StorageBackend,

    /// SQLite database path when `--storage=sqlite`.
    #[arg(long, env = "AXON_SQLITE_PATH", default_value = "axon-server.db")]
    pub sqlite_path: String,

    /// PostgreSQL DSN when `--storage=postgres`.
    #[arg(long, env = "AXON_POSTGRES_DSN")]
    pub postgres_dsn: Option<String>,

    /// SQLite database path for the control-plane (tenant provisioning).
    #[arg(
        long,
        env = "AXON_CONTROL_PLANE_PATH",
        default_value = "axon-control-plane.db"
    )]
    pub control_plane_path: String,

    /// Serve built admin UI assets from this directory under the `/ui` path prefix.
    #[arg(long, env = "AXON_UI_DIR")]
    pub ui_dir: Option<PathBuf>,
}

/// Initialise the `tracing` subscriber.
///
/// When `mcp_stdio` is true, log output is directed to stderr so that stdout
/// remains clean for the MCP JSON-RPC protocol.
pub fn init_tracing(mcp_stdio: bool) {
    let subscriber = tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env());
    let result = if mcp_stdio {
        subscriber.with_writer(std::io::stderr).try_init()
    } else {
        subscriber.try_init()
    };
    let _ = result;
}

/// Build an [`AuthContext`] from the parsed CLI / env flags.
///
/// Priority: `--no-auth` > `--guest-role` > Tailscale (default).
///
/// | Condition | Mode | Notes |
/// |-----------|------|-------|
/// | `--no-auth` | `NoAuth` | All requests get `actor=anonymous, role=Admin`. |
/// | `--guest-role <role>` | `Guest` | Tailscale daemon not required. |
/// | *(default)* | `Tailscale` | Contacts `--tailscale-socket` on every cache miss. |
///
/// After construction, call [`AuthContext::verify`] (already done in [`serve`])
/// to fail fast if the daemon is unreachable.
pub fn auth_context_from_serve_args(args: &ServeArgs) -> AuthContext {
    if args.no_auth {
        AuthContext::no_auth()
    } else if let Some(ref guest_role) = args.guest_role {
        AuthContext::guest(guest_role.clone().into())
    } else {
        AuthContext::tailscale(
            args.tailscale_default_role.clone().into(),
            args.tailscale_socket.clone(),
            Duration::from_secs(args.auth_cache_ttl_secs),
        )
    }
}

/// Entry point that replaces the former binary `main`.
///
/// Selects the storage backend based on `args.storage` and delegates to
/// [`run_with_sqlite_storage`].  The HTTP gateway always uses SQLite via
/// [`TenantRouter`] for per-tenant isolation.  The `--storage=memory`
/// backend uses an in-memory SQLite database.
pub async fn serve(args: ServeArgs) -> Result<(), String> {
    init_tracing(args.mcp_stdio);

    if args.no_auth {
        tracing::info!(
            "running in --no-auth mode: all requests succeed as admin (actor=anonymous)"
        );
    } else if let Some(ref guest_role) = args.guest_role {
        tracing::info!(
            "running in guest mode: unauthenticated requests get role={guest_role:?} (actor=guest)"
        );
    }

    match args.storage {
        StorageBackend::Memory => {
            let storage = SqliteStorageAdapter::open_in_memory()
                .map_err(|e| format!("failed to open in-memory SQLite: {e}"))?;
            run_with_sqlite_storage(storage, &args, "memory").await
        }
        StorageBackend::Sqlite => {
            let storage = SqliteStorageAdapter::open(&args.sqlite_path)
                .map_err(|error| format!("failed to open SQLite backing store: {error}"))?;
            run_with_sqlite_storage(storage, &args, format!("sqlite:{}", args.sqlite_path)).await
        }
        StorageBackend::Postgres => {
            let superadmin_dsn = args
                .postgres_dsn
                .as_deref()
                .ok_or_else(|| "--postgres-dsn is required when --storage=postgres".to_string())?;
            run_with_postgres_storage(superadmin_dsn, &args).await
        }
    }
}

/// Run the server with a SQLite storage adapter.
///
/// This is the primary server startup path.  The HTTP gateway always uses
/// [`TenantRouter`] for per-tenant handler isolation.
pub async fn run_with_sqlite_storage(
    storage: SqliteStorageAdapter,
    args: &ServeArgs,
    backend: impl Into<String>,
) -> Result<(), String> {
    let backend = backend.into();

    if args.mcp_stdio {
        tracing::info!("starting MCP stdio server with backend {backend}");
        let handler = Arc::new(std::sync::Mutex::new(AxonHandler::new(storage)));
        return crate::run_mcp_stdio(handler, &[]).map_err(|error| error.to_string());
    }

    let auth = auth_context_from_serve_args(args);

    auth.verify().await.map_err(|error| {
        format!(
            "failed to initialize auth via {}: {error}",
            args.tailscale_socket.display()
        )
    })?;

    let control_plane_db = crate::control_plane::ControlPlaneDb::open(&args.control_plane_path)
        .map_err(|error| format!("failed to open control-plane database: {error}"))?;
    let control_plane_db = Arc::new(tokio::sync::Mutex::new(control_plane_db));
    tracing::info!(
        "control-plane database opened at {}",
        args.control_plane_path
    );

    // Derive the data directory for tenant databases from the control-plane path.
    let data_dir = std::path::Path::new(&args.control_plane_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();
    let control_plane_state =
        crate::control_plane_routes::ControlPlaneState::new(control_plane_db, data_dir.clone());

    let handler: crate::tenant_router::TenantHandler =
        Arc::new(tokio::sync::Mutex::new(AxonHandler::new(
            Box::new(storage) as Box<dyn axon_storage::adapter::StorageAdapter + Send + Sync>,
        )));
    let tenant_router = Arc::new(crate::tenant_router::TenantRouter::new(
        data_dir,
        handler.clone(),
    ));
    let http_app = crate::gateway::build_router_with_auth(
        tenant_router,
        backend.clone(),
        args.ui_dir.clone(),
        auth.clone(),
        crate::rate_limit::RateLimitConfig::default(),
        crate::actor_scope::ActorScopeGuard::default(),
        Some(control_plane_state),
    );
    let http_addr: SocketAddr = ([0, 0, 0, 0], args.http_port).into();

    tracing::info!("HTTP gateway listening on {http_addr}");

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

    if let Some(port) = args.grpc_port {
        let grpc_svc = AxonServiceImpl::from_shared_with_auth(handler, auth);
        let grpc_addr: SocketAddr = ([0, 0, 0, 0], port).into();
        tracing::info!("gRPC service listening on {grpc_addr}");

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
    } else {
        // gRPC disabled — only run the HTTP server.
        http_handle
            .await
            .map_err(|error| format!("HTTP task join error: {error}"))??;
    }

    Ok(())
}

/// Run the server with a PostgreSQL storage backend.
///
/// On startup this function:
/// 1. Creates `axon_master` (the master database) using the superadmin DSN if it
///    does not already exist, then opens an `AxonHandler<PostgresStorageAdapter>`
///    against it.
/// 2. Constructs a [`crate::tenant_router::TenantRouter`] in Postgres mode so
///    that subsequent tenant provisioning calls issue `CREATE DATABASE axon_{name}`
///    against the cluster.
/// 3. Starts the HTTP gateway backed by this router.  gRPC is not started in
///    Postgres mode.
pub async fn run_with_postgres_storage(
    superadmin_dsn: &str,
    args: &ServeArgs,
) -> Result<(), String> {
    if args.mcp_stdio {
        return Err("MCP stdio mode is not supported with --storage=postgres".to_string());
    }

    let auth = auth_context_from_serve_args(args);

    auth.verify().await.map_err(|error| {
        format!(
            "failed to initialize auth via {}: {error}",
            args.tailscale_socket.display()
        )
    })?;

    // Provision axon_master (the master / control-plane database).
    let superadmin_dsn_owned = superadmin_dsn.to_owned();
    let master_conn_str = tokio::task::spawn_blocking({
        let dsn = superadmin_dsn_owned.clone();
        move || {
            match provision_postgres_database(&dsn, "master") {
                Ok(()) => {
                    tracing::info!("created master PostgreSQL database 'axon_master'");
                }
                Err(axon_core::error::AxonError::AlreadyExists(_)) => {
                    tracing::info!("master PostgreSQL database 'axon_master' already exists");
                }
                Err(e) => {
                    return Err(format!("failed to provision axon_master: {e}"));
                }
            }
            Ok(axon_storage::tenant_dsn(&dsn, "master"))
        }
    })
    .await
    .map_err(|e| format!("thread join error while provisioning master database: {e}"))??;

    // Connect to axon_master.
    let pg_master = tokio::task::spawn_blocking({
        let conn = master_conn_str.clone();
        move || {
            PostgresStorageAdapter::connect(&conn)
                .map_err(|e| format!("failed to connect to axon_master: {e}"))
        }
    })
    .await
    .map_err(|e| format!("thread join error while connecting to master database: {e}"))??;

    tracing::info!("connected to master PostgreSQL database at axon_master");

    let default_handler: crate::tenant_router::TenantHandler =
        Arc::new(tokio::sync::Mutex::new(AxonHandler::new(
            Box::new(pg_master) as Box<dyn axon_storage::adapter::StorageAdapter + Send + Sync>,
        )));
    let tenant_router = Arc::new(crate::tenant_router::TenantRouter::new_postgres(
        superadmin_dsn_owned,
        default_handler,
    ));

    let control_plane_db = crate::control_plane::ControlPlaneDb::open(&args.control_plane_path)
        .map_err(|error| format!("failed to open control-plane database: {error}"))?;
    let control_plane_db = Arc::new(tokio::sync::Mutex::new(control_plane_db));
    tracing::info!(
        "control-plane database opened at {}",
        args.control_plane_path
    );

    let data_dir = std::path::Path::new(&args.control_plane_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();
    let control_plane_state =
        crate::control_plane_routes::ControlPlaneState::new(control_plane_db, data_dir);

    let http_app = crate::gateway::build_router_with_auth(
        tenant_router,
        "postgres",
        args.ui_dir.clone(),
        auth,
        crate::rate_limit::RateLimitConfig::default(),
        crate::actor_scope::ActorScopeGuard::default(),
        Some(control_plane_state),
    );
    let http_addr: std::net::SocketAddr = ([0, 0, 0, 0], args.http_port).into();

    tracing::info!("HTTP gateway (PostgreSQL) listening on {http_addr}");

    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(http_addr)
            .await
            .map_err(|error| format!("failed to bind HTTP listener: {error}"))?;
        axum::serve(
            listener,
            http_app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|error| format!("HTTP server error: {error}"))
    })
    .await
    .map_err(|error| format!("HTTP task join error: {error}"))??;

    Ok(())
}

pub async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C handler");
    tracing::info!("shutdown signal received, stopping server");
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::AuthMode;

    #[test]
    fn cli_defaults_to_tailscale_auth() {
        let args = ServeArgs::parse_from(["axon-serve"]);

        assert!(!args.no_auth, "default startup must keep auth enabled");
        assert_eq!(args.http_port, 4170);
        assert!(args.grpc_port.is_none(), "gRPC should be off by default");
        assert_eq!(
            auth_context_from_serve_args(&args).mode(),
            &AuthMode::Tailscale {
                default_role: Role::Read,
            }
        );
    }

    #[test]
    fn cli_no_auth_flag_keeps_explicit_bypass() {
        let args = ServeArgs::parse_from(["axon-serve", "--no-auth"]);

        assert!(args.no_auth, "--no-auth must remain an explicit bypass");
        assert_eq!(
            auth_context_from_serve_args(&args).mode(),
            &AuthMode::NoAuth
        );
    }

    #[test]
    fn cli_no_auth_accepts_boolish_values() {
        let args = ServeArgs::parse_from(["axon-serve", "--no-auth=1"]);

        assert!(
            args.no_auth,
            "boolish values must enable the no-auth bypass"
        );
        assert_eq!(
            auth_context_from_serve_args(&args).mode(),
            &AuthMode::NoAuth
        );
    }

    #[test]
    fn cli_guest_role_enables_guest_mode() {
        let args = ServeArgs::parse_from(["axon-serve", "--guest-role=read"]);

        assert!(!args.no_auth);
        assert_eq!(
            auth_context_from_serve_args(&args).mode(),
            &AuthMode::Guest { role: Role::Read }
        );
    }

    #[test]
    fn cli_guest_role_write() {
        let args = ServeArgs::parse_from(["axon-serve", "--guest-role=write"]);

        assert_eq!(
            auth_context_from_serve_args(&args).mode(),
            &AuthMode::Guest { role: Role::Write }
        );
    }

    #[test]
    fn cli_no_auth_takes_precedence_over_guest_role() {
        let args = ServeArgs::parse_from(["axon-serve", "--no-auth", "--guest-role=read"]);

        assert!(args.no_auth);
        assert_eq!(
            auth_context_from_serve_args(&args).mode(),
            &AuthMode::NoAuth,
            "--no-auth should take precedence over --guest-role"
        );
    }

    #[test]
    fn grpc_opt_in_with_port() {
        let args = ServeArgs::parse_from(["axon-serve", "--grpc-port", "4171"]);
        assert_eq!(args.grpc_port, Some(4171));
    }
}
