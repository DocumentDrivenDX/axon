//! HTTP/JSON API for the control plane.
//!
//! Mirrors the shape of [`crate::service::ControlPlaneService`] and returns
//! structured JSON errors as `{"code": "...", "detail": "..."}` — matching
//! the style used by `axon-server`'s HTTP gateway.
//!
//! This is the "north-bound" interface operators talk to. A second endpoint
//! (`POST /byoc/register`) is the one customer-hosted Axon instances call on
//! startup to announce themselves to the control plane.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::ControlPlaneError;
use crate::model::{HealthReport, TenantId, TenantSpec};
use crate::service::ControlPlaneService;

/// Structured JSON error envelope. Matches the shape used elsewhere in the
/// Axon HTTP surface so operators can share response handlers.
#[derive(Serialize)]
pub struct ApiError {
    pub code: String,
    pub detail: Value,
}

impl ApiError {
    fn new(code: &str, detail: impl Into<Value>) -> Self {
        Self {
            code: code.into(),
            detail: detail.into(),
        }
    }
}

fn error_response(err: ControlPlaneError) -> Response {
    match err {
        ControlPlaneError::TenantNotFound(id) => (
            StatusCode::NOT_FOUND,
            Json(ApiError::new("tenant_not_found", id)),
        )
            .into_response(),
        ControlPlaneError::TenantAlreadyExists(id) => (
            StatusCode::CONFLICT,
            Json(ApiError::new("tenant_already_exists", id)),
        )
            .into_response(),
        ControlPlaneError::InvalidArgument(msg) => (
            StatusCode::BAD_REQUEST,
            Json(ApiError::new("invalid_argument", msg)),
        )
            .into_response(),
        ControlPlaneError::InvalidState {
            tenant_id,
            current,
            operation,
        } => (
            StatusCode::CONFLICT,
            Json(ApiError::new(
                "invalid_state",
                json!({
                    "tenant_id": tenant_id,
                    "current": current,
                    "operation": operation,
                }),
            )),
        )
            .into_response(),
        ControlPlaneError::Store(msg) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new("store_error", msg)),
        )
            .into_response(),
    }
}

/// Request body for `POST /tenants`.
#[derive(Deserialize)]
pub struct ProvisionTenantBody {
    #[serde(flatten)]
    pub spec: TenantSpec,
}

/// Request body for `POST /tenants/{id}/activate`.
#[derive(Default, Deserialize)]
pub struct ActivateBody {
    #[serde(default)]
    pub instance_endpoint: Option<String>,
}

/// Request body for `POST /byoc/register`.
///
/// BYOC instances send their pre-issued tenant id (which an operator handed
/// the customer out-of-band) and the reachable endpoint the control plane
/// should use when presenting instance details to operators.
#[derive(Deserialize)]
pub struct RegisterByocBody {
    pub tenant_id: String,
    pub instance_endpoint: String,
}

// ── Route handlers ───────────────────────────────────────────────────────────

async fn provision_tenant(
    State(svc): State<ControlPlaneService>,
    Json(body): Json<ProvisionTenantBody>,
) -> Response {
    match svc.provision_tenant(body.spec) {
        Ok(tenant) => (StatusCode::CREATED, Json(tenant)).into_response(),
        Err(e) => error_response(e),
    }
}

async fn list_tenants(State(svc): State<ControlPlaneService>) -> Response {
    match svc.list_tenants() {
        Ok(tenants) => Json(json!({ "tenants": tenants })).into_response(),
        Err(e) => error_response(e),
    }
}

async fn get_tenant(
    State(svc): State<ControlPlaneService>,
    Path(id): Path<String>,
) -> Response {
    match svc.get_tenant(&TenantId::new(id)) {
        Ok(tenant) => Json(tenant).into_response(),
        Err(e) => error_response(e),
    }
}

async fn activate_tenant(
    State(svc): State<ControlPlaneService>,
    Path(id): Path<String>,
    body: Option<Json<ActivateBody>>,
) -> Response {
    let endpoint = body.and_then(|Json(b)| b.instance_endpoint);
    match svc.mark_active(&TenantId::new(id), endpoint) {
        Ok(tenant) => Json(tenant).into_response(),
        Err(e) => error_response(e),
    }
}

async fn suspend_tenant(
    State(svc): State<ControlPlaneService>,
    Path(id): Path<String>,
) -> Response {
    match svc.suspend(&TenantId::new(id)) {
        Ok(tenant) => Json(tenant).into_response(),
        Err(e) => error_response(e),
    }
}

async fn deprovision_tenant(
    State(svc): State<ControlPlaneService>,
    Path(id): Path<String>,
) -> Response {
    match svc.deprovision(&TenantId::new(id)) {
        Ok(tenant) => Json(tenant).into_response(),
        Err(e) => error_response(e),
    }
}

async fn terminate_tenant(
    State(svc): State<ControlPlaneService>,
    Path(id): Path<String>,
) -> Response {
    match svc.terminate(&TenantId::new(id)) {
        Ok(tenant) => Json(tenant).into_response(),
        Err(e) => error_response(e),
    }
}

async fn register_byoc(
    State(svc): State<ControlPlaneService>,
    Json(body): Json<RegisterByocBody>,
) -> Response {
    match svc.register_byoc_instance(&TenantId::new(body.tenant_id), body.instance_endpoint) {
        Ok(tenant) => Json(tenant).into_response(),
        Err(e) => error_response(e),
    }
}

async fn report_health(
    State(svc): State<ControlPlaneService>,
    Path(id): Path<String>,
    Json(report): Json<HealthReport>,
) -> Response {
    match svc.record_health(&TenantId::new(id), report) {
        Ok(tenant) => Json(tenant).into_response(),
        Err(e) => error_response(e),
    }
}

async fn dashboard(State(svc): State<ControlPlaneService>) -> Response {
    match svc.list_tenants() {
        Ok(tenants) => {
            let rows: Vec<Value> = tenants
                .iter()
                .map(|t| {
                    json!({
                        "tenant_id": t.id,
                        "name": t.spec.name,
                        "deployment_mode": t.spec.deployment_mode,
                        "status": t.status,
                        "health": t.last_health.as_ref().map(|h| h.status),
                        "instance_endpoint": t.instance_endpoint,
                        "updated_at_ms": t.updated_at_ms,
                    })
                })
                .collect();
            Json(json!({
                "total": rows.len(),
                "tenants": rows,
            }))
            .into_response()
        }
        Err(e) => error_response(e),
    }
}

/// Build the axum router for the control plane HTTP API.
pub fn build_router(service: ControlPlaneService) -> Router {
    Router::new()
        .route("/tenants", post(provision_tenant).get(list_tenants))
        .route("/tenants/{id}", get(get_tenant))
        .route("/tenants/{id}/activate", post(activate_tenant))
        .route("/tenants/{id}/suspend", post(suspend_tenant))
        .route("/tenants/{id}/deprovision", post(deprovision_tenant))
        .route("/tenants/{id}/terminate", post(terminate_tenant))
        .route("/tenants/{id}/health", post(report_health))
        .route("/byoc/register", post(register_byoc))
        .route("/dashboard", get(dashboard))
        .route(
            "/health",
            get(|| async {
                (
                    StatusCode::OK,
                    Json(json!({
                        "status": "ok",
                        "component": "axon-control-plane",
                    })),
                )
                    .into_response()
            }),
        )
        .with_state(service)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        BackingStore, DataRetentionPolicy, DeploymentMode, HealthStatus, TenantStatus,
    };
    use crate::store::{ControlPlaneStore, InMemoryControlPlaneStore};
    use axum_test::TestServer;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn test_server() -> TestServer {
        let store: Arc<dyn ControlPlaneStore> = Arc::new(InMemoryControlPlaneStore::new());
        let svc = ControlPlaneService::new(store);
        TestServer::new(build_router(svc))
    }

    fn hosted_spec_body(name: &str) -> Value {
        json!({
            "name": name,
            "deployment_mode": "hosted",
            "backing_store": { "kind": "memory" },
            "labels": {},
        })
    }

    fn byoc_spec_body(name: &str) -> Value {
        json!({
            "name": name,
            "deployment_mode": "byoc",
            "backing_store": {
                "kind": "postgres",
                "uri": "postgres://byoc@customer.example/db",
                "region": "customer-vpc"
            },
            "retention": "retain",
            "labels": {},
        })
    }

    #[tokio::test]
    async fn health_endpoint_returns_ok() {
        let server = test_server();
        let resp = server.get("/health").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["status"], "ok");
        assert_eq!(body["component"], "axon-control-plane");
    }

    #[tokio::test]
    async fn provision_and_list_tenants() {
        let server = test_server();
        let resp = server.post("/tenants").json(&hosted_spec_body("prod")).await;
        resp.assert_status(StatusCode::CREATED);
        let body: Value = resp.json();
        assert_eq!(body["status"], "provisioning");
        let id = body["id"].as_str().unwrap().to_string();

        let list = server.get("/tenants").await;
        list.assert_status_ok();
        let list_body: Value = list.json();
        assert_eq!(list_body["tenants"].as_array().unwrap().len(), 1);

        let single = server.get(&format!("/tenants/{id}")).await;
        single.assert_status_ok();
        let single_body: Value = single.json();
        assert_eq!(single_body["id"], id);
    }

    #[tokio::test]
    async fn provision_rejects_bad_backing_store() {
        let server = test_server();
        let bad = json!({
            "name": "prod",
            "deployment_mode": "hosted",
            "backing_store": { "kind": "postgres", "uri": "not-a-uri" },
            "labels": {},
        });
        let resp = server.post("/tenants").json(&bad).await;
        resp.assert_status(StatusCode::BAD_REQUEST);
        let body: Value = resp.json();
        assert_eq!(body["code"], "invalid_argument");
    }

    #[tokio::test]
    async fn activate_then_suspend_then_deprovision_flow() {
        let server = test_server();
        let resp = server.post("/tenants").json(&hosted_spec_body("p")).await;
        let id = resp.json::<Value>()["id"].as_str().unwrap().to_string();

        let activate = server
            .post(&format!("/tenants/{id}/activate"))
            .json(&json!({ "instance_endpoint": "https://prod.example" }))
            .await;
        activate.assert_status_ok();
        assert_eq!(activate.json::<Value>()["status"], "active");

        let suspend = server.post(&format!("/tenants/{id}/suspend")).await;
        suspend.assert_status_ok();
        assert_eq!(suspend.json::<Value>()["status"], "suspended");

        // Suspended -> Active (reactivate via activate route).
        let reactivate = server
            .post(&format!("/tenants/{id}/activate"))
            .json(&json!({}))
            .await;
        reactivate.assert_status_ok();
        assert_eq!(reactivate.json::<Value>()["status"], "active");

        let deprov = server.post(&format!("/tenants/{id}/deprovision")).await;
        deprov.assert_status_ok();
        assert_eq!(deprov.json::<Value>()["status"], "deprovisioning");

        // terminate -> terminated
        let term = server.post(&format!("/tenants/{id}/terminate")).await;
        term.assert_status_ok();
        assert_eq!(term.json::<Value>()["status"], "terminated");
    }

    #[tokio::test]
    async fn byoc_registration_activates_tenant() {
        let server = test_server();
        let resp = server
            .post("/tenants")
            .json(&byoc_spec_body("customer-a"))
            .await;
        let id = resp.json::<Value>()["id"].as_str().unwrap().to_string();

        let register = server
            .post("/byoc/register")
            .json(&json!({
                "tenant_id": id,
                "instance_endpoint": "https://axon.customer-a.example",
            }))
            .await;
        register.assert_status_ok();
        let body: Value = register.json();
        assert_eq!(body["status"], "active");
        assert_eq!(body["instance_endpoint"], "https://axon.customer-a.example");
    }

    #[tokio::test]
    async fn health_report_visible_in_dashboard() {
        let server = test_server();
        let resp = server.post("/tenants").json(&hosted_spec_body("p")).await;
        let id = resp.json::<Value>()["id"].as_str().unwrap().to_string();
        server
            .post(&format!("/tenants/{id}/activate"))
            .json(&json!({}))
            .await
            .assert_status_ok();

        let report = server
            .post(&format!("/tenants/{id}/health"))
            .json(&json!({
                "reported_at_ms": 1_700_000_000_000u64,
                "status": "healthy",
                "instance_version": "0.1.0",
                "storage_bytes": 8192,
                "open_connections": 3,
                "p99_latency_ms": 15,
                "error_rate": 0.002,
            }))
            .await;
        report.assert_status_ok();

        let dash = server.get("/dashboard").await;
        dash.assert_status_ok();
        let body: Value = dash.json();
        assert_eq!(body["total"], 1);
        assert_eq!(body["tenants"][0]["health"], "healthy");
        assert_eq!(body["tenants"][0]["status"], "active");
    }

    #[tokio::test]
    async fn dashboard_exposes_no_entity_data_fields() {
        // The data-sovereignty contract: the dashboard payload for a tenant
        // must only surface operational metadata, never any keys suggestive
        // of customer entity data. This is an anti-regression test — if
        // somebody adds an "entities" or "data" field to the dashboard
        // response it will blow up here and force a review.
        let server = test_server();
        server
            .post("/tenants")
            .json(&hosted_spec_body("prod"))
            .await;
        let dash = server.get("/dashboard").await;
        let body: Value = dash.json();
        let row = &body["tenants"][0];
        for forbidden in ["entities", "data", "rows", "collections", "links"] {
            assert!(
                row.get(forbidden).is_none(),
                "dashboard row unexpectedly exposed field {forbidden}",
            );
        }
    }

    #[tokio::test]
    async fn get_unknown_tenant_404() {
        let server = test_server();
        let resp = server.get("/tenants/does-not-exist").await;
        resp.assert_status(StatusCode::NOT_FOUND);
    }

    // Silence unused-import warnings on fields used only in this module.
    #[allow(dead_code)]
    fn _compile_touch() {
        let _ = (
            TenantStatus::Active,
            HealthStatus::Healthy,
            DeploymentMode::Byoc,
            BackingStore::Memory,
            DataRetentionPolicy::default(),
            BTreeMap::<String, String>::new(),
        );
    }
}
