//! MCP tool registry: maps collection schemas to typed tool definitions.
//!
//! Each registered Axon collection produces tools like:
//! - `beads.create` — create an entity
//! - `beads.get` — get an entity by ID
//! - `beads.patch` — merge-patch an entity
//! - `beads.delete` — delete an entity
//! - `beads.transition` — state transition (if gates defined)

use serde::Serialize;
use serde_json::Value;

/// A tool handler function.
pub type ToolHandler = Box<dyn Fn(&Value) -> Result<Value, ToolError> + Send + Sync>;

/// Error returned by tool handlers.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("internal: {0}")]
    Internal(String),
}

/// A tool definition exposed via MCP tools/list.
pub struct ToolDef {
    /// Tool name (e.g., "beads.create").
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the tool's input parameters.
    pub input_schema: Value,
    /// The handler function invoked by tools/call.
    pub handler: ToolHandler,
}

impl std::fmt::Debug for ToolDef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolDef")
            .field("name", &self.name)
            .field("description", &self.description)
            .field("input_schema", &self.input_schema)
            .finish_non_exhaustive()
    }
}

/// Serialized tool info for the tools/list response.
#[derive(Debug, Clone, Serialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// Registry of MCP tools.
///
/// Tools are generated from collection schemas and can be updated when
/// schemas change (triggering tools/list_changed notification).
pub struct ToolRegistry {
    tools: Vec<ToolDef>,
}

impl ToolRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    /// Register a tool.
    pub fn register(&mut self, tool: ToolDef) {
        self.tools.push(tool);
    }

    /// List all registered tools (for tools/list response).
    pub fn list_tools(&self) -> Vec<ToolInfo> {
        self.tools
            .iter()
            .map(|t| ToolInfo {
                name: t.name.clone(),
                description: t.description.clone(),
                input_schema: t.input_schema.clone(),
            })
            .collect()
    }

    /// Call a tool by name.
    pub fn call_tool(&self, name: &str, arguments: &Value) -> Result<Value, ToolError> {
        let tool = self
            .tools
            .iter()
            .find(|t| t.name == name)
            .ok_or_else(|| ToolError::NotFound(format!("unknown tool: {name}")))?;
        (tool.handler)(arguments)
    }

    /// Number of registered tools.
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// Generate CRUD tools for a collection.
    ///
    /// Produces stub tools: `{collection}.create`, `{collection}.get`,
    /// `{collection}.patch`, `{collection}.delete`.
    /// Parameters are derived from the collection name.
    pub fn register_collection_tools(&mut self, collection: &str) {
        let col = collection.to_string();

        // {collection}.create
        let col_c = col.clone();
        self.register(ToolDef {
            name: format!("{col}.create"),
            description: format!("Create a new entity in the {col} collection"),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Entity ID (optional, auto-generated if omitted)" },
                    "data": { "type": "object", "description": "Entity data" }
                },
                "required": ["data"]
            }),
            handler: Box::new(move |args| {
                Ok(serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": format!("Created entity in {col_c}: {args}")
                    }]
                }))
            }),
        });

        // {collection}.get
        let col_g = col.clone();
        self.register(ToolDef {
            name: format!("{col}.get"),
            description: format!("Get an entity from the {col} collection by ID"),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Entity ID" }
                },
                "required": ["id"]
            }),
            handler: Box::new(move |args| {
                Ok(serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": format!("Get entity from {col_g}: {args}")
                    }]
                }))
            }),
        });

        // {collection}.patch
        let col_p = col.clone();
        self.register(ToolDef {
            name: format!("{col}.patch"),
            description: format!("Merge-patch an entity in the {col} collection"),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Entity ID" },
                    "data": { "type": "object", "description": "Merge-patch data" },
                    "expected_version": { "type": "integer", "description": "Expected version for OCC" }
                },
                "required": ["id", "data", "expected_version"]
            }),
            handler: Box::new(move |args| {
                Ok(serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": format!("Patched entity in {col_p}: {args}")
                    }]
                }))
            }),
        });

        // {collection}.delete
        let col_d = col.clone();
        self.register(ToolDef {
            name: format!("{col}.delete"),
            description: format!("Delete an entity from the {col} collection"),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Entity ID" }
                },
                "required": ["id"]
            }),
            handler: Box::new(move |args| {
                Ok(serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": format!("Deleted entity from {col_d}: {args}")
                    }]
                }))
            }),
        });
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_registry_lists_no_tools() {
        let registry = ToolRegistry::new();
        assert!(registry.list_tools().is_empty());
        assert_eq!(registry.tool_count(), 0);
    }

    #[test]
    fn register_and_list_tool() {
        let mut registry = ToolRegistry::new();
        registry.register(ToolDef {
            name: "test.tool".into(),
            description: "desc".into(),
            input_schema: serde_json::json!({"type": "object"}),
            handler: Box::new(|_| Ok(serde_json::json!({}))),
        });

        let tools = registry.list_tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "test.tool");
    }

    #[test]
    fn call_tool_dispatches_to_handler() {
        let mut registry = ToolRegistry::new();
        registry.register(ToolDef {
            name: "add".into(),
            description: "add".into(),
            input_schema: serde_json::json!({}),
            handler: Box::new(|args| {
                let a = args["a"].as_i64().unwrap_or(0);
                let b = args["b"].as_i64().unwrap_or(0);
                Ok(serde_json::json!({"sum": a + b}))
            }),
        });

        let result = registry
            .call_tool("add", &serde_json::json!({"a": 3, "b": 4}))
            .unwrap();
        assert_eq!(result["sum"], 7);
    }

    #[test]
    fn call_unknown_tool_returns_error() {
        let registry = ToolRegistry::new();
        let err = registry
            .call_tool("nope", &serde_json::json!({}))
            .unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    #[test]
    fn register_collection_tools_creates_crud() {
        let mut registry = ToolRegistry::new();
        registry.register_collection_tools("tasks");

        let tools = registry.list_tools();
        assert_eq!(tools.len(), 4);

        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"tasks.create"));
        assert!(names.contains(&"tasks.get"));
        assert!(names.contains(&"tasks.patch"));
        assert!(names.contains(&"tasks.delete"));
    }

    #[test]
    fn collection_tools_have_input_schemas() {
        let mut registry = ToolRegistry::new();
        registry.register_collection_tools("users");

        let tools = registry.list_tools();
        for tool in &tools {
            assert_eq!(tool.input_schema["type"], "object");
        }
    }

    #[test]
    fn collection_tool_handlers_respond() {
        let mut registry = ToolRegistry::new();
        registry.register_collection_tools("items");

        let result = registry
            .call_tool("items.create", &serde_json::json!({"data": {"name": "x"}}))
            .unwrap();
        assert!(result["content"][0]["text"].as_str().is_some());
    }

    #[test]
    fn tools_list_changed_on_schema_update() {
        // Simulate schema update: old registry has tasks, new one has tasks + users.
        let mut registry = ToolRegistry::new();
        registry.register_collection_tools("tasks");
        assert_eq!(registry.tool_count(), 4);

        // After schema update, rebuild registry with both collections.
        let mut updated = ToolRegistry::new();
        updated.register_collection_tools("tasks");
        updated.register_collection_tools("users");
        assert_eq!(updated.tool_count(), 8);

        // The old and new registries differ in tool count.
        let old_names: Vec<String> = registry
            .list_tools()
            .iter()
            .map(|t| t.name.clone())
            .collect();
        let new_names: Vec<String> = updated
            .list_tools()
            .iter()
            .map(|t| t.name.clone())
            .collect();
        assert!(new_names.len() > old_names.len());
    }
}
