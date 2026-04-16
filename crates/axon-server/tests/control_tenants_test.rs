//! Integration tests for tenant-membership and retention-policy control-plane
//! endpoints (axon-c6908e78, ADR-018).
//!
//! Covered:
//! - admin_can_list_members
//! - non_admin_cannot_upsert_member
//! - admin_upsert_creates_row
//! - admin_remove_deletes_row
//! - retention_default_is_7y
//! - retention_can_be_overridden
//! - SCN-012 user_in_two_tenants (membership isolation)

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

use axon_core::auth::{Grants, JwtClaims, TenantId, TenantRole, UserId};
use axon_server::auth::AuthContext;
use axon_server::auth_pipeline::JwtIssuer;
use axon_server::control_plane::ControlPlaneDb;
use axon_server::control_plane_routes::{
    ControlPlaneState, control_plane_routes, optional_jwt_middleware,
};
use axon_server::user_roles::UserRoleStore;
use axon_server::cors_config::CorsStore;
use axon_storage::MemoryStorageAdapter;

// ── Constants ─────────────────────────────────────────────────────────────────

const SECRET: &[u8] = b"test-secret-for-control-tenants";
const ISSUER_ID: &str = "test-issuer";

// ── Helpers ───────────────────────────────────────────────────────────────────

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Build a JWT for the given user_id with no database grants.
/// The `aud` claim is set to "deployment" to indicate a control-plane token.
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
    UserRoleStore,
) {
    let issuer = Arc::new(JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string()));

    let admin_user_id = Uuid::now_v7().to_string();
    let non_admin_user_id = Uuid::now_v7().to_string();

    let user_roles = UserRoleStore::default();
    user_roles.set_cached(admin_user_id.clone(), axon_server::auth::Role::Admin);
    // non_admin_user_id intentionally NOT set (no role).

    let storage: Box<dyn axon_storage::StorageAdapter + Send + Sync> =
        Box::new(MemoryStorageAdapter::default());
    let storage = Arc::new(Mutex::new(storage));

    let cp_db = ControlPlaneDb::open_in_memory().expect("open in-memory control-plane db");
    let state = ControlPlaneState::new(
        Arc::new(tokio::sync::Mutex::new(cp_db)),
        std::path::PathBuf::from("/tmp/axon-test-tenants"),
        user_roles.clone(),
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
    (server, issuer, admin_user_id, non_admin_user_id, storage, user_roles)
}

/// Helper: insert a tenant into the in-memory storage's tenant_members.
fn insert_member(
    storage: &Arc<Mutex<Box<dyn axon_storage::StorageAdapter + Send + Sync>>>,
    tenant_id: &str,
    user_id: &str,
    role: TenantRole,
) {
    let s = storage.lock().unwrap();
    s.upsert_tenant_member(TenantId::new(tenant_id), UserId::new(user_id), role)
        .expect("upsert_tenant_member should succeed");
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// admin_can_list_members — deployment-admin JWT can GET /members and sees all members.
#[tokio::test(flavor = "multi_thread")]
async fn admin_can_list_members() {
    let (server, issuer, admin_uid, _, storage, _) = build_test_env();

    let tenant_id = "tenant-aclm-01";
    let user_a = "user-a-aclm";
    let user_b = "user-b-aclm";

    insert_member(&storage, tenant_id, user_a, TenantRole::Write);
    insert_member(&storage, tenant_id, user_b, TenantRole::Read);

    let jwt = make_jwt(&issuer, &admin_uid);
    let resp = server
        .get(&format!("/control/tenants/{tenant_id}/members"))
        .add_header(
            http::header::AUTHORIZATION,
            format!("Bearer {jwt}").parse::<HeaderValue>().unwrap(),
        )
        .await;

    resp.assert_status(StatusCode::OK);
    let body: Value = resp.json();
    let members = body["members"].as_array().expect("members array");
    assert_eq!(members.len(), 2, "expected 2 members");
}

/// non_admin_cannot_upsert_member — non-admin JWT receives 403.
#[tokio::test(flavor = "multi_thread")]
async fn non_admin_cannot_upsert_member() {
    let (server, issuer, _, non_admin_uid, _, _) = build_test_env();

    let tenant_id = "tenant-nacum-01";
    let target_user = "user-target-nacum";

    let jwt = make_jwt(&issuer, &non_admin_uid);
    let resp = server
        .put(&format!("/control/tenants/{tenant_id}/members/{target_user}"))
        .add_header(
            http::header::AUTHORIZATION,
            format!("Bearer {jwt}").parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({ "role": "read" }))
        .await;

    resp.assert_status(StatusCode::FORBIDDEN);
}

/// admin_upsert_creates_row — admin PUT .../members/:uid → 200 + row appears in GET.
#[tokio::test(flavor = "multi_thread")]
async fn admin_upsert_creates_row() {
    let (server, issuer, admin_uid, _, _storage, _) = build_test_env();

    let tenant_id = "tenant-aucr-01";
    let target_user = "user-target-aucr";

    let jwt = make_jwt(&issuer, &admin_uid);

    // PUT to upsert the member.
    let resp = server
        .put(&format!("/control/tenants/{tenant_id}/members/{target_user}"))
        .add_header(
            http::header::AUTHORIZATION,
            format!("Bearer {jwt}").parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({ "role": "write" }))
        .await;

    resp.assert_status(StatusCode::OK);
    let body: Value = resp.json();
    assert_eq!(body["user_id"].as_str().unwrap(), target_user);
    assert_eq!(body["role"].as_str().unwrap(), "write");

    // Verify via GET.
    let resp2 = server
        .get(&format!("/control/tenants/{tenant_id}/members"))
        .add_header(
            http::header::AUTHORIZATION,
            format!("Bearer {jwt}").parse::<HeaderValue>().unwrap(),
        )
        .await;
    resp2.assert_status(StatusCode::OK);
    let body2: Value = resp2.json();
    let members = body2["members"].as_array().unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0]["user_id"].as_str().unwrap(), target_user);
}

/// admin_remove_deletes_row — admin DELETE .../members/:uid → 204 + row gone.
#[tokio::test(flavor = "multi_thread")]
async fn admin_remove_deletes_row() {
    let (server, issuer, admin_uid, _, storage, _) = build_test_env();

    let tenant_id = "tenant-ardr-01";
    let target_user = "user-target-ardr";

    // Pre-insert via storage directly.
    insert_member(&storage, tenant_id, target_user, TenantRole::Read);

    let jwt = make_jwt(&issuer, &admin_uid);

    // DELETE the member.
    let resp = server
        .delete(&format!(
            "/control/tenants/{tenant_id}/members/{target_user}"
        ))
        .add_header(
            http::header::AUTHORIZATION,
            format!("Bearer {jwt}").parse::<HeaderValue>().unwrap(),
        )
        .await;
    resp.assert_status(StatusCode::NO_CONTENT);

    // Verify via GET — list should be empty.
    let resp2 = server
        .get(&format!("/control/tenants/{tenant_id}/members"))
        .add_header(
            http::header::AUTHORIZATION,
            format!("Bearer {jwt}").parse::<HeaderValue>().unwrap(),
        )
        .await;
    resp2.assert_status(StatusCode::OK);
    let body: Value = resp2.json();
    assert_eq!(
        body["members"].as_array().unwrap().len(),
        0,
        "member should be removed"
    );
}

/// retention_default_is_7y — fresh tenant has default 220752000 s archive window.
#[tokio::test(flavor = "multi_thread")]
async fn retention_default_is_7y() {
    let (server, issuer, admin_uid, _, _, _) = build_test_env();
    let tenant_id = "tenant-rdy-01";

    let jwt = make_jwt(&issuer, &admin_uid);
    let resp = server
        .get(&format!("/control/tenants/{tenant_id}/retention"))
        .add_header(
            http::header::AUTHORIZATION,
            format!("Bearer {jwt}").parse::<HeaderValue>().unwrap(),
        )
        .await;

    resp.assert_status(StatusCode::OK);
    let body: Value = resp.json();
    assert_eq!(
        body["archive_after_seconds"].as_u64().unwrap(),
        220_752_000,
        "default archive window should be 7 years (220_752_000 s)"
    );
    assert!(body.get("purge_after_seconds").is_none() || body["purge_after_seconds"].is_null());
}

/// retention_can_be_overridden — PUT custom policy, GET returns it.
#[tokio::test(flavor = "multi_thread")]
async fn retention_can_be_overridden() {
    let (server, issuer, admin_uid, _, _, _) = build_test_env();
    let tenant_id = "tenant-rcbo-01";

    let jwt = make_jwt(&issuer, &admin_uid);

    let resp = server
        .put(&format!("/control/tenants/{tenant_id}/retention"))
        .add_header(
            http::header::AUTHORIZATION,
            format!("Bearer {jwt}").parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "archive_after_seconds": 30,
            "purge_after_seconds": 60
        }))
        .await;
    resp.assert_status(StatusCode::OK);

    let resp2 = server
        .get(&format!("/control/tenants/{tenant_id}/retention"))
        .add_header(
            http::header::AUTHORIZATION,
            format!("Bearer {jwt}").parse::<HeaderValue>().unwrap(),
        )
        .await;
    resp2.assert_status(StatusCode::OK);
    let body: Value = resp2.json();
    assert_eq!(body["archive_after_seconds"].as_u64().unwrap(), 30);
    assert_eq!(body["purge_after_seconds"].as_u64().unwrap(), 60);
}

/// SCN-012 user_in_two_tenants — same user as admin in A and read in B;
/// GET /members on each returns independent role assignments.
#[tokio::test(flavor = "multi_thread")]
async fn scn_012_user_in_two_tenants() {
    let (server, issuer, admin_uid, _, storage, _) = build_test_env();

    let tenant_a = "tenant-scn012-a";
    let tenant_b = "tenant-scn012-b";
    let shared_user = "user-scn012-shared";

    insert_member(&storage, tenant_a, shared_user, TenantRole::Admin);
    insert_member(&storage, tenant_b, shared_user, TenantRole::Read);

    let jwt = make_jwt(&issuer, &admin_uid);

    // GET members for tenant A.
    let resp_a = server
        .get(&format!("/control/tenants/{tenant_a}/members"))
        .add_header(
            http::header::AUTHORIZATION,
            format!("Bearer {jwt}").parse::<HeaderValue>().unwrap(),
        )
        .await;
    resp_a.assert_status(StatusCode::OK);
    let body_a: Value = resp_a.json();
    let members_a = body_a["members"].as_array().unwrap();
    assert_eq!(members_a.len(), 1);
    assert_eq!(members_a[0]["user_id"].as_str().unwrap(), shared_user);
    assert_eq!(members_a[0]["role"].as_str().unwrap(), "admin");

    // GET members for tenant B.
    let resp_b = server
        .get(&format!("/control/tenants/{tenant_b}/members"))
        .add_header(
            http::header::AUTHORIZATION,
            format!("Bearer {jwt}").parse::<HeaderValue>().unwrap(),
        )
        .await;
    resp_b.assert_status(StatusCode::OK);
    let body_b: Value = resp_b.json();
    let members_b = body_b["members"].as_array().unwrap();
    assert_eq!(members_b.len(), 1);
    assert_eq!(members_b[0]["user_id"].as_str().unwrap(), shared_user);
    assert_eq!(members_b[0]["role"].as_str().unwrap(), "read");
}
