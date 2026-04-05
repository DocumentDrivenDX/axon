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
    DeleteLinkRequest, DescribeCollectionRequest, DropCollectionRequest, GetEntityRequest,
    GetSchemaRequest, ListCollectionsRequest, PutSchemaRequest, QueryAuditRequest,
    QueryEntitiesRequest, RevertEntityRequest, TraverseRequest, UpdateEntityRequest,
};
use axon_audit::AuditLog;
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_schema::schema::CollectionSchema;
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
            Json(ApiError::new("schema_validation", detail)),
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
    pub actor: Option<String>,
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
    Delete {
        collection: String,
        id: String,
        expected_version: u64,
    },
}

/// Request body for `POST /transactions`.
#[derive(Deserialize)]
pub struct TransactionBody {
    pub operations: Vec<TransactionOp>,
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

async fn query_entities(
    State(handler): State<SharedHandler>,
    Path(collection): Path<String>,
    Json(body): Json<QueryEntitiesRequest>,
) -> Response {
    // Allow the caller to omit the collection field in the body; the path wins.
    let req = QueryEntitiesRequest {
        collection: axon_core::id::CollectionId::new(&collection),
        ..body
    };
    match handler.lock().await.query_entities(req) {
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
            }))
            .into_response()
        }
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

async fn delete_link(
    State(handler): State<SharedHandler>,
    Json(body): Json<DeleteLinkBody>,
) -> Response {
    match handler.lock().await.delete_link(DeleteLinkRequest {
        source_collection: CollectionId::new(&body.source_collection),
        source_id: EntityId::new(&body.source_id),
        target_collection: CollectionId::new(&body.target_collection),
        target_id: EntityId::new(&body.target_id),
        link_type: body.link_type,
        actor: body.actor,
    }) {
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
        direction: Default::default(),
        hop_filter: None,
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
    body: Option<Json<CreateCollectionBody>>,
) -> Response {
    let (actor, schema_body) = match body.and_then(|Json(b)| b.schema.map(|s| (b.actor, s))) {
        Some((actor, schema_body)) => (actor, schema_body),
        None => {
            return axon_error_response(AxonError::InvalidArgument(
                "'schema' field is required to create a collection".into(),
            ));
        }
    };
    let collection_id = CollectionId::new(&name);
    let schema = CollectionSchema {
        collection: collection_id.clone(),
        description: schema_body.description,
        version: schema_body.version,
        entity_schema: schema_body.entity_schema,
        link_types: schema_body.link_types.unwrap_or_default(),
    };
    match handler
        .lock()
        .await
        .create_collection(CreateCollectionRequest {
            name: collection_id,
            schema,
            actor,
        }) {
        Ok(resp) => (StatusCode::CREATED, Json(json!({ "name": resp.name }))).into_response(),
        Err(e) => axon_error_response(e),
    }
}

async fn drop_collection(
    State(handler): State<SharedHandler>,
    Path(name): Path<String>,
    body: Option<Json<CollectionActorBody>>,
) -> Response {
    let actor = body.and_then(|b| b.0.actor);
    match handler.lock().await.drop_collection(DropCollectionRequest {
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

async fn list_collections(State(handler): State<SharedHandler>) -> Response {
    match handler
        .lock()
        .await
        .list_collections(ListCollectionsRequest {})
    {
        Ok(resp) => Json(json!({ "collections": resp.collections })).into_response(),
        Err(e) => axon_error_response(e),
    }
}

async fn describe_collection(
    State(handler): State<SharedHandler>,
    Path(name): Path<String>,
) -> Response {
    match handler
        .lock()
        .await
        .describe_collection(DescribeCollectionRequest {
            name: CollectionId::new(&name),
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

async fn put_schema(
    State(handler): State<SharedHandler>,
    Path(collection): Path<String>,
    Json(body): Json<PutSchemaBody>,
) -> Response {
    // Populate schema from body; collection always comes from the path.
    let schema = CollectionSchema {
        collection: axon_core::id::CollectionId::new(&collection),
        description: body.description,
        version: body.version,
        entity_schema: body.entity_schema,
        link_types: body.link_types.unwrap_or_default(),
    };
    match handler.lock().await.handle_put_schema(PutSchemaRequest {
        schema,
        actor: body.actor,
    }) {
        Ok(resp) => (StatusCode::OK, Json(json!({ "schema": resp.schema }))).into_response(),
        Err(e) => axon_error_response(e),
    }
}

async fn get_schema(
    State(handler): State<SharedHandler>,
    Path(collection): Path<String>,
) -> Response {
    match handler.lock().await.handle_get_schema(GetSchemaRequest {
        collection: axon_core::id::CollectionId::new(&collection),
    }) {
        Ok(resp) => Json(json!({ "schema": resp.schema })).into_response(),
        Err(e) => axon_error_response(e),
    }
}

// ── Transaction endpoint ─────────────────────────────────────────────────────

async fn commit_transaction(
    State(handler): State<SharedHandler>,
    Json(body): Json<TransactionBody>,
) -> Response {
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
                CollectionId::new(&collection),
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
                    .get_entity(GetEntityRequest {
                        collection: CollectionId::new(&collection),
                        id: EntityId::new(&id),
                    })
                    .ok()
                    .map(|r| r.entity.data);
                drop(h);
                tx.update(
                    Entity::new(CollectionId::new(&collection), EntityId::new(&id), data),
                    expected_version,
                    data_before,
                )
            }
            TransactionOp::Delete {
                collection,
                id,
                expected_version,
            } => {
                let h = handler.lock().await;
                let data_before = h
                    .get_entity(GetEntityRequest {
                        collection: CollectionId::new(&collection),
                        id: EntityId::new(&id),
                    })
                    .ok()
                    .map(|r| r.entity.data);
                drop(h);
                tx.delete(
                    CollectionId::new(&collection),
                    EntityId::new(&id),
                    expected_version,
                    data_before,
                )
            }
        };
        if let Err(e) = result {
            return axon_error_response(e);
        }
    }

    // Commit atomically.
    let tx_id = tx.id.clone();
    let mut h = handler.lock().await;
    let (storage, audit) = h.storage_and_audit_mut();
    match tx.commit(storage, audit, body.actor) {
        Ok(written) => {
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
            (
                StatusCode::OK,
                Json(json!({
                    "transaction_id": tx_id,
                    "entities": entities,
                })),
            )
                .into_response()
        }
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
        .route("/collections/{collection}/query", post(query_entities))
        .route("/links", post(create_link))
        .route("/links", delete(delete_link))
        .route("/traverse/{collection}/{id}", get(traverse))
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
        .route("/collections/{name}/schema", put(put_schema))
        .route("/collections/{name}/schema", get(get_schema))
        .route("/transactions", post(commit_transaction))
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
    async fn http_create_then_delete_link() {
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
        server
            .post("/links")
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
        let resp = server.get("/traverse/users/u-001?link_type=owns").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["entities"].as_array().unwrap().len(), 1);

        // Delete the link.
        let resp = server
            .delete("/links")
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
        let resp = server.get("/traverse/users/u-001?link_type=owns").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["entities"].as_array().unwrap().len(), 0);
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
        let resp = server
            .get("/audit/query?entity_id=t-001&collection=tasks")
            .await;
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
            .json(&json!({"schema": {}, "actor": "admin"}))
            .await;
        resp.assert_status(StatusCode::CREATED);
        let body: Value = resp.json();
        assert_eq!(body["name"], "my-col");

        // Duplicate create returns 409.
        let resp = server
            .post("/collections/my-col")
            .json(&json!({"schema": {}, "actor": "admin"}))
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
    async fn http_query_entities_filter_and_count() {
        let server = test_server();

        // Seed three tasks.
        for (id, status) in [("t-1", "open"), ("t-2", "done"), ("t-3", "open")] {
            server
                .post(&format!("/entities/tasks/{id}"))
                .json(&json!({"data": {"status": status}}))
                .await
                .assert_status(StatusCode::CREATED);
        }

        // Filter: status = "open"
        let resp = server
            .post("/collections/tasks/query")
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
            .post("/collections/tasks/query")
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

    #[tokio::test]
    async fn http_list_collections_empty() {
        let server = test_server();
        let resp = server.get("/collections").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["collections"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn http_list_and_describe_collections() {
        let server = test_server();

        // Create two collections.
        server
            .post("/collections/apples")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/collections/bananas")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);

        // Seed an entity into "bananas".
        server
            .post("/entities/bananas/b-001")
            .json(&json!({"data": {"name": "cavendish"}}))
            .await
            .assert_status(StatusCode::CREATED);

        // List.
        let resp = server.get("/collections").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        let cols = body["collections"].as_array().unwrap();
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[0]["name"], "apples");
        assert_eq!(cols[0]["entity_count"], 0);
        assert_eq!(cols[1]["name"], "bananas");
        assert_eq!(cols[1]["entity_count"], 1);

        // Describe "bananas".
        let resp = server.get("/collections/bananas").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["name"], "bananas");
        assert_eq!(body["entity_count"], 1);
    }

    #[tokio::test]
    async fn http_describe_unknown_collection_returns_404() {
        let server = test_server();
        let resp = server.get("/collections/ghost").await;
        resp.assert_status_not_found();
        let body: Value = resp.json();
        assert_eq!(body["code"], "not_found");
    }

    #[tokio::test]
    async fn http_create_collection_with_invalid_name_returns_400() {
        let server = test_server();
        let resp = server
            .post("/collections/BadName")
            .json(&json!({"schema": {}}))
            .await;
        resp.assert_status(StatusCode::BAD_REQUEST);
        let body: Value = resp.json();
        assert_eq!(body["code"], "invalid_argument");
    }

    #[tokio::test]
    async fn http_create_collection_without_schema_returns_400() {
        let server = test_server();
        let resp = server.post("/collections/good-name").json(&json!({})).await;
        resp.assert_status(StatusCode::BAD_REQUEST);
        let body: Value = resp.json();
        assert_eq!(body["code"], "invalid_argument");
    }

    // ── Schema endpoints ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn http_put_and_get_schema() {
        let server = test_server();

        // PUT schema.
        let resp = server
            .put("/collections/invoices/schema")
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
        let resp = server.get("/collections/invoices/schema").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["schema"]["collection"], "invoices");
        assert_eq!(body["schema"]["version"], 1);
        assert!(body["schema"]["entity_schema"]["required"].is_array());
    }

    #[tokio::test]
    async fn http_get_schema_missing_returns_404() {
        let server = test_server();
        let resp = server.get("/collections/nonexistent/schema").await;
        resp.assert_status_not_found();
        let body: Value = resp.json();
        assert_eq!(body["code"], "not_found");
    }

    #[tokio::test]
    async fn http_schema_enforced_on_entity_create() {
        let server = test_server();

        // Register a schema requiring "amount" field.
        server
            .put("/collections/payments/schema")
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
            .post("/entities/payments/p-001")
            .json(&json!({"data": {"note": "oops"}}))
            .await;
        resp.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
        let body: Value = resp.json();
        assert_eq!(body["code"], "schema_validation");

        // Entity with "amount" must succeed.
        let resp = server
            .post("/entities/payments/p-001")
            .json(&json!({"data": {"amount": 42.0}}))
            .await;
        resp.assert_status(StatusCode::CREATED);
    }

    #[tokio::test]
    async fn http_put_schema_actor_recorded_in_audit() {
        let server = test_server();

        // PUT schema with an explicit actor.
        server
            .put("/collections/invoices/schema")
            .json(&json!({
                "version": 1,
                "actor": "schema-admin"
            }))
            .await
            .assert_status_ok();

        // Audit log must contain a SchemaUpdate entry with the provided actor.
        let resp = server.get("/audit/query?collection=invoices").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        let entries = body["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["actor"], "schema-admin");
        assert_eq!(entries[0]["mutation"], "schema.update");
    }

    #[tokio::test]
    async fn http_query_entities_and_combinator() {
        let server = test_server();

        server
            .post("/entities/tasks/t-1")
            .json(&json!({"data": {"status": "open", "assignee": "alice"}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/entities/tasks/t-2")
            .json(&json!({"data": {"status": "open", "assignee": "bob"}}))
            .await
            .assert_status(StatusCode::CREATED);

        let resp = server
            .post("/collections/tasks/query")
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
    #[tokio::test]
    async fn http_entity_with_id_query_create_and_get() {
        let server = test_server();

        // POST /entities/tasks/query must create an entity with ID "query".
        let resp = server
            .post("/entities/tasks/query")
            .json(&json!({"data": {"title": "reserved-id"}}))
            .await;
        resp.assert_status(StatusCode::CREATED);
        let body: Value = resp.json();
        assert_eq!(body["entity"]["id"], "query");

        // GET /entities/tasks/query must retrieve the entity with ID "query".
        let resp = server.get("/entities/tasks/query").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["entity"]["id"], "query");
        assert_eq!(body["entity"]["data"]["title"], "reserved-id");
    }

    #[tokio::test]
    async fn http_query_endpoint_accessible_at_collections_path() {
        let server = test_server();

        server
            .post("/entities/tasks/t-1")
            .json(&json!({"data": {"status": "open"}}))
            .await
            .assert_status(StatusCode::CREATED);

        // POST /collections/{collection}/query is the non-conflicting query endpoint.
        let resp = server
            .post("/collections/tasks/query")
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

    // ── Transaction endpoint ────────────────────────────────────────────────

    #[tokio::test]
    async fn http_transaction_commits_atomically() {
        let server = test_server();

        // Create two entities first.
        server
            .post("/entities/accounts/A")
            .json(&json!({"data": {"balance": 100}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/entities/accounts/B")
            .json(&json!({"data": {"balance": 50}}))
            .await
            .assert_status(StatusCode::CREATED);

        // Commit a transaction: debit A, credit B.
        let resp = server
            .post("/transactions")
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
        let resp = server.get("/entities/accounts/A").await;
        let body: Value = resp.json();
        assert_eq!(body["entity"]["data"]["balance"], 70);
        assert_eq!(body["entity"]["version"], 2);
    }

    #[tokio::test]
    async fn http_transaction_rolls_back_on_conflict() {
        let server = test_server();

        server
            .post("/entities/accounts/X")
            .json(&json!({"data": {"balance": 100}}))
            .await
            .assert_status(StatusCode::CREATED);

        // Transaction with wrong expected_version.
        let resp = server
            .post("/transactions")
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
        let resp = server.get("/entities/accounts/X").await;
        let body: Value = resp.json();
        assert_eq!(body["entity"]["data"]["balance"], 100);
    }

    #[tokio::test]
    async fn http_transaction_creates_and_deletes() {
        let server = test_server();

        // Seed an entity to delete.
        server
            .post("/entities/temp/d-001")
            .json(&json!({"data": {"x": 1}}))
            .await
            .assert_status(StatusCode::CREATED);

        // Transaction: create one entity, delete another.
        let resp = server
            .post("/transactions")
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
        server.get("/entities/temp/c-001").await.assert_status_ok();
        // d-001 should be gone.
        server
            .get("/entities/temp/d-001")
            .await
            .assert_status_not_found();
    }
}
