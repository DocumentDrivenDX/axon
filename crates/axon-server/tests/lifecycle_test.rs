//! L6 HTTP contract tests for the lifecycle transition endpoint (FEAT-015).
//!
//! Covers the four documented HTTP responses of `POST /lifecycle/{collection}
//! /{id}/transition`:
//!
//! 1. `200 OK` with the updated entity on a valid transition.
//! 2. `422 Unprocessable Entity` with the `valid_transitions` list when the
//!    target state is not reachable from the current state.
//! 3. `404 Not Found` when the named lifecycle does not exist on the
//!    collection schema.
//! 4. `409 Conflict` when `expected_version` does not match the stored
//!    entity version (OCC guard).
//!
//! These tests drive the HTTP gateway end-to-end via `axum_test::TestServer`.
//! Since the HTTP create-collection route does not yet expose `lifecycles`,
//! the collection schema is populated directly against the shared handler
//! before the test server is constructed, then the same handler is wrapped
//! in a [`TenantRouter::single`] so HTTP requests reach the same state.

#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::sync::Arc;

use axon_api::handler::AxonHandler;
use axon_api::request::CreateCollectionRequest;
use axon_core::id::CollectionId;
use axon_schema::schema::{CollectionSchema, LifecycleDef};
use axon_server::gateway::build_router;
use axon_server::tenant_router::TenantRouter;
use axon_storage::adapter::StorageAdapter;
use axon_storage::SqliteStorageAdapter;
use serde_json::{json, Value};
use tokio::sync::Mutex;

/// Build an HTTP test server whose shared handler has a `tasks` collection
/// with a `status` lifecycle: `draft -> submitted -> approved`.
///
/// The collection (and its lifecycle definition) is installed directly on
/// the handler before the test server is constructed, because the HTTP
/// create-collection route does not yet accept a `lifecycles` schema field.
async fn make_server_with_lifecycle() -> axum_test::TestServer {
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let handler = Arc::new(Mutex::new(AxonHandler::new(storage)));

    let mut transitions = HashMap::new();
    transitions.insert("draft".to_string(), vec!["submitted".to_string()]);
    transitions.insert("submitted".to_string(), vec!["approved".to_string()]);
    transitions.insert("approved".to_string(), vec![]);

    let lifecycle = LifecycleDef {
        field: "status".to_string(),
        initial: "draft".to_string(),
        transitions,
    };

    let mut lifecycles = HashMap::new();
    lifecycles.insert("status".to_string(), lifecycle);

    let mut schema = CollectionSchema::new(CollectionId::new("tasks"));
    schema.lifecycles = lifecycles;

    handler
        .lock()
        .await
        .create_collection(CreateCollectionRequest {
            name: CollectionId::new("tasks"),
            schema,
            actor: Some("test-setup".into()),
        })
        .expect("create_collection should succeed");

    let tenant_router = Arc::new(TenantRouter::single(handler));
    let app = build_router(tenant_router, "memory", None);
    axum_test::TestServer::new(app)
}

/// Create the seed entity `tasks/t-001` in `status: "draft"` so that each
/// test starts from a known pre-condition.
async fn seed_draft_entity(http: &axum_test::TestServer) {
    http.post("/entities/tasks/t-001")
        .json(&json!({
            "data": {
                "status": "draft",
                "title": "design the thing"
            }
        }))
        .await
        .assert_status(axum::http::StatusCode::CREATED);
}

/// Acceptance test 1 (happy path): `draft -> submitted` on a `status`
/// lifecycle returns `200 OK` and the entity at version 2 with
/// `data.status == "submitted"`.
#[tokio::test]
async fn http_transition_lifecycle_happy_path() {
    let http = make_server_with_lifecycle().await;
    seed_draft_entity(&http).await;

    let resp = http
        .post("/lifecycle/tasks/t-001/transition")
        .json(&json!({
            "lifecycle_name": "status",
            "target_state": "submitted",
            "expected_version": 1
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["entity"]["collection"], "tasks");
    assert_eq!(body["entity"]["id"], "t-001");
    assert_eq!(body["entity"]["version"], 2);
    assert_eq!(body["entity"]["data"]["status"], "submitted");
    // Non-lifecycle fields must be preserved.
    assert_eq!(body["entity"]["data"]["title"], "design the thing");
}

/// Acceptance test 2: `draft -> approved` is not allowed (only
/// `draft -> submitted` is in the transition table). The response must be
/// `422 Unprocessable Entity` with `code: "invalid_transition"` and must
/// include the `current_state` and the list of `valid_transitions`.
#[tokio::test]
async fn http_transition_lifecycle_invalid_transition() {
    let http = make_server_with_lifecycle().await;
    seed_draft_entity(&http).await;

    let resp = http
        .post("/lifecycle/tasks/t-001/transition")
        .json(&json!({
            "lifecycle_name": "status",
            "target_state": "approved",
            "expected_version": 1
        }))
        .await;

    resp.assert_status(axum::http::StatusCode::UNPROCESSABLE_ENTITY);
    let body: Value = resp.json();
    assert_eq!(body["code"], "invalid_transition");
    let detail = &body["detail"];
    assert_eq!(detail["lifecycle_name"], "status");
    assert_eq!(detail["current_state"], "draft");
    assert_eq!(detail["target_state"], "approved");
    let valid: Vec<String> = detail["valid_transitions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(valid, vec!["submitted".to_string()]);
}

/// Acceptance test 3: unknown lifecycle name returns `404` with
/// `code: "lifecycle_not_found"` and echoes the requested lifecycle name.
#[tokio::test]
async fn http_transition_lifecycle_not_found() {
    let http = make_server_with_lifecycle().await;
    seed_draft_entity(&http).await;

    let resp = http
        .post("/lifecycle/tasks/t-001/transition")
        .json(&json!({
            "lifecycle_name": "does_not_exist",
            "target_state": "anything",
            "expected_version": 1
        }))
        .await;

    resp.assert_status(axum::http::StatusCode::NOT_FOUND);
    let body: Value = resp.json();
    assert_eq!(body["code"], "lifecycle_not_found");
    assert_eq!(body["detail"]["lifecycle_name"], "does_not_exist");
}

/// Acceptance test 4: stale `expected_version` returns `409 Conflict` with
/// `code: "version_conflict"` and echoes the expected / actual version so
/// clients can reconcile.
#[tokio::test]
async fn http_transition_lifecycle_version_conflict() {
    let http = make_server_with_lifecycle().await;
    seed_draft_entity(&http).await;

    // Entity is at version 1; passing expected_version = 99 must conflict.
    let resp = http
        .post("/lifecycle/tasks/t-001/transition")
        .json(&json!({
            "lifecycle_name": "status",
            "target_state": "submitted",
            "expected_version": 99
        }))
        .await;

    resp.assert_status(axum::http::StatusCode::CONFLICT);
    let body: Value = resp.json();
    assert_eq!(body["code"], "version_conflict");
    assert_eq!(body["detail"]["expected"], 99);
    assert_eq!(body["detail"]["actual"], 1);
}
