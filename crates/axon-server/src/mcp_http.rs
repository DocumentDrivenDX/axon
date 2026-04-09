//! HTTP + SSE transport for Axon's MCP server.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

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
use uuid::Uuid;

type SharedHandler<S> = Arc<Mutex<AxonHandler<S>>>;
const MCP_SESSION_HEADER: &str = "x-axon-mcp-session";
const MCP_SESSION_COOKIE: &str = "axon_mcp_session";

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

#[derive(Clone, Debug, Eq, PartialEq)]
struct ResolvedSession {
    key: SessionKey,
    issued_session_id: Option<String>,
}

struct McpHttpSession {
    server: StdMutex<McpServer>,
    listeners: StdMutex<SessionListeners>,
}

#[derive(Default)]
struct SessionListeners {
    senders: Vec<mpsc::UnboundedSender<String>>,
    had_listener: bool,
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
    fn new_session<S: StorageAdapter + 'static>(
        session_key: &SessionKey,
        handler: SharedHandler<S>,
    ) -> Result<Arc<McpHttpSession>, McpError> {
        let server = build_mcp_server(handler, &session_key.database)?;
        Ok(Arc::new(McpHttpSession {
            server: StdMutex::new(server),
            listeners: StdMutex::new(SessionListeners::default()),
        }))
    }

    fn get_or_create<S: StorageAdapter + 'static>(
        &self,
        session_key: SessionKey,
        handler: SharedHandler<S>,
    ) -> Result<Arc<McpHttpSession>, McpError> {
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some(session) = sessions.get(&session_key).cloned() {
            if !Self::session_is_stale(&session) {
                return Ok(session);
            }
            sessions.remove(&session_key);
        }

        let session = Self::new_session(&session_key, handler)?;
        sessions.insert(session_key, Arc::clone(&session));
        Ok(session)
    }

    fn connect_session<S: StorageAdapter + 'static>(
        &self,
        session_key: SessionKey,
        handler: SharedHandler<S>,
    ) -> Result<Arc<McpHttpSession>, McpError> {
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some(session) = sessions.get(&session_key).cloned() {
            if Self::session_needs_sse_reset(&session) {
                sessions.remove(&session_key);
            } else {
                return Ok(session);
            }
        }

        let session = Self::new_session(&session_key, handler)?;
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
        let session = self.connect_session(session_key.clone(), handler)?;
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut listeners = session
            .listeners
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        listeners.had_listener = true;
        listeners.senders.push(sender.clone());

        let sessions = self.clone();
        tokio::runtime::Handle::current().spawn(async move {
            sender.closed().await;
            sessions.disconnect_listener(&session_key, &sender);
        });

        Ok(receiver)
    }

    fn disconnect_listener(
        &self,
        session_key: &SessionKey,
        disconnected: &mpsc::UnboundedSender<String>,
    ) {
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(session) = sessions.get(session_key).cloned() else {
            return;
        };

        let should_remove = {
            let mut listeners = session
                .listeners
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            listeners
                .senders
                .retain(|listener| !listener.same_channel(disconnected));
            listeners.had_listener && listeners.senders.is_empty()
        };

        if should_remove {
            sessions.remove(session_key);
        }
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
            listeners.senders.retain(|listener| {
                matched_payloads
                    .iter()
                    .all(|payload| listener.send(payload.clone()).is_ok())
            });
        }
    }

    fn session_is_stale(session: &McpHttpSession) -> bool {
        let mut listeners = session
            .listeners
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        listeners.senders.retain(|listener| !listener.is_closed());
        listeners.had_listener && listeners.senders.is_empty()
    }

    fn session_needs_sse_reset(session: &McpHttpSession) -> bool {
        let mut listeners = session
            .listeners
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        listeners.senders.retain(|listener| !listener.is_closed());
        listeners.had_listener
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

fn requested_session_id(headers: &HeaderMap, query: &McpSessionQuery) -> Option<String> {
    query
        .session
        .as_deref()
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            headers
                .get(MCP_SESSION_HEADER)
                .and_then(|value| value.to_str().ok())
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            headers
                .get_all(header::COOKIE)
                .iter()
                .filter_map(|value| value.to_str().ok())
                .flat_map(|value| value.split(';'))
                .map(str::trim)
                .filter_map(|cookie| cookie.split_once('='))
                .find_map(|(name, value)| {
                    (name == MCP_SESSION_COOKIE && !value.is_empty()).then(|| value.to_string())
                })
        })
}

fn resolve_session_from_request(
    current_database: &CurrentDatabase,
    headers: &HeaderMap,
    query: &McpSessionQuery,
) -> ResolvedSession {
    let (session_id, issued_session_id) =
        if let Some(session_id) = requested_session_id(headers, query) {
            (session_id, None)
        } else {
            let session_id = Uuid::now_v7().to_string();
            (session_id.clone(), Some(session_id))
        };

    ResolvedSession {
        key: SessionKey {
            database: current_database.as_str().to_string(),
            session_id,
        },
        issued_session_id,
    }
}

fn apply_session_metadata(response: &mut Response, session: &ResolvedSession) {
    let Some(session_id) = session.issued_session_id.as_deref() else {
        return;
    };

    if let Ok(header_value) = HeaderValue::from_str(session_id) {
        response
            .headers_mut()
            .insert(MCP_SESSION_HEADER, header_value);
    }

    let cookie = format!("{MCP_SESSION_COOKIE}={session_id}; Path=/; HttpOnly; SameSite=Lax");
    if let Ok(cookie_value) = HeaderValue::from_str(&cookie) {
        response
            .headers_mut()
            .append(header::SET_COOKIE, cookie_value);
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

    let session = resolve_session_from_request(&current_database, &headers, &query);
    let session_key = session.key.clone();
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

    let mut response = match response {
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
    };
    apply_session_metadata(&mut response, &session);
    response
}

async fn handle_mcp_sse<S: StorageAdapter + 'static>(
    State(handler): State<SharedHandler<S>>,
    Extension(sessions): Extension<McpHttpSessions>,
    Extension(current_database): Extension<CurrentDatabase>,
    Query(query): Query<McpSessionQuery>,
    headers: HeaderMap,
) -> Response {
    let session = resolve_session_from_request(&current_database, &headers, &query);
    let session_key = session.key.clone();
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

    let mut response = Sse::new(ready.chain(updates))
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keepalive"),
        )
        .into_response();
    apply_session_metadata(&mut response, &session);
    response
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::auth::Identity;
    use crate::gateway::build_router;
    use axon_api::handler::AxonHandler;
    use axon_core::id::DEFAULT_DATABASE;
    use axon_storage::MemoryStorageAdapter;
    use axum::Router;
    use axum_test::TestServer;
    use reqwest::Response as ReqwestResponse;
    use serde_json::{json, Value};

    struct SseEventFrame {
        event: Option<String>,
        data: String,
    }

    struct LiveSseConnection {
        response: ReqwestResponse,
        buffer: Vec<u8>,
    }

    fn test_server() -> TestServer {
        let handler = Arc::new(Mutex::new(
            AxonHandler::new(MemoryStorageAdapter::default()),
        ));
        TestServer::new(build_router(handler, "memory", None))
    }

    fn issued_session_id(response: &Response) -> String {
        response
            .headers()
            .get(MCP_SESSION_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
            .expect("response should expose an MCP session id")
    }

    fn session_cookie_headers(session_id: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        let cookie = format!("{MCP_SESSION_COOKIE}={session_id}");
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&cookie).expect("cookie header should be valid"),
        );
        headers
    }

    fn test_mcp_transport_server(sessions: McpHttpSessions) -> TestServer {
        let handler = Arc::new(Mutex::new(
            AxonHandler::new(MemoryStorageAdapter::default()),
        ));
        let app = Router::new()
            .merge(routes::<MemoryStorageAdapter>())
            .with_state(handler)
            .layer(Extension(sessions))
            .layer(Extension(CurrentDatabase::new(DEFAULT_DATABASE)))
            .layer(Extension(Identity::anonymous_admin()));
        TestServer::builder().http_transport().build(app)
    }

    async fn connect_sse(server: &TestServer, path: &str) -> LiveSseConnection {
        let url = server
            .server_url(path)
            .expect("test server should expose an HTTP transport URL");
        let response = reqwest::Client::new()
            .get(url)
            .header(header::ACCEPT, "text/event-stream")
            .send()
            .await
            .expect("SSE client should connect")
            .error_for_status()
            .expect("SSE request should succeed");
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        assert!(
            content_type.starts_with("text/event-stream"),
            "SSE response should declare event-stream"
        );

        let mut connection = LiveSseConnection {
            response,
            buffer: Vec::new(),
        };
        let ready = read_sse_event(&mut connection).await;
        assert_eq!(ready.event.as_deref(), Some("ready"));
        assert_eq!(ready.data, "{}");
        connection
    }

    fn find_sse_frame_end(buffer: &[u8]) -> Option<(usize, usize)> {
        buffer
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|index| (index, 4))
            .or_else(|| {
                buffer
                    .windows(2)
                    .position(|window| window == b"\n\n")
                    .map(|index| (index, 2))
            })
    }

    async fn read_sse_event(connection: &mut LiveSseConnection) -> SseEventFrame {
        loop {
            if let Some((frame_end, delimiter_len)) = find_sse_frame_end(&connection.buffer) {
                let frame_bytes = connection
                    .buffer
                    .drain(..frame_end + delimiter_len)
                    .collect::<Vec<_>>();
                let frame = String::from_utf8(frame_bytes[..frame_end].to_vec())
                    .expect("SSE frame should be valid UTF-8")
                    .replace("\r\n", "\n");
                let mut event = None;
                let mut data_lines = Vec::new();
                for line in frame.lines() {
                    if let Some(value) = line.strip_prefix("event: ") {
                        event = Some(value.to_string());
                        continue;
                    }
                    if let Some(value) = line.strip_prefix("data: ") {
                        data_lines.push(value.to_string());
                    }
                }
                return SseEventFrame {
                    event,
                    data: data_lines.join("\n"),
                };
            }

            let chunk = connection
                .response
                .chunk()
                .await
                .expect("SSE chunk should be readable")
                .expect("SSE stream ended before the next frame");
            connection.buffer.extend_from_slice(&chunk);
        }
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
            Query(McpSessionQuery::default()),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/event-stream"
        );
        assert!(response.headers().contains_key(MCP_SESSION_HEADER));
        assert!(response.headers().contains_key(header::SET_COOKIE));
    }

    #[tokio::test]
    async fn http_mcp_subscriptions_persist_across_post_requests() {
        let handler = Arc::new(Mutex::new(
            AxonHandler::new(MemoryStorageAdapter::default()),
        ));
        let sessions = McpHttpSessions::default();
        let subscribe = handle_mcp(
            State(handler.clone()),
            Extension(sessions.clone()),
            Extension(CurrentDatabase::new(DEFAULT_DATABASE)),
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
        let session_id = issued_session_id(&subscribe);
        let session_key = McpHttpSessions::test_session_key(DEFAULT_DATABASE, &session_id);
        assert_eq!(sessions.subscription_count(&session_key), Some(1));

        let ping = handle_mcp(
            State(handler),
            Extension(sessions.clone()),
            Extension(CurrentDatabase::new(DEFAULT_DATABASE)),
            Query(McpSessionQuery::default()),
            session_cookie_headers(&session_id),
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

        let subscribe = handle_mcp(
            State(handler.clone()),
            Extension(sessions.clone()),
            Extension(CurrentDatabase::new(DEFAULT_DATABASE)),
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
        let session_key =
            McpHttpSessions::test_session_key(DEFAULT_DATABASE, &issued_session_id(&subscribe));

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

    #[tokio::test]
    async fn http_mcp_issued_sessions_isolate_subscriptions_for_same_actor() {
        let handler = Arc::new(Mutex::new(
            AxonHandler::new(MemoryStorageAdapter::default()),
        ));
        let sessions = McpHttpSessions::default();
        let current_database = CurrentDatabase::new(DEFAULT_DATABASE);

        let first_session = resolve_session_from_request(
            &current_database,
            &HeaderMap::new(),
            &McpSessionQuery::default(),
        );
        let second_session = resolve_session_from_request(
            &current_database,
            &HeaderMap::new(),
            &McpSessionQuery::default(),
        );
        assert_ne!(first_session.key.session_id, second_session.key.session_id);

        let mut first_receiver = tokio::task::spawn_blocking({
            let sessions = sessions.clone();
            let handler = handler.clone();
            let session_key = first_session.key.clone();
            move || {
                sessions
                    .connect(session_key, handler)
                    .expect("first session should accept an SSE listener")
            }
        })
        .await
        .expect("first session worker should not panic");
        let mut second_receiver = tokio::task::spawn_blocking({
            let sessions = sessions.clone();
            let handler = handler.clone();
            let session_key = second_session.key.clone();
            move || {
                sessions
                    .connect(session_key, handler)
                    .expect("second session should accept an SSE listener")
            }
        })
        .await
        .expect("second session worker should not panic");

        let subscribe = handle_mcp(
            State(handler.clone()),
            Extension(sessions.clone()),
            Extension(CurrentDatabase::new(DEFAULT_DATABASE)),
            Query(McpSessionQuery::default()),
            session_cookie_headers(&first_session.key.session_id),
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

        notify_entity_change_by_parts(
            &sessions,
            &CurrentDatabase::new(DEFAULT_DATABASE),
            "tasks",
            "t-001",
        );

        let payload = tokio::time::timeout(Duration::from_secs(1), first_receiver.recv())
            .await
            .expect("first session should receive the notification in time")
            .expect("first session listener should remain open");
        let payload: Value =
            serde_json::from_str(&payload).expect("notification payload should be valid JSON");
        assert_eq!(payload["method"], "resource_updated");
        assert_eq!(payload["params"]["uri"], "axon://tasks/t-001");
        assert!(
            tokio::time::timeout(Duration::from_millis(100), second_receiver.recv())
                .await
                .is_err(),
            "independent anonymous sessions must not receive each other's updates"
        );
    }

    #[tokio::test]
    async fn http_mcp_immediate_reconnect_does_not_inherit_subscriptions() {
        let sessions = McpHttpSessions::default();
        let session_id = "transport-reconnect";
        let session_key = McpHttpSessions::test_session_key(DEFAULT_DATABASE, session_id);
        let server = test_mcp_transport_server(sessions.clone());

        let first_listener = connect_sse(&server, &format!("/mcp/sse?session={session_id}")).await;

        let subscribe = server
            .post(&format!("/mcp?session={session_id}"))
            .text(
                json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "resources/subscribe",
                    "params": { "uri": "axon://tasks/t-001" }
                })
                .to_string(),
            )
            .await;
        subscribe.assert_status_ok();
        assert_eq!(sessions.subscription_count(&session_key), Some(1));

        drop(first_listener);

        let mut second_listener =
            connect_sse(&server, &format!("/mcp/sse?session={session_id}")).await;
        assert_eq!(sessions.subscription_count(&session_key), Some(0));
        notify_entity_change_by_parts(
            &sessions,
            &CurrentDatabase::new(DEFAULT_DATABASE),
            "tasks",
            "t-001",
        );

        assert!(
            tokio::time::timeout(
                Duration::from_millis(100),
                read_sse_event(&mut second_listener)
            )
            .await
            .is_err(),
            "reconnected listener should not inherit prior subscriptions"
        );
    }
}
