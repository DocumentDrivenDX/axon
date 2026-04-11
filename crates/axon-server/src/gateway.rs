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
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, get_service, post, put};
use axum::{Extension, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tower_http::services::{ServeDir, ServeFile};

use axon_graphql::BroadcastBroker;

use crate::actor_scope::ActorScopeGuard;
use crate::auth::{AuthContext, AuthError, Identity};
use crate::collection_listing::{filter_audit_entries_to_database, list_collections_for_database};
use crate::mcp_http::{notify_entity_change, notify_entity_change_by_parts, McpHttpSessions};
use crate::rate_limit::{RateLimited, WriteRateLimiter};
use axon_api::handler::AxonHandler;
use axon_api::request::{
    CreateCollectionRequest, CreateDatabaseRequest, CreateEntityRequest, CreateLinkRequest,
    CreateNamespaceRequest, DeleteCollectionTemplateRequest, DeleteEntityRequest,
    DeleteLinkRequest, DescribeCollectionRequest, DropCollectionRequest, DropDatabaseRequest,
    DropNamespaceRequest, GetCollectionTemplateRequest, GetEntityRequest, GetSchemaRequest,
    ListCollectionsRequest, ListDatabasesRequest, ListNamespaceCollectionsRequest,
    ListNamespacesRequest, PutCollectionTemplateRequest, PutSchemaRequest, QueryAuditRequest,
    QueryEntitiesRequest, RevertEntityRequest, RollbackCollectionRequest, RollbackEntityRequest,
    RollbackEntityTarget, RollbackTransactionRequest, TraverseRequest, UpdateEntityRequest,
};
use axon_api::response::GetEntityMarkdownResponse;
use axon_audit::AuditLog;
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId, Namespace, DEFAULT_DATABASE, DEFAULT_SCHEMA};
use axon_core::types::Entity;
use axon_schema::schema::CollectionSchema;
use axon_storage::adapter::StorageAdapter;

type SharedHandler<S> = Arc<Mutex<AxonHandler<S>>>;
const AXON_DATABASE_HEADER: &str = "x-axon-database";

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
        AxonError::UniqueViolation { field, value } => (
            StatusCode::CONFLICT,
            Json(ApiError::new(
                "unique_violation",
                json!({"field": field, "value": value}),
            )),
        )
            .into_response(),
    }
}

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
            "rate_limited",
            format!(
                "write rate limit exceeded, retry after {} seconds",
                limited.retry_after_secs
            ),
        )),
    )
        .into_response()
}

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
struct RequestedDatabaseScope(Option<String>);

impl RequestedDatabaseScope {
    fn database(&self) -> Option<&str> {
        self.0.as_deref()
    }
}

fn requested_database_scope(request: &axum::extract::Request) -> RequestedDatabaseScope {
    if let Some(database) = request
        .uri()
        .path()
        .strip_prefix("/db/")
        .and_then(|rest| rest.split('/').next())
        .filter(|database| !database.is_empty())
    {
        return RequestedDatabaseScope(Some(database.to_string()));
    }

    RequestedDatabaseScope(
        request
            .headers()
            .get(AXON_DATABASE_HEADER)
            .and_then(|value| value.to_str().ok())
            .filter(|database| !database.is_empty())
            .map(str::to_string),
    )
}

fn request_current_database(request: &axum::extract::Request) -> CurrentDatabase {
    let requested_scope = requested_database_scope(request);
    CurrentDatabase::new(requested_scope.database().unwrap_or(DEFAULT_DATABASE))
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

pub(crate) async fn authenticate_http_request(
    State(auth): State<AuthContext>,
    mut request: axum::extract::Request,
    next: Next,
) -> Response {
    let requested_database_scope = requested_database_scope(&request);
    let current_database = request_current_database(&request);
    request.extensions_mut().insert(current_database);
    request.extensions_mut().insert(requested_database_scope);
    match auth.resolve_peer(request_peer_address(&request)).await {
        Ok(identity) => {
            request.extensions_mut().insert(identity);
            next.run(request).await
        }
        Err(error) => auth_error_response(error),
    }
}

/// Returns the current authenticated identity as JSON.
///
/// This endpoint always succeeds for authenticated (or guest/anonymous) users.
/// The UI calls this on load to determine who the current user is.
async fn auth_me(Extension(identity): Extension<Identity>) -> impl IntoResponse {
    Json(json!({
        "actor": identity.actor,
        "role": identity.role,
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
fn broadcast_entity_change(broker: &BroadcastBroker, entity: &Entity, operation: &str) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let event = axon_graphql::ChangeEvent {
        collection: entity.collection.to_string(),
        entity_id: entity.id.to_string(),
        operation: operation.to_string(),
        data: Some(entity.data.clone()),
        previous_data: None,
        version: entity.version,
        timestamp_ms: now,
        actor: "system".to_string(),
    };
    let _ = broker.publish(event);
}

/// Broadcast a delete event to GraphQL subscription clients.
fn broadcast_entity_delete(broker: &BroadcastBroker, collection: &str, entity_id: &str) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let event = axon_graphql::ChangeEvent {
        collection: collection.to_string(),
        entity_id: entity_id.to_string(),
        operation: "delete".to_string(),
        data: None,
        previous_data: None,
        version: 0,
        timestamp_ms: now,
        actor: "system".to_string(),
    };
    let _ = broker.publish(event);
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

#[allow(clippy::too_many_arguments)]
async fn create_entity<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
    Extension(mcp_sessions): Extension<McpHttpSessions>,
    Extension(broker): Extension<BroadcastBroker>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(identity): Extension<Identity>,
    Extension(rate_limiter): Extension<WriteRateLimiter>,
    Extension(actor_scope): Extension<ActorScopeGuard>,
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
    match handler.lock().await.create_entity(CreateEntityRequest {
        collection: qualify_collection_name(&collection, &current_database),
        id: EntityId::new(&id),
        data: body.data,
        actor: Some(identity.actor),
        audit_metadata: None,
    }) {
        Ok(resp) => {
            notify_entity_change(&mcp_sessions, &current_database, &resp.entity);
            broadcast_entity_change(&broker, &resp.entity, "create");
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

async fn get_entity<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
    Extension(current_database): Extension<CurrentDatabase>,
    Path(CollectionEntityPath { collection, id }): Path<CollectionEntityPath>,
) -> Response {
    match handler.lock().await.get_entity(GetEntityRequest {
        collection: qualify_collection_name(&collection, &current_database),
        id: EntityId::new(&id),
    }) {
        Ok(resp) => Json(json!({
            "entity": entity_payload(&resp.entity)
        }))
        .into_response(),
        Err(e) => axon_error_response(e),
    }
}

async fn get_collection_entity<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
    Extension(current_database): Extension<CurrentDatabase>,
    Path(CollectionEntityPath { collection, id }): Path<CollectionEntityPath>,
    Query(params): Query<GetEntityParams>,
) -> Response {
    let collection_id = qualify_collection_name(&collection, &current_database);
    let entity_id = EntityId::new(&id);

    match params.format.as_deref() {
        Some("markdown") => match handler
            .lock()
            .await
            .get_entity_markdown(&collection_id, &entity_id)
        {
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
        None => match handler.lock().await.get_entity(GetEntityRequest {
            collection: collection_id,
            id: entity_id,
        }) {
            Ok(resp) => Json(json!({
                "entity": entity_payload(&resp.entity)
            }))
            .into_response(),
            Err(e) => axon_error_response(e),
        },
    }
}

#[allow(clippy::too_many_arguments)]
async fn update_entity<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
    Extension(mcp_sessions): Extension<McpHttpSessions>,
    Extension(broker): Extension<BroadcastBroker>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(identity): Extension<Identity>,
    Extension(rate_limiter): Extension<WriteRateLimiter>,
    Extension(actor_scope): Extension<ActorScopeGuard>,
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
    match handler.lock().await.update_entity(UpdateEntityRequest {
        collection: qualify_collection_name(&collection, &current_database),
        id: EntityId::new(&id),
        data: body.data,
        expected_version: body.expected_version,
        actor: Some(identity.actor),
        audit_metadata: None,
    }) {
        Ok(resp) => {
            notify_entity_change(&mcp_sessions, &current_database, &resp.entity);
            broadcast_entity_change(&broker, &resp.entity, "update");
            Json(json!({
                "entity": entity_payload(&resp.entity)
            }))
            .into_response()
        }
        Err(e) => axon_error_response(e),
    }
}

#[allow(clippy::too_many_arguments)]
async fn delete_entity<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
    Extension(mcp_sessions): Extension<McpHttpSessions>,
    Extension(broker): Extension<BroadcastBroker>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(identity): Extension<Identity>,
    Extension(rate_limiter): Extension<WriteRateLimiter>,
    Extension(actor_scope): Extension<ActorScopeGuard>,
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
    match handler.lock().await.delete_entity(DeleteEntityRequest {
        collection: qualify_collection_name(&collection, &current_database),
        id: EntityId::new(&id),
        actor: Some(identity.actor),
        audit_metadata: None,
        force: false,
    }) {
        Ok(resp) => {
            notify_entity_change_by_parts(&mcp_sessions, &current_database, &collection, &id);
            broadcast_entity_delete(&broker, &collection, &id);
            Json(json!({"collection": resp.collection, "id": resp.id})).into_response()
        }
        Err(e) => axon_error_response(e),
    }
}

async fn query_entities<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
    Extension(current_database): Extension<CurrentDatabase>,
    Path(CollectionPath { collection }): Path<CollectionPath>,
    Json(body): Json<QueryEntitiesRequest>,
) -> Response {
    // Allow the caller to omit the collection field in the body; the path wins.
    let req = QueryEntitiesRequest {
        collection: qualify_collection_name(&collection, &current_database),
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

async fn create_link<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(identity): Extension<Identity>,
    Extension(rate_limiter): Extension<WriteRateLimiter>,
    Extension(actor_scope): Extension<ActorScopeGuard>,
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
    match handler.lock().await.create_link(CreateLinkRequest {
        source_collection: qualify_collection_name(&body.source_collection, &current_database),
        source_id: EntityId::new(&body.source_id),
        target_collection: qualify_collection_name(&body.target_collection, &current_database),
        target_id: EntityId::new(&body.target_id),
        link_type: body.link_type,
        metadata: body.metadata,
        actor: Some(identity.actor),
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

async fn delete_link<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(identity): Extension<Identity>,
    Extension(rate_limiter): Extension<WriteRateLimiter>,
    Extension(actor_scope): Extension<ActorScopeGuard>,
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
    match handler.lock().await.delete_link(DeleteLinkRequest {
        source_collection: qualify_collection_name(&body.source_collection, &current_database),
        source_id: EntityId::new(&body.source_id),
        target_collection: qualify_collection_name(&body.target_collection, &current_database),
        target_id: EntityId::new(&body.target_id),
        link_type: body.link_type,
        actor: Some(identity.actor),
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

async fn traverse<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
    Extension(current_database): Extension<CurrentDatabase>,
    Path(CollectionEntityPath { collection, id }): Path<CollectionEntityPath>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let link_type = params.get("link_type").cloned();
    let max_depth = params.get("max_depth").and_then(|s| s.parse().ok());

    match handler.lock().await.traverse(TraverseRequest {
        collection: qualify_collection_name(&collection, &current_database),
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

async fn query_audit_by_entity<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(requested_database_scope): Extension<RequestedDatabaseScope>,
    Path(CollectionEntityPath {
        collection,
        id: entity_id,
    }): Path<CollectionEntityPath>,
) -> Response {
    let handler = handler.lock().await;
    match handler.audit_log().query_by_entity(
        &qualify_collection_name(&collection, &current_database),
        &EntityId::new(&entity_id),
    ) {
        Ok(entries) => {
            let entries =
                filter_audit_entries_to_database(entries, requested_database_scope.database());
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

async fn query_audit<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(requested_database_scope): Extension<RequestedDatabaseScope>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let req = QueryAuditRequest {
        database: requested_database_scope.database().map(str::to_string),
        collection: params
            .get("collection")
            .map(|collection| qualify_collection_name(collection, &current_database)),
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
                        "data_before": e.data_before,
                        "data_after": e.data_after,
                        "actor": e.actor,
                        "transaction_id": e.transaction_id,
                    })
                })
                .collect();
            Json(json!({ "entries": proto, "next_cursor": next_cursor })).into_response()
        }
        Err(e) => axon_error_response(e),
    }
}

async fn revert_entity<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
    Extension(mcp_sessions): Extension<McpHttpSessions>,
    Extension(broker): Extension<BroadcastBroker>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(identity): Extension<Identity>,
    Extension(rate_limiter): Extension<WriteRateLimiter>,
    Json(body): Json<RevertEntityBody>,
) -> Response {
    if let Err(e) = identity.require_write() {
        return auth_error_response(e);
    }
    if let Err(limited) = rate_limiter.check(&identity.actor).await {
        return rate_limit_response(&limited);
    }
    match handler
        .lock()
        .await
        .revert_entity_to_audit_entry(RevertEntityRequest {
            audit_entry_id: body.audit_entry_id,
            actor: Some(identity.actor),
            force: body.force,
        }) {
        Ok(resp) => {
            notify_entity_change(&mcp_sessions, &current_database, &resp.entity);
            broadcast_entity_change(&broker, &resp.entity, "update");
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
            Json(ApiError::new("schema_validation", detail)),
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
async fn rollback_collection_entity<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
    Extension(mcp_sessions): Extension<McpHttpSessions>,
    Extension(broker): Extension<BroadcastBroker>,
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

    match handler.lock().await.rollback_entity(RollbackEntityRequest {
        collection: qualify_collection_name(&collection, &current_database),
        id: EntityId::new(&id),
        target,
        expected_version: body.expected_version,
        actor: Some(identity.actor),
        dry_run: body.dry_run,
    }) {
        Ok(axon_api::response::RollbackEntityResponse::Applied {
            entity,
            audit_entry,
        }) => {
            notify_entity_change(&mcp_sessions, &current_database, &entity);
            broadcast_entity_change(&broker, &entity, "update");
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

async fn rollback_collection_handler<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
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

async fn rollback_transaction_handler<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
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

async fn create_collection<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
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
        gates: Default::default(),
        validation_rules: Default::default(),
        indexes: Default::default(),
        compound_indexes: Default::default(),
    };
    match handler
        .lock()
        .await
        .create_collection(CreateCollectionRequest {
            name: collection_id,
            schema,
            actor: Some(identity.actor),
        }) {
        Ok(resp) => (StatusCode::CREATED, Json(json!({ "name": resp.name }))).into_response(),
        Err(e) => axon_error_response(e),
    }
}

async fn drop_collection<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
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
        Ok(resp) => Json(json!({
            "name": resp.name,
            "entities_removed": resp.entities_removed,
        }))
        .into_response(),
        Err(e) => axon_error_response(e),
    }
}

async fn list_collections<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
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

async fn describe_collection<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
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

async fn put_collection_template<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
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

async fn get_collection_template<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
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

async fn delete_collection_template<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
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

async fn put_schema<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
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
        gates: Default::default(),
        validation_rules: Default::default(),
        indexes: Default::default(),
        compound_indexes: Default::default(),
    };
    match handler.lock().await.handle_put_schema(PutSchemaRequest {
        schema,
        actor: Some(identity.actor),
        force: body.force,
        dry_run: body.dry_run,
    }) {
        Ok(resp) => {
            let mut result = json!({ "schema": resp.schema });
            if let Some(compat) = &resp.compatibility {
                result["compatibility"] = json!(compat);
            }
            if let Some(diff) = &resp.diff {
                result["diff"] = json!(diff);
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

async fn get_schema<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
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

async fn create_database<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
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

async fn list_databases<S: StorageAdapter>(State(handler): State<SharedHandler<S>>) -> Response {
    match handler.lock().await.list_databases(ListDatabasesRequest {}) {
        Ok(resp) => Json(json!({ "databases": resp.databases })).into_response(),
        Err(err) => axon_error_response(err),
    }
}

async fn drop_database<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
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

async fn create_namespace<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
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

async fn list_namespaces<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
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

async fn list_namespace_collections<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
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

async fn drop_namespace<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
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

#[allow(clippy::too_many_arguments)]
async fn commit_transaction<S: StorageAdapter>(
    State(handler): State<SharedHandler<S>>,
    Extension(mcp_sessions): Extension<McpHttpSessions>,
    Extension(broker): Extension<BroadcastBroker>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(identity): Extension<Identity>,
    Extension(rate_limiter): Extension<WriteRateLimiter>,
    Extension(actor_scope): Extension<ActorScopeGuard>,
    Json(body): Json<TransactionBody>,
) -> Response {
    if let Err(e) = identity.require_write() {
        return auth_error_response(e);
    }
    // Check actor scope for every collection referenced in the transaction.
    for op in &body.operations {
        let collection = match op {
            TransactionOp::Create { collection, .. }
            | TransactionOp::Update { collection, .. }
            | TransactionOp::Delete { collection, .. } => collection,
        };
        if let Err(e) = actor_scope.check(&identity.actor, collection, &identity.role) {
            return auth_error_response(e);
        }
    }
    if let Err(limited) = rate_limiter.check(&identity.actor).await {
        return rate_limit_response(&limited);
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
                    .get_entity(GetEntityRequest {
                        collection: qualify_collection_name(&collection, &current_database),
                        id: EntityId::new(&id),
                    })
                    .ok()
                    .map(|r| r.entity.data);
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
            TransactionOp::Delete {
                collection,
                id,
                expected_version,
            } => {
                let h = handler.lock().await;
                let data_before = h
                    .get_entity(GetEntityRequest {
                        collection: qualify_collection_name(&collection, &current_database),
                        id: EntityId::new(&id),
                    })
                    .ok()
                    .map(|r| r.entity.data);
                drop(h);
                tx.delete(
                    qualify_collection_name(&collection, &current_database),
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
    match tx.commit(storage, audit, Some(identity.actor)) {
        Ok(written) => {
            for entity in &written {
                notify_entity_change(&mcp_sessions, &current_database, entity);
                broadcast_entity_change(&broker, entity, "update");
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

// ── GraphQL handler ──────────────────────────────────────────────────────────

/// Collect all `CollectionSchema`s from the handler, then build and execute
/// a dynamic GraphQL schema for each incoming request.
///
/// Rebuilding per-request ensures newly-created (or dropped) collections are
/// always reflected in the GraphQL API. A caching layer can be added later
/// for performance.
async fn graphql_handler<S: StorageAdapter + 'static>(
    State(handler): State<SharedHandler<S>>,
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

    // 3. Execute the request.
    let response = gql_schema.schema.execute(req.into_inner()).await;
    async_graphql_axum::GraphQLResponse::from(response).into_response()
}

/// GraphQL WebSocket subscription endpoint.
///
/// Upgrades the HTTP connection to a WebSocket and runs the graphql-ws
/// protocol. The schema is rebuilt on connection to reflect current
/// collections.
async fn graphql_ws_handler<S: StorageAdapter + 'static>(
    State(handler): State<SharedHandler<S>>,
    Extension(broker): Extension<BroadcastBroker>,
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
    let gql_schema = match axon_graphql::build_schema_with_handler_and_broker(
        &schemas,
        handler.clone(),
        Some(broker),
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

// ── Router construction ───────────────────────────────────────────────────────

/// Build the axum router for the HTTP gateway.
pub fn build_router<S: StorageAdapter + 'static>(
    handler: SharedHandler<S>,
    backend: impl Into<String>,
    ui_dir: Option<PathBuf>,
) -> Router {
    build_router_with_auth(
        handler,
        backend,
        ui_dir,
        AuthContext::no_auth(),
        crate::rate_limit::RateLimitConfig::default(),
        ActorScopeGuard::default(),
        None,
    )
}

fn data_routes<S: StorageAdapter + 'static>() -> Router<SharedHandler<S>> {
    Router::new()
        .merge(crate::mcp_http::routes::<S>())
        .route("/entities/{collection}/{id}", post(create_entity::<S>))
        .route("/entities/{collection}/{id}", get(get_entity::<S>))
        .route("/entities/{collection}/{id}", put(update_entity::<S>))
        .route("/entities/{collection}/{id}", delete(delete_entity::<S>))
        .route(
            "/collections/{collection}/entities/{id}",
            get(get_collection_entity::<S>),
        )
        .route(
            "/collections/{collection}/entities/{id}/rollback",
            post(rollback_collection_entity::<S>),
        )
        .route(
            "/collections/{collection}/rollback",
            post(rollback_collection_handler::<S>),
        )
        .route("/collections/{collection}/query", post(query_entities::<S>))
        .route("/links", post(create_link::<S>))
        .route("/links", delete(delete_link::<S>))
        .route("/traverse/{collection}/{id}", get(traverse::<S>))
        .route(
            "/audit/entity/{collection}/{id}",
            get(query_audit_by_entity::<S>),
        )
        .route("/audit/query", get(query_audit::<S>))
        .route("/audit/revert", post(revert_entity::<S>))
        .route("/collections", get(list_collections::<S>))
        .route("/collections/{name}", post(create_collection::<S>))
        .route("/collections/{name}", get(describe_collection::<S>))
        .route("/collections/{name}", delete(drop_collection::<S>))
        .route(
            "/collections/{collection}/template",
            put(put_collection_template::<S>),
        )
        .route(
            "/collections/{collection}/template",
            get(get_collection_template::<S>),
        )
        .route(
            "/collections/{collection}/template",
            delete(delete_collection_template::<S>),
        )
        .route("/collections/{name}/schema", put(put_schema::<S>))
        .route("/collections/{name}/schema", get(get_schema::<S>))
        .route("/transactions", post(commit_transaction::<S>))
        .route(
            "/transactions/{transaction_id}/rollback",
            post(rollback_transaction_handler::<S>),
        )
}

/// Build the axum router for the HTTP gateway with request authentication.
pub fn build_router_with_auth<S: StorageAdapter + 'static>(
    handler: SharedHandler<S>,
    backend: impl Into<String>,
    ui_dir: Option<PathBuf>,
    auth: AuthContext,
    rate_limit_config: crate::rate_limit::RateLimitConfig,
    actor_scope: ActorScopeGuard,
    control_plane: Option<crate::control_plane_routes::ControlPlaneState>,
) -> Router {
    let start = Instant::now();
    let backend = backend.into();
    let mcp_sessions = McpHttpSessions::default();
    let rate_limiter = WriteRateLimiter::new(rate_limit_config);
    let mut router = Router::new()
        .merge(data_routes::<S>())
        .nest("/db/{database}", data_routes::<S>())
        .nest("/v1", data_routes::<S>())
        .nest("/db/{database}/v1", data_routes::<S>())
        .route("/databases", get(list_databases::<S>))
        .route("/databases/{name}", post(create_database::<S>))
        .route("/databases/{name}", delete(drop_database::<S>))
        .route("/databases/{database}/schemas", get(list_namespaces::<S>))
        .route(
            "/databases/{database}/schemas/{schema}",
            post(create_namespace::<S>),
        )
        .route(
            "/databases/{database}/schemas/{schema}",
            delete(drop_namespace::<S>),
        )
        .route(
            "/databases/{database}/schemas/{schema}/collections",
            get(list_namespace_collections::<S>),
        )
        .route(
            "/graphql",
            get(graphql_handler::<S>).post(graphql_handler::<S>),
        )
        .route("/graphql/ws", get(graphql_ws_handler::<S>))
        .with_state(handler.clone())
        .layer(Extension(rate_limiter))
        .layer(Extension(actor_scope))
        .layer(Extension(mcp_sessions))
        .layer(Extension(BroadcastBroker::default()))
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

    router = router.route("/auth/me", get(auth_me));

    if let Some(ui_dir) = ui_dir {
        let index_path = ui_dir.join("index.html");
        let ui_service = get_service(ServeDir::new(ui_dir).fallback(ServeFile::new(index_path)));
        router = router.nest_service("/ui", ui_service);
    }

    if let Some(cp) = control_plane {
        let cp_routes = crate::control_plane_routes::control_plane_routes()
            .with_state(cp);
        router = router.nest("/control", cp_routes);
    }

    router.layer(middleware::from_fn_with_state(
        auth,
        authenticate_http_request,
    ))
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
    use axon_core::id::{CollectionId, Namespace};
    use axon_schema::schema::{CollectionSchema, CollectionView, IndexDef, IndexType};
    use axon_storage::adapter::StorageAdapter;
    use axon_storage::MemoryStorageAdapter;
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

    fn test_server_with_handler() -> (TestServer, SharedHandler<MemoryStorageAdapter>) {
        let handler = Arc::new(Mutex::new(
            AxonHandler::new(MemoryStorageAdapter::default()),
        ));
        let app = build_router(handler.clone(), "memory", None);
        (TestServer::new(app), handler)
    }

    fn test_server() -> TestServer {
        test_server_with_handler().0
    }

    fn test_server_with_auth(peer: SocketAddr, auth: AuthContext) -> TestServer {
        let handler = Arc::new(Mutex::new(
            AxonHandler::new(MemoryStorageAdapter::default()),
        ));
        let app = build_router_with_auth(
            handler,
            "memory",
            None,
            auth,
            crate::rate_limit::RateLimitConfig::default(),
            ActorScopeGuard::default(),
            None,
        )
        .layer(MockConnectInfo(peer));
        TestServer::new(app)
    }

    fn ok_or_panic<T, E: Display>(result: Result<T, E>, context: &str) -> T {
        match result {
            Ok(value) => value,
            Err(err) => panic!("{context}: {err}"),
        }
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
    async fn http_collection_entity_get_defaults_to_json() {
        let server = test_server();

        server
            .post("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "hello"}}))
            .await
            .assert_status(StatusCode::CREATED);

        let resp = server.get("/collections/tasks/entities/t-001").await;

        resp.assert_status_ok();
        resp.assert_header("content-type", "application/json");
        let body: Value = resp.json();
        assert_eq!(body["entity"]["data"]["title"], "hello");
    }

    #[tokio::test]
    async fn http_collection_entity_get_markdown_returns_text_markdown() {
        let (server, handler) = test_server_with_handler();

        server
            .post("/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/entities/tasks/t-001")
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
            .get("/collections/tasks/entities/t-001?format=markdown")
            .await;

        resp.assert_status_ok();
        resp.assert_header("content-type", "text/markdown; charset=utf-8");
        assert_eq!(resp.text(), "# hello\n\nStatus: open");
    }

    #[tokio::test]
    async fn http_collection_entity_get_markdown_requires_template() {
        let server = test_server();

        server
            .post("/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "hello"}}))
            .await
            .assert_status(StatusCode::CREATED);

        let resp = server
            .get("/collections/tasks/entities/t-001?format=markdown")
            .await;

        resp.assert_status(StatusCode::BAD_REQUEST);
        let body: Value = resp.json();
        assert_eq!(body["code"], "invalid_argument");
        assert!(body["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("has no markdown template defined"));
    }

    #[tokio::test]
    async fn http_collection_entity_get_markdown_render_failure_returns_entity_payload() {
        let (server, handler) = test_server_with_handler();

        server
            .post("/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/entities/tasks/t-001")
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
            .get("/collections/tasks/entities/t-001?format=markdown")
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

    #[tokio::test]
    async fn http_collection_template_crud_round_trip_uses_public_surface() {
        let server = test_server();

        server
            .post("/collections/tasks")
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
            .put("/collections/tasks/template")
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

        let get = server.get("/collections/tasks/template").await;
        get.assert_status_ok();
        let body: Value = get.json();
        assert_eq!(body["template"], "# {{title}}\n\n{{notes}}");

        let delete = server.delete("/collections/tasks/template").await;
        delete.assert_status_ok();
        let body: Value = delete.json();
        assert_eq!(body["collection"], "tasks");
        assert_eq!(body["status"], "deleted");

        server
            .get("/collections/tasks/template")
            .await
            .assert_status(StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn http_collection_template_delete_accepts_empty_json_body() {
        let server = test_server();

        server
            .post("/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);

        server
            .put("/collections/tasks/template")
            .json(&json!({
                "template": "# {{title}}"
            }))
            .await
            .assert_status_ok();

        let delete = server
            .delete("/collections/tasks/template")
            .content_type("application/json")
            .bytes(Bytes::new())
            .await;
        delete.assert_status_ok();
        let body: Value = delete.json();
        assert_eq!(body["collection"], "tasks");
        assert_eq!(body["status"], "deleted");
    }

    #[tokio::test]
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
                    gates: Default::default(),
                    validation_rules: Default::default(),
                    indexes: Default::default(),
                    compound_indexes: Default::default(),
                }),
                "storing qualified schema for template HTTP test",
            );
        }

        let put = server
            .put("/collections/prod.billing.tasks/template")
            .json(&json!({
                "template": "# {{title}}",
                "actor": "operator"
            }))
            .await;
        put.assert_status_ok();
        let body: Value = put.json();
        assert_eq!(body["collection"], "prod.billing.tasks");
        assert_eq!(body["template"], "# {{title}}");

        let get = server.get("/collections/prod.billing.tasks/template").await;
        get.assert_status_ok();
        let body: Value = get.json();
        assert_eq!(body["collection"], "prod.billing.tasks");
        assert_eq!(body["template"], "# {{title}}");
    }

    #[tokio::test]
    async fn http_collection_template_put_accepts_text_plain_body() {
        let server = test_server();

        server
            .post("/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);

        let put = server
            .put("/collections/tasks/template")
            .text("# {{title}}")
            .await;
        put.assert_status_ok();
        let body: Value = put.json();
        assert_eq!(body["template"], "# {{title}}");
        assert_eq!(body["warnings"], json!([]));

        server
            .post("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "hello"}}))
            .await
            .assert_status(StatusCode::CREATED);

        let markdown = server
            .get("/collections/tasks/entities/t-001?format=markdown")
            .await;
        markdown.assert_status_ok();
        assert_eq!(markdown.text(), "# hello");
    }

    #[tokio::test]
    async fn http_collection_template_put_rejects_unknown_schema_fields() {
        let server = test_server();

        server
            .post("/collections/tasks")
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
            .put("/collections/tasks/template")
            .json(&json!({"template": "{{ghost}}"}))
            .await;

        resp.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
        let body: Value = resp.json();
        assert_eq!(body["code"], "schema_validation");
        assert!(body["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("template references field 'ghost'"));
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
        assert_eq!(entries[0]["actor"], "anonymous");
    }

    #[tokio::test]
    async fn http_query_audit_by_entity_scopes_to_requested_database() {
        let server = test_server();

        server
            .post("/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/databases/prod")
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/db/prod/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);

        server
            .post("/entities/tasks/t-001")
            .json(&json!({"data": {"scope": "default"}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/db/prod/entities/tasks/t-001")
            .json(&json!({"data": {"scope": "prod"}}))
            .await
            .assert_status(StatusCode::CREATED);

        let path_scoped_resp = server.get("/db/prod/audit/entity/tasks/t-001").await;
        path_scoped_resp.assert_status_ok();
        let path_scoped_body: Value = path_scoped_resp.json();
        let path_scoped_entries = path_scoped_body["entries"]
            .as_array()
            .expect("path-scoped audit/entity should return an entries array");
        assert!(!path_scoped_entries.is_empty());
        assert!(path_scoped_entries
            .iter()
            .all(|entry| entry["collection"] == "prod.default.tasks"));

        let header_scoped_resp = server
            .get("/audit/entity/tasks/t-001")
            .add_header(AXON_DATABASE_HEADER, "prod")
            .await;
        header_scoped_resp.assert_status_ok();
        let header_scoped_body: Value = header_scoped_resp.json();
        let header_scoped_entries = header_scoped_body["entries"]
            .as_array()
            .expect("header-scoped audit/entity should return an entries array");
        assert!(!header_scoped_entries.is_empty());
        assert!(header_scoped_entries
            .iter()
            .all(|entry| entry["collection"] == "prod.default.tasks"));

        let cross_database_resp = server
            .get("/audit/entity/default.default.tasks/t-001")
            .add_header(AXON_DATABASE_HEADER, "prod")
            .await;
        cross_database_resp.assert_status_ok();
        let cross_database_body: Value = cross_database_resp.json();
        let cross_database_entries = cross_database_body["entries"]
            .as_array()
            .expect("cross-database audit/entity should return an entries array");
        assert!(cross_database_entries.is_empty());
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
        let resp = server.get("/audit/query?actor=anonymous").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        let entries = body["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["actor"], "anonymous");

        // Filter by collection.
        let resp = server.get("/audit/query?collection=tasks").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["entries"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
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
            .post("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v1"}, "actor": "spoofed"}))
            .await
            .assert_status(StatusCode::CREATED);

        let resp = server.get("/audit/query?actor=ts-agent").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        let entries = body["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["actor"], "ts-agent");
    }

    #[tokio::test]
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
            .post("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v1"}, "actor": "spoofed"}))
            .await;
        resp.assert_status(StatusCode::UNAUTHORIZED);
        let body: Value = resp.json();
        assert_eq!(body["code"], "unauthorized");
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
    async fn http_entity_rollback_by_version_on_v1_route() {
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

        let resp = server
            .post("/v1/collections/tasks/entities/t-001/rollback")
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

    #[tokio::test]
    async fn http_entity_rollback_recreates_deleted_entity_on_v1_route() {
        let server = test_server();

        server
            .post("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v1"}, "actor": "creator"}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .put("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v2"}, "expected_version": 1, "actor": "editor"}))
            .await
            .assert_status_ok();
        server
            .delete("/entities/tasks/t-001")
            .await
            .assert_status_ok();

        let resp = server
            .post("/v1/collections/tasks/entities/t-001/rollback")
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

        let restored = server.get("/entities/tasks/t-001").await;
        restored.assert_status_ok();
        let restored_body: Value = restored.json();
        assert_eq!(restored_body["entity"]["version"], 3);
        assert_eq!(restored_body["entity"]["data"]["title"], "v1");
    }

    #[tokio::test]
    async fn http_entity_rollback_dry_run_returns_preview_without_write() {
        let server = test_server();

        server
            .post("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v1"}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .put("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v2"}, "expected_version": 1}))
            .await
            .assert_status_ok();

        let resp = server
            .post("/collections/tasks/entities/t-001/rollback")
            .json(&json!({"to_version": 1, "dry_run": true}))
            .await;
        resp.assert_status_ok();

        let body: Value = resp.json();
        assert_eq!(body["current"]["version"], 2);
        assert_eq!(body["current"]["data"]["title"], "v2");
        assert_eq!(body["target"]["version"], 3);
        assert_eq!(body["target"]["data"]["title"], "v1");
        assert_eq!(body["diff"]["title"]["after"], "v1");

        let current = server.get("/entities/tasks/t-001").await;
        current.assert_status_ok();
        let current_body: Value = current.json();
        assert_eq!(current_body["entity"]["version"], 2);
        assert_eq!(current_body["entity"]["data"]["title"], "v2");
    }

    #[tokio::test]
    async fn http_entity_rollback_dry_run_previews_deleted_entity_without_recreate() {
        let server = test_server();

        server
            .post("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "v1", "status": "draft"}, "actor": "alice"}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .put("/entities/tasks/t-001")
            .json(&json!({
                "data": {"title": "v2", "status": "published"},
                "expected_version": 1,
                "actor": "alice"
            }))
            .await
            .assert_status_ok();
        server
            .delete("/entities/tasks/t-001")
            .await
            .assert_status_ok();

        let resp = server
            .post("/v1/collections/tasks/entities/t-001/rollback")
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
            .get("/entities/tasks/t-001")
            .await
            .assert_status(StatusCode::NOT_FOUND);
    }

    #[tokio::test]
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
                })
                .unwrap();
            guard
                .put_schema(CollectionSchema {
                    collection: col.clone(),
                    description: None,
                    version: 1,
                    entity_schema: None,
                    link_types: Default::default(),
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
                })
                .unwrap();
        }

        let resp = server
            .post("/collections/items/entities/g-1/rollback")
            .json(&json!({"to_version": 1}))
            .await;
        resp.assert_status(StatusCode::CONFLICT);
        let body: Value = resp.json();
        assert_eq!(body["code"], "schema_validation");
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

        // Audit log must contain a SchemaUpdate entry with the resolved actor.
        let resp = server.get("/audit/query?collection=invoices").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        let entries = body["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["actor"], "anonymous");
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

    #[tokio::test]
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
                gates: Default::default(),
                validation_rules: Default::default(),
                indexes: vec![IndexDef {
                    field: "external_id".into(),
                    index_type: IndexType::String,
                    unique: true,
                }],
                compound_indexes: Default::default(),
            };
            storage
                .put_schema(&schema(billing_invoices.clone()))
                .expect("billing schema put should succeed");
            storage
                .put_schema(&schema(engineering_invoices.clone()))
                .expect("engineering schema put should succeed");
        }

        server
            .post("/entities/prod.billing.invoices/inv-001")
            .json(&json!({"data": {"external_id": "shared-1", "scope": "billing"}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/entities/prod.engineering.invoices/inv-001")
            .json(&json!({"data": {"external_id": "shared-1", "scope": "engineering"}}))
            .await
            .assert_status(StatusCode::CREATED);

        let resp = server.get("/entities/prod.billing.invoices/inv-001").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["entity"]["collection"], "prod.billing.invoices");
        assert_eq!(body["entity"]["data"]["scope"], "billing");

        let resp = server
            .post("/collections/prod.engineering.invoices/query")
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
            .delete("/entities/prod.billing.invoices/inv-001")
            .await
            .assert_status_ok();
        server
            .get("/entities/prod.billing.invoices/inv-001")
            .await
            .assert_status_not_found();
        server
            .get("/entities/prod.engineering.invoices/inv-001")
            .await
            .assert_status_ok();
    }

    #[tokio::test]
    async fn http_header_current_database_routes_unqualified_collection_operations() {
        let server = test_server();

        server
            .post("/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/databases/prod")
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/collections/tasks")
            .add_header(AXON_DATABASE_HEADER, "prod")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);

        server
            .post("/entities/tasks/t-001")
            .json(&json!({"data": {"scope": "default"}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/entities/tasks/t-001")
            .add_header(AXON_DATABASE_HEADER, "prod")
            .json(&json!({"data": {"scope": "prod"}}))
            .await
            .assert_status(StatusCode::CREATED);

        let default_resp = server.get("/entities/tasks/t-001").await;
        default_resp.assert_status_ok();
        let default_body: Value = default_resp.json();
        assert_eq!(default_body["entity"]["data"]["scope"], "default");

        let prod_resp = server
            .get("/entities/tasks/t-001")
            .add_header(AXON_DATABASE_HEADER, "prod")
            .await;
        prod_resp.assert_status_ok();
        let prod_body: Value = prod_resp.json();
        assert_eq!(prod_body["entity"]["collection"], "prod.default.tasks");
        assert_eq!(prod_body["entity"]["data"]["scope"], "prod");
    }

    #[tokio::test]
    async fn http_db_path_prefix_routes_unqualified_collection_operations() {
        let server = test_server();

        server
            .post("/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/databases/prod")
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/db/prod/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);

        server
            .post("/entities/tasks/t-001")
            .json(&json!({"data": {"scope": "default"}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/db/prod/entities/tasks/t-001")
            .json(&json!({"data": {"scope": "prod"}}))
            .await
            .assert_status(StatusCode::CREATED);

        let prod_resp = server.get("/db/prod/entities/tasks/t-001").await;
        prod_resp.assert_status_ok();
        let prod_body: Value = prod_resp.json();
        assert_eq!(prod_body["entity"]["collection"], "prod.default.tasks");
        assert_eq!(prod_body["entity"]["data"]["scope"], "prod");

        let default_resp = server.get("/entities/tasks/t-001").await;
        default_resp.assert_status_ok();
        let default_body: Value = default_resp.json();
        assert_eq!(default_body["entity"]["data"]["scope"], "default");
    }

    #[tokio::test]
    async fn http_collection_listings_scope_to_selected_database_only_when_requested() {
        let server = test_server();

        server
            .post("/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/databases/prod")
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/db/prod/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);

        let global_resp = server.get("/collections").await;
        global_resp.assert_status_ok();
        let global_body: Value = global_resp.json();
        let global_names: Vec<&str> = global_body["collections"]
            .as_array()
            .expect("global collection list should be an array")
            .iter()
            .map(|collection| {
                collection["name"]
                    .as_str()
                    .expect("collection metadata should include a name")
            })
            .collect();
        assert_eq!(global_names, vec!["tasks", "tasks"]);

        let header_scoped_resp = server
            .get("/collections")
            .add_header(AXON_DATABASE_HEADER, "prod")
            .await;
        header_scoped_resp.assert_status_ok();
        let header_scoped_body: Value = header_scoped_resp.json();
        let header_scoped_collections = header_scoped_body["collections"]
            .as_array()
            .expect("header scoped collection list should be an array");
        assert_eq!(header_scoped_collections.len(), 1);
        assert_eq!(header_scoped_collections[0]["name"], "prod.default.tasks");

        let path_scoped_resp = server.get("/db/prod/collections").await;
        path_scoped_resp.assert_status_ok();
        let path_scoped_body: Value = path_scoped_resp.json();
        let path_scoped_collections = path_scoped_body["collections"]
            .as_array()
            .expect("path scoped collection list should be an array");
        assert_eq!(path_scoped_collections.len(), 1);
        assert_eq!(path_scoped_collections[0]["name"], "prod.default.tasks");
    }

    #[tokio::test]
    async fn http_audit_queries_scope_to_selected_database_only_when_requested() {
        let server = test_server();

        server
            .post("/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/databases/prod")
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/db/prod/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);

        server
            .post("/entities/tasks/t-001")
            .json(&json!({"data": {"scope": "default"}}))
            .await
            .assert_status(StatusCode::CREATED);
        server
            .post("/db/prod/entities/tasks/t-001")
            .json(&json!({"data": {"scope": "prod"}}))
            .await
            .assert_status(StatusCode::CREATED);

        let global_resp = server.get("/audit/query").await;
        global_resp.assert_status_ok();
        let global_body: Value = global_resp.json();
        let global_entries = global_body["entries"]
            .as_array()
            .expect("global audit query should return an entries array");
        assert!(global_entries
            .iter()
            .any(|entry| entry["collection"] == "tasks"));
        assert!(global_entries
            .iter()
            .any(|entry| entry["collection"] == "prod.default.tasks"));

        let header_scoped_resp = server
            .get("/audit/query")
            .add_header(AXON_DATABASE_HEADER, "prod")
            .await;
        header_scoped_resp.assert_status_ok();
        let header_scoped_body: Value = header_scoped_resp.json();
        let header_scoped_entries = header_scoped_body["entries"]
            .as_array()
            .expect("header scoped audit query should return an entries array");
        assert!(!header_scoped_entries.is_empty());
        assert!(header_scoped_entries
            .iter()
            .all(|entry| entry["collection"] == "prod.default.tasks"));

        let path_scoped_resp = server.get("/db/prod/audit/query").await;
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

    #[tokio::test]
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

    #[tokio::test]
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

    #[tokio::test]
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
            .post("/collections/tasks")
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

    #[tokio::test]
    async fn http_serves_ui_assets_under_ui_prefix() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("index.html"),
            "<html><body>Axon UI</body></html>",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("_app")).unwrap();
        std::fs::write(dir.path().join("_app/app.js"), "console.log('ui');").unwrap();

        let handler = Arc::new(Mutex::new(
            AxonHandler::new(MemoryStorageAdapter::default()),
        ));
        let app = build_router(handler, "memory", Some(dir.path().to_path_buf()));
        let server = TestServer::new(app);

        let index = server.get("/ui").await;
        index.assert_status_ok();
        assert!(index.text().contains("Axon UI"));

        let asset = server.get("/ui/_app/app.js").await;
        asset.assert_status_ok();
        assert!(asset.text().contains("console.log"));
    }

    #[tokio::test]
    async fn http_ui_nested_routes_fallback_to_index_html() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("index.html"),
            "<html><body>Axon UI Shell</body></html>",
        )
        .unwrap();

        let handler = Arc::new(Mutex::new(
            AxonHandler::new(MemoryStorageAdapter::default()),
        ));
        let app = build_router(handler, "memory", Some(dir.path().to_path_buf()));
        let server = TestServer::new(app);

        let resp = server.get("/ui/collections/tasks").await;
        resp.assert_status_ok();
        assert!(resp.text().contains("Axon UI Shell"));
    }

    #[tokio::test]
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
        let handler = Arc::new(Mutex::new(
            AxonHandler::new(MemoryStorageAdapter::default()),
        ));
        let app = build_router_with_auth(
            handler,
            "memory",
            None,
            auth,
            crate::rate_limit::RateLimitConfig::default(),
            ActorScopeGuard::default(),
            None,
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
        let handler = Arc::new(Mutex::new(
            AxonHandler::new(MemoryStorageAdapter::default()),
        ));

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
                })
                .unwrap();
        }

        let app = build_router_with_auth(
            handler,
            "memory",
            None,
            auth,
            crate::rate_limit::RateLimitConfig::default(),
            ActorScopeGuard::default(),
            None,
        )
        .layer(MockConnectInfo(peer));
        TestServer::new(app)
    }

    // ── Admin role: all operations succeed ────────────────────────────────

    #[tokio::test]
    async fn rbac_admin_can_create_entity() {
        let server = test_server_with_role(Role::Admin);
        let resp = server
            .post("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "hello"}}))
            .await;
        resp.assert_status(StatusCode::CREATED);
    }

    #[tokio::test]
    async fn rbac_admin_can_get_entity() {
        let server = seeded_server_with_role(Role::Admin).await;
        let resp = server.get("/entities/tasks/t-001").await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn rbac_admin_can_update_entity() {
        let server = seeded_server_with_role(Role::Admin).await;
        let resp = server
            .put("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "updated"}, "expected_version": 1}))
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn rbac_admin_can_delete_entity() {
        let server = seeded_server_with_role(Role::Admin).await;
        let resp = server.delete("/entities/tasks/t-001").await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn rbac_admin_can_create_collection() {
        let server = test_server_with_role(Role::Admin);
        let resp = server
            .post("/collections/new-col")
            .json(&json!({"schema": {}}))
            .await;
        resp.assert_status(StatusCode::CREATED);
    }

    #[tokio::test]
    async fn rbac_admin_can_drop_collection() {
        let server = seeded_server_with_role(Role::Admin).await;
        let resp = server.delete("/collections/tasks").await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn rbac_admin_can_put_schema() {
        let server = seeded_server_with_role(Role::Admin).await;
        let resp = server
            .put("/collections/tasks/schema")
            .json(&json!({
                "version": 2,
                "entity_schema": {"type": "object"}
            }))
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn rbac_admin_can_create_link() {
        let server = seeded_server_with_role(Role::Admin).await;
        let resp = server
            .post("/links")
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

    #[tokio::test]
    async fn rbac_admin_can_delete_link() {
        let server = seeded_server_with_role(Role::Admin).await;
        // Create a link first.
        server
            .post("/links")
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
            .delete("/links")
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

    #[tokio::test]
    async fn rbac_admin_can_put_template() {
        let server = seeded_server_with_role(Role::Admin).await;
        let resp = server
            .put("/collections/tasks/template")
            .json(&json!({"template": "# {{title}}"}))
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn rbac_admin_can_delete_template() {
        let server = seeded_server_with_role(Role::Admin).await;
        server
            .put("/collections/tasks/template")
            .json(&json!({"template": "# {{title}}"}))
            .await
            .assert_status_ok();
        let resp = server.delete("/collections/tasks/template").await;
        resp.assert_status_ok();
    }

    // ── Write role: write ops succeed, admin ops return 403 ──────────────

    #[tokio::test]
    async fn rbac_write_can_create_entity() {
        let server = test_server_with_role(Role::Write);
        let resp = server
            .post("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "hello"}}))
            .await;
        resp.assert_status(StatusCode::CREATED);
    }

    #[tokio::test]
    async fn rbac_write_can_get_entity() {
        let server = seeded_server_with_role(Role::Write).await;
        let resp = server.get("/entities/tasks/t-001").await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn rbac_write_can_update_entity() {
        let server = seeded_server_with_role(Role::Write).await;
        let resp = server
            .put("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "updated"}, "expected_version": 1}))
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn rbac_write_can_delete_entity() {
        let server = seeded_server_with_role(Role::Write).await;
        let resp = server.delete("/entities/tasks/t-001").await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn rbac_write_can_create_link() {
        let server = seeded_server_with_role(Role::Write).await;
        let resp = server
            .post("/links")
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

    #[tokio::test]
    async fn rbac_write_can_delete_link() {
        let server = seeded_server_with_role(Role::Write).await;
        server
            .post("/links")
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
            .delete("/links")
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

    #[tokio::test]
    async fn rbac_write_cannot_create_collection() {
        let server = test_server_with_role(Role::Write);
        let resp = server
            .post("/collections/new-col")
            .json(&json!({"schema": {}}))
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
        let body: Value = resp.json();
        assert_eq!(body["code"], "forbidden");
    }

    #[tokio::test]
    async fn rbac_write_cannot_drop_collection() {
        let server = seeded_server_with_role(Role::Write).await;
        let resp = server.delete("/collections/tasks").await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn rbac_write_cannot_put_schema() {
        let server = seeded_server_with_role(Role::Write).await;
        let resp = server
            .put("/collections/tasks/schema")
            .json(&json!({
                "version": 2,
                "entity_schema": {"type": "object"}
            }))
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn rbac_write_cannot_put_template() {
        let server = seeded_server_with_role(Role::Write).await;
        let resp = server
            .put("/collections/tasks/template")
            .json(&json!({"template": "# {{title}}"}))
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn rbac_write_cannot_delete_template() {
        let server = seeded_server_with_role(Role::Write).await;
        let resp = server.delete("/collections/tasks/template").await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    // ── Read role: only read ops succeed, write/admin ops return 403 ─────

    #[tokio::test]
    async fn rbac_read_can_get_entity() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server.get("/entities/tasks/t-001").await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn rbac_read_can_list_collections() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server.get("/collections").await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn rbac_read_can_describe_collection() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server.get("/collections/tasks").await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn rbac_read_can_get_schema() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server.get("/collections/tasks/schema").await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn rbac_read_can_traverse() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server.get("/traverse/tasks/t-001?link_type=blocks").await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn rbac_read_can_query_audit() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server.get("/audit/entity/tasks/t-001").await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn rbac_read_can_query_entities() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server
            .post("/collections/tasks/query")
            .json(&json!({}))
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn rbac_read_cannot_create_entity() {
        let server = test_server_with_role(Role::Read);
        let resp = server
            .post("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "hello"}}))
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
        let body: Value = resp.json();
        assert_eq!(body["code"], "forbidden");
    }

    #[tokio::test]
    async fn rbac_read_cannot_update_entity() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server
            .put("/entities/tasks/t-001")
            .json(&json!({"data": {"title": "updated"}, "expected_version": 1}))
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn rbac_read_cannot_delete_entity() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server.delete("/entities/tasks/t-001").await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn rbac_read_cannot_create_link() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server
            .post("/links")
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

    #[tokio::test]
    async fn rbac_read_cannot_delete_link() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server
            .delete("/links")
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

    #[tokio::test]
    async fn rbac_read_cannot_create_collection() {
        let server = test_server_with_role(Role::Read);
        let resp = server
            .post("/collections/new-col")
            .json(&json!({"schema": {}}))
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn rbac_read_cannot_drop_collection() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server.delete("/collections/tasks").await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn rbac_read_cannot_put_schema() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server
            .put("/collections/tasks/schema")
            .json(&json!({
                "version": 2,
                "entity_schema": {"type": "object"}
            }))
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn rbac_read_cannot_put_template() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server
            .put("/collections/tasks/template")
            .json(&json!({"template": "# {{title}}"}))
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn rbac_read_cannot_delete_template() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server.delete("/collections/tasks/template").await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn rbac_read_cannot_commit_transaction() {
        let server = seeded_server_with_role(Role::Read).await;
        let resp = server
            .post("/transactions")
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

    #[tokio::test]
    async fn rbac_forbidden_response_is_descriptive() {
        let server = test_server_with_role(Role::Read);
        let resp = server
            .post("/entities/tasks/t-001")
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

    #[tokio::test]
    async fn rbac_write_forbidden_for_admin_op_is_descriptive() {
        let server = test_server_with_role(Role::Write);
        let resp = server
            .post("/collections/new-col")
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

    #[tokio::test]
    async fn auth_me_returns_anonymous_admin_in_no_auth_mode() {
        let server = test_server();
        let resp = server.get("/auth/me").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["actor"], "anonymous");
        assert_eq!(body["role"], "admin");
    }

    #[tokio::test]
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

    #[tokio::test]
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
        assert_eq!(body["actor"], "erik-laptop");
        assert_eq!(body["role"], "admin");
    }

    #[tokio::test]
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
}
