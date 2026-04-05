//! HTTP/JSON gateway for the Axon service.
//!
//! Provides a REST API that mirrors the gRPC service operations. All responses
//! use structured JSON. Errors are returned as `{"code": "...", "detail": "..."}`
//! JSON objects with appropriate HTTP status codes.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use axon_api::handler::AxonHandler;
use axon_api::request::{
    CreateCollectionRequest, CreateEntityRequest, CreateLinkRequest, DeleteEntityRequest,
    DropCollectionRequest, GetEntityRequest, QueryAuditRequest, RevertEntityRequest,
    TraverseRequest, UpdateEntityRequest,
};
use axon_audit::AuditLog;
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_storage::memory::MemoryStorageAdapter;

type SharedHandler = Arc<Mutex<AxonHandler<MemoryStorageAdapter>>>;

// ── Error response ────────────────────────────────────────────────────────────

/// Structured JSON error response with field-level details.
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

fn axon_error_response(err: AxonError) -> Response {
    match err {
        AxonError::NotFound(msg) => {
            (StatusCode::NOT_FOUND, Json(ApiError::new("not_found", msg))).into_response()
        }
        AxonError::ConflictingVersion { expected, actual } => (
            StatusCode::CONFLICT,
            Json(ApiError::new(
                "version_conflict",
                json!({"expected": expected, "actual": actual}),
            )),
        )
            .into_response(),
        AxonError::SchemaValidation(detail) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiError::new("schema_validation", detail)),
        )
            .into_response(),
        AxonError::AlreadyExists(msg) => (
            StatusCode::CONFLICT,
            Json(ApiError::new("already_exists", msg)),
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

#[derive(Deserialize)]
pub struct RevertEntityBody {
    pub audit_entry_id: u64,
    pub actor: Option<String>,
    #[serde(default)]
    pub force: bool,
}

#[derive(Deserialize)]
pub struct CollectionActorBody {
    pub actor: Option<String>,
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

// ── Route handlers ────────────────────────────────────────────────────────────

async fn create_entity(
    State(handler): State<SharedHandler>,
    Path((collection, id)): Path<(String, String)>,
    Json(body): Json<CreateEntityBody>,
) -> Response {
    match handler.lock().await.create_entity(CreateEntityRequest {
        collection: CollectionId::new(&collection),
        id: EntityId::new(&id),
        data: body.data,
        actor: body.actor,
    }) {
        Ok(resp) => (
            StatusCode::CREATED,
            Json(json!({
                "entity": {
                    "collection": resp.entity.collection.to_string(),
                    "id": resp.entity.id.to_string(),
                    "version": resp.entity.version,
                    "data": resp.entity.data,
                }
            })),
        )
            .into_response(),
        Err(e) => axon_error_response(e),
    }
}

async fn get_entity(
    State(handler): State<SharedHandler>,
    Path((collection, id)): Path<(String, String)>,
) -> Response {
    match handler.lock().await.get_entity(GetEntityRequest {
        collection: CollectionId::new(&collection),
        id: EntityId::new(&id),
    }) {
        Ok(resp) => Json(json!({
            "entity": {
                "collection": resp.entity.collection.to_string(),
                "id": resp.entity.id.to_string(),
                "version": resp.entity.version,
                "data": resp.entity.data,
            }
        }))
        .into_response(),
        Err(e) => axon_error_response(e),
    }
}

async fn update_entity(
    State(handler): State<SharedHandler>,
    Path((collection, id)): Path<(String, String)>,
    Json(body): Json<UpdateEntityBody>,
) -> Response {
    match handler.lock().await.update_entity(UpdateEntityRequest {
        collection: CollectionId::new(&collection),
        id: EntityId::new(&id),
        data: body.data,
        expected_version: body.expected_version,
        actor: body.actor,
    }) {
        Ok(resp) => Json(json!({
            "entity": {
                "collection": resp.entity.collection.to_string(),
                "id": resp.entity.id.to_string(),
                "version": resp.entity.version,
                "data": resp.entity.data,
            }
        }))
        .into_response(),
        Err(e) => axon_error_response(e),
    }
}

async fn delete_entity(
    State(handler): State<SharedHandler>,
    Path((collection, id)): Path<(String, String)>,
    body: Option<Json<DeleteEntityBody>>,
) -> Response {
    let actor = body.and_then(|b| b.0.actor);
    match handler.lock().await.delete_entity(DeleteEntityRequest {
        collection: CollectionId::new(&collection),
        id: EntityId::new(&id),
        actor,
    }) {
        Ok(resp) => Json(json!({"collection": resp.collection, "id": resp.id})).into_response(),
        Err(e) => axon_error_response(e),
    }
}

async fn create_link(
    State(handler): State<SharedHandler>,
    Json(body): Json<CreateLinkBody>,
) -> Response {
    match handler.lock().await.create_link(CreateLinkRequest {
        source_collection: CollectionId::new(&body.source_collection),
        source_id: EntityId::new(&body.source_id),
        target_collection: CollectionId::new(&body.target_collection),
        target_id: EntityId::new(&body.target_id),
        link_type: body.link_type,
        metadata: body.metadata,
        actor: body.actor,
    }) {
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

async fn traverse(
    State(handler): State<SharedHandler>,
    Path((collection, id)): Path<(String, String)>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let link_type = params.get("link_type").cloned();
    let max_depth = params.get("max_depth").and_then(|s| s.parse().ok());

    match handler.lock().await.traverse(TraverseRequest {
        collection: CollectionId::new(&collection),
        id: EntityId::new(&id),
        link_type,
        max_depth,
    }) {
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
            Json(json!({ "entities": entities })).into_response()
        }
        Err(e) => axon_error_response(e),
    }
}

async fn query_audit_by_entity(
    State(handler): State<SharedHandler>,
    Path((collection, entity_id)): Path<(String, String)>,
) -> Response {
    let handler = handler.lock().await;
    match handler
        .audit_log()
        .query_by_entity(&CollectionId::new(&collection), &EntityId::new(&entity_id))
    {
        Ok(entries) => {
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
                        "data_before": e.data_before,
                        "data_after": e.data_after,
                        "actor": e.actor,
                        "transaction_id": e.transaction_id,
                    })
                })
                .collect();
            Json(json!({ "entries": proto })).into_response()
        }
        Err(e) => axon_error_response(e),
    }
}

async fn query_audit(
    State(handler): State<SharedHandler>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let req = QueryAuditRequest {
        collection: params.get("collection").map(CollectionId::new),
        entity_id: params.get("entity_id").map(EntityId::new),
        actor: params.get("actor").cloned(),
        operation: params.get("operation").cloned(),
        since_ns: params.get("since_ns").and_then(|s| s.parse().ok()),
        until_ns: params.get("until_ns").and_then(|s| s.parse().ok()),
        after_id: params.get("after_id").and_then(|s| s.parse().ok()),
        limit: params.get("limit").and_then(|s| s.parse().ok()),
    };
    match handler.lock().await.query_audit(req) {
        Ok(resp) => {
            let entries: Vec<Value> = resp
                .entries
                .iter()
                .map(|e: &axon_audit::AuditEntry| {
                    json!({
                        "id": e.id,
                        "timestamp_ns": e.timestamp_ns,
                        "collection": e.collection.to_string(),
                        "entity_id": e.entity_id.to_string(),
                        "version": e.version,
                        "mutation": e.mutation.to_string(),
                        "data_before": e.data_before,
                        "data_after": e.data_after,
                        "actor": e.actor,
                        "transaction_id": e.transaction_id,
                    })
                })
                .collect();
            Json(json!({ "entries": entries, "next_cursor": resp.next_cursor })).into_response()
        }
        Err(e) => axon_error_response(e),
    }
}

async fn revert_entity(
    State(handler): State<SharedHandler>,
    Json(body): Json<RevertEntityBody>,
) -> Response {
    match handler
        .lock()
        .await
        .revert_entity_to_audit_entry(RevertEntityRequest {
            audit_entry_id: body.audit_entry_id,
            actor: body.actor,
            force: body.force,
        }) {
        Ok(resp) => Json(json!({
            "entity": {
                "collection": resp.entity.collection.to_string(),
                "id": resp.entity.id.to_string(),
                "version": resp.entity.version,
                "data": resp.entity.data,
            },
            "audit_entry_id": resp.audit_entry.id,
        }))
        .into_response(),
        Err(e) => axon_error_response(e),
    }
}

async fn create_collection(
    State(handler): State<SharedHandler>,
    Path(name): Path<String>,
    body: Option<Json<CollectionActorBody>>,
) -> Response {
    let actor = body.and_then(|b| b.0.actor);
    match handler
        .lock()
        .await
        .create_collection(CreateCollectionRequest {
            name: CollectionId::new(&name),
            actor,
        }) {
        Ok(resp) => {
            (StatusCode::CREATED, Json(json!({ "name": resp.name }))).into_response()
        }
        Err(e) => axon_error_response(e),
    }
}

async fn drop_collection(
    State(handler): State<SharedHandler>,
    Path(name): Path<String>,
    body: Option<Json<CollectionActorBody>>,
) -> Response {
    let actor = body.and_then(|b| b.0.actor);
    match handler
        .lock()
        .await
        .drop_collection(DropCollectionRequest {
            name: CollectionId::new(&name),
            actor,
        }) {
        Ok(resp) => Json(json!({
            "name": resp.name,
            "entities_removed": resp.entities_removed,
        }))
        .into_response(),
        Err(e) => axon_error_response(e),
    }
}

// ── Router construction ───────────────────────────────────────────────────────

/// Build the axum router for the HTTP gateway.
pub fn build_router(handler: SharedHandler) -> Router {
    Router::new()
        .route("/entities/{collection}/{id}", post(create_entity))
        .route("/entities/{collection}/{id}", get(get_entity))
        .route("/entities/{collection}/{id}", put(update_entity))
        .route("/entities/{collection}/{id}", delete(delete_entity))
        .route("/links", post(create_link))
        .route("/traverse/{collection}/{id}", get(traverse))
        .route(
            "/audit/entity/{collection}/{id}",
            get(query_audit_by_entity),
        )
        .route("/audit/query", get(query_audit))
        .route("/audit/revert", post(revert_entity))
        .route("/collections/{name}", post(create_collection))
        .route("/collections/{name}", delete(drop_collection))
        .with_state(handler)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum_test::TestServer;
    use serde_json::json;

    fn test_server() -> TestServer {
        let handler = Arc::new(Mutex::new(
            AxonHandler::new(MemoryStorageAdapter::default()),
        ));
        let app = build_router(handler);
        TestServer::new(app)
    }

    #[tokio::test]
    async fn http_create_then_get_entity() {
        let server = test_server();

        // Create
        let resp = server
            .post("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "hello"}, "actor": "test"}))
            .await;
        resp.assert_status(StatusCode::CREATED);
        let body: Value = resp.json();
        assert_eq!(body["entity"]["version"], 1);

        // Get
        let resp = server.get("/entities/tasks/t-001").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["entity"]["data"]["title"], "hello");
    }

    #[tokio::test]
    async fn http_get_missing_returns_404() {
        let server = test_server();
        let resp = server.get("/entities/tasks/ghost").await;
        resp.assert_status_not_found();
        let body: Value = resp.json();
        assert_eq!(body["code"], "not_found");
    }

    #[tokio::test]
    async fn http_update_entity() {
        let server = test_server();

        server
            .post("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v1"}}))
            .await
            .assert_status(StatusCode::CREATED);

        let resp = server
            .put("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v2"}, "expected_version": 1}))
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["entity"]["version"], 2);
    }

    #[tokio::test]
    async fn http_update_version_conflict_returns_409() {
        let server = test_server();

        server
            .post("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v1"}}))
            .await
            .assert_status(StatusCode::CREATED);

        let resp = server
            .put("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v2"}, "expected_version": 99}))
            .await;
        resp.assert_status(StatusCode::CONFLICT);
        let body: Value = resp.json();
        assert_eq!(body["code"], "version_conflict");
        assert_eq!(body["detail"]["expected"], 99);
    }

    #[tokio::test]
    async fn http_delete_entity() {
        let server = test_server();

        server
            .post("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "bye"}}))
            .await
            .assert_status(StatusCode::CREATED);

        server
            .delete("/entities/tasks/t-001")
            .await
            .assert_status_ok();

        server
            .get("/entities/tasks/t-001")
            .await
            .assert_status_not_found();
    }

    #[tokio::test]
    async fn http_create_link_and_traverse() {
        let server = test_server();

        // Create two entities.
        server
            .post("/entities/users/u-001")
            .json(&json!({"data": {"name": "Alice"}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "Task 1"}}))
            .await
            .assert_status(StatusCode::CREATED);

        // Create link.
        let resp = server
            .post("/links")
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
        let resp = server.get("/traverse/users/u-001?link_type=owns").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["entities"].as_array().unwrap().len(), 1);
        assert_eq!(body["entities"][0]["id"], "t-001");
    }

    #[tokio::test]
    async fn http_query_audit_log() {
        let server = test_server();

        server
            .post("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v1"}, "actor": "agent-1"}))
            .await
            .assert_status(StatusCode::CREATED);

        let resp = server.get("/audit/entity/tasks/t-001").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        let entries = body["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["actor"], "agent-1");
    }

    #[tokio::test]
    async fn http_query_audit_filtered() {
        let server = test_server();

        server
            .post("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v1"}, "actor": "alice"}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/entities/tasks/t-002")
            .json(&json!({"data": {"title": "v2"}, "actor": "bob"}))
            .await
            .assert_status(StatusCode::CREATED);

        // Filter by actor.
        let resp = server.get("/audit/query?actor=alice").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        let entries = body["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["actor"], "alice");

        // Filter by collection.
        let resp = server.get("/audit/query?collection=tasks").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["entries"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn http_revert_entity() {
        let server = test_server();

        server
            .post("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v1"}, "actor": "alice"}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .put("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v2"}, "expected_version": 1, "actor": "alice"}))
            .await
            .assert_status_ok();

        // Get audit entries to find the entry_id for the create.
        let resp = server.get("/audit/query?entity_id=t-001&collection=tasks").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        let entries = body["entries"].as_array().unwrap();
        // First entry is the create (data_before is null, data_after has v1).
        let create_entry_id = entries[0]["id"].as_u64().unwrap();

        // Revert back to v1 state — but entry 0 is a create (no before), so use entry 1 (update).
        let update_entry_id = entries[1]["id"].as_u64().unwrap();
        let resp = server
            .post("/audit/revert")
            .json(&json!({"audit_entry_id": update_entry_id, "actor": "admin"}))
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["entity"]["data"]["title"], "v1");
        // Silence unused variable warning.
        let _ = create_entry_id;
    }

    #[tokio::test]
    async fn http_create_and_drop_collection() {
        let server = test_server();

        // Create collection.
        let resp = server
            .post("/collections/my-col")
            .json(&json!({"actor": "admin"}))
            .await;
        resp.assert_status(StatusCode::CREATED);
        let body: Value = resp.json();
        assert_eq!(body["name"], "my-col");

        // Duplicate create returns 409.
        let resp = server
            .post("/collections/my-col")
            .json(&json!({"actor": "admin"}))
            .await;
        resp.assert_status(StatusCode::CONFLICT);
        let body: Value = resp.json();
        assert_eq!(body["code"], "already_exists");

        // Drop collection.
        let resp = server.delete("/collections/my-col").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["name"], "my-col");
    }

    #[tokio::test]
    async fn http_errors_are_structured_with_field_level_details() {
        let server = test_server();

        // Version conflict includes expected/actual fields.
        server
            .post("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v1"}}))
            .await
            .assert_status(StatusCode::CREATED);

        let resp = server
            .put("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v2"}, "expected_version": 5}))
            .await;
        let body: Value = resp.json();
        assert_eq!(body["code"], "version_conflict");
        // Field-level details: expected and actual versions.
        assert!(body["detail"]["expected"].is_number());
        assert!(body["detail"]["actual"].is_number());
    }
}
