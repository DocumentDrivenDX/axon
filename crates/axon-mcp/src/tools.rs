//! MCP tool registry: maps collection schemas to typed tool definitions.
//!
//! Each registered Axon collection produces tools like:
//! - `beads.create` — create an entity
//! - `beads.get` — get an entity by ID
//! - `beads.patch` — merge-patch an entity
//! - `beads.delete` — delete an entity
//! - `beads.transition` — state transition (if gates defined)

use std::collections::BTreeMap;

use axon_schema::{ApprovalRoute, PolicyDecision, PolicyEnvelopeSummary};
use serde::Serialize;
use serde_json::Value;

const COLLECTION_TOOL_SUFFIXES: [&str; 7] = [
    ".create",
    ".get",
    ".patch",
    ".delete",
    ".aggregate",
    ".link_candidates",
    ".neighbors",
];

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

/// Caller-specific policy metadata attached to generated collection tools.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolPolicyMetadata {
    pub collection: String,
    pub policy_version: u32,
    pub capabilities: ToolPolicyCapabilities,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_operation: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub redacted_fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub denied_fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub envelopes: Vec<ToolPolicyEnvelopeSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub applicable_envelopes: Vec<ToolPolicyEnvelopeSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub envelope_summary: Option<String>,
}

impl ToolPolicyMetadata {
    pub fn describe_tool(&self, base: &str) -> String {
        let mut parts = vec![
            format!("canRead={}", self.capabilities.can_read),
            format!("canCreate={}", self.capabilities.can_create),
            format!("canUpdate={}", self.capabilities.can_update),
            format!("canDelete={}", self.capabilities.can_delete),
            format!("policyVersion={}", self.policy_version),
        ];

        if let Some(operation) = &self.tool_operation {
            parts.push(format!("toolOperation={operation}"));
        }
        if !self.redacted_fields.is_empty() {
            parts.push(format!("redactedFields={}", self.redacted_fields.join(",")));
        }
        if !self.denied_fields.is_empty() {
            parts.push(format!("deniedFields={}", self.denied_fields.join(",")));
        }
        if let Some(summary) = &self.envelope_summary {
            parts.push(format!("envelopes={summary}"));
        }

        format!("{base}. Policy: {}.", parts.join("; "))
    }

    fn for_tool(&self, tool_name: &str, collection: &str) -> Self {
        let tool_operation = tool_operation_for_tool(tool_name, collection).map(str::to_string);
        let applicable_envelopes = tool_operation
            .as_deref()
            .map(|operation| {
                self.envelopes
                    .iter()
                    .filter(|envelope| envelope_applies_to_tool_operation(envelope, operation))
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let envelope_summary = envelope_summary_text(&applicable_envelopes);

        Self {
            collection: self.collection.clone(),
            policy_version: self.policy_version,
            capabilities: self.capabilities.clone(),
            tool_operation,
            redacted_fields: self.redacted_fields.clone(),
            denied_fields: self.denied_fields.clone(),
            envelopes: self.envelopes.clone(),
            applicable_envelopes,
            envelope_summary,
        }
    }
}

/// Effective collection capabilities for a caller.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolPolicyCapabilities {
    pub can_read: bool,
    pub can_create: bool,
    pub can_update: bool,
    pub can_delete: bool,
}

/// Approval route metadata for a policy envelope.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolPolicyApproval {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub reason_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline_seconds: Option<u64>,
    pub separation_of_duties: bool,
}

impl From<ApprovalRoute> for ToolPolicyApproval {
    fn from(approval: ApprovalRoute) -> Self {
        Self {
            role: approval.role,
            reason_required: approval.reason_required,
            deadline_seconds: approval.deadline_seconds,
            separation_of_duties: approval.separation_of_duties,
        }
    }
}

/// Decision envelope summary exposed in `tools/list` policy metadata.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolPolicyEnvelopeSummary {
    pub collection: String,
    pub operation: String,
    pub envelope_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub decision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<ToolPolicyApproval>,
}

impl ToolPolicyEnvelopeSummary {
    fn label(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.envelope_id)
    }

    fn summary_text(&self) -> String {
        let decision = match self.decision.as_str() {
            "allow" => "autonomous",
            other => other,
        };
        let mut parts = vec![format!("{}:{}={decision}", self.operation, self.label())];
        if let Some(approval) = &self.approval {
            if let Some(role) = &approval.role {
                parts.push(format!("role={role}"));
            }
            parts.push(format!("reasonRequired={}", approval.reason_required));
            if let Some(deadline) = approval.deadline_seconds {
                parts.push(format!("deadlineSeconds={deadline}"));
            }
            if approval.separation_of_duties {
                parts.push("separationOfDuties=true".into());
            }
        }
        parts.join(" ")
    }
}

impl From<PolicyEnvelopeSummary> for ToolPolicyEnvelopeSummary {
    fn from(summary: PolicyEnvelopeSummary) -> Self {
        Self {
            collection: summary.collection,
            operation: summary.operation.as_str().to_string(),
            envelope_id: summary.envelope_id,
            name: summary.name,
            decision: policy_decision_name(&summary.decision).to_string(),
            approval: summary.approval.map(ToolPolicyApproval::from),
        }
    }
}

fn envelope_summary_text(envelopes: &[ToolPolicyEnvelopeSummary]) -> Option<String> {
    (!envelopes.is_empty()).then(|| {
        envelopes
            .iter()
            .map(ToolPolicyEnvelopeSummary::summary_text)
            .collect::<Vec<_>>()
            .join(", ")
    })
}

fn tool_operation_for_tool<'a>(tool_name: &'a str, collection: &str) -> Option<&'a str> {
    let suffix = tool_name.strip_prefix(collection)?;
    match suffix {
        ".create" => Some("create"),
        ".get" | ".aggregate" | ".link_candidates" | ".neighbors" => Some("read"),
        ".patch" => Some("patch"),
        ".delete" => Some("delete"),
        _ => None,
    }
}

fn envelope_applies_to_tool_operation(
    envelope: &ToolPolicyEnvelopeSummary,
    tool_operation: &str,
) -> bool {
    match tool_operation {
        "create" => matches!(envelope.operation.as_str(), "create" | "write"),
        "patch" | "update" | "transition" | "rollback" => {
            matches!(envelope.operation.as_str(), "update" | "write")
        }
        "delete" => matches!(envelope.operation.as_str(), "delete" | "write"),
        operation => envelope.operation == operation,
    }
}

fn policy_decision_name(decision: &PolicyDecision) -> &'static str {
    match decision {
        PolicyDecision::Allow => "allow",
        PolicyDecision::NeedsApproval => "needs_approval",
        PolicyDecision::Deny => "deny",
    }
}

/// Serialized tool info for the tools/list response.
#[derive(Debug, Clone, Serialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<ToolPolicyMetadata>,
}

/// Registry of MCP tools.
///
/// Tools are generated from collection schemas and can be updated when
/// schemas change (triggering tools/list_changed notification).
pub struct ToolRegistry {
    tools: Vec<ToolDef>,
    collection_policies: BTreeMap<String, ToolPolicyMetadata>,
}

impl ToolRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            tools: Vec::new(),
            collection_policies: BTreeMap::new(),
        }
    }

    /// Register a tool.
    pub fn register(&mut self, tool: ToolDef) {
        self.tools.push(tool);
    }

    /// Attach caller-specific policy metadata to all generated tools for a collection.
    pub fn set_collection_policy(
        &mut self,
        collection: impl Into<String>,
        policy: ToolPolicyMetadata,
    ) {
        self.collection_policies.insert(collection.into(), policy);
    }

    /// List all registered tools (for tools/list response).
    pub fn list_tools(&self) -> Vec<ToolInfo> {
        self.tools
            .iter()
            .map(|t| {
                let policy = self
                    .policy_for_tool(&t.name)
                    .map(|(collection, policy)| policy.for_tool(&t.name, collection));
                ToolInfo {
                    name: t.name.clone(),
                    description: policy.as_ref().map_or_else(
                        || t.description.clone(),
                        |policy| policy.describe_tool(&t.description),
                    ),
                    input_schema: input_schema_with_policy(&t.input_schema, policy.as_ref()),
                    policy,
                }
            })
            .collect()
    }

    fn policy_for_tool(&self, tool_name: &str) -> Option<(&str, &ToolPolicyMetadata)> {
        self.collection_policies
            .iter()
            .filter_map(|(collection, policy)| {
                let suffix = tool_name.strip_prefix(collection)?;
                COLLECTION_TOOL_SUFFIXES.contains(&suffix).then_some((
                    collection.len(),
                    collection.as_str(),
                    policy,
                ))
            })
            .max_by_key(|(len, _, _)| *len)
            .map(|(_, collection, policy)| (collection, policy))
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

fn input_schema_with_policy(schema: &Value, policy: Option<&ToolPolicyMetadata>) -> Value {
    let mut schema = schema.clone();
    let Some(policy) = policy else {
        return schema;
    };
    let Ok(policy_value) = serde_json::to_value(policy) else {
        return schema;
    };
    if let Value::Object(map) = &mut schema {
        map.insert("x-axon-policy".into(), policy_value);
    }
    schema
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
            .expect("registered tool should dispatch successfully");
        assert_eq!(result["sum"], 7);
    }

    #[test]
    fn call_unknown_tool_returns_error() {
        let registry = ToolRegistry::new();
        let err = registry
            .call_tool("nope", &serde_json::json!({}))
            .expect_err("unknown tools should return a not-found error");
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
            .expect("collection tool should respond successfully");
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
        let old_tool_count = registry.list_tools().len();
        let new_tool_count = updated.list_tools().len();
        assert!(new_tool_count > old_tool_count);
    }
}
