//! Tests for the no-auth synthetic identity module (axon-f9e7f523).
//!
//! Covers:
//! - synthesize_returns_admin_grant  — unit: grants contain admin ops on the URL database
//! - tenant_id_is_deterministic      — unit: same (tenant, db) always yields same tenant_id
//! - different_names_different_ids   — unit: distinct tenant names produce distinct tenant_ids
//! - middleware_installs_identity     — integration: no_auth_layer populates ResolvedIdentity
//! - middleware_skips_when_path_not_data_plane — integration: /health gets no identity

use axon_core::auth::{Op, ResolvedIdentity};
use axon_server::no_auth::synthesize_no_auth_identity;
use axon_server::path_router::path_router_layer;
use axum::body::Body;
use axum::extract::Extension;
use axum::http::{Request, StatusCode};
use axum::middleware;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Unit: synthesize_returns_admin_grant
// ---------------------------------------------------------------------------

#[test]
fn synthesize_returns_admin_grant() {
    let identity = synthesize_no_auth_identity("acme", "orders");

    assert_eq!(identity.grants.databases.len(), 1);
    let db = &identity.grants.databases[0];
    assert_eq!(db.name, "orders");

    let ops: Vec<Op> = db.ops.clone();
    assert!(ops.contains(&Op::Read), "expected Read grant");
    assert!(ops.contains(&Op::Write), "expected Write grant");
    assert!(ops.contains(&Op::Admin), "expected Admin grant");
}

// ---------------------------------------------------------------------------
// Unit: tenant_id_is_deterministic
// ---------------------------------------------------------------------------

#[test]
fn tenant_id_is_deterministic() {
    let id1 = synthesize_no_auth_identity("acme", "orders");
    let id2 = synthesize_no_auth_identity("acme", "orders");
    assert_eq!(id1.tenant_id, id2.tenant_id);
}

// ---------------------------------------------------------------------------
// Unit: different_names_different_ids
// ---------------------------------------------------------------------------

#[test]
fn different_names_different_ids() {
    let id_a = synthesize_no_auth_identity("a", "x");
    let id_b = synthesize_no_auth_identity("b", "x");
    assert_ne!(
        id_a.tenant_id, id_b.tenant_id,
        "distinct tenant names must produce distinct tenant_ids"
    );
}

// ---------------------------------------------------------------------------
// Integration: middleware_installs_identity
// ---------------------------------------------------------------------------

#[tokio::test]
async fn middleware_installs_identity() {
    async fn handler(Extension(identity): Extension<ResolvedIdentity>) -> impl IntoResponse {
        // Return the database name from the first grant to prove identity was set.
        identity
            .grants
            .databases
            .first()
            .map(|db| db.name.clone())
            .unwrap_or_default()
    }

    let app = Router::new()
        .route(
            "/tenants/{tenant}/databases/{database}/ping",
            get(handler),
        )
        .layer(middleware::from_fn(axon_server::no_auth::no_auth_layer))
        .layer(middleware::from_fn(path_router_layer));

    let request = Request::builder()
        .uri("/tenants/acme/databases/orders/ping")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    assert_eq!(body_bytes, "orders");
}

// ---------------------------------------------------------------------------
// Integration: middleware_skips_when_path_not_data_plane
// ---------------------------------------------------------------------------

#[tokio::test]
async fn middleware_skips_when_path_not_data_plane() {
    // Handler that returns 200 when no ResolvedIdentity is present,
    // 500 when one is unexpectedly injected.
    async fn health_handler(
        identity: Option<Extension<ResolvedIdentity>>,
    ) -> impl IntoResponse {
        if identity.is_some() {
            StatusCode::INTERNAL_SERVER_ERROR
        } else {
            StatusCode::OK
        }
    }

    let app = Router::new()
        .route("/health", get(health_handler))
        .layer(middleware::from_fn(axon_server::no_auth::no_auth_layer))
        .layer(middleware::from_fn(path_router_layer));

    let request = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "/health must not receive a ResolvedIdentity"
    );
}
