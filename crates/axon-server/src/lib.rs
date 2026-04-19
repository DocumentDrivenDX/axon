//! Axon server — HTTP gateway, gRPC service, MCP stdio, and authentication.
//!
//! # Authentication
//!
//! Identity is resolved once per request by [`auth::AuthContext`] and
//! injected as a typed extension.  Three modes are supported:
//!
//! | Mode | Flag | Actor | Role |
//! |------|------|-------|------|
//! | `NoAuth` | `--no-auth` | `"anonymous"` | Admin |
//! | `Tailscale` | *(default)* | node name | from ACL tags |
//! | `Guest` | `--guest-role <role>` | `"guest"` | configured role |
//!
//! In `Tailscale` mode the server contacts the local Tailscale daemon over its
//! Unix socket (`/run/tailscale/tailscaled.sock` by default) and calls
//! `/localapi/v0/whois?addr=<peer>` to resolve the connecting node's identity.
//! Resolved identities are cached by peer IP for the duration of
//! `--auth-cache-ttl-secs` (default 60 s).
//!
//! ACL tag → role mapping:
//! - `tag:axon-admin` → [`Role::Admin`]
//! - `tag:axon-write` / `tag:axon-agent` → [`Role::Write`]
//! - `tag:axon-read` → [`Role::Read`]
//! - no matching tag → `--tailscale-default-role` (default `read`)
//!
//! Connections that are not on the tailnet are rejected with HTTP 401 / gRPC
//! `UNAUTHENTICATED`.  If the Tailscale daemon is unreachable the server
//! returns HTTP 503 / gRPC `UNAVAILABLE`.
//!
//! # Request flow
//!
//! ```text
//! TCP accept
//!   └─ authenticate_http_request  (gateway.rs middleware)
//!        ├─ AuthContext::resolve_peer  →  Identity
//!        └─ insert Identity into request extensions
//!             └─ route handler
//!                  ├─ extract Extension<Identity>
//!                  ├─ identity.require_read() / require_write() / require_admin()
//!                  └─ handler logic
//! ```
//!
//! gRPC follows the same pattern via [`service::AxonServiceImpl::authorize`].
pub mod actor_scope;
pub mod auth;
pub mod auth_pipeline;
pub mod bootstrap;
mod collection_listing;
pub mod control_plane;
pub mod control_plane_authz;
pub mod control_plane_routes;
pub mod cors_config;
pub mod database_router;
pub(crate) mod embedded_ui;
pub mod federation;
pub mod gateway;
pub mod idempotency;
pub(crate) mod mcp_http;
pub mod mcp_stdio;
pub mod no_auth;
pub mod path_router;
pub mod rate_limit;
pub mod schema_registry;
pub mod serve;
pub mod service;
pub mod tenant_router;
pub mod tls_bootstrap;
pub mod user_roles;

pub use auth::{AuthContext, AuthMode, Identity, Role};
pub use auth_pipeline::{jwt_verify_layer, InMemoryRevocationCache, JwtIssuer};
pub use database_router::{DatabaseAdapterFactory, DatabaseRouter, MemoryAdapterFactory};
pub use mcp_stdio::run_mcp_stdio;
pub use no_auth::{no_auth_layer, synthesize_no_auth_identity};
pub use path_router::{extract_tenant_database, path_router_layer, ResolvedPath};
pub use service::AxonServiceImpl;
