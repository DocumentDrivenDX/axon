//! HTTP + SSE transport for Axon's MCP server.

use std::convert::Infallible;
use std::sync::Arc;

use crate::gateway::CurrentDatabase;
use axon_api::handler::AxonHandler;
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
use axum::extract::{Extension, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use tokio::sync::Mutex;

type SharedHandler<S> = Arc<Mutex<AxonHandler<S>>>;

pub fn routes<S: StorageAdapter + 'static>() -> Router<SharedHandler<S>> {
    Router::new()
        .route("/mcp", post(handle_mcp::<S>))
        .route("/mcp/sse", get(handle_mcp_sse))
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
    Extension(current_database): Extension<CurrentDatabase>,
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

    let current_database = current_database.as_str().to_string();
    let response = match tokio::task::spawn_blocking(move || {
        let mut server = build_mcp_server(handler, &current_database)?;
        Ok::<Option<String>, McpError>(server.handle_message(&input))
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

async fn handle_mcp_sse() -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let stream = tokio_stream::once(Ok(Event::default().event("ready").data("{}")));
    Sse::new(stream)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::gateway::build_router;
    use axon_api::handler::AxonHandler;
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
        let server = test_server();
        let response = server.get("/mcp/sse").await;
        response.assert_status_ok();
        response.assert_header("content-type", "text/event-stream");
    }
}
