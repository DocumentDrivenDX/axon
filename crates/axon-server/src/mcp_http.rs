//! HTTP + SSE transport for Axon's MCP server.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use crate::auth::Identity;
use crate::gateway::CurrentDatabase;
use axon_api::handler::AxonHandler;
use axon_core::id::{CollectionId, EntityId, Namespace, DEFAULT_SCHEMA};
use axon_core::types::Entity;
use axon_mcp::handlers::{
    build_aggregate_tool_tokio, build_crud_tools_tokio, build_link_candidates_tool_tokio,
    build_neighbors_tool_tokio, build_query_tool_tokio,
};
use axon_mcp::prompts::{get_prompt_from_handler, prompt_infos, PromptRegistry};
use axon_mcp::protocol::{McpError, McpServer};
use axon_mcp::resources::{
    discover_collections, read_resource_from_handler, resource_infos, resource_template_infos,
    ResourceRegistry,
};
use axon_mcp::tools::ToolRegistry;
use axon_storage::adapter::StorageAdapter;
use axum::body::Bytes;
use axum::extract::{Extension, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::StreamExt;

type SharedHandler<S> = Arc<Mutex<AxonHandler<S>>>;
const MCP_SESSION_HEADER: &str = "x-axon-mcp-session";

pub fn routes<S: StorageAdapter + 'static>() -> Router<SharedHandler<S>> {
    Router::new()
        .route("/mcp", post(handle_mcp::<S>))
        .route("/mcp/sse", get(handle_mcp_sse::<S>))
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct SessionKey {
    database: String,
    session_id: String,
}

struct McpHttpSession {
    server: StdMutex<McpServer>,
    listeners: StdMutex<Vec<mpsc::UnboundedSender<String>>>,
}

#[derive(Clone, Default)]
pub struct McpHttpSessions {
    sessions: Arc<StdMutex<HashMap<SessionKey, Arc<McpHttpSession>>>>,
}

#[derive(Debug, Default, Deserialize)]
struct McpSessionQuery {
    session: Option<String>,
}

impl McpHttpSessions {
    fn get_or_create<S: StorageAdapter + 'static>(
        &self,
        session_key: SessionKey,
        handler: SharedHandler<S>,
    ) -> Result<Arc<McpHttpSession>, McpError> {
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some(session) = sessions.get(&session_key) {
            return Ok(Arc::clone(session));
        }

        let server = build_mcp_server(handler, &session_key.database)?;
        let session = Arc::new(McpHttpSession {
            server: StdMutex::new(server),
            listeners: StdMutex::new(Vec::new()),
        });
        sessions.insert(session_key, Arc::clone(&session));
        Ok(session)
    }

    fn handle_message<S: StorageAdapter + 'static>(
        &self,
        session_key: SessionKey,
        handler: SharedHandler<S>,
        input: &str,
    ) -> Result<Option<String>, McpError> {
        let session = self.get_or_create(session_key.clone(), Arc::clone(&handler))?;
        let mut server = session
            .server
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        refresh_mcp_server(&mut server, handler, &session_key.database)?;
        Ok(server.handle_message(input))
    }

    fn connect<S: StorageAdapter + 'static>(
        &self,
        session_key: SessionKey,
        handler: SharedHandler<S>,
    ) -> Result<mpsc::UnboundedReceiver<String>, McpError> {
        let session = self.get_or_create(session_key, handler)?;
        let (sender, receiver) = mpsc::unbounded_channel();
        session
            .listeners
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(sender);
        Ok(receiver)
    }

    fn publish_entity_change(
        &self,
        current_database: &CurrentDatabase,
        collection: &CollectionId,
        entity_id: &EntityId,
    ) {
        let impacted_uris = impacted_entity_uris(current_database.as_str(), collection, entity_id);
        let sessions: Vec<Arc<McpHttpSession>> = self
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .filter(|(key, _)| key.database == current_database.as_str())
            .map(|(_, session)| Arc::clone(session))
            .collect();

        for session in sessions {
            let subscribed = session
                .server
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .subscribed_resource_uris();

            let matched_payloads: Vec<String> = subscribed
                .into_iter()
                .filter(|uri| impacted_uris.iter().any(|candidate| candidate == uri))
                .map(|uri| resource_updated_notification(&uri))
                .collect();

            if matched_payloads.is_empty() {
                continue;
            }

            let mut listeners = session
                .listeners
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            listeners.retain(|listener| {
                matched_payloads
                    .iter()
                    .all(|payload| listener.send(payload.clone()).is_ok())
            });
        }
    }

    #[cfg(test)]
    fn subscription_count(&self, session_key: &SessionKey) -> Option<usize> {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(session_key)
            .map(|session| {
                session
                    .server
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .subscription_count()
            })
    }

    #[cfg(test)]
    fn test_session_key(database: &str, session_id: &str) -> SessionKey {
        SessionKey {
            database: database.to_string(),
            session_id: session_id.to_string(),
        }
    }
}

fn collection_names_for_mcp<S: StorageAdapter>(
    handler: &SharedHandler<S>,
    current_database: &str,
    collections: &[String],
) -> Result<Vec<String>, McpError> {
    if !collections.is_empty() {
        return Ok(collections.to_vec());
    }

    let guard = handler.blocking_lock();
    discover_collections(&guard, current_database)
}

fn build_tool_registry<S: StorageAdapter + 'static>(
    handler: SharedHandler<S>,
    current_database: &str,
    collections: &[String],
) -> Result<ToolRegistry, McpError> {
    let mut registry = ToolRegistry::new();
    let collection_names = collection_names_for_mcp(&handler, current_database, collections)?;

    for collection in &collection_names {
        for tool in build_crud_tools_tokio(collection, Arc::clone(&handler)) {
            registry.register(tool);
        }
        registry.register(build_aggregate_tool_tokio(collection, Arc::clone(&handler)));
        registry.register(build_link_candidates_tool_tokio(
            collection,
            Arc::clone(&handler),
        ));
        registry.register(build_neighbors_tool_tokio(collection, Arc::clone(&handler)));
    }

    registry.register(build_query_tool_tokio(handler));
    Ok(registry)
}

fn build_resource_registry<S: StorageAdapter + 'static>(
    handler: SharedHandler<S>,
    current_database: &str,
    collections: &[String],
) -> Result<ResourceRegistry, McpError> {
    let collection_names = collection_names_for_mcp(&handler, current_database, collections)?;
    let current_database = current_database.to_string();
    Ok(ResourceRegistry::new(
        resource_infos(&collection_names),
        resource_template_infos(),
        Box::new(move |uri| {
            let guard = handler.blocking_lock();
            read_resource_from_handler(&guard, &current_database, uri)
        }),
    ))
}

fn build_prompt_registry<S: StorageAdapter + 'static>(
    handler: SharedHandler<S>,
    current_database: &str,
) -> PromptRegistry {
    let current_database = current_database.to_string();
    PromptRegistry::new(
        prompt_infos(),
        Box::new(move |name, arguments| {
            let guard = handler.blocking_lock();
            get_prompt_from_handler(&guard, &current_database, name, arguments)
        }),
    )
}

fn build_mcp_server<S: StorageAdapter + 'static>(
    handler: SharedHandler<S>,
    current_database: &str,
) -> Result<McpServer, McpError> {
    let tools = build_tool_registry(Arc::clone(&handler), current_database, &[])?;
    let resources = build_resource_registry(Arc::clone(&handler), current_database, &[])?;
    let prompts = build_prompt_registry(handler, current_database);
    Ok(McpServer::new(tools, resources, prompts))
}

fn refresh_mcp_server<S: StorageAdapter + 'static>(
    server: &mut McpServer,
    handler: SharedHandler<S>,
    current_database: &str,
) -> Result<(), McpError> {
    let tools = build_tool_registry(Arc::clone(&handler), current_database, &[])?;
    let resources = build_resource_registry(Arc::clone(&handler), current_database, &[])?;
    let prompts = build_prompt_registry(handler, current_database);
    server.refresh_registries(tools, resources, prompts);
    Ok(())
}

fn session_id_from_request(
    headers: &HeaderMap,
    query: &McpSessionQuery,
    identity: &Identity,
) -> String {
    query
        .session
        .clone()
        .or_else(|| {
            headers
                .get(MCP_SESSION_HEADER)
                .and_then(|value| value.to_str().ok())
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| identity.actor.clone())
}

fn session_key_from_request(
    current_database: &CurrentDatabase,
    headers: &HeaderMap,
    query: &McpSessionQuery,
    identity: &Identity,
) -> SessionKey {
    SessionKey {
        database: current_database.as_str().to_string(),
        session_id: session_id_from_request(headers, query, identity),
    }
}

fn visible_collection_name(collection: &CollectionId, current_database: &str) -> String {
    let collection_name = collection.as_str();
    let (namespace, local_collection) =
        Namespace::parse_with_database(collection_name, current_database);
    if namespace.database != current_database {
        return namespace.qualify(&local_collection);
    }
    if namespace.schema == DEFAULT_SCHEMA {
        return local_collection;
    }
    format!("{}.{}", namespace.schema, local_collection)
}

fn impacted_entity_uris(
    current_database: &str,
    collection: &CollectionId,
    entity_id: &EntityId,
) -> [String; 4] {
    let collection_name = collection.as_str();
    let (namespace, local_collection) =
        Namespace::parse_with_database(collection_name, current_database);
    let visible_collection = visible_collection_name(collection, current_database);
    let entity_id = entity_id.to_string();

    [
        format!("axon://{visible_collection}"),
        format!("axon://{visible_collection}/{entity_id}"),
        format!(
            "axon://{}/{}/{}",
            namespace.database, namespace.schema, local_collection
        ),
        format!(
            "axon://{}/{}/{}/{}",
            namespace.database, namespace.schema, local_collection, entity_id
        ),
    ]
}

fn resource_updated_notification(uri: &str) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "resource_updated",
        "params": {
            "uri": uri,
        }
    })
    .to_string()
}

pub fn notify_entity_change(
    sessions: &McpHttpSessions,
    current_database: &CurrentDatabase,
    entity: &Entity,
) {
    sessions.publish_entity_change(current_database, &entity.collection, &entity.id);
}

pub fn notify_entity_change_by_parts(
    sessions: &McpHttpSessions,
    current_database: &CurrentDatabase,
    collection: &str,
    entity_id: &str,
) {
    sessions.publish_entity_change(
        current_database,
        &CollectionId::new(Namespace::qualify_with_database(
            collection,
            current_database.as_str(),
        )),
        &EntityId::new(entity_id),
    );
}

fn json_rpc_response(status: StatusCode, payload: serde_json::Value) -> Response {
    (
        status,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        )],
        payload.to_string(),
    )
        .into_response()
}

fn json_rpc_error_response(error: McpError) -> Response {
    let code = match &error {
        McpError::Parse(_) => -32700,
        McpError::MethodNotFound(_) => -32601,
        McpError::InvalidParams(_) | McpError::NotFound(_) => -32602,
        McpError::Internal(_) => -32603,
    };
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": serde_json::Value::Null,
        "error": {
            "code": code,
            "message": error.to_string(),
        }
    });
    json_rpc_response(StatusCode::OK, payload)
}

async fn handle_mcp<S: StorageAdapter + 'static>(
    State(handler): State<SharedHandler<S>>,
    Extension(sessions): Extension<McpHttpSessions>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(identity): Extension<Identity>,
    Query(query): Query<McpSessionQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let input = match String::from_utf8(body.to_vec()) {
        Ok(input) => input,
        Err(error) => {
            return json_rpc_error_response(McpError::Parse(serde_json::Error::io(
                std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()),
            )));
        }
    };

    let session_key = session_key_from_request(&current_database, &headers, &query, &identity);
    let response = match tokio::task::spawn_blocking(move || {
        sessions.handle_message(session_key, handler, &input)
    })
    .await
    {
        Ok(result) => result,
        Err(error) => {
            return json_rpc_error_response(McpError::Internal(format!(
                "failed to join MCP worker: {error}"
            )));
        }
    };

    match response {
        Ok(Some(payload)) => (
            StatusCode::OK,
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            )],
            payload,
        )
            .into_response(),
        Ok(None) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => json_rpc_error_response(error),
    }
}

async fn handle_mcp_sse<S: StorageAdapter + 'static>(
    State(handler): State<SharedHandler<S>>,
    Extension(sessions): Extension<McpHttpSessions>,
    Extension(current_database): Extension<CurrentDatabase>,
    Extension(identity): Extension<Identity>,
    Query(query): Query<McpSessionQuery>,
    headers: HeaderMap,
) -> Response {
    let session_key = session_key_from_request(&current_database, &headers, &query, &identity);
    let receiver =
        match tokio::task::spawn_blocking(move || sessions.connect(session_key, handler)).await {
            Ok(Ok(receiver)) => receiver,
            Ok(Err(error)) => return json_rpc_error_response(error),
            Err(error) => {
                return json_rpc_error_response(McpError::Internal(format!(
                    "failed to join MCP SSE worker: {error}"
                )));
            }
        };

    let ready = tokio_stream::once(Ok(Event::default().event("ready").data("{}")));
    let updates = UnboundedReceiverStream::new(receiver)
        .map(|payload| Ok::<Event, Infallible>(Event::default().event("message").data(payload)));

    Sse::new(ready.chain(updates))
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keepalive"),
        )
        .into_response()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::gateway::build_router;
    use axon_api::handler::AxonHandler;
    use axon_core::id::DEFAULT_DATABASE;
    use axon_storage::MemoryStorageAdapter;
    use axum_test::TestServer;
    use serde_json::{json, Value};

    fn test_server() -> TestServer {
        let handler = Arc::new(Mutex::new(
            AxonHandler::new(MemoryStorageAdapter::default()),
        ));
        TestServer::new(build_router(handler, "memory", None))
    }

    #[tokio::test]
    async fn http_mcp_initializes_with_resources_and_prompts() {
        let server = test_server();
        let response = server
            .post("/mcp")
            .text(
                json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {}
                })
                .to_string(),
            )
            .await;
        response.assert_status_ok();
        let body: Value = response.json();
        assert!(body["result"]["capabilities"]["resources"]["subscribe"]
            .as_bool()
            .unwrap());
        assert!(body["result"]["capabilities"]["prompts"]["listChanged"].is_boolean());
    }

    #[tokio::test]
    async fn http_mcp_lists_and_reads_resources() {
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

        let list = server
            .post("/mcp")
            .text(
                json!({
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "resources/list"
                })
                .to_string(),
            )
            .await;
        list.assert_status_ok();
        let body: Value = list.json();
        let resources = body["result"]["resources"].as_array().unwrap();
        assert!(resources
            .iter()
            .any(|resource| resource["uri"] == "axon://tasks"));

        let read = server
            .post("/mcp")
            .text(
                json!({
                    "jsonrpc": "2.0",
                    "id": 3,
                    "method": "resources/read",
                    "params": {"uri": "axon://tasks/t-001"}
                })
                .to_string(),
            )
            .await;
        read.assert_status_ok();
        let body: Value = read.json();
        let text = body["result"]["contents"][0]["text"].as_str().unwrap();
        assert!(text.contains("hello"));
    }

    #[tokio::test]
    async fn http_mcp_lists_templates_and_prompts_and_reports_read_errors() {
        let server = test_server();
        server
            .post("/collections/tasks")
            .json(&json!({"schema": {}}))
            .await
            .assert_status(StatusCode::CREATED);

        let templates = server
            .post("/mcp")
            .text(
                json!({
                    "jsonrpc": "2.0",
                    "id": 4,
                    "method": "resources/templates/list"
                })
                .to_string(),
            )
            .await;
        templates.assert_status_ok();
        let templates_body: Value = templates.json();
        assert!(templates_body["result"]["resourceTemplates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|template| template["uriTemplate"] == "axon://{collection}/{id}"));

        let prompts = server
            .post("/mcp")
            .text(
                json!({
                    "jsonrpc": "2.0",
                    "id": 5,
                    "method": "prompts/list"
                })
                .to_string(),
            )
            .await;
        prompts.assert_status_ok();
        let prompts_body: Value = prompts.json();
        assert!(prompts_body["result"]["prompts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|prompt| prompt["name"] == "axon.schema_review"));

        let prompt = server
            .post("/mcp")
            .text(
                json!({
                    "jsonrpc": "2.0",
                    "id": 6,
                    "method": "prompts/get",
                    "params": {
                        "name": "axon.schema_review",
                        "arguments": {"collection": "tasks"}
                    }
                })
                .to_string(),
            )
            .await;
        prompt.assert_status_ok();
        let prompt_result: Value = prompt.json();
        assert!(prompt_result["result"]["messages"][0]["content"]["text"]
            .as_str()
            .unwrap()
            .contains("tasks"));

        let bad_read = server
            .post("/mcp")
            .text(
                json!({
                    "jsonrpc": "2.0",
                    "id": 7,
                    "method": "resources/read",
                    "params": {"uri": "axon://missing/nope"}
                })
                .to_string(),
            )
            .await;
        bad_read.assert_status_ok();
        let bad_read_body: Value = bad_read.json();
        assert_eq!(bad_read_body["error"]["code"], -32602);
    }

    #[tokio::test]
    async fn http_mcp_exposes_sse_endpoint() {
        let handler = Arc::new(Mutex::new(
            AxonHandler::new(MemoryStorageAdapter::default()),
        ));
        let response = handle_mcp_sse::<MemoryStorageAdapter>(
            State(handler),
            Extension(McpHttpSessions::default()),
            Extension(CurrentDatabase::new(DEFAULT_DATABASE)),
            Extension(Identity::anonymous_admin()),
            Query(McpSessionQuery::default()),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/event-stream"
        );
    }

    #[tokio::test]
    async fn http_mcp_subscriptions_persist_across_post_requests() {
        let handler = Arc::new(Mutex::new(
            AxonHandler::new(MemoryStorageAdapter::default()),
        ));
        let sessions = McpHttpSessions::default();
        let session_key = McpHttpSessions::test_session_key(DEFAULT_DATABASE, "anonymous");
        let subscribe = handle_mcp(
            State(handler.clone()),
            Extension(sessions.clone()),
            Extension(CurrentDatabase::new(DEFAULT_DATABASE)),
            Extension(Identity::anonymous_admin()),
            Query(McpSessionQuery::default()),
            HeaderMap::new(),
            Bytes::from(
                json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "resources/subscribe",
                    "params": { "uri": "axon://tasks/t-001" }
                })
                .to_string(),
            ),
        )
        .await;
        assert_eq!(subscribe.status(), StatusCode::OK);
        assert_eq!(sessions.subscription_count(&session_key), Some(1));

        let ping = handle_mcp(
            State(handler),
            Extension(sessions.clone()),
            Extension(CurrentDatabase::new(DEFAULT_DATABASE)),
            Extension(Identity::anonymous_admin()),
            Query(McpSessionQuery::default()),
            HeaderMap::new(),
            Bytes::from(
                json!({
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "ping"
                })
                .to_string(),
            ),
        )
        .await;
        assert_eq!(ping.status(), StatusCode::OK);
        assert_eq!(sessions.subscription_count(&session_key), Some(1));
    }

    #[tokio::test]
    async fn http_mcp_session_delivers_resource_updates_to_sse_listeners() {
        let handler = Arc::new(Mutex::new(
            AxonHandler::new(MemoryStorageAdapter::default()),
        ));
        let sessions = McpHttpSessions::default();
        let session_key = McpHttpSessions::test_session_key(DEFAULT_DATABASE, "anonymous");

        handle_mcp(
            State(handler.clone()),
            Extension(sessions.clone()),
            Extension(CurrentDatabase::new(DEFAULT_DATABASE)),
            Extension(Identity::anonymous_admin()),
            Query(McpSessionQuery::default()),
            HeaderMap::new(),
            Bytes::from(
                json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "resources/subscribe",
                    "params": { "uri": "axon://tasks/t-001" }
                })
                .to_string(),
            ),
        )
        .await;

        let mut receiver = sessions
            .connect(session_key, handler)
            .expect("session should accept an SSE listener");
        notify_entity_change_by_parts(
            &sessions,
            &CurrentDatabase::new(DEFAULT_DATABASE),
            "tasks",
            "t-001",
        );

        let payload = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
            .await
            .expect("notification should arrive in time")
            .expect("notification channel should remain open");
        let payload: Value =
            serde_json::from_str(&payload).expect("notification payload should be valid JSON");
        assert_eq!(payload["method"], "resource_updated");
        assert_eq!(payload["params"]["uri"], "axon://tasks/t-001");
    }
}
