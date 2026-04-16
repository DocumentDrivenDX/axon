//! Integration tests for tenant-scoped database CRUD on the control plane
//! (axon-df98e262, ADR-018, FEAT-014).
//!
//! Covered:
//! - admin_can_create_database
//! - non_admin_cannot_create_database
//! - create_duplicate_returns_409
//! - delete_removes_row
//! - delete_missing_returns_404
//! - two_tenants_same_database_name
//! - invalid_database_name_rejected

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::connect_info::MockConnectInfo;
use axum::http::StatusCode;
use axum::Router;
use axum_test::TestServer;
use http::HeaderValue;
use serde_json::{json, Value};
use uuid::Uuid;

use axon_core::auth::{Grants, JwtClaims};
use axon_server::auth::AuthContext;
use axon_server::auth_pipeline::JwtIssuer;
use axon_server::control_plane::ControlPlaneDb;
use axon_server::control_plane_routes::{
    ControlPlaneState, control_plane_routes, optional_jwt_middleware,
};
use axon_server::cors_config::CorsStore;
use axon_server::user_roles::UserRoleStore;
use axon_storage::MemoryStorageAdapter;

// ── Constants ─────────────────────────────────────────────────────────────────

const SECRET: &[u8] = b"test-secret-for-control-databases";
const ISSUER_ID: &str = "test-issuer-databases";

// ── Helpers ───────────────────────────────────────────────────────────────────

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Build a JWT for the given user_id with no database grants.
fn make_jwt(issuer: &JwtIssuer, user_id: &str) -> String {
    let now = now_secs();
    let claims = JwtClaims {
        iss: ISSUER_ID.to_string(),
        sub: user_id.to_string(),
        aud: "deployment".to_string(),
        jti: Uuid::now_v7().to_string(),
        iat: now,
        nbf: now,
        exp: now + 3600,
        grants: Grants { databases: vec![] },
    };
    issuer.issue(&claims).expect("JWT issue should succeed")
}

/// Build a test environment: in-memory CP DB, MemoryStorageAdapter, JwtIssuer.
///
/// Returns (TestServer, issuer, admin_user_id, non_admin_user_id, storage_arc).
///
/// The `admin_user_id` is registered as `Role::Admin` in the user_roles store.
/// The `non_admin_user_id` has no explicit role assignment.
#[allow(clippy::type_complexity)]
fn build_test_env() -> (
    TestServer,
    Arc<JwtIssuer>,
    String,
    String,
    Arc<Mutex<Box<dyn axon_storage::StorageAdapter + Send + Sync>>>,
) {
    let issuer = Arc::new(JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string()));

    let admin_user_id = Uuid::now_v7().to_string();
    let non_admin_user_id = Uuid::now_v7().to_string();

    let user_roles = UserRoleStore::default();
    user_roles.set_cached(admin_user_id.clone(), axon_server::auth::Role::Admin);

    let storage: Box<dyn axon_storage::StorageAdapter + Send + Sync> =
        Box::new(MemoryStorageAdapter::default());
    let storage = Arc::new(Mutex::new(storage));

    let cp_db = ControlPlaneDb::open_in_memory().expect("open in-memory control-plane db");
    let state = ControlPlaneState::new(
        Arc::new(tokio::sync::Mutex::new(cp_db)),
        std::path::PathBuf::from("/tmp/axon-test-databases"),
        user_roles,
        CorsStore::default(),
    )
    .with_storage(storage.clone())
    .with_jwt_issuer(issuer.clone());

    let peer: SocketAddr = "127.0.0.1:12345".parse().unwrap();
    let auth = AuthContext::no_auth();

    let app = Router::new()
        .nest("/control", control_plane_routes())
        .with_state(state.clone())
        .layer(axum::middleware::from_fn_with_state(
            state,
            optional_jwt_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            auth,
            axon_server::gateway::authenticate_http_request,
        ))
        .layer(MockConnectInfo(peer));

    let server = TestServer::new(app);
    (server, issuer, admin_user_id, non_admin_user_id, storage)
}

fn auth_header(jwt: &str) -> (http::HeaderName, HeaderValue) {
    (
        http::header::AUTHORIZATION,
        format!("Bearer {jwt}").parse::<HeaderValue>().unwrap(),
    )
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// admin_can_create_database — deployment admin creates a database in a tenant;
/// 201 + row present in subsequent GET.
#[tokio::test]
async fn admin_can_create_database() {
    let (server, issuer, admin_uid, _, _) = build_test_env();
    let tenant_id = "tenant-acdb-01";

    let jwt = make_jwt(&issuer, &admin_uid);
    let (hname, hval) = auth_header(&jwt);

    // POST to create the database.
    let resp = server
        .post(&format!("/control/tenants/{tenant_id}/databases"))
        .add_header(hname.clone(), hval.clone())
        .json(&json!({ "name": "orders" }))
        .await;

    resp.assert_status(StatusCode::CREATED);
    let body: Value = resp.json();
    assert_eq!(body["name"].as_str().unwrap(), "orders");
    assert_eq!(body["tenant_id"].as_str().unwrap(), tenant_id);

    // Verify via GET.
    let resp2 = server
        .get(&format!("/control/tenants/{tenant_id}/databases"))
        .add_header(hname, hval)
        .await;
    resp2.assert_status(StatusCode::OK);
    let body2: Value = resp2.json();
    let dbs = body2["databases"].as_array().expect("databases array");
    assert_eq!(dbs.len(), 1);
    assert_eq!(dbs[0]["name"].as_str().unwrap(), "orders");
}

/// non_admin_cannot_create_database — non-admin JWT receives 403.
#[tokio::test]
async fn non_admin_cannot_create_database() {
    let (server, issuer, _, non_admin_uid, _) = build_test_env();
    let tenant_id = "tenant-nacd-01";

    let jwt = make_jwt(&issuer, &non_admin_uid);
    let (hname, hval) = auth_header(&jwt);

    let resp = server
        .post(&format!("/control/tenants/{tenant_id}/databases"))
        .add_header(hname, hval)
        .json(&json!({ "name": "orders" }))
        .await;

    resp.assert_status(StatusCode::FORBIDDEN);
}

/// create_duplicate_returns_409 — two create calls for same (tenant, name),
/// second returns 409.
#[tokio::test]
async fn create_duplicate_returns_409() {
    let (server, issuer, admin_uid, _, _) = build_test_env();
    let tenant_id = "tenant-cdr409-01";

    let jwt = make_jwt(&issuer, &admin_uid);
    let (hname, hval) = auth_header(&jwt);

    // First create — should succeed.
    let resp1 = server
        .post(&format!("/control/tenants/{tenant_id}/databases"))
        .add_header(hname.clone(), hval.clone())
        .json(&json!({ "name": "orders" }))
        .await;
    resp1.assert_status(StatusCode::CREATED);

    // Second create — same tenant + name → 409.
    let resp2 = server
        .post(&format!("/control/tenants/{tenant_id}/databases"))
        .add_header(hname, hval)
        .json(&json!({ "name": "orders" }))
        .await;
    resp2.assert_status(StatusCode::CONFLICT);
}

/// delete_removes_row — create then delete; GET shows empty.
#[tokio::test]
async fn delete_removes_row() {
    let (server, issuer, admin_uid, _, _) = build_test_env();
    let tenant_id = "tenant-drr-01";

    let jwt = make_jwt(&issuer, &admin_uid);
    let (hname, hval) = auth_header(&jwt);

    // Create the database.
    let resp = server
        .post(&format!("/control/tenants/{tenant_id}/databases"))
        .add_header(hname.clone(), hval.clone())
        .json(&json!({ "name": "inventory" }))
        .await;
    resp.assert_status(StatusCode::CREATED);

    // Delete it.
    let resp2 = server
        .delete(&format!("/control/tenants/{tenant_id}/databases/inventory"))
        .add_header(hname.clone(), hval.clone())
        .await;
    resp2.assert_status(StatusCode::NO_CONTENT);

    // Verify GET shows empty.
    let resp3 = server
        .get(&format!("/control/tenants/{tenant_id}/databases"))
        .add_header(hname, hval)
        .await;
    resp3.assert_status(StatusCode::OK);
    let body: Value = resp3.json();
    assert_eq!(body["databases"].as_array().unwrap().len(), 0, "database should be removed");
}

/// delete_missing_returns_404 — delete a never-created database returns 404.
#[tokio::test]
async fn delete_missing_returns_404() {
    let (server, issuer, admin_uid, _, _) = build_test_env();
    let tenant_id = "tenant-dmr404-01";

    let jwt = make_jwt(&issuer, &admin_uid);
    let (hname, hval) = auth_header(&jwt);

    let resp = server
        .delete(&format!("/control/tenants/{tenant_id}/databases/nonexistent"))
        .add_header(hname, hval)
        .await;
    resp.assert_status(StatusCode::NOT_FOUND);
}

/// two_tenants_same_database_name — verify (tenant_a, "orders") and
/// (tenant_b, "orders") coexist (different rows, different tenants).
#[tokio::test]
async fn two_tenants_same_database_name() {
    let (server, issuer, admin_uid, _, _) = build_test_env();
    let tenant_a = "tenant-ttsdn-a";
    let tenant_b = "tenant-ttsdn-b";

    let jwt = make_jwt(&issuer, &admin_uid);
    let (hname, hval) = auth_header(&jwt);

    // Create "orders" in tenant A.
    let resp_a = server
        .post(&format!("/control/tenants/{tenant_a}/databases"))
        .add_header(hname.clone(), hval.clone())
        .json(&json!({ "name": "orders" }))
        .await;
    resp_a.assert_status(StatusCode::CREATED);

    // Create "orders" in tenant B — should also succeed.
    let resp_b = server
        .post(&format!("/control/tenants/{tenant_b}/databases"))
        .add_header(hname.clone(), hval.clone())
        .json(&json!({ "name": "orders" }))
        .await;
    resp_b.assert_status(StatusCode::CREATED);

    // GET for tenant A → 1 database named "orders" belonging to tenant A.
    let resp_get_a = server
        .get(&format!("/control/tenants/{tenant_a}/databases"))
        .add_header(hname.clone(), hval.clone())
        .await;
    resp_get_a.assert_status(StatusCode::OK);
    let body_a: Value = resp_get_a.json();
    let dbs_a = body_a["databases"].as_array().unwrap();
    assert_eq!(dbs_a.len(), 1);
    assert_eq!(dbs_a[0]["tenant_id"].as_str().unwrap(), tenant_a);
    assert_eq!(dbs_a[0]["name"].as_str().unwrap(), "orders");

    // GET for tenant B → 1 database named "orders" belonging to tenant B.
    let resp_get_b = server
        .get(&format!("/control/tenants/{tenant_b}/databases"))
        .add_header(hname, hval)
        .await;
    resp_get_b.assert_status(StatusCode::OK);
    let body_b: Value = resp_get_b.json();
    let dbs_b = body_b["databases"].as_array().unwrap();
    assert_eq!(dbs_b.len(), 1);
    assert_eq!(dbs_b[0]["tenant_id"].as_str().unwrap(), tenant_b);
    assert_eq!(dbs_b[0]["name"].as_str().unwrap(), "orders");
}

/// invalid_database_name_rejected — names violating the D1 identifier rule
/// return 400.
#[tokio::test]
async fn invalid_database_name_rejected() {
    let (server, issuer, admin_uid, _, _) = build_test_env();
    let tenant_id = "tenant-idnr-01";

    let jwt = make_jwt(&issuer, &admin_uid);
    let (hname, hval) = auth_header(&jwt);

    let too_long = "a".repeat(64);
    let invalid_names: &[&str] = &[
        "1starts-with-digit",  // starts with digit
        "has space",           // space not allowed
        "has@symbol",          // @ not allowed
        &too_long,             // 64 chars — too long
    ];

    for bad_name in invalid_names {
        let resp = server
            .post(&format!("/control/tenants/{tenant_id}/databases"))
            .add_header(hname.clone(), hval.clone())
            .json(&json!({ "name": bad_name }))
            .await;
        assert_eq!(
            resp.status_code(),
            StatusCode::BAD_REQUEST,
            "expected 400 for invalid name: {bad_name:?}"
        );
    }

    // A valid name should still succeed.
    let resp_ok = server
        .post(&format!("/control/tenants/{tenant_id}/databases"))
        .add_header(hname, hval)
        .json(&json!({ "name": "valid-name_123" }))
        .await;
    resp_ok.assert_status(StatusCode::CREATED);
}
