//! MCP stdio transport for local agent connections.
//!
//! When `axon-server --mcp-stdio` is used, the server reads JSON-RPC 2.0
//! messages from stdin and writes responses to stdout. No authentication
//! is required for stdio connections.

use std::io::{self, BufRead, Write};
use std::sync::{Arc, Mutex};

use axon_api::handler::AxonHandler;
use axon_core::auth::CallerIdentity;
use axon_core::id::DEFAULT_DATABASE;
use axon_mcp::handlers::{
    apply_policy_metadata_to_registry, build_aggregate_tool, build_crud_tools,
    build_link_candidates_tool, build_neighbors_tool, build_query_tool,
    build_transition_lifecycle_tool,
};
use axon_mcp::prompts::{get_prompt_from_handler, prompt_infos, PromptRegistry};
use axon_mcp::protocol::McpServer;
use axon_mcp::resources::{
    discover_collections, read_resource_from_handler, resource_infos, resource_template_infos,
    ResourceRegistry,
};
use axon_mcp::tools::ToolRegistry;
use axon_storage::adapter::StorageAdapter;

fn collection_names_for_mcp<S: StorageAdapter>(
    handler: &Arc<Mutex<AxonHandler<S>>>,
    collections: &[String],
) -> io::Result<Vec<String>> {
    if !collections.is_empty() {
        return Ok(collections.to_vec());
    }

    let guard = handler
        .lock()
        .map_err(|e| io::Error::other(format!("failed to lock handler: {e}")))?;
    discover_collections(&guard, DEFAULT_DATABASE)
        .map_err(|e| io::Error::other(format!("failed to discover collections: {e}")))
}

fn build_registry<S: StorageAdapter + 'static>(
    handler: Arc<Mutex<AxonHandler<S>>>,
    collections: &[String],
) -> io::Result<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    let collection_names = collection_names_for_mcp(&handler, collections)?;

    for col in &collection_names {
        let tools = build_crud_tools(col, Arc::clone(&handler));
        for tool in tools {
            registry.register(tool);
        }
        registry.register(build_aggregate_tool(col, Arc::clone(&handler)));
        registry.register(build_link_candidates_tool(col, Arc::clone(&handler)));
        registry.register(build_neighbors_tool(col, Arc::clone(&handler)));
    }

    {
        let guard = handler
            .lock()
            .map_err(|e| io::Error::other(format!("failed to lock handler: {e}")))?;
        apply_policy_metadata_to_registry(
            &mut registry,
            &guard,
            DEFAULT_DATABASE,
            &collection_names,
            &CallerIdentity::anonymous(),
        )
        .map_err(|e| io::Error::other(format!("failed to build policy metadata: {e}")))?;
    }

    registry.register(build_query_tool(
        Arc::clone(&handler),
        CallerIdentity::anonymous(),
    ));
    registry.register(build_transition_lifecycle_tool(handler));
    Ok(registry)
}

fn build_resource_registry<S: StorageAdapter + 'static>(
    handler: Arc<Mutex<AxonHandler<S>>>,
    collections: &[String],
) -> io::Result<ResourceRegistry> {
    let collection_names = collection_names_for_mcp(&handler, collections)?;
    Ok(ResourceRegistry::new(
        resource_infos(&collection_names),
        resource_template_infos(),
        Box::new(move |uri| {
            let guard = handler.lock().map_err(|e| {
                axon_mcp::McpError::Internal(format!("failed to lock handler: {e}"))
            })?;
            read_resource_from_handler(&guard, DEFAULT_DATABASE, uri)
        }),
    ))
}

fn build_prompt_registry<S: StorageAdapter + 'static>(
    handler: Arc<Mutex<AxonHandler<S>>>,
) -> PromptRegistry {
    PromptRegistry::new(
        prompt_infos(),
        Box::new(move |name, arguments| {
            let guard = handler.lock().map_err(|e| {
                axon_mcp::McpError::Internal(format!("failed to lock handler: {e}"))
            })?;
            get_prompt_from_handler(&guard, DEFAULT_DATABASE, name, arguments)
        }),
    )
}

/// Run the MCP stdio loop: read lines from stdin, process, write to stdout.
///
/// This blocks the calling thread until stdin is closed or an I/O error occurs.
/// No authentication is applied for stdio connections.
pub fn run_mcp_stdio<S: StorageAdapter + 'static>(
    handler: Arc<Mutex<AxonHandler<S>>>,
    collections: &[String],
) -> io::Result<()> {
    let registry = build_registry(Arc::clone(&handler), collections)?;
    let resources = build_resource_registry(Arc::clone(&handler), collections)?;
    let prompts = build_prompt_registry(handler);
    let mut server = McpServer::new(registry, resources, prompts);

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        if let Some(response) = server.handle_message(&line) {
            writeln!(stdout, "{response}")?;
            stdout.flush()?;
        }
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use axon_api::handler::AxonHandler;
    use axon_api::request::CreateCollectionRequest;
    use axon_core::id::CollectionId;
    use axon_schema::schema::CollectionSchema;
    use axon_storage::memory::MemoryStorageAdapter;

    fn make_handler() -> Arc<Mutex<AxonHandler<MemoryStorageAdapter>>> {
        Arc::new(Mutex::new(
            AxonHandler::new(MemoryStorageAdapter::default()),
        ))
    }

    #[test]
    fn mcp_stdio_server_initializes() {
        let handler = make_handler();
        let registry = build_registry(Arc::clone(&handler), &[String::from("tasks")]).unwrap();
        let resources =
            build_resource_registry(Arc::clone(&handler), &[String::from("tasks")]).unwrap();
        let prompts = build_prompt_registry(handler);

        let mut server = McpServer::new(registry, resources, prompts);

        // Test initialize
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "test-agent", "version": "0.1" }
            }
        });
        let resp_str = server.handle_message(&req.to_string()).unwrap();
        let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();
        assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
    }

    #[test]
    fn mcp_stdio_lists_collection_tools_and_query() {
        let handler = make_handler();
        let registry = build_registry(Arc::clone(&handler), &[String::from("tasks")]).unwrap();
        let resources =
            build_resource_registry(Arc::clone(&handler), &[String::from("tasks")]).unwrap();
        let prompts = build_prompt_registry(handler);

        let mut server = McpServer::new(registry, resources, prompts);

        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        });
        let resp_str = server.handle_message(&req.to_string()).unwrap();
        let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();
        let tools = resp["result"]["tools"].as_array().unwrap();
        // 4 CRUD tools + aggregate + link_candidates + neighbors
        //   + axon.query + axon.transition_lifecycle
        assert_eq!(tools.len(), 9);

        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"tasks.create"));
        assert!(names.contains(&"tasks.get"));
        assert!(names.contains(&"tasks.patch"));
        assert!(names.contains(&"tasks.delete"));
        assert!(names.contains(&"tasks.aggregate"));
        assert!(names.contains(&"tasks.link_candidates"));
        assert!(names.contains(&"tasks.neighbors"));
        assert!(names.contains(&"axon.query"));
        assert!(names.contains(&"axon.transition_lifecycle"));
    }

    #[test]
    fn mcp_stdio_crud_roundtrip() {
        let handler = make_handler();
        let registry = build_registry(Arc::clone(&handler), &[String::from("items")]).unwrap();
        let resources =
            build_resource_registry(Arc::clone(&handler), &[String::from("items")]).unwrap();
        let prompts = build_prompt_registry(handler);

        let mut server = McpServer::new(registry, resources, prompts);

        // Create via tools/call
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "items.create",
                "arguments": {
                    "id": "i-001",
                    "data": {"name": "Widget"}
                }
            }
        });
        let resp_str = server.handle_message(&req.to_string()).unwrap();
        let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();
        assert!(resp["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Widget"));

        // Get
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "items.get",
                "arguments": { "id": "i-001" }
            }
        });
        let resp_str = server.handle_message(&req.to_string()).unwrap();
        let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();
        assert!(resp["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Widget"));
    }

    #[test]
    fn mcp_stdio_no_auth_for_stdio() {
        // Verify that no auth check happens for stdio connections.
        // Just verify we can create entities without any identity header.
        let handler = make_handler();
        let registry = build_registry(Arc::clone(&handler), &[String::from("noauth")]).unwrap();
        let resources =
            build_resource_registry(Arc::clone(&handler), &[String::from("noauth")]).unwrap();
        let prompts = build_prompt_registry(handler);

        let mut server = McpServer::new(registry, resources, prompts);
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "noauth.create",
                "arguments": {
                    "id": "x-001",
                    "data": {"val": 42}
                }
            }
        });
        let resp_str = server.handle_message(&req.to_string()).unwrap();
        let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();
        // Should succeed without auth.
        assert!(resp["error"].is_null());
        assert!(resp["result"]["content"].is_array());
    }

    #[test]
    fn mcp_stdio_discovers_existing_collections_when_none_are_provided() {
        let handler = make_handler();
        handler
            .lock()
            .unwrap()
            .create_collection(CreateCollectionRequest {
                name: CollectionId::new("tasks"),
                schema: CollectionSchema::new(CollectionId::new("tasks")),
                actor: Some("test".into()),
            })
            .unwrap();

        let registry = build_registry(Arc::clone(&handler), &[]).unwrap();
        let names: Vec<String> = registry
            .list_tools()
            .into_iter()
            .map(|tool| tool.name)
            .collect();
        assert!(names.iter().any(|name| name == "tasks.create"));
        assert!(names.iter().any(|name| name == "tasks.aggregate"));
        assert!(names.iter().any(|name| name == "axon.query"));
    }

    #[test]
    fn mcp_stdio_lists_resources_and_prompts() {
        let handler = make_handler();
        handler
            .lock()
            .unwrap()
            .create_collection(CreateCollectionRequest {
                name: CollectionId::new("tasks"),
                schema: CollectionSchema::new(CollectionId::new("tasks")),
                actor: Some("test".into()),
            })
            .unwrap();

        let registry = build_registry(Arc::clone(&handler), &[]).unwrap();
        let resources = build_resource_registry(Arc::clone(&handler), &[]).unwrap();
        let prompts = build_prompt_registry(handler);
        let mut server = McpServer::new(registry, resources, prompts);

        let resources_resp = server
            .handle_message(
                &serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "resources/list"
                })
                .to_string(),
            )
            .unwrap();
        let resources_json: serde_json::Value = serde_json::from_str(&resources_resp).unwrap();
        assert!(resources_json["result"]["resources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|resource| resource["uri"] == "axon://tasks"));

        let prompts_resp = server
            .handle_message(
                &serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "prompts/list"
                })
                .to_string(),
            )
            .unwrap();
        let prompts_json: serde_json::Value = serde_json::from_str(&prompts_resp).unwrap();
        assert!(prompts_json["result"]["prompts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|prompt| prompt["name"] == "axon.schema_review"));
    }
}
