//! MCP JSON-RPC 2.0 protocol implementation.

use axon_api::intent::MutationIntentCommitValidationError;
use axon_core::intent::{
    MutationApprovalRoute, MutationIntent, MutationIntentToken, MutationIntentTokenLookupError,
    MutationReviewSummary,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::prompts::PromptRegistry;
use crate::resources::ResourceRegistry;
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

    #[error("not found: {0}")]
    NotFound(String),

    #[error("internal error: {0}")]
    Internal(String),
}

impl McpError {
    fn jsonrpc_code(&self) -> i64 {
        match self {
            Self::Parse(_) => -32700,
            Self::MethodNotFound(_) => -32601,
            Self::InvalidParams(_) | Self::NotFound(_) => -32602,
            Self::Internal(_) => -32603,
        }
    }

    fn jsonrpc_message(&self) -> String {
        match self {
            Self::Parse(message) => format!("Parse error: {message}"),
            Self::MethodNotFound(method) => format!("Method not found: {method}"),
            Self::InvalidParams(message) => format!("Invalid params: {message}"),
            Self::NotFound(message) => format!("Not found: {message}"),
            Self::Internal(message) => format!("Internal error: {message}"),
        }
    }

    fn jsonrpc_data(&self) -> Option<Value> {
        match self {
            Self::NotFound(detail) => Some(serde_json::json!({
                "code": "not_found",
                "detail": detail,
            })),
            Self::InvalidParams(detail) => Some(serde_json::json!({
                "code": "invalid_params",
                "detail": detail,
            })),
            Self::Internal(detail) => Some(serde_json::json!({
                "code": "internal_error",
                "detail": detail,
            })),
            Self::Parse(_) | Self::MethodNotFound(_) => None,
        }
    }
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

/// MCP text content block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpTextContent {
    /// Content block type.
    #[serde(rename = "type")]
    pub content_type: String,
    /// Text payload.
    pub text: String,
}

impl McpTextContent {
    fn new(text: impl Into<String>) -> Self {
        Self {
            content_type: "text".into(),
            text: text.into(),
        }
    }
}

/// Stable MCP error code for mutation intent outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpMutationIntentErrorCode {
    IntentStale,
    IntentMismatch,
    DeniedPolicy,
    ExpiredToken,
    RejectedIntent,
    AlreadyCommitted,
    ApprovalRequired,
    InvalidToken,
    IntentNotFound,
    TenantDatabaseMismatch,
}

impl McpMutationIntentErrorCode {
    fn as_str(self) -> &'static str {
        match self {
            Self::IntentStale => "intent_stale",
            Self::IntentMismatch => "intent_mismatch",
            Self::DeniedPolicy => "denied_policy",
            Self::ExpiredToken => "expired_token",
            Self::RejectedIntent => "rejected_intent",
            Self::AlreadyCommitted => "already_committed",
            Self::ApprovalRequired => "approval_required",
            Self::InvalidToken => "invalid_token",
            Self::IntentNotFound => "intent_not_found",
            Self::TenantDatabaseMismatch => "tenant_database_mismatch",
        }
    }
}

/// Dimension that made a mutation intent stale or mismatched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpMutationIntentConflictDimension {
    GrantVersion,
    SchemaVersion,
    PolicyVersion,
    PreImage,
    OperationHash,
    Token,
    ApprovalState,
    TenantDatabase,
}

/// Detail for stale or mismatched mutation intent execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpMutationIntentConflictDetail {
    /// Stale or mismatched binding dimension.
    pub dimension: McpMutationIntentConflictDimension,
    /// Expected value, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<Value>,
    /// Actual value, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual: Option<Value>,
    /// Human-readable detail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Structured MCP mutation intent outcome payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum McpMutationIntentOutcome {
    /// Mutation can commit without human approval.
    Allowed {
        intent_id: String,
        intent_token: String,
        approval_summary: MutationReviewSummary,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        policy_explanation: Vec<String>,
    },
    /// Mutation is valid but requires approval before commit.
    NeedsApproval {
        intent_id: String,
        intent_token: String,
        approval_summary: MutationReviewSummary,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        approval_route: Option<MutationApprovalRoute>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        policy_explanation: Vec<String>,
    },
    /// Mutation intent was committed successfully.
    Committed {
        intent_id: String,
        transaction_id: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        written: Vec<Value>,
    },
    /// Mutation is denied by policy.
    Denied {
        error_code: McpMutationIntentErrorCode,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        intent_id: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        policy_explanation: Vec<String>,
    },
    /// Mutation intent cannot commit because the reviewed binding is stale,
    /// mismatched, expired, rejected, or otherwise not currently executable.
    Conflict {
        error_code: McpMutationIntentErrorCode,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        intent_id: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        details: Vec<McpMutationIntentConflictDetail>,
    },
}

impl McpMutationIntentOutcome {
    /// Build an `allowed` result from a stored mutation intent and token.
    pub fn allowed(intent: &MutationIntent, token: &MutationIntentToken) -> Self {
        Self::Allowed {
            intent_id: intent.intent_id.clone(),
            intent_token: token.as_str().to_string(),
            approval_summary: intent.review_summary.clone(),
            policy_explanation: intent.review_summary.policy_explanation.clone(),
        }
    }

    /// Build a `needs_approval` result from a stored mutation intent and token.
    pub fn needs_approval(intent: &MutationIntent, token: &MutationIntentToken) -> Self {
        Self::NeedsApproval {
            intent_id: intent.intent_id.clone(),
            intent_token: token.as_str().to_string(),
            approval_summary: intent.review_summary.clone(),
            approval_route: intent.approval_route.clone(),
            policy_explanation: intent.review_summary.policy_explanation.clone(),
        }
    }

    /// Build a denied policy result.
    pub fn denied_policy(
        message: impl Into<String>,
        intent_id: Option<String>,
        policy_explanation: Vec<String>,
    ) -> Self {
        Self::Denied {
            error_code: McpMutationIntentErrorCode::DeniedPolicy,
            message: message.into(),
            intent_id,
            policy_explanation,
        }
    }

    /// Build a committed result.
    pub fn committed(intent: &MutationIntent, transaction_id: String, written: Vec<Value>) -> Self {
        Self::Committed {
            intent_id: intent.intent_id.clone(),
            transaction_id,
            written,
        }
    }

    /// Map a core token/commit validation failure into a stable MCP outcome.
    pub fn from_token_lookup_error(error: MutationIntentTokenLookupError) -> Self {
        match error {
            MutationIntentTokenLookupError::GrantVersionStale => stale_conflict(
                McpMutationIntentConflictDimension::GrantVersion,
                "grant version changed since preview",
            ),
            MutationIntentTokenLookupError::SchemaVersionStale => stale_conflict(
                McpMutationIntentConflictDimension::SchemaVersion,
                "schema version changed since preview",
            ),
            MutationIntentTokenLookupError::PolicyVersionStale => stale_conflict(
                McpMutationIntentConflictDimension::PolicyVersion,
                "policy version changed since preview",
            ),
            MutationIntentTokenLookupError::PreImageStale => stale_conflict(
                McpMutationIntentConflictDimension::PreImage,
                "reviewed entity or link version changed since preview",
            ),
            MutationIntentTokenLookupError::OperationMismatch => Self::Conflict {
                error_code: McpMutationIntentErrorCode::IntentMismatch,
                message: "mutation intent operation hash does not match reviewed operation".into(),
                intent_id: None,
                details: vec![McpMutationIntentConflictDetail {
                    dimension: McpMutationIntentConflictDimension::OperationHash,
                    expected: None,
                    actual: None,
                    detail: Some("operation hash mismatch".into()),
                }],
            },
            MutationIntentTokenLookupError::Unauthorized => {
                Self::denied_policy("mutation denied by policy", None, Vec::new())
            }
            MutationIntentTokenLookupError::Expired => Self::Conflict {
                error_code: McpMutationIntentErrorCode::ExpiredToken,
                message: "mutation intent token is expired".into(),
                intent_id: None,
                details: vec![McpMutationIntentConflictDetail {
                    dimension: McpMutationIntentConflictDimension::Token,
                    expected: None,
                    actual: None,
                    detail: Some("token expired".into()),
                }],
            },
            MutationIntentTokenLookupError::Rejected => Self::Conflict {
                error_code: McpMutationIntentErrorCode::RejectedIntent,
                message: "mutation intent was rejected".into(),
                intent_id: None,
                details: vec![McpMutationIntentConflictDetail {
                    dimension: McpMutationIntentConflictDimension::ApprovalState,
                    expected: None,
                    actual: Some(serde_json::json!("rejected")),
                    detail: Some("approval state is rejected".into()),
                }],
            },
            MutationIntentTokenLookupError::AlreadyCommitted => Self::Conflict {
                error_code: McpMutationIntentErrorCode::AlreadyCommitted,
                message: "mutation intent was already committed".into(),
                intent_id: None,
                details: vec![McpMutationIntentConflictDetail {
                    dimension: McpMutationIntentConflictDimension::ApprovalState,
                    expected: None,
                    actual: Some(serde_json::json!("committed")),
                    detail: Some("approval state is committed".into()),
                }],
            },
            MutationIntentTokenLookupError::ApprovalRequired => Self::Conflict {
                error_code: McpMutationIntentErrorCode::ApprovalRequired,
                message: "mutation intent requires approval before commit".into(),
                intent_id: None,
                details: vec![McpMutationIntentConflictDetail {
                    dimension: McpMutationIntentConflictDimension::ApprovalState,
                    expected: Some(serde_json::json!("approved")),
                    actual: None,
                    detail: Some("approval state is not approved".into()),
                }],
            },
            MutationIntentTokenLookupError::MalformedToken
            | MutationIntentTokenLookupError::InvalidSignature => Self::Conflict {
                error_code: McpMutationIntentErrorCode::InvalidToken,
                message: "mutation intent token is invalid".into(),
                intent_id: None,
                details: vec![McpMutationIntentConflictDetail {
                    dimension: McpMutationIntentConflictDimension::Token,
                    expected: None,
                    actual: None,
                    detail: Some("token is malformed or has an invalid signature".into()),
                }],
            },
            MutationIntentTokenLookupError::NotFound => Self::Conflict {
                error_code: McpMutationIntentErrorCode::IntentNotFound,
                message: "mutation intent was not found".into(),
                intent_id: None,
                details: Vec::new(),
            },
            MutationIntentTokenLookupError::TenantDatabaseMismatch => Self::Conflict {
                error_code: McpMutationIntentErrorCode::TenantDatabaseMismatch,
                message: "mutation intent tenant/database scope does not match request".into(),
                intent_id: None,
                details: vec![McpMutationIntentConflictDetail {
                    dimension: McpMutationIntentConflictDimension::TenantDatabase,
                    expected: None,
                    actual: None,
                    detail: Some("tenant/database scope mismatch".into()),
                }],
            },
        }
    }

    /// Map a core commit validation failure into a stable MCP outcome.
    pub fn from_commit_validation_error(error: MutationIntentCommitValidationError) -> Self {
        match error {
            MutationIntentCommitValidationError::Token(error) => {
                Self::from_token_lookup_error(error)
            }
            MutationIntentCommitValidationError::IntentMismatch {
                intent_id,
                expected_hash,
                actual_hash,
            } => Self::Conflict {
                error_code: McpMutationIntentErrorCode::IntentMismatch,
                message: "mutation intent operation hash does not match reviewed operation".into(),
                intent_id: Some(intent_id),
                details: vec![McpMutationIntentConflictDetail {
                    dimension: McpMutationIntentConflictDimension::OperationHash,
                    expected: Some(Value::String(expected_hash)),
                    actual: Some(Value::String(actual_hash)),
                    detail: Some("operation hash mismatch".into()),
                }],
            },
            MutationIntentCommitValidationError::IntentStale {
                intent_id,
                dimensions,
            } => Self::Conflict {
                error_code: McpMutationIntentErrorCode::IntentStale,
                message: "mutation intent is stale".into(),
                intent_id: Some(intent_id),
                details: dimensions
                    .into_iter()
                    .map(|dimension| McpMutationIntentConflictDetail {
                        dimension: stale_dimension_name(&dimension.dimension),
                        expected: dimension.expected.map(Value::String),
                        actual: dimension.actual.map(Value::String),
                        detail: dimension.path,
                    })
                    .collect(),
            },
            MutationIntentCommitValidationError::AuthorizationFailed { intent_id, reason } => {
                Self::Denied {
                    error_code: McpMutationIntentErrorCode::DeniedPolicy,
                    message: reason,
                    intent_id: Some(intent_id),
                    policy_explanation: Vec::new(),
                }
            }
            MutationIntentCommitValidationError::Storage(message)
            | MutationIntentCommitValidationError::CommitFailed {
                source: message, ..
            } => Self::Conflict {
                error_code: McpMutationIntentErrorCode::IntentStale,
                message,
                intent_id: None,
                details: Vec::new(),
            },
        }
    }

    fn is_error(&self) -> bool {
        matches!(self, Self::Denied { .. } | Self::Conflict { .. })
    }

    fn summary_text(&self) -> String {
        match self {
            Self::Allowed { intent_id, .. } => {
                format!("Mutation intent {intent_id} is allowed.")
            }
            Self::NeedsApproval { intent_id, .. } => {
                format!("Mutation intent {intent_id} needs approval.")
            }
            Self::Committed {
                intent_id,
                transaction_id,
                ..
            } => {
                format!("Mutation intent {intent_id} committed in transaction {transaction_id}.")
            }
            Self::Denied {
                error_code,
                message,
                ..
            }
            | Self::Conflict {
                error_code,
                message,
                ..
            } => {
                format!("{}: {message}", error_code.as_str())
            }
        }
    }
}

/// MCP tool result wrapper for mutation intent outcomes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpMutationIntentToolResult {
    /// Human-readable MCP content block.
    pub content: Vec<McpTextContent>,
    /// Machine-readable outcome payload.
    #[serde(rename = "structuredContent")]
    pub structured_content: McpMutationIntentOutcome,
    /// Marks denied and conflict outcomes as MCP tool errors while preserving structured details.
    #[serde(rename = "isError", skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

impl From<McpMutationIntentOutcome> for McpMutationIntentToolResult {
    fn from(outcome: McpMutationIntentOutcome) -> Self {
        let is_error = outcome.is_error().then_some(true);
        Self {
            content: vec![McpTextContent::new(outcome.summary_text())],
            structured_content: outcome,
            is_error,
        }
    }
}

fn stale_dimension_name(dimension: &str) -> McpMutationIntentConflictDimension {
    match dimension {
        "grant_version" => McpMutationIntentConflictDimension::GrantVersion,
        "schema_version" => McpMutationIntentConflictDimension::SchemaVersion,
        "policy_version" => McpMutationIntentConflictDimension::PolicyVersion,
        "pre_image" => McpMutationIntentConflictDimension::PreImage,
        "operation_hash" => McpMutationIntentConflictDimension::OperationHash,
        "tenant_database" => McpMutationIntentConflictDimension::TenantDatabase,
        "approval_state" => McpMutationIntentConflictDimension::ApprovalState,
        _ => McpMutationIntentConflictDimension::Token,
    }
}

fn stale_conflict(
    dimension: McpMutationIntentConflictDimension,
    detail: &'static str,
) -> McpMutationIntentOutcome {
    McpMutationIntentOutcome::Conflict {
        error_code: McpMutationIntentErrorCode::IntentStale,
        message: "mutation intent is stale".into(),
        intent_id: None,
        details: vec![McpMutationIntentConflictDetail {
            dimension,
            expected: None,
            actual: None,
            detail: Some(detail.into()),
        }],
    }
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

    fn error(id: Option<Value>, code: i64, message: String, data: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message,
                data,
            }),
        }
    }

    fn from_mcp_error(id: Option<Value>, error: McpError) -> Self {
        Self::error(
            id,
            error.jsonrpc_code(),
            error.jsonrpc_message(),
            error.jsonrpc_data(),
        )
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
    pub resources: ResourcesCapability,
    pub prompts: PromptsCapability,
}

/// Tool-related capabilities.
#[derive(Debug, Clone, Serialize)]
pub struct ToolsCapability {
    /// Whether the server supports tools/list_changed notifications.
    #[serde(rename = "listChanged")]
    pub list_changed: bool,
}

/// Resource-related capabilities.
#[derive(Debug, Clone, Serialize)]
pub struct ResourcesCapability {
    /// Whether the server supports resources/list_changed notifications.
    #[serde(rename = "listChanged")]
    pub list_changed: bool,
    /// Whether the server supports resource subscriptions.
    pub subscribe: bool,
}

/// Prompt-related capabilities.
#[derive(Debug, Clone, Serialize)]
pub struct PromptsCapability {
    /// Whether the server supports prompts/list_changed notifications.
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
    resource_registry: ResourceRegistry,
    prompt_registry: PromptRegistry,
    initialized: bool,
    /// Active resource subscriptions.
    subscriptions: Vec<ResourceSubscription>,
    next_sub_id: u64,
}

impl McpServer {
    /// Create a new MCP server with the given tool registry.
    pub fn new(
        tool_registry: ToolRegistry,
        resource_registry: ResourceRegistry,
        prompt_registry: PromptRegistry,
    ) -> Self {
        Self {
            tool_registry,
            resource_registry,
            prompt_registry,
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
                let resp = JsonRpcResponse::error(None, -32700, format!("Parse error: {e}"), None);
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
            "resources/list" => self.handle_resources_list(request),
            "resources/templates/list" => self.handle_resource_templates_list(request),
            "resources/read" => self.handle_resources_read(request),
            "resources/subscribe" => self.handle_resource_subscribe(request),
            "resources/unsubscribe" => self.handle_resource_unsubscribe(request),
            "prompts/list" => self.handle_prompts_list(request),
            "prompts/get" => self.handle_prompts_get(request),
            "ping" => JsonRpcResponse::success(request.id.clone(), serde_json::json!({})),
            _ => JsonRpcResponse::from_mcp_error(
                request.id.clone(),
                McpError::MethodNotFound(request.method.clone()),
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
                    "listChanged": false,
                    "subscribe": true
                },
                "prompts": {
                    "listChanged": false
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
                return JsonRpcResponse::from_mcp_error(
                    request.id.clone(),
                    McpError::InvalidParams("missing params".into()),
                );
            }
        };

        let tool_name = match params.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => {
                return JsonRpcResponse::from_mcp_error(
                    request.id.clone(),
                    McpError::InvalidParams("missing 'name' in params".into()),
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

    fn handle_resources_list(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        let resources = self.resource_registry.list_resources();
        JsonRpcResponse::success(
            request.id.clone(),
            serde_json::json!({
                "resources": resources,
            }),
        )
    }

    fn handle_resource_templates_list(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        let resource_templates = self.resource_registry.list_resource_templates();
        JsonRpcResponse::success(
            request.id.clone(),
            serde_json::json!({
                "resourceTemplates": resource_templates,
            }),
        )
    }

    fn handle_resources_read(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        let params = match &request.params {
            Some(p) => p,
            None => {
                return JsonRpcResponse::from_mcp_error(
                    request.id.clone(),
                    McpError::InvalidParams("missing params".into()),
                );
            }
        };

        let uri = match params.get("uri").and_then(|value| value.as_str()) {
            Some(uri) => uri,
            None => {
                return JsonRpcResponse::from_mcp_error(
                    request.id.clone(),
                    McpError::InvalidParams("missing 'uri' in params".into()),
                );
            }
        };

        match self.resource_registry.read_resource(uri) {
            Ok(result) => JsonRpcResponse::success(request.id.clone(), result),
            Err(error) => JsonRpcResponse::from_mcp_error(request.id.clone(), error),
        }
    }

    fn handle_resource_subscribe(&mut self, request: &JsonRpcRequest) -> JsonRpcResponse {
        let params = match &request.params {
            Some(p) => p,
            None => {
                return JsonRpcResponse::from_mcp_error(
                    request.id.clone(),
                    McpError::InvalidParams("missing params".into()),
                );
            }
        };

        let uri = match params.get("uri").and_then(|v| v.as_str()) {
            Some(u) => u.to_string(),
            None => {
                return JsonRpcResponse::from_mcp_error(
                    request.id.clone(),
                    McpError::InvalidParams("missing 'uri' in params".into()),
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
                return JsonRpcResponse::from_mcp_error(
                    request.id.clone(),
                    McpError::InvalidParams("missing params".into()),
                );
            }
        };

        let uri = match params.get("uri").and_then(|v| v.as_str()) {
            Some(u) => u,
            None => {
                return JsonRpcResponse::from_mcp_error(
                    request.id.clone(),
                    McpError::InvalidParams("missing 'uri' in params".into()),
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

    fn handle_prompts_list(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        let prompts = self.prompt_registry.list_prompts();
        JsonRpcResponse::success(
            request.id.clone(),
            serde_json::json!({
                "prompts": prompts,
            }),
        )
    }

    fn handle_prompts_get(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        let params = match &request.params {
            Some(params) => params,
            None => {
                return JsonRpcResponse::from_mcp_error(
                    request.id.clone(),
                    McpError::InvalidParams("missing params".into()),
                );
            }
        };

        let name = match params.get("name").and_then(|value| value.as_str()) {
            Some(name) => name,
            None => {
                return JsonRpcResponse::from_mcp_error(
                    request.id.clone(),
                    McpError::InvalidParams("missing 'name' in params".into()),
                );
            }
        };

        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));

        match self.prompt_registry.get_prompt(name, &arguments) {
            Ok(result) => JsonRpcResponse::success(request.id.clone(), result),
            Err(error) => JsonRpcResponse::from_mcp_error(request.id.clone(), error),
        }
    }

    /// Update the tool registry (e.g., after schema changes).
    pub fn update_registry(&mut self, registry: ToolRegistry) {
        self.tool_registry = registry;
    }

    /// Refresh all discoverable registries while preserving session state.
    pub fn refresh_registries(
        &mut self,
        tool_registry: ToolRegistry,
        resource_registry: ResourceRegistry,
        prompt_registry: PromptRegistry,
    ) {
        self.tool_registry = tool_registry;
        self.resource_registry = resource_registry;
        self.prompt_registry = prompt_registry;
    }

    /// Return the currently subscribed resource URIs.
    pub fn subscribed_resource_uris(&self) -> Vec<String> {
        self.subscriptions
            .iter()
            .map(|subscription| subscription.uri.clone())
            .collect()
    }

    /// Check if the server has been initialized via the handshake.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompts::PromptRegistry;
    use crate::resources::ResourceRegistry;
    use crate::tools::ToolDef;
    use axon_core::id::{CollectionId, EntityId};
    use axon_core::intent::{
        ApprovalState, CanonicalOperationMetadata, MutationIntentDecision,
        MutationIntentScopeBinding, MutationIntentSubjectBinding, MutationOperationKind,
        PreImageBinding,
    };

    fn empty_server() -> McpServer {
        McpServer::new(
            ToolRegistry::new(),
            ResourceRegistry::default(),
            PromptRegistry::default(),
        )
    }

    fn response_json(server: &mut McpServer, request: Value) -> Value {
        let response = server
            .handle_message(&request.to_string())
            .expect("request should produce a protocol response");
        serde_json::from_str(&response).expect("protocol response should be valid JSON")
    }

    fn sample_intent(decision: MutationIntentDecision) -> MutationIntent {
        let approval_route =
            (decision == MutationIntentDecision::NeedsApproval).then_some(MutationApprovalRoute {
                role: Some("finance_approver".into()),
                reason_required: true,
                deadline_seconds: Some(3600),
                separation_of_duties: true,
            });
        let approval_state = match decision {
            MutationIntentDecision::Allow | MutationIntentDecision::Deny => ApprovalState::None,
            MutationIntentDecision::NeedsApproval => ApprovalState::Pending,
        };

        MutationIntent {
            intent_id: "mint_01H".into(),
            scope: MutationIntentScopeBinding {
                tenant_id: "acme".into(),
                database_id: "finance".into(),
            },
            subject: MutationIntentSubjectBinding::default(),
            schema_version: 12,
            policy_version: 12,
            operation: CanonicalOperationMetadata {
                operation_kind: MutationOperationKind::UpdateEntity,
                operation_hash: "sha256:abc123".into(),
                canonical_operation: Some(serde_json::json!({
                    "collection": "invoices",
                    "id": "inv-001",
                    "patch": {"amount_cents": 1250000}
                })),
            },
            pre_images: vec![PreImageBinding::Entity {
                collection: CollectionId::new("invoices"),
                id: EntityId::new("inv-001"),
                version: 5,
            }],
            decision,
            approval_state,
            approval_route,
            expires_at: 1000,
            review_summary: MutationReviewSummary {
                title: Some("Invoice amount update".into()),
                summary: "Review invoice amount before commit.".into(),
                risk: Some("amount_above_autonomous_limit".into()),
                affected_records: Vec::new(),
                affected_fields: vec!["amount_cents".into()],
                diff: serde_json::json!({
                    "amount_cents": {"before": 900000, "after": 1250000}
                }),
                policy_explanation: vec!["require-approval-large-invoice matched".into()],
            },
        }
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
        let resp = response_json(&mut server, req);

        assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(resp["result"]["serverInfo"]["name"], "axon-mcp");
        assert!(resp["result"]["capabilities"]["tools"]["listChanged"]
            .as_bool()
            .expect("initialize response should include tools.listChanged"));
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
            handler: Box::new(|_args| {
                Ok(serde_json::json!({
                    "content": [{"type": "text", "text": "ok"}]
                }))
            }),
        });

        let mut server = McpServer::new(
            registry,
            ResourceRegistry::default(),
            PromptRegistry::default(),
        );
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        });
        let resp = response_json(&mut server, req);

        let tools = resp["result"]["tools"]
            .as_array()
            .expect("tools/list response should include a tools array");
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

        let mut server = McpServer::new(
            registry,
            ResourceRegistry::default(),
            PromptRegistry::default(),
        );
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "axon.echo",
                "arguments": { "text": "hello" }
            }
        });
        let resp = response_json(&mut server, req);

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
        let resp = response_json(&mut server, req);

        assert!(resp["result"]["isError"]
            .as_bool()
            .expect("error tool response should include isError"));
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
        let resp = response_json(&mut server, req);

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
        let resp = response_json(&mut server, req);
        assert_eq!(resp["result"], serde_json::json!({}));
    }

    #[test]
    fn invalid_json_returns_parse_error() {
        let mut server = empty_server();
        let resp = serde_json::from_str::<Value>(
            &server
                .handle_message("not json")
                .expect("invalid JSON should still yield a parse error response"),
        )
        .expect("parse error response should be valid JSON");
        assert_eq!(resp["error"]["code"], -32700);
    }

    #[test]
    fn intent_allowed_outcome_serializes_structured_tool_result() {
        let intent = sample_intent(MutationIntentDecision::Allow);
        let token = MutationIntentToken::new("token.allowed");
        let result =
            McpMutationIntentToolResult::from(McpMutationIntentOutcome::allowed(&intent, &token));
        let value = serde_json::to_value(result).expect("tool result should serialize");

        assert_eq!(value["content"][0]["type"], "text");
        assert_eq!(value["structuredContent"]["outcome"], "allowed");
        assert_eq!(value["structuredContent"]["intent_id"], "mint_01H");
        assert_eq!(value["structuredContent"]["intent_token"], "token.allowed");
        assert_eq!(
            value["structuredContent"]["approval_summary"]["summary"],
            "Review invoice amount before commit."
        );
        assert_eq!(
            value["structuredContent"]["policy_explanation"][0],
            "require-approval-large-invoice matched"
        );
    }

    #[test]
    fn intent_needs_approval_outcome_serializes_approval_route() {
        let intent = sample_intent(MutationIntentDecision::NeedsApproval);
        let token = MutationIntentToken::new("token.pending");
        let value = serde_json::to_value(McpMutationIntentToolResult::from(
            McpMutationIntentOutcome::needs_approval(&intent, &token),
        ))
        .expect("tool result should serialize");

        assert_eq!(value["structuredContent"]["outcome"], "needs_approval");
        assert_eq!(value["structuredContent"]["intent_id"], "mint_01H");
        assert_eq!(value["structuredContent"]["intent_token"], "token.pending");
        assert_eq!(
            value["structuredContent"]["approval_route"]["role"],
            "finance_approver"
        );
        assert_eq!(
            value["structuredContent"]["approval_route"]["reason_required"],
            true
        );
    }

    #[test]
    fn intent_denied_outcome_serializes_policy_explanation() {
        let outcome = McpMutationIntentOutcome::denied_policy(
            "amount exceeds limit",
            Some("mint_denied".into()),
            vec!["deny-large-write matched".into()],
        );
        let value = serde_json::to_value(McpMutationIntentToolResult::from(outcome))
            .expect("tool result should serialize");

        assert_eq!(value["structuredContent"]["outcome"], "denied");
        assert_eq!(value["structuredContent"]["error_code"], "denied_policy");
        assert_eq!(value["structuredContent"]["intent_id"], "mint_denied");
        assert_eq!(
            value["structuredContent"]["policy_explanation"][0],
            "deny-large-write matched"
        );
    }

    #[test]
    fn intent_conflict_outcome_serializes_stale_details() {
        let outcome = McpMutationIntentOutcome::from_token_lookup_error(
            MutationIntentTokenLookupError::PolicyVersionStale,
        );
        let value = serde_json::to_value(McpMutationIntentToolResult::from(outcome))
            .expect("tool result should serialize");

        assert_eq!(value["structuredContent"]["outcome"], "conflict");
        assert_eq!(value["structuredContent"]["error_code"], "intent_stale");
        assert_eq!(
            value["structuredContent"]["details"][0]["dimension"],
            "policy_version"
        );
        assert_eq!(
            value["structuredContent"]["details"][0]["detail"],
            "policy version changed since preview"
        );
    }

    #[test]
    fn intent_error_mapping_uses_stable_codes() {
        let cases = [
            (
                MutationIntentTokenLookupError::SchemaVersionStale,
                "conflict",
                "intent_stale",
            ),
            (
                MutationIntentTokenLookupError::OperationMismatch,
                "conflict",
                "intent_mismatch",
            ),
            (
                MutationIntentTokenLookupError::Unauthorized,
                "denied",
                "denied_policy",
            ),
            (
                MutationIntentTokenLookupError::Expired,
                "conflict",
                "expired_token",
            ),
            (
                MutationIntentTokenLookupError::Rejected,
                "conflict",
                "rejected_intent",
            ),
        ];

        for (error, outcome, code) in cases {
            let value =
                serde_json::to_value(McpMutationIntentOutcome::from_token_lookup_error(error))
                    .expect("outcome should serialize");
            assert_eq!(value["outcome"], outcome);
            assert_eq!(value["error_code"], code);
        }
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
        let resp = response_json(&mut server, req);
        assert!(resp["result"]["capabilities"]["resources"]["subscribe"]
            .as_bool()
            .expect("initialize response should advertise resource subscribe support"));
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
        let resp = response_json(&mut server, req);
        assert!(
            resp["result"]["subscriptionId"]
                .as_u64()
                .expect("subscribe response should include a numeric subscription id")
                > 0
        );
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
