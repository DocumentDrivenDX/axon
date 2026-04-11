//! HTTP routes for control-plane tenant lifecycle management.
//!
//! All endpoints live under `/control` and require the `Admin` role.
//! The control-plane database is separate from the per-tenant data stores.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Extension, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Mutex;

use crate::auth::Identity;
use crate::control_plane::{ControlPlaneDb, Tenant, TenantDatabase};
use crate::gateway::{auth_error_response, ApiError};

/// Shared state for control-plane routes, holding the DB and a data directory
/// where tenant SQLite databases are provisioned.
#[derive(Clone)]
pub struct ControlPlaneState {
    pub db: Arc<Mutex<ControlPlaneDb>>,
    /// Directory where tenant database files are created.
    pub data_dir: PathBuf,
}

/// Shared handle to the control-plane SQLite database (legacy alias).
pub type SharedControlPlane = Arc<Mutex<ControlPlaneDb>>;

impl ControlPlaneState {
    /// Create a new control-plane state.
    pub fn new(db: Arc<Mutex<ControlPlaneDb>>, data_dir: PathBuf) -> Self {
        Self { db, data_dir }
    }

    /// Build the file path for a tenant database.
    pub fn tenant_db_path(&self, tenant_id: &str, db_name: &str) -> PathBuf {
        self.data_dir
            .join(format!("{tenant_id}_{db_name}.db"))
    }
}

// ── Request bodies ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateTenantBody {
    pub name: String,
}

#[derive(Deserialize)]
pub struct AssignDatabaseBody {
    pub db_name: String,
}

// ── Response types ───────────────────────────────────────────────────────────

#[derive(Serialize)]
struct TenantResponse {
    id: String,
    name: String,
    created_at: String,
}

impl From<Tenant> for TenantResponse {
    fn from(t: Tenant) -> Self {
        Self {
            id: t.id,
            name: t.name,
            created_at: t.created_at,
        }
    }
}

#[derive(Serialize)]
struct TenantDatabaseResponse {
    tenant_id: String,
    db_name: String,
    node_id: Option<String>,
    created_at: String,
}

impl From<TenantDatabase> for TenantDatabaseResponse {
    fn from(td: TenantDatabase) -> Self {
        Self {
            tenant_id: td.tenant_id,
            db_name: td.db_name,
            node_id: td.node_id,
            created_at: td.created_at,
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Generate an ISO-8601 UTC timestamp string using the standard library.
fn now_iso8601() -> String {
    // Use humantime for simple UTC formatting (already a dependency).
    humantime::format_rfc3339_seconds(std::time::SystemTime::now()).to_string()
}

/// Detect SQLite UNIQUE constraint violations in error messages.
fn is_unique_violation(msg: &str) -> bool {
    msg.contains("UNIQUE constraint failed")
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
        .route("/tenants/{id}/databases", post(assign_database))
        .route("/tenants/{id}/databases", get(list_tenant_databases))
        .route(
            "/tenants/{tenant_id}/databases/{db_name}",
            delete(remove_database),
        )
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// `POST /control/tenants` — create a new tenant.
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

    let db = state.db.lock().await;
    match db.create_tenant(&id, &body.name, &created_at) {
        Ok(()) => (
            StatusCode::CREATED,
            Json(json!({
                "id": id,
                "name": body.name,
                "created_at": created_at,
            })),
        )
            .into_response(),
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

/// `POST /control/tenants/{id}/databases` — assign a database to a tenant.
///
/// Records the assignment in the control-plane database and provisions the
/// actual SQLite file in the configured data directory.
async fn assign_database(
    State(state): State<ControlPlaneState>,
    Extension(identity): Extension<Identity>,
    Path(tenant_id): Path<String>,
    Json(body): Json<AssignDatabaseBody>,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }

    let created_at = now_iso8601();

    let db = state.db.lock().await;

    // Verify the tenant exists first, so we can distinguish 404 from other errors.
    if let Err(axon_core::error::AxonError::NotFound(_)) = db.get_tenant(&tenant_id) {
        return (
            StatusCode::NOT_FOUND,
            Json(ApiError::new("not_found", format!("tenant {tenant_id}"))),
        )
            .into_response();
    }

    match db.assign_database(&tenant_id, &body.db_name, None, &created_at) {
        Ok(()) => {
            // Provision the SQLite database file.
            let db_path = state.tenant_db_path(&tenant_id, &body.db_name);
            if let Err(e) = provision_tenant_database(&db_path) {
                // Best-effort rollback of the assignment record.
                let _ = db.remove_database(&tenant_id, &body.db_name);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiError::new("provisioning_error", e.to_string())),
                )
                    .into_response();
            }

            (
                StatusCode::CREATED,
                Json(json!({
                    "tenant_id": tenant_id,
                    "db_name": body.db_name,
                    "db_path": db_path.display().to_string(),
                    "created_at": created_at,
                })),
            )
                .into_response()
        }
        Err(e) => {
            let msg = e.to_string();
            if is_unique_violation(&msg) || msg.contains("PRIMARY KEY constraint failed") {
                (
                    StatusCode::CONFLICT,
                    Json(ApiError::new(
                        "already_exists",
                        format!(
                            "database '{}' already assigned to tenant '{}'",
                            body.db_name, tenant_id,
                        ),
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

/// `GET /control/tenants/{id}/databases` — list databases for a tenant.
async fn list_tenant_databases(
    State(state): State<ControlPlaneState>,
    Extension(identity): Extension<Identity>,
    Path(tenant_id): Path<String>,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }

    let db = state.db.lock().await;

    // Verify the tenant exists first.
    if let Err(axon_core::error::AxonError::NotFound(_)) = db.get_tenant(&tenant_id) {
        return (
            StatusCode::NOT_FOUND,
            Json(ApiError::new("not_found", format!("tenant {tenant_id}"))),
        )
            .into_response();
    }

    match db.list_databases_for_tenant(&tenant_id) {
        Ok(dbs) => {
            let payload: Vec<TenantDatabaseResponse> = dbs.into_iter().map(Into::into).collect();
            Json(json!({ "databases": payload })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", e.to_string())),
        )
            .into_response(),
    }
}

// ── New handlers ────────────────────────────────────────────────────────────

/// Query parameters for `DELETE /control/tenants/{id}`.
#[derive(Deserialize, Default)]
struct DeleteTenantQuery {
    /// When `true`, cascade-delete all database assignments and their
    /// provisioned SQLite files.
    #[serde(default)]
    force: bool,
}

/// `DELETE /control/tenants/{id}` — delete a tenant.
///
/// Returns 409 if the tenant still has databases and `?force=true` is not set.
/// With `?force=true`, all database assignments and provisioned files are removed.
async fn delete_tenant(
    State(state): State<ControlPlaneState>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<String>,
    Query(query): Query<DeleteTenantQuery>,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }

    let db = state.db.lock().await;

    match db.delete_tenant(&id, query.force) {
        Ok(removed_dbs) => {
            // Clean up provisioned database files.
            for db_name in &removed_dbs {
                let path = state.tenant_db_path(&id, db_name);
                let _ = std::fs::remove_file(&path);
            }
            (
                StatusCode::OK,
                Json(json!({
                    "deleted": true,
                    "tenant_id": id,
                    "removed_databases": removed_dbs,
                })),
            )
                .into_response()
        }
        Err(axon_core::error::AxonError::NotFound(_)) => (
            StatusCode::NOT_FOUND,
            Json(ApiError::new("not_found", format!("tenant {id}"))),
        )
            .into_response(),
        Err(axon_core::error::AxonError::InvalidOperation(msg)) => (
            StatusCode::CONFLICT,
            Json(ApiError::new("has_databases", msg)),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", e.to_string())),
        )
            .into_response(),
    }
}

/// `DELETE /control/tenants/{tenant_id}/databases/{db_name}` — remove a
/// database assignment and delete the provisioned SQLite file.
async fn remove_database(
    State(state): State<ControlPlaneState>,
    Extension(identity): Extension<Identity>,
    Path((tenant_id, db_name)): Path<(String, String)>,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }

    let db = state.db.lock().await;

    // Verify the tenant exists first.
    if let Err(axon_core::error::AxonError::NotFound(_)) = db.get_tenant(&tenant_id) {
        return (
            StatusCode::NOT_FOUND,
            Json(ApiError::new("not_found", format!("tenant {tenant_id}"))),
        )
            .into_response();
    }

    match db.remove_database(&tenant_id, &db_name) {
        Ok(true) => {
            // Remove the provisioned SQLite file (best-effort).
            let path = state.tenant_db_path(&tenant_id, &db_name);
            let _ = std::fs::remove_file(&path);
            (
                StatusCode::OK,
                Json(json!({
                    "deleted": true,
                    "tenant_id": tenant_id,
                    "db_name": db_name,
                })),
            )
                .into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ApiError::new(
                "not_found",
                format!("database '{db_name}' not assigned to tenant '{tenant_id}'"),
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
///
/// Uses `rusqlite::Connection::open` which creates the file if it doesn't
/// exist.  We also ensure the parent directory exists.
fn provision_tenant_database(path: &std::path::Path) -> Result<(), axon_core::error::AxonError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| axon_core::error::AxonError::Storage(format!(
                "failed to create data directory {}: {e}",
                parent.display()
            )))?;
    }
    // Open (creating) the database and run a minimal pragma to verify it works.
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

    /// Create a test server backed by a temp directory (for database provisioning).
    fn test_control_plane_server_with_dir(
        tmp: &tempfile::TempDir,
    ) -> TestServer {
        let cp_db = ControlPlaneDb::open_in_memory().expect("open in-memory control-plane db");
        let state = ControlPlaneState::new(
            Arc::new(Mutex::new(cp_db)),
            tmp.path().to_path_buf(),
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

    /// Helper: create tenant and return the ID.
    async fn create_test_tenant(server: &TestServer, name: &str) -> String {
        let resp = server
            .post("/control/tenants")
            .json(&json!({ "name": name }))
            .await;
        resp.assert_status(StatusCode::CREATED);
        resp.json::<Value>()["id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    // -- POST /control/tenants ------------------------------------------------

    #[tokio::test]
    async fn create_tenant_returns_201() {
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
        assert!(body["created_at"].as_str().is_some());
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
    async fn list_tenants_returns_created_tenants() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        create_test_tenant(&server, "alpha").await;
        create_test_tenant(&server, "beta").await;
        let resp = server.get("/control/tenants").await;
        resp.assert_status(StatusCode::OK);
        let body: Value = resp.json();
        assert_eq!(body["tenants"].as_array().unwrap().len(), 2);
    }

    // -- GET /control/tenants/{id} --------------------------------------------

    #[tokio::test]
    async fn get_tenant_returns_200() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        let id = create_test_tenant(&server, "acme").await;

        let resp = server.get(&format!("/control/tenants/{id}")).await;
        resp.assert_status(StatusCode::OK);
        let body: Value = resp.json();
        assert_eq!(body["id"], id);
        assert_eq!(body["name"], "acme");
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

    // -- POST /control/tenants/{id}/databases ---------------------------------

    #[tokio::test]
    async fn assign_database_returns_201_and_provisions_file() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        let id = create_test_tenant(&server, "acme").await;

        let resp = server
            .post(&format!("/control/tenants/{id}/databases"))
            .json(&json!({ "db_name": "prod" }))
            .await;
        resp.assert_status(StatusCode::CREATED);
        let body: Value = resp.json();
        assert_eq!(body["tenant_id"], id);
        assert_eq!(body["db_name"], "prod");
        assert!(body["db_path"].as_str().is_some());

        // Verify the SQLite file was actually created.
        let db_path = tmp.path().join(format!("{id}_prod.db"));
        assert!(db_path.exists(), "provisioned database file should exist");
    }

    #[tokio::test]
    async fn assign_database_to_nonexistent_tenant_returns_404() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        let resp = server
            .post("/control/tenants/00000000-0000-0000-0000-000000000000/databases")
            .json(&json!({ "db_name": "prod" }))
            .await;
        resp.assert_status(StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn assign_duplicate_database_returns_409() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        let id = create_test_tenant(&server, "acme").await;

        server
            .post(&format!("/control/tenants/{id}/databases"))
            .json(&json!({ "db_name": "prod" }))
            .await
            .assert_status(StatusCode::CREATED);

        let resp = server
            .post(&format!("/control/tenants/{id}/databases"))
            .json(&json!({ "db_name": "prod" }))
            .await;
        resp.assert_status(StatusCode::CONFLICT);
    }

    // -- GET /control/tenants/{id}/databases ----------------------------------

    #[tokio::test]
    async fn list_tenant_databases_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        let id = create_test_tenant(&server, "acme").await;

        let resp = server
            .get(&format!("/control/tenants/{id}/databases"))
            .await;
        resp.assert_status(StatusCode::OK);
        let body: Value = resp.json();
        assert_eq!(body["databases"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn list_tenant_databases_returns_assigned() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        let id = create_test_tenant(&server, "acme").await;

        server
            .post(&format!("/control/tenants/{id}/databases"))
            .json(&json!({ "db_name": "prod" }))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post(&format!("/control/tenants/{id}/databases"))
            .json(&json!({ "db_name": "staging" }))
            .await
            .assert_status(StatusCode::CREATED);

        let resp = server
            .get(&format!("/control/tenants/{id}/databases"))
            .await;
        resp.assert_status(StatusCode::OK);
        let body: Value = resp.json();
        assert_eq!(body["databases"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn list_databases_for_nonexistent_tenant_returns_404() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        let resp = server
            .get("/control/tenants/00000000-0000-0000-0000-000000000000/databases")
            .await;
        resp.assert_status(StatusCode::NOT_FOUND);
    }

    // -- DELETE /control/tenants/{id}/databases/{db_name} ---------------------

    #[tokio::test]
    async fn remove_database_returns_200_and_deletes_file() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        let id = create_test_tenant(&server, "acme").await;

        server
            .post(&format!("/control/tenants/{id}/databases"))
            .json(&json!({ "db_name": "prod" }))
            .await
            .assert_status(StatusCode::CREATED);

        let db_path = tmp.path().join(format!("{id}_prod.db"));
        assert!(db_path.exists(), "file should exist after assign");

        let resp = server
            .delete(&format!("/control/tenants/{id}/databases/prod"))
            .await;
        resp.assert_status(StatusCode::OK);
        let body: Value = resp.json();
        assert_eq!(body["deleted"], true);
        assert_eq!(body["db_name"], "prod");

        // Verify the file was removed.
        assert!(!db_path.exists(), "file should be deleted after remove");

        // Verify the assignment is gone.
        let list_resp = server
            .get(&format!("/control/tenants/{id}/databases"))
            .await;
        let list_body: Value = list_resp.json();
        assert_eq!(list_body["databases"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn remove_nonexistent_database_returns_404() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        let id = create_test_tenant(&server, "acme").await;

        let resp = server
            .delete(&format!("/control/tenants/{id}/databases/nope"))
            .await;
        resp.assert_status(StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn remove_database_for_nonexistent_tenant_returns_404() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        let resp = server
            .delete("/control/tenants/00000000-0000-0000-0000-000000000000/databases/foo")
            .await;
        resp.assert_status(StatusCode::NOT_FOUND);
    }

    // -- DELETE /control/tenants/{id} -----------------------------------------

    #[tokio::test]
    async fn delete_tenant_without_databases_returns_200() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        let id = create_test_tenant(&server, "acme").await;

        let resp = server
            .delete(&format!("/control/tenants/{id}"))
            .await;
        resp.assert_status(StatusCode::OK);
        let body: Value = resp.json();
        assert_eq!(body["deleted"], true);
        assert_eq!(body["removed_databases"].as_array().unwrap().len(), 0);

        // Verify the tenant is gone.
        server
            .get(&format!("/control/tenants/{id}"))
            .await
            .assert_status(StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn delete_tenant_with_databases_without_force_returns_409() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        let id = create_test_tenant(&server, "acme").await;

        server
            .post(&format!("/control/tenants/{id}/databases"))
            .json(&json!({ "db_name": "prod" }))
            .await
            .assert_status(StatusCode::CREATED);

        let resp = server
            .delete(&format!("/control/tenants/{id}"))
            .await;
        resp.assert_status(StatusCode::CONFLICT);
        let body: Value = resp.json();
        assert_eq!(body["code"], "has_databases");

        // Tenant should still exist.
        server
            .get(&format!("/control/tenants/{id}"))
            .await
            .assert_status(StatusCode::OK);
    }

    #[tokio::test]
    async fn delete_tenant_with_databases_force_cascades() {
        let tmp = tempfile::tempdir().unwrap();
        let server = test_control_plane_server_with_dir(&tmp);
        let id = create_test_tenant(&server, "acme").await;

        server
            .post(&format!("/control/tenants/{id}/databases"))
            .json(&json!({ "db_name": "prod" }))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post(&format!("/control/tenants/{id}/databases"))
            .json(&json!({ "db_name": "staging" }))
            .await
            .assert_status(StatusCode::CREATED);

        let prod_path = tmp.path().join(format!("{id}_prod.db"));
        let staging_path = tmp.path().join(format!("{id}_staging.db"));
        assert!(prod_path.exists());
        assert!(staging_path.exists());

        let resp = server
            .delete(&format!("/control/tenants/{id}?force=true"))
            .await;
        resp.assert_status(StatusCode::OK);
        let body: Value = resp.json();
        assert_eq!(body["deleted"], true);
        assert_eq!(body["removed_databases"].as_array().unwrap().len(), 2);

        // Verify files are cleaned up.
        assert!(!prod_path.exists(), "prod db file should be removed");
        assert!(!staging_path.exists(), "staging db file should be removed");

        // Verify the tenant is gone.
        server
            .get(&format!("/control/tenants/{id}"))
            .await
            .assert_status(StatusCode::NOT_FOUND);
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
