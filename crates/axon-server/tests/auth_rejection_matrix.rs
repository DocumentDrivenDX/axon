//! Scenario test: ADR-018 §4 failure-mode table — all 14 rows.
//!
//! Each test case:
//! 1. Installs a `CaptureLayer` tracing subscriber scoped to the current thread.
//! 2. Builds the minimal defective request for the failure scenario.
//! 3. Sends the request through the full middleware + handler stack.
//! 4. Asserts the HTTP status and `error_code` JSON field.
//! 5. Asserts exactly one `axon.auth.reject` tracing event with the expected
//!    `error_code` field.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::StatusCode;
use axum::Router;
use http::Request;
use serde::Serialize;
use serde_json::Value;
use tower::ServiceExt;
use tracing_subscriber::prelude::*;
use uuid::Uuid;

use axon_core::auth::{
    GrantedDatabase, Grants, JwtClaims, Op, TenantId, TenantMember, TenantRole, User, UserId,
};
use axon_server::auth_pipeline::{
    jwt_verify_layer, AuthPipelineState, InMemoryRevocationCache, JwtIssuer,
};
use axon_storage::MemoryStorageAdapter;

// ── Constants ─────────────────────────────────────────────────────────────────

const ISSUER: &str = "matrix-issuer";
const TENANT: &str = "acme";
const DATABASE: &str = "orders";
const USER_ID: &str = "u-matrix-01";
const SECRET: &[u8] = b"matrix-test-secret";
const PATH: &str = "/tenants/acme/databases/orders/ping";

// ── Tracing capture layer ─────────────────────────────────────────────────────

/// Layer that records every tracing event emitted during the test.
#[derive(Default, Clone)]
struct CaptureLayer {
    events: Arc<Mutex<Vec<CapturedEvent>>>,
}

#[derive(Clone, Debug)]
struct CapturedEvent {
    target: String,
    fields: HashMap<String, String>,
}

impl CaptureLayer {
    /// Return events whose target is `"axon.auth.reject"`.
    fn reject_events(&self) -> Vec<CapturedEvent> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.target == "axon.auth.reject")
            .cloned()
            .collect()
    }
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for CaptureLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let target = event.metadata().target().to_string();
        let mut visitor = FieldCapture::default();
        event.record(&mut visitor);
        self.events.lock().unwrap().push(CapturedEvent {
            target,
            fields: visitor.fields,
        });
    }
}

#[derive(Default)]
struct FieldCapture {
    fields: HashMap<String, String>,
}

impl tracing::field::Visit for FieldCapture {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.fields
            .insert(field.name().to_string(), format!("{value:?}"));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.fields
            .insert(field.name().to_string(), value.to_string());
    }
}

// ── Test infrastructure ──────────────────────────────────────────────────────

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Standard state: one active user who is a tenant member with Write role.
fn make_state() -> Arc<AuthPipelineState> {
    let mut storage = MemoryStorageAdapter::default();
    storage.insert_user(User {
        id: UserId::new(USER_ID),
        display_name: "Matrix User".to_string(),
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

fn valid_claims() -> JwtClaims {
    let now = now_secs();
    JwtClaims {
        iss: ISSUER.to_string(),
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

fn make_app(state: Arc<AuthPipelineState>) -> Router {
    Router::new()
        .route(PATH, axum::routing::any(|| async { StatusCode::OK }))
        .layer(axum::middleware::from_fn_with_state(
            state,
            jwt_verify_layer,
        ))
}

async fn send(app: Router, req: Request<Body>) -> (StatusCode, Value) {
    let resp = app.oneshot(req).await.expect("oneshot should not fail");
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

// ── ADR-018 §4 table-driven scenario test ─────────────────────────────────────

struct Row {
    name: &'static str,
    expected_status: u16,
    expected_code: &'static str,
}

type Setup = Box<dyn FnOnce() -> (Arc<AuthPipelineState>, Request<Body>)>;

/// Walk every row of the ADR-018 §4 failure table.
///
/// For each row:
/// - Assert HTTP status matches the table.
/// - Assert the JSON body has the expected `error.code`.
/// - Assert exactly one `axon.auth.reject` tracing event fired with the
///   matching `error_code` field.
#[tokio::test(flavor = "current_thread")]
async fn auth_rejection_matrix() {
    let cases: Vec<(Row, Setup)> =
        vec![
            // ── Row 1: No Authorization header → 401 unauthenticated ───────────
            (
                Row {
                    name: "no_auth_header",
                    expected_status: 401,
                    expected_code: "unauthenticated",
                },
                Box::new(|| {
                    let req = Request::builder()
                        .method("GET")
                        .uri(PATH)
                        .body(Body::empty())
                        .unwrap();
                    (make_state(), req)
                }),
            ),
            // ── Row 2: Header not Bearer → 401 credential_malformed ─────────────
            (
                Row {
                    name: "not_bearer",
                    expected_status: 401,
                    expected_code: "credential_malformed",
                },
                Box::new(|| {
                    let req = Request::builder()
                        .method("GET")
                        .uri(PATH)
                        .header("Authorization", "Basic dXNlcjpwYXNz")
                        .body(Body::empty())
                        .unwrap();
                    (make_state(), req)
                }),
            ),
            // ── Row 3: JWT structurally invalid → 401 credential_malformed ──────
            (
                Row {
                    name: "jwt_malformed",
                    expected_status: 401,
                    expected_code: "credential_malformed",
                },
                Box::new(|| {
                    let req = Request::builder()
                        .method("GET")
                        .uri(PATH)
                        .header("Authorization", "Bearer not.a.valid.jwt")
                        .body(Body::empty())
                        .unwrap();
                    (make_state(), req)
                }),
            ),
            // ── Row 4: aud as JSON array → 401 credential_malformed ─────────────
            (
                Row {
                    name: "aud_is_array",
                    expected_status: 401,
                    expected_code: "credential_malformed",
                },
                Box::new(|| {
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
                    let bad = ArrayAudClaims {
                        iss: ISSUER.to_string(),
                        sub: USER_ID.to_string(),
                        aud: vec![TENANT.to_string()],
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
                        &bad,
                        &jsonwebtoken::EncodingKey::from_secret(SECRET),
                    )
                    .unwrap();
                    let auth = bearer(&token);
                    let req = Request::builder()
                        .method("GET")
                        .uri(PATH)
                        .header("Authorization", auth)
                        .body(Body::empty())
                        .unwrap();
                    (make_state(), req)
                }),
            ),
            // ── Row 5: Signature invalid → 401 credential_invalid ───────────────
            (
                Row {
                    name: "signature_invalid",
                    expected_status: 401,
                    expected_code: "credential_invalid",
                },
                Box::new(|| {
                    let wrong = JwtIssuer::new(b"wrong-key".to_vec(), ISSUER.to_string());
                    let token = wrong.issue(&valid_claims()).unwrap();
                    let auth = bearer(&token);
                    let req = Request::builder()
                        .method("GET")
                        .uri(PATH)
                        .header("Authorization", auth)
                        .body(Body::empty())
                        .unwrap();
                    (make_state(), req)
                }),
            ),
            // ── Row 6: exp in past (beyond skew) → 401 credential_expired ───────
            (
                Row {
                    name: "exp_expired",
                    expected_status: 401,
                    expected_code: "credential_expired",
                },
                Box::new(|| {
                    let state = make_state();
                    let mut claims = valid_claims();
                    claims.exp = now_secs() - 31;
                    let token = state.issuer.issue(&claims).unwrap();
                    let auth = bearer(&token);
                    let req = Request::builder()
                        .method("GET")
                        .uri(PATH)
                        .header("Authorization", auth)
                        .body(Body::empty())
                        .unwrap();
                    (state, req)
                }),
            ),
            // ── Row 7: nbf in future (beyond skew) → 401 credential_not_yet_valid
            (
                Row {
                    name: "nbf_future",
                    expected_status: 401,
                    expected_code: "credential_not_yet_valid",
                },
                Box::new(|| {
                    let state = make_state();
                    let mut claims = valid_claims();
                    claims.nbf = now_secs() + 31;
                    let token = state.issuer.issue(&claims).unwrap();
                    let auth = bearer(&token);
                    let req = Request::builder()
                        .method("GET")
                        .uri(PATH)
                        .header("Authorization", auth)
                        .body(Body::empty())
                        .unwrap();
                    (state, req)
                }),
            ),
            // ── Row 8: jti revoked → 401 credential_revoked ─────────────────────
            (
                Row {
                    name: "jti_revoked",
                    expected_status: 401,
                    expected_code: "credential_revoked",
                },
                Box::new(|| {
                    let state = make_state();
                    let claims = valid_claims();
                    let jti_uuid: Uuid = claims.jti.parse().unwrap();
                    let token = state.issuer.issue(&claims).unwrap();
                    state.revocation_cache.insert(jti_uuid);
                    let auth = bearer(&token);
                    let req = Request::builder()
                        .method("GET")
                        .uri(PATH)
                        .header("Authorization", auth)
                        .body(Body::empty())
                        .unwrap();
                    (state, req)
                }),
            ),
            // ── Row 9: iss unknown → 401 credential_foreign_issuer ──────────────
            (
                Row {
                    name: "iss_unknown",
                    expected_status: 401,
                    expected_code: "credential_foreign_issuer",
                },
                Box::new(|| {
                    let state = make_state();
                    let mut claims = valid_claims();
                    claims.iss = "some-other-issuer".to_string();
                    let token = state.issuer.issue(&claims).unwrap();
                    let auth = bearer(&token);
                    let req = Request::builder()
                        .method("GET")
                        .uri(PATH)
                        .header("Authorization", auth)
                        .body(Body::empty())
                        .unwrap();
                    (state, req)
                }),
            ),
            // ── Row 10: aud != URL tenant → 403 credential_wrong_tenant ─────────
            (
                Row {
                    name: "aud_wrong_tenant",
                    expected_status: 403,
                    expected_code: "credential_wrong_tenant",
                },
                Box::new(|| {
                    let state = make_state();
                    let mut claims = valid_claims();
                    claims.aud = "different-tenant".to_string();
                    let token = state.issuer.issue(&claims).unwrap();
                    let auth = bearer(&token);
                    let req = Request::builder()
                        .method("GET")
                        .uri(PATH)
                        .header("Authorization", auth)
                        .body(Body::empty())
                        .unwrap();
                    (state, req)
                }),
            ),
            // ── Row 11: sub suspended/deleted → 401 user_suspended ──────────────
            (
                Row {
                    name: "user_suspended",
                    expected_status: 401,
                    expected_code: "user_suspended",
                },
                Box::new(|| {
                    let mut storage = MemoryStorageAdapter::default();
                    storage.insert_user(User {
                        id: UserId::new(USER_ID),
                        display_name: "Suspended".to_string(),
                        email: None,
                        created_at_ms: 0,
                        suspended_at_ms: Some(1000),
                    });
                    storage.insert_tenant_member(TenantMember {
                        tenant_id: TenantId::new(TENANT),
                        user_id: UserId::new(USER_ID),
                        role: TenantRole::Write,
                    });
                    let state = Arc::new(AuthPipelineState {
                        issuer: Arc::new(JwtIssuer::new(SECRET.to_vec(), ISSUER.to_string())),
                        revocation_cache: Arc::new(InMemoryRevocationCache::new()),
                        storage: Arc::new(Mutex::new(Box::new(storage)
                            as Box<dyn axon_storage::StorageAdapter + Send + Sync>)),
                    });
                    let token = state.issuer.issue(&valid_claims()).unwrap();
                    let auth = bearer(&token);
                    let req = Request::builder()
                        .method("GET")
                        .uri(PATH)
                        .header("Authorization", auth)
                        .body(Body::empty())
                        .unwrap();
                    (state, req)
                }),
            ),
            // ── Row 12: sub not a tenant member → 403 not_a_tenant_member ───────
            (
                Row {
                    name: "not_tenant_member",
                    expected_status: 403,
                    expected_code: "not_a_tenant_member",
                },
                Box::new(|| {
                    let mut storage = MemoryStorageAdapter::default();
                    storage.insert_user(User {
                        id: UserId::new(USER_ID),
                        display_name: "No Member".to_string(),
                        email: None,
                        created_at_ms: 0,
                        suspended_at_ms: None,
                    });
                    // intentionally no insert_tenant_member
                    let state = Arc::new(AuthPipelineState {
                        issuer: Arc::new(JwtIssuer::new(SECRET.to_vec(), ISSUER.to_string())),
                        revocation_cache: Arc::new(InMemoryRevocationCache::new()),
                        storage: Arc::new(Mutex::new(Box::new(storage)
                            as Box<dyn axon_storage::StorageAdapter + Send + Sync>)),
                    });
                    let token = state.issuer.issue(&valid_claims()).unwrap();
                    let auth = bearer(&token);
                    let req = Request::builder()
                        .method("GET")
                        .uri(PATH)
                        .header("Authorization", auth)
                        .body(Body::empty())
                        .unwrap();
                    (state, req)
                }),
            ),
            // ── Row 13: URL database not in grants → 403 database_not_granted ───
            (
                Row {
                    name: "database_not_granted",
                    expected_status: 403,
                    expected_code: "database_not_granted",
                },
                Box::new(|| {
                    let state = make_state();
                    let mut claims = valid_claims();
                    claims.grants = Grants {
                        databases: vec![GrantedDatabase {
                            name: "invoices".to_string(), // URL says "orders"
                            ops: vec![Op::Read, Op::Write],
                        }],
                    };
                    let token = state.issuer.issue(&claims).unwrap();
                    let auth = bearer(&token);
                    let req = Request::builder()
                        .method("GET")
                        .uri(PATH)
                        .header("Authorization", auth)
                        .body(Body::empty())
                        .unwrap();
                    (state, req)
                }),
            ),
            // ── Row 14: Op not in grant → 403 op_not_granted ────────────────────
            (
                Row {
                    name: "op_not_granted",
                    expected_status: 403,
                    expected_code: "op_not_granted",
                },
                Box::new(|| {
                    let state = make_state();
                    let mut claims = valid_claims();
                    claims.grants = Grants {
                        databases: vec![GrantedDatabase {
                            name: DATABASE.to_string(),
                            ops: vec![Op::Read], // only Read; POST requires Write
                        }],
                    };
                    let token = state.issuer.issue(&claims).unwrap();
                    let auth = bearer(&token);
                    let req = Request::builder()
                        .method("POST")
                        .uri(PATH)
                        .header("Authorization", auth)
                        .body(Body::empty())
                        .unwrap();
                    (state, req)
                }),
            ),
        ];

    for (row, setup) in cases {
        let capture = CaptureLayer::default();
        let subscriber = tracing_subscriber::registry().with(capture.clone());
        let guard = tracing::subscriber::set_default(subscriber);

        let (state, req) = setup();
        let app = make_app(state);
        let (status, json) = send(app, req).await;

        drop(guard);

        assert_eq!(
            status.as_u16(),
            row.expected_status,
            "row '{}': expected HTTP {}",
            row.name,
            row.expected_status
        );
        assert_eq!(
            json["error"]["code"].as_str().unwrap_or(""),
            row.expected_code,
            "row '{}': expected error code '{}'",
            row.name,
            row.expected_code
        );

        let events = capture.reject_events();
        assert_eq!(
            events.len(),
            1,
            "row '{}': expected exactly 1 axon.auth.reject event, got {}",
            row.name,
            events.len()
        );
        assert_eq!(
            events[0].fields.get("error_code").map(String::as_str),
            Some(row.expected_code),
            "row '{}': expected error_code field '{}'",
            row.name,
            row.expected_code
        );
    }
}
