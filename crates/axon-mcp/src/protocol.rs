//! MCP JSON-RPC 2.0 protocol implementation.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tools::ToolRegistry;

/// MCP protocol error.
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("JSON parse error: {0}")]
    Parse(#[from] serde_json::Error),

    #[error("method not found: {0}")]
    MethodNotFound(String),

    #[error("invalid params: {0}")]
    InvalidParams(String),

    #[error("internal error: {0}")]
    Internal(String),
}

/// JSON-RPC 2.0 request.
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

/// JSON-RPC 2.0 response.
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcResponse {
    fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: Option<Value>, code: i64, message: String) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message,
                data: None,
            }),
        }
    }
}

/// Server info returned in initialize response.
#[derive(Debug, Clone, Serialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

/// Capabilities declared by the server.
#[derive(Debug, Clone, Serialize)]
pub struct ServerCapabilities {
    pub tools: ToolsCapability,
}

/// Tool-related capabilities.
#[derive(Debug, Clone, Serialize)]
pub struct ToolsCapability {
    /// Whether the server supports tools/list_changed notifications.
    #[serde(rename = "listChanged")]
    pub list_changed: bool,
}

/// A resource subscription tracked by the MCP server.
#[derive(Debug, Clone)]
pub struct ResourceSubscription {
    /// The resource URI (e.g., "axon://collections/tasks/entities/t-001").
    pub uri: String,
    /// Subscription ID for tracking.
    pub id: u64,
}

/// The MCP server: processes JSON-RPC requests and returns responses.
pub struct McpServer {
    tool_registry: ToolRegistry,
    initialized: bool,
    /// Active resource subscriptions.
    subscriptions: Vec<ResourceSubscription>,
    next_sub_id: u64,
}

impl McpServer {
    /// Create a new MCP server with the given tool registry.
    pub fn new(tool_registry: ToolRegistry) -> Self {
        Self {
            tool_registry,
            initialized: false,
            subscriptions: Vec::new(),
            next_sub_id: 0,
        }
    }

    /// Process a single JSON-RPC request line and return a response.
    ///
    /// Returns `None` for notifications (no `id` field).
    pub fn handle_message(&mut self, input: &str) -> Option<String> {
        let request: JsonRpcRequest = match serde_json::from_str(input) {
            Ok(r) => r,
            Err(e) => {
                let resp = JsonRpcResponse::error(None, -32700, format!("Parse error: {e}"));
                return Some(serde_json::to_string(&resp).unwrap_or_default());
            }
        };

        // Notifications have no id — no response expected.
        request.id.as_ref()?;

        let response = self.dispatch(&request);
        Some(serde_json::to_string(&response).unwrap_or_default())
    }

    fn dispatch(&mut self, request: &JsonRpcRequest) -> JsonRpcResponse {
        match request.method.as_str() {
            "initialize" => self.handle_initialize(request),
            "tools/list" => self.handle_tools_list(request),
            "tools/call" => self.handle_tools_call(request),
            "resources/subscribe" => self.handle_resource_subscribe(request),
            "resources/unsubscribe" => self.handle_resource_unsubscribe(request),
            "ping" => JsonRpcResponse::success(request.id.clone(), serde_json::json!({})),
            _ => JsonRpcResponse::error(
                request.id.clone(),
                -32601,
                format!("Method not found: {}", request.method),
            ),
        }
    }

    fn handle_initialize(&mut self, request: &JsonRpcRequest) -> JsonRpcResponse {
        self.initialized = true;
        let result = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {
                    "listChanged": true
                },
                "resources": {
                    "subscribe": true
                }
            },
            "serverInfo": {
                "name": "axon-mcp",
                "version": env!("CARGO_PKG_VERSION")
            }
        });
        JsonRpcResponse::success(request.id.clone(), result)
    }

    fn handle_tools_list(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        let tools = self.tool_registry.list_tools();
        let result = serde_json::json!({ "tools": tools });
        JsonRpcResponse::success(request.id.clone(), result)
    }

    fn handle_tools_call(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        let params = match &request.params {
            Some(p) => p,
            None => {
                return JsonRpcResponse::error(
                    request.id.clone(),
                    -32602,
                    "Missing params".into(),
                );
            }
        };

        let tool_name = match params.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => {
                return JsonRpcResponse::error(
                    request.id.clone(),
                    -32602,
                    "Missing 'name' in params".into(),
                );
            }
        };

        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));

        match self.tool_registry.call_tool(tool_name, &arguments) {
            Ok(result) => JsonRpcResponse::success(request.id.clone(), result),
            Err(e) => {
                let result = serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": e.to_string()
                    }],
                    "isError": true
                });
                JsonRpcResponse::success(request.id.clone(), result)
            }
        }
    }

    fn handle_resource_subscribe(&mut self, request: &JsonRpcRequest) -> JsonRpcResponse {
        let params = match &request.params {
            Some(p) => p,
            None => {
                return JsonRpcResponse::error(
                    request.id.clone(),
                    -32602,
                    "Missing params".into(),
                );
            }
        };

        let uri = match params.get("uri").and_then(|v| v.as_str()) {
            Some(u) => u.to_string(),
            None => {
                return JsonRpcResponse::error(
                    request.id.clone(),
                    -32602,
                    "Missing 'uri' in params".into(),
                );
            }
        };

        self.next_sub_id += 1;
        let sub = ResourceSubscription {
            uri: uri.clone(),
            id: self.next_sub_id,
        };
        self.subscriptions.push(sub);

        JsonRpcResponse::success(
            request.id.clone(),
            serde_json::json!({ "subscriptionId": self.next_sub_id }),
        )
    }

    fn handle_resource_unsubscribe(&mut self, request: &JsonRpcRequest) -> JsonRpcResponse {
        let params = match &request.params {
            Some(p) => p,
            None => {
                return JsonRpcResponse::error(
                    request.id.clone(),
                    -32602,
                    "Missing params".into(),
                );
            }
        };

        let uri = match params.get("uri").and_then(|v| v.as_str()) {
            Some(u) => u,
            None => {
                return JsonRpcResponse::error(
                    request.id.clone(),
                    -32602,
                    "Missing 'uri' in params".into(),
                );
            }
        };

        self.subscriptions.retain(|s| s.uri != uri);
        JsonRpcResponse::success(request.id.clone(), serde_json::json!({}))
    }

    /// Get active subscription count.
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    /// Update the tool registry (e.g., after schema changes).
    pub fn update_registry(&mut self, registry: ToolRegistry) {
        self.tool_registry = registry;
    }

    /// Check if the server has been initialized via the handshake.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolDef;

    fn empty_server() -> McpServer {
        McpServer::new(ToolRegistry::new())
    }

    #[test]
    fn initialize_returns_capabilities() {
        let mut server = empty_server();
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "0.1" }
            }
        });
        let resp_str = server.handle_message(&req.to_string()).unwrap();
        let resp: Value = serde_json::from_str(&resp_str).unwrap();

        assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(resp["result"]["serverInfo"]["name"], "axon-mcp");
        assert!(resp["result"]["capabilities"]["tools"]["listChanged"].as_bool().unwrap());
        assert!(server.is_initialized());
    }

    #[test]
    fn tools_list_returns_registered_tools() {
        let mut registry = ToolRegistry::new();
        registry.register(ToolDef {
            name: "axon.test".into(),
            description: "A test tool".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            handler: Box::new(|_args| Ok(serde_json::json!({
                "content": [{"type": "text", "text": "ok"}]
            }))),
        });

        let mut server = McpServer::new(registry);
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        });
        let resp_str = server.handle_message(&req.to_string()).unwrap();
        let resp: Value = serde_json::from_str(&resp_str).unwrap();

        let tools = resp["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "axon.test");
        assert_eq!(tools[0]["description"], "A test tool");
    }

    #[test]
    fn tools_call_dispatches_to_handler() {
        let mut registry = ToolRegistry::new();
        registry.register(ToolDef {
            name: "axon.echo".into(),
            description: "Echo tool".into(),
            input_schema: serde_json::json!({"type": "object"}),
            handler: Box::new(|args| {
                let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("none");
                Ok(serde_json::json!({
                    "content": [{"type": "text", "text": text}]
                }))
            }),
        });

        let mut server = McpServer::new(registry);
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "axon.echo",
                "arguments": { "text": "hello" }
            }
        });
        let resp_str = server.handle_message(&req.to_string()).unwrap();
        let resp: Value = serde_json::from_str(&resp_str).unwrap();

        assert_eq!(resp["result"]["content"][0]["text"], "hello");
    }

    #[test]
    fn tools_call_unknown_tool_returns_error_content() {
        let mut server = empty_server();
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": { "name": "nonexistent" }
        });
        let resp_str = server.handle_message(&req.to_string()).unwrap();
        let resp: Value = serde_json::from_str(&resp_str).unwrap();

        assert!(resp["result"]["isError"].as_bool().unwrap());
    }

    #[test]
    fn notification_returns_none() {
        let mut server = empty_server();
        let notif = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        let result = server.handle_message(&notif.to_string());
        assert!(result.is_none());
    }

    #[test]
    fn unknown_method_returns_error() {
        let mut server = empty_server();
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "bogus/method"
        });
        let resp_str = server.handle_message(&req.to_string()).unwrap();
        let resp: Value = serde_json::from_str(&resp_str).unwrap();

        assert_eq!(resp["error"]["code"], -32601);
    }

    #[test]
    fn ping_returns_empty_object() {
        let mut server = empty_server();
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "ping"
        });
        let resp_str = server.handle_message(&req.to_string()).unwrap();
        let resp: Value = serde_json::from_str(&resp_str).unwrap();
        assert_eq!(resp["result"], serde_json::json!({}));
    }

    #[test]
    fn invalid_json_returns_parse_error() {
        let mut server = empty_server();
        let resp_str = server.handle_message("not json").unwrap();
        let resp: Value = serde_json::from_str(&resp_str).unwrap();
        assert_eq!(resp["error"]["code"], -32700);
    }

    // ── Resource subscription tests (US-055) ───────────────────────────

    #[test]
    fn initialize_declares_resource_subscribe_capability() {
        let mut server = empty_server();
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        });
        let resp_str = server.handle_message(&req.to_string()).unwrap();
        let resp: Value = serde_json::from_str(&resp_str).unwrap();
        assert!(resp["result"]["capabilities"]["resources"]["subscribe"].as_bool().unwrap());
    }

    #[test]
    fn resource_subscribe_returns_subscription_id() {
        let mut server = empty_server();
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "resources/subscribe",
            "params": { "uri": "axon://collections/tasks/entities/t-001" }
        });
        let resp_str = server.handle_message(&req.to_string()).unwrap();
        let resp: Value = serde_json::from_str(&resp_str).unwrap();
        assert!(resp["result"]["subscriptionId"].as_u64().unwrap() > 0);
        assert_eq!(server.subscription_count(), 1);
    }

    #[test]
    fn resource_unsubscribe_removes_subscription() {
        let mut server = empty_server();
        let uri = "axon://collections/tasks/entities/t-001";

        // Subscribe
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "resources/subscribe",
            "params": { "uri": uri }
        });
        server.handle_message(&req.to_string());
        assert_eq!(server.subscription_count(), 1);

        // Unsubscribe
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "resources/unsubscribe",
            "params": { "uri": uri }
        });
        server.handle_message(&req.to_string());
        assert_eq!(server.subscription_count(), 0);
    }

    #[test]
    fn multiple_subscriptions_tracked() {
        let mut server = empty_server();
        for i in 1..=3 {
            let req = serde_json::json!({
                "jsonrpc": "2.0",
                "id": i,
                "method": "resources/subscribe",
                "params": { "uri": format!("axon://collections/tasks/entities/t-{i:03}") }
            });
            server.handle_message(&req.to_string());
        }
        assert_eq!(server.subscription_count(), 3);
    }
}
