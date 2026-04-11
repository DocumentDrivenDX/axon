//! HTTP routes for control-plane tenant lifecycle management.
//!
//! All endpoints live under `/control` and require the `Admin` role.
//! The control-plane database is separate from the per-tenant data stores.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Mutex;

use crate::auth::Identity;
use crate::control_plane::{ControlPlaneDb, Tenant, TenantDatabase};
use crate::gateway::{auth_error_response, ApiError};

/// Shared handle to the control-plane SQLite database.
pub type SharedControlPlane = Arc<Mutex<ControlPlaneDb>>;

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
/// under the `/control` prefix and ensuring the `SharedControlPlane` extension
/// is available.
pub fn control_plane_routes() -> Router<SharedControlPlane> {
    Router::new()
        .route("/tenants", post(create_tenant))
        .route("/tenants", get(list_tenants))
        .route("/tenants/{id}", get(get_tenant))
        .route("/tenants/{id}/databases", post(assign_database))
        .route("/tenants/{id}/databases", get(list_tenant_databases))
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// `POST /control/tenants` — create a new tenant.
async fn create_tenant(
    State(cp): State<SharedControlPlane>,
    Extension(identity): Extension<Identity>,
    Json(body): Json<CreateTenantBody>,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }

    let id = uuid::Uuid::now_v7().to_string();
    let created_at = now_iso8601();

    let db = cp.lock().await;
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
    State(cp): State<SharedControlPlane>,
    Extension(identity): Extension<Identity>,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }

    let db = cp.lock().await;
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
    State(cp): State<SharedControlPlane>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<String>,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }

    let db = cp.lock().await;
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
async fn assign_database(
    State(cp): State<SharedControlPlane>,
    Extension(identity): Extension<Identity>,
    Path(tenant_id): Path<String>,
    Json(body): Json<AssignDatabaseBody>,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }

    let created_at = now_iso8601();

    let db = cp.lock().await;

    // Verify the tenant exists first, so we can distinguish 404 from other errors.
    if let Err(axon_core::error::AxonError::NotFound(_)) = db.get_tenant(&tenant_id) {
        return (
            StatusCode::NOT_FOUND,
            Json(ApiError::new("not_found", format!("tenant {tenant_id}"))),
        )
            .into_response();
    }

    match db.assign_database(&tenant_id, &body.db_name, None, &created_at) {
        Ok(()) => (
            StatusCode::CREATED,
            Json(json!({
                "tenant_id": tenant_id,
                "db_name": body.db_name,
                "created_at": created_at,
            })),
        )
            .into_response(),
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
    State(cp): State<SharedControlPlane>,
    Extension(identity): Extension<Identity>,
    Path(tenant_id): Path<String>,
) -> Response {
    if let Err(e) = identity.require_admin() {
        return auth_error_response(e);
    }

    let db = cp.lock().await;

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
            let payload: Vec<TenantDatabaseResponse> =
                dbs.into_iter().map(Into::into).collect();
            Json(json!({ "databases": payload })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("storage_error", e.to_string())),
        )
            .into_response(),
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

    fn test_control_plane_server() -> TestServer {
        test_control_plane_server_with_db(
            ControlPlaneDb::open_in_memory().expect("open in-memory control-plane db"),
        )
    }

    fn test_control_plane_server_with_db(cp_db: ControlPlaneDb) -> TestServer {
        let cp = Arc::new(Mutex::new(cp_db));

        // Build a minimal router with the control plane routes + no-auth
        let auth = AuthContext::no_auth();
        let peer: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        let app = Router::new()
            .nest("/control", control_plane_routes())
            .with_state(cp)
            .layer(axum::middleware::from_fn_with_state(
                auth,
                crate::gateway::authenticate_http_request,
            ))
            .layer(MockConnectInfo(peer));
        TestServer::new(app)
    }

    // -- POST /control/tenants ------------------------------------------------

    #[tokio::test]
    async fn create_tenant_returns_201() {
        let server = test_control_plane_server();
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
        let server = test_control_plane_server();
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
        let server = test_control_plane_server();
        let resp = server.get("/control/tenants").await;
        resp.assert_status(StatusCode::OK);
        let body: Value = resp.json();
        assert_eq!(body["tenants"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn list_tenants_returns_created_tenants() {
        let server = test_control_plane_server();
        server
            .post("/control/tenants")
            .json(&json!({ "name": "alpha" }))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/control/tenants")
            .json(&json!({ "name": "beta" }))
            .await
            .assert_status(StatusCode::CREATED);
        let resp = server.get("/control/tenants").await;
        resp.assert_status(StatusCode::OK);
        let body: Value = resp.json();
        assert_eq!(body["tenants"].as_array().unwrap().len(), 2);
    }

    // -- GET /control/tenants/{id} --------------------------------------------

    #[tokio::test]
    async fn get_tenant_returns_200() {
        let server = test_control_plane_server();
        let create_resp = server
            .post("/control/tenants")
            .json(&json!({ "name": "acme" }))
            .await;
        create_resp.assert_status(StatusCode::CREATED);
        let id = create_resp.json::<Value>()["id"]
            .as_str()
            .unwrap()
            .to_string();

        let resp = server.get(&format!("/control/tenants/{id}")).await;
        resp.assert_status(StatusCode::OK);
        let body: Value = resp.json();
        assert_eq!(body["id"], id);
        assert_eq!(body["name"], "acme");
    }

    #[tokio::test]
    async fn get_nonexistent_tenant_returns_404() {
        let server = test_control_plane_server();
        let resp = server
            .get("/control/tenants/00000000-0000-0000-0000-000000000000")
            .await;
        resp.assert_status(StatusCode::NOT_FOUND);
    }

    // -- POST /control/tenants/{id}/databases ---------------------------------

    #[tokio::test]
    async fn assign_database_returns_201() {
        let server = test_control_plane_server();
        let create_resp = server
            .post("/control/tenants")
            .json(&json!({ "name": "acme" }))
            .await;
        create_resp.assert_status(StatusCode::CREATED);
        let id = create_resp.json::<Value>()["id"]
            .as_str()
            .unwrap()
            .to_string();

        let resp = server
            .post(&format!("/control/tenants/{id}/databases"))
            .json(&json!({ "db_name": "prod" }))
            .await;
        resp.assert_status(StatusCode::CREATED);
        let body: Value = resp.json();
        assert_eq!(body["tenant_id"], id);
        assert_eq!(body["db_name"], "prod");
    }

    #[tokio::test]
    async fn assign_database_to_nonexistent_tenant_returns_404() {
        let server = test_control_plane_server();
        let resp = server
            .post("/control/tenants/00000000-0000-0000-0000-000000000000/databases")
            .json(&json!({ "db_name": "prod" }))
            .await;
        resp.assert_status(StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn assign_duplicate_database_returns_409() {
        let server = test_control_plane_server();
        let create_resp = server
            .post("/control/tenants")
            .json(&json!({ "name": "acme" }))
            .await;
        let id = create_resp.json::<Value>()["id"]
            .as_str()
            .unwrap()
            .to_string();

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
        let server = test_control_plane_server();
        let create_resp = server
            .post("/control/tenants")
            .json(&json!({ "name": "acme" }))
            .await;
        let id = create_resp.json::<Value>()["id"]
            .as_str()
            .unwrap()
            .to_string();

        let resp = server
            .get(&format!("/control/tenants/{id}/databases"))
            .await;
        resp.assert_status(StatusCode::OK);
        let body: Value = resp.json();
        assert_eq!(body["databases"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn list_tenant_databases_returns_assigned() {
        let server = test_control_plane_server();
        let create_resp = server
            .post("/control/tenants")
            .json(&json!({ "name": "acme" }))
            .await;
        let id = create_resp.json::<Value>()["id"]
            .as_str()
            .unwrap()
            .to_string();

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
        let server = test_control_plane_server();
        let resp = server
            .get("/control/tenants/00000000-0000-0000-0000-000000000000/databases")
            .await;
        resp.assert_status(StatusCode::NOT_FOUND);
    }
}
