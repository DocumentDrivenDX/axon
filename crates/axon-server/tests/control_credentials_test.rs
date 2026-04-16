//! Integration tests for credential (JWT) issuance, listing, and revocation
//! on the control plane (axon-906b527a, ADR-018 §4).
//!
//! Covered:
//! - issue_with_deployment_admin
//! - issue_over_scoped_returns_403
//! - self_issue_within_ceiling
//! - list_credentials_admin
//! - list_credentials_self
//! - list_never_returns_signed_jwt
//! - revoke_by_admin
//! - revoke_by_owner
//! - revoke_by_unrelated_user_returns_403
//! - revoked_credential_rejected_on_next_request

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::StatusCode;
use axum::routing::get;
use axum::Router;
use axum_test::TestServer;
use http::{HeaderValue, Request};
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;

use axon_core::auth::{
    Grants, JwtClaims, TenantId, TenantMember, TenantRole, User, UserId,
};
use axon_server::auth::AuthContext;
use axon_server::auth_pipeline::{AuthPipelineState, InMemoryRevocationCache, JwtIssuer, jwt_verify_layer};
use axon_server::control_plane::ControlPlaneDb;
use axon_server::control_plane_routes::{
    ControlPlaneState, control_plane_routes, optional_jwt_middleware,
};
use axon_server::cors_config::CorsStore;
use axon_server::user_roles::UserRoleStore;
use axon_storage::MemoryStorageAdapter;

// ── Constants ─────────────────────────────────────────────────────────────────

const SECRET: &[u8] = b"test-secret-for-control-credentials";
const ISSUER_ID: &str = "test-issuer-credentials";
const TENANT_ID: &str = "tenant-cred-test-01";

// ── Helpers ───────────────────────────────────────────────────────────────────

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Build a control-plane JWT for the given user (aud = "deployment").
fn make_control_jwt(issuer: &JwtIssuer, user_id: &str) -> String {
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

fn auth_header(jwt: &str) -> (http::HeaderName, HeaderValue) {
    (
        http::header::AUTHORIZATION,
        format!("Bearer {jwt}").parse::<HeaderValue>().unwrap(),
    )
}

/// Shared test environment.
///
/// Returns (TestServer, Arc<JwtIssuer>, admin_user_id, write_user_id,
///          other_user_id, Arc<Mutex<StorageAdapter>>, UserRoleStore).
#[allow(clippy::type_complexity)]
fn build_test_env(
    mem: MemoryStorageAdapter,
) -> (
    TestServer,
    Arc<JwtIssuer>,
    String,
    String,
    String,
    Arc<Mutex<Box<dyn axon_storage::StorageAdapter + Send + Sync>>>,
    UserRoleStore,
) {
    let issuer = Arc::new(JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string()));

    let admin_user_id = Uuid::now_v7().to_string();
    let write_user_id = Uuid::now_v7().to_string();
    let other_user_id = Uuid::now_v7().to_string();

    let user_roles = UserRoleStore::default();
    // admin_user_id is a deployment-level admin.
    user_roles.set_cached(admin_user_id.clone(), axon_server::auth::Role::Admin);

    let storage: Box<dyn axon_storage::StorageAdapter + Send + Sync> = Box::new(mem);
    let storage = Arc::new(Mutex::new(storage));

    let cp_db = ControlPlaneDb::open_in_memory().expect("open in-memory control-plane db");
    let state = ControlPlaneState::new(
        Arc::new(tokio::sync::Mutex::new(cp_db)),
        std::path::PathBuf::from("/tmp/axon-test-credentials"),
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
    (server, issuer, admin_user_id, write_user_id, other_user_id, storage, user_roles)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// issue_with_deployment_admin — POST with admin JWT, verify response has signed
/// JWT + jti; verify that the credential issuance can be listed.
#[tokio::test]
async fn issue_with_deployment_admin() {
    let mut mem = MemoryStorageAdapter::default();
    let target_user_id = Uuid::now_v7().to_string();
    // Pre-insert target user as a tenant admin so ceiling check passes.
    mem.insert_tenant_member(TenantMember {
        tenant_id: TenantId::new(TENANT_ID),
        user_id: UserId::new(&target_user_id),
        role: TenantRole::Admin,
    });

    let (server, issuer, admin_uid, _, _, storage, _) = build_test_env(mem);

    let admin_jwt = make_control_jwt(&issuer, &admin_uid);
    let (hname, hval) = auth_header(&admin_jwt);

    let resp = server
        .post(&format!("/control/tenants/{TENANT_ID}/credentials"))
        .add_header(hname.clone(), hval.clone())
        .json(&json!({
            "target_user": target_user_id,
            "grants": { "databases": [{ "name": "mydb", "ops": ["read"] }] },
            "ttl_seconds": 3600
        }))
        .await;

    resp.assert_status(StatusCode::CREATED);
    let body: Value = resp.json();
    assert!(body["jwt"].is_string(), "response should have jwt field");
    assert!(body["jti"].is_string(), "response should have jti field");
    assert!(body["expires_at"].is_u64(), "response should have expires_at field");

    let jti = body["jti"].as_str().unwrap().to_string();

    // Verify issuance row is present via GET.
    let list_resp = server
        .get(&format!("/control/tenants/{TENANT_ID}/credentials"))
        .add_header(hname, hval)
        .await;
    list_resp.assert_status(StatusCode::OK);
    let list_body: Value = list_resp.json();
    let creds = list_body["credentials"].as_array().expect("credentials array");
    assert_eq!(creds.len(), 1, "one credential should be listed");
    assert_eq!(creds[0]["jti"].as_str().unwrap(), jti, "jti should match");

    let _ = storage; // keep storage alive
}

/// issue_over_scoped_returns_403 — caller's role is Write; tries to issue admin
/// grants → 403 grants_exceed_role.
#[tokio::test]
async fn issue_over_scoped_returns_403() {
    let mut mem = MemoryStorageAdapter::default();
    let write_uid = Uuid::now_v7().to_string();
    // write_uid is a Write member of the tenant.
    mem.insert_tenant_member(TenantMember {
        tenant_id: TenantId::new(TENANT_ID),
        user_id: UserId::new(&write_uid),
        role: TenantRole::Write,
    });

    let (server, issuer, _, _, _, storage, _) = build_test_env(mem);

    // self-issue with admin grants — should be rejected because Write cannot delegate Admin.
    let caller_jwt = make_control_jwt(&issuer, &write_uid);
    let (hname, hval) = auth_header(&caller_jwt);

    let resp = server
        .post(&format!("/control/tenants/{TENANT_ID}/credentials"))
        .add_header(hname, hval)
        .json(&json!({
            "target_user": write_uid,
            "grants": { "databases": [{ "name": "mydb", "ops": ["admin"] }] },
            "ttl_seconds": 3600
        }))
        .await;

    resp.assert_status(StatusCode::FORBIDDEN);
    let body: Value = resp.json();
    assert_eq!(
        body["code"].as_str().unwrap_or(""),
        "grants_exceed_role",
        "error code should be grants_exceed_role, got: {body}"
    );

    let _ = storage;
}

/// self_issue_within_ceiling — caller issues to themselves with grants ⊆ their
/// own role; succeeds.
#[tokio::test]
async fn self_issue_within_ceiling() {
    let mut mem = MemoryStorageAdapter::default();
    let write_uid = Uuid::now_v7().to_string();
    mem.insert_tenant_member(TenantMember {
        tenant_id: TenantId::new(TENANT_ID),
        user_id: UserId::new(&write_uid),
        role: TenantRole::Write,
    });

    let (server, issuer, _, _, _, storage, _) = build_test_env(mem);

    let caller_jwt = make_control_jwt(&issuer, &write_uid);
    let (hname, hval) = auth_header(&caller_jwt);

    // Write user self-issues with Read+Write grants (within ceiling).
    let resp = server
        .post(&format!("/control/tenants/{TENANT_ID}/credentials"))
        .add_header(hname, hval)
        .json(&json!({
            "target_user": write_uid,
            "grants": { "databases": [{ "name": "mydb", "ops": ["read", "write"] }] },
            "ttl_seconds": 1800
        }))
        .await;

    resp.assert_status(StatusCode::CREATED);
    let body: Value = resp.json();
    assert!(body["jwt"].is_string(), "should receive a signed JWT");
    assert!(body["jti"].is_string(), "should receive a jti");

    let _ = storage;
}

/// list_credentials_admin — admin sees all credentials in tenant.
#[tokio::test]
async fn list_credentials_admin() {
    let mut mem = MemoryStorageAdapter::default();
    let user_a = Uuid::now_v7().to_string();
    let user_b = Uuid::now_v7().to_string();
    mem.insert_tenant_member(TenantMember {
        tenant_id: TenantId::new(TENANT_ID),
        user_id: UserId::new(&user_a),
        role: TenantRole::Admin,
    });
    mem.insert_tenant_member(TenantMember {
        tenant_id: TenantId::new(TENANT_ID),
        user_id: UserId::new(&user_b),
        role: TenantRole::Write,
    });

    let (server, issuer, admin_uid, _, _, storage, _) = build_test_env(mem);
    let admin_jwt = make_control_jwt(&issuer, &admin_uid);
    let (hname, hval) = auth_header(&admin_jwt);

    // Issue one credential for user_a and one for user_b.
    for uid in [&user_a, &user_b] {
        let resp = server
            .post(&format!("/control/tenants/{TENANT_ID}/credentials"))
            .add_header(hname.clone(), hval.clone())
            .json(&json!({
                "target_user": uid,
                "grants": { "databases": [{ "name": "db1", "ops": ["read"] }] },
                "ttl_seconds": 3600
            }))
            .await;
        resp.assert_status(StatusCode::CREATED);
    }

    // Admin lists all credentials — should see 2.
    let list_resp = server
        .get(&format!("/control/tenants/{TENANT_ID}/credentials"))
        .add_header(hname, hval)
        .await;
    list_resp.assert_status(StatusCode::OK);
    let list_body: Value = list_resp.json();
    let creds = list_body["credentials"].as_array().expect("credentials array");
    assert_eq!(creds.len(), 2, "admin should see all 2 credentials, got: {}", creds.len());

    let _ = storage;
}

/// list_credentials_self — non-admin sees only their own credentials.
#[tokio::test]
async fn list_credentials_self() {
    let mut mem = MemoryStorageAdapter::default();
    let user_a = Uuid::now_v7().to_string();
    let user_b = Uuid::now_v7().to_string();
    mem.insert_tenant_member(TenantMember {
        tenant_id: TenantId::new(TENANT_ID),
        user_id: UserId::new(&user_a),
        role: TenantRole::Write,
    });
    mem.insert_tenant_member(TenantMember {
        tenant_id: TenantId::new(TENANT_ID),
        user_id: UserId::new(&user_b),
        role: TenantRole::Write,
    });

    let (server, issuer, admin_uid, _, _, storage, _) = build_test_env(mem);

    // Admin issues one credential for user_a and one for user_b.
    let admin_jwt = make_control_jwt(&issuer, &admin_uid);
    let (admin_hname, admin_hval) = auth_header(&admin_jwt);
    for uid in [&user_a, &user_b] {
        let resp = server
            .post(&format!("/control/tenants/{TENANT_ID}/credentials"))
            .add_header(admin_hname.clone(), admin_hval.clone())
            .json(&json!({
                "target_user": uid,
                "grants": { "databases": [{ "name": "db1", "ops": ["read"] }] },
                "ttl_seconds": 3600
            }))
            .await;
        resp.assert_status(StatusCode::CREATED);
    }

    // user_a lists credentials — should see only their own.
    let user_a_jwt = make_control_jwt(&issuer, &user_a);
    let (ua_hname, ua_hval) = auth_header(&user_a_jwt);
    let list_resp = server
        .get(&format!("/control/tenants/{TENANT_ID}/credentials"))
        .add_header(ua_hname, ua_hval)
        .await;
    list_resp.assert_status(StatusCode::OK);
    let list_body: Value = list_resp.json();
    let creds = list_body["credentials"].as_array().expect("credentials array");
    assert_eq!(creds.len(), 1, "user_a should see only their own credential");
    assert_eq!(
        creds[0]["user_id"].as_str().unwrap(),
        user_a,
        "the credential should belong to user_a"
    );

    let _ = storage;
}

/// list_never_returns_signed_jwt — assert that the list response does not
/// contain a "jwt" field at any level.
#[tokio::test]
async fn list_never_returns_signed_jwt() {
    let mut mem = MemoryStorageAdapter::default();
    let target_uid = Uuid::now_v7().to_string();
    mem.insert_tenant_member(TenantMember {
        tenant_id: TenantId::new(TENANT_ID),
        user_id: UserId::new(&target_uid),
        role: TenantRole::Admin,
    });

    let (server, issuer, admin_uid, _, _, storage, _) = build_test_env(mem);
    let admin_jwt = make_control_jwt(&issuer, &admin_uid);
    let (hname, hval) = auth_header(&admin_jwt);

    // Issue a credential.
    let issue_resp = server
        .post(&format!("/control/tenants/{TENANT_ID}/credentials"))
        .add_header(hname.clone(), hval.clone())
        .json(&json!({
            "target_user": target_uid,
            "grants": { "databases": [{ "name": "db1", "ops": ["read"] }] },
            "ttl_seconds": 3600
        }))
        .await;
    issue_resp.assert_status(StatusCode::CREATED);

    // List credentials.
    let list_resp = server
        .get(&format!("/control/tenants/{TENANT_ID}/credentials"))
        .add_header(hname, hval)
        .await;
    list_resp.assert_status(StatusCode::OK);

    let raw_body = list_resp.text();
    // The "jwt" key must not appear in the list response.
    assert!(
        !raw_body.contains("\"jwt\""),
        "list response must not contain a 'jwt' field, got: {raw_body}"
    );

    let _ = storage;
}

/// revoke_by_admin — admin revokes a jti; subsequent list shows revoked=true.
#[tokio::test]
async fn revoke_by_admin() {
    let mut mem = MemoryStorageAdapter::default();
    let target_uid = Uuid::now_v7().to_string();
    mem.insert_tenant_member(TenantMember {
        tenant_id: TenantId::new(TENANT_ID),
        user_id: UserId::new(&target_uid),
        role: TenantRole::Admin,
    });

    let (server, issuer, admin_uid, _, _, storage, _) = build_test_env(mem);
    let admin_jwt = make_control_jwt(&issuer, &admin_uid);
    let (hname, hval) = auth_header(&admin_jwt);

    // Issue a credential.
    let issue_resp = server
        .post(&format!("/control/tenants/{TENANT_ID}/credentials"))
        .add_header(hname.clone(), hval.clone())
        .json(&json!({
            "target_user": target_uid,
            "grants": { "databases": [{ "name": "db1", "ops": ["read"] }] },
            "ttl_seconds": 3600
        }))
        .await;
    issue_resp.assert_status(StatusCode::CREATED);
    let jti = issue_resp.json::<Value>()["jti"].as_str().unwrap().to_string();

    // Admin revokes the credential.
    let revoke_resp = server
        .delete(&format!("/control/tenants/{TENANT_ID}/credentials/{jti}"))
        .add_header(hname.clone(), hval.clone())
        .await;
    revoke_resp.assert_status(StatusCode::NO_CONTENT);

    // The list should show the credential as revoked.
    let list_resp = server
        .get(&format!("/control/tenants/{TENANT_ID}/credentials"))
        .add_header(hname, hval)
        .await;
    list_resp.assert_status(StatusCode::OK);
    let list_body: Value = list_resp.json();
    let creds = list_body["credentials"].as_array().unwrap();
    assert_eq!(creds.len(), 1, "should have one credential");
    assert!(
        creds[0]["revoked"].as_bool().unwrap(),
        "credential should be marked revoked"
    );

    let _ = storage;
}

/// revoke_by_owner — credential owner revokes their own; succeeds.
#[tokio::test]
async fn revoke_by_owner() {
    let mut mem = MemoryStorageAdapter::default();
    let write_uid = Uuid::now_v7().to_string();
    mem.insert_tenant_member(TenantMember {
        tenant_id: TenantId::new(TENANT_ID),
        user_id: UserId::new(&write_uid),
        role: TenantRole::Write,
    });

    let (server, issuer, admin_uid, _, _, storage, _) = build_test_env(mem);

    // Admin issues a credential for write_uid.
    let admin_jwt = make_control_jwt(&issuer, &admin_uid);
    let (admin_hname, admin_hval) = auth_header(&admin_jwt);
    let issue_resp = server
        .post(&format!("/control/tenants/{TENANT_ID}/credentials"))
        .add_header(admin_hname, admin_hval)
        .json(&json!({
            "target_user": write_uid,
            "grants": { "databases": [{ "name": "db1", "ops": ["read"] }] },
            "ttl_seconds": 3600
        }))
        .await;
    issue_resp.assert_status(StatusCode::CREATED);
    let jti = issue_resp.json::<Value>()["jti"].as_str().unwrap().to_string();

    // Owner revokes their own credential.
    let owner_jwt = make_control_jwt(&issuer, &write_uid);
    let (owner_hname, owner_hval) = auth_header(&owner_jwt);
    let revoke_resp = server
        .delete(&format!("/control/tenants/{TENANT_ID}/credentials/{jti}"))
        .add_header(owner_hname, owner_hval)
        .await;
    revoke_resp.assert_status(StatusCode::NO_CONTENT);

    let _ = storage;
}

/// revoke_by_unrelated_user_returns_403 — a different user tries to revoke
/// someone else's credential; receives 403.
#[tokio::test]
async fn revoke_by_unrelated_user_returns_403() {
    let mut mem = MemoryStorageAdapter::default();
    let owner_uid = Uuid::now_v7().to_string();
    let other_uid = Uuid::now_v7().to_string();
    mem.insert_tenant_member(TenantMember {
        tenant_id: TenantId::new(TENANT_ID),
        user_id: UserId::new(&owner_uid),
        role: TenantRole::Write,
    });
    mem.insert_tenant_member(TenantMember {
        tenant_id: TenantId::new(TENANT_ID),
        user_id: UserId::new(&other_uid),
        role: TenantRole::Write,
    });

    let (server, issuer, admin_uid, _, _, storage, _) = build_test_env(mem);

    // Admin issues a credential for owner_uid.
    let admin_jwt = make_control_jwt(&issuer, &admin_uid);
    let (admin_hname, admin_hval) = auth_header(&admin_jwt);
    let issue_resp = server
        .post(&format!("/control/tenants/{TENANT_ID}/credentials"))
        .add_header(admin_hname, admin_hval)
        .json(&json!({
            "target_user": owner_uid,
            "grants": { "databases": [{ "name": "db1", "ops": ["read"] }] },
            "ttl_seconds": 3600
        }))
        .await;
    issue_resp.assert_status(StatusCode::CREATED);
    let jti = issue_resp.json::<Value>()["jti"].as_str().unwrap().to_string();

    // other_uid tries to revoke owner_uid's credential — should be 403.
    let other_jwt = make_control_jwt(&issuer, &other_uid);
    let (other_hname, other_hval) = auth_header(&other_jwt);
    let revoke_resp = server
        .delete(&format!("/control/tenants/{TENANT_ID}/credentials/{jti}"))
        .add_header(other_hname, other_hval)
        .await;
    revoke_resp.assert_status(StatusCode::FORBIDDEN);

    let _ = storage;
}

/// revoked_credential_rejected_on_next_request — issue a credential, revoke it,
/// then use it via B1's jwt_verify_layer; assert credential_revoked rejection.
#[tokio::test]
async fn revoked_credential_rejected_on_next_request() {
    let tenant = "acme-cred-revoke";
    let database = "orders";
    let protected_path = format!("/tenants/{tenant}/databases/{database}/ping");

    // Set up a user who is a tenant member with write access.
    let mut mem = MemoryStorageAdapter::default();
    let user_uid = Uuid::now_v7().to_string();
    mem.insert_user(User {
        id: UserId::new(&user_uid),
        display_name: "Test User".to_string(),
        email: None,
        created_at_ms: 0,
        suspended_at_ms: None,
    });
    mem.insert_tenant_member(TenantMember {
        tenant_id: TenantId::new(tenant),
        user_id: UserId::new(&user_uid),
        role: TenantRole::Write,
    });

    // Also need the user in the control plane tenant for credential issuance.
    // The control plane tenant check uses `get_tenant_member(TENANT_ID, user_uid)`.
    // We reuse the same "tenant" for both control plane and data plane here.
    // Actually the control plane tenant is separate from the data-plane tenant,
    // so we also add the user to the control plane tenant.
    mem.insert_tenant_member(TenantMember {
        tenant_id: TenantId::new(tenant),
        user_id: UserId::new(&user_uid),
        role: TenantRole::Write,
    });

    let issuer = Arc::new(JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string()));
    let admin_uid = Uuid::now_v7().to_string();

    let user_roles = UserRoleStore::default();
    user_roles.set_cached(admin_uid.clone(), axon_server::auth::Role::Admin);

    let storage: Box<dyn axon_storage::StorageAdapter + Send + Sync> = Box::new(mem);
    let storage_arc = Arc::new(Mutex::new(storage));

    // Build the control plane server.
    let cp_db = ControlPlaneDb::open_in_memory().expect("open in-memory control-plane db");
    let state = ControlPlaneState::new(
        Arc::new(tokio::sync::Mutex::new(cp_db)),
        std::path::PathBuf::from("/tmp/axon-test-cred-revoke"),
        user_roles.clone(),
        CorsStore::default(),
    )
    .with_storage(storage_arc.clone())
    .with_jwt_issuer(issuer.clone());

    let peer: SocketAddr = "127.0.0.1:12345".parse().unwrap();
    let auth = AuthContext::no_auth();
    let cp_app = Router::new()
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
    let cp_server = TestServer::new(cp_app);

    // Step 1: Issue a data-plane credential for the user.
    let admin_jwt = make_control_jwt(&issuer, &admin_uid);
    let (admin_hname, admin_hval) = auth_header(&admin_jwt);
    let issue_resp = cp_server
        .post(&format!("/control/tenants/{tenant}/credentials"))
        .add_header(admin_hname, admin_hval)
        .json(&json!({
            "target_user": user_uid,
            "grants": {
                "databases": [{
                    "name": database,
                    "ops": ["read", "write"]
                }]
            },
            "ttl_seconds": 3600
        }))
        .await;
    issue_resp.assert_status(StatusCode::CREATED);
    let issue_body: Value = issue_resp.json();
    let signed_jwt = issue_body["jwt"].as_str().unwrap().to_string();
    let jti = issue_body["jti"].as_str().unwrap().to_string();

    // Step 2: Build a data-plane app using B1's jwt_verify_layer.
    let pipeline_state = Arc::new(AuthPipelineState {
        issuer: issuer.clone(),
        revocation_cache: Arc::new(InMemoryRevocationCache::new()),
        storage: storage_arc.clone(),
    });
    let data_app = Router::new()
        .route(&protected_path, get(|| async { StatusCode::OK }))
        .layer(axum::middleware::from_fn_with_state(
            pipeline_state,
            jwt_verify_layer,
        ));

    // Step 3: Verify the credential works BEFORE revocation.
    let req_before = Request::builder()
        .uri(&protected_path)
        .method("GET")
        .header("Authorization", format!("Bearer {signed_jwt}"))
        .body(Body::empty())
        .unwrap();
    let (status_before, _) = send_oneshot(data_app.clone(), req_before).await;
    assert_eq!(
        status_before,
        StatusCode::OK,
        "credential should be accepted before revocation"
    );

    // Step 4: Revoke the credential via the control plane.
    let admin_jwt2 = make_control_jwt(&issuer, &admin_uid);
    let (admin_hname2, admin_hval2) = auth_header(&admin_jwt2);
    let revoke_resp = cp_server
        .delete(&format!("/control/tenants/{tenant}/credentials/{jti}"))
        .add_header(admin_hname2, admin_hval2)
        .await;
    revoke_resp.assert_status(StatusCode::NO_CONTENT);

    // Step 5: Verify the credential is rejected AFTER revocation.
    let req_after = Request::builder()
        .uri(&protected_path)
        .method("GET")
        .header("Authorization", format!("Bearer {signed_jwt}"))
        .body(Body::empty())
        .unwrap();
    let (status_after, body_after) = send_oneshot(data_app, req_after).await;
    assert_eq!(
        status_after,
        StatusCode::UNAUTHORIZED,
        "revoked credential should be rejected"
    );
    assert_eq!(
        body_after["error"]["code"].as_str().unwrap_or(""),
        "credential_revoked",
        "error code should be credential_revoked, got: {body_after}"
    );
}

/// Helper: call a Router::oneshot and decode the response body as JSON.
async fn send_oneshot(app: Router, req: Request<Body>) -> (StatusCode, Value) {
    let resp = app.oneshot(req).await.expect("service should not fail");
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
        .await
        .unwrap_or_default();
    let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}
