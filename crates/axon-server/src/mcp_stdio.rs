//! MCP stdio transport for local agent connections.
//!
//! When `axon-server --mcp-stdio` is used, the server reads JSON-RPC 2.0
//! messages from stdin and writes responses to stdout. No authentication
//! is required for stdio connections.

use std::io::{self, BufRead, Write};
use std::sync::{Arc, Mutex};

use axon_api::handler::AxonHandler;
use axon_api::request::ListCollectionsRequest;
use axon_mcp::handlers::{
    build_aggregate_tool, build_crud_tools, build_link_candidates_tool, build_neighbors_tool,
    build_query_tool,
};
use axon_mcp::protocol::McpServer;
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
    let response = guard
        .list_collections(ListCollectionsRequest::default())
        .map_err(|e| io::Error::other(format!("failed to list collections: {e}")))?;
    Ok(response
        .collections
        .into_iter()
        .map(|collection| collection.name)
        .collect())
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

    registry.register(build_query_tool(handler));
    Ok(registry)
}

/// Run the MCP stdio loop: read lines from stdin, process, write to stdout.
///
/// This blocks the calling thread until stdin is closed or an I/O error occurs.
/// No authentication is applied for stdio connections.
pub fn run_mcp_stdio<S: StorageAdapter + 'static>(
    handler: Arc<Mutex<AxonHandler<S>>>,
    collections: &[String],
) -> io::Result<()> {
    let registry = build_registry(handler, collections)?;
    let mut server = McpServer::new(registry);

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
        let registry = build_registry(handler, &[String::from("tasks")]).unwrap();

        let mut server = McpServer::new(registry);

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
        let registry = build_registry(handler, &[String::from("tasks")]).unwrap();

        let mut server = McpServer::new(registry);

        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        });
        let resp_str = server.handle_message(&req.to_string()).unwrap();
        let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();
        let tools = resp["result"]["tools"].as_array().unwrap();
        // 4 CRUD tools + aggregate + link_candidates + neighbors + query
        assert_eq!(tools.len(), 8);

        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"tasks.create"));
        assert!(names.contains(&"tasks.get"));
        assert!(names.contains(&"tasks.patch"));
        assert!(names.contains(&"tasks.delete"));
        assert!(names.contains(&"tasks.aggregate"));
        assert!(names.contains(&"tasks.link_candidates"));
        assert!(names.contains(&"tasks.neighbors"));
        assert!(names.contains(&"axon.query"));
    }

    #[test]
    fn mcp_stdio_crud_roundtrip() {
        let handler = make_handler();
        let registry = build_registry(handler, &[String::from("items")]).unwrap();

        let mut server = McpServer::new(registry);

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
        let registry = build_registry(handler, &[String::from("noauth")]).unwrap();

        let mut server = McpServer::new(registry);
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
}
