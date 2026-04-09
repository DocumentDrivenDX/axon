//! MCP stdio transport for local agent connections.
//!
//! When `axon-server --mcp-stdio` is used, the server reads JSON-RPC 2.0
//! messages from stdin and writes responses to stdout. No authentication
//! is required for stdio connections.

use std::io::{self, BufRead, Write};
use std::sync::{Arc, Mutex};

use axon_api::handler::AxonHandler;
use axon_mcp::handlers::{build_crud_tools, build_query_tool};
use axon_mcp::protocol::McpServer;
use axon_mcp::tools::ToolRegistry;
use axon_storage::adapter::StorageAdapter;

/// Run the MCP stdio loop: read lines from stdin, process, write to stdout.
///
/// This blocks the calling thread until stdin is closed or an I/O error occurs.
/// No authentication is applied for stdio connections.
pub fn run_mcp_stdio<S: StorageAdapter + 'static>(
    handler: Arc<Mutex<AxonHandler<S>>>,
    collections: &[String],
) -> io::Result<()> {
    let mut registry = ToolRegistry::new();

    // Register CRUD tools for each known collection.
    for col in collections {
        let tools = build_crud_tools(col, Arc::clone(&handler));
        for tool in tools {
            registry.register(tool);
        }
    }

    // Register the GraphQL query tool.
    registry.register(build_query_tool());

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
    use axon_storage::memory::MemoryStorageAdapter;

    fn make_handler() -> Arc<Mutex<AxonHandler<MemoryStorageAdapter>>> {
        Arc::new(Mutex::new(
            AxonHandler::new(MemoryStorageAdapter::default()),
        ))
    }

    #[test]
    fn mcp_stdio_server_initializes() {
        let handler = make_handler();
        let mut registry = ToolRegistry::new();
        let tools = build_crud_tools("tasks", Arc::clone(&handler));
        for tool in tools {
            registry.register(tool);
        }
        registry.register(build_query_tool());

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
        let mut registry = ToolRegistry::new();
        let tools = build_crud_tools("tasks", Arc::clone(&handler));
        for tool in tools {
            registry.register(tool);
        }
        registry.register(build_query_tool());

        let mut server = McpServer::new(registry);

        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        });
        let resp_str = server.handle_message(&req.to_string()).unwrap();
        let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();
        let tools = resp["result"]["tools"].as_array().unwrap();
        // 4 CRUD tools + 1 query tool
        assert_eq!(tools.len(), 5);

        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"tasks.create"));
        assert!(names.contains(&"tasks.get"));
        assert!(names.contains(&"tasks.patch"));
        assert!(names.contains(&"tasks.delete"));
        assert!(names.contains(&"axon.query"));
    }

    #[test]
    fn mcp_stdio_crud_roundtrip() {
        let handler = make_handler();
        let mut registry = ToolRegistry::new();
        let tools = build_crud_tools("items", Arc::clone(&handler));
        for tool in tools {
            registry.register(tool);
        }

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
        let mut registry = ToolRegistry::new();
        let tools = build_crud_tools("noauth", Arc::clone(&handler));
        for tool in tools {
            registry.register(tool);
        }

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
}
