//! Integration tests for user provisioning on the control plane
//! (axon-0a6eb28a).
//!
//! Covered:
//! - admin_can_create_user
//! - list_users_returns_created_user
//! - duplicate_display_name_is_accepted
//! - suspend_user_sets_suspended_at_ms
//! - non_admin_gets_403

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
    control_plane_routes, optional_jwt_middleware, ControlPlaneState,
};
use axon_server::cors_config::CorsStore;
use axon_server::user_roles::UserRoleStore;
use axon_storage::MemoryStorageAdapter;

// ── Constants ─────────────────────────────────────────────────────────────────

const SECRET: &[u8] = b"test-secret-for-control-users-provision";
const ISSUER_ID: &str = "test-issuer-users-provision";

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
/// Returns (TestServer, issuer, admin_user_id, non_admin_user_id).
fn build_test_env() -> (TestServer, Arc<JwtIssuer>, String, String) {
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
        std::path::PathBuf::from("/tmp/axon-test-users-provision"),
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
    (server, issuer, admin_user_id, non_admin_user_id)
}

fn auth_header(jwt: &str) -> (http::HeaderName, HeaderValue) {
    (
        http::header::AUTHORIZATION,
        format!("Bearer {jwt}").parse::<HeaderValue>().unwrap(),
    )
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// admin_can_create_user — deployment admin POSTs to /control/users/provision;
/// receives 201 with a generated id.
#[tokio::test(flavor = "multi_thread")]
async fn admin_can_create_user() {
    let (server, issuer, admin_uid, _) = build_test_env();
    let jwt = make_jwt(&issuer, &admin_uid);
    let (hname, hval) = auth_header(&jwt);

    let resp = server
        .post("/control/users/provision")
        .add_header(hname, hval)
        .json(&json!({ "display_name": "Alice", "email": "alice@example.com" }))
        .await;

    resp.assert_status(StatusCode::CREATED);
    let body: Value = resp.json();
    assert!(
        !body["id"].as_str().unwrap_or("").is_empty(),
        "id must be present"
    );
    assert_eq!(body["display_name"].as_str().unwrap(), "Alice");
    assert_eq!(body["email"].as_str().unwrap(), "alice@example.com");
    assert!(body["created_at_ms"].as_u64().unwrap() > 0);
}

/// list_users_returns_created_user — after creating a user, GET /control/users/list
/// returns that user in the response.
#[tokio::test(flavor = "multi_thread")]
async fn list_users_returns_created_user() {
    let (server, issuer, admin_uid, _) = build_test_env();
    let jwt = make_jwt(&issuer, &admin_uid);
    let (hname, hval) = auth_header(&jwt);

    // Create a user.
    let create_resp = server
        .post("/control/users/provision")
        .add_header(hname.clone(), hval.clone())
        .json(&json!({ "display_name": "Bob" }))
        .await;
    create_resp.assert_status(StatusCode::CREATED);
    let created: Value = create_resp.json();
    let created_id = created["id"].as_str().unwrap().to_string();

    // List users and verify the created user appears.
    let list_resp = server
        .get("/control/users/list")
        .add_header(hname, hval)
        .await;
    list_resp.assert_status(StatusCode::OK);
    let list_body: Value = list_resp.json();
    let users = list_body["users"].as_array().expect("users array");
    let found = users
        .iter()
        .any(|u| u["id"].as_str().unwrap_or("") == created_id);
    assert!(found, "created user should appear in list");
}

/// duplicate_display_name_is_accepted — two users with the same display_name
/// are accepted (no uniqueness constraint on display_name).
#[tokio::test(flavor = "multi_thread")]
async fn duplicate_display_name_is_accepted() {
    let (server, issuer, admin_uid, _) = build_test_env();
    let jwt = make_jwt(&issuer, &admin_uid);
    let (hname, hval) = auth_header(&jwt);

    let resp1 = server
        .post("/control/users/provision")
        .add_header(hname.clone(), hval.clone())
        .json(&json!({ "display_name": "Charlie" }))
        .await;
    resp1.assert_status(StatusCode::CREATED);

    let resp2 = server
        .post("/control/users/provision")
        .add_header(hname, hval)
        .json(&json!({ "display_name": "Charlie" }))
        .await;
    resp2.assert_status(StatusCode::CREATED);

    // The two users should have different ids.
    let body1: Value = resp1.json();
    let body2: Value = resp2.json();
    assert_ne!(
        body1["id"].as_str().unwrap(),
        body2["id"].as_str().unwrap(),
        "each user should have a distinct id"
    );
}

/// suspend_user_sets_suspended_at_ms — after suspending a user, listing shows
/// suspended_at_ms is set.
#[tokio::test(flavor = "multi_thread")]
async fn suspend_user_sets_suspended_at_ms() {
    let (server, issuer, admin_uid, _) = build_test_env();
    let jwt = make_jwt(&issuer, &admin_uid);
    let (hname, hval) = auth_header(&jwt);

    // Create a user.
    let create_resp = server
        .post("/control/users/provision")
        .add_header(hname.clone(), hval.clone())
        .json(&json!({ "display_name": "Dana" }))
        .await;
    create_resp.assert_status(StatusCode::CREATED);
    let created: Value = create_resp.json();
    let user_id = created["id"].as_str().unwrap().to_string();

    // Suspend the user.
    let suspend_resp = server
        .delete(&format!("/control/users/suspend/{user_id}"))
        .add_header(hname.clone(), hval.clone())
        .await;
    suspend_resp.assert_status(StatusCode::OK);

    // Verify suspended_at_ms is now set in the list.
    let list_resp = server
        .get("/control/users/list")
        .add_header(hname, hval)
        .await;
    list_resp.assert_status(StatusCode::OK);
    let list_body: Value = list_resp.json();
    let users = list_body["users"].as_array().expect("users array");
    let user = users
        .iter()
        .find(|u| u["id"].as_str().unwrap_or("") == user_id)
        .expect("suspended user should still appear in list");
    assert!(
        user["suspended_at_ms"].as_u64().unwrap_or(0) > 0,
        "suspended_at_ms must be set after suspension"
    );
}

/// non_admin_gets_403 — a non-admin JWT receives 403 on all three endpoints.
#[tokio::test(flavor = "multi_thread")]
async fn non_admin_gets_403() {
    let (server, issuer, _, non_admin_uid) = build_test_env();
    let jwt = make_jwt(&issuer, &non_admin_uid);
    let (hname, hval) = auth_header(&jwt);

    // POST /control/users/provision
    let resp = server
        .post("/control/users/provision")
        .add_header(hname.clone(), hval.clone())
        .json(&json!({ "display_name": "Eve" }))
        .await;
    resp.assert_status(StatusCode::FORBIDDEN);

    // GET /control/users/list
    let resp2 = server
        .get("/control/users/list")
        .add_header(hname.clone(), hval.clone())
        .await;
    resp2.assert_status(StatusCode::FORBIDDEN);

    // DELETE /control/users/suspend/some-id
    let resp3 = server
        .delete("/control/users/suspend/some-fake-id")
        .add_header(hname, hval)
        .await;
    resp3.assert_status(StatusCode::FORBIDDEN);
}
