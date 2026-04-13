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

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Extension, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Mutex;

use crate::auth::{Identity, Role};
use crate::control_plane::{ControlPlaneDb, Tenant};
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
        Self { db, data_dir, user_roles, cors_store }
    }

    /// Build the file path for a tenant database.
    ///
    /// Uses `{data_dir}/tenants/{db_name}.db` layout, matching [`crate::tenant_router::TenantRouter`].
    pub fn tenant_db_path(&self, db_name: &str) -> PathBuf {
        self.data_dir
            .join("tenants")
            .join(format!("{db_name}.db"))
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
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
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
        // User-role management
        .route("/users", get(list_users))
        .route("/users/{login}", put(set_user_role))
        .route("/users/{login}", delete(remove_user_role))
        // CORS origin management
        .route("/cors", get(list_cors_origins))
        .route("/cors", put(add_cors_origin))
        .route("/cors", delete(remove_cors_origin_handler))
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
            state.user_roles.set_cached(login.clone(), body.role.clone());
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
            (StatusCode::OK, Json(json!({ "origin": body.origin, "deleted": true })))
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

// ── Provisioning ────────────────────────────────────────────────────────────

/// Create and initialize a new tenant SQLite database at the given path.
fn provision_tenant_database(path: &std::path::Path) -> Result<(), axon_core::error::AxonError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| axon_core::error::AxonError::Storage(format!(
                "failed to create data directory {}: {e}",
                parent.display()
            )))?;
    }
    let conn = rusqlite::Connection::open(path)
        .map_err(|e| axon_core::error::AxonError::Storage(format!(
            "failed to create tenant database at {}: {e}",
            path.display()
        )))?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")
        .map_err(|e| axon_core::error::AxonError::Storage(format!(
            "failed to initialize tenant database at {}: {e}",
            path.display()
        )))?;
    Ok(())
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

    #[tokio::test]
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

    #[tokio::test]
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

    #[tokio::test]
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

    #[tokio::test]
    async fn list_tenants_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        let resp = server.get("/control/tenants").await;
        resp.assert_status(StatusCode::OK);
        let body: Value = resp.json();
        assert_eq!(body["tenants"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn list_tenants_includes_db_name() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        create_test_tenant(&server, "alpha").await;
        let resp = server.get("/control/tenants").await;
        resp.assert_status(StatusCode::OK);
        let body: Value = resp.json();
        let tenants = body["tenants"].as_array().unwrap();
        assert_eq!(tenants.len(), 1);
        assert!(tenants[0]["db_name"].as_str().unwrap().starts_with("alpha-"));
    }

    // -- GET /control/tenants/{id} --------------------------------------------

    #[tokio::test]
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

    #[tokio::test]
    async fn get_nonexistent_tenant_returns_404() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        let resp = server
            .get("/control/tenants/00000000-0000-0000-0000-000000000000")
            .await;
        resp.assert_status(StatusCode::NOT_FOUND);
    }

    // -- DELETE /control/tenants/{id} -----------------------------------------

    #[tokio::test]
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

    #[tokio::test]
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

    #[tokio::test]
    async fn delete_nonexistent_tenant_returns_404() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        let resp = server
            .delete("/control/tenants/00000000-0000-0000-0000-000000000000")
            .await;
        resp.assert_status(StatusCode::NOT_FOUND);
    }
}
