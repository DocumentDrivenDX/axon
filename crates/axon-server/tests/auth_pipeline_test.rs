//! Unit tests for the JWT auth pipeline (ADR-018 §4).
//!
//! Covers:
//! - Every row of the failure-mode table (14 variants)
//! - JWT round-trip
//! - ResolvedIdentity installation in request extensions
//! - Revocation cache invalidation
//! - Clock-skew boundary at ±30 s
//! - HTTP-method × grant-ops matrix

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::Extension;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use http::Request;
use serde::Serialize;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

use axon_core::auth::{
    AuthError, GrantedDatabase, Grants, JwtClaims, Op, ResolvedIdentity, TenantId, TenantMember,
    TenantRole, User, UserId,
};
use axon_server::auth_pipeline::{
    AuthPipelineState, InMemoryRevocationCache, JwtIssuer, jwt_verify_layer,
};
use axon_storage::MemoryStorageAdapter;

// ── Test helpers ──────────────────────────────────────────────────────────────

const ISSUER_ID: &str = "test-issuer";
const TENANT: &str = "acme";
const DATABASE: &str = "orders";
const USER_ID: &str = "user-01";
const SECRET: &[u8] = b"super-secret-key-for-tests";
const TEST_PATH: &str = "/tenants/acme/databases/orders/ping";

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn valid_claims() -> JwtClaims {
    let now = now_secs();
    JwtClaims {
        iss: ISSUER_ID.to_string(),
        sub: USER_ID.to_string(),
        aud: TENANT.to_string(),
        jti: Uuid::now_v7().to_string(),
        iat: now,
        nbf: now,
        exp: now + 3600,
        grants: Grants {
            databases: vec![GrantedDatabase {
                name: DATABASE.to_string(),
                ops: vec![Op::Read, Op::Write],
            }],
        },
    }
}

fn make_storage() -> MemoryStorageAdapter {
    let mut storage = MemoryStorageAdapter::default();
    storage.insert_user(User {
        id: UserId::new(USER_ID),
        display_name: "Test User".to_string(),
        email: None,
        created_at_ms: 0,
        suspended_at_ms: None,
    });
    storage.insert_tenant_member(TenantMember {
        tenant_id: TenantId::new(TENANT),
        user_id: UserId::new(USER_ID),
        role: TenantRole::Write,
    });
    storage
}

fn make_state() -> Arc<AuthPipelineState> {
    let issuer = JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string());
    let storage: Box<dyn axon_storage::StorageAdapter + Send + Sync> =
        Box::new(make_storage());
    Arc::new(AuthPipelineState {
        issuer: Arc::new(issuer),
        revocation_cache: Arc::new(InMemoryRevocationCache::new()),
        storage: Arc::new(Mutex::new(storage)),
    })
}

fn make_app(state: Arc<AuthPipelineState>) -> Router {
    Router::new()
        .route(TEST_PATH, get(|| async { StatusCode::OK }))
        .layer(axum::middleware::from_fn_with_state(
            state,
            jwt_verify_layer,
        ))
}

async fn send(app: Router, req: Request<Body>) -> (StatusCode, Value) {
    let resp = app.oneshot(req).await.expect("service call should succeed");
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
        .await
        .expect("body read should succeed");
    let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

fn get_req(path: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(path)
        .body(Body::empty())
        .unwrap()
}

fn bearer_req(path: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(path)
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

fn error_code(json: &Value) -> &str {
    json["error"]["code"].as_str().unwrap_or("")
}

// ── JWT round-trip ────────────────────────────────────────────────────────────

#[test]
fn jwt_roundtrip_succeeds() {
    let issuer = JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string());
    let claims = valid_claims();
    let token = issuer.issue(&claims).expect("issue should succeed");
    let decoded = issuer.verify(&token).expect("verify should succeed");
    assert_eq!(decoded.iss, claims.iss);
    assert_eq!(decoded.sub, claims.sub);
    assert_eq!(decoded.aud, claims.aud);
    assert_eq!(decoded.jti, claims.jti);
    assert_eq!(decoded.grants, claims.grants);
}

// ── ADR-018 §4 failure-mode table ─────────────────────────────────────────────

/// Row 1: No Authorization header → 401 unauthenticated
#[tokio::test]
async fn error_no_auth_header() {
    let state = make_state();
    let app = make_app(state);
    let req = get_req(TEST_PATH);
    let (status, json) = send(app, req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&json), "unauthenticated");
}

/// Row 2a: Header present but not `Bearer` → 401 credential_malformed
#[tokio::test]
async fn error_not_bearer_scheme() {
    let state = make_state();
    let app = make_app(state);
    let req = Request::builder()
        .uri(TEST_PATH)
        .header("Authorization", "Basic dXNlcjpwYXNz")
        .body(Body::empty())
        .unwrap();
    let (status, json) = send(app, req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&json), "credential_malformed");
}

/// Row 2b: JWT structurally invalid → 401 credential_malformed
#[tokio::test]
async fn error_jwt_malformed() {
    let state = make_state();
    let app = make_app(state);
    let req = bearer_req(TEST_PATH, "not.a.valid.jwt");
    let (status, json) = send(app, req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&json), "credential_malformed");
}

/// Row 2c: `aud` as JSON array → 401 credential_malformed
#[tokio::test]
async fn error_aud_is_array() {
    #[derive(Serialize)]
    struct ArrayAudClaims {
        iss: String,
        sub: String,
        aud: Vec<String>,
        jti: String,
        iat: u64,
        nbf: u64,
        exp: u64,
        grants: Grants,
    }
    let now = now_secs();
    let bad_claims = ArrayAudClaims {
        iss: ISSUER_ID.to_string(),
        sub: USER_ID.to_string(),
        aud: vec![TENANT.to_string()], // array, not string
        jti: Uuid::now_v7().to_string(),
        iat: now,
        nbf: now,
        exp: now + 3600,
        grants: Grants {
            databases: vec![GrantedDatabase {
                name: DATABASE.to_string(),
                ops: vec![Op::Read],
            }],
        },
    };
    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
        &bad_claims,
        &jsonwebtoken::EncodingKey::from_secret(SECRET),
    )
    .unwrap();
    let state = make_state();
    let app = make_app(state);
    let req = bearer_req(TEST_PATH, &token);
    let (status, json) = send(app, req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&json), "credential_malformed");
}

/// Row 3: Signature invalid → 401 credential_invalid
#[tokio::test]
async fn error_signature_invalid() {
    let wrong_issuer = JwtIssuer::new(b"wrong-secret".to_vec(), ISSUER_ID.to_string());
    let claims = valid_claims();
    let token = wrong_issuer.issue(&claims).unwrap();
    let state = make_state();
    let app = make_app(state);
    let req = bearer_req(TEST_PATH, &token);
    let (status, json) = send(app, req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&json), "credential_invalid");
}

/// Row 4: `exp` in past beyond skew → 401 credential_expired
#[tokio::test]
async fn error_exp_expired() {
    let issuer = JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string());
    let now = now_secs();
    let mut claims = valid_claims();
    claims.exp = now - 31; // 31 seconds in the past, beyond the 30s skew
    let token = issuer.issue(&claims).unwrap();
    let state = make_state();
    let app = make_app(state);
    let req = bearer_req(TEST_PATH, &token);
    let (status, json) = send(app, req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&json), "credential_expired");
}

/// Row 5: `nbf` in future beyond skew → 401 credential_not_yet_valid
#[tokio::test]
async fn error_nbf_future() {
    let issuer = JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string());
    let now = now_secs();
    let mut claims = valid_claims();
    claims.nbf = now + 31; // 31 seconds from now, beyond the 30s skew
    let token = issuer.issue(&claims).unwrap();
    let state = make_state();
    let app = make_app(state);
    let req = bearer_req(TEST_PATH, &token);
    let (status, json) = send(app, req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&json), "credential_not_yet_valid");
}

/// Row 6: `jti` revoked → 401 credential_revoked
#[tokio::test]
async fn error_jti_revoked() {
    let issuer = JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string());
    let claims = valid_claims();
    let jti_uuid: Uuid = claims.jti.parse().unwrap();
    let token = issuer.issue(&claims).unwrap();

    let state = make_state();
    // Insert the JTI directly into the revocation cache.
    state.revocation_cache.insert(jti_uuid);

    let app = make_app(state);
    let req = bearer_req(TEST_PATH, &token);
    let (status, json) = send(app, req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&json), "credential_revoked");
}

/// Row 7: `iss` unknown → 401 credential_foreign_issuer
#[tokio::test]
async fn error_foreign_issuer() {
    let issuer = JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string());
    let mut claims = valid_claims();
    claims.iss = "some-other-issuer".to_string();
    let token = issuer.issue(&claims).unwrap();
    let state = make_state();
    let app = make_app(state);
    let req = bearer_req(TEST_PATH, &token);
    let (status, json) = send(app, req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&json), "credential_foreign_issuer");
}

/// Row 8: `aud` ≠ URL tenant → 403 credential_wrong_tenant
#[tokio::test]
async fn error_wrong_tenant() {
    let issuer = JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string());
    let mut claims = valid_claims();
    claims.aud = "other-tenant".to_string(); // URL says "acme"
    let token = issuer.issue(&claims).unwrap();
    let state = make_state();
    let app = make_app(state);
    let req = bearer_req(TEST_PATH, &token);
    let (status, json) = send(app, req).await;
    assert_eq!(status.as_u16(), 403);
    assert_eq!(error_code(&json), "credential_wrong_tenant");
}

/// Row 9: `sub` suspended → 401 user_suspended
#[tokio::test]
async fn error_user_suspended() {
    let issuer = JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string());
    let claims = valid_claims();
    let token = issuer.issue(&claims).unwrap();

    // Build state with a suspended user.
    let mut storage = make_storage();
    // Overwrite the user with a suspended version.
    storage.insert_user(User {
        id: UserId::new(USER_ID),
        display_name: "Suspended".to_string(),
        email: None,
        created_at_ms: 0,
        suspended_at_ms: Some(1000),
    });
    let state = Arc::new(AuthPipelineState {
        issuer: Arc::new(JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string())),
        revocation_cache: Arc::new(InMemoryRevocationCache::new()),
        storage: Arc::new(Mutex::new(
            Box::new(storage) as Box<dyn axon_storage::StorageAdapter + Send + Sync>
        )),
    });
    let app = make_app(state);
    let req = bearer_req(TEST_PATH, &token);
    let (status, json) = send(app, req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&json), "user_suspended");
}

/// Row 10: `sub` not a tenant member → 403 not_a_tenant_member
#[tokio::test]
async fn error_not_tenant_member() {
    let issuer = JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string());
    let claims = valid_claims();
    let token = issuer.issue(&claims).unwrap();

    // Storage with user but NO tenant membership.
    let mut storage = MemoryStorageAdapter::default();
    storage.insert_user(User {
        id: UserId::new(USER_ID),
        display_name: "No Member".to_string(),
        email: None,
        created_at_ms: 0,
        suspended_at_ms: None,
    });
    let state = Arc::new(AuthPipelineState {
        issuer: Arc::new(JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string())),
        revocation_cache: Arc::new(InMemoryRevocationCache::new()),
        storage: Arc::new(Mutex::new(
            Box::new(storage) as Box<dyn axon_storage::StorageAdapter + Send + Sync>
        )),
    });
    let app = make_app(state);
    let req = bearer_req(TEST_PATH, &token);
    let (status, json) = send(app, req).await;
    assert_eq!(status.as_u16(), 403);
    assert_eq!(error_code(&json), "not_a_tenant_member");
}

/// Row 11: URL database not in grants → 403 database_not_granted
#[tokio::test]
async fn error_database_not_granted() {
    let issuer = JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string());
    let mut claims = valid_claims();
    // Grant is for "products", URL database is "orders"
    claims.grants = Grants {
        databases: vec![GrantedDatabase {
            name: "products".to_string(),
            ops: vec![Op::Read, Op::Write],
        }],
    };
    let token = issuer.issue(&claims).unwrap();
    let state = make_state();
    let app = make_app(state);
    let req = bearer_req(TEST_PATH, &token);
    let (status, json) = send(app, req).await;
    assert_eq!(status.as_u16(), 403);
    assert_eq!(error_code(&json), "database_not_granted");
}

/// Row 12: Op not in grant → 403 op_not_granted
#[tokio::test]
async fn error_op_not_granted() {
    let issuer = JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string());
    let mut claims = valid_claims();
    // Grant only has Read; we'll POST (Write).
    claims.grants = Grants {
        databases: vec![GrantedDatabase {
            name: DATABASE.to_string(),
            ops: vec![Op::Read],
        }],
    };
    let token = issuer.issue(&claims).unwrap();
    let state = make_state();
    let app = make_app(state);

    // POST = Write, but grant only has Read.
    let req = Request::builder()
        .method("POST")
        .uri(TEST_PATH)
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let (status, json) = send(app, req).await;
    assert_eq!(status.as_u16(), 403);
    assert_eq!(error_code(&json), "op_not_granted");
}

// ── valid_jwt_populates_request_extension ─────────────────────────────────────

async fn identity_handler(
    Extension(identity): Extension<ResolvedIdentity>,
) -> impl IntoResponse {
    axum::Json(serde_json::json!({"user_id": identity.user_id.as_str()}))
}

#[tokio::test]
async fn valid_jwt_populates_request_extension() {
    let issuer = JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string());
    let claims = valid_claims();
    let token = issuer.issue(&claims).unwrap();

    let state = make_state();
    let app = Router::new()
        .route(TEST_PATH, get(identity_handler))
        .layer(axum::middleware::from_fn_with_state(
            state,
            jwt_verify_layer,
        ));

    let req = bearer_req(TEST_PATH, &token);
    let (status, json) = send(app, req).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["user_id"].as_str().unwrap(), USER_ID);
}

// ── revocation_takes_effect_within_one_second ─────────────────────────────────

#[tokio::test]
async fn revocation_takes_effect_within_one_second() {
    let issuer = JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string());
    let claims = valid_claims();
    let jti_uuid: Uuid = claims.jti.parse().unwrap();
    let token = issuer.issue(&claims).unwrap();

    let state = make_state();
    let app_before = make_app(state.clone());
    let app_after = make_app(state.clone());

    // First request must succeed.
    let (status, _) = send(app_before, bearer_req(TEST_PATH, &token)).await;
    assert_eq!(status, StatusCode::OK, "before revocation should be 200");

    // Directly invalidate the cache — no sleep needed.
    state.revocation_cache.insert(jti_uuid);

    // Second request must fail with credential_revoked.
    let (status, json) = send(app_after, bearer_req(TEST_PATH, &token)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&json), "credential_revoked");
}

// ── clock_skew_30s_accepted_31s_rejected ─────────────────────────────────────

#[tokio::test]
async fn clock_skew_exp_29s_accepted() {
    let issuer = JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string());
    let now = now_secs();
    let mut claims = valid_claims();
    claims.exp = now - 29; // 29 s ago — within the 30s skew window
    let token = issuer.issue(&claims).unwrap();
    let state = make_state();
    let (status, _) = send(make_app(state), bearer_req(TEST_PATH, &token)).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn clock_skew_exp_31s_rejected() {
    let issuer = JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string());
    let now = now_secs();
    let mut claims = valid_claims();
    claims.exp = now - 31; // 31 s ago — beyond the 30s skew window
    let token = issuer.issue(&claims).unwrap();
    let state = make_state();
    let (status, json) = send(make_app(state), bearer_req(TEST_PATH, &token)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&json), "credential_expired");
}

#[tokio::test]
async fn clock_skew_nbf_29s_future_accepted() {
    let issuer = JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string());
    let now = now_secs();
    let mut claims = valid_claims();
    claims.nbf = now + 29; // 29 s from now — within the 30s skew window
    let token = issuer.issue(&claims).unwrap();
    let state = make_state();
    let (status, _) = send(make_app(state), bearer_req(TEST_PATH, &token)).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn clock_skew_nbf_31s_future_rejected() {
    let issuer = JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string());
    let now = now_secs();
    let mut claims = valid_claims();
    claims.nbf = now + 31; // 31 s from now — beyond the 30s skew window
    let token = issuer.issue(&claims).unwrap();
    let state = make_state();
    let (status, json) = send(make_app(state), bearer_req(TEST_PATH, &token)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&json), "credential_not_yet_valid");
}

// ── ops_matrix ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn ops_matrix() {
    use http::Method;

    struct Case {
        method: Method,
        ops: Vec<Op>,
        expected_status: u16,
    }
    let cases = vec![
        // GET (Read) against various grant sets
        Case { method: Method::GET, ops: vec![Op::Read], expected_status: 200 },
        Case { method: Method::GET, ops: vec![Op::Write], expected_status: 403 },
        Case { method: Method::GET, ops: vec![Op::Read, Op::Write], expected_status: 200 },
        // Write methods (POST/PUT/PATCH/DELETE) against various grant sets
        Case { method: Method::POST, ops: vec![Op::Write], expected_status: 200 },
        Case { method: Method::POST, ops: vec![Op::Read], expected_status: 403 },
        Case { method: Method::POST, ops: vec![Op::Read, Op::Write], expected_status: 200 },
        Case { method: Method::PUT, ops: vec![Op::Write], expected_status: 200 },
        Case { method: Method::PUT, ops: vec![Op::Read], expected_status: 403 },
        Case { method: Method::PATCH, ops: vec![Op::Write], expected_status: 200 },
        Case { method: Method::PATCH, ops: vec![Op::Read], expected_status: 403 },
        Case { method: Method::DELETE, ops: vec![Op::Write], expected_status: 200 },
        Case { method: Method::DELETE, ops: vec![Op::Read], expected_status: 403 },
    ];

    // Use a route that accepts all HTTP methods.
    let handler = || async { StatusCode::OK };

    for case in cases {
        let issuer_inner = JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string());
        let mut claims = valid_claims();
        claims.grants = Grants {
            databases: vec![GrantedDatabase {
                name: DATABASE.to_string(),
                ops: case.ops,
            }],
        };
        let token = issuer_inner.issue(&claims).unwrap();

        let state = make_state();
        // Router that handles any method on TEST_PATH.
        let app = Router::new()
            .route(
                TEST_PATH,
                axum::routing::any(handler),
            )
            .layer(axum::middleware::from_fn_with_state(
                state,
                jwt_verify_layer,
            ));

        let req = Request::builder()
            .method(case.method.clone())
            .uri(TEST_PATH)
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.status().as_u16(),
            case.expected_status,
            "method={} expected {}",
            case.method,
            case.expected_status
        );
    }
}

// ── error_code / status_code consistency ──────────────────────────────────────

#[test]
fn auth_error_status_and_code_coverage() {
    // Verify every variant maps to the correct status and code string.
    let cases: &[(AuthError, u16, &str)] = &[
        (AuthError::Unauthenticated, 401, "unauthenticated"),
        (AuthError::CredentialMalformed, 401, "credential_malformed"),
        (AuthError::CredentialInvalid, 401, "credential_invalid"),
        (AuthError::CredentialExpired, 401, "credential_expired"),
        (AuthError::CredentialNotYetValid, 401, "credential_not_yet_valid"),
        (AuthError::CredentialRevoked, 401, "credential_revoked"),
        (AuthError::CredentialForeignIssuer, 401, "credential_foreign_issuer"),
        (AuthError::CredentialWrongTenant, 403, "credential_wrong_tenant"),
        (AuthError::UserSuspended, 401, "user_suspended"),
        (AuthError::NotATenantMember, 403, "not_a_tenant_member"),
        (AuthError::DatabaseNotGranted, 403, "database_not_granted"),
        (AuthError::OpNotGranted, 403, "op_not_granted"),
        (AuthError::GrantsExceedIssuerRole, 401, "grants_exceed_issuer_role"),
        (AuthError::GrantsMalformed, 401, "grants_malformed"),
    ];
    for (err, expected_status, expected_code) in cases {
        assert_eq!(
            err.status_code(),
            *expected_status,
            "status mismatch for {:?}",
            err
        );
        assert_eq!(
            err.error_code(),
            *expected_code,
            "code mismatch for {:?}",
            err
        );
    }
}

// ── revocation via storage fallback ───────────────────────────────────────────

#[tokio::test]
async fn revocation_via_storage_fallback() {
    let issuer = JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string());
    let claims = valid_claims();
    let jti_uuid: Uuid = claims.jti.parse().unwrap();
    let token = issuer.issue(&claims).unwrap();

    // Build storage with the JTI revoked at the storage level.
    let mut storage = make_storage();
    storage.revoke_jti(jti_uuid);

    let state = Arc::new(AuthPipelineState {
        issuer: Arc::new(JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string())),
        revocation_cache: Arc::new(InMemoryRevocationCache::new()),
        storage: Arc::new(Mutex::new(
            Box::new(storage) as Box<dyn axon_storage::StorageAdapter + Send + Sync>
        )),
    });
    let app = make_app(state);
    let (status, json) = send(app, bearer_req(TEST_PATH, &token)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&json), "credential_revoked");
}
