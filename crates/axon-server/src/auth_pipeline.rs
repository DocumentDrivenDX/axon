//! JWT-based auth pipeline: sign, verify, and enforce credentials (ADR-018 §4).
//!
//! # Verification order
//!
//! 1. Extract `Authorization: Bearer <token>` → `Unauthenticated` if missing.
//! 2. Parse + verify HS256 signature → `CredentialInvalid` / `CredentialMalformed`.
//! 3. Check `iss` against issuer ID → `CredentialForeignIssuer`.
//! 4. Check `exp`/`nbf` with ±30 s skew → `CredentialExpired` / `CredentialNotYetValid`.
//! 5. Check `jti` revocation (cache → storage) → `CredentialRevoked`.
//! 6. Compare `aud` to URL tenant → `CredentialWrongTenant`.
//! 7. Look up `sub` in users table, check suspension → `UserSuspended`.
//! 8. Check tenant membership → `NotATenantMember`.
//! 9. Walk grants for URL database + HTTP-method-derived op → `DatabaseNotGranted` / `OpNotGranted`.
//!
//! On success, installs `ResolvedIdentity` into the request extension.
//! On failure, returns a JSON error response built from `AuthError::status_code` and
//! `AuthError::error_code`.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use http::{HeaderMap, Method, StatusCode};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde_json::json;
use uuid::Uuid;

use axon_core::auth::{AuthError, JwtClaims, Op, ResolvedIdentity, TenantId, UserId};
use axon_storage::adapter::StorageAdapter;

// ── JwtIssuer ────────────────────────────────────────────────────────────────

/// Signs and verifies JWTs using HS256.
pub struct JwtIssuer {
    secret: Vec<u8>,
    /// Issuer identifier placed in the `iss` claim and verified on every request.
    pub issuer_id: String,
}

impl JwtIssuer {
    /// Create a new issuer with the given HMAC secret and issuer ID.
    pub fn new(secret: Vec<u8>, issuer_id: String) -> Self {
        Self { secret, issuer_id }
    }

    /// Sign `claims` into a compact HS256 JWT string.
    pub fn issue(&self, claims: &JwtClaims) -> Result<String, AuthError> {
        encode(
            &Header::new(Algorithm::HS256),
            claims,
            &EncodingKey::from_secret(&self.secret),
        )
        .map_err(|_| AuthError::CredentialMalformed)
    }

    /// Verify the signature and deserialise the claims.
    ///
    /// Time-based checks (`exp`, `nbf`) are intentionally skipped here; the
    /// middleware performs them manually with a configurable skew budget.
    pub fn verify(&self, token: &str) -> Result<JwtClaims, AuthError> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = false;
        validation.validate_nbf = false;
        validation.validate_aud = false;
        validation.required_spec_claims = HashSet::new();

        decode::<JwtClaims>(token, &DecodingKey::from_secret(&self.secret), &validation)
            .map(|td| td.claims)
            .map_err(|e| match e.kind() {
                jsonwebtoken::errors::ErrorKind::InvalidSignature => AuthError::CredentialInvalid,
                jsonwebtoken::errors::ErrorKind::Json(_) => AuthError::CredentialMalformed,
                jsonwebtoken::errors::ErrorKind::Base64(_) => AuthError::CredentialMalformed,
                jsonwebtoken::errors::ErrorKind::Utf8(_) => AuthError::CredentialMalformed,
                jsonwebtoken::errors::ErrorKind::InvalidToken => AuthError::CredentialMalformed,
                _ => AuthError::CredentialMalformed,
            })
    }
}

// ── InMemoryRevocationCache ───────────────────────────────────────────────────

/// In-process cache of revoked JWT IDs.
///
/// The middleware checks this first before falling back to the storage adapter,
/// avoiding a round-trip to storage on every request for known-revoked tokens.
#[derive(Default)]
pub struct InMemoryRevocationCache {
    inner: RwLock<HashSet<Uuid>>,
}

impl InMemoryRevocationCache {
    /// Create an empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if `jti` is in the cache.
    pub fn contains(&self, jti: &Uuid) -> bool {
        self.inner
            .read()
            .map(|g| g.contains(jti))
            .unwrap_or(false)
    }

    /// Add `jti` to the cache.
    pub fn insert(&self, jti: Uuid) {
        if let Ok(mut g) = self.inner.write() {
            g.insert(jti);
        }
    }
}

// ── AuthPipelineState ─────────────────────────────────────────────────────────

/// Shared state for the JWT verification middleware.
///
/// Wrap in `Arc` before passing to `axum::middleware::from_fn_with_state`.
#[derive(Clone)]
pub struct AuthPipelineState {
    /// Token issuer / verifier.
    pub issuer: Arc<JwtIssuer>,
    /// Fast in-process revocation check.
    pub revocation_cache: Arc<InMemoryRevocationCache>,
    /// Authoritative storage for user, membership, and revocation data.
    pub storage: Arc<Mutex<Box<dyn StorageAdapter + Send + Sync>>>,
}

impl AuthPipelineState {
    /// Convenience constructor.
    pub fn new(issuer: JwtIssuer, storage: Box<dyn StorageAdapter + Send + Sync>) -> Self {
        Self {
            issuer: Arc::new(issuer),
            revocation_cache: Arc::new(InMemoryRevocationCache::new()),
            storage: Arc::new(Mutex::new(storage)),
        }
    }
}

// ── Middleware ────────────────────────────────────────────────────────────────

/// Axum middleware that enforces the ADR-018 §4 JWT verification order.
///
/// Install with:
/// ```ignore
/// Router::new()
///     .route("/tenants/:t/databases/:db/…", …)
///     .layer(axum::middleware::from_fn_with_state(
///         Arc::new(state),
///         jwt_verify_layer,
///     ))
/// ```
pub async fn jwt_verify_layer(
    State(state): State<Arc<AuthPipelineState>>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let path = request.uri().path().to_string();

    // Skip middleware for paths that are not data-plane paths.
    let Some((tenant, database)) = extract_tenant_database(&path) else {
        return next.run(request).await;
    };

    let tenant = tenant.to_string();
    let database = database.to_string();
    let method = request.method().clone();
    let headers = request.headers().clone();

    match verify_request(&state, &headers, &method, &tenant, &database) {
        Ok(identity) => {
            let mut request = request;
            request.extensions_mut().insert(identity);
            next.run(request).await
        }
        Err(err) => AuthErrorResponse(err).into_response(),
    }
}

// ── Core verification logic ───────────────────────────────────────────────────

fn verify_request(
    state: &AuthPipelineState,
    headers: &HeaderMap,
    method: &Method,
    tenant: &str,
    database: &str,
) -> Result<ResolvedIdentity, AuthError> {
    // Step 1: Extract Bearer token.
    let token = extract_bearer_token(headers)?;

    // Step 2: Verify JWT signature and structure.
    let claims = state.issuer.verify(token)?;

    // Step 3: Check iss.
    if claims.iss != state.issuer.issuer_id {
        return Err(AuthError::CredentialForeignIssuer);
    }

    // Step 4: Check exp/nbf with 30 s skew.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    const SKEW: u64 = 30;
    // Expired: exp + skew < now  ↔  expired more than `skew` seconds ago
    if claims.exp.saturating_add(SKEW) < now {
        return Err(AuthError::CredentialExpired);
    }
    // Not yet valid: nbf > now + skew  ↔  valid more than `skew` seconds from now
    if claims.nbf > now.saturating_add(SKEW) {
        return Err(AuthError::CredentialNotYetValid);
    }

    // Step 5: Check JTI revocation (cache first, then storage).
    let jti_uuid: Uuid = claims
        .jti
        .parse()
        .map_err(|_| AuthError::CredentialMalformed)?;
    if state.revocation_cache.contains(&jti_uuid) {
        return Err(AuthError::CredentialRevoked);
    }
    {
        let storage = state
            .storage
            .lock()
            .map_err(|_| AuthError::Unauthenticated)?;
        if storage
            .is_jti_revoked(jti_uuid)
            .map_err(|_| AuthError::Unauthenticated)?
        {
            // Populate the cache so subsequent checks avoid another storage round-trip.
            drop(storage);
            state.revocation_cache.insert(jti_uuid);
            return Err(AuthError::CredentialRevoked);
        }
    }

    // Step 6: Compare aud to URL tenant.
    if claims.aud != tenant {
        return Err(AuthError::CredentialWrongTenant);
    }

    // Step 7: Look up user, check suspension.
    let user_id = UserId::new(&claims.sub);
    let user = {
        let storage = state
            .storage
            .lock()
            .map_err(|_| AuthError::Unauthenticated)?;
        storage
            .get_user(user_id.clone())
            .map_err(|_| AuthError::UserSuspended)?
            .ok_or(AuthError::UserSuspended)?
    };
    if user.suspended_at_ms.is_some() {
        return Err(AuthError::UserSuspended);
    }

    // Step 8: Check tenant membership.
    let tenant_id = TenantId::new(tenant);
    {
        let storage = state
            .storage
            .lock()
            .map_err(|_| AuthError::Unauthenticated)?;
        storage
            .get_tenant_member(tenant_id.clone(), user_id.clone())
            .map_err(|_| AuthError::NotATenantMember)?
            .ok_or(AuthError::NotATenantMember)?;
    }

    // Step 9: Walk grants for the URL database + HTTP-method-derived op.
    let granted_db = claims
        .grants
        .find_database(database)
        .ok_or(AuthError::DatabaseNotGranted)?;

    let required_op = if method == Method::GET {
        Op::Read
    } else {
        Op::Write
    };
    if !granted_db.has_op(required_op) {
        return Err(AuthError::OpNotGranted);
    }

    Ok(ResolvedIdentity {
        user_id,
        tenant_id,
        grants: claims.grants,
    })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Extract the `Bearer <token>` value from the `Authorization` header.
fn extract_bearer_token(headers: &HeaderMap) -> Result<&str, AuthError> {
    let auth_header = headers
        .get(http::header::AUTHORIZATION)
        .ok_or(AuthError::Unauthenticated)?;

    let auth_str = auth_header
        .to_str()
        .map_err(|_| AuthError::CredentialMalformed)?;

    auth_str
        .strip_prefix("Bearer ")
        .ok_or(AuthError::CredentialMalformed)
}

/// Extract `(tenant, database)` from a data-plane URL path.
///
/// Returns `Some((tenant, database))` for paths matching
/// `/tenants/{tenant}/databases/{database}/…`, `None` otherwise.
/// Non-data-plane paths skip the middleware entirely.
pub fn extract_tenant_database(path: &str) -> Option<(&str, &str)> {
    let path = path.strip_prefix('/')?;
    let mut parts = path.splitn(5, '/');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some("tenants"), Some(tenant), Some("databases"), Some(rest))
            if !tenant.is_empty() && !rest.is_empty() =>
        {
            let database = rest.split('/').next()?;
            if database.is_empty() {
                None
            } else {
                Some((tenant, database))
            }
        }
        _ => None,
    }
}

// ── AuthError → axum Response ─────────────────────────────────────────────────

/// Newtype wrapper that lets us implement `IntoResponse` for `AuthError` in
/// this crate (orphan-rule: neither `IntoResponse` nor `AuthError` is local).
pub struct AuthErrorResponse(pub AuthError);

impl IntoResponse for AuthErrorResponse {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.0.status_code())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = json!({
            "error": {
                "code": self.0.error_code(),
                "message": self.0.to_string()
            }
        });
        (status, axum::Json(body)).into_response()
    }
}
