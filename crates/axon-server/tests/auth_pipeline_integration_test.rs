//! Integration tests for the JWT auth middleware (ADR-018 §4).
//!
//! Builds a minimal axum router with a single protected data-plane route:
//!   GET /tenants/acme/databases/orders/ping
//!
//! Tests the four golden paths required by the bead:
//! 1. No auth header → 401 unauthenticated
//! 2. Valid JWT with correct grants → 200
//! 3. Valid JWT with wrong tenant in aud → 403 credential_wrong_tenant
//! 4. Valid JWT with grants for a different database → 403 database_not_granted

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::StatusCode;
use axum::routing::get;
use axum::Router;
use http::Request;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

use axon_core::auth::{
    GrantedDatabase, Grants, JwtClaims, Op, TenantId, TenantMember, TenantRole, User, UserId,
};
use axon_server::auth_pipeline::{AuthPipelineState, InMemoryRevocationCache, JwtIssuer, jwt_verify_layer};
use axon_storage::MemoryStorageAdapter;

const ISSUER: &str = "integration-issuer";
const TENANT: &str = "acme";
const DATABASE: &str = "orders";
const USER_ID: &str = "u-integration-01";
const SECRET: &[u8] = b"integration-test-secret";
const PROTECTED_PATH: &str = "/tenants/acme/databases/orders/ping";

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn make_state() -> Arc<AuthPipelineState> {
    let mut storage = MemoryStorageAdapter::default();
    storage.insert_user(User {
        id: UserId::new(USER_ID),
        display_name: "Integration User".to_string(),
        email: None,
        created_at_ms: 0,
        suspended_at_ms: None,
    });
    storage.insert_tenant_member(TenantMember {
        tenant_id: TenantId::new(TENANT),
        user_id: UserId::new(USER_ID),
        role: TenantRole::Write,
    });
    Arc::new(AuthPipelineState {
        issuer: Arc::new(JwtIssuer::new(SECRET.to_vec(), ISSUER.to_string())),
        revocation_cache: Arc::new(InMemoryRevocationCache::new()),
        storage: Arc::new(Mutex::new(
            Box::new(storage) as Box<dyn axon_storage::StorageAdapter + Send + Sync>
        )),
    })
}

fn make_app(state: Arc<AuthPipelineState>) -> Router {
    Router::new()
        .route(PROTECTED_PATH, get(|| async { StatusCode::OK }))
        .layer(axum::middleware::from_fn_with_state(
            state,
            jwt_verify_layer,
        ))
}

async fn send(app: Router, req: Request<Body>) -> (StatusCode, Value) {
    let resp = app.oneshot(req).await.expect("service should not fail");
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
        .await
        .unwrap_or_default();
    let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

fn valid_token(issuer: &JwtIssuer, grants: Grants) -> String {
    let now = now_secs();
    let claims = JwtClaims {
        iss: ISSUER.to_string(),
        sub: USER_ID.to_string(),
        aud: TENANT.to_string(),
        jti: Uuid::now_v7().to_string(),
        iat: now,
        nbf: now,
        exp: now + 3600,
        grants,
    };
    issuer.issue(&claims).expect("issue should succeed")
}

fn orders_read_grants() -> Grants {
    Grants {
        databases: vec![GrantedDatabase {
            name: DATABASE.to_string(),
            ops: vec![Op::Read],
        }],
    }
}

// ── Test 1: No auth header → 401 unauthenticated ──────────────────────────────

#[tokio::test]
async fn integration_no_auth_header_returns_401() {
    let state = make_state();
    let app = make_app(state);
    let req = Request::builder()
        .method("GET")
        .uri(PROTECTED_PATH)
        .body(Body::empty())
        .unwrap();
    let (status, json) = send(app, req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        json["error"]["code"].as_str().unwrap_or(""),
        "unauthenticated"
    );
}

// ── Test 2: Valid JWT with read grants → 200 ──────────────────────────────────

#[tokio::test]
async fn integration_valid_jwt_with_read_grant_returns_200() {
    let state = make_state();
    let token = valid_token(&state.issuer, orders_read_grants());
    let app = make_app(state);
    let req = Request::builder()
        .method("GET")
        .uri(PROTECTED_PATH)
        .header("Authorization", bearer(&token))
        .body(Body::empty())
        .unwrap();
    let (status, _) = send(app, req).await;
    assert_eq!(status, StatusCode::OK);
}

// ── Test 3: Wrong tenant in aud → 403 credential_wrong_tenant ─────────────────

#[tokio::test]
async fn integration_wrong_tenant_returns_403() {
    let state = make_state();
    let now = now_secs();
    let claims = JwtClaims {
        iss: ISSUER.to_string(),
        sub: USER_ID.to_string(),
        aud: "different-tenant".to_string(), // URL says "acme"
        jti: Uuid::now_v7().to_string(),
        iat: now,
        nbf: now,
        exp: now + 3600,
        grants: orders_read_grants(),
    };
    let token = state.issuer.issue(&claims).unwrap();
    let app = make_app(state);
    let req = Request::builder()
        .method("GET")
        .uri(PROTECTED_PATH)
        .header("Authorization", bearer(&token))
        .body(Body::empty())
        .unwrap();
    let (status, json) = send(app, req).await;
    assert_eq!(status.as_u16(), 403);
    assert_eq!(
        json["error"]["code"].as_str().unwrap_or(""),
        "credential_wrong_tenant"
    );
}

// ── Test 4: Grants for different database → 403 database_not_granted ──────────

#[tokio::test]
async fn integration_wrong_database_returns_403() {
    let state = make_state();
    let grants = Grants {
        databases: vec![GrantedDatabase {
            name: "invoices".to_string(), // URL says "orders"
            ops: vec![Op::Read],
        }],
    };
    let token = valid_token(&state.issuer, grants);
    let app = make_app(state);
    let req = Request::builder()
        .method("GET")
        .uri(PROTECTED_PATH)
        .header("Authorization", bearer(&token))
        .body(Body::empty())
        .unwrap();
    let (status, json) = send(app, req).await;
    assert_eq!(status.as_u16(), 403);
    assert_eq!(
        json["error"]["code"].as_str().unwrap_or(""),
        "database_not_granted"
    );
}
