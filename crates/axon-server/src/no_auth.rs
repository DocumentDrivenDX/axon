//! No-auth mode: synthetic identity injection for `--no-auth` servers.
//!
//! When the server starts with `--no-auth`, every incoming data-plane request
//! bypasses JWT verification.  This module synthesizes a [`ResolvedIdentity`]
//! that carries an anonymous user UUID, a deterministic tenant ID derived from
//! the URL's tenant segment, and a full admin grant on the URL's database.
//!
//! # Middleware ordering requirement
//!
//! [`no_auth_layer`] reads the [`ResolvedPath`] extension installed by
//! [`crate::path_router::path_router_layer`].  Therefore
//! `path_router_layer` **must run before** `no_auth_layer` in the middleware
//! stack.

use axum::body::Body;
use axum::extract::Extension;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;

use axon_core::auth::{GrantedDatabase, Grants, Op, ResolvedIdentity, TenantId, UserId};

use crate::path_router::ResolvedPath;

/// Synthesize a [`ResolvedIdentity`] for `--no-auth` mode.
///
/// The returned identity carries:
/// - An anonymous user (nil UUID — all-zeros, deterministic).
/// - A deterministic `TenantId` derived from `tenant` via
///   [`TenantId::from_name`] (UUIDv5, stable across restarts).
/// - An admin grant on `database` covering all three operations
///   (`Read`, `Write`, `Admin`).
pub fn synthesize_no_auth_identity(tenant: &str, database: &str) -> ResolvedIdentity {
    ResolvedIdentity {
        user_id: UserId::nil(),
        tenant_id: TenantId::from_name(tenant),
        grants: Grants {
            databases: vec![GrantedDatabase {
                name: database.to_string(),
                ops: vec![Op::Read, Op::Write, Op::Admin],
            }],
        },
    }
}

/// Axum middleware that installs a synthetic [`ResolvedIdentity`] for
/// `--no-auth` mode.
///
/// Reads the [`ResolvedPath`] extension (populated by
/// `path_router_layer`) and calls [`synthesize_no_auth_identity`] to
/// construct an admin identity scoped to the URL's `(tenant, database)`.
///
/// If no [`ResolvedPath`] is present (e.g. `/health`, `/metrics`), the
/// middleware is a no-op and no identity is installed.
///
/// # Ordering
///
/// `path_router_layer` **must run before** this layer so that
/// [`ResolvedPath`] is populated when this middleware executes.
pub async fn no_auth_layer(
    resolved_path: Option<Extension<ResolvedPath>>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    if let Some(Extension(path)) = resolved_path {
        let identity = synthesize_no_auth_identity(&path.tenant, &path.database);
        req.extensions_mut().insert(identity);
    }
    next.run(req).await
}
