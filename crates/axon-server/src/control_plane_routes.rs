//! HTTP routes for control-plane tenant lifecycle management.
//!
//! All endpoints live under `/control` and require the `Admin` role.
//! The control-plane database is separate from the per-tenant data stores.
//!
//! # Tenant model
//!
//! Each tenant owns exactly one database.  When a tenant is created the server
//! auto-generates a `db_name` slug from the tenant name (e.g. "Acme Corp" →
//! `acme-corp-<uuid-prefix>`) and provisions the backing SQLite file.
//! Deleting a tenant removes the SQLite file as well.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Extension, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Mutex;

use axon_core::auth::{
    AuthError, Grants, JwtClaims, ResolvedIdentity, RetentionPolicy, TenantDatabase, TenantId,
    TenantRole, UserId,
};
use axon_storage::StorageAdapter;

use crate::auth::{Identity, Role};
use crate::auth_pipeline::JwtIssuer;
use crate::control_plane::{ControlPlaneDb, Tenant};
use crate::control_plane_authz;
use crate::cors_config::CorsStore;
use crate::gateway::{auth_error_response, ApiError};
use crate::user_roles::UserRoleStore;

/// Shared state for control-plane routes, holding the DB and a data directory
/// where tenant SQLite databases are provisioned.
#[derive(Clone)]
pub struct ControlPlaneState {
    pub db: Arc<Mutex<ControlPlaneDb>>,
    /// Directory where tenant database files are created.
    pub data_dir: PathBuf,
    /// In-memory write-through cache of principal → role assignments.
    ///
    /// Shared with [`AuthContext`] so that role changes take effect within the
    /// next identity-cache TTL window without a server restart.
    pub user_roles: UserRoleStore,
    /// In-memory write-through cache of CORS allowed origins.
    ///
    /// Shared with the CORS middleware layer so that changes take effect on the
    /// next request without a server restart.
    pub cors_store: CorsStore,
    /// Storage adapter for ADR-018 auth/membership tables.
    ///
    /// Populated when the server is configured with JWT-based auth support.
    /// If `None` the membership and retention endpoints return 503.
    pub storage: Option<Arc<std::sync::Mutex<Box<dyn StorageAdapter + Send + Sync>>>>,
    /// JWT issuer used by the optional JWT extraction middleware.
    ///
    /// When `Some`, requests to the membership/retention endpoints that carry
    /// an `Authorization: Bearer …` header are verified and a
    /// [`ResolvedIdentity`] is installed into the request extensions.
    pub jwt_issuer: Option<Arc<JwtIssuer>>,
}

/// Shared handle to the control-plane SQLite database (legacy alias).
pub type SharedControlPlane = Arc<Mutex<ControlPlaneDb>>;

impl ControlPlaneState {
    /// Create a new control-plane state.
    ///
    /// Pass the same `UserRoleStore` and `CorsStore` instances to the auth
    /// context and CORS middleware respectively so that management changes
    /// take effect without a server restart.
    pub fn new(
        db: Arc<Mutex<ControlPlaneDb>>,
        data_dir: PathBuf,
        user_roles: UserRoleStore,
        cors_store: CorsStore,
    ) -> Self {
        Self {
            db,
            data_dir,
            user_roles,
            cors_store,
            storage: None,
            jwt_issuer: None,
        }
    }

    /// Attach a storage adapter for ADR-018 membership and retention tables.
    pub fn with_storage(
        mut self,
        storage: Arc<std::sync::Mutex<Box<dyn StorageAdapter + Send + Sync>>>,
    ) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Attach a JWT issuer to enable optional JWT verification on the
    /// membership/retention endpoints.
    pub fn with_jwt_issuer(mut self, issuer: Arc<JwtIssuer>) -> Self {
        self.jwt_issuer = Some(issuer);
        self
    }

    /// Build the file path for a tenant database.
    ///
    /// Uses `{data_dir}/tenants/{db_name}.db` layout, matching [`crate::tenant_router::TenantRouter`].
    pub fn tenant_db_path(&self, db_name: &str) -> PathBuf {
        self.data_dir.join("tenants").join(format!("{db_name}.db"))
    }
}

// ── Request bodies ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateTenantBody {
    pub name: String,
}

// ── Response types ───────────────────────────────────────────────────────────

#[derive(Serialize)]
struct TenantResponse {
    id: String,
    name: String,
    db_name: String,
    created_at: String,
}

impl From<Tenant> for TenantResponse {
    fn from(t: Tenant) -> Self {
        Self {
            id: t.id,
            name: t.name,
            db_name: t.db_name,
            created_at: t.created_at,
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Generate an ISO-8601 UTC timestamp string.
fn now_iso8601() -> String {
    humantime::format_rfc3339_seconds(std::time::SystemTime::now()).to_string()
}

/// Detect SQLite UNIQUE constraint violations in error messages.
fn is_unique_violation(msg: &str) -> bool {
    msg.contains("UNIQUE constraint failed")
}

/// Derive a filesystem-safe slug from a tenant name.
///
/// Converts to lowercase, replaces non-alphanumeric characters with `-`,
/// collapses runs of `-`, and trims leading/trailing dashes.  Always appends
/// the first 8 hex characters of the supplied UUID to guarantee uniqueness.
///
/// # Examples
/// - `"Acme Corp"` + `"01966b3c-..."` → `"acme-corp-01966b3c"`
/// - `"  -- "` + `"01966b3c-..."` → `"tenant-01966b3c"`
fn name_to_db_slug(name: &str, id: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug: String = slug
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    // Take first 8 hex chars from the UUID (strip dashes).
    let uuid_prefix: String = id.chars().filter(|c| c != &'-').take(8).collect();

    if slug.is_empty() {
        format!("tenant-{uuid_prefix}")
    } else {
        format!("{slug}-{uuid_prefix}")
    }
}

// ── Route builder ────────────────────────────────────────────────────────────

/// Build the `/control` router.  The caller is responsible for nesting this
/// under the `/control` prefix and providing a `ControlPlaneState`.
pub fn control_plane_routes() -> Router<ControlPlaneState> {
    Router::new()
        .route("/tenants", post(create_tenant))
        .route("/tenants", get(list_tenants))
        .route("/tenants/{id}", get(get_tenant))
        .route("/tenants/{id}", delete(delete_tenant))
        // User-role management (legacy login-based assignments)
        .route("/users", get(list_users))
        .route("/users/{login}", put(set_user_role))
        .route("/users/{login}", delete(remove_user_role))
        // User provisioning (ADR-018, axon-0a6eb28a)
        .route("/users/provision", post(create_user_handler))
        .route("/users/list", get(list_users_handler))
        .route("/users/suspend/{id}", delete(suspend_user_handler))
        // CORS origin management
        .route("/cors", get(list_cors_origins))
        .route("/cors", put(add_cors_origin))
        .route("/cors", delete(remove_cors_origin_handler))
        // Tenant membership (ADR-018, axon-c6908e78)
        .route("/tenants/{id}/members", get(list_tenant_members))
        .route("/tenants/{id}/members/{user_id}", put(upsert_tenant_member))
        .route(
            "/tenants/{id}/members/{user_id}",
            delete(remove_tenant_member_handler),
        )
        // Tenant retention policy (axon-c6908e78)
        .route("/tenants/{id}/retention", get(get_retention_policy_handler))
        .route("/tenants/{id}/retention", put(set_retention_policy_handler))
        // Tenant database management (axon-df98e262)
        .route("/tenants/{id}/databases", get(list_tenant_databases))
        .route("/tenants/{id}/databases", post(create_tenant_database))
        .route(
            "/tenants/{id}/databases/{name}",
            delete(delete_tenant_database),
        )
        // Credential issuance (axon-906b527a)
        .route("/tenants/{id}/credentials", post(issue_credential))
        .route("/tenants/{id}/credentials", get(list_credentials_handler))
        .route(
            "/tenants/{id}/credentials/{jti}",
            delete(revoke_credential_handler),
        )
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// `POST /control/tenants` — create a new tenant.
///
/// Auto-generates `db_name` from the tenant name and provisions a backing
/// SQLite database file in the configured data directory.
async fn create_tenant(
    State(state): State<ControlPlaneState>,
    Extension(identity): Extension<Identity>,
    Json(body): Json<CreateTenantBody>,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }

    let id = uuid::Uuid::now_v7().to_string();
    let created_at = now_iso8601();
    let db_name = name_to_db_slug(&body.name, &id);

    let db = state.db.lock().await;
    match db.create_tenant(&id, &body.name, &db_name, &created_at) {
        Ok(()) => {
            // Provision the SQLite database file.
            let db_path = state.tenant_db_path(&db_name);
            if let Err(e) = provision_tenant_database(&db_path) {
                // Best-effort rollback.
                let _ = db.delete_tenant(&id);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiError::new("provisioning_error", e.to_string())),
                )
                    .into_response();
            }

            (
                StatusCode::CREATED,
                Json(json!({
                    "id": id,
                    "name": body.name,
                    "db_name": db_name,
                    "db_path": db_path.display().to_string(),
                    "created_at": created_at,
                })),
            )
                .into_response()
        }
        Err(e) => {
            let msg = e.to_string();
            if is_unique_violation(&msg) {
                (
                    StatusCode::CONFLICT,
                    Json(ApiError::new(
                        "already_exists",
                        format!("tenant with name '{}' already exists", body.name),
                    )),
                )
                    .into_response()
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiError::new("storage_error", msg)),
                )
                    .into_response()
            }
        }
    }
}

/// `GET /control/tenants` — list all tenants.
async fn list_tenants(
    State(state): State<ControlPlaneState>,
    Extension(identity): Extension<Identity>,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }

    let db = state.db.lock().await;
    match db.list_tenants() {
        Ok(tenants) => {
            let payload: Vec<TenantResponse> = tenants.into_iter().map(Into::into).collect();
            Json(json!({ "tenants": payload })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", e.to_string())),
        )
            .into_response(),
    }
}

/// `GET /control/tenants/{id}` — get a single tenant.
async fn get_tenant(
    State(state): State<ControlPlaneState>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<String>,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }

    let db = state.db.lock().await;
    match db.get_tenant(&id) {
        Ok(tenant) => Json(json!(TenantResponse::from(tenant))).into_response(),
        Err(axon_core::error::AxonError::NotFound(_)) => (
            StatusCode::NOT_FOUND,
            Json(ApiError::new("not_found", format!("tenant {id}"))),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", e.to_string())),
        )
            .into_response(),
    }
}

/// `DELETE /control/tenants/{id}` — delete a tenant and its provisioned database.
async fn delete_tenant(
    State(state): State<ControlPlaneState>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<String>,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }

    let db = state.db.lock().await;

    match db.delete_tenant(&id) {
        Ok(db_name) => {
            // Remove the provisioned SQLite file (best-effort).
            let path = state.tenant_db_path(&db_name);
            let _ = std::fs::remove_file(&path);
            (
                StatusCode::OK,
                Json(json!({
                    "deleted": true,
                    "tenant_id": id,
                    "db_name": db_name,
                })),
            )
                .into_response()
        }
        Err(axon_core::error::AxonError::NotFound(_)) => (
            StatusCode::NOT_FOUND,
            Json(ApiError::new("not_found", format!("tenant {id}"))),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", e.to_string())),
        )
            .into_response(),
    }
}

// ── User-role handlers ───────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SetUserRoleBody {
    role: Role,
}

/// `GET /control/users` — list all explicit user-role assignments.
async fn list_users(
    State(state): State<ControlPlaneState>,
    Extension(identity): Extension<Identity>,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }
    let db = state.db.lock().await;
    match db.list_user_roles() {
        Ok(entries) => {
            let users: Vec<_> = entries
                .into_iter()
                .map(|e| json!({ "login": e.login, "role": e.role }))
                .collect();
            Json(json!({ "users": users })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", e.to_string())),
        )
            .into_response(),
    }
}

/// `PUT /control/users/{login}` — assign or update a role for a principal.
async fn set_user_role(
    State(state): State<ControlPlaneState>,
    Extension(identity): Extension<Identity>,
    Path(login): Path<String>,
    Json(body): Json<SetUserRoleBody>,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }
    let db = state.db.lock().await;
    match db.set_user_role(&login, &body.role) {
        Ok(()) => {
            state
                .user_roles
                .set_cached(login.clone(), body.role.clone());
            (
                StatusCode::OK,
                Json(json!({ "login": login, "role": body.role })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", e.to_string())),
        )
            .into_response(),
    }
}

/// `DELETE /control/users/{login}` — remove an explicit role assignment.
async fn remove_user_role(
    State(state): State<ControlPlaneState>,
    Extension(identity): Extension<Identity>,
    Path(login): Path<String>,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }
    let db = state.db.lock().await;
    match db.remove_user_role(&login) {
        Ok(true) => {
            state.user_roles.remove_cached(&login);
            (
                StatusCode::OK,
                Json(json!({ "login": login, "deleted": true })),
            )
                .into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ApiError::new(
                "not_found",
                format!("no explicit role assigned to '{login}'"),
            )),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", e.to_string())),
        )
            .into_response(),
    }
}

// ── CORS origin handlers ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CorsOriginBody {
    origin: String,
}

/// `GET /control/cors` — list all configured CORS allowed origins.
async fn list_cors_origins(
    State(state): State<ControlPlaneState>,
    Extension(identity): Extension<Identity>,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }
    let db = state.db.lock().await;
    match db.list_cors_origins() {
        Ok(origins) => Json(json!({ "origins": origins })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", e.to_string())),
        )
            .into_response(),
    }
}

/// `PUT /control/cors` — add an allowed origin (idempotent).
async fn add_cors_origin(
    State(state): State<ControlPlaneState>,
    Extension(identity): Extension<Identity>,
    Json(body): Json<CorsOriginBody>,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }
    let db = state.db.lock().await;
    match db.add_cors_origin(&body.origin) {
        Ok(()) => {
            state.cors_store.add_cached(body.origin.clone());
            (StatusCode::OK, Json(json!({ "origin": body.origin }))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", e.to_string())),
        )
            .into_response(),
    }
}

/// `DELETE /control/cors` — remove an allowed origin.
async fn remove_cors_origin_handler(
    State(state): State<ControlPlaneState>,
    Extension(identity): Extension<Identity>,
    Json(body): Json<CorsOriginBody>,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }
    let db = state.db.lock().await;
    match db.remove_cors_origin(&body.origin) {
        Ok(true) => {
            state.cors_store.remove_cached(&body.origin);
            (
                StatusCode::OK,
                Json(json!({ "origin": body.origin, "deleted": true })),
            )
                .into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ApiError::new(
                "not_found",
                format!("origin '{}' is not in the CORS allow-list", body.origin),
            )),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", e.to_string())),
        )
            .into_response(),
    }
}

// ── New membership + retention helpers ───────────────────────────────────────

/// Return a 403 Forbidden response for authz failures.
fn forbidden_response(msg: &str) -> Response {
    (StatusCode::FORBIDDEN, Json(ApiError::new("forbidden", msg))).into_response()
}

/// Return a 503 Service Unavailable response when storage is not configured.
fn storage_not_configured() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ApiError::new(
            "not_configured",
            "storage adapter not configured for this endpoint",
        )),
    )
        .into_response()
}

/// Optional JWT extraction middleware.
///
/// If the request carries an `Authorization: Bearer <token>` header AND the
/// [`ControlPlaneState`] has a `jwt_issuer` configured, the token is verified
/// and a [`ResolvedIdentity`] is inserted into the request extensions.
///
/// If no token is present the request passes through unchanged — handlers can
/// fall back to the legacy [`Identity`] extension.
///
/// If a token is present but verification fails the middleware returns 401.
pub async fn optional_jwt_middleware(
    State(state): State<ControlPlaneState>,
    mut request: axum::extract::Request,
    next: Next,
) -> Response {
    use axum::http::header::AUTHORIZATION;

    let auth_value = request.headers().get(AUTHORIZATION).cloned();
    let Some(auth_header) = auth_value else {
        // No token — fall through to legacy auth.
        return next.run(request).await;
    };

    let Some(issuer) = &state.jwt_issuer else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ApiError::new("unauthenticated", "JWT auth not configured")),
        )
            .into_response();
    };

    let auth_str = match auth_header.to_str() {
        Ok(s) => s.to_string(),
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ApiError::new(
                    "unauthenticated",
                    "malformed Authorization header",
                )),
            )
                .into_response();
        }
    };

    let token = match auth_str.strip_prefix("Bearer ") {
        Some(t) => t.to_string(),
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ApiError::new("unauthenticated", "expected Bearer token")),
            )
                .into_response();
        }
    };

    match issuer.verify(&token) {
        Ok(claims) => {
            // Validate exp/nbf with 30 s clock skew.
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            const SKEW: u64 = 30;
            if claims.exp.saturating_add(SKEW) < now {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(ApiError::new("credential_expired", "JWT has expired")),
                )
                    .into_response();
            }
            if claims.nbf > now.saturating_add(SKEW) {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(ApiError::new(
                        "credential_not_yet_valid",
                        "JWT not yet valid",
                    )),
                )
                    .into_response();
            }

            let identity = ResolvedIdentity {
                user_id: UserId::new(&claims.sub),
                tenant_id: TenantId::new(&claims.aud),
                grants: claims.grants,
            };
            request.extensions_mut().insert(identity);
        }
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ApiError::new("credential_invalid", "invalid JWT")),
            )
                .into_response();
        }
    }

    next.run(request).await
}

// ── Membership request/response types ────────────────────────────────────────

#[derive(Deserialize)]
struct UpsertMemberBody {
    role: String,
}

#[derive(Serialize)]
struct MemberResponse {
    tenant_id: String,
    user_id: String,
    role: String,
}

fn tenant_role_from_str(s: &str) -> Option<TenantRole> {
    match s {
        "admin" => Some(TenantRole::Admin),
        "write" => Some(TenantRole::Write),
        "read" => Some(TenantRole::Read),
        _ => None,
    }
}

fn tenant_role_to_str(r: TenantRole) -> &'static str {
    match r {
        TenantRole::Admin => "admin",
        TenantRole::Write => "write",
        TenantRole::Read => "read",
    }
}

// ── Membership handlers ───────────────────────────────────────────────────────

/// `GET /control/tenants/{id}/members` — list tenant memberships.
///
/// Requires tenant-admin or deployment-admin.
async fn list_tenant_members(
    State(state): State<ControlPlaneState>,
    Extension(legacy): Extension<Identity>,
    resolved: Option<Extension<ResolvedIdentity>>,
    Path(id): Path<String>,
) -> Response {
    // Authorization check.
    match &resolved {
        Some(Extension(r)) => {
            let tenant_id = TenantId::new(&id);
            let ok = control_plane_authz::require_tenant_admin(r, tenant_id, &state.user_roles)
                .is_ok()
                || control_plane_authz::require_deployment_admin(r, &state.user_roles).is_ok();
            if !ok {
                return forbidden_response("tenant admin or deployment admin required");
            }
        }
        None => {
            if let Err(e) = legacy.require_admin() {
                return auth_error_response(e);
            }
        }
    }

    let Some(storage) = &state.storage else {
        return storage_not_configured();
    };

    let tenant_id = TenantId::new(&id);
    let result = {
        let s = storage
            .lock()
            .map_err(|_| "storage mutex poisoned".to_string());
        match s {
            Ok(s) => s.list_tenant_members(tenant_id),
            Err(msg) => Err(axon_core::error::AxonError::Storage(msg)),
        }
    };

    match result {
        Ok(members) => {
            let payload: Vec<MemberResponse> = members
                .into_iter()
                .map(|m| MemberResponse {
                    tenant_id: m.tenant_id.0,
                    user_id: m.user_id.0,
                    role: tenant_role_to_str(m.role).to_string(),
                })
                .collect();
            Json(json!({ "members": payload })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", e.to_string())),
        )
            .into_response(),
    }
}

/// `PUT /control/tenants/{id}/members/{user_id}` — upsert tenant membership.
///
/// Requires deployment-admin.
async fn upsert_tenant_member(
    State(state): State<ControlPlaneState>,
    Extension(legacy): Extension<Identity>,
    resolved: Option<Extension<ResolvedIdentity>>,
    Path((id, uid)): Path<(String, String)>,
    Json(body): Json<UpsertMemberBody>,
) -> Response {
    // Authorization check.
    match &resolved {
        Some(Extension(r)) => {
            if let Err(e) = control_plane_authz::require_deployment_admin(r, &state.user_roles) {
                return forbidden_response(&e.to_string());
            }
        }
        None => {
            if let Err(e) = legacy.require_admin() {
                return auth_error_response(e);
            }
        }
    }

    let Some(role) = tenant_role_from_str(&body.role) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                "invalid_role",
                format!("unknown role '{}'", body.role),
            )),
        )
            .into_response();
    };

    let Some(storage) = &state.storage else {
        return storage_not_configured();
    };

    let tenant_id = TenantId::new(&id);
    let user_id = UserId::new(&uid);
    let result = {
        let s = storage
            .lock()
            .map_err(|_| "storage mutex poisoned".to_string());
        match s {
            Ok(s) => s.upsert_tenant_member(tenant_id, user_id, role),
            Err(msg) => Err(axon_core::error::AxonError::Storage(msg)),
        }
    };

    match result {
        Ok(member) => (
            StatusCode::OK,
            Json(json!({
                "tenant_id": member.tenant_id.0,
                "user_id": member.user_id.0,
                "role": tenant_role_to_str(member.role),
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", e.to_string())),
        )
            .into_response(),
    }
}

/// `DELETE /control/tenants/{id}/members/{user_id}` — remove tenant membership.
///
/// Requires deployment-admin. Returns 204 on success, 404 if the membership
/// does not exist.
async fn remove_tenant_member_handler(
    State(state): State<ControlPlaneState>,
    Extension(legacy): Extension<Identity>,
    resolved: Option<Extension<ResolvedIdentity>>,
    Path((id, uid)): Path<(String, String)>,
) -> Response {
    // Authorization check.
    match &resolved {
        Some(Extension(r)) => {
            if let Err(e) = control_plane_authz::require_deployment_admin(r, &state.user_roles) {
                return forbidden_response(&e.to_string());
            }
        }
        None => {
            if let Err(e) = legacy.require_admin() {
                return auth_error_response(e);
            }
        }
    }

    let Some(storage) = &state.storage else {
        return storage_not_configured();
    };

    let tenant_id = TenantId::new(&id);
    let user_id = UserId::new(&uid);
    let result = {
        let s = storage
            .lock()
            .map_err(|_| "storage mutex poisoned".to_string());
        match s {
            Ok(s) => s.remove_tenant_member(tenant_id, user_id),
            Err(msg) => Err(axon_core::error::AxonError::Storage(msg)),
        }
    };

    match result {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ApiError::new(
                "not_found",
                format!("member {uid} not found in tenant {id}"),
            )),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", e.to_string())),
        )
            .into_response(),
    }
}

// ── Retention policy handlers ─────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct RetentionPolicyBody {
    archive_after_seconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    purge_after_seconds: Option<u64>,
}

/// `GET /control/tenants/{id}/retention` — get the tenant's retention policy.
///
/// Returns the default 7-year policy if none has been explicitly configured.
async fn get_retention_policy_handler(
    State(state): State<ControlPlaneState>,
    Extension(legacy): Extension<Identity>,
    resolved: Option<Extension<ResolvedIdentity>>,
    Path(id): Path<String>,
) -> Response {
    // Authorization check.
    match &resolved {
        Some(Extension(r)) => {
            let tenant_id = TenantId::new(&id);
            let ok = control_plane_authz::require_tenant_admin(r, tenant_id, &state.user_roles)
                .is_ok()
                || control_plane_authz::require_deployment_admin(r, &state.user_roles).is_ok();
            if !ok {
                return forbidden_response("tenant admin or deployment admin required");
            }
        }
        None => {
            if let Err(e) = legacy.require_admin() {
                return auth_error_response(e);
            }
        }
    }

    let Some(storage) = &state.storage else {
        return storage_not_configured();
    };

    let tenant_id = TenantId::new(&id);
    let result = {
        let s = storage
            .lock()
            .map_err(|_| "storage mutex poisoned".to_string());
        match s {
            Ok(s) => s.get_retention_policy(tenant_id),
            Err(msg) => Err(axon_core::error::AxonError::Storage(msg)),
        }
    };

    match result {
        Ok(policy) => {
            let p = policy.unwrap_or_default();
            Json(RetentionPolicyBody {
                archive_after_seconds: p.archive_after_seconds,
                purge_after_seconds: p.purge_after_seconds,
            })
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", e.to_string())),
        )
            .into_response(),
    }
}

/// `PUT /control/tenants/{id}/retention` — set or update the tenant's retention policy.
async fn set_retention_policy_handler(
    State(state): State<ControlPlaneState>,
    Extension(legacy): Extension<Identity>,
    resolved: Option<Extension<ResolvedIdentity>>,
    Path(id): Path<String>,
    Json(body): Json<RetentionPolicyBody>,
) -> Response {
    // Authorization check.
    match &resolved {
        Some(Extension(r)) => {
            if let Err(e) = control_plane_authz::require_deployment_admin(r, &state.user_roles) {
                return forbidden_response(&e.to_string());
            }
        }
        None => {
            if let Err(e) = legacy.require_admin() {
                return auth_error_response(e);
            }
        }
    }

    let Some(storage) = &state.storage else {
        return storage_not_configured();
    };

    let tenant_id = TenantId::new(&id);
    let policy = RetentionPolicy {
        archive_after_seconds: body.archive_after_seconds,
        purge_after_seconds: body.purge_after_seconds,
    };
    let result = {
        let s = storage
            .lock()
            .map_err(|_| "storage mutex poisoned".to_string());
        match s {
            Ok(s) => s.set_retention_policy(tenant_id, &policy),
            Err(msg) => Err(axon_core::error::AxonError::Storage(msg)),
        }
    };

    match result {
        Ok(()) => (
            StatusCode::OK,
            Json(RetentionPolicyBody {
                archive_after_seconds: policy.archive_after_seconds,
                purge_after_seconds: policy.purge_after_seconds,
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", e.to_string())),
        )
            .into_response(),
    }
}

// ── Tenant database management (axon-df98e262) ───────────────────────────────

#[derive(Deserialize)]
struct CreateDatabaseBody {
    name: String,
}

/// Validate a database identifier against the D1 naming rules:
/// - 1–63 characters
/// - ASCII only: `[a-zA-Z0-9_-]`
/// - First character must not be a digit
fn is_valid_database_identifier(s: &str) -> bool {
    if s.is_empty() || s.len() > 63 {
        return false;
    }
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_digit() => return false,
        Some(c) if !matches!(c, 'a'..='z' | 'A'..='Z' | '_' | '-') => return false,
        None => return false,
        _ => {}
    }
    chars.all(|c| matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-'))
}

fn tenant_database_to_json(db: &TenantDatabase) -> serde_json::Value {
    json!({
        "tenant_id": db.tenant_id.as_str(),
        "name": db.name,
        "created_at_ms": db.created_at_ms,
    })
}

/// `GET /control/tenants/{id}/databases` — list databases registered for a tenant.
///
/// Requires tenant-admin or deployment-admin.
async fn list_tenant_databases(
    State(state): State<ControlPlaneState>,
    Extension(legacy): Extension<Identity>,
    resolved: Option<Extension<ResolvedIdentity>>,
    Path(id): Path<String>,
) -> Response {
    match &resolved {
        Some(Extension(r)) => {
            let tenant_id = TenantId::new(&id);
            let ok = control_plane_authz::require_tenant_admin(r, tenant_id, &state.user_roles)
                .is_ok()
                || control_plane_authz::require_deployment_admin(r, &state.user_roles).is_ok();
            if !ok {
                return forbidden_response("tenant admin or deployment admin required");
            }
        }
        None => {
            if let Err(e) = legacy.require_admin() {
                return auth_error_response(e);
            }
        }
    }

    let Some(storage) = &state.storage else {
        return storage_not_configured();
    };

    let tenant_id = TenantId::new(&id);
    let result = {
        let s = storage
            .lock()
            .map_err(|_| "storage mutex poisoned".to_string());
        match s {
            Ok(s) => s.list_tenant_databases(tenant_id),
            Err(msg) => Err(axon_core::error::AxonError::Storage(msg)),
        }
    };

    match result {
        Ok(dbs) => {
            let payload: Vec<serde_json::Value> = dbs.iter().map(tenant_database_to_json).collect();
            Json(json!({ "databases": payload })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", e.to_string())),
        )
            .into_response(),
    }
}

/// `POST /control/tenants/{id}/databases` — register a new database for a tenant.
///
/// Requires tenant-admin or deployment-admin. Returns 201 + the new row,
/// 409 if the name already exists, or 400 if the name is invalid.
async fn create_tenant_database(
    State(state): State<ControlPlaneState>,
    Extension(legacy): Extension<Identity>,
    resolved: Option<Extension<ResolvedIdentity>>,
    Path(id): Path<String>,
    Json(body): Json<CreateDatabaseBody>,
) -> Response {
    match &resolved {
        Some(Extension(r)) => {
            let tenant_id = TenantId::new(&id);
            let ok = control_plane_authz::require_tenant_admin(r, tenant_id, &state.user_roles)
                .is_ok()
                || control_plane_authz::require_deployment_admin(r, &state.user_roles).is_ok();
            if !ok {
                return forbidden_response("tenant admin or deployment admin required");
            }
        }
        None => {
            if let Err(e) = legacy.require_admin() {
                return auth_error_response(e);
            }
        }
    }

    if !is_valid_database_identifier(&body.name) {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                "invalid_identifier",
                format!(
                    "database name '{}' is invalid: must be 1–63 ASCII characters \
                     [a-zA-Z0-9_-] and must not start with a digit",
                    body.name
                ),
            )),
        )
            .into_response();
    }

    let Some(storage) = &state.storage else {
        return storage_not_configured();
    };

    let tenant_id = TenantId::new(&id);
    let result = {
        let s = storage
            .lock()
            .map_err(|_| "storage mutex poisoned".to_string());
        match s {
            Ok(s) => s.create_tenant_database(tenant_id, &body.name),
            Err(msg) => Err(axon_core::error::AxonError::Storage(msg)),
        }
    };

    match result {
        Ok(db) => (StatusCode::CREATED, Json(tenant_database_to_json(&db))).into_response(),
        Err(axon_core::error::AxonError::AlreadyExists(_)) => (
            StatusCode::CONFLICT,
            Json(ApiError::new(
                "already_exists",
                format!("database '{}' already exists in tenant '{}'", body.name, id),
            )),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", e.to_string())),
        )
            .into_response(),
    }
}

/// `DELETE /control/tenants/{id}/databases/{name}` — remove a database registration.
///
/// Requires tenant-admin or deployment-admin. Returns 204 on success, 404 if
/// the registration does not exist. Does NOT cascade-delete stored data.
async fn delete_tenant_database(
    State(state): State<ControlPlaneState>,
    Extension(legacy): Extension<Identity>,
    resolved: Option<Extension<ResolvedIdentity>>,
    Path((id, name)): Path<(String, String)>,
) -> Response {
    match &resolved {
        Some(Extension(r)) => {
            let tenant_id = TenantId::new(&id);
            let ok = control_plane_authz::require_tenant_admin(r, tenant_id, &state.user_roles)
                .is_ok()
                || control_plane_authz::require_deployment_admin(r, &state.user_roles).is_ok();
            if !ok {
                return forbidden_response("tenant admin or deployment admin required");
            }
        }
        None => {
            if let Err(e) = legacy.require_admin() {
                return auth_error_response(e);
            }
        }
    }

    let Some(storage) = &state.storage else {
        return storage_not_configured();
    };

    let tenant_id = TenantId::new(&id);
    let result = {
        let s = storage
            .lock()
            .map_err(|_| "storage mutex poisoned".to_string());
        match s {
            Ok(s) => s.delete_tenant_database(tenant_id, &name),
            Err(msg) => Err(axon_core::error::AxonError::Storage(msg)),
        }
    };

    match result {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ApiError::new(
                "not_found",
                format!("database '{}' not found in tenant '{}'", name, id),
            )),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", e.to_string())),
        )
            .into_response(),
    }
}

// ── Credential handlers (axon-906b527a) ─────────────────────────────────────

#[derive(serde::Deserialize)]
struct IssueCredentialBody {
    target_user: String,
    grants: Grants,
    ttl_seconds: u64,
}

#[derive(serde::Serialize)]
struct IssueCredentialResponse {
    jwt: String,
    jti: String,
    expires_at: u64,
}

/// Convert an `AuthError` into a 403 response with the canonical error code.
fn auth_error_to_response(err: AuthError) -> Response {
    (
        axum::http::StatusCode::from_u16(err.status_code()).unwrap_or(StatusCode::FORBIDDEN),
        Json(ApiError::new(err.error_code(), err.to_string())),
    )
        .into_response()
}

/// `POST /control/tenants/{id}/credentials` — issue a signed JWT credential.
///
/// Authorization:
/// - Deployment admin may issue to any user in the tenant (subject to target's role ceiling).
/// - A user may self-issue (target_user == caller) within their own role ceiling.
///
/// The signed JWT is returned once in the response body and never persisted.
async fn issue_credential(
    State(state): State<ControlPlaneState>,
    Extension(legacy): Extension<Identity>,
    resolved: Option<Extension<ResolvedIdentity>>,
    Path(id): Path<String>,
    Json(body): Json<IssueCredentialBody>,
) -> Response {
    let target_user_id = UserId::new(&body.target_user);
    let tenant_id = TenantId::new(&id);

    // Determine caller identity and check authorization.
    let caller_user_id: Option<UserId> = match &resolved {
        Some(Extension(r)) => {
            let is_deployment_admin =
                control_plane_authz::require_deployment_admin(r, &state.user_roles).is_ok();
            let is_self = r.user_id == target_user_id;
            if !is_deployment_admin && !is_self {
                return forbidden_response("deployment admin or self-issue required");
            }
            Some(r.user_id.clone())
        }
        None => {
            // Legacy auth: only deployment admins.
            if let Err(e) = legacy.require_admin() {
                return auth_error_response(e);
            }
            None
        }
    };

    let Some(storage) = &state.storage else {
        return storage_not_configured();
    };
    let Some(issuer) = &state.jwt_issuer else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiError::new("not_configured", "JWT issuer not configured")),
        )
            .into_response();
    };

    // Look up target user's membership and role.
    let target_member = {
        let s = storage
            .lock()
            .map_err(|_| "storage mutex poisoned".to_string());
        match s {
            Ok(s) => s.get_tenant_member(tenant_id.clone(), target_user_id.clone()),
            Err(msg) => Err(axon_core::error::AxonError::Storage(msg)),
        }
    };

    let target_member = match target_member {
        Ok(Some(m)) => m,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiError::new(
                    "not_a_tenant_member",
                    format!(
                        "user '{}' is not a member of tenant '{}'",
                        body.target_user, id
                    ),
                )),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new("storage_error", e.to_string())),
            )
                .into_response();
        }
    };

    // Enforce target user's role ceiling (always applied).
    if let Err(_err) = target_member.role.enforce_ceiling(&body.grants) {
        return auth_error_to_response(AuthError::GrantsExceedRole);
    }

    // For self-issue, also enforce the caller's role ceiling (same as target
    // ceiling when caller == target, but this makes the logic explicit and
    // correctly handles any future cases where the caller is not the target).
    if let Some(ref caller_id) = caller_user_id {
        if caller_id == &target_user_id {
            // Self-issue: enforce caller's own ceiling.
            let caller_member = {
                let s = storage
                    .lock()
                    .map_err(|_| "storage mutex poisoned".to_string());
                match s {
                    Ok(s) => s.get_tenant_member(tenant_id.clone(), caller_id.clone()),
                    Err(msg) => Err(axon_core::error::AxonError::Storage(msg)),
                }
            };
            match caller_member {
                Ok(Some(m)) => {
                    if m.role.enforce_ceiling(&body.grants).is_err() {
                        return auth_error_to_response(AuthError::GrantsExceedRole);
                    }
                }
                Ok(None) => {
                    return forbidden_response("caller is not a member of this tenant");
                }
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiError::new("storage_error", e.to_string())),
                    )
                        .into_response();
                }
            }
        }
    }

    // Build claims.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let jti_uuid = uuid::Uuid::now_v7();
    let expires_at = now + body.ttl_seconds;

    let claims = JwtClaims {
        iss: issuer.issuer_id.clone(),
        sub: body.target_user.clone(),
        aud: id.clone(),
        jti: jti_uuid.to_string(),
        iat: now,
        nbf: now,
        exp: expires_at,
        grants: body.grants.clone(),
    };

    // Sign the JWT.
    let jwt = match issuer.issue(&claims) {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new("signing_error", e.to_string())),
            )
                .into_response();
        }
    };

    // Persist issuance metadata.
    let grants_json = match serde_json::to_string(&body.grants) {
        Ok(j) => j,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new("serialization_error", e.to_string())),
            )
                .into_response();
        }
    };

    let track_result = {
        let s = storage
            .lock()
            .map_err(|_| "storage mutex poisoned".to_string());
        match s {
            Ok(s) => s.track_credential_issuance(
                jti_uuid,
                target_user_id,
                tenant_id,
                now as i64 * 1000,
                expires_at as i64 * 1000,
                &grants_json,
            ),
            Err(msg) => Err(axon_core::error::AxonError::Storage(msg)),
        }
    };

    if let Err(e) = track_result {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", e.to_string())),
        )
            .into_response();
    }

    (
        StatusCode::CREATED,
        Json(IssueCredentialResponse {
            jwt,
            jti: jti_uuid.to_string(),
            expires_at,
        }),
    )
        .into_response()
}

/// `GET /control/tenants/{id}/credentials` — list credential metadata for a tenant.
///
/// Deployment admin sees all credentials; authenticated users see only their own.
/// The signed JWT is never returned (it is not persisted).
async fn list_credentials_handler(
    State(state): State<ControlPlaneState>,
    Extension(legacy): Extension<Identity>,
    resolved: Option<Extension<ResolvedIdentity>>,
    Path(id): Path<String>,
) -> Response {
    let tenant_id = TenantId::new(&id);

    // Determine user_filter: admins see all, regular users see their own.
    let user_filter: Option<UserId> = match &resolved {
        Some(Extension(r)) => {
            let is_admin = control_plane_authz::require_deployment_admin(r, &state.user_roles)
                .is_ok()
                || control_plane_authz::require_tenant_admin(
                    r,
                    tenant_id.clone(),
                    &state.user_roles,
                )
                .is_ok();
            if is_admin {
                None // Admin sees all
            } else {
                Some(r.user_id.clone()) // Regular user sees own only
            }
        }
        None => {
            // Legacy auth: admin required.
            if let Err(e) = legacy.require_admin() {
                return auth_error_response(e);
            }
            None
        }
    };

    let Some(storage) = &state.storage else {
        return storage_not_configured();
    };

    let result = {
        let s = storage
            .lock()
            .map_err(|_| "storage mutex poisoned".to_string());
        match s {
            Ok(s) => s.list_credentials(tenant_id, user_filter),
            Err(msg) => Err(axon_core::error::AxonError::Storage(msg)),
        }
    };

    match result {
        Ok(creds) => {
            let payload: Vec<serde_json::Value> = creds
                .into_iter()
                .map(|m| {
                    let grants = serde_json::from_str::<serde_json::Value>(&m.grants_json)
                        .unwrap_or_else(|_| serde_json::json!({ "databases": [] }));
                    serde_json::json!({
                        "jti": m.jti,
                        "user_id": m.user_id.as_str(),
                        "tenant_id": m.tenant_id.as_str(),
                        "issued_at_ms": m.issued_at_ms,
                        "expires_at_ms": m.expires_at_ms,
                        "revoked": m.revoked,
                        "grants": grants,
                    })
                })
                .collect();
            Json(serde_json::json!({ "credentials": payload })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", e.to_string())),
        )
            .into_response(),
    }
}

/// `DELETE /control/tenants/{id}/credentials/{jti}` — revoke a credential.
///
/// Requires tenant-admin OR the credential's owner.
/// Returns 204 on success.
async fn revoke_credential_handler(
    State(state): State<ControlPlaneState>,
    Extension(legacy): Extension<Identity>,
    resolved: Option<Extension<ResolvedIdentity>>,
    Path((id, jti_str)): Path<(String, String)>,
) -> Response {
    let tenant_id = TenantId::new(&id);

    // Parse jti.
    let jti_uuid = match jti_str.parse::<uuid::Uuid>() {
        Ok(u) => u,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiError::new("invalid_jti", "jti must be a valid UUID")),
            )
                .into_response();
        }
    };

    let Some(storage) = &state.storage else {
        return storage_not_configured();
    };

    // Look up the credential to find its owner and validate it belongs to this tenant.
    let cred_opt = {
        let s = storage
            .lock()
            .map_err(|_| "storage mutex poisoned".to_string());
        match s {
            Ok(s) => s.list_credentials(tenant_id.clone(), None),
            Err(msg) => Err(axon_core::error::AxonError::Storage(msg)),
        }
    };

    let creds = match cred_opt {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new("storage_error", e.to_string())),
            )
                .into_response();
        }
    };

    let cred = creds.into_iter().find(|m| m.jti == jti_str);
    let Some(cred) = cred else {
        return (
            StatusCode::NOT_FOUND,
            Json(ApiError::new(
                "not_found",
                format!("credential '{jti_str}' not found"),
            )),
        )
            .into_response();
    };

    // Authorization check.
    let revoked_by: UserId = match &resolved {
        Some(Extension(r)) => {
            let is_admin = control_plane_authz::require_deployment_admin(r, &state.user_roles)
                .is_ok()
                || control_plane_authz::require_tenant_admin(
                    r,
                    tenant_id.clone(),
                    &state.user_roles,
                )
                .is_ok();
            let is_owner = r.user_id == cred.user_id;
            if !is_admin && !is_owner {
                return forbidden_response("tenant admin or credential owner required");
            }
            r.user_id.clone()
        }
        None => {
            if let Err(e) = legacy.require_admin() {
                return auth_error_response(e);
            }
            UserId::new("legacy-admin")
        }
    };

    // Revoke the credential.
    let result = {
        let s = storage
            .lock()
            .map_err(|_| "storage mutex poisoned".to_string());
        match s {
            Ok(s) => s.revoke_credential(jti_uuid, revoked_by),
            Err(msg) => Err(axon_core::error::AxonError::Storage(msg)),
        }
    };

    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", e.to_string())),
        )
            .into_response(),
    }
}

// ── User provisioning handlers (axon-0a6eb28a) ───────────────────────────────

#[derive(Deserialize)]
struct CreateUserBody {
    display_name: String,
    #[serde(default)]
    email: Option<String>,
}

/// `POST /control/users/provision` — create a bare user row.
///
/// Requires deployment-admin. Generates a fresh UserId, inserts a `users`
/// row, and returns 201 with the new user's fields.
async fn create_user_handler(
    State(state): State<ControlPlaneState>,
    Extension(legacy): Extension<Identity>,
    resolved: Option<Extension<ResolvedIdentity>>,
    Json(body): Json<CreateUserBody>,
) -> Response {
    match &resolved {
        Some(Extension(r)) => {
            if let Err(e) = control_plane_authz::require_deployment_admin(r, &state.user_roles) {
                return forbidden_response(&e.to_string());
            }
        }
        None => {
            if let Err(e) = legacy.require_admin() {
                return auth_error_response(e);
            }
        }
    }

    let Some(storage) = &state.storage else {
        return storage_not_configured();
    };

    let new_id = axon_core::auth::UserId::generate();
    let result = {
        let s = storage
            .lock()
            .map_err(|_| "storage mutex poisoned".to_string());
        match s {
            Ok(s) => s.create_user(&new_id, &body.display_name, body.email.as_deref()),
            Err(msg) => Err(axon_core::error::AxonError::Storage(msg)),
        }
    };

    match result {
        Ok(user) => (
            StatusCode::CREATED,
            Json(json!({
                "id": user.id.as_str(),
                "display_name": user.display_name,
                "email": user.email,
                "created_at_ms": user.created_at_ms,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", e.to_string())),
        )
            .into_response(),
    }
}

/// `GET /control/users/list` — list all provisioned users deployment-wide.
///
/// Requires deployment-admin. Returns newest-first by `created_at_ms`.
async fn list_users_handler(
    State(state): State<ControlPlaneState>,
    Extension(legacy): Extension<Identity>,
    resolved: Option<Extension<ResolvedIdentity>>,
) -> Response {
    match &resolved {
        Some(Extension(r)) => {
            if let Err(e) = control_plane_authz::require_deployment_admin(r, &state.user_roles) {
                return forbidden_response(&e.to_string());
            }
        }
        None => {
            if let Err(e) = legacy.require_admin() {
                return auth_error_response(e);
            }
        }
    }

    let Some(storage) = &state.storage else {
        return storage_not_configured();
    };

    let result = {
        let s = storage
            .lock()
            .map_err(|_| "storage mutex poisoned".to_string());
        match s {
            Ok(s) => s.list_users(),
            Err(msg) => Err(axon_core::error::AxonError::Storage(msg)),
        }
    };

    match result {
        Ok(users) => {
            let payload: Vec<serde_json::Value> = users
                .into_iter()
                .map(|u| {
                    json!({
                        "id": u.id.as_str(),
                        "display_name": u.display_name,
                        "email": u.email,
                        "created_at_ms": u.created_at_ms,
                        "suspended_at_ms": u.suspended_at_ms,
                    })
                })
                .collect();
            Json(json!({ "users": payload })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", e.to_string())),
        )
            .into_response(),
    }
}

/// `DELETE /control/users/suspend/{id}` — soft-delete a user by setting `suspended_at_ms`.
///
/// Requires deployment-admin. Idempotent: returns 200 whether or not the user
/// was found.
async fn suspend_user_handler(
    State(state): State<ControlPlaneState>,
    Extension(legacy): Extension<Identity>,
    resolved: Option<Extension<ResolvedIdentity>>,
    Path(id): Path<String>,
) -> Response {
    match &resolved {
        Some(Extension(r)) => {
            if let Err(e) = control_plane_authz::require_deployment_admin(r, &state.user_roles) {
                return forbidden_response(&e.to_string());
            }
        }
        None => {
            if let Err(e) = legacy.require_admin() {
                return auth_error_response(e);
            }
        }
    }

    let Some(storage) = &state.storage else {
        return storage_not_configured();
    };

    let user_id = UserId::new(&id);
    let result = {
        let s = storage
            .lock()
            .map_err(|_| "storage mutex poisoned".to_string());
        match s {
            Ok(s) => s.suspend_user(&user_id),
            Err(msg) => Err(axon_core::error::AxonError::Storage(msg)),
        }
    };

    match result {
        Ok(_found) => Json(json!({})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", e.to_string())),
        )
            .into_response(),
    }
}

// ── Provisioning ────────────────────────────────────────────────────────────

/// Create and initialize a new tenant SQLite database at the given path.
fn provision_tenant_database(path: &std::path::Path) -> Result<(), axon_core::error::AxonError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            axon_core::error::AxonError::Storage(format!(
                "failed to create data directory {}: {e}",
                parent.display()
            ))
        })?;
    }
    let url = format!("sqlite:{}?mode=rwc", path.display());
    let run = async {
        let pool = sqlx::SqlitePool::connect(&url).await.map_err(|e| {
            axon_core::error::AxonError::Storage(format!(
                "failed to create tenant database at {}: {e}",
                path.display()
            ))
        })?;
        sqlx::query("PRAGMA journal_mode=WAL")
            .execute(&pool)
            .await
            .map_err(|e| {
                axon_core::error::AxonError::Storage(format!(
                    "failed to initialize tenant database at {}: {e}",
                    path.display()
                ))
            })?;
        Ok(())
    };
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(run)),
        Err(_) => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| axon_core::error::AxonError::Storage(e.to_string()))?;
            rt.block_on(run)
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::auth::AuthContext;
    use axum::extract::connect_info::MockConnectInfo;
    use axum_test::TestServer;
    use serde_json::Value;
    use std::net::SocketAddr;

    fn test_control_plane_server_with_dir(tmp: &tempfile::TempDir) -> TestServer {
        let cp_db = ControlPlaneDb::open_in_memory().expect("open in-memory control-plane db");
        let state = ControlPlaneState::new(
            Arc::new(Mutex::new(cp_db)),
            tmp.path().to_path_buf(),
            UserRoleStore::default(),
            CorsStore::default(),
        );
        build_test_server(state)
    }

    fn build_test_server(state: ControlPlaneState) -> TestServer {
        let auth = AuthContext::no_auth();
        let peer: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        let app = Router::new()
            .nest("/control", control_plane_routes())
            .with_state(state)
            .layer(axum::middleware::from_fn_with_state(
                auth,
                crate::gateway::authenticate_http_request,
            ))
            .layer(MockConnectInfo(peer));
        TestServer::new(app)
    }

    /// Helper: create a tenant and return the id and db_name.
    async fn create_test_tenant(server: &TestServer, name: &str) -> (String, String) {
        let resp = server
            .post("/control/tenants")
            .json(&json!({ "name": name }))
            .await;
        resp.assert_status(StatusCode::CREATED);
        let body: Value = resp.json();
        (
            body["id"].as_str().unwrap().to_string(),
            body["db_name"].as_str().unwrap().to_string(),
        )
    }

    // -- name_to_db_slug -------------------------------------------------------

    #[test]
    fn slug_basic() {
        let slug = name_to_db_slug("Acme Corp", "01966b3c-1234-0000-0000-000000000000");
        assert!(slug.starts_with("acme-corp-"));
        assert!(slug.ends_with("01966b3c"));
    }

    #[test]
    fn slug_empty_name_uses_tenant_prefix() {
        let slug = name_to_db_slug("   ---   ", "abcdef01-0000-0000-0000-000000000000");
        assert_eq!(slug, "tenant-abcdef01");
    }

    // -- POST /control/tenants ------------------------------------------------

    #[tokio::test(flavor = "multi_thread")]
    async fn create_tenant_returns_201_with_db_name() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        let resp = server
            .post("/control/tenants")
            .json(&json!({ "name": "acme" }))
            .await;
        resp.assert_status(StatusCode::CREATED);
        let body: Value = resp.json();
        assert_eq!(body["name"], "acme");
        assert!(body["id"].as_str().is_some());
        assert!(body["db_name"].as_str().unwrap().starts_with("acme-"));
        assert!(body["created_at"].as_str().is_some());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn create_tenant_provisions_db_file() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        let resp = server
            .post("/control/tenants")
            .json(&json!({ "name": "widget-co" }))
            .await;
        resp.assert_status(StatusCode::CREATED);
        let body: Value = resp.json();
        let db_name = body["db_name"].as_str().unwrap();
        let expected_path = tmp.path().join("tenants").join(format!("{db_name}.db"));
        assert!(expected_path.exists(), "provisioned db file should exist");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn create_duplicate_tenant_returns_409() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        server
            .post("/control/tenants")
            .json(&json!({ "name": "acme" }))
            .await
            .assert_status(StatusCode::CREATED);
        let resp = server
            .post("/control/tenants")
            .json(&json!({ "name": "acme" }))
            .await;
        resp.assert_status(StatusCode::CONFLICT);
    }

    // -- GET /control/tenants -------------------------------------------------

    #[tokio::test(flavor = "multi_thread")]
    async fn list_tenants_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        let resp = server.get("/control/tenants").await;
        resp.assert_status(StatusCode::OK);
        let body: Value = resp.json();
        assert_eq!(body["tenants"].as_array().unwrap().len(), 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn list_tenants_includes_db_name() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        create_test_tenant(&server, "alpha").await;
        let resp = server.get("/control/tenants").await;
        resp.assert_status(StatusCode::OK);
        let body: Value = resp.json();
        let tenants = body["tenants"].as_array().unwrap();
        assert_eq!(tenants.len(), 1);
        assert!(tenants[0]["db_name"]
            .as_str()
            .unwrap()
            .starts_with("alpha-"));
    }

    // -- GET /control/tenants/{id} --------------------------------------------

    #[tokio::test(flavor = "multi_thread")]
    async fn get_tenant_returns_200_with_db_name() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        let (id, _) = create_test_tenant(&server, "acme").await;

        let resp = server.get(&format!("/control/tenants/{id}")).await;
        resp.assert_status(StatusCode::OK);
        let body: Value = resp.json();
        assert_eq!(body["id"], id);
        assert_eq!(body["name"], "acme");
        assert!(body["db_name"].as_str().unwrap().starts_with("acme-"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_nonexistent_tenant_returns_404() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        let resp = server
            .get("/control/tenants/00000000-0000-0000-0000-000000000000")
            .await;
        resp.assert_status(StatusCode::NOT_FOUND);
    }

    // -- DELETE /control/tenants/{id} -----------------------------------------

    #[tokio::test(flavor = "multi_thread")]
    async fn delete_tenant_returns_200() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        let (id, _) = create_test_tenant(&server, "acme").await;

        let resp = server.delete(&format!("/control/tenants/{id}")).await;
        resp.assert_status(StatusCode::OK);
        let body: Value = resp.json();
        assert_eq!(body["deleted"], true);
        assert_eq!(body["tenant_id"], id);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn delete_tenant_removes_db_file() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        let (id, db_name) = create_test_tenant(&server, "byebye").await;

        let db_path = tmp.path().join("tenants").join(format!("{db_name}.db"));
        assert!(db_path.exists(), "db file should exist before delete");

        server
            .delete(&format!("/control/tenants/{id}"))
            .await
            .assert_status(StatusCode::OK);

        assert!(!db_path.exists(), "db file should be removed after delete");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn delete_nonexistent_tenant_returns_404() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        let resp = server
            .delete("/control/tenants/00000000-0000-0000-0000-000000000000")
            .await;
        resp.assert_status(StatusCode::NOT_FOUND);
    }
}
