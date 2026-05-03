//! HTTP/JSON gateway for the Axon service.
//!
//! Provides a REST API that mirrors the gRPC service operations. All responses
//! use structured JSON. Errors are returned as `{"code": "...", "detail": "..."}`
//! JSON objects with appropriate HTTP status codes.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use axum::body::Bytes;
use axum::extract::connect_info::MockConnectInfo;
use axum::extract::{Path, Query, State, WebSocketUpgrade};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, get_service, post, put};
use axum::{Extension, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tower_http::services::{ServeDir, ServeFile};
use uuid::Uuid;

use async_graphql::http::{playground_source, GraphQLPlaygroundConfig};
use axon_graphql::{BroadcastBroker, GraphqlIdempotencyScope};

use crate::actor_scope::ActorScopeGuard;
use crate::auth::{AuthContext, AuthError, Identity};
use crate::collection_listing::{filter_audit_entries_to_database, list_collections_for_database};
use crate::cors_config::CorsStore;
use crate::idempotency::{IdempotencyStore, ReservationResult};
use crate::mcp_http::{
    notify_entity_change, notify_entity_change_by_parts, notify_tool_list_changed, McpHttpSessions,
};
use crate::rate_limit::{RateLimited, WriteRateLimiter};
use axon_api::handler::AxonHandler;
use axon_api::request::{
    CreateCollectionRequest, CreateDatabaseRequest, CreateEntityRequest, CreateLinkRequest,
    CreateNamespaceRequest, DeleteCollectionTemplateRequest, DeleteEntityRequest,
    DeleteLinkRequest, DescribeCollectionRequest, DropCollectionRequest, DropDatabaseRequest,
    DropNamespaceRequest, FilterNode, GetCollectionTemplateRequest, GetEntityRequest,
    GetSchemaRequest, ListCollectionsRequest, ListDatabasesRequest,
    ListNamespaceCollectionsRequest, ListNamespacesRequest, PutCollectionTemplateRequest,
    PutSchemaRequest, QueryAuditRequest, QueryEntitiesRequest, RevertEntityRequest,
    RollbackCollectionRequest, RollbackEntityRequest, RollbackEntityTarget,
    RollbackTransactionRequest, SnapshotRequest, TransitionLifecycleRequest, TraverseDirection,
    TraverseRequest, UpdateEntityRequest,
};
use axon_api::response::GetEntityMarkdownResponse;
use axon_audit::entry::{AuditAttribution, AuditEntry, MutationType};
use axon_audit::AuditLog;
use axon_core::auth::{CallerIdentity as CoreCallerIdentity, ResolvedIdentity, Role as CoreRole};
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId, Namespace, DEFAULT_DATABASE, DEFAULT_SCHEMA};
use axon_core::types::{Entity, Link};
use axon_schema::schema::CollectionSchema;
use axon_storage::adapter::StorageAdapter;

use crate::tenant_router::{TenantHandler, TenantRouter};

/// Shared handler type alias — used by MCP HTTP routes which continue to
/// use axum `State` for backward compatibility.
#[allow(dead_code)]
type SharedHandler<S> = Arc<Mutex<AxonHandler<S>>>;

/// Idempotency store type used by the HTTP gateway.
///
/// Caches a terminal `POST /transactions` response, keyed by
/// `(database_id, idempotency_key)`, for up to 5 minutes (FEAT-008 US-081).
pub type HttpIdempotencyStore = Arc<IdempotencyStore<CachedHttpResponse>>;

#[derive(Debug, Clone)]
pub struct CachedHttpResponse {
    status: StatusCode,
    body: Value,
}

/// Header name for idempotency keys on `POST /transactions` (FEAT-008 US-081).
const IDEMPOTENCY_KEY_HEADER: &str = "idempotency-key";

/// Response header emitted when returning a cached idempotent response.
const IDEMPOTENCY_CACHE_HEADER: &str = "x-idempotent-cache";

/// Header a static browser bundle can send to assert the schema hash it was
/// generated against during app-load compatibility checks.
const SCHEMA_HASH_HEADER: &str = "x-axon-schema-hash";

/// Request/response correlation header emitted on every HTTP response.
const REQUEST_ID_HEADER: &str = "x-request-id";

const CORS_ALLOW_METHODS: &str = "GET, POST, PUT, PATCH, DELETE, OPTIONS";
const CORS_ALLOW_HEADERS: &str = "Content-Type, Authorization, X-Axon-Schema-Hash, X-Axon-Actor";
const CORS_EXPOSE_HEADERS: &str =
    "X-Idempotent-Cache, X-Axon-Schema-Hash, X-Request-Id, X-Axon-Query-Cost";
const CORS_MAX_AGE_SECONDS: &str = "86400";

/// Header carrying the caller-declared actor identity (FEAT-012).
///
/// The HTTP gateway and gRPC service read this header on every request and
/// use its value as the source of truth for audit entry `actor` fields,
/// overriding any body-level `actor` string the client might send.
pub(crate) const AXON_ACTOR_HEADER: &str = "x-axon-actor";

/// Outcome of parsing the `x-axon-actor` request header.
enum ActorHeaderOutcome {
    /// Header absent or empty after trimming — fall back to [`CallerIdentity`]
    /// derived from the authenticated [`Identity`].
    Absent,
    /// Header present and valid — override `caller.actor` with this value.
    Present(String),
    /// Header value contains control characters or non-ASCII-safe bytes; the
    /// middleware rejects the request with `400 Bad Request`.
    Invalid,
}

/// Parse the `x-axon-actor` header into an [`ActorHeaderOutcome`].
///
/// Accepts any ASCII-printable identifier after trimming whitespace. Rejects
/// values that contain control characters, carriage returns, or newlines
/// (would corrupt downstream audit log formatting and HTTP protocol framing).
fn parse_actor_header(headers: &HeaderMap) -> ActorHeaderOutcome {
    let Some(raw) = headers.get(AXON_ACTOR_HEADER) else {
        return ActorHeaderOutcome::Absent;
    };
    let value = match raw.to_str() {
        Ok(s) => s,
        Err(_) => return ActorHeaderOutcome::Invalid,
    };
    if value
        .chars()
        .any(|c| c.is_control() || c == '\n' || c == '\r')
    {
        return ActorHeaderOutcome::Invalid;
    }
    let trimmed = value.trim();
    if trimmed.is_empty() {
        ActorHeaderOutcome::Absent
    } else {
        ActorHeaderOutcome::Present(trimmed.to_string())
    }
}

/// Map the authenticated [`Identity::role`] into a core [`CoreRole`].
fn core_role_from_identity(identity: &Identity) -> CoreRole {
    match identity.role {
        crate::auth::Role::Admin => CoreRole::Admin,
        crate::auth::Role::Write => CoreRole::Write,
        crate::auth::Role::Read => CoreRole::Read,
    }
}

/// Build a [`CoreCallerIdentity`] from the authenticated [`Identity`] and the
/// `x-axon-actor` header (FEAT-012).
///
/// - Header present + non-empty → `actor` = header value (trimmed), `role`
///   inherited from the authenticated identity.
/// - Header missing or empty after trimming → `actor` = `identity.actor`
///   (preserving existing Tailscale/guest/no-auth semantics), `role`
///   inherited from the authenticated identity.
///
/// In `--no-auth` mode the inherited `identity.actor` is `"anonymous"`, which
/// satisfies the FEAT-012 acceptance criterion that a missing header falls
/// back to an anonymous caller identity.
fn caller_from_parts(identity: &Identity, header: ActorHeaderOutcome) -> CoreCallerIdentity {
    let role = core_role_from_identity(identity);
    match header {
        ActorHeaderOutcome::Present(actor) => CoreCallerIdentity::new(actor, role),
        ActorHeaderOutcome::Absent => CoreCallerIdentity::new(identity.actor.clone(), role),
        ActorHeaderOutcome::Invalid => {
            // Reached only if `resolve_caller_identity` skipped its own
            // validation; defensively return a non-actor-leaking fallback.
            CoreCallerIdentity::new(identity.actor.clone(), role)
        }
    }
}

// ── Error response ────────────────────────────────────────────────────────────

/// Structured JSON error response with field-level details.
#[derive(Serialize)]
pub struct ApiError {
    pub code: String,
    pub detail: Value,
}

impl ApiError {
    pub(crate) fn new(code: &str, detail: impl Into<Value>) -> Self {
        Self {
            code: code.into(),
            detail: detail.into(),
        }
    }
}

fn axon_error_response(err: AxonError) -> Response {
    match err {
        AxonError::NotFound(msg) => {
            (StatusCode::NOT_FOUND, Json(ApiError::new("not_found", msg))).into_response()
        }
        AxonError::ConflictingVersion {
            expected,
            actual,
            current_entity,
        } => (
            StatusCode::CONFLICT,
            Json(ApiError::new(
                "version_conflict",
                json!({"expected": expected, "actual": actual, "current_entity": current_entity}),
            )),
        )
            .into_response(),
        AxonError::SchemaValidation(detail) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiError::new(
                "schema_validation",
                axon_core::error::schema_validation_detail(&detail),
            )),
        )
            .into_response(),
        AxonError::AlreadyExists(msg) => (
            StatusCode::CONFLICT,
            Json(ApiError::new("already_exists", msg)),
        )
            .into_response(),
        AxonError::InvalidArgument(msg) => (
            StatusCode::BAD_REQUEST,
            Json(ApiError::new("invalid_argument", msg)),
        )
            .into_response(),
        AxonError::InvalidOperation(msg) => (
            StatusCode::BAD_REQUEST,
            Json(ApiError::new("invalid_operation", msg)),
        )
            .into_response(),
        AxonError::Storage(msg) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", msg)),
        )
            .into_response(),
        AxonError::Serialization(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("serialization_error", e.to_string())),
        )
            .into_response(),
        AxonError::UniqueViolation { field, value } => (
            StatusCode::CONFLICT,
            Json(ApiError::new(
                "unique_violation",
                json!({"field": field, "value": value}),
            )),
        )
            .into_response(),
        AxonError::LifecycleNotFound { lifecycle_name } => (
            StatusCode::NOT_FOUND,
            Json(ApiError::new(
                "lifecycle_not_found",
                json!({"lifecycle_name": lifecycle_name}),
            )),
        )
            .into_response(),
        AxonError::InvalidTransition {
            lifecycle_name,
            current_state,
            target_state,
            valid_transitions,
        } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiError::new(
                "invalid_transition",
                json!({
                    "lifecycle_name": lifecycle_name,
                    "current_state": current_state,
                    "target_state": target_state,
                    "valid_transitions": valid_transitions,
                }),
            )),
        )
            .into_response(),
        AxonError::LifecycleFieldMissing { field } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiError::new(
                "lifecycle_field_missing",
                json!({"field": field}),
            )),
        )
            .into_response(),
        AxonError::LifecycleStateInvalid { field, actual } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiError::new(
                "lifecycle_state_invalid",
                json!({"field": field, "actual": actual}),
            )),
        )
            .into_response(),
        AxonError::RateLimitExceeded {
            actor,
            retry_after_ms,
        } => {
            let retry_after_seconds = retry_after_ms.saturating_add(999) / 1000;
            (
                StatusCode::TOO_MANY_REQUESTS,
                [(header::RETRY_AFTER, retry_after_seconds.to_string())],
                Json(ApiError::new(
                    "rate_limit_exceeded",
                    json!({
                        "actor": actor,
                        "retry_after_seconds": retry_after_seconds,
                    }),
                )),
            )
                .into_response()
        }
        AxonError::PolicyDenied(denial) => {
            let (status, code) = if denial.is_policy_filter_unindexed() {
                (StatusCode::BAD_REQUEST, "policy_filter_unindexed")
            } else {
                (StatusCode::FORBIDDEN, "forbidden")
            };
            (status, Json(ApiError::new(code, denial.detail()))).into_response()
        }
        AxonError::Forbidden(msg) => {
            (StatusCode::FORBIDDEN, Json(ApiError::new("forbidden", msg))).into_response()
        }
        AxonError::ScopeViolation {
            actor,
            entity_id,
            filter_field,
            filter_value,
        } => (
            StatusCode::FORBIDDEN,
            Json(ApiError::new(
                "scope_violation",
                json!({
                    "actor": actor,
                    "entity_id": entity_id,
                    "filter_field": filter_field,
                    "filter_value": filter_value,
                }),
            )),
        )
            .into_response(),
    }
}

/// Convert an [`AuthError`] into an HTTP error response.
///
/// | `AuthError` variant | HTTP status | JSON code |
/// |---------------------|-------------|-----------|
/// | `MissingPeerAddress` / `Unauthorized` | 401 | `"unauthorized"` |
/// | `Forbidden` | 403 | `"forbidden"` |
/// | `ProviderUnavailable` | 503 | `"auth_unavailable"` |
pub(crate) fn auth_error_response(err: AuthError) -> Response {
    match err {
        AuthError::MissingPeerAddress | AuthError::Unauthorized(_) => (
            StatusCode::UNAUTHORIZED,
            Json(ApiError::new("unauthorized", err.to_string())),
        )
            .into_response(),
        AuthError::Forbidden(_) => (
            StatusCode::FORBIDDEN,
            Json(ApiError::new("forbidden", err.to_string())),
        )
            .into_response(),
        AuthError::ProviderUnavailable(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiError::new("auth_unavailable", err.to_string())),
        )
            .into_response(),
    }
}

fn rate_limit_response(limited: &RateLimited) -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::RETRY_AFTER, limited.retry_after_secs.to_string())],
        Json(ApiError::new(
            "rate_limit_exceeded",
            json!({
                "message": "write rate limit exceeded",
                "retry_after_seconds": limited.retry_after_secs,
                "scope": "actor_write",
            }),
        )),
    )
        .into_response()
}

/// Extract the connecting peer's socket address from an axum request.
///
/// Checks both [`axum::extract::ConnectInfo`] (real TCP connections) and
/// [`axum::extract::connect_info::MockConnectInfo`] (integration tests) so
/// auth middleware works in both production and test contexts.
fn request_peer_address(request: &axum::extract::Request) -> Option<SocketAddr> {
    request
        .extensions()
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|connect_info| connect_info.0)
        .or_else(|| {
            request
                .extensions()
                .get::<MockConnectInfo<SocketAddr>>()
                .map(|connect_info| connect_info.0)
        })
}

#[derive(Clone, Debug)]
pub(crate) struct CurrentDatabase(String);

impl CurrentDatabase {
    pub(crate) fn new(database: impl Into<String>) -> Self {
        Self(database.into())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CurrentTenant(String);

impl CurrentTenant {
    pub(crate) fn new(tenant: impl Into<String>) -> Self {
        Self(tenant.into())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug)]
struct RequestedDatabaseScope(Option<String>);

impl RequestedDatabaseScope {
    fn database(&self) -> Option<&str> {
        self.0.as_deref()
    }
}

fn requested_database_scope(request: &axum::extract::Request) -> RequestedDatabaseScope {
    RequestedDatabaseScope(
        crate::path_router::extract_tenant_database(request.uri().path())
            .map(|(_tenant, database)| database),
    )
}

fn request_current_database(request: &axum::extract::Request) -> CurrentDatabase {
    let requested_scope = requested_database_scope(request);
    CurrentDatabase::new(requested_scope.database().unwrap_or(DEFAULT_DATABASE))
}

fn request_current_tenant(request: &axum::extract::Request) -> CurrentTenant {
    let tenant = crate::path_router::extract_tenant_database(request.uri().path())
        .map(|(tenant, _database)| tenant)
        .unwrap_or_else(|| "default".to_string());
    CurrentTenant::new(tenant)
}

fn qualify_collection_name(collection: &str, current_database: &CurrentDatabase) -> CollectionId {
    if current_database.as_str() == DEFAULT_DATABASE {
        return CollectionId::new(collection);
    }

    CollectionId::new(Namespace::qualify_with_database(
        collection,
        current_database.as_str(),
    ))
}

fn idempotency_scope(current_tenant: &CurrentTenant, current_database: &CurrentDatabase) -> String {
    format!("{}:{}", current_tenant.as_str(), current_database.as_str())
}

fn default_namespace_health<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    databases: &[String],
) -> Result<(Option<String>, &'static str), AxonError> {
    if !databases
        .iter()
        .any(|database| database == DEFAULT_DATABASE)
    {
        return Ok((None, "missing"));
    }

    let has_default_schema = handler
        .list_namespaces(ListNamespacesRequest {
            database: DEFAULT_DATABASE.to_string(),
        })?
        .schemas
        .iter()
        .any(|schema| schema == DEFAULT_SCHEMA);

    Ok((
        has_default_schema.then(|| format!("{DEFAULT_DATABASE}.{DEFAULT_SCHEMA}")),
        if has_default_schema { "ok" } else { "missing" },
    ))
}

/// Axum middleware that extracts the `x-axon-actor` header and injects a
/// [`CoreCallerIdentity`] extension (FEAT-012).
///
/// Runs after [`authenticate_http_request`], so an [`Identity`] is already
/// available in `request.extensions_mut()`. The header value, when present
/// and valid, takes precedence over `Identity::actor` for audit entry
/// provenance; when absent, the middleware preserves back-compat by
/// populating `CallerIdentity::actor` from the authenticated identity
/// (which is `"anonymous"` in `--no-auth` mode).
///
/// A header value containing ASCII control characters (CR, LF, or any
/// `c.is_control()`) is rejected with `400 Bad Request` so callers cannot
/// smuggle protocol-framing bytes into the audit log.
pub(crate) async fn resolve_caller_identity(
    mut request: axum::extract::Request,
    next: Next,
) -> Response {
    let identity = match request.extensions().get::<Identity>().cloned() {
        Some(id) => id,
        None => {
            // Auth middleware did not run — fall back to anonymous-admin so
            // unauthenticated test harnesses or misconfigurations still
            // produce a usable CallerIdentity rather than panicking.
            Identity::anonymous_admin()
        }
    };
    let outcome = parse_actor_header(request.headers());
    if matches!(outcome, ActorHeaderOutcome::Invalid) {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                "invalid_argument",
                "x-axon-actor header must not contain control characters",
            )),
        )
            .into_response();
    }
    let caller = caller_from_parts(&identity, outcome);
    request.extensions_mut().insert(caller);
    next.run(request).await
}

/// Axum middleware layer that resolves the caller's [`Identity`] and injects
/// it as a typed request extension.
///
/// This middleware runs before every route handler.  It:
/// 1. Extracts the peer socket address via [`request_peer_address`].
/// 2. Calls [`AuthContext::resolve_peer`] (cache-first, then Tailscale whois).
/// 3. Inserts the resolved [`Identity`] into `request.extensions`.
/// 4. On auth failure, short-circuits with an appropriate HTTP error response
///    (401, 403, or 503) without reaching the route handler.
///
/// Route handlers extract identity with:
/// ```rust,ignore
/// async fn my_handler(Extension(identity): Extension<Identity>, ...) { ... }
/// ```
/// and then call `identity.require_read()` / `require_write()` / `require_admin()`
/// to enforce the minimum required role for that operation.
pub async fn authenticate_http_request(
    State(auth): State<AuthContext>,
    mut request: axum::extract::Request,
    next: Next,
) -> Response {
    let requested_database_scope = requested_database_scope(&request);
    let current_database = request_current_database(&request);
    let current_tenant = request_current_tenant(&request);
    request.extensions_mut().insert(current_database);
    request.extensions_mut().insert(current_tenant);
    request.extensions_mut().insert(requested_database_scope);
    match auth.resolve_peer(request_peer_address(&request)).await {
        Ok(identity) => {
            request.extensions_mut().insert(identity);
            next.run(request).await
        }
        Err(error) => auth_error_response(error),
    }
}

/// CORS middleware — runs **outside** the auth layer so that OPTIONS preflights
/// bypass authentication entirely.
///
/// Behaviour depends on the configured [`CorsStore`]:
///
/// | Store state          | OPTIONS response                           | Non-OPTIONS  |
/// |---------------------|--------------------------------------------|-------------|
/// | Empty (no config)   | 200, no CORS headers                       | No headers  |
/// | Origin in allow-list | 200 + CORS headers, `ACAO: <echo origin>` | `ACAO: <echo origin>` |
/// | Wildcard (`*`)      | 200 + CORS headers, `ACAO: *`              | `ACAO: *`   |
///
/// `Access-Control-Max-Age: 86400` is included in preflight responses so that
/// browsers cache the preflight for 24 hours.
pub(crate) async fn cors_middleware(
    State(cors): State<CorsStore>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    // Short-circuit OPTIONS preflight — bypass auth entirely.
    if request.method() == axum::http::Method::OPTIONS {
        let mut builder = axum::http::Response::builder().status(StatusCode::OK);
        if let Some(ref orig) = origin {
            if !cors.is_empty() && (cors.is_wildcard() || cors.is_allowed(orig)) {
                let acao = if cors.is_wildcard() {
                    "*"
                } else {
                    orig.as_str()
                };
                builder = builder
                    .header("Access-Control-Allow-Origin", acao)
                    .header("Access-Control-Allow-Methods", CORS_ALLOW_METHODS)
                    .header("Access-Control-Allow-Headers", CORS_ALLOW_HEADERS)
                    .header("Access-Control-Max-Age", CORS_MAX_AGE_SECONDS)
                    .header("Vary", "Origin");
            }
        }
        return builder
            .body(axum::body::Body::empty())
            .unwrap_or_else(|_| axum::http::Response::new(axum::body::Body::empty()));
    }

    let mut response = next.run(request).await;

    if let Some(ref orig) = origin {
        if !cors.is_empty() {
            let acao: Option<String> = if cors.is_wildcard() {
                Some("*".into())
            } else if cors.is_allowed(orig) {
                Some(orig.clone())
            } else {
                None
            };
            if let Some(value) = acao {
                if let Ok(v) = axum::http::HeaderValue::from_str(&value) {
                    response
                        .headers_mut()
                        .insert("Access-Control-Allow-Origin", v);
                    response.headers_mut().insert(
                        "Access-Control-Expose-Headers",
                        HeaderValue::from_static(CORS_EXPOSE_HEADERS),
                    );
                    response
                        .headers_mut()
                        .insert("Vary", HeaderValue::from_static("Origin"));
                }
            }
        }
    }

    response
}

/// Attach a stable request correlation ID to every response. If the caller
/// supplied a syntactically valid `x-request-id`, echo it; otherwise generate a
/// fresh UUIDv7 so logs and browser-visible failures can be correlated.
pub(crate) async fn request_id_middleware(request: axum::extract::Request, next: Next) -> Response {
    let request_id = request
        .headers()
        .get(REQUEST_ID_HEADER)
        .filter(|value| value.to_str().is_ok())
        .cloned()
        .unwrap_or_else(|| {
            let generated = Uuid::now_v7().to_string();
            HeaderValue::from_str(&generated).unwrap_or_else(|_| HeaderValue::from_static("0"))
        });

    let mut response = next.run(request).await;
    response.headers_mut().insert(REQUEST_ID_HEADER, request_id);
    response
}

/// Middleware that resolves the per-tenant handler from the `TenantRouter`
/// and inserts it as a request [`Extension<TenantHandler>`].
///
/// Reads `(tenant, database)` from the URL path for data-plane requests
/// matching `/tenants/{tenant}/databases/{database}/…` and calls
/// [`TenantRouter::get_or_create_any`] with the composite slug
/// `{tenant}:{database}`. For all other paths (health, metrics, UI, etc.)
/// falls back to the `"default"` handler.
async fn resolve_tenant_handler(
    Extension(router): Extension<Arc<TenantRouter>>,
    mut request: axum::extract::Request,
    next: Next,
) -> Response {
    let slug = crate::path_router::extract_tenant_database(request.uri().path())
        .map(|(tenant, database)| format!("{tenant}:{database}"))
        .unwrap_or_else(|| "default".to_string());

    match router.get_or_create_any(&slug).await {
        Ok(handler) => {
            request.extensions_mut().insert(handler);
            next.run(request).await
        }
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("tenant_error", err)),
        )
            .into_response(),
    }
}

/// Returns the current authenticated identity as JSON.
///
/// This endpoint always succeeds for authenticated (or guest/anonymous) users.
/// The UI calls this on load to determine who the current user is.
async fn auth_me(
    Extension(identity): Extension<Identity>,
    jwt_identity: Option<Extension<ResolvedIdentity>>,
) -> impl IntoResponse {
    let resolved = jwt_identity.map(|Extension(id)| id);
    Json(json!({
        "actor": identity.actor,
        "role": identity.role,
        "user_id": resolved.as_ref().map(|id| id.user_id.to_string()),
        "tenant_id": resolved.as_ref().map(|id| id.tenant_id.to_string()),
    }))
}

fn entity_payload(entity: &Entity) -> Value {
    let mut payload = json!({
        "collection": entity.collection.to_string(),
        "id": entity.id.to_string(),
        "version": entity.version,
        "data": &entity.data,
    });
    if let Some(sv) = entity.schema_version {
        payload["schema_version"] = json!(sv);
    }
    payload
}

fn stable_json_hash(value: &Value) -> Result<String, AxonError> {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let bytes = serde_json::to_vec(value)?;
    let hash = bytes.iter().fold(FNV_OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME)
    });
    Ok(format!("fnv64:{hash:016x}"))
}

fn audit_entry_payload(entry: &axon_audit::AuditEntry) -> Value {
    json!({
        "id": entry.id,
        "timestamp_ns": entry.timestamp_ns,
        "collection": entry.collection.to_string(),
        "entity_id": entry.entity_id.to_string(),
        "version": entry.version,
        "operation": entry.mutation.to_string(),
        "data_before": &entry.data_before,
        "data_after": &entry.data_after,
        "diff": &entry.diff,
        "actor": &entry.actor,
        "metadata": &entry.metadata,
        "transaction_id": &entry.transaction_id,
    })
}

// ── GraphQL subscription broadcast helpers ───────────────────────────────────

/// Broadcast an entity change to GraphQL subscription clients.
///
/// Silently drops the event if no subscribers are connected.
///
/// `audit_id` is the stringified audit entry id produced by the write.
/// Consumers use it as a resume cursor via `since_audit_id` on reconnect.
/// Pass an empty string only when the audit id is genuinely unavailable.
fn broadcast_entity_change(
    broker: &BroadcastBroker,
    entity: &Entity,
    operation: &str,
    audit_id: String,
    actor: &str,
    current_tenant: &CurrentTenant,
    current_database: &CurrentDatabase,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let event = axon_graphql::ChangeEvent {
        tenant: Some(current_tenant.as_str().to_string()),
        database: Some(current_database.as_str().to_string()),
        audit_id,
        collection: entity.collection.to_string(),
        entity_id: entity.id.to_string(),
        operation: operation.to_string(),
        data: Some(entity.data.clone()),
        previous_data: None,
        version: entity.version,
        previous_version: if operation == "create" || entity.version == 0 {
            None
        } else {
            Some(entity.version.saturating_sub(1))
        },
        timestamp_ms: now,
        actor: actor.to_string(),
    };
    let _ = broker.publish(event);
}

/// Broadcast a delete event to GraphQL subscription clients.
///
/// `audit_id` is the stringified audit entry id produced by the delete.
/// See [`broadcast_entity_change`] for details.
fn broadcast_entity_delete(
    broker: &BroadcastBroker,
    collection: &str,
    entity_id: &str,
    audit_id: String,
    actor: &str,
    current_tenant: &CurrentTenant,
    current_database: &CurrentDatabase,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let event = axon_graphql::ChangeEvent {
        tenant: Some(current_tenant.as_str().to_string()),
        database: Some(current_database.as_str().to_string()),
        audit_id,
        collection: collection.to_string(),
        entity_id: entity_id.to_string(),
        operation: "delete".to_string(),
        data: None,
        previous_data: None,
        version: 0,
        previous_version: None,
        timestamp_ms: now,
        actor: actor.to_string(),
    };
    let _ = broker.publish(event);
}

/// Format an optional audit id as a string for `ChangeEvent.audit_id`.
///
/// Returns the decimal representation when present, or an empty string when
/// the caller did not capture the id. An empty string preserves the previous
/// wire-level default so older subscribers do not observe a regression.
fn audit_id_string(audit_id: Option<u64>) -> String {
    match audit_id {
        Some(id) => id.to_string(),
        None => String::new(),
    }
}

fn change_event_from_audit_entry(
    entry: &AuditEntry,
    current_tenant: &CurrentTenant,
    current_database: &CurrentDatabase,
) -> Option<axon_graphql::ChangeEvent> {
    let operation = match entry.mutation {
        MutationType::EntityCreate => "create",
        MutationType::EntityUpdate | MutationType::EntityRevert => "update",
        MutationType::EntityDelete => "delete",
        _ => return None,
    };
    let previous_version = match entry.mutation {
        MutationType::EntityCreate => None,
        MutationType::EntityUpdate | MutationType::EntityRevert => entry.version.checked_sub(1),
        MutationType::EntityDelete => Some(entry.version),
        _ => None,
    };

    Some(axon_graphql::ChangeEvent {
        tenant: Some(current_tenant.as_str().to_string()),
        database: Some(current_database.as_str().to_string()),
        audit_id: entry.id.to_string(),
        collection: entry.collection.to_string(),
        entity_id: entry.entity_id.to_string(),
        operation: operation.to_string(),
        data: entry.data_after.clone(),
        previous_data: entry.data_before.clone(),
        version: entry.version,
        previous_version,
        timestamp_ms: entry.timestamp_ns / 1_000_000,
        actor: entry.actor.clone(),
    })
}

fn publish_change_event_from_audit_entry(
    broker: &BroadcastBroker,
    entry: &AuditEntry,
    current_tenant: &CurrentTenant,
    current_database: &CurrentDatabase,
) -> bool {
    match change_event_from_audit_entry(entry, current_tenant, current_database) {
        Some(event) => {
            let _ = broker.publish(event);
            true
        }
        None => false,
    }
}

// ── Request bodies ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateEntityBody {
    pub data: Value,
    pub actor: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateEntityBody {
    pub data: Value,
    pub expected_version: u64,
    pub actor: Option<String>,
}

#[derive(Deserialize)]
pub struct DeleteEntityBody {
    pub actor: Option<String>,
}

#[derive(Default, Deserialize)]
pub struct DeleteCollectionTemplateBody {
    pub actor: Option<String>,
}

#[derive(Deserialize)]
pub struct RevertEntityBody {
    pub audit_entry_id: u64,
    pub actor: Option<String>,
    #[serde(default)]
    pub force: bool,
}

#[derive(Deserialize)]
pub struct RollbackEntityBody {
    pub to_version: Option<u64>,
    pub to_audit_id: Option<String>,
    pub expected_version: Option<u64>,
    pub actor: Option<String>,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Deserialize)]
pub struct CollectionActorBody {
    pub actor: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct ForceQuery {
    #[serde(default)]
    pub force: bool,
}

#[derive(Deserialize, Default)]
pub struct GetEntityParams {
    pub format: Option<String>,
}

#[derive(Deserialize)]
struct CollectionEntityPath {
    collection: String,
    id: String,
}

#[derive(Deserialize)]
struct CollectionPath {
    collection: String,
}

#[derive(Deserialize)]
struct NamePath {
    name: String,
}

/// Request body for `POST /collections/{name}`.
///
/// A `schema` field is required; schemaless collections are not supported (FEAT-001).
#[derive(Deserialize)]
pub struct CreateCollectionBody {
    /// Schema fields (excluding `collection`, which is taken from the path).
    /// Must be present — omitting this field returns a 400 error.
    pub schema: Option<CreateCollectionSchemaBody>,
    pub actor: Option<String>,
}

/// The schema portion of a `CreateCollectionBody`.
#[derive(Deserialize)]
pub struct CreateCollectionSchemaBody {
    pub description: Option<String>,
    #[serde(default = "default_schema_version")]
    pub version: u32,
    pub entity_schema: Option<Value>,
    pub link_types: Option<std::collections::HashMap<String, axon_schema::LinkTypeDef>>,
    pub access_control: Option<axon_schema::AccessControlPolicy>,
    pub gates: Option<std::collections::HashMap<String, axon_schema::GateDef>>,
    pub validation_rules: Option<Vec<axon_schema::ValidationRule>>,
    pub indexes: Option<Vec<axon_schema::IndexDef>>,
    pub compound_indexes: Option<Vec<axon_schema::CompoundIndexDef>>,
    pub lifecycles: Option<std::collections::HashMap<String, axon_schema::LifecycleDef>>,
}

fn default_schema_version() -> u32 {
    1
}

#[derive(Deserialize)]
pub struct CreateLinkBody {
    pub source_collection: String,
    pub source_id: String,
    pub target_collection: String,
    pub target_id: String,
    pub link_type: String,
    #[serde(default)]
    pub metadata: Value,
    pub actor: Option<String>,
}

#[derive(Deserialize)]
pub struct DeleteLinkBody {
    pub source_collection: String,
    pub source_id: String,
    pub target_collection: String,
    pub target_id: String,
    pub link_type: String,
    pub actor: Option<String>,
}

#[derive(Deserialize)]
pub struct PutSchemaBody {
    pub description: Option<String>,
    pub version: u32,
    pub entity_schema: Option<Value>,
    pub link_types: Option<std::collections::HashMap<String, axon_schema::LinkTypeDef>>,
    pub access_control: Option<axon_schema::AccessControlPolicy>,
    pub gates: Option<std::collections::HashMap<String, axon_schema::GateDef>>,
    pub validation_rules: Option<Vec<axon_schema::ValidationRule>>,
    pub indexes: Option<Vec<axon_schema::IndexDef>>,
    pub compound_indexes: Option<Vec<axon_schema::CompoundIndexDef>>,
    pub lifecycles: Option<std::collections::HashMap<String, axon_schema::LifecycleDef>>,
    pub actor: Option<String>,
    /// If true, apply even if the change is classified as breaking.
    #[serde(default)]
    pub force: bool,
    /// If true, check compatibility and return the diff without applying.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Deserialize)]
pub struct PutCollectionTemplateBody {
    pub template: String,
    pub actor: Option<String>,
}

/// Request body for `POST /db/{database}/lifecycle/{collection}/{entity}/transition`.
///
/// Lifecycle transitions are driven by the named lifecycle declared in the
/// collection schema. The body carries the lifecycle name, the target state,
/// an OCC guard (`expected_version`), and optional audit metadata. The
/// `actor` defaults to the authenticated identity when omitted.
#[derive(Deserialize)]
pub struct TransitionLifecycleBody {
    /// Name of the lifecycle declared in the collection schema.
    pub lifecycle_name: String,
    /// State the caller wants to transition to.
    pub target_state: String,
    /// The version the caller believes is current (OCC guard).
    pub expected_version: u64,
    /// Optional override for the audit-log actor — falls back to identity.
    pub actor: Option<String>,
    /// Optional key-value metadata attached to the audit entry.
    #[serde(default)]
    pub audit_metadata: Option<std::collections::HashMap<String, String>>,
}

fn parse_collection_template_request(
    headers: &HeaderMap,
    body: Bytes,
) -> Result<PutCollectionTemplateBody, AxonError> {
    if body.is_empty() {
        return Err(AxonError::InvalidArgument(
            "template body must not be empty".into(),
        ));
    }

    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();

    if content_type.starts_with("application/json") {
        serde_json::from_slice::<PutCollectionTemplateBody>(&body).map_err(|error| {
            AxonError::InvalidArgument(format!("invalid template JSON body: {error}"))
        })
    } else {
        let template = std::str::from_utf8(&body).map_err(|error| {
            AxonError::InvalidArgument(format!("template body must be valid UTF-8: {error}"))
        })?;
        Ok(PutCollectionTemplateBody {
            template: template.to_string(),
            actor: None,
        })
    }
}

fn parse_delete_collection_template_request(
    headers: &HeaderMap,
    body: Bytes,
) -> Result<DeleteCollectionTemplateBody, AxonError> {
    if body.is_empty() {
        return Ok(DeleteCollectionTemplateBody::default());
    }

    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();

    if !content_type.starts_with("application/json") {
        return Err(AxonError::InvalidArgument(
            "delete template body must use application/json".into(),
        ));
    }

    serde_json::from_slice::<DeleteCollectionTemplateBody>(&body).map_err(|error| {
        AxonError::InvalidArgument(format!("invalid delete template JSON body: {error}"))
    })
}

// ── Transaction request body ─────────────────────────────────────────────────

/// A single operation within a batch transaction.
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum TransactionOp {
    Create {
        collection: String,
        id: String,
        data: Value,
    },
    Update {
        collection: String,
        id: String,
        data: Value,
        expected_version: u64,
    },
    Patch {
        collection: String,
        id: String,
        patch: Value,
        expected_version: u64,
    },
    Delete {
        collection: String,
        id: String,
        expected_version: u64,
    },
    CreateLink {
        source_collection: String,
        source_id: String,
        target_collection: String,
        target_id: String,
        link_type: String,
        #[serde(default)]
        metadata: Value,
    },
    DeleteLink {
        source_collection: String,
        source_id: String,
        target_collection: String,
        target_id: String,
        link_type: String,
    },
}

/// Request body for `POST /transactions`.
#[derive(Deserialize)]
pub struct TransactionBody {
    pub operations: Vec<TransactionOp>,
    pub idempotency_key: Option<String>,
    pub actor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TraverseBody {
    pub link_type: Option<String>,
    pub max_depth: Option<usize>,
    #[serde(default)]
    pub direction: TraverseDirection,
    pub hop_filter: Option<FilterNode>,
}

#[derive(Debug, Deserialize)]
struct SchemaManifestQuery {
    expected_hash: Option<String>,
}

// ── Route handlers ────────────────────────────────────────────────────────────

async fn schema_manifest(
    Extension(handler): Extension<TenantHandler>,
    Extension(current_database): Extension<CurrentDatabase>,
    headers: HeaderMap,
    Query(query): Query<SchemaManifestQuery>,
) -> Response {
    let guard = handler.lock().await;
    let collection_metadata = match list_collections_for_database(&guard, current_database.as_str())
    {
        Ok(collections) => collections,
        Err(error) => return axon_error_response(error),
    };

    let mut collections = Vec::with_capacity(collection_metadata.len());
    for meta in collection_metadata {
        let name = CollectionId::new(&meta.name);
        let description = match guard.describe_collection(DescribeCollectionRequest { name }) {
            Ok(description) => description,
            Err(error) => return axon_error_response(error),
        };
        collections.push(json!({
            "name": description.name,
            "version": description.schema.as_ref().map(|schema| schema.version),
            "entity_count": description.entity_count,
            "schema": description.schema,
        }));
    }

    let manifest_for_hash = json!({
        "database": current_database.as_str(),
        "collections": collections,
    });
    let schema_hash = match stable_json_hash(&manifest_for_hash) {
        Ok(hash) => hash,
        Err(error) => return axon_error_response(error),
    };

    let mut manifest = manifest_for_hash;
    manifest["schema_hash"] = json!(schema_hash);
    manifest["expected_header"] = json!(SCHEMA_HASH_HEADER);
    manifest["compatibility"] = json!({
        "additive_changes": "compatible",
        "breaking_changes": "rejected_without_force",
        "client_policy": "static clients should compare schema_hash on app load and fail closed on mismatch",
    });

    let expected = headers
        .get(SCHEMA_HASH_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .or(query.expected_hash);

    if let Some(expected) = expected {
        if expected != schema_hash {
            let mut response = (
                StatusCode::CONFLICT,
                Json(ApiError::new(
                    "schema_mismatch",
                    json!({
                        "expected": expected,
                        "actual": schema_hash,
                        "manifest": manifest,
                    }),
                )),
            )
                .into_response();
            if let Ok(value) = HeaderValue::from_str(&schema_hash) {
                response.headers_mut().insert(SCHEMA_HASH_HEADER, value);
            }
            return response;
        }
    }

    let mut response = Json(manifest).into_response();
    if let Ok(value) = HeaderValue::from_str(&schema_hash) {
        response.headers_mut().insert(SCHEMA_HASH_HEADER, value);
    }
    response
}

#[allow(clippy::too_many_arguments)]
async fn create_entity(
    Extension(handler): Extension<TenantHandler>,
    Extension(mcp_sessions): Extension<McpHttpSessions>,
    Extension(broker): Extension<BroadcastBroker>,
    Extension(current_tenant): Extension<CurrentTenant>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(identity): Extension<Identity>,
    Extension(caller): Extension<CoreCallerIdentity>,
    Extension(rate_limiter): Extension<WriteRateLimiter>,
    Extension(actor_scope): Extension<ActorScopeGuard>,
    jwt_identity: Option<Extension<ResolvedIdentity>>,
    Path(CollectionEntityPath { collection, id }): Path<CollectionEntityPath>,
    Json(body): Json<CreateEntityBody>,
) -> Response {
    if let Err(e) = identity.require_write() {
        return auth_error_response(e);
    }
    if let Err(e) = actor_scope.check(&identity.actor, &collection, &identity.role) {
        return auth_error_response(e);
    }
    if let Err(limited) = rate_limiter.check(&identity.actor).await {
        return rate_limit_response(&limited);
    }
    let attribution = attribution_from_jwt(jwt_identity);
    let mut guard = handler.lock().await;
    // HTTP POST /entities is a create-or-replace/upsert surface per
    // create-semantics.md Pattern B. Duplicate rejection is reserved for typed
    // GraphQL createXxx and transaction op:create paths.
    let result = guard.create_entity_with_caller(
        CreateEntityRequest {
            collection: qualify_collection_name(&collection, &current_database),
            id: EntityId::new(&id),
            data: body.data,
            actor: None,
            audit_metadata: None,
            attribution: None,
        },
        &caller,
        attribution,
    );
    match result {
        Ok(resp) => {
            notify_entity_change(&mcp_sessions, &current_database, &resp.entity);
            broadcast_entity_change(
                &broker,
                &resp.entity,
                "create",
                audit_id_string(resp.audit_id),
                &caller.actor,
                &current_tenant,
                &current_database,
            );
            (
                StatusCode::CREATED,
                Json(json!({
                    "entity": entity_payload(&resp.entity)
                })),
            )
                .into_response()
        }
        Err(e) => axon_error_response(e),
    }
}

async fn get_entity(
    Extension(handler): Extension<TenantHandler>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(caller): Extension<CoreCallerIdentity>,
    Path(CollectionEntityPath { collection, id }): Path<CollectionEntityPath>,
) -> Response {
    match handler.lock().await.get_entity_with_caller(
        GetEntityRequest {
            collection: qualify_collection_name(&collection, &current_database),
            id: EntityId::new(&id),
        },
        &caller,
        None,
    ) {
        Ok(resp) => Json(json!({
            "entity": entity_payload(&resp.entity)
        }))
        .into_response(),
        Err(e) => axon_error_response(e),
    }
}

async fn get_collection_entity(
    Extension(handler): Extension<TenantHandler>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(caller): Extension<CoreCallerIdentity>,
    Path(CollectionEntityPath { collection, id }): Path<CollectionEntityPath>,
    Query(params): Query<GetEntityParams>,
) -> Response {
    let collection_id = qualify_collection_name(&collection, &current_database);
    let entity_id = EntityId::new(&id);

    match params.format.as_deref() {
        Some("markdown") => match handler.lock().await.get_entity_markdown_with_caller(
            &collection_id,
            &entity_id,
            &caller,
            None,
        ) {
            Ok(GetEntityMarkdownResponse::Rendered {
                rendered_markdown, ..
            }) => (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/markdown; charset=utf-8")],
                rendered_markdown,
            )
                .into_response(),
            Ok(GetEntityMarkdownResponse::RenderFailed { entity, detail }) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "code": "storage_error",
                    "detail": detail,
                    "entity": entity_payload(&entity),
                })),
            )
                .into_response(),
            Err(e) => axon_error_response(e),
        },
        Some(other) => axon_error_response(AxonError::InvalidArgument(format!(
            "unsupported format '{other}'; expected 'markdown'"
        ))),
        None => match handler.lock().await.get_entity_with_caller(
            GetEntityRequest {
                collection: collection_id,
                id: entity_id,
            },
            &caller,
            None,
        ) {
            Ok(resp) => Json(json!({
                "entity": entity_payload(&resp.entity)
            }))
            .into_response(),
            Err(e) => axon_error_response(e),
        },
    }
}

#[allow(clippy::too_many_arguments)]
async fn update_entity(
    Extension(handler): Extension<TenantHandler>,
    Extension(mcp_sessions): Extension<McpHttpSessions>,
    Extension(broker): Extension<BroadcastBroker>,
    Extension(current_tenant): Extension<CurrentTenant>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(identity): Extension<Identity>,
    Extension(caller): Extension<CoreCallerIdentity>,
    Extension(rate_limiter): Extension<WriteRateLimiter>,
    Extension(actor_scope): Extension<ActorScopeGuard>,
    jwt_identity: Option<Extension<ResolvedIdentity>>,
    Path(CollectionEntityPath { collection, id }): Path<CollectionEntityPath>,
    Json(body): Json<UpdateEntityBody>,
) -> Response {
    if let Err(e) = identity.require_write() {
        return auth_error_response(e);
    }
    if let Err(e) = actor_scope.check(&identity.actor, &collection, &identity.role) {
        return auth_error_response(e);
    }
    if let Err(limited) = rate_limiter.check(&identity.actor).await {
        return rate_limit_response(&limited);
    }
    let attribution = attribution_from_jwt(jwt_identity);
    let mut guard = handler.lock().await;
    let result = guard.update_entity_with_caller(
        UpdateEntityRequest {
            collection: qualify_collection_name(&collection, &current_database),
            id: EntityId::new(&id),
            data: body.data,
            expected_version: body.expected_version,
            actor: None,
            audit_metadata: None,
            attribution: None,
        },
        &caller,
        attribution,
    );
    match result {
        Ok(resp) => {
            notify_entity_change(&mcp_sessions, &current_database, &resp.entity);
            broadcast_entity_change(
                &broker,
                &resp.entity,
                "update",
                audit_id_string(resp.audit_id),
                &caller.actor,
                &current_tenant,
                &current_database,
            );
            Json(json!({
                "entity": entity_payload(&resp.entity)
            }))
            .into_response()
        }
        Err(e) => axon_error_response(e),
    }
}

#[allow(clippy::too_many_arguments)]
async fn delete_entity(
    Extension(handler): Extension<TenantHandler>,
    Extension(mcp_sessions): Extension<McpHttpSessions>,
    Extension(broker): Extension<BroadcastBroker>,
    Extension(current_tenant): Extension<CurrentTenant>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(identity): Extension<Identity>,
    Extension(caller): Extension<CoreCallerIdentity>,
    Extension(rate_limiter): Extension<WriteRateLimiter>,
    Extension(actor_scope): Extension<ActorScopeGuard>,
    jwt_identity: Option<Extension<ResolvedIdentity>>,
    Path(CollectionEntityPath { collection, id }): Path<CollectionEntityPath>,
    _body: Option<Json<DeleteEntityBody>>,
) -> Response {
    if let Err(e) = identity.require_write() {
        return auth_error_response(e);
    }
    if let Err(e) = actor_scope.check(&identity.actor, &collection, &identity.role) {
        return auth_error_response(e);
    }
    if let Err(limited) = rate_limiter.check(&identity.actor).await {
        return rate_limit_response(&limited);
    }
    let attribution = attribution_from_jwt(jwt_identity);
    let mut guard = handler.lock().await;
    let result = guard.delete_entity_with_caller(
        DeleteEntityRequest {
            collection: qualify_collection_name(&collection, &current_database),
            id: EntityId::new(&id),
            actor: None,
            audit_metadata: None,
            force: false,
            attribution: None,
        },
        &caller,
        attribution,
    );
    match result {
        Ok(resp) => {
            notify_entity_change_by_parts(&mcp_sessions, &current_database, &collection, &id);
            broadcast_entity_delete(
                &broker,
                &collection,
                &id,
                audit_id_string(resp.audit_id),
                &caller.actor,
                &current_tenant,
                &current_database,
            );
            Json(json!({"collection": resp.collection, "id": resp.id})).into_response()
        }
        Err(e) => axon_error_response(e),
    }
}

async fn query_entities(
    Extension(handler): Extension<TenantHandler>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(caller): Extension<CoreCallerIdentity>,
    Path(CollectionPath { collection }): Path<CollectionPath>,
    Json(body): Json<QueryEntitiesRequest>,
) -> Response {
    // Allow the caller to omit the collection field in the body; the path wins.
    let req = QueryEntitiesRequest {
        collection: qualify_collection_name(&collection, &current_database),
        ..body
    };
    match handler
        .lock()
        .await
        .query_entities_with_caller(req, &caller, None)
    {
        Ok(resp) => {
            let entities: Vec<Value> = resp
                .entities
                .iter()
                .map(|e| {
                    json!({
                        "collection": e.collection.to_string(),
                        "id": e.id.to_string(),
                        "version": e.version,
                        "data": e.data,
                    })
                })
                .collect();
            Json(json!({
                "entities": entities,
                "total_count": resp.total_count,
                "next_cursor": resp.next_cursor,
                "policy_plan": resp.policy_plan,
            }))
            .into_response()
        }
        Err(e) => axon_error_response(e),
    }
}

/// `POST /snapshot` (and `POST /db/{database}/snapshot`) — atomic multi-collection
/// snapshot with audit cursor (US-080, FEAT-004).
///
/// Request body is a [`SnapshotRequest`] JSON document. Collections named in
/// `collections` are rewritten into the current database namespace (so callers
/// can pass bare collection names). Response is a `SnapshotResponse` JSON
/// document.
async fn snapshot_entities_handler(
    Extension(handler): Extension<TenantHandler>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(identity): Extension<Identity>,
    Json(body): Json<SnapshotRequest>,
) -> Response {
    if let Err(e) = identity.require_read() {
        return auth_error_response(e);
    }

    let collections = body.collections.as_ref().map(|cols| {
        cols.iter()
            .map(|c| qualify_collection_name(c.as_str(), &current_database))
            .collect::<Vec<_>>()
    });

    let req = SnapshotRequest {
        collections,
        limit: body.limit,
        after_page_token: body.after_page_token,
    };

    match handler.lock().await.snapshot_entities(req) {
        Ok(resp) => {
            let entities: Vec<Value> = resp
                .entities
                .iter()
                .map(|e| {
                    json!({
                        "collection": e.collection.to_string(),
                        "id": e.id.to_string(),
                        "version": e.version,
                        "data": e.data,
                    })
                })
                .collect();
            Json(json!({
                "entities": entities,
                "audit_cursor": resp.audit_cursor,
                "next_page_token": resp.next_page_token,
            }))
            .into_response()
        }
        Err(e) => axon_error_response(e),
    }
}

/// `POST /db/{database}/lifecycle/{collection}/{entity}/transition` (and the
/// un-prefixed `POST /lifecycle/{collection}/{entity}/transition`) — drive a
/// schema-declared lifecycle transition against an entity (FEAT-015).
///
/// The body carries the lifecycle name, target state, and OCC guard. Errors
/// map through [`axon_error_response`]:
/// - [`AxonError::LifecycleNotFound`] -> `404 lifecycle_not_found`
/// - [`AxonError::InvalidTransition`] -> `422 invalid_transition`
/// - [`AxonError::ConflictingVersion`] -> `409 version_conflict`
#[allow(clippy::too_many_arguments)]
async fn transition_lifecycle_handler(
    Extension(handler): Extension<TenantHandler>,
    Extension(mcp_sessions): Extension<McpHttpSessions>,
    Extension(broker): Extension<BroadcastBroker>,
    Extension(current_tenant): Extension<CurrentTenant>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(identity): Extension<Identity>,
    Extension(caller): Extension<CoreCallerIdentity>,
    Extension(rate_limiter): Extension<WriteRateLimiter>,
    Extension(actor_scope): Extension<ActorScopeGuard>,
    jwt_identity: Option<Extension<ResolvedIdentity>>,
    Path(CollectionEntityPath { collection, id }): Path<CollectionEntityPath>,
    Json(body): Json<TransitionLifecycleBody>,
) -> Response {
    if let Err(e) = identity.require_write() {
        return auth_error_response(e);
    }
    if let Err(e) = actor_scope.check(&identity.actor, &collection, &identity.role) {
        return auth_error_response(e);
    }
    if let Err(limited) = rate_limiter.check(&identity.actor).await {
        return rate_limit_response(&limited);
    }

    // Body-level `actor` is ignored in favor of the transport-resolved caller
    // (FEAT-012); `_with_caller` overrides `req.actor` with `caller.actor`.
    let _ = body.actor;

    let attribution = attribution_from_jwt(jwt_identity);
    let mut guard = handler.lock().await;
    let result = guard.transition_lifecycle_with_caller(
        TransitionLifecycleRequest {
            collection_id: qualify_collection_name(&collection, &current_database),
            entity_id: EntityId::new(&id),
            lifecycle_name: body.lifecycle_name,
            target_state: body.target_state,
            expected_version: body.expected_version,
            actor: None,
            audit_metadata: body.audit_metadata,
            attribution: None,
        },
        &caller,
        attribution,
    );
    match result {
        Ok(resp) => {
            notify_entity_change(&mcp_sessions, &current_database, &resp.entity);
            broadcast_entity_change(
                &broker,
                &resp.entity,
                "update",
                audit_id_string(resp.audit_id),
                &caller.actor,
                &current_tenant,
                &current_database,
            );
            Json(json!({
                "entity": entity_payload(&resp.entity)
            }))
            .into_response()
        }
        Err(e) => axon_error_response(e),
    }
}

#[allow(clippy::too_many_arguments)]
async fn create_link(
    Extension(handler): Extension<TenantHandler>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(identity): Extension<Identity>,
    Extension(caller): Extension<CoreCallerIdentity>,
    Extension(rate_limiter): Extension<WriteRateLimiter>,
    Extension(actor_scope): Extension<ActorScopeGuard>,
    jwt_identity: Option<Extension<ResolvedIdentity>>,
    Json(body): Json<CreateLinkBody>,
) -> Response {
    if let Err(e) = identity.require_write() {
        return auth_error_response(e);
    }
    if let Err(e) = actor_scope.check(&identity.actor, &body.source_collection, &identity.role) {
        return auth_error_response(e);
    }
    if let Err(limited) = rate_limiter.check(&identity.actor).await {
        return rate_limit_response(&limited);
    }
    let attribution = attribution_from_jwt(jwt_identity);
    let mut guard = handler.lock().await;
    let result = guard.create_link_with_caller(
        CreateLinkRequest {
            source_collection: qualify_collection_name(&body.source_collection, &current_database),
            source_id: EntityId::new(&body.source_id),
            target_collection: qualify_collection_name(&body.target_collection, &current_database),
            target_id: EntityId::new(&body.target_id),
            link_type: body.link_type,
            metadata: body.metadata,
            actor: None,
            attribution: None,
        },
        &caller,
        attribution,
    );
    match result {
        Ok(resp) => {
            let link = resp.link;
            (
                StatusCode::CREATED,
                Json(json!({
                    "link": {
                        "source_collection": link.source_collection.to_string(),
                        "source_id": link.source_id.to_string(),
                        "target_collection": link.target_collection.to_string(),
                        "target_id": link.target_id.to_string(),
                        "link_type": link.link_type,
                        "metadata": link.metadata,
                    }
                })),
            )
                .into_response()
        }
        Err(e) => axon_error_response(e),
    }
}

#[allow(clippy::too_many_arguments)]
async fn delete_link(
    Extension(handler): Extension<TenantHandler>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(identity): Extension<Identity>,
    Extension(caller): Extension<CoreCallerIdentity>,
    Extension(rate_limiter): Extension<WriteRateLimiter>,
    Extension(actor_scope): Extension<ActorScopeGuard>,
    jwt_identity: Option<Extension<ResolvedIdentity>>,
    Json(body): Json<DeleteLinkBody>,
) -> Response {
    if let Err(e) = identity.require_write() {
        return auth_error_response(e);
    }
    if let Err(e) = actor_scope.check(&identity.actor, &body.source_collection, &identity.role) {
        return auth_error_response(e);
    }
    if let Err(limited) = rate_limiter.check(&identity.actor).await {
        return rate_limit_response(&limited);
    }
    let attribution = attribution_from_jwt(jwt_identity);
    let mut guard = handler.lock().await;
    let result = guard.delete_link_with_caller(
        DeleteLinkRequest {
            source_collection: qualify_collection_name(&body.source_collection, &current_database),
            source_id: EntityId::new(&body.source_id),
            target_collection: qualify_collection_name(&body.target_collection, &current_database),
            target_id: EntityId::new(&body.target_id),
            link_type: body.link_type,
            actor: None,
            attribution: None,
        },
        &caller,
        attribution,
    );
    match result {
        Ok(resp) => Json(json!({
            "source_collection": resp.source_collection,
            "source_id": resp.source_id,
            "target_collection": resp.target_collection,
            "target_id": resp.target_id,
            "link_type": resp.link_type,
        }))
        .into_response(),
        Err(e) => axon_error_response(e),
    }
}

async fn traverse(
    Extension(handler): Extension<TenantHandler>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(caller): Extension<CoreCallerIdentity>,
    Path(CollectionEntityPath { collection, id }): Path<CollectionEntityPath>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let link_type = params.get("link_type").cloned();
    let max_depth = params.get("max_depth").and_then(|s| s.parse().ok());
    let direction = match params.get("direction").map(|s| s.as_str()) {
        Some("reverse") => TraverseDirection::Reverse,
        _ => TraverseDirection::Forward,
    };
    let hop_filter = match params.get("filter").or_else(|| params.get("hop_filter")) {
        Some(raw) => match serde_json::from_str::<FilterNode>(raw) {
            Ok(filter) => Some(filter),
            Err(error) => {
                return axon_error_response(AxonError::InvalidArgument(format!(
                    "invalid traversal filter JSON: {error}"
                )));
            }
        },
        None => None,
    };

    traverse_with_request(
        handler,
        current_database,
        caller,
        collection,
        id,
        TraverseBody {
            link_type,
            max_depth,
            direction,
            hop_filter,
        },
    )
    .await
}

async fn traverse_post(
    Extension(handler): Extension<TenantHandler>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(caller): Extension<CoreCallerIdentity>,
    Path(CollectionEntityPath { collection, id }): Path<CollectionEntityPath>,
    Json(body): Json<TraverseBody>,
) -> Response {
    traverse_with_request(handler, current_database, caller, collection, id, body).await
}

async fn traverse_with_request(
    handler: TenantHandler,
    current_database: CurrentDatabase,
    caller: CoreCallerIdentity,
    collection: String,
    id: String,
    body: TraverseBody,
) -> Response {
    match handler.lock().await.traverse_with_caller(
        TraverseRequest {
            collection: qualify_collection_name(&collection, &current_database),
            id: EntityId::new(&id),
            link_type: body.link_type,
            max_depth: body.max_depth,
            direction: body.direction,
            hop_filter: body.hop_filter,
        },
        &caller,
        None,
    ) {
        Ok(resp) => {
            let entities: Vec<Value> = resp
                .entities
                .iter()
                .map(|e| {
                    json!({
                        "collection": e.collection.to_string(),
                        "id": e.id.to_string(),
                        "version": e.version,
                        "data": e.data,
                    })
                })
                .collect();
            let paths: Vec<Value> = resp
                .paths
                .iter()
                .flat_map(|p| {
                    p.hops.iter().map(|hop| {
                        json!({
                            "source_collection": hop.link.source_collection.to_string(),
                            "source_id": hop.link.source_id.to_string(),
                            "target_collection": hop.link.target_collection.to_string(),
                            "target_id": hop.link.target_id.to_string(),
                            "link_type": hop.link.link_type,
                            "metadata": hop.link.metadata,
                        })
                    })
                })
                .collect();
            Json(json!({ "entities": entities, "paths": paths })).into_response()
        }
        Err(e) => axon_error_response(e),
    }
}

async fn query_audit_by_entity(
    Extension(handler): Extension<TenantHandler>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(requested_database_scope): Extension<RequestedDatabaseScope>,
    Extension(caller): Extension<CoreCallerIdentity>,
    Path(CollectionEntityPath {
        collection,
        id: entity_id,
    }): Path<CollectionEntityPath>,
) -> Response {
    let handler = handler.lock().await;
    match handler.query_audit_with_caller(
        QueryAuditRequest {
            database: requested_database_scope.database().map(str::to_string),
            collection: Some(qualify_collection_name(&collection, &current_database)),
            entity_id: Some(EntityId::new(&entity_id)),
            ..Default::default()
        },
        &caller,
        None,
    ) {
        Ok(resp) => {
            let entries =
                filter_audit_entries_to_database(resp.entries, requested_database_scope.database());
            let proto: Vec<Value> = entries
                .iter()
                .map(|e: &axon_audit::AuditEntry| {
                    json!({
                        "id": e.id,
                        "timestamp_ns": e.timestamp_ns,
                        "collection": e.collection.to_string(),
                        "entity_id": e.entity_id.to_string(),
                        "version": e.version,
                        "mutation": e.mutation.to_string(),
                        "operation": e.mutation.to_string(),
                        "data_before": e.data_before,
                        "data_after": e.data_after,
                        "diff": &e.diff,
                        "actor": e.actor,
                        "metadata": &e.metadata,
                        "transaction_id": e.transaction_id,
                        "intent_lineage": &e.intent_lineage,
                    })
                })
                .collect();
            Json(json!({ "entries": proto })).into_response()
        }
        Err(e) => axon_error_response(e),
    }
}

/// Parse the `?collections=name1,name2` query parameter into a `Vec<CollectionId>`,
/// qualifying each name with the current database scope. Missing or empty values yield an
/// empty vec, which `AuditQuery` interprets as "no multi-collection filter" (FEAT-003 US-079).
fn parse_audit_collections_param(
    params: &std::collections::HashMap<String, String>,
    current_database: &CurrentDatabase,
) -> Vec<CollectionId> {
    params
        .get("collections")
        .map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(|part| qualify_collection_name(part, current_database))
                .collect()
        })
        .unwrap_or_default()
}

async fn query_audit(
    Extension(handler): Extension<TenantHandler>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(requested_database_scope): Extension<RequestedDatabaseScope>,
    Extension(caller): Extension<CoreCallerIdentity>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    if let Some(filter) = unsupported_audit_filter_param(&params) {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                "unsupported_audit_filter",
                json!({
                    "filter": filter,
                    "supported_filters": [
                        "collection",
                        "collections",
                        "entity_id",
                        "actor",
                        "operation",
                        "intent_id",
                        "approval_id",
                        "since_ns",
                        "until_ns",
                        "after_id",
                        "limit"
                    ],
                }),
            )),
        )
            .into_response();
    }

    let req = QueryAuditRequest {
        database: requested_database_scope.database().map(str::to_string),
        collection: params
            .get("collection")
            .map(|collection| qualify_collection_name(collection, &current_database)),
        collection_ids: parse_audit_collections_param(&params, &current_database),
        entity_id: params.get("entity_id").map(EntityId::new),
        actor: params.get("actor").cloned(),
        operation: params.get("operation").cloned(),
        intent_id: params.get("intent_id").cloned(),
        approval_id: params.get("approval_id").cloned(),
        since_ns: params.get("since_ns").and_then(|s| s.parse().ok()),
        until_ns: params.get("until_ns").and_then(|s| s.parse().ok()),
        after_id: params.get("after_id").and_then(|s| s.parse().ok()),
        limit: params.get("limit").and_then(|s| s.parse().ok()),
    };
    match handler
        .lock()
        .await
        .query_audit_with_caller(req, &caller, None)
    {
        Ok(resp) => {
            let next_cursor = resp.next_cursor;
            let entries =
                filter_audit_entries_to_database(resp.entries, requested_database_scope.database());
            let proto: Vec<Value> = entries
                .iter()
                .map(|e: &axon_audit::AuditEntry| {
                    json!({
                        "id": e.id,
                        "timestamp_ns": e.timestamp_ns,
                        "collection": e.collection.to_string(),
                        "entity_id": e.entity_id.to_string(),
                        "version": e.version,
                        "mutation": e.mutation.to_string(),
                        "operation": e.mutation.to_string(),
                        "data_before": e.data_before,
                        "data_after": e.data_after,
                        "diff": &e.diff,
                        "actor": e.actor,
                        "metadata": &e.metadata,
                        "transaction_id": e.transaction_id,
                        "intent_lineage": &e.intent_lineage,
                    })
                })
                .collect();
            Json(json!({ "entries": proto, "next_cursor": next_cursor })).into_response()
        }
        Err(e) => axon_error_response(e),
    }
}

fn unsupported_audit_filter_param(
    params: &std::collections::HashMap<String, String>,
) -> Option<String> {
    params.keys().find_map(|key| {
        let normalized = key.replace('-', "_").to_ascii_lowercase();
        if normalized.starts_with("metadata")
            || normalized.starts_with("data_after")
            || normalized.starts_with("dataafter")
        {
            Some(key.clone())
        } else {
            None
        }
    })
}

#[allow(clippy::too_many_arguments)]
async fn revert_entity(
    Extension(handler): Extension<TenantHandler>,
    Extension(mcp_sessions): Extension<McpHttpSessions>,
    Extension(broker): Extension<BroadcastBroker>,
    Extension(current_tenant): Extension<CurrentTenant>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(identity): Extension<Identity>,
    Extension(rate_limiter): Extension<WriteRateLimiter>,
    jwt_identity: Option<Extension<ResolvedIdentity>>,
    Json(body): Json<RevertEntityBody>,
) -> Response {
    if let Err(e) = identity.require_write() {
        return auth_error_response(e);
    }
    if let Err(limited) = rate_limiter.check(&identity.actor).await {
        return rate_limit_response(&limited);
    }
    let attribution = attribution_from_jwt(jwt_identity);
    let actor = identity.actor.clone();
    let mut guard = handler.lock().await;
    let result = guard.revert_entity_to_audit_entry(RevertEntityRequest {
        audit_entry_id: body.audit_entry_id,
        actor: Some(actor),
        force: body.force,
        attribution,
    });
    match result {
        Ok(resp) => {
            notify_entity_change(&mcp_sessions, &current_database, &resp.entity);
            broadcast_entity_change(
                &broker,
                &resp.entity,
                "update",
                resp.audit_entry.id.to_string(),
                &identity.actor,
                &current_tenant,
                &current_database,
            );
            Json(json!({
                "entity": entity_payload(&resp.entity),
                "audit_entry_id": resp.audit_entry.id,
            }))
            .into_response()
        }
        Err(e) => axon_error_response(e),
    }
}

fn rollback_error_response(err: AxonError) -> Response {
    match err {
        AxonError::SchemaValidation(detail) => (
            StatusCode::CONFLICT,
            Json(ApiError::new(
                "schema_validation",
                axon_core::error::schema_validation_detail(&detail),
            )),
        )
            .into_response(),
        other => axon_error_response(other),
    }
}

fn rollback_target_from_body(body: &RollbackEntityBody) -> Result<RollbackEntityTarget, AxonError> {
    match (&body.to_version, &body.to_audit_id) {
        (Some(version), None) => Ok(RollbackEntityTarget::Version(*version)),
        (None, Some(audit_id)) => {
            let parsed = audit_id.parse::<u64>().map_err(|error| {
                AxonError::InvalidArgument(format!("invalid to_audit_id '{}': {error}", audit_id))
            })?;
            Ok(RollbackEntityTarget::AuditEntryId(parsed))
        }
        (Some(_), Some(_)) => Err(AxonError::InvalidArgument(
            "provide exactly one of 'to_version' or 'to_audit_id'".into(),
        )),
        (None, None) => Err(AxonError::InvalidArgument(
            "one of 'to_version' or 'to_audit_id' is required".into(),
        )),
    }
}

#[allow(clippy::too_many_arguments)]
async fn rollback_collection_entity(
    Extension(handler): Extension<TenantHandler>,
    Extension(mcp_sessions): Extension<McpHttpSessions>,
    Extension(broker): Extension<BroadcastBroker>,
    Extension(current_tenant): Extension<CurrentTenant>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(identity): Extension<Identity>,
    Extension(rate_limiter): Extension<WriteRateLimiter>,
    Extension(actor_scope): Extension<ActorScopeGuard>,
    Path(CollectionEntityPath { collection, id }): Path<CollectionEntityPath>,
    Json(body): Json<RollbackEntityBody>,
) -> Response {
    if let Err(e) = identity.require_write() {
        return auth_error_response(e);
    }
    if let Err(e) = actor_scope.check(&identity.actor, &collection, &identity.role) {
        return auth_error_response(e);
    }
    if let Err(limited) = rate_limiter.check(&identity.actor).await {
        return rate_limit_response(&limited);
    }
    let target = match rollback_target_from_body(&body) {
        Ok(target) => target,
        Err(error) => return axon_error_response(error),
    };

    let actor = identity.actor.clone();
    match handler.lock().await.rollback_entity(RollbackEntityRequest {
        collection: qualify_collection_name(&collection, &current_database),
        id: EntityId::new(&id),
        target,
        expected_version: body.expected_version,
        actor: Some(actor.clone()),
        dry_run: body.dry_run,
    }) {
        Ok(axon_api::response::RollbackEntityResponse::Applied {
            entity,
            audit_entry,
        }) => {
            notify_entity_change(&mcp_sessions, &current_database, &entity);
            broadcast_entity_change(
                &broker,
                &entity,
                "update",
                audit_entry.id.to_string(),
                &actor,
                &current_tenant,
                &current_database,
            );
            Json(json!({
                "entity": entity_payload(&entity),
                "audit_entry": audit_entry_payload(&audit_entry),
            }))
            .into_response()
        }
        Ok(axon_api::response::RollbackEntityResponse::DryRun {
            current,
            target,
            diff,
        }) => Json(json!({
            "current": current.as_ref().map(entity_payload),
            "target": entity_payload(&target),
            "diff": diff,
        }))
        .into_response(),
        Err(error) => rollback_error_response(error),
    }
}

// ── Collection-level rollback ────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct RollbackCollectionBody {
    /// ISO 8601 timestamp *or* nanoseconds since Unix epoch.
    pub timestamp: String,
    #[serde(default)]
    pub dry_run: bool,
}

/// Parse a timestamp string into nanoseconds since Unix epoch.
///
/// Accepts:
/// - Raw nanosecond integer as a string (e.g. `"1712750400000000000"`)
/// - RFC 3339 / ISO 8601 with `Z` suffix (e.g. `"2026-04-10T12:00:00Z"`)
/// - RFC 3339 with fractional seconds (e.g. `"2026-04-10T12:00:00.123456789Z"`)
fn parse_timestamp_ns(input: &str) -> Result<u64, AxonError> {
    // Try parsing as a raw u64 first (nanoseconds since epoch).
    if let Ok(ns) = input.parse::<u64>() {
        return Ok(ns);
    }

    // Minimal RFC 3339 parser: YYYY-MM-DDThh:mm:ss[.frac]Z
    // Only UTC (Z suffix) is supported.
    let err = || {
        AxonError::InvalidArgument(format!(
            "invalid timestamp '{}': expected RFC 3339 (UTC) or nanoseconds since epoch",
            input
        ))
    };

    let s = input.trim();
    if !s.ends_with('Z') && !s.ends_with('z') {
        return Err(err());
    }
    let s = &s[..s.len() - 1]; // strip trailing Z

    let (datetime_part, frac_ns) = if let Some(dot_pos) = s.find('.') {
        let frac_str = &s[dot_pos + 1..];
        // Pad or truncate to 9 digits for nanoseconds.
        let padded: String = if frac_str.len() >= 9 {
            frac_str[..9].to_string()
        } else {
            format!("{:0<9}", frac_str)
        };
        let ns: u64 = padded.parse().map_err(|_| err())?;
        (&s[..dot_pos], ns)
    } else {
        (s, 0u64)
    };

    // Parse "YYYY-MM-DDThh:mm:ss"
    let parts: Vec<&str> = datetime_part.split('T').collect();
    if parts.len() != 2 {
        return Err(err());
    }
    let date_parts: Vec<u64> = parts[0]
        .split('-')
        .map(|p| p.parse::<u64>().map_err(|_| err()))
        .collect::<Result<Vec<_>, _>>()?;
    let time_parts: Vec<u64> = parts[1]
        .split(':')
        .map(|p| p.parse::<u64>().map_err(|_| err()))
        .collect::<Result<Vec<_>, _>>()?;

    if date_parts.len() != 3 || time_parts.len() != 3 {
        return Err(err());
    }

    let (year, month, day) = (date_parts[0], date_parts[1], date_parts[2]);
    let (hour, minute, second) = (time_parts[0], time_parts[1], time_parts[2]);

    // Convert to days since Unix epoch using a simple calendar calculation.
    let days = days_since_epoch(year, month, day).ok_or_else(err)?;
    let total_seconds = days * 86400 + hour * 3600 + minute * 60 + second;
    Ok(total_seconds * 1_000_000_000 + frac_ns)
}

/// Compute days since 1970-01-01 for a given date.
fn days_since_epoch(year: u64, month: u64, day: u64) -> Option<u64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || year < 1970 {
        return None;
    }
    // Cumulative days before each month (non-leap year).
    const MONTH_DAYS: [u64; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let mut days: u64 = 0;
    for y in 1970..year {
        days += if is_leap(y) { 366 } else { 365 };
    }
    days += MONTH_DAYS[(month - 1) as usize];
    if month > 2 && is_leap(year) {
        days += 1;
    }
    days += day - 1;
    Some(days)
}

fn is_leap(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

async fn rollback_collection_handler(
    Extension(handler): Extension<TenantHandler>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(identity): Extension<Identity>,
    Extension(actor_scope): Extension<ActorScopeGuard>,
    Path(CollectionPath { collection }): Path<CollectionPath>,
    Json(body): Json<RollbackCollectionBody>,
) -> Response {
    if let Err(e) = identity.require_write() {
        return auth_error_response(e);
    }
    if let Err(e) = actor_scope.check(&identity.actor, &collection, &identity.role) {
        return auth_error_response(e);
    }
    let timestamp_ns = match parse_timestamp_ns(&body.timestamp) {
        Ok(ns) => ns,
        Err(e) => return axon_error_response(e),
    };

    match handler
        .lock()
        .await
        .rollback_collection(RollbackCollectionRequest {
            collection: qualify_collection_name(&collection, &current_database),
            timestamp_ns,
            actor: Some(identity.actor),
            dry_run: body.dry_run,
        }) {
        Ok(resp) => Json(json!({
            "entities_affected": resp.entities_affected,
            "entities_rolled_back": resp.entities_rolled_back,
            "errors": resp.errors,
            "dry_run": resp.dry_run,
            "details": resp.details,
        }))
        .into_response(),
        Err(error) => axon_error_response(error),
    }
}

// ── Transaction-level rollback ────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct RollbackTransactionBody {
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Deserialize)]
pub struct TransactionIdPath {
    pub transaction_id: String,
}

async fn rollback_transaction_handler(
    Extension(handler): Extension<TenantHandler>,
    Extension(identity): Extension<Identity>,
    Extension(rate_limiter): Extension<WriteRateLimiter>,
    Path(TransactionIdPath { transaction_id }): Path<TransactionIdPath>,
    body: Option<Json<RollbackTransactionBody>>,
) -> Response {
    if let Err(e) = identity.require_write() {
        return auth_error_response(e);
    }
    if let Err(limited) = rate_limiter.check(&identity.actor).await {
        return rate_limit_response(&limited);
    }

    let dry_run = body.is_some_and(|Json(b)| b.dry_run);

    match handler
        .lock()
        .await
        .rollback_transaction(RollbackTransactionRequest {
            transaction_id: transaction_id.clone(),
            actor: Some(identity.actor),
            dry_run,
        }) {
        Ok(resp) => Json(json!({
            "transaction_id": resp.transaction_id,
            "entities_affected": resp.entities_affected,
            "entities_rolled_back": resp.entities_rolled_back,
            "errors": resp.errors,
            "dry_run": resp.dry_run,
            "details": resp.details,
        }))
        .into_response(),
        Err(error) => axon_error_response(error),
    }
}

async fn create_collection(
    Extension(handler): Extension<TenantHandler>,
    Extension(mcp_sessions): Extension<McpHttpSessions>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(identity): Extension<Identity>,
    Path(NamePath { name }): Path<NamePath>,
    body: Option<Json<CreateCollectionBody>>,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }
    let schema_body = match body.and_then(|Json(b)| b.schema) {
        Some(schema_body) => schema_body,
        None => {
            return axon_error_response(AxonError::InvalidArgument(
                "'schema' field is required to create a collection".into(),
            ));
        }
    };
    let collection_id = qualify_collection_name(&name, &current_database);
    let schema = CollectionSchema {
        collection: collection_id.clone(),
        description: schema_body.description,
        version: schema_body.version,
        entity_schema: schema_body.entity_schema,
        link_types: schema_body.link_types.unwrap_or_default(),
        access_control: schema_body.access_control,
        gates: schema_body.gates.unwrap_or_default(),
        validation_rules: schema_body.validation_rules.unwrap_or_default(),
        indexes: schema_body.indexes.unwrap_or_default(),
        compound_indexes: schema_body.compound_indexes.unwrap_or_default(),
        queries: Default::default(),
        lifecycles: schema_body.lifecycles.unwrap_or_default(),
    };
    match handler
        .lock()
        .await
        .create_collection(CreateCollectionRequest {
            name: collection_id,
            schema,
            actor: Some(identity.actor),
        }) {
        Ok(resp) => {
            notify_tool_list_changed(&mcp_sessions, &current_database);
            (StatusCode::CREATED, Json(json!({ "name": resp.name }))).into_response()
        }
        Err(e) => axon_error_response(e),
    }
}

async fn drop_collection(
    Extension(handler): Extension<TenantHandler>,
    Extension(mcp_sessions): Extension<McpHttpSessions>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(identity): Extension<Identity>,
    Path(NamePath { name }): Path<NamePath>,
    _body: Option<Json<CollectionActorBody>>,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }
    match handler.lock().await.drop_collection(DropCollectionRequest {
        name: qualify_collection_name(&name, &current_database),
        actor: Some(identity.actor),
        confirm: true,
    }) {
        Ok(resp) => {
            notify_tool_list_changed(&mcp_sessions, &current_database);
            Json(json!({
                "name": resp.name,
                "entities_removed": resp.entities_removed,
            }))
            .into_response()
        }
        Err(e) => axon_error_response(e),
    }
}

async fn list_collections(
    Extension(handler): Extension<TenantHandler>,
    Extension(requested_database_scope): Extension<RequestedDatabaseScope>,
) -> Response {
    let handler = handler.lock().await;
    let collections = match requested_database_scope.database() {
        Some(database) => list_collections_for_database(&handler, database),
        None => handler
            .list_collections(ListCollectionsRequest {})
            .map(|resp| resp.collections),
    };

    match collections {
        Ok(collections) => Json(json!({ "collections": collections })).into_response(),
        Err(e) => axon_error_response(e),
    }
}

async fn describe_collection(
    Extension(handler): Extension<TenantHandler>,
    Extension(current_database): Extension<CurrentDatabase>,
    Path(NamePath { name }): Path<NamePath>,
) -> Response {
    match handler
        .lock()
        .await
        .describe_collection(DescribeCollectionRequest {
            name: qualify_collection_name(&name, &current_database),
        }) {
        Ok(resp) => Json(json!({
            "name": resp.name,
            "entity_count": resp.entity_count,
            "schema": resp.schema,
            "created_at_ns": resp.created_at_ns,
            "updated_at_ns": resp.updated_at_ns,
        }))
        .into_response(),
        Err(e) => axon_error_response(e),
    }
}

async fn put_collection_template(
    Extension(handler): Extension<TenantHandler>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(identity): Extension<Identity>,
    Path(CollectionPath { collection }): Path<CollectionPath>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }
    let body = match parse_collection_template_request(&headers, body) {
        Ok(body) => body,
        Err(error) => return axon_error_response(error),
    };

    match handler
        .lock()
        .await
        .put_collection_template(PutCollectionTemplateRequest {
            collection: qualify_collection_name(&collection, &current_database),
            template: body.template,
            actor: Some(identity.actor),
        }) {
        Ok(resp) => Json(json!({
            "collection": resp.view.collection,
            "template": resp.view.markdown_template,
            "version": resp.view.version,
            "updated_at_ns": resp.view.updated_at_ns,
            "updated_by": resp.view.updated_by,
            "warnings": resp.warnings,
        }))
        .into_response(),
        Err(e) => axon_error_response(e),
    }
}

async fn get_collection_template(
    Extension(handler): Extension<TenantHandler>,
    Extension(current_database): Extension<CurrentDatabase>,
    Path(CollectionPath { collection }): Path<CollectionPath>,
) -> Response {
    match handler
        .lock()
        .await
        .get_collection_template(GetCollectionTemplateRequest {
            collection: qualify_collection_name(&collection, &current_database),
        }) {
        Ok(resp) => Json(json!({
            "collection": resp.view.collection,
            "template": resp.view.markdown_template,
            "version": resp.view.version,
            "updated_at_ns": resp.view.updated_at_ns,
            "updated_by": resp.view.updated_by,
        }))
        .into_response(),
        Err(e) => axon_error_response(e),
    }
}

async fn delete_collection_template(
    Extension(handler): Extension<TenantHandler>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(identity): Extension<Identity>,
    Path(CollectionPath { collection }): Path<CollectionPath>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }
    if let Err(error) = parse_delete_collection_template_request(&headers, body) {
        return axon_error_response(error);
    }
    match handler
        .lock()
        .await
        .delete_collection_template(DeleteCollectionTemplateRequest {
            collection: qualify_collection_name(&collection, &current_database),
            actor: Some(identity.actor),
        }) {
        Ok(resp) => {
            Json(json!({ "collection": resp.collection, "status": "deleted" })).into_response()
        }
        Err(e) => axon_error_response(e),
    }
}

async fn put_schema(
    Extension(handler): Extension<TenantHandler>,
    Extension(mcp_sessions): Extension<McpHttpSessions>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(identity): Extension<Identity>,
    Path(NamePath { name: collection }): Path<NamePath>,
    Json(body): Json<PutSchemaBody>,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }
    // Populate schema from body; collection always comes from the path.
    let schema = CollectionSchema {
        collection: qualify_collection_name(&collection, &current_database),
        description: body.description,
        version: body.version,
        entity_schema: body.entity_schema,
        link_types: body.link_types.unwrap_or_default(),
        access_control: body.access_control,
        gates: body.gates.unwrap_or_default(),
        validation_rules: body.validation_rules.unwrap_or_default(),
        indexes: body.indexes.unwrap_or_default(),
        compound_indexes: body.compound_indexes.unwrap_or_default(),
        queries: Default::default(),
        lifecycles: body.lifecycles.unwrap_or_default(),
    };
    match handler.lock().await.handle_put_schema(PutSchemaRequest {
        schema,
        actor: Some(identity.actor),
        force: body.force,
        dry_run: body.dry_run,
        // REST does not currently accept fixture explain inputs; leave empty.
        explain_inputs: Vec::new(),
    }) {
        Ok(resp) => {
            if !resp.dry_run {
                notify_tool_list_changed(&mcp_sessions, &current_database);
            }
            let mut result = json!({ "schema": resp.schema });
            if let Some(compat) = &resp.compatibility {
                result["compatibility"] = json!(compat);
            }
            if let Some(diff) = &resp.diff {
                result["diff"] = json!(diff);
            }
            if let Some(report) = &resp.policy_compile_report {
                result["policy_compile_report"] = json!(report);
            }
            if resp.dry_run {
                result["dry_run"] = json!(true);
            }
            (StatusCode::OK, Json(result)).into_response()
        }
        Err(AxonError::InvalidOperation(msg)) => (
            StatusCode::CONFLICT,
            Json(ApiError::new("breaking_schema_change", msg)),
        )
            .into_response(),
        Err(e) => axon_error_response(e),
    }
}

async fn get_schema(
    Extension(handler): Extension<TenantHandler>,
    Extension(current_database): Extension<CurrentDatabase>,
    Path(NamePath { name: collection }): Path<NamePath>,
) -> Response {
    match handler.lock().await.handle_get_schema(GetSchemaRequest {
        collection: qualify_collection_name(&collection, &current_database),
    }) {
        Ok(resp) => Json(json!({ "schema": resp.schema })).into_response(),
        Err(e) => axon_error_response(e),
    }
}

async fn create_database(
    Extension(handler): Extension<TenantHandler>,
    Extension(identity): Extension<Identity>,
    Path(name): Path<String>,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }
    match handler
        .lock()
        .await
        .create_database(CreateDatabaseRequest { name })
    {
        Ok(resp) => (StatusCode::CREATED, Json(json!({ "name": resp.name }))).into_response(),
        Err(err) => axon_error_response(err),
    }
}

async fn list_databases(Extension(handler): Extension<TenantHandler>) -> Response {
    match handler.lock().await.list_databases(ListDatabasesRequest {}) {
        Ok(resp) => Json(json!({ "databases": resp.databases })).into_response(),
        Err(err) => axon_error_response(err),
    }
}

async fn drop_database(
    Extension(handler): Extension<TenantHandler>,
    Extension(identity): Extension<Identity>,
    Path(name): Path<String>,
    Query(force): Query<ForceQuery>,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }
    match handler.lock().await.drop_database(DropDatabaseRequest {
        name,
        force: force.force,
    }) {
        Ok(resp) => (
            StatusCode::OK,
            Json(json!({
                "name": resp.name,
                "collections_removed": resp.collections_removed,
            })),
        )
            .into_response(),
        Err(err) => axon_error_response(err),
    }
}

async fn create_namespace(
    Extension(handler): Extension<TenantHandler>,
    Extension(identity): Extension<Identity>,
    Path((database, schema)): Path<(String, String)>,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }
    match handler
        .lock()
        .await
        .create_namespace(CreateNamespaceRequest { database, schema })
    {
        Ok(resp) => (
            StatusCode::CREATED,
            Json(json!({
                "database": resp.database,
                "schema": resp.schema,
            })),
        )
            .into_response(),
        Err(err) => axon_error_response(err),
    }
}

async fn list_namespaces(
    Extension(handler): Extension<TenantHandler>,
    Path(database): Path<String>,
) -> Response {
    match handler
        .lock()
        .await
        .list_namespaces(ListNamespacesRequest { database })
    {
        Ok(resp) => Json(json!({
            "database": resp.database,
            "schemas": resp.schemas,
        }))
        .into_response(),
        Err(err) => axon_error_response(err),
    }
}

async fn list_namespace_collections(
    Extension(handler): Extension<TenantHandler>,
    Path((database, schema)): Path<(String, String)>,
) -> Response {
    match handler
        .lock()
        .await
        .list_namespace_collections(ListNamespaceCollectionsRequest { database, schema })
    {
        Ok(resp) => Json(json!({
            "database": resp.database,
            "schema": resp.schema,
            "collections": resp.collections,
        }))
        .into_response(),
        Err(err) => axon_error_response(err),
    }
}

async fn drop_namespace(
    Extension(handler): Extension<TenantHandler>,
    Extension(identity): Extension<Identity>,
    Path((database, schema)): Path<(String, String)>,
    Query(force): Query<ForceQuery>,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }
    match handler.lock().await.drop_namespace(DropNamespaceRequest {
        database,
        schema,
        force: force.force,
    }) {
        Ok(resp) => (
            StatusCode::OK,
            Json(json!({
                "database": resp.database,
                "schema": resp.schema,
                "collections_removed": resp.collections_removed,
            })),
        )
            .into_response(),
        Err(err) => axon_error_response(err),
    }
}

// ── Transaction endpoint ─────────────────────────────────────────────────────

fn validate_idempotency_key(key: &str) -> Result<(), AxonError> {
    if key.is_empty() || key.len() > 128 {
        return Err(AxonError::InvalidArgument(
            "idempotency_key length must be 1..128 characters".into(),
        ));
    }
    if !key
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-'))
    {
        return Err(AxonError::InvalidArgument(
            "idempotency_key must use ASCII [A-Za-z0-9_.:-] characters".into(),
        ));
    }
    Ok(())
}

fn json_merge_patch(target: &mut Value, patch: &Value) {
    if let Value::Object(patch_map) = patch {
        if !target.is_object() {
            *target = Value::Object(serde_json::Map::new());
        }
        if let Value::Object(target_map) = target {
            for (key, value) in patch_map {
                if value.is_null() {
                    target_map.remove(key);
                } else {
                    let entry = target_map.entry(key.clone()).or_insert(Value::Null);
                    json_merge_patch(entry, value);
                }
            }
        }
    } else {
        *target = patch.clone();
    }
}

#[allow(clippy::too_many_arguments)]
async fn commit_transaction(
    Extension(handler): Extension<TenantHandler>,
    Extension(mcp_sessions): Extension<McpHttpSessions>,
    Extension(broker): Extension<BroadcastBroker>,
    Extension(current_tenant): Extension<CurrentTenant>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(identity): Extension<Identity>,
    Extension(caller): Extension<CoreCallerIdentity>,
    Extension(rate_limiter): Extension<WriteRateLimiter>,
    Extension(actor_scope): Extension<ActorScopeGuard>,
    Extension(idempotency): Extension<HttpIdempotencyStore>,
    jwt_identity: Option<Extension<ResolvedIdentity>>,
    headers: HeaderMap,
    Json(body): Json<TransactionBody>,
) -> Response {
    if let Err(e) = identity.require_write() {
        return auth_error_response(e);
    }
    if body.operations.len() > 100 {
        return axon_error_response(AxonError::InvalidArgument(
            "transaction exceeds maximum of 100 operations".into(),
        ));
    }
    // Check actor scope for every collection referenced in the transaction.
    for op in &body.operations {
        let collections: Vec<&str> = match op {
            TransactionOp::Create { collection, .. }
            | TransactionOp::Update { collection, .. }
            | TransactionOp::Patch { collection, .. }
            | TransactionOp::Delete { collection, .. } => vec![collection],
            TransactionOp::CreateLink {
                source_collection,
                target_collection,
                ..
            }
            | TransactionOp::DeleteLink {
                source_collection,
                target_collection,
                ..
            } => vec![source_collection, target_collection],
        };
        for collection in collections {
            if let Err(e) = actor_scope.check(&identity.actor, collection, &identity.role) {
                return auth_error_response(e);
            }
        }
    }
    if let Err(limited) = rate_limiter.check(&identity.actor).await {
        return rate_limit_response(&limited);
    }

    // ── Idempotency check (FEAT-008 US-081) ─────────────────────────────────
    //
    // Keys are scoped per database. The `current_database` extension
    // (populated from the URL path by the auth middleware) is authoritative.
    let idem_key = match body.idempotency_key.clone() {
        Some(key) => Some(key),
        None => headers
            .get(IDEMPOTENCY_KEY_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string()),
    };
    if let Some(ref key) = idem_key {
        if let Err(error) = validate_idempotency_key(key) {
            return axon_error_response(error);
        }
    }
    let idem_scope = idempotency_scope(&current_tenant, &current_database);

    if let Some(ref key) = idem_key {
        match idempotency.try_reserve(&idem_scope, key) {
            ReservationResult::AlreadyCached(cached) => {
                let mut response = (cached.status, Json(cached.body)).into_response();
                response.headers_mut().insert(
                    IDEMPOTENCY_CACHE_HEADER,
                    axum::http::HeaderValue::from_static("hit"),
                );
                return response;
            }
            ReservationResult::InFlight { retry_after_ms } => {
                return (
                    StatusCode::CONFLICT,
                    Json(json!({
                        "code": "in_flight",
                        "retryable": true,
                        "retry_after_ms": retry_after_ms,
                    })),
                )
                    .into_response();
            }
            ReservationResult::Reserved => {
                // Proceed with execution; we'll call store_response on
                // success or release() on failure.
            }
        }
    }

    use axon_api::transaction::Transaction;
    use axon_core::types::Entity;

    let mut tx = Transaction::new();

    // Stage all operations.
    for op in body.operations {
        let result = match op {
            TransactionOp::Create {
                collection,
                id,
                data,
            } => tx.create(Entity::new(
                qualify_collection_name(&collection, &current_database),
                EntityId::new(&id),
                data,
            )),
            TransactionOp::Update {
                collection,
                id,
                data,
                expected_version,
            } => {
                // Read current state for audit before-snapshot.
                let h = handler.lock().await;
                let data_before = h
                    .storage_ref()
                    .get(
                        &qualify_collection_name(&collection, &current_database),
                        &EntityId::new(&id),
                    )
                    .ok()
                    .flatten()
                    .map(|entity| entity.data);
                drop(h);
                tx.update(
                    Entity::new(
                        qualify_collection_name(&collection, &current_database),
                        EntityId::new(&id),
                        data,
                    ),
                    expected_version,
                    data_before,
                )
            }
            TransactionOp::Patch {
                collection,
                id,
                patch,
                expected_version,
            } => {
                let h = handler.lock().await;
                let existing = match h.storage_ref().get(
                    &qualify_collection_name(&collection, &current_database),
                    &EntityId::new(&id),
                ) {
                    Ok(Some(entity)) => entity,
                    Ok(None) => {
                        drop(h);
                        if let Some(ref key) = idem_key {
                            idempotency.release(&idem_scope, key);
                        }
                        return axon_error_response(AxonError::NotFound(id.clone()));
                    }
                    Err(e) => {
                        drop(h);
                        if let Some(ref key) = idem_key {
                            idempotency.release(&idem_scope, key);
                        }
                        return axon_error_response(e);
                    }
                };
                drop(h);
                let mut merged = existing.data.clone();
                json_merge_patch(&mut merged, &patch);
                tx.update(
                    Entity::new(
                        qualify_collection_name(&collection, &current_database),
                        EntityId::new(&id),
                        merged,
                    ),
                    expected_version,
                    Some(existing.data),
                )
            }
            TransactionOp::Delete {
                collection,
                id,
                expected_version,
            } => {
                let h = handler.lock().await;
                let data_before = h
                    .storage_ref()
                    .get(
                        &qualify_collection_name(&collection, &current_database),
                        &EntityId::new(&id),
                    )
                    .ok()
                    .flatten()
                    .map(|entity| entity.data);
                drop(h);
                tx.delete(
                    qualify_collection_name(&collection, &current_database),
                    EntityId::new(&id),
                    expected_version,
                    data_before,
                )
            }
            TransactionOp::CreateLink {
                source_collection,
                source_id,
                target_collection,
                target_id,
                link_type,
                metadata,
            } => tx.create_link(Link {
                source_collection: qualify_collection_name(&source_collection, &current_database),
                source_id: EntityId::new(source_id),
                target_collection: qualify_collection_name(&target_collection, &current_database),
                target_id: EntityId::new(target_id),
                link_type,
                metadata,
            }),
            TransactionOp::DeleteLink {
                source_collection,
                source_id,
                target_collection,
                target_id,
                link_type,
            } => tx.delete_link(Link {
                source_collection: qualify_collection_name(&source_collection, &current_database),
                source_id: EntityId::new(source_id),
                target_collection: qualify_collection_name(&target_collection, &current_database),
                target_id: EntityId::new(target_id),
                link_type,
                metadata: Value::Null,
            }),
        };
        if let Err(e) = result {
            // Staging failure — not cacheable; release the reservation so
            // the client can retry with the same key after correction.
            if let Some(ref key) = idem_key {
                idempotency.release(&idem_scope, key);
            }
            return axon_error_response(e);
        }
    }

    // Body-level `actor` is ignored in favor of the caller identity resolved
    // from `x-axon-actor` (FEAT-012); the authoritative actor is `caller.actor`.
    let _ = body.actor;

    // Commit atomically.
    let tx_id = tx.id.clone();
    let mut h = handler.lock().await;
    let commit_result =
        h.commit_transaction_with_caller(tx, &caller, attribution_from_jwt(jwt_identity));
    match commit_result {
        Ok(written) => {
            // Look up the audit entries produced by this transaction so we can
            // stamp each broadcast ChangeEvent with a resume cursor. All
            // entries share the tx_id; match each to its (collection, id) pair.
            let tx_entries = h
                .audit_log()
                .query_by_transaction_id(&tx_id)
                .unwrap_or_default();
            for entity in &written {
                notify_entity_change(&mcp_sessions, &current_database, entity);
                let entity_collection = &entity.collection;
                let entity_key = &entity.id;
                let entry = tx_entries
                    .iter()
                    .find(|e| &e.collection == entity_collection && &e.entity_id == entity_key)
                    .filter(|e| {
                        matches!(
                            e.mutation,
                            MutationType::EntityCreate
                                | MutationType::EntityUpdate
                                | MutationType::EntityDelete
                                | MutationType::EntityRevert
                        )
                    });
                if let Some(entry) = entry {
                    publish_change_event_from_audit_entry(
                        &broker,
                        entry,
                        &current_tenant,
                        &current_database,
                    );
                } else if entity.collection != Link::links_collection() {
                    broadcast_entity_change(
                        &broker,
                        entity,
                        "update",
                        String::new(),
                        &caller.actor,
                        &current_tenant,
                        &current_database,
                    );
                }
            }
            let entities: Vec<Value> = written
                .iter()
                .map(|e| {
                    json!({
                        "collection": e.collection.to_string(),
                        "id": e.id.to_string(),
                        "version": e.version,
                    })
                })
                .collect();
            let body = json!({
                "transaction_id": tx_id,
                "entities": entities,
            });
            // Cache the response body so a retry with the same key gets the
            // same result without re-executing (FEAT-008 US-081).
            if let Some(ref key) = idem_key {
                idempotency.store_response(
                    &idem_scope,
                    key,
                    CachedHttpResponse {
                        status: StatusCode::OK,
                        body: body.clone(),
                    },
                );
            }
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(e) => {
            // Policy-forbidden transactions are terminal for this payload and
            // cacheable; validation/conflict/storage failures remain retryable
            // after correction and release their reservation.
            if let AxonError::PolicyDenied(ref denial) = e {
                if !denial.is_policy_filter_unindexed() {
                    let body = ApiError::new("forbidden", denial.detail());
                    let body = serde_json::to_value(body).unwrap_or_else(
                        |_| json!({"code": "forbidden", "detail": denial.detail()}),
                    );
                    if let Some(ref key) = idem_key {
                        idempotency.store_response(
                            &idem_scope,
                            key,
                            CachedHttpResponse {
                                status: StatusCode::FORBIDDEN,
                                body: body.clone(),
                            },
                        );
                    }
                    return (StatusCode::FORBIDDEN, Json(body)).into_response();
                }
            }
            if let Some(ref key) = idem_key {
                idempotency.release(&idem_scope, key);
            }
            axon_error_response(e)
        }
    }
}

// ── GraphQL handler ──────────────────────────────────────────────────────────

/// Collect all `CollectionSchema`s from the handler, then build and execute
/// a dynamic GraphQL schema for each incoming request.
///
/// Rebuilding per-request ensures newly-created (or dropped) collections are
/// always reflected in the GraphQL API. A caching layer can be added later
/// for performance.
async fn graphql_handler(
    Extension(handler): Extension<TenantHandler>,
    Extension(caller): Extension<CoreCallerIdentity>,
    Extension(current_tenant): Extension<CurrentTenant>,
    Extension(current_database): Extension<CurrentDatabase>,
    req: async_graphql_axum::GraphQLRequest,
) -> Response {
    // 1. Gather current collection schemas.
    let schemas: Vec<CollectionSchema> = {
        let guard = handler.lock().await;
        let names = match guard.list_collections(ListCollectionsRequest {}) {
            Ok(resp) => resp.collections,
            Err(err) => return axon_error_response(err),
        };
        names
            .iter()
            .filter_map(|meta| {
                let cid = CollectionId::new(&meta.name);
                match guard.get_schema(&cid) {
                    Ok(Some(s)) => Some(s),
                    _ => None,
                }
            })
            .collect()
    };

    // 2. Build the dynamic schema.
    let gql_schema = match axon_graphql::build_schema_with_handler(&schemas, handler.clone()) {
        Ok(s) => s,
        Err(msg) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "code": "GRAPHQL_SCHEMA_ERROR", "detail": msg })),
            )
                .into_response();
        }
    };

    // 3. Execute the request, injecting the resolved caller identity so
    //    mutation resolvers can attribute audit entries to the authenticated
    //    actor (FEAT-012).
    let response =
        gql_schema
            .schema
            .execute(req.into_inner().data(caller).data(GraphqlIdempotencyScope(
                idempotency_scope(&current_tenant, &current_database),
            )))
            .await;
    async_graphql_axum::GraphQLResponse::from(response).into_response()
}

/// GraphQL WebSocket subscription endpoint.
///
/// Upgrades the HTTP connection to a WebSocket and runs the graphql-ws
/// protocol. The schema is rebuilt on connection to reflect current
/// collections.
async fn graphql_ws_handler(
    Extension(handler): Extension<TenantHandler>,
    Extension(broker): Extension<BroadcastBroker>,
    Extension(current_tenant): Extension<CurrentTenant>,
    Extension(current_database): Extension<CurrentDatabase>,
    protocol: async_graphql_axum::GraphQLProtocol,
    ws: WebSocketUpgrade,
) -> Response {
    // 1. Gather current collection schemas.
    let schemas: Vec<CollectionSchema> = {
        let guard = handler.lock().await;
        let names = match guard.list_collections(ListCollectionsRequest {}) {
            Ok(resp) => resp.collections,
            Err(err) => return axon_error_response(err),
        };
        names
            .iter()
            .filter_map(|meta| {
                let cid = CollectionId::new(&meta.name);
                match guard.get_schema(&cid) {
                    Ok(Some(s)) => Some(s),
                    _ => None,
                }
            })
            .collect()
    };

    // 2. Build the dynamic schema with subscription support.
    let gql_schema = match axon_graphql::build_schema_with_handler_and_broker_scoped(
        &schemas,
        handler.clone(),
        Some(broker),
        Some((
            current_tenant.as_str().to_string(),
            current_database.as_str().to_string(),
        )),
    ) {
        Ok(s) => s,
        Err(msg) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "code": "GRAPHQL_SCHEMA_ERROR", "detail": msg })),
            )
                .into_response();
        }
    };

    // 3. Upgrade to WebSocket and serve the subscription protocol.
    // Protocol names from the graphql-ws and subscriptions-transport-ws specs.
    ws.protocols(["graphql-transport-ws", "graphql-ws"])
        .on_upgrade(move |stream| {
            let ws = async_graphql_axum::GraphQLWebSocket::new(stream, gql_schema.schema, protocol);
            ws.serve()
        })
}

/// GraphQL Playground — serves the interactive browser IDE at `/graphql/playground`.
async fn graphql_playground_handler() -> impl IntoResponse {
    axum::response::Html(playground_source(
        GraphQLPlaygroundConfig::new("/tenants/default/databases/default/graphql")
            .subscription_endpoint("/tenants/default/databases/default/graphql/ws"),
    ))
}

// ── Router construction ───────────────────────────────────────────────────────

/// Build the axum router for the HTTP gateway.
///
/// Wraps [`build_router_with_auth`] with no-auth mode and default
/// rate-limiting / actor-scope configuration.  Used by tests.
pub fn build_router(
    tenant_router: Arc<TenantRouter>,
    backend: impl Into<String>,
    ui_dir: Option<PathBuf>,
) -> Router {
    build_router_with_auth(
        tenant_router,
        backend,
        ui_dir,
        AuthContext::no_auth(),
        crate::rate_limit::RateLimitConfig::default(),
        ActorScopeGuard::default(),
        None,
        CorsStore::default(),
    )
}

/// Construct the default production [`HttpIdempotencyStore`] (5-minute TTL
/// backed by a [`SystemClock`](axon_core::clock::SystemClock)).
fn default_idempotency_store() -> HttpIdempotencyStore {
    use axon_core::clock::SystemClock;
    Arc::new(IdempotencyStore::with_default_ttl(Arc::new(SystemClock)))
}

/// Build the axum router for the HTTP gateway with an injected
/// [`HttpIdempotencyStore`].
///
/// Tests use this entry point to exercise TTL expiry deterministically by
/// passing a store whose clock is a [`FakeClock`](axon_core::clock::FakeClock)
/// or whose TTL is a small duration. Production paths should use
/// [`build_router`] or [`build_router_with_auth`], which construct a default
/// store with a `SystemClock` and the FEAT-008 US-081 5-minute TTL.
pub fn build_router_with_idempotency(
    tenant_router: Arc<TenantRouter>,
    backend: impl Into<String>,
    ui_dir: Option<PathBuf>,
    idempotency_store: HttpIdempotencyStore,
) -> Router {
    build_router_with_auth_inner(
        tenant_router,
        backend,
        ui_dir,
        AuthContext::no_auth(),
        crate::rate_limit::RateLimitConfig::default(),
        ActorScopeGuard::default(),
        None,
        CorsStore::default(),
        idempotency_store,
    )
}

/// Data routes for the HTTP gateway.
///
/// Handlers extract the tenant-resolved handler from
/// [`Extension<TenantHandler>`] rather than axum [`State`].  The
/// [`resolve_tenant_handler`] middleware populates this extension before
/// handlers run.
///
/// MCP HTTP routes are **not** included here — they continue to use the
/// default handler via axum State and are merged separately.
fn data_routes() -> Router {
    Router::new()
        .route("/entities/{collection}/{id}", post(create_entity))
        .route("/entities/{collection}/{id}", get(get_entity))
        .route("/entities/{collection}/{id}", put(update_entity))
        .route("/entities/{collection}/{id}", delete(delete_entity))
        .route(
            "/collections/{collection}/entities/{id}",
            get(get_collection_entity),
        )
        .route(
            "/collections/{collection}/entities/{id}/rollback",
            post(rollback_collection_entity),
        )
        .route(
            "/collections/{collection}/rollback",
            post(rollback_collection_handler),
        )
        .route("/schema", get(schema_manifest))
        .route("/collections/{collection}/query", post(query_entities))
        .route("/snapshot", post(snapshot_entities_handler))
        .route(
            "/lifecycle/{collection}/{id}/transition",
            post(transition_lifecycle_handler),
        )
        .route("/links", post(create_link))
        .route("/links", delete(delete_link))
        .route(
            "/traverse/{collection}/{id}",
            get(traverse).post(traverse_post),
        )
        .route(
            "/audit/entity/{collection}/{id}",
            get(query_audit_by_entity),
        )
        .route("/audit/query", get(query_audit))
        .route("/audit/revert", post(revert_entity))
        .route("/collections", get(list_collections))
        .route("/collections/{name}", post(create_collection))
        .route("/collections/{name}", get(describe_collection))
        .route("/collections/{name}", delete(drop_collection))
        .route(
            "/collections/{collection}/template",
            put(put_collection_template),
        )
        .route(
            "/collections/{collection}/template",
            get(get_collection_template),
        )
        .route(
            "/collections/{collection}/template",
            delete(delete_collection_template),
        )
        .route("/collections/{name}/schema", put(put_schema))
        .route("/collections/{name}/schema", get(get_schema))
        .route("/transactions", post(commit_transaction))
        .route(
            "/transactions/{transaction_id}/rollback",
            post(rollback_transaction_handler),
        )
        .route("/graphql", get(graphql_handler).post(graphql_handler))
        .route("/graphql/ws", get(graphql_ws_handler))
}

/// Build the axum router for the HTTP gateway with request authentication.
///
/// The `tenant_router` provides per-database handler isolation.  A
/// middleware layer resolves the tenant handler from the URL path
/// before any route handler runs.
///
/// `cors` controls the CORS allowed-origin policy.  An empty `CorsStore`
/// (the default) disables CORS headers entirely, preserving backward
/// compatibility with non-browser clients.
#[allow(clippy::too_many_arguments)]
pub fn build_router_with_auth(
    tenant_router: Arc<TenantRouter>,
    backend: impl Into<String>,
    ui_dir: Option<PathBuf>,
    auth: AuthContext,
    rate_limit_config: crate::rate_limit::RateLimitConfig,
    actor_scope: ActorScopeGuard,
    control_plane: Option<crate::control_plane_routes::ControlPlaneState>,
    cors: CorsStore,
) -> Router {
    build_router_with_auth_inner(
        tenant_router,
        backend,
        ui_dir,
        auth,
        rate_limit_config,
        actor_scope,
        control_plane,
        cors,
        default_idempotency_store(),
    )
}

#[allow(clippy::too_many_arguments)]
fn build_router_with_auth_inner(
    tenant_router: Arc<TenantRouter>,
    backend: impl Into<String>,
    ui_dir: Option<PathBuf>,
    auth: AuthContext,
    rate_limit_config: crate::rate_limit::RateLimitConfig,
    actor_scope: ActorScopeGuard,
    control_plane: Option<crate::control_plane_routes::ControlPlaneState>,
    cors: CorsStore,
    idempotency_store: HttpIdempotencyStore,
) -> Router {
    build_router_full(
        tenant_router,
        backend,
        ui_dir,
        auth,
        rate_limit_config,
        actor_scope,
        control_plane,
        cors,
        idempotency_store,
        BroadcastBroker::default(),
    )
}

/// Build the router with an externally-provided [`BroadcastBroker`].
///
/// Useful in tests that want to subscribe to the broker and observe the
/// [`axon_graphql::ChangeEvent`]s published by HTTP write handlers. Returns
/// the router configured to use the supplied broker instance.
pub fn build_router_with_broker(
    tenant_router: Arc<TenantRouter>,
    backend: impl Into<String>,
    ui_dir: Option<PathBuf>,
    broker: BroadcastBroker,
) -> Router {
    build_router_full(
        tenant_router,
        backend,
        ui_dir,
        AuthContext::no_auth(),
        crate::rate_limit::RateLimitConfig::default(),
        ActorScopeGuard::default(),
        None,
        CorsStore::default(),
        default_idempotency_store(),
        broker,
    )
}

/// Optional JWT verification middleware (ADR-018 cutover).
///
/// Falls through to the next handler when no `Authorization` header is
/// present so that existing Tailscale / guest / no-auth sessions continue
/// to work unchanged. When the header IS present the request is forwarded
/// to the canonical [`crate::auth_pipeline::jwt_verify_layer`] which either
/// installs a [`ResolvedIdentity`] extension or returns a 401/403 error.
async fn optional_jwt_verify_layer(
    State(state): State<Arc<crate::auth_pipeline::AuthPipelineState>>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    if !request
        .headers()
        .contains_key(axum::http::header::AUTHORIZATION)
    {
        return next.run(request).await;
    }
    crate::auth_pipeline::jwt_verify_layer(State(state), request, next).await
}

/// Build an [`AuditAttribution`] from a [`ResolvedIdentity`] installed by the
/// JWT middleware, if one is present in the request extensions.
///
/// Returns `None` when the request was authenticated by Tailscale or `--no-auth`.
#[allow(clippy::single_option_map)]
fn attribution_from_jwt(
    jwt_identity: Option<Extension<ResolvedIdentity>>,
) -> Option<AuditAttribution> {
    jwt_identity.map(|Extension(id)| AuditAttribution {
        user_id: id.user_id.to_string(),
        tenant_id: id.tenant_id.to_string(),
        jti: None, // JTI is in the claims but not copied into ResolvedIdentity
        auth_method: "jwt".to_string(),
    })
}

#[allow(clippy::too_many_arguments)]
fn build_router_full(
    tenant_router: Arc<TenantRouter>,
    backend: impl Into<String>,
    ui_dir: Option<PathBuf>,
    auth: AuthContext,
    rate_limit_config: crate::rate_limit::RateLimitConfig,
    actor_scope: ActorScopeGuard,
    control_plane: Option<crate::control_plane_routes::ControlPlaneState>,
    cors: CorsStore,
    idempotency_store: HttpIdempotencyStore,
    broker: BroadcastBroker,
) -> Router {
    let start = Instant::now();
    let backend = backend.into();
    let mcp_sessions = McpHttpSessions::default();
    let rate_limiter = WriteRateLimiter::new(rate_limit_config);
    let handler = tenant_router.default_handler().clone();

    // MCP HTTP routes use axum State<SharedHandler<Box<dyn StorageAdapter>>>
    // and always operate on the default handler.  They are merged separately
    // from the tenant-aware data routes.
    let mcp_routes = crate::mcp_http::routes::<Box<dyn StorageAdapter + Send + Sync>>()
        .with_state(handler.clone());

    let mut router = Router::new()
        .merge(mcp_routes)
        .nest("/tenants/{tenant}/databases/{database}", data_routes())
        .route("/databases", get(list_databases))
        .route("/databases/{name}", post(create_database))
        .route("/databases/{name}", delete(drop_database))
        .route("/databases/{database}/schemas", get(list_namespaces))
        .route(
            "/databases/{database}/schemas/{schema}",
            post(create_namespace),
        )
        .route(
            "/databases/{database}/schemas/{schema}",
            delete(drop_namespace),
        )
        .route(
            "/databases/{database}/schemas/{schema}/collections",
            get(list_namespace_collections),
        )
        .layer(Extension(rate_limiter))
        .layer(Extension(actor_scope))
        .layer(Extension(mcp_sessions))
        .layer(Extension(broker))
        .layer(Extension(idempotency_store))
        .layer(middleware::from_fn(resolve_tenant_handler))
        .layer(Extension(tenant_router))
        .route(
            "/health",
            get(move || {
                let uptime = start.elapsed().as_secs();
                let handler = handler.clone();
                let backend = backend.clone();
                async move {
                    let guard = handler.lock().await;
                    let databases = match guard.list_databases(ListDatabasesRequest {}) {
                        Ok(resp) => resp.databases,
                        Err(err) => return axon_error_response(err),
                    };
                    let (default_namespace, default_namespace_status) =
                        match default_namespace_health(&guard, &databases) {
                            Ok(health) => health,
                            Err(err) => return axon_error_response(err),
                        };

                    (
                        StatusCode::OK,
                        Json(json!({
                            "status": "ok",
                            "version": env!("CARGO_PKG_VERSION"),
                            "uptime_seconds": uptime,
                            "backing_store": {
                                "backend": backend,
                                "status": "ok",
                            },
                            "databases": databases,
                            "default_namespace": default_namespace,
                            "default_namespace_status": default_namespace_status,
                        })),
                    )
                        .into_response()
                }
            }),
        );

    router = router
        .route(
            "/",
            get(|| async { axum::response::Redirect::temporary("/ui") }),
        )
        .route("/auth/me", get(auth_me))
        .route("/graphql/playground", get(graphql_playground_handler));

    // UI: disk directory overrides the embedded build (useful during development).
    // When --ui-dir is not set, serve the UI compiled into the binary.
    if let Some(ui_dir) = ui_dir {
        let index_path = ui_dir.join("index.html");
        let ui_service = get_service(ServeDir::new(ui_dir).fallback(ServeFile::new(index_path)));
        router = router.nest_service("/ui", ui_service);
    } else {
        router = router
            .route("/ui", get(crate::embedded_ui::embedded_ui_handler))
            .route("/ui/", get(crate::embedded_ui::embedded_ui_handler))
            .route("/ui/{*path}", get(crate::embedded_ui::embedded_ui_handler));
    }

    if let Some(cp) = control_plane {
        // Install the optional JWT verification layer when the control plane
        // has both an issuer and a storage adapter configured (ADR-018 cutover).
        // This runs *inside* the CORS/auth stack so it fires after Tailscale auth
        // but the ResolvedIdentity it installs is available to all route handlers.
        if let (Some(issuer), Some(storage)) = (cp.jwt_issuer.clone(), cp.storage.clone()) {
            let pipeline_state = Arc::new(crate::auth_pipeline::AuthPipelineState {
                issuer,
                revocation_cache: Arc::new(crate::auth_pipeline::InMemoryRevocationCache::new()),
                storage,
            });
            router = router.layer(middleware::from_fn_with_state(
                pipeline_state,
                optional_jwt_verify_layer,
            ));
        }
        let cp_routes = crate::control_plane_routes::control_plane_routes().with_state(cp);
        router = router.nest("/control", cp_routes);
    }

    // Auth is the inner gatekeeper; CORS is the outer envelope so that OPTIONS
    // preflights never reach the auth middleware. The caller-identity
    // middleware runs immediately after auth so route handlers see a fully
    // resolved `CoreCallerIdentity` in their extensions (FEAT-012).
    router
        .layer(middleware::from_fn(resolve_caller_identity))
        .layer(middleware::from_fn_with_state(
            auth,
            authenticate_http_request,
        ))
        .layer(middleware::from_fn_with_state(cors, cors_middleware))
        .layer(middleware::from_fn(request_id_middleware))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::collections::HashMap;
    use std::fmt::Display;
    use std::future::Future;
    use std::net::SocketAddr;
    use std::pin::Pin;
    use std::sync::Mutex as StdMutex;
    use std::time::Duration;

    use super::*;
    use crate::auth::{
        AuthContext, AuthError, AuthMode, Role, TailscaleWhoisProvider, TailscaleWhoisResponse,
    };
    use crate::tenant_router::TenantRouter;
    use axon_core::id::{CollectionId, Namespace};
    use axon_schema::schema::{CollectionSchema, CollectionView, IndexDef, IndexType};
    use axon_storage::adapter::StorageAdapter;
    use axon_storage::SqliteStorageAdapter;
    use axum::extract::connect_info::MockConnectInfo;
    use axum_test::TestServer;
    use serde_json::json;

    struct FakeWhoisProvider {
        results: StdMutex<HashMap<SocketAddr, Result<TailscaleWhoisResponse, AuthError>>>,
    }

    impl FakeWhoisProvider {
        fn with_result(
            peer: SocketAddr,
            result: Result<TailscaleWhoisResponse, AuthError>,
        ) -> Self {
            let mut results = HashMap::new();
            results.insert(peer, result);
            Self {
                results: StdMutex::new(results),
            }
        }
    }

    impl TailscaleWhoisProvider for FakeWhoisProvider {
        fn verify(&self) -> Pin<Box<dyn Future<Output = Result<(), AuthError>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }

        fn whois(
            &self,
            address: SocketAddr,
        ) -> Pin<Box<dyn Future<Output = Result<TailscaleWhoisResponse, AuthError>> + Send + '_>>
        {
            Box::pin(async move {
                let results = match self.results.lock() {
                    Ok(results) => results,
                    Err(poisoned) => poisoned.into_inner(),
                };
                results.get(&address).cloned().unwrap_or_else(|| {
                    Err(AuthError::Unauthorized(
                        "peer is not a recognized tailnet address".into(),
                    ))
                })
            })
        }
    }

    fn test_server_with_handler() -> (TestServer, TenantHandler) {
        let storage: Box<dyn StorageAdapter + Send + Sync> =
            Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite should open"));
        let handler: TenantHandler = Arc::new(Mutex::new(AxonHandler::new(storage)));
        let tenant_router = Arc::new(TenantRouter::single(handler.clone()));
        let app = build_router(tenant_router, "memory", None);
        (TestServer::new(app), handler)
    }

    fn test_server() -> TestServer {
        test_server_with_handler().0
    }

    fn test_server_with_auth(peer: SocketAddr, auth: AuthContext) -> TestServer {
        let storage: Box<dyn StorageAdapter + Send + Sync> =
            Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite should open"));
        let handler: TenantHandler = Arc::new(Mutex::new(AxonHandler::new(storage)));
        let tenant_router = Arc::new(TenantRouter::single(handler));
        let app = build_router_with_auth(
            tenant_router,
            "memory",
            None,
            auth,
            crate::rate_limit::RateLimitConfig::default(),
            ActorScopeGuard::default(),
            None,
            CorsStore::default(),
        )
        .layer(MockConnectInfo(peer));
        TestServer::new(app)
    }

    fn test_server_with_rate_limit(
        rate_limit_config: crate::rate_limit::RateLimitConfig,
    ) -> TestServer {
        let storage: Box<dyn StorageAdapter + Send + Sync> =
            Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite should open"));
        let handler: TenantHandler = Arc::new(Mutex::new(AxonHandler::new(storage)));
        let tenant_router = Arc::new(TenantRouter::single(handler));
        let app = build_router_with_auth(
            tenant_router,
            "memory",
            None,
            AuthContext::no_auth(),
            rate_limit_config,
            ActorScopeGuard::default(),
            None,
            CorsStore::default(),
        );
        TestServer::new(app)
    }

    fn ok_or_panic<T, E: Display>(result: Result<T, E>, context: &str) -> T {
        match result {
            Ok(value) => value,
            Err(err) => panic!("{context}: {err}"),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_create_then_get_entity() {
        let server = test_server();

        // Create
        let resp = server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "hello"}, "actor": "test"}))
            .await;
        resp.assert_status(StatusCode::CREATED);
        let body: Value = resp.json();
        assert_eq!(body["entity"]["version"], 1);

        // Get
        let resp = server
            .get("/tenants/default/databases/default/entities/tasks/t-001")
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["entity"]["data"]["title"], "hello");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_get_missing_returns_404() {
        let server = test_server();
        let resp = server
            .get("/tenants/default/databases/default/entities/tasks/ghost")
            .await;
        resp.assert_status_not_found();
        let body: Value = resp.json();
        assert_eq!(body["code"], "not_found");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_collection_entity_get_defaults_to_json() {
        let server = test_server();

        server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "hello"}}))
            .await
            .assert_status(StatusCode::CREATED);

        let resp = server
            .get("/tenants/default/databases/default/collections/tasks/entities/t-001")
            .await;

        resp.assert_status_ok();
        resp.assert_header("content-type", "application/json");
        let body: Value = resp.json();
        assert_eq!(body["entity"]["data"]["title"], "hello");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_collection_entity_get_markdown_returns_text_markdown() {
        let (server, handler) = test_server_with_handler();

        server
            .post("/tenants/default/databases/default/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "hello", "status": "open"}}))
            .await
            .assert_status(StatusCode::CREATED);

        ok_or_panic(
            handler
                .lock()
                .await
                .storage_mut()
                .put_collection_view(&CollectionView::new(
                    CollectionId::new("tasks"),
                    "# {{title}}\n\nStatus: {{status}}",
                )),
            "storing collection view for markdown HTTP test",
        );

        let resp = server
            .get("/tenants/default/databases/default/collections/tasks/entities/t-001?format=markdown")
            .await;

        resp.assert_status_ok();
        resp.assert_header("content-type", "text/markdown; charset=utf-8");
        assert_eq!(resp.text(), "# hello\n\nStatus: open");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_collection_entity_get_markdown_requires_template() {
        let server = test_server();

        server
            .post("/tenants/default/databases/default/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "hello"}}))
            .await
            .assert_status(StatusCode::CREATED);

        let resp = server
            .get("/tenants/default/databases/default/collections/tasks/entities/t-001?format=markdown")
            .await;

        resp.assert_status(StatusCode::BAD_REQUEST);
        let body: Value = resp.json();
        assert_eq!(body["code"], "invalid_argument");
        assert!(body["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("has no markdown template defined"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_collection_entity_get_markdown_render_failure_returns_entity_payload() {
        let (server, handler) = test_server_with_handler();

        server
            .post("/tenants/default/databases/default/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "hello", "status": "open"}}))
            .await
            .assert_status(StatusCode::CREATED);

        ok_or_panic(
            handler
                .lock()
                .await
                .storage_mut()
                .put_collection_view(&CollectionView::new(
                    CollectionId::new("tasks"),
                    "{{#title}",
                )),
            "storing invalid collection view for markdown HTTP test",
        );

        let resp = server
            .get("/tenants/default/databases/default/collections/tasks/entities/t-001?format=markdown")
            .await;

        resp.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
        resp.assert_header("content-type", "application/json");
        let body: Value = resp.json();
        assert_eq!(body["code"], "storage_error");
        assert!(body["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("failed to render markdown"));
        assert_eq!(body["entity"]["collection"], "tasks");
        assert_eq!(body["entity"]["id"], "t-001");
        assert_eq!(body["entity"]["version"], 1);
        assert_eq!(body["entity"]["data"]["title"], "hello");
        assert_eq!(body["entity"]["data"]["status"], "open");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_collection_template_crud_round_trip_uses_public_surface() {
        let server = test_server();

        server
            .post("/tenants/default/databases/default/collections/tasks")
            .json(&json!({
                "schema": {
                    "entity_schema": {
                        "type": "object",
                        "properties": {
                            "title": {"type": "string"},
                            "notes": {"type": "string"}
                        },
                        "required": ["title"]
                    }
                }
            }))
            .await
            .assert_status(StatusCode::CREATED);

        let put = server
            .put("/tenants/default/databases/default/collections/tasks/template")
            .json(&json!({
                "template": "# {{title}}\n\n{{notes}}",
                "actor": "operator"
            }))
            .await;
        put.assert_status_ok();
        let body: Value = put.json();
        assert_eq!(body["collection"], "tasks");
        assert_eq!(body["template"], "# {{title}}\n\n{{notes}}");
        assert_eq!(body["version"], 1);
        assert_eq!(body["updated_by"], "anonymous");
        assert_eq!(body["warnings"].as_array().map_or(0, Vec::len), 1);

        let get = server
            .get("/tenants/default/databases/default/collections/tasks/template")
            .await;
        get.assert_status_ok();
        let body: Value = get.json();
        assert_eq!(body["template"], "# {{title}}\n\n{{notes}}");

        let delete = server
            .delete("/tenants/default/databases/default/collections/tasks/template")
            .await;
        delete.assert_status_ok();
        let body: Value = delete.json();
        assert_eq!(body["collection"], "tasks");
        assert_eq!(body["status"], "deleted");

        server
            .get("/tenants/default/databases/default/collections/tasks/template")
            .await
            .assert_status(StatusCode::NOT_FOUND);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_collection_template_delete_accepts_empty_json_body() {
        let server = test_server();

        server
            .post("/tenants/default/databases/default/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);

        server
            .put("/tenants/default/databases/default/collections/tasks/template")
            .json(&json!({
                "template": "# {{title}}"
            }))
            .await
            .assert_status_ok();

        let delete = server
            .delete("/tenants/default/databases/default/collections/tasks/template")
            .content_type("application/json")
            .bytes(Bytes::new())
            .await;
        delete.assert_status_ok();
        let body: Value = delete.json();
        assert_eq!(body["collection"], "tasks");
        assert_eq!(body["status"], "deleted");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_collection_template_responses_preserve_qualified_collection_id() {
        let (server, handler) = test_server_with_handler();
        let qualified = CollectionId::new("prod.billing.tasks");
        let bare = CollectionId::new("tasks");
        let billing = Namespace::new("prod", "billing");

        {
            let mut handler = handler.lock().await;
            ok_or_panic(
                handler.storage_mut().create_database("prod"),
                "creating database for qualified template HTTP test",
            );
            ok_or_panic(
                handler.storage_mut().create_namespace(&billing),
                "creating namespace for qualified template HTTP test",
            );
            ok_or_panic(
                handler
                    .storage_mut()
                    .register_collection_in_namespace(&bare, &billing),
                "registering collection in namespace for qualified template HTTP test",
            );
            ok_or_panic(
                handler.storage_mut().put_schema(&CollectionSchema {
                    collection: qualified.clone(),
                    description: None,
                    version: 1,
                    entity_schema: Some(json!({
                        "type": "object",
                        "properties": {
                            "title": {"type": "string"}
                        },
                        "required": ["title"]
                    })),
                    link_types: Default::default(),
                    access_control: None,
                    gates: Default::default(),
                    validation_rules: Default::default(),
                    indexes: Default::default(),
                    compound_indexes: Default::default(),
                    queries: Default::default(),
                    lifecycles: Default::default(),
                }),
                "storing qualified schema for template HTTP test",
            );
        }

        let put = server
            .put("/tenants/default/databases/default/collections/prod.billing.tasks/template")
            .json(&json!({
                "template": "# {{title}}",
                "actor": "operator"
            }))
            .await;
        put.assert_status_ok();
        let body: Value = put.json();
        assert_eq!(body["collection"], "prod.billing.tasks");
        assert_eq!(body["template"], "# {{title}}");

        let get = server
            .get("/tenants/default/databases/default/collections/prod.billing.tasks/template")
            .await;
        get.assert_status_ok();
        let body: Value = get.json();
        assert_eq!(body["collection"], "prod.billing.tasks");
        assert_eq!(body["template"], "# {{title}}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_collection_template_put_accepts_text_plain_body() {
        let server = test_server();

        server
            .post("/tenants/default/databases/default/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);

        let put = server
            .put("/tenants/default/databases/default/collections/tasks/template")
            .text("# {{title}}")
            .await;
        put.assert_status_ok();
        let body: Value = put.json();
        assert_eq!(body["template"], "# {{title}}");
        assert_eq!(body["warnings"], json!([]));

        server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "hello"}}))
            .await
            .assert_status(StatusCode::CREATED);

        let markdown = server
            .get("/tenants/default/databases/default/collections/tasks/entities/t-001?format=markdown")
            .await;
        markdown.assert_status_ok();
        assert_eq!(markdown.text(), "# hello");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_collection_template_put_rejects_unknown_schema_fields() {
        let server = test_server();

        server
            .post("/tenants/default/databases/default/collections/tasks")
            .json(&json!({
                "schema": {
                    "entity_schema": {
                        "type": "object",
                        "properties": {
                            "title": {"type": "string"}
                        },
                        "required": ["title"]
                    }
                }
            }))
            .await
            .assert_status(StatusCode::CREATED);

        let resp = server
            .put("/tenants/default/databases/default/collections/tasks/template")
            .json(&json!({"template": "{{ghost}}"}))
            .await;

        resp.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
        let body: Value = resp.json();
        assert_eq!(body["code"], "schema_validation");
        assert!(body["detail"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("template references field 'ghost'"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_update_entity() {
        let server = test_server();

        server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v1"}}))
            .await
            .assert_status(StatusCode::CREATED);

        let resp = server
            .put("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v2"}, "expected_version": 1}))
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["entity"]["version"], 2);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_update_version_conflict_returns_409() {
        let server = test_server();

        server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v1"}}))
            .await
            .assert_status(StatusCode::CREATED);

        let resp = server
            .put("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v2"}, "expected_version": 99}))
            .await;
        resp.assert_status(StatusCode::CONFLICT);
        let body: Value = resp.json();
        assert_eq!(body["code"], "version_conflict");
        assert_eq!(body["detail"]["expected"], 99);

        // Verify current_entity is present with correct fields (hx-b2c2a758).
        let current = &body["detail"]["current_entity"];
        assert!(
            !current.is_null(),
            "409 conflict response must include current_entity"
        );
        assert_eq!(current["id"], "t-001");
        assert_eq!(current["version"], 1);
        assert_eq!(current["data"]["title"], "v1");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_delete_entity() {
        let server = test_server();

        server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "bye"}}))
            .await
            .assert_status(StatusCode::CREATED);

        server
            .delete("/tenants/default/databases/default/entities/tasks/t-001")
            .await
            .assert_status_ok();

        server
            .get("/tenants/default/databases/default/entities/tasks/t-001")
            .await
            .assert_status_not_found();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_create_link_and_traverse() {
        let server = test_server();

        // Create two entities.
        server
            .post("/tenants/default/databases/default/entities/users/u-001")
            .json(&json!({"data": {"name": "Alice"}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "Task 1"}}))
            .await
            .assert_status(StatusCode::CREATED);

        // Create link.
        let resp = server
            .post("/tenants/default/databases/default/links")
            .json(&json!({
                "source_collection": "users",
                "source_id": "u-001",
                "target_collection": "tasks",
                "target_id": "t-001",
                "link_type": "owns"
            }))
            .await;
        resp.assert_status(StatusCode::CREATED);

        // Traverse.
        let resp = server
            .get("/tenants/default/databases/default/traverse/users/u-001?link_type=owns")
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["entities"].as_array().unwrap().len(), 1);
        assert_eq!(body["entities"][0]["id"], "t-001");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_create_then_delete_link() {
        let server = test_server();

        // Create two entities.
        server
            .post("/tenants/default/databases/default/entities/users/u-001")
            .json(&json!({"data": {"name": "Alice"}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "Task 1"}}))
            .await
            .assert_status(StatusCode::CREATED);

        // Create link.
        server
            .post("/tenants/default/databases/default/links")
            .json(&json!({
                "source_collection": "users",
                "source_id": "u-001",
                "target_collection": "tasks",
                "target_id": "t-001",
                "link_type": "owns"
            }))
            .await
            .assert_status(StatusCode::CREATED);

        // Verify traverse returns the linked entity.
        let resp = server
            .get("/tenants/default/databases/default/traverse/users/u-001?link_type=owns")
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["entities"].as_array().unwrap().len(), 1);

        // Delete the link.
        let resp = server
            .delete("/tenants/default/databases/default/links")
            .json(&json!({
                "source_collection": "users",
                "source_id": "u-001",
                "target_collection": "tasks",
                "target_id": "t-001",
                "link_type": "owns",
                "actor": "admin"
            }))
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["link_type"], "owns");

        // Traverse now returns no entities.
        let resp = server
            .get("/tenants/default/databases/default/traverse/users/u-001?link_type=owns")
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["entities"].as_array().unwrap().len(), 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_query_audit_log() {
        let server = test_server();

        server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v1"}, "actor": "agent-1"}))
            .await
            .assert_status(StatusCode::CREATED);

        let resp = server
            .get("/tenants/default/databases/default/audit/entity/tasks/t-001")
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        let entries = body["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["actor"], "anonymous");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_query_audit_by_entity_scopes_to_requested_database() {
        let server = test_server();

        server
            .post("/tenants/default/databases/default/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/databases/prod")
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/tenants/default/databases/prod/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);

        server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"scope": "default"}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/tenants/default/databases/prod/entities/tasks/t-001")
            .json(&json!({"data": {"scope": "prod"}}))
            .await
            .assert_status(StatusCode::CREATED);

        // Path-scoped: auditing tasks/t-001 in the prod database returns entries
        // for the fully-qualified "prod.default.tasks" collection.
        let path_scoped_resp = server
            .get("/tenants/default/databases/prod/audit/entity/tasks/t-001")
            .await;
        path_scoped_resp.assert_status_ok();
        let path_scoped_body: Value = path_scoped_resp.json();
        let path_scoped_entries = path_scoped_body["entries"]
            .as_array()
            .expect("path-scoped audit/entity should return an entries array");
        assert!(!path_scoped_entries.is_empty());
        assert!(path_scoped_entries
            .iter()
            .all(|entry| entry["collection"] == "prod.default.tasks"));

        // Cross-database: querying prod database for "default.default.tasks" (a
        // default-scope collection) must return empty — the namespaces don't overlap.
        let cross_database_resp = server
            .get("/tenants/default/databases/prod/audit/entity/default.default.tasks/t-001")
            .await;
        cross_database_resp.assert_status_ok();
        let cross_database_body: Value = cross_database_resp.json();
        let cross_database_entries = cross_database_body["entries"]
            .as_array()
            .expect("cross-database audit/entity should return an entries array");
        assert!(cross_database_entries.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_query_audit_filtered() {
        let server = test_server();

        server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v1"}, "actor": "alice"}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/tenants/default/databases/default/entities/tasks/t-002")
            .json(&json!({"data": {"title": "v2"}, "actor": "bob"}))
            .await
            .assert_status(StatusCode::CREATED);

        // Filter by actor.
        let resp = server
            .get("/tenants/default/databases/default/audit/query?actor=anonymous")
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        let entries = body["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["actor"], "anonymous");

        // Filter by collection.
        let resp = server
            .get("/tenants/default/databases/default/audit/query?collection=tasks")
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["entries"].as_array().unwrap().len(), 2);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_query_audit_rejects_unsupported_metadata_filter() {
        let server = test_server();

        let resp = server
            .get("/tenants/default/databases/default/audit/query?metadata.kind=status_change")
            .await;

        resp.assert_status(StatusCode::BAD_REQUEST);
        let body: Value = resp.json();
        assert_eq!(body["code"], "unsupported_audit_filter");
        assert_eq!(body["detail"]["filter"], "metadata.kind");
    }

    /// Verifies that GET /audit/query honors the multi-collection `collections=` query
    /// parameter (FEAT-003 US-079) and returns entries from the union of requested
    /// collections globally ordered by `audit_id` ascending.
    #[tokio::test(flavor = "multi_thread")]
    async fn http_query_audit_multi_collection_tail() {
        let server = test_server();

        // Interleave entries across three collections. "users" is NOT in the query set
        // and must be excluded from the response, while the cursor must still advance
        // past it in global audit_id order.
        server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v1"}, "actor": "agent"}))
            .await
            .assert_status(StatusCode::CREATED); // audit id = 1
        server
            .post("/tenants/default/databases/default/entities/beads/b-001")
            .json(&json!({"data": {"name": "b1"}, "actor": "agent"}))
            .await
            .assert_status(StatusCode::CREATED); // audit id = 2
        server
            .post("/tenants/default/databases/default/entities/users/u-001")
            .json(&json!({"data": {"name": "u1"}, "actor": "agent"}))
            .await
            .assert_status(StatusCode::CREATED); // audit id = 3 — excluded by filter
        server
            .put("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v2"}, "expected_version": 1, "actor": "agent"}))
            .await
            .assert_status_ok(); // audit id = 4

        // Multi-collection tail: request tasks + beads, expect 3 entries in global id order.
        let resp = server
            .get("/tenants/default/databases/default/audit/query?collections=tasks,beads")
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        let entries = body["entries"].as_array().unwrap();
        assert_eq!(
            entries.len(),
            3,
            "multi-collection tail must return all matching entries"
        );
        assert_eq!(entries[0]["collection"], "tasks");
        assert_eq!(entries[1]["collection"], "beads");
        assert_eq!(entries[2]["collection"], "tasks");
        // Strictly ascending audit_id across all three entries.
        let ids: Vec<u64> = entries.iter().map(|e| e["id"].as_u64().unwrap()).collect();
        assert!(ids[0] < ids[1] && ids[1] < ids[2]);

        // Cursor walks: after_id=ids[0] should skip the first tasks entry.
        let resp = server
            .get(&format!(
                "/tenants/default/databases/default/audit/query?collections=tasks,beads&after_id={}",
                ids[0]
            ))
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        let entries2 = body["entries"].as_array().unwrap();
        assert_eq!(entries2.len(), 2);
        assert!(entries2.iter().all(|e| e["id"].as_u64().unwrap() > ids[0]));

        // Union semantics: ?collection=tasks combined with ?collections=beads should return both.
        let resp = server
            .get(
                "/tenants/default/databases/default/audit/query?collection=tasks&collections=beads",
            )
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        let entries3 = body["entries"].as_array().unwrap();
        assert_eq!(entries3.len(), 3);
        assert!(entries3
            .iter()
            .all(|e| e["collection"] == "tasks" || e["collection"] == "beads"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_tailscale_identity_overrides_body_actor_in_audit() {
        let peer = SocketAddr::from(([100, 64, 0, 10], 3000));
        let auth = AuthContext::with_provider(
            AuthMode::Tailscale {
                default_role: Role::Read,
            },
            Arc::new(FakeWhoisProvider::with_result(
                peer,
                Ok(TailscaleWhoisResponse {
                    node_name: "ts-agent".into(),
                    user_login: "agent@example.com".into(),
                    tags: vec!["tag:axon-write".into()],
                }),
            )),
            Duration::from_secs(60),
        );
        let server = test_server_with_auth(peer, auth);

        server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v1"}, "actor": "spoofed"}))
            .await
            .assert_status(StatusCode::CREATED);

        let resp = server
            .get("/tenants/default/databases/default/audit/query?actor=agent@example.com")
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        let entries = body["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["actor"], "agent@example.com");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_tailscale_rejects_non_tailnet_peer() {
        let peer = SocketAddr::from(([127, 0, 0, 1], 3000));
        let auth = AuthContext::with_provider(
            AuthMode::Tailscale {
                default_role: Role::Read,
            },
            Arc::new(FakeWhoisProvider::with_result(
                peer,
                Err(AuthError::Unauthorized(
                    "peer is not a recognized tailnet address".into(),
                )),
            )),
            Duration::from_secs(60),
        );
        let server = test_server_with_auth(peer, auth);

        let resp = server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v1"}, "actor": "spoofed"}))
            .await;
        resp.assert_status(StatusCode::UNAUTHORIZED);
        let body: Value = resp.json();
        assert_eq!(body["code"], "unauthorized");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_revert_entity() {
        let server = test_server();

        server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v1"}, "actor": "alice"}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .put("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v2"}, "expected_version": 1, "actor": "alice"}))
            .await
            .assert_status_ok();

        // Get audit entries to find the entry_id for the create.
        let resp = server
            .get("/tenants/default/databases/default/audit/query?entity_id=t-001&collection=tasks")
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        let entries = body["entries"].as_array().unwrap();
        // First entry is the create (data_before is null, data_after has v1).
        let create_entry_id = entries[0]["id"].as_u64().unwrap();

        // Revert back to v1 state — but entry 0 is a create (no before), so use entry 1 (update).
        let update_entry_id = entries[1]["id"].as_u64().unwrap();
        let resp = server
            .post("/tenants/default/databases/default/audit/revert")
            .json(&json!({"audit_entry_id": update_entry_id, "actor": "admin"}))
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["entity"]["data"]["title"], "v1");
        // Silence unused variable warning.
        let _ = create_entry_id;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_entity_rollback_by_version_on_v1_route() {
        let server = test_server();

        server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v1"}, "actor": "alice"}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .put("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v2"}, "expected_version": 1, "actor": "alice"}))
            .await
            .assert_status_ok();

        let resp = server
            .post("/tenants/default/databases/default/collections/tasks/entities/t-001/rollback")
            .json(&json!({"to_version": 1}))
            .await;
        resp.assert_status_ok();

        let body: Value = resp.json();
        assert_eq!(body["entity"]["version"], 3);
        assert_eq!(body["entity"]["data"]["title"], "v1");
        assert_eq!(body["audit_entry"]["operation"], "entity.revert");
        assert_eq!(
            body["audit_entry"]["metadata"]["reverted_from_entry_id"],
            "1"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_entity_rollback_recreates_deleted_entity_on_v1_route() {
        let server = test_server();

        server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v1"}, "actor": "creator"}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .put("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v2"}, "expected_version": 1, "actor": "editor"}))
            .await
            .assert_status_ok();
        server
            .delete("/tenants/default/databases/default/entities/tasks/t-001")
            .await
            .assert_status_ok();

        let resp = server
            .post("/tenants/default/databases/default/collections/tasks/entities/t-001/rollback")
            .json(&json!({"to_version": 1}))
            .await;
        resp.assert_status_ok();

        let body: Value = resp.json();
        assert_eq!(body["entity"]["version"], 3);
        assert_eq!(body["entity"]["data"]["title"], "v1");
        assert_eq!(body["audit_entry"]["operation"], "entity.revert");
        assert_eq!(body["audit_entry"]["data_before"], Value::Null);
        assert_eq!(body["audit_entry"]["data_after"]["title"], "v1");
        assert_eq!(
            body["audit_entry"]["metadata"]["reverted_from_entry_id"],
            "1"
        );

        let restored = server
            .get("/tenants/default/databases/default/entities/tasks/t-001")
            .await;
        restored.assert_status_ok();
        let restored_body: Value = restored.json();
        assert_eq!(restored_body["entity"]["version"], 3);
        assert_eq!(restored_body["entity"]["data"]["title"], "v1");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_entity_rollback_dry_run_returns_preview_without_write() {
        let server = test_server();

        server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v1"}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .put("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v2"}, "expected_version": 1}))
            .await
            .assert_status_ok();

        let resp = server
            .post("/tenants/default/databases/default/collections/tasks/entities/t-001/rollback")
            .json(&json!({"to_version": 1, "dry_run": true}))
            .await;
        resp.assert_status_ok();

        let body: Value = resp.json();
        assert_eq!(body["current"]["version"], 2);
        assert_eq!(body["current"]["data"]["title"], "v2");
        assert_eq!(body["target"]["version"], 3);
        assert_eq!(body["target"]["data"]["title"], "v1");
        assert_eq!(body["diff"]["title"]["after"], "v1");

        let current = server
            .get("/tenants/default/databases/default/entities/tasks/t-001")
            .await;
        current.assert_status_ok();
        let current_body: Value = current.json();
        assert_eq!(current_body["entity"]["version"], 2);
        assert_eq!(current_body["entity"]["data"]["title"], "v2");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_entity_rollback_dry_run_previews_deleted_entity_without_recreate() {
        let server = test_server();

        server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v1", "status": "draft"}, "actor": "alice"}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .put("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({
                "data": {"title": "v2", "status": "published"},
                "expected_version": 1,
                "actor": "alice"
            }))
            .await
            .assert_status_ok();
        server
            .delete("/tenants/default/databases/default/entities/tasks/t-001")
            .await
            .assert_status_ok();

        let resp = server
            .post("/tenants/default/databases/default/collections/tasks/entities/t-001/rollback")
            .json(&json!({"to_version": 1, "dry_run": true}))
            .await;
        resp.assert_status_ok();

        let body: Value = resp.json();
        assert_eq!(body["current"], Value::Null);
        assert_eq!(body["target"]["version"], 3);
        assert_eq!(body["target"]["data"]["title"], "v1");
        assert_eq!(body["target"]["data"]["status"], "draft");
        assert_eq!(body["diff"]["title"]["before"], Value::Null);
        assert_eq!(body["diff"]["title"]["after"], "v1");
        assert_eq!(body["diff"]["status"]["after"], "draft");

        server
            .get("/tenants/default/databases/default/entities/tasks/t-001")
            .await
            .assert_status(StatusCode::NOT_FOUND);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_entity_rollback_save_gate_failure_returns_conflict() {
        use axon_api::request::{
            CreateCollectionRequest, CreateEntityRequest, UpdateEntityRequest,
        };
        use axon_schema::rules::{RequirementOp, RuleRequirement, ValidationRule};
        use axon_schema::schema::{CollectionSchema, GateDef};
        use std::collections::HashMap;

        let (server, handler) = test_server_with_handler();
        let col = CollectionId::new("items");
        let id = EntityId::new("g-1");

        {
            let mut guard = handler.lock().await;
            guard
                .create_collection(CreateCollectionRequest {
                    name: col.clone(),
                    schema: CollectionSchema::new(col.clone()),
                    actor: None,
                })
                .unwrap();
            guard
                .create_entity(CreateEntityRequest {
                    collection: col.clone(),
                    id: id.clone(),
                    data: json!({"title": "draft"}),
                    actor: None,
                    audit_metadata: None,
                    attribution: None,
                })
                .unwrap();
            guard
                .put_schema(CollectionSchema {
                    collection: col.clone(),
                    description: None,
                    version: 1,
                    entity_schema: None,
                    link_types: Default::default(),
                    access_control: None,
                    gates: HashMap::from([(
                        "complete".into(),
                        GateDef {
                            description: Some("Ready".into()),
                            includes: vec![],
                        },
                    )]),
                    validation_rules: vec![ValidationRule {
                        name: "need-type".into(),
                        gate: Some("save".into()),
                        advisory: false,
                        when: None,
                        require: RuleRequirement {
                            field: "bead_type".into(),
                            op: RequirementOp::NotNull(true),
                        },
                        message: "bead_type is required".into(),
                        fix: Some("Set bead_type".into()),
                    }],
                    indexes: Default::default(),
                    compound_indexes: Default::default(),
                    queries: Default::default(),
                    lifecycles: Default::default(),
                })
                .unwrap();
            guard
                .update_entity(UpdateEntityRequest {
                    collection: col.clone(),
                    id: id.clone(),
                    data: json!({"title": "draft", "bead_type": "task"}),
                    expected_version: 1,
                    actor: None,
                    audit_metadata: None,
                    attribution: None,
                })
                .unwrap();
        }

        let resp = server
            .post("/tenants/default/databases/default/collections/items/entities/g-1/rollback")
            .json(&json!({"to_version": 1}))
            .await;
        resp.assert_status(StatusCode::CONFLICT);
        let body: Value = resp.json();
        assert_eq!(body["code"], "schema_validation");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_create_and_drop_collection() {
        let server = test_server();

        // Create collection.
        let resp = server
            .post("/tenants/default/databases/default/collections/my-col")
            .json(&json!({"schema": {}, "actor": "admin"}))
            .await;
        resp.assert_status(StatusCode::CREATED);
        let body: Value = resp.json();
        assert_eq!(body["name"], "my-col");

        // Duplicate create returns 409.
        let resp = server
            .post("/tenants/default/databases/default/collections/my-col")
            .json(&json!({"schema": {}, "actor": "admin"}))
            .await;
        resp.assert_status(StatusCode::CONFLICT);
        let body: Value = resp.json();
        assert_eq!(body["code"], "already_exists");

        // Drop collection.
        let resp = server
            .delete("/tenants/default/databases/default/collections/my-col")
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["name"], "my-col");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_query_entities_filter_and_count() {
        let server = test_server();

        // Seed three tasks.
        for (id, status) in [("t-1", "open"), ("t-2", "done"), ("t-3", "open")] {
            server
                .post(&format!(
                    "/tenants/default/databases/default/entities/tasks/{id}"
                ))
                .json(&json!({"data": {"status": status}}))
                .await
                .assert_status(StatusCode::CREATED);
        }

        // Filter: status = "open"
        let resp = server
            .post("/tenants/default/databases/default/collections/tasks/query")
            .json(&json!({
                "filter": {
                    "type": "field",
                    "field": "status",
                    "op": "eq",
                    "value": "open"
                }
            }))
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["total_count"], 2);
        assert_eq!(body["entities"].as_array().unwrap().len(), 2);

        // count_only
        let resp2 = server
            .post("/tenants/default/databases/default/collections/tasks/query")
            .json(&json!({
                "filter": {
                    "type": "field",
                    "field": "status",
                    "op": "eq",
                    "value": "open"
                },
                "count_only": true
            }))
            .await;
        resp2.assert_status_ok();
        let body2: Value = resp2.json();
        assert_eq!(body2["total_count"], 2);
        assert_eq!(body2["entities"].as_array().unwrap().len(), 0);
    }

    // ── Collection list / describe endpoints ─────────────────────────────────

    #[tokio::test(flavor = "multi_thread")]
    async fn http_list_collections_empty() {
        let server = test_server();
        let resp = server
            .get("/tenants/default/databases/default/collections")
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["collections"].as_array().unwrap().len(), 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_list_and_describe_collections() {
        let server = test_server();

        // Create two collections.
        server
            .post("/tenants/default/databases/default/collections/apples")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/tenants/default/databases/default/collections/bananas")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);

        // Seed an entity into "bananas".
        server
            .post("/tenants/default/databases/default/entities/bananas/b-001")
            .json(&json!({"data": {"name": "cavendish"}}))
            .await
            .assert_status(StatusCode::CREATED);

        // List.
        let resp = server
            .get("/tenants/default/databases/default/collections")
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        let cols = body["collections"].as_array().unwrap();
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[0]["name"], "apples");
        assert_eq!(cols[0]["entity_count"], 0);
        assert_eq!(cols[1]["name"], "bananas");
        assert_eq!(cols[1]["entity_count"], 1);

        // Describe "bananas".
        let resp = server
            .get("/tenants/default/databases/default/collections/bananas")
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["name"], "bananas");
        assert_eq!(body["entity_count"], 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_describe_unknown_collection_returns_404() {
        let server = test_server();
        let resp = server
            .get("/tenants/default/databases/default/collections/ghost")
            .await;
        resp.assert_status_not_found();
        let body: Value = resp.json();
        assert_eq!(body["code"], "not_found");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_create_collection_with_invalid_name_returns_400() {
        let server = test_server();
        let resp = server
            .post("/tenants/default/databases/default/collections/BadName")
            .json(&json!({"schema": {}}))
            .await;
        resp.assert_status(StatusCode::BAD_REQUEST);
        let body: Value = resp.json();
        assert_eq!(body["code"], "invalid_argument");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_create_collection_without_schema_returns_400() {
        let server = test_server();
        let resp = server
            .post("/tenants/default/databases/default/collections/good-name")
            .json(&json!({}))
            .await;
        resp.assert_status(StatusCode::BAD_REQUEST);
        let body: Value = resp.json();
        assert_eq!(body["code"], "invalid_argument");
    }

    // ── Schema endpoints ─────────────────────────────────────────────────────

    #[tokio::test(flavor = "multi_thread")]
    async fn http_put_and_get_schema() {
        let server = test_server();

        // PUT schema.
        let resp = server
            .put("/tenants/default/databases/default/collections/invoices/schema")
            .json(&json!({
                "collection": "invoices",
                "version": 1,
                "entity_schema": {
                    "type": "object",
                    "required": ["amount"],
                    "properties": {
                        "amount": {"type": "number"}
                    }
                }
            }))
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["schema"]["collection"], "invoices");

        // GET schema — must return what was stored.
        let resp = server
            .get("/tenants/default/databases/default/collections/invoices/schema")
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["schema"]["collection"], "invoices");
        assert_eq!(body["schema"]["version"], 1);
        assert!(body["schema"]["entity_schema"]["required"].is_array());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_get_schema_missing_returns_404() {
        let server = test_server();
        let resp = server
            .get("/tenants/default/databases/default/collections/nonexistent/schema")
            .await;
        resp.assert_status_not_found();
        let body: Value = resp.json();
        assert_eq!(body["code"], "not_found");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_schema_enforced_on_entity_create() {
        let server = test_server();

        // Register a schema requiring "amount" field.
        server
            .put("/tenants/default/databases/default/collections/payments/schema")
            .json(&json!({
                "collection": "payments",
                "version": 1,
                "entity_schema": {
                    "type": "object",
                    "required": ["amount"],
                    "properties": {
                        "amount": {"type": "number"}
                    }
                }
            }))
            .await
            .assert_status_ok();

        // Entity without "amount" must be rejected.
        let resp = server
            .post("/tenants/default/databases/default/entities/payments/p-001")
            .add_header(REQUEST_ID_HEADER, HeaderValue::from_static("schema-req-1"))
            .json(&json!({"data": {"note": "oops"}}))
            .await;
        resp.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            resp.headers()
                .get(REQUEST_ID_HEADER)
                .and_then(|value| value.to_str().ok()),
            Some("schema-req-1")
        );
        let body: Value = resp.json();
        assert_eq!(body["code"], "schema_validation");
        assert!(body["detail"]["message"].as_str().is_some());
        assert!(body["detail"]["field_errors"]
            .as_array()
            .is_some_and(|errors| !errors.is_empty()));
        assert_eq!(
            body["detail"]["field_errors"][0]["field_path"]
                .as_str()
                .unwrap_or_default(),
            "/"
        );

        // Entity with "amount" must succeed.
        let resp = server
            .post("/tenants/default/databases/default/entities/payments/p-001")
            .json(&json!({"data": {"amount": 42.0}}))
            .await;
        resp.assert_status(StatusCode::CREATED);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_write_rate_limit_uses_stable_error_and_retry_after() {
        let server = test_server_with_rate_limit(crate::rate_limit::RateLimitConfig {
            max_writes: 1,
            window: Duration::from_secs(60),
        });

        server
            .post("/tenants/default/databases/default/entities/limits/first")
            .json(&json!({"data": {"ok": true}}))
            .await
            .assert_status(StatusCode::CREATED);

        let resp = server
            .post("/tenants/default/databases/default/entities/limits/second")
            .json(&json!({"data": {"ok": false}}))
            .await;

        resp.assert_status(StatusCode::TOO_MANY_REQUESTS);
        let retry_after = resp
            .headers()
            .get(header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .expect("429 must include Retry-After seconds");
        assert!(retry_after > 0);
        let body: Value = resp.json();
        assert_eq!(body["code"], "rate_limit_exceeded");
        assert_eq!(
            body["detail"]["retry_after_seconds"].as_u64(),
            Some(retry_after)
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_put_schema_actor_recorded_in_audit() {
        let server = test_server();

        // PUT schema with an explicit actor.
        server
            .put("/tenants/default/databases/default/collections/invoices/schema")
            .json(&json!({
                "version": 1,
                "actor": "schema-admin"
            }))
            .await
            .assert_status_ok();

        // Audit log must contain a SchemaUpdate entry with the resolved actor.
        let resp = server
            .get("/tenants/default/databases/default/audit/query?collection=invoices")
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        let entries = body["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["actor"], "anonymous");
        assert_eq!(entries[0]["mutation"], "schema.update");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_query_entities_and_combinator() {
        let server = test_server();

        server
            .post("/tenants/default/databases/default/entities/tasks/t-1")
            .json(&json!({"data": {"status": "open", "assignee": "alice"}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/tenants/default/databases/default/entities/tasks/t-2")
            .json(&json!({"data": {"status": "open", "assignee": "bob"}}))
            .await
            .assert_status(StatusCode::CREATED);

        let resp = server
            .post("/tenants/default/databases/default/collections/tasks/query")
            .json(&json!({
                "filter": {
                    "type": "and",
                    "filters": [
                        {"type": "field", "field": "status", "op": "eq", "value": "open"},
                        {"type": "field", "field": "assignee", "op": "eq", "value": "alice"}
                    ]
                }
            }))
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["total_count"], 1);
    }

    // Regression tests for route conflict: literal "query" segment must not shadow
    // the {id} capture in /entities/{collection}/{id}.
    #[tokio::test(flavor = "multi_thread")]
    async fn http_entity_with_id_query_create_and_get() {
        let server = test_server();

        // POST /entities/tasks/query must create an entity with ID "query".
        let resp = server
            .post("/tenants/default/databases/default/entities/tasks/query")
            .json(&json!({"data": {"title": "reserved-id"}}))
            .await;
        resp.assert_status(StatusCode::CREATED);
        let body: Value = resp.json();
        assert_eq!(body["entity"]["id"], "query");

        // GET /entities/tasks/query must retrieve the entity with ID "query".
        let resp = server
            .get("/tenants/default/databases/default/entities/tasks/query")
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["entity"]["id"], "query");
        assert_eq!(body["entity"]["data"]["title"], "reserved-id");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_query_endpoint_accessible_at_collections_path() {
        let server = test_server();

        server
            .post("/tenants/default/databases/default/entities/tasks/t-1")
            .json(&json!({"data": {"status": "open"}}))
            .await
            .assert_status(StatusCode::CREATED);

        // POST /collections/{collection}/query is the non-conflicting query endpoint.
        let resp = server
            .post("/tenants/default/databases/default/collections/tasks/query")
            .json(&json!({
                "filter": {
                    "type": "field",
                    "field": "status",
                    "op": "eq",
                    "value": "open"
                }
            }))
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["total_count"], 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_errors_are_structured_with_field_level_details() {
        let server = test_server();

        // Version conflict includes expected/actual fields.
        server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v1"}}))
            .await
            .assert_status(StatusCode::CREATED);

        let resp = server
            .put("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v2"}, "expected_version": 5}))
            .await;
        let body: Value = resp.json();
        assert_eq!(body["code"], "version_conflict");
        // Field-level details: expected and actual versions.
        assert!(body["detail"]["expected"].is_number());
        assert!(body["detail"]["actual"].is_number());
    }

    // ── Transaction endpoint ────────────────────────────────────────────────

    #[tokio::test(flavor = "multi_thread")]
    async fn http_transaction_commits_atomically() {
        let server = test_server();

        // Create two entities first.
        server
            .post("/tenants/default/databases/default/entities/accounts/A")
            .json(&json!({"data": {"balance": 100}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/tenants/default/databases/default/entities/accounts/B")
            .json(&json!({"data": {"balance": 50}}))
            .await
            .assert_status(StatusCode::CREATED);

        // Commit a transaction: debit A, credit B.
        let resp = server
            .post("/tenants/default/databases/default/transactions")
            .json(&json!({
                "operations": [
                    {"op": "update", "collection": "accounts", "id": "A", "data": {"balance": 70}, "expected_version": 1},
                    {"op": "update", "collection": "accounts", "id": "B", "data": {"balance": 80}, "expected_version": 1}
                ],
                "actor": "transfer-agent"
            }))
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert!(body["transaction_id"].is_string());
        assert_eq!(body["entities"].as_array().unwrap().len(), 2);

        // Verify updates applied.
        let resp = server
            .get("/tenants/default/databases/default/entities/accounts/A")
            .await;
        let body: Value = resp.json();
        assert_eq!(body["entity"]["data"]["balance"], 70);
        assert_eq!(body["entity"]["version"], 2);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_transaction_rolls_back_on_conflict() {
        let server = test_server();

        server
            .post("/tenants/default/databases/default/entities/accounts/X")
            .json(&json!({"data": {"balance": 100}}))
            .await
            .assert_status(StatusCode::CREATED);

        // Transaction with wrong expected_version.
        let resp = server
            .post("/tenants/default/databases/default/transactions")
            .json(&json!({
                "operations": [
                    {"op": "update", "collection": "accounts", "id": "X", "data": {"balance": 0}, "expected_version": 99}
                ]
            }))
            .await;
        resp.assert_status(StatusCode::CONFLICT);
        let body: Value = resp.json();
        assert_eq!(body["code"], "version_conflict");

        // Entity must be unchanged.
        let resp = server
            .get("/tenants/default/databases/default/entities/accounts/X")
            .await;
        let body: Value = resp.json();
        assert_eq!(body["entity"]["data"]["balance"], 100);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_transaction_creates_and_deletes() {
        let server = test_server();

        // Seed an entity to delete.
        server
            .post("/tenants/default/databases/default/entities/temp/d-001")
            .json(&json!({"data": {"x": 1}}))
            .await
            .assert_status(StatusCode::CREATED);

        // Transaction: create one entity, delete another.
        let resp = server
            .post("/tenants/default/databases/default/transactions")
            .json(&json!({
                "operations": [
                    {"op": "create", "collection": "temp", "id": "c-001", "data": {"y": 2}},
                    {"op": "delete", "collection": "temp", "id": "d-001", "expected_version": 1}
                ],
                "actor": "batch-agent"
            }))
            .await;
        resp.assert_status_ok();

        // c-001 should exist.
        server
            .get("/tenants/default/databases/default/entities/temp/c-001")
            .await
            .assert_status_ok();
        // d-001 should be gone.
        server
            .get("/tenants/default/databases/default/entities/temp/d-001")
            .await
            .assert_status_not_found();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_transaction_replays_policy_forbidden_response() {
        let (server, handler) = test_server_with_handler();
        let collection = CollectionId::new("tx_policy_denials");
        {
            let mut guard = handler.lock().await;
            guard
                .put_schema(CollectionSchema {
                    collection: collection.clone(),
                    description: None,
                    version: 1,
                    entity_schema: Some(json!({
                        "type": "object",
                        "properties": {
                            "title": {"type": "string"},
                            "secret": {"type": "string"}
                        }
                    })),
                    link_types: Default::default(),
                    access_control: Some(axon_schema::AccessControlPolicy {
                        create: Some(axon_schema::OperationPolicy {
                            allow: vec![axon_schema::PolicyRule {
                                name: Some("allow-all".into()),
                                ..Default::default()
                            }],
                            deny: vec![],
                        }),
                        fields: HashMap::from([(
                            "secret".into(),
                            axon_schema::FieldPolicy {
                                read: None,
                                write: Some(axon_schema::FieldAccessPolicy {
                                    allow: vec![],
                                    deny: vec![axon_schema::FieldPolicyRule {
                                        name: Some("never-write-secret".into()),
                                        when: None,
                                        where_clause: None,
                                        redact_as: None,
                                    }],
                                }),
                            },
                        )]),
                        ..Default::default()
                    }),
                    gates: Default::default(),
                    validation_rules: Default::default(),
                    indexes: Default::default(),
                    compound_indexes: Default::default(),
                    queries: Default::default(),
                    lifecycles: Default::default(),
                })
                .expect("policy schema");
        }

        let body = json!({
            "idempotency_key": "denied-policy-replay-1",
            "operations": [{
                "op": "create",
                "collection": collection.to_string(),
                "id": "denied",
                "data": {"title": "denied", "secret": "classified"}
            }]
        });

        let first = server
            .post("/tenants/default/databases/default/transactions")
            .json(&body)
            .await;
        first.assert_status(StatusCode::FORBIDDEN);
        let first_body: Value = first.json();
        assert_eq!(first_body["code"], "forbidden");
        assert_eq!(first_body["detail"]["reason"], "field_write_denied");
        assert_eq!(first_body["detail"]["collection"], collection.to_string());
        assert_eq!(first_body["detail"]["entity_id"], "denied");
        assert_eq!(first_body["detail"]["field_path"], "secret");
        assert_eq!(first_body["detail"]["policy"], "never-write-secret");
        assert_eq!(first_body["detail"]["operation_index"], 0);

        let replay = server
            .post("/tenants/default/databases/default/transactions")
            .json(&body)
            .await;
        replay.assert_status(StatusCode::FORBIDDEN);
        assert_eq!(
            replay
                .headers()
                .get(IDEMPOTENCY_CACHE_HEADER)
                .map(|value| value.to_str().unwrap()),
            Some("hit")
        );
        let replay_body: Value = replay.json();
        assert_eq!(replay_body, first_body);

        server
            .get("/tenants/default/databases/default/entities/tx_policy_denials/denied")
            .await
            .assert_status_not_found();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_namespaced_entity_paths_isolate_same_named_collections() {
        let (server, handler) = test_server_with_handler();
        let billing = Namespace::new("prod", "billing");
        let engineering = Namespace::new("prod", "engineering");
        let invoices = CollectionId::new("invoices");
        let billing_invoices = CollectionId::new("prod.billing.invoices");
        let engineering_invoices = CollectionId::new("prod.engineering.invoices");

        {
            let mut guard = handler.lock().await;
            let storage = guard.storage_mut();
            storage
                .create_database("prod")
                .expect("database create should succeed");
            storage
                .create_namespace(&billing)
                .expect("billing namespace create should succeed");
            storage
                .create_namespace(&engineering)
                .expect("engineering namespace create should succeed");
            storage
                .register_collection_in_namespace(&invoices, &billing)
                .expect("billing collection register should succeed");
            storage
                .register_collection_in_namespace(&invoices, &engineering)
                .expect("engineering collection register should succeed");

            let schema = |collection: CollectionId| CollectionSchema {
                collection,
                description: None,
                version: 1,
                entity_schema: None,
                link_types: Default::default(),
                access_control: None,
                gates: Default::default(),
                validation_rules: Default::default(),
                indexes: vec![IndexDef {
                    field: "external_id".into(),
                    index_type: IndexType::String,
                    unique: true,
                }],
                compound_indexes: Default::default(),
                queries: Default::default(),
                lifecycles: Default::default(),
            };
            storage
                .put_schema(&schema(billing_invoices.clone()))
                .expect("billing schema put should succeed");
            storage
                .put_schema(&schema(engineering_invoices.clone()))
                .expect("engineering schema put should succeed");
        }

        server
            .post("/tenants/default/databases/default/entities/prod.billing.invoices/inv-001")
            .json(&json!({"data": {"external_id": "shared-1", "scope": "billing"}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/tenants/default/databases/default/entities/prod.engineering.invoices/inv-001")
            .json(&json!({"data": {"external_id": "shared-1", "scope": "engineering"}}))
            .await
            .assert_status(StatusCode::CREATED);

        let resp = server
            .get("/tenants/default/databases/default/entities/prod.billing.invoices/inv-001")
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["entity"]["collection"], "prod.billing.invoices");
        assert_eq!(body["entity"]["data"]["scope"], "billing");

        let resp = server
            .post("/tenants/default/databases/default/collections/prod.engineering.invoices/query")
            .json(&json!({
                "filter": {
                    "type": "field",
                    "field": "external_id",
                    "op": "eq",
                    "value": "shared-1"
                }
            }))
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["total_count"], 1);
        assert_eq!(
            body["entities"][0]["collection"],
            "prod.engineering.invoices"
        );
        assert_eq!(body["entities"][0]["data"]["scope"], "engineering");

        server
            .delete("/tenants/default/databases/default/entities/prod.billing.invoices/inv-001")
            .await
            .assert_status_ok();
        server
            .get("/tenants/default/databases/default/entities/prod.billing.invoices/inv-001")
            .await
            .assert_status_not_found();
        server
            .get("/tenants/default/databases/default/entities/prod.engineering.invoices/inv-001")
            .await
            .assert_status_ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_path_current_database_routes_unqualified_collection_operations() {
        let server = test_server();

        server
            .post("/tenants/default/databases/default/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/databases/prod")
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/tenants/default/databases/prod/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);

        server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"scope": "default"}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/tenants/default/databases/prod/entities/tasks/t-001")
            .json(&json!({"data": {"scope": "prod"}}))
            .await
            .assert_status(StatusCode::CREATED);

        let default_resp = server
            .get("/tenants/default/databases/default/entities/tasks/t-001")
            .await;
        default_resp.assert_status_ok();
        let default_body: Value = default_resp.json();
        assert_eq!(default_body["entity"]["data"]["scope"], "default");

        let prod_resp = server
            .get("/tenants/default/databases/prod/entities/tasks/t-001")
            .await;
        prod_resp.assert_status_ok();
        let prod_body: Value = prod_resp.json();
        assert_eq!(prod_body["entity"]["collection"], "prod.default.tasks");
        assert_eq!(prod_body["entity"]["data"]["scope"], "prod");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_db_path_prefix_routes_unqualified_collection_operations() {
        let server = test_server();

        server
            .post("/tenants/default/databases/default/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/databases/prod")
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/tenants/default/databases/prod/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);

        server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"scope": "default"}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/tenants/default/databases/prod/entities/tasks/t-001")
            .json(&json!({"data": {"scope": "prod"}}))
            .await
            .assert_status(StatusCode::CREATED);

        let prod_resp = server
            .get("/tenants/default/databases/prod/entities/tasks/t-001")
            .await;
        prod_resp.assert_status_ok();
        let prod_body: Value = prod_resp.json();
        assert_eq!(prod_body["entity"]["collection"], "prod.default.tasks");
        assert_eq!(prod_body["entity"]["data"]["scope"], "prod");

        let default_resp = server
            .get("/tenants/default/databases/default/entities/tasks/t-001")
            .await;
        default_resp.assert_status_ok();
        let default_body: Value = default_resp.json();
        assert_eq!(default_body["entity"]["data"]["scope"], "default");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_collection_listings_scope_to_selected_database_only_when_requested() {
        let server = test_server();

        server
            .post("/tenants/default/databases/default/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/databases/prod")
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/tenants/default/databases/prod/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);

        // Default-scoped: only collections belonging to the default database.
        let default_resp = server
            .get("/tenants/default/databases/default/collections")
            .await;
        default_resp.assert_status_ok();
        let default_body: Value = default_resp.json();
        let default_collections = default_body["collections"]
            .as_array()
            .expect("default-scoped collection list should be an array");
        assert_eq!(default_collections.len(), 1);
        assert_eq!(default_collections[0]["name"], "tasks");

        // Prod-scoped: only collections belonging to the prod database.
        let path_scoped_resp = server
            .get("/tenants/default/databases/prod/collections")
            .await;
        path_scoped_resp.assert_status_ok();
        let path_scoped_body: Value = path_scoped_resp.json();
        let path_scoped_collections = path_scoped_body["collections"]
            .as_array()
            .expect("path scoped collection list should be an array");
        assert_eq!(path_scoped_collections.len(), 1);
        assert_eq!(path_scoped_collections[0]["name"], "prod.default.tasks");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_audit_queries_scope_to_selected_database_only_when_requested() {
        let server = test_server();

        server
            .post("/tenants/default/databases/default/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/databases/prod")
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/tenants/default/databases/prod/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);

        server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"scope": "default"}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/tenants/default/databases/prod/entities/tasks/t-001")
            .json(&json!({"data": {"scope": "prod"}}))
            .await
            .assert_status(StatusCode::CREATED);

        // Default-scoped: audit query returns only entries for default database.
        let default_resp = server
            .get("/tenants/default/databases/default/audit/query")
            .await;
        default_resp.assert_status_ok();
        let default_body: Value = default_resp.json();
        let default_entries = default_body["entries"]
            .as_array()
            .expect("default-scoped audit query should return an entries array");
        assert!(default_entries
            .iter()
            .any(|entry| entry["collection"] == "tasks"));
        assert!(!default_entries
            .iter()
            .any(|entry| entry["collection"] == "prod.default.tasks"));

        // Prod-scoped: audit query returns only entries for prod database.
        let path_scoped_resp = server
            .get("/tenants/default/databases/prod/audit/query")
            .await;
        path_scoped_resp.assert_status_ok();
        let path_scoped_body: Value = path_scoped_resp.json();
        let path_scoped_entries = path_scoped_body["entries"]
            .as_array()
            .expect("path scoped audit query should return an entries array");
        assert!(!path_scoped_entries.is_empty());
        assert!(path_scoped_entries
            .iter()
            .all(|entry| entry["collection"] == "prod.default.tasks"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_health_returns_ok_with_version() {
        let server = test_server();
        let resp = server.get("/health").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["status"], "ok");
        assert!(body["version"].is_string());
        assert!(body["uptime_seconds"].is_number());
        assert_eq!(body["backing_store"]["backend"], "memory");
        assert_eq!(body["backing_store"]["status"], "ok");
        assert_eq!(body["default_namespace"], "default.default");
        assert_eq!(body["default_namespace_status"], "ok");
        assert!(body["databases"].is_array());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_health_reports_default_namespace_from_storage_state() {
        let (server, handler) = test_server_with_handler();

        handler
            .lock()
            .await
            .storage_mut()
            .drop_database(DEFAULT_DATABASE)
            .expect("direct storage drop of default database should succeed for health regression");

        let resp = server.get("/health").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert!(body["default_namespace"].is_null());
        assert_eq!(body["default_namespace_status"], "missing");
        let databases = body["databases"]
            .as_array()
            .expect("health databases should be an array");
        assert!(!databases
            .iter()
            .any(|database| database.as_str() == Some(DEFAULT_DATABASE)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_rejects_dropping_default_database_and_preserves_zero_config_paths() {
        let server = test_server();

        let drop_resp = server.delete("/databases/default?force=true").await;
        drop_resp.assert_status(StatusCode::BAD_REQUEST);
        let drop_body: Value = drop_resp.json();
        assert_eq!(drop_body["code"], "invalid_operation");
        assert!(drop_body["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("database 'default' is implicit and cannot be dropped"));

        server
            .post("/tenants/default/databases/default/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);

        let health_resp = server.get("/health").await;
        health_resp.assert_status_ok();
        let health_body: Value = health_resp.json();
        assert_eq!(health_body["default_namespace"], "default.default");
        assert_eq!(health_body["default_namespace_status"], "ok");
        assert!(health_body["databases"]
            .as_array()
            .expect("health databases should be an array")
            .iter()
            .any(|database| database.as_str() == Some(DEFAULT_DATABASE)));

        let collections_resp = server
            .get("/databases/default/schemas/default/collections")
            .await;
        collections_resp.assert_status_ok();
        let collections_body: Value = collections_resp.json();
        assert!(collections_body["collections"]
            .as_array()
            .expect("default namespace collection list should be an array")
            .iter()
            .any(|collection| collection == "tasks"));
    }

    // ── Embedded UI tests ─────────────────────────────────────────────────────
    // These tests exercise the embedded-UI path (ui_dir = None), which is the
    // default when axon is run without --ui-dir.  They rely on the real
    // ui/build assets compiled into the binary via rust-embed.

    #[tokio::test(flavor = "multi_thread")]
    async fn http_root_redirects_to_ui() {
        let server = test_server();
        let resp = server.get("/").await;
        resp.assert_status(StatusCode::TEMPORARY_REDIRECT);
        let location = resp
            .headers()
            .get(header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(
            location.ends_with("/ui"),
            "expected redirect to /ui, got: {location}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_embedded_ui_index_returns_html() {
        let server = test_server();
        let resp = server.get("/ui").await;
        resp.assert_status_ok();
        let ct = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(ct.starts_with("text/html"), "expected text/html, got: {ct}");
        // SvelteKit's static build always starts with <!DOCTYPE html>
        assert!(
            resp.text().starts_with("<!DOCTYPE html"),
            "expected HTML document"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_embedded_ui_static_asset_served_with_correct_content_type() {
        // _app/env.js is a stable filename generated by SvelteKit.
        let server = test_server();
        let resp = server.get("/ui/_app/env.js").await;
        resp.assert_status_ok();
        let ct = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(
            ct.starts_with("application/javascript") || ct.starts_with("text/javascript"),
            "expected JS content-type, got: {ct}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_embedded_ui_unknown_path_falls_back_to_index_html() {
        // Unrecognised paths must return index.html so SvelteKit's client-side
        // router can handle them without a hard 404.
        let server = test_server();
        let resp = server.get("/ui/some/deep/spa/route").await;
        resp.assert_status_ok();
        let ct = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(
            ct.starts_with("text/html"),
            "fallback must be HTML, got: {ct}"
        );
        assert!(
            resp.text().starts_with("<!DOCTYPE html"),
            "expected index.html fallback"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_database_and_namespace_endpoints_round_trip() {
        let server = test_server();

        let resp = server.post("/databases/prod").await;
        resp.assert_status(StatusCode::CREATED);
        let body: Value = resp.json();
        assert_eq!(body["name"], "prod");

        let resp = server.get("/databases").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert!(body["databases"]
            .as_array()
            .is_some_and(|databases| { databases.iter().any(|value| value == "prod") }));

        let resp = server.post("/databases/prod/schemas/billing").await;
        resp.assert_status(StatusCode::CREATED);
        let body: Value = resp.json();
        assert_eq!(body["database"], "prod");
        assert_eq!(body["schema"], "billing");

        let resp = server.get("/databases/prod/schemas").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert!(body["schemas"]
            .as_array()
            .is_some_and(|schemas| schemas.iter().any(|value| value == "billing")));

        let resp = server
            .get("/databases/prod/schemas/billing/collections")
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["collections"], json!([]));
    }

    // ── RBAC role boundary enforcement tests ──────────────────────────────────

    /// Build a test server where every request is authenticated with the given role.
    fn test_server_with_role(role: Role) -> TestServer {
        let peer: SocketAddr = "100.64.0.1:12345".parse().unwrap();
        let provider = Arc::new(FakeWhoisProvider::with_result(
            peer,
            Ok(TailscaleWhoisResponse {
                node_name: "test-node".into(),
                user_login: "test@example.com".into(),
                tags: match role {
                    Role::Admin => vec!["tag:axon-admin".into()],
                    Role::Write => vec!["tag:axon-write".into()],
                    Role::Read => vec!["tag:axon-read".into()],
                },
            }),
        ));
        let auth = AuthContext::with_provider(
            AuthMode::Tailscale {
                default_role: Role::Read,
            },
            provider,
            Duration::from_secs(300),
        );
        let storage: Box<dyn StorageAdapter + Send + Sync> =
            Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite should open"));
        let handler: TenantHandler = Arc::new(Mutex::new(AxonHandler::new(storage)));
        let tenant_router = Arc::new(TenantRouter::single(handler));
        let app = build_router_with_auth(
            tenant_router,
            "memory",
            None,
            auth,
            crate::rate_limit::RateLimitConfig::default(),
            ActorScopeGuard::default(),
            None,
            CorsStore::default(),
        )
        .layer(MockConnectInfo(peer));
        TestServer::new(app)
    }

    /// Seed an entity and a collection with schema using an admin test server,
    /// then return a new test server with the given role for boundary testing.
    /// Returns the server and the seed collection name ("tasks").
    async fn seeded_server_with_role(role: Role) -> TestServer {
        let peer: SocketAddr = "100.64.0.1:12345".parse().unwrap();
        let provider = Arc::new(FakeWhoisProvider::with_result(
            peer,
            Ok(TailscaleWhoisResponse {
                node_name: "test-node".into(),
                user_login: "test@example.com".into(),
                tags: match role {
                    Role::Admin => vec!["tag:axon-admin".into()],
                    Role::Write => vec!["tag:axon-write".into()],
                    Role::Read => vec!["tag:axon-read".into()],
                },
            }),
        ));
        let auth = AuthContext::with_provider(
            AuthMode::Tailscale {
                default_role: Role::Read,
            },
            provider,
            Duration::from_secs(300),
        );
        let storage: Box<dyn StorageAdapter + Send + Sync> =
            Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite should open"));
        let handler: TenantHandler = Arc::new(Mutex::new(AxonHandler::new(storage)));

        // Seed data directly via the handler (bypasses RBAC).
        {
            use axon_api::request::{CreateCollectionRequest, CreateEntityRequest};
            let mut guard = handler.lock().await;
            guard
                .create_collection(CreateCollectionRequest {
                    name: CollectionId::new("tasks"),
                    schema: axon_schema::schema::CollectionSchema::new(CollectionId::new("tasks")),
                    actor: None,
                })
                .unwrap();
            guard
                .create_entity(CreateEntityRequest {
                    collection: CollectionId::new("tasks"),
                    id: axon_core::id::EntityId::new("t-001"),
                    data: json!({"title": "seed entity"}),
                    actor: None,
                    audit_metadata: None,
                    attribution: None,
                })
                .unwrap();
            // Create a second entity for link tests.
            guard
                .create_entity(CreateEntityRequest {
                    collection: CollectionId::new("tasks"),
                    id: axon_core::id::EntityId::new("t-002"),
                    data: json!({"title": "link target"}),
                    actor: None,
                    audit_metadata: None,
                    attribution: None,
                })
                .unwrap();
        }

        let tenant_router = Arc::new(TenantRouter::single(handler));
        let app = build_router_with_auth(
            tenant_router,
            "memory",
            None,
            auth,
            crate::rate_limit::RateLimitConfig::default(),
            ActorScopeGuard::default(),
            None,
            CorsStore::default(),
        )
        .layer(MockConnectInfo(peer));
        TestServer::new(app)
    }

    // ── Admin role: all operations succeed ────────────────────────────────

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_admin_can_create_entity() {
        let server = test_server_with_role(Role::Admin);
        let resp = server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "hello"}}))
            .await;
        resp.assert_status(StatusCode::CREATED);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_admin_can_get_entity() {
        let server = seeded_server_with_role(Role::Admin).await;
        let resp = server
            .get("/tenants/default/databases/default/entities/tasks/t-001")
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_admin_can_update_entity() {
        let server = seeded_server_with_role(Role::Admin).await;
        let resp = server
            .put("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "updated"}, "expected_version": 1}))
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_admin_can_delete_entity() {
        let server = seeded_server_with_role(Role::Admin).await;
        let resp = server
            .delete("/tenants/default/databases/default/entities/tasks/t-001")
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_admin_can_create_collection() {
        let server = test_server_with_role(Role::Admin);
        let resp = server
            .post("/tenants/default/databases/default/collections/new-col")
            .json(&json!({"schema": {}}))
            .await;
        resp.assert_status(StatusCode::CREATED);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_admin_can_drop_collection() {
        let server = seeded_server_with_role(Role::Admin).await;
        let resp = server
            .delete("/tenants/default/databases/default/collections/tasks")
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_admin_can_put_schema() {
        let server = seeded_server_with_role(Role::Admin).await;
        let resp = server
            .put("/tenants/default/databases/default/collections/tasks/schema")
            .json(&json!({
                "version": 2,
                "entity_schema": {"type": "object"}
            }))
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_admin_can_create_link() {
        let server = seeded_server_with_role(Role::Admin).await;
        let resp = server
            .post("/tenants/default/databases/default/links")
            .json(&json!({
                "source_collection": "tasks",
                "source_id": "t-001",
                "target_collection": "tasks",
                "target_id": "t-002",
                "link_type": "blocks"
            }))
            .await;
        resp.assert_status(StatusCode::CREATED);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_admin_can_delete_link() {
        let server = seeded_server_with_role(Role::Admin).await;
        // Create a link first.
        server
            .post("/tenants/default/databases/default/links")
            .json(&json!({
                "source_collection": "tasks",
                "source_id": "t-001",
                "target_collection": "tasks",
                "target_id": "t-002",
                "link_type": "blocks"
            }))
            .await
            .assert_status(StatusCode::CREATED);
        let resp = server
            .delete("/tenants/default/databases/default/links")
            .json(&json!({
                "source_collection": "tasks",
                "source_id": "t-001",
                "target_collection": "tasks",
                "target_id": "t-002",
                "link_type": "blocks"
            }))
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_admin_can_put_template() {
        let server = seeded_server_with_role(Role::Admin).await;
        let resp = server
            .put("/tenants/default/databases/default/collections/tasks/template")
            .json(&json!({"template": "# {{title}}"}))
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_admin_can_delete_template() {
        let server = seeded_server_with_role(Role::Admin).await;
        server
            .put("/tenants/default/databases/default/collections/tasks/template")
            .json(&json!({"template": "# {{title}}"}))
            .await
            .assert_status_ok();
        let resp = server
            .delete("/tenants/default/databases/default/collections/tasks/template")
            .await;
        resp.assert_status_ok();
    }

    // ── Write role: write ops succeed, admin ops return 403 ──────────────

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_write_can_create_entity() {
        let server = test_server_with_role(Role::Write);
        let resp = server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "hello"}}))
            .await;
        resp.assert_status(StatusCode::CREATED);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_write_can_get_entity() {
        let server = seeded_server_with_role(Role::Write).await;
        let resp = server
            .get("/tenants/default/databases/default/entities/tasks/t-001")
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_write_can_update_entity() {
        let server = seeded_server_with_role(Role::Write).await;
        let resp = server
            .put("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "updated"}, "expected_version": 1}))
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_write_can_delete_entity() {
        let server = seeded_server_with_role(Role::Write).await;
        let resp = server
            .delete("/tenants/default/databases/default/entities/tasks/t-001")
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_write_can_create_link() {
        let server = seeded_server_with_role(Role::Write).await;
        let resp = server
            .post("/tenants/default/databases/default/links")
            .json(&json!({
                "source_collection": "tasks",
                "source_id": "t-001",
                "target_collection": "tasks",
                "target_id": "t-002",
                "link_type": "blocks"
            }))
            .await;
        resp.assert_status(StatusCode::CREATED);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_write_can_delete_link() {
        let server = seeded_server_with_role(Role::Write).await;
        server
            .post("/tenants/default/databases/default/links")
            .json(&json!({
                "source_collection": "tasks",
                "source_id": "t-001",
                "target_collection": "tasks",
                "target_id": "t-002",
                "link_type": "blocks"
            }))
            .await
            .assert_status(StatusCode::CREATED);
        let resp = server
            .delete("/tenants/default/databases/default/links")
            .json(&json!({
                "source_collection": "tasks",
                "source_id": "t-001",
                "target_collection": "tasks",
                "target_id": "t-002",
                "link_type": "blocks"
            }))
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_write_cannot_create_collection() {
        let server = test_server_with_role(Role::Write);
        let resp = server
            .post("/tenants/default/databases/default/collections/new-col")
            .json(&json!({"schema": {}}))
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
        let body: Value = resp.json();
        assert_eq!(body["code"], "forbidden");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_write_cannot_drop_collection() {
        let server = seeded_server_with_role(Role::Write).await;
        let resp = server
            .delete("/tenants/default/databases/default/collections/tasks")
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_write_cannot_put_schema() {
        let server = seeded_server_with_role(Role::Write).await;
        let resp = server
            .put("/tenants/default/databases/default/collections/tasks/schema")
            .json(&json!({
                "version": 2,
                "entity_schema": {"type": "object"}
            }))
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_write_cannot_put_template() {
        let server = seeded_server_with_role(Role::Write).await;
        let resp = server
            .put("/tenants/default/databases/default/collections/tasks/template")
            .json(&json!({"template": "# {{title}}"}))
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_write_cannot_delete_template() {
        let server = seeded_server_with_role(Role::Write).await;
        let resp = server
            .delete("/tenants/default/databases/default/collections/tasks/template")
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    // ── Read role: only read ops succeed, write/admin ops return 403 ─────

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_read_can_get_entity() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server
            .get("/tenants/default/databases/default/entities/tasks/t-001")
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_read_can_list_collections() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server
            .get("/tenants/default/databases/default/collections")
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_read_can_describe_collection() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server
            .get("/tenants/default/databases/default/collections/tasks")
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_read_can_get_schema() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server
            .get("/tenants/default/databases/default/collections/tasks/schema")
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_read_can_traverse() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server
            .get("/tenants/default/databases/default/traverse/tasks/t-001?link_type=blocks")
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_read_can_query_audit() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server
            .get("/tenants/default/databases/default/audit/entity/tasks/t-001")
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_read_can_query_entities() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server
            .post("/tenants/default/databases/default/collections/tasks/query")
            .json(&json!({}))
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_read_cannot_create_entity() {
        let server = test_server_with_role(Role::Read);
        let resp = server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "hello"}}))
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
        let body: Value = resp.json();
        assert_eq!(body["code"], "forbidden");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_read_cannot_update_entity() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server
            .put("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "updated"}, "expected_version": 1}))
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_read_cannot_delete_entity() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server
            .delete("/tenants/default/databases/default/entities/tasks/t-001")
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_read_cannot_create_link() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server
            .post("/tenants/default/databases/default/links")
            .json(&json!({
                "source_collection": "tasks",
                "source_id": "t-001",
                "target_collection": "tasks",
                "target_id": "t-002",
                "link_type": "blocks"
            }))
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_read_cannot_delete_link() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server
            .delete("/tenants/default/databases/default/links")
            .json(&json!({
                "source_collection": "tasks",
                "source_id": "t-001",
                "target_collection": "tasks",
                "target_id": "t-002",
                "link_type": "blocks"
            }))
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_read_cannot_create_collection() {
        let server = test_server_with_role(Role::Read);
        let resp = server
            .post("/tenants/default/databases/default/collections/new-col")
            .json(&json!({"schema": {}}))
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_read_cannot_drop_collection() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server
            .delete("/tenants/default/databases/default/collections/tasks")
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_read_cannot_put_schema() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server
            .put("/tenants/default/databases/default/collections/tasks/schema")
            .json(&json!({
                "version": 2,
                "entity_schema": {"type": "object"}
            }))
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_read_cannot_put_template() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server
            .put("/tenants/default/databases/default/collections/tasks/template")
            .json(&json!({"template": "# {{title}}"}))
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_read_cannot_delete_template() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server
            .delete("/tenants/default/databases/default/collections/tasks/template")
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_read_cannot_commit_transaction() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server
            .post("/tenants/default/databases/default/transactions")
            .json(&json!({
                "operations": [{
                    "op": "create",
                    "collection": "tasks",
                    "id": "tx-1",
                    "data": {"title": "txn"}
                }]
            }))
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    // ── Cross-role boundary: forbidden error contains descriptive message ─

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_forbidden_response_is_descriptive() {
        let server = test_server_with_role(Role::Read);
        let resp = server
            .post("/tenants/default/databases/default/entities/tasks/t-001")
            .json(&json!({"data": {"title": "nope"}}))
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
        let body: Value = resp.json();
        assert_eq!(body["code"], "forbidden");
        let detail = body["detail"].as_str().unwrap_or_default();
        assert!(
            detail.contains("permission denied"),
            "forbidden detail should mention 'permission denied': {detail}"
        );
        assert!(
            detail.contains("read"),
            "forbidden detail should mention role: {detail}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rbac_write_forbidden_for_admin_op_is_descriptive() {
        let server = test_server_with_role(Role::Write);
        let resp = server
            .post("/tenants/default/databases/default/collections/new-col")
            .json(&json!({"schema": {}}))
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
        let body: Value = resp.json();
        assert_eq!(body["code"], "forbidden");
        let detail = body["detail"].as_str().unwrap_or_default();
        assert!(
            detail.contains("permission denied"),
            "forbidden detail should mention 'permission denied': {detail}"
        );
        assert!(
            detail.contains("write"),
            "forbidden detail should mention role 'write': {detail}"
        );
        assert!(
            detail.contains("admin"),
            "forbidden detail should mention required role 'admin': {detail}"
        );
    }

    // ── /auth/me endpoint tests ────────────────────────────────────────

    #[tokio::test(flavor = "multi_thread")]
    async fn auth_me_returns_anonymous_admin_in_no_auth_mode() {
        let server = test_server();
        let resp = server.get("/auth/me").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["actor"], "anonymous");
        assert_eq!(body["role"], "admin");
        assert!(body["user_id"].is_null());
        assert!(body["tenant_id"].is_null());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn auth_me_returns_guest_identity_in_guest_mode() {
        let peer = SocketAddr::from(([127, 0, 0, 1], 3000));
        let auth = AuthContext::guest(Role::Read);
        let server = test_server_with_auth(peer, auth);
        let resp = server.get("/auth/me").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["actor"], "guest");
        assert_eq!(body["role"], "read");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn auth_me_returns_tailscale_identity() {
        let peer = SocketAddr::from(([100, 101, 102, 103], 443));
        let auth = AuthContext::with_provider(
            AuthMode::Tailscale {
                default_role: Role::Read,
            },
            Arc::new(FakeWhoisProvider::with_result(
                peer,
                Ok(TailscaleWhoisResponse {
                    node_name: "erik-laptop".into(),
                    user_login: "erik@example.com".into(),
                    tags: vec!["tag:axon-admin".into()],
                }),
            )),
            Duration::from_secs(60),
        );
        let server = test_server_with_auth(peer, auth);
        let resp = server.get("/auth/me").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["actor"], "erik@example.com");
        assert_eq!(body["role"], "admin");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn auth_me_returns_401_for_unauthorized_peer() {
        let peer = SocketAddr::from(([127, 0, 0, 1], 3000));
        let auth = AuthContext::with_provider(
            AuthMode::Tailscale {
                default_role: Role::Read,
            },
            Arc::new(FakeWhoisProvider::with_result(
                peer,
                Err(AuthError::Unauthorized(
                    "peer is not a recognized tailnet address".into(),
                )),
            )),
            Duration::from_secs(60),
        );
        let server = test_server_with_auth(peer, auth);
        let resp = server.get("/auth/me").await;
        resp.assert_status(StatusCode::UNAUTHORIZED);
    }

    // ── CORS middleware tests ─────────────────────────────────────────────────

    fn cors_server(cors: CorsStore) -> axum_test::TestServer {
        let storage: Box<dyn StorageAdapter + Send + Sync> =
            Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite should open"));
        let handler: TenantHandler = Arc::new(Mutex::new(AxonHandler::new(storage)));
        let tenant_router = Arc::new(TenantRouter::single(handler));
        let app = build_router_with_auth(
            tenant_router,
            "memory",
            None,
            AuthContext::no_auth(),
            crate::rate_limit::RateLimitConfig::default(),
            ActorScopeGuard::default(),
            None,
            cors,
        );
        axum_test::TestServer::new(app)
    }

    fn cors_server_with_auth(
        peer: SocketAddr,
        auth: AuthContext,
        cors: CorsStore,
    ) -> axum_test::TestServer {
        let storage: Box<dyn StorageAdapter + Send + Sync> =
            Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite should open"));
        let handler: TenantHandler = Arc::new(Mutex::new(AxonHandler::new(storage)));
        let tenant_router = Arc::new(TenantRouter::single(handler));
        let app = build_router_with_auth(
            tenant_router,
            "memory",
            None,
            auth,
            crate::rate_limit::RateLimitConfig::default(),
            ActorScopeGuard::default(),
            None,
            cors,
        )
        .layer(MockConnectInfo(peer));
        axum_test::TestServer::new(app)
    }

    fn header_list_contains(headers: &HeaderMap, name: &str, expected: &str) -> bool {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .any(|part| part.eq_ignore_ascii_case(expected))
            })
            .unwrap_or(false)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cors_options_preflight_allowed_origin_returns_200_with_headers() {
        let cors = CorsStore::default();
        cors.add_cached("https://sindri:5173");
        let server = cors_server(cors);

        let resp = server
            .method(
                axum::http::Method::OPTIONS,
                "/tenants/default/databases/default/entities/tasks/t-001",
            )
            .add_header(
                axum::http::header::ORIGIN,
                axum::http::HeaderValue::from_static("https://sindri:5173"),
            )
            .add_header(
                axum::http::header::HeaderName::from_static("access-control-request-method"),
                axum::http::HeaderValue::from_static("POST"),
            )
            .await;

        resp.assert_status_ok();
        assert_eq!(
            resp.headers()
                .get("access-control-allow-origin")
                .map(|v| v.to_str().unwrap()),
            Some("https://sindri:5173")
        );
        assert!(resp.headers().contains_key("access-control-allow-methods"));
        assert!(header_list_contains(
            resp.headers(),
            "access-control-allow-headers",
            "content-type"
        ));
        assert!(header_list_contains(
            resp.headers(),
            "access-control-allow-headers",
            "authorization"
        ));
        assert!(header_list_contains(
            resp.headers(),
            "access-control-allow-headers",
            "x-axon-schema-hash"
        ));
        assert!(header_list_contains(
            resp.headers(),
            "access-control-allow-headers",
            "x-axon-actor"
        ));
        assert!(
            !header_list_contains(
                resp.headers(),
                "access-control-allow-headers",
                "idempotency-key"
            ),
            "idempotency-key is a compatibility header, not canonical browser CORS contract"
        );
        assert_eq!(
            resp.headers()
                .get("access-control-max-age")
                .map(|v| v.to_str().unwrap()),
            Some(CORS_MAX_AGE_SECONDS)
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cors_options_preflight_unknown_origin_returns_200_no_cors_headers() {
        let cors = CorsStore::default();
        cors.add_cached("https://allowed.example.com");
        let server = cors_server(cors);

        let resp = server
            .method(
                axum::http::Method::OPTIONS,
                "/tenants/default/databases/default/entities/tasks/t-001",
            )
            .add_header(
                axum::http::header::ORIGIN,
                axum::http::HeaderValue::from_static("https://evil.example.com"),
            )
            .await;

        resp.assert_status_ok();
        assert!(
            resp.headers().get("access-control-allow-origin").is_none(),
            "unknown origin must not receive ACAO header"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cors_wildcard_echoes_star_for_any_origin() {
        let cors = CorsStore::default();
        cors.add_cached("*");
        let server = cors_server(cors);

        let resp = server
            .method(
                axum::http::Method::OPTIONS,
                "/tenants/default/databases/default/entities/tasks/t-001",
            )
            .add_header(
                axum::http::header::ORIGIN,
                axum::http::HeaderValue::from_static("https://any.example.com"),
            )
            .await;

        resp.assert_status_ok();
        assert_eq!(
            resp.headers()
                .get("access-control-allow-origin")
                .map(|v| v.to_str().unwrap()),
            Some("*")
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cors_non_options_request_gets_acao_header_for_allowed_origin() {
        let cors = CorsStore::default();
        cors.add_cached("https://sindri:5173");
        let server = cors_server(cors);

        // POST to a real endpoint (create entity).
        let resp = server
            .post("/tenants/default/databases/default/entities/tasks/t-cors-test")
            .add_header(
                axum::http::header::ORIGIN,
                axum::http::HeaderValue::from_static("https://sindri:5173"),
            )
            .json(&serde_json::json!({"data": {"x": 1}, "actor": "test"}))
            .await;

        // The entity write should succeed and carry ACAO header.
        resp.assert_status(StatusCode::CREATED);
        assert_eq!(
            resp.headers()
                .get("access-control-allow-origin")
                .map(|v| v.to_str().unwrap()),
            Some("https://sindri:5173")
        );
        assert!(header_list_contains(
            resp.headers(),
            "access-control-expose-headers",
            "x-idempotent-cache"
        ));
        assert!(header_list_contains(
            resp.headers(),
            "access-control-expose-headers",
            "x-axon-schema-hash"
        ));
        assert!(header_list_contains(
            resp.headers(),
            "access-control-expose-headers",
            "x-request-id"
        ));
        assert!(header_list_contains(
            resp.headers(),
            "access-control-expose-headers",
            "x-axon-query-cost"
        ));
        assert!(
            resp.headers().contains_key("x-request-id"),
            "every actual response should include a request id"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cors_empty_store_adds_no_headers() {
        let server = cors_server(CorsStore::default());

        let resp = server
            .get("/health")
            .add_header(
                axum::http::header::ORIGIN,
                axum::http::HeaderValue::from_static("https://example.com"),
            )
            .await;

        resp.assert_status_ok();
        assert!(
            resp.headers().get("access-control-allow-origin").is_none(),
            "empty CORS store must not add any CORS headers"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cors_graphql_actual_response_exposes_diagnostic_headers() {
        let cors = CorsStore::default();
        cors.add_cached("https://nexiq.test");
        let server = cors_server(cors);

        let resp = server
            .post("/tenants/default/databases/default/graphql")
            .add_header(
                axum::http::header::ORIGIN,
                axum::http::HeaderValue::from_static("https://nexiq.test"),
            )
            .json(&serde_json::json!({"query": "{ __typename }"}))
            .await;

        resp.assert_status_ok();
        assert_eq!(
            resp.headers()
                .get("access-control-allow-origin")
                .map(|v| v.to_str().unwrap()),
            Some("https://nexiq.test")
        );
        assert!(header_list_contains(
            resp.headers(),
            "access-control-expose-headers",
            "x-request-id"
        ));
        assert!(header_list_contains(
            resp.headers(),
            "access-control-expose-headers",
            "x-axon-query-cost"
        ));
        assert!(resp.headers().contains_key("x-request-id"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cors_schema_manifest_exposes_schema_hash_header() {
        let cors = CorsStore::default();
        cors.add_cached("https://nexiq.test");
        let server = cors_server(cors);

        let resp = server
            .get("/tenants/default/databases/default/schema")
            .add_header(
                axum::http::header::ORIGIN,
                axum::http::HeaderValue::from_static("https://nexiq.test"),
            )
            .await;

        resp.assert_status_ok();
        let schema_hash_header = resp
            .headers()
            .get("x-axon-schema-hash")
            .and_then(|value| value.to_str().ok())
            .expect("schema manifest should emit x-axon-schema-hash")
            .to_string();
        let body: Value = resp.json();
        assert_eq!(body["schema_hash"], schema_hash_header);
        assert!(header_list_contains(
            resp.headers(),
            "access-control-expose-headers",
            "x-axon-schema-hash"
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cors_idempotent_transaction_replay_header_is_browser_readable() {
        let cors = CorsStore::default();
        cors.add_cached("https://nexiq.test");
        let server = cors_server(cors);

        server
            .post("/tenants/default/databases/default/entities/idem/e-1")
            .json(&serde_json::json!({"data": {"v": 0}, "actor": "test"}))
            .await
            .assert_status(StatusCode::CREATED);

        let body = serde_json::json!({
            "idempotency_key": "browser-retry-1",
            "operations": [{
                "op": "update",
                "collection": "idem",
                "id": "e-1",
                "data": {"v": 1},
                "expected_version": 1
            }]
        });

        server
            .post("/tenants/default/databases/default/transactions")
            .add_header(
                axum::http::header::ORIGIN,
                axum::http::HeaderValue::from_static("https://nexiq.test"),
            )
            .json(&body)
            .await
            .assert_status_ok();

        let replay = server
            .post("/tenants/default/databases/default/transactions")
            .add_header(
                axum::http::header::ORIGIN,
                axum::http::HeaderValue::from_static("https://nexiq.test"),
            )
            .json(&body)
            .await;

        replay.assert_status_ok();
        assert_eq!(
            replay
                .headers()
                .get("x-idempotent-cache")
                .map(|value| value.to_str().unwrap()),
            Some("hit")
        );
        assert!(header_list_contains(
            replay.headers(),
            "access-control-expose-headers",
            "x-idempotent-cache"
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cors_options_preflight_bypasses_auth_layer() {
        let cors = CorsStore::default();
        cors.add_cached("https://nexiq.test");
        let peer = SocketAddr::from(([127, 0, 0, 1], 3000));
        let auth = AuthContext::with_provider(
            AuthMode::Tailscale {
                default_role: Role::Read,
            },
            Arc::new(FakeWhoisProvider::with_result(
                peer,
                Err(AuthError::Unauthorized(
                    "peer is not a recognized tailnet address".into(),
                )),
            )),
            Duration::from_secs(60),
        );
        let server = cors_server_with_auth(peer, auth, cors);

        let resp = server
            .method(
                axum::http::Method::OPTIONS,
                "/tenants/default/databases/default/graphql",
            )
            .add_header(
                axum::http::header::ORIGIN,
                axum::http::HeaderValue::from_static("https://nexiq.test"),
            )
            .add_header(
                axum::http::header::HeaderName::from_static("access-control-request-method"),
                axum::http::HeaderValue::from_static("POST"),
            )
            .await;

        resp.assert_status_ok();
        assert_eq!(
            resp.headers()
                .get("access-control-allow-origin")
                .map(|v| v.to_str().unwrap()),
            Some("https://nexiq.test")
        );
    }
}
