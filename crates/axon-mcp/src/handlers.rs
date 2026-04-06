//! MCP tool handlers wired to the Axon API layer.
//!
//! Each handler function creates a [`ToolDef`] that dispatches to the
//! appropriate `AxonHandler` method via a shared `Arc<Mutex<AxonHandler>>`.
//!
//! Uses `std::sync::Mutex` since all `AxonHandler` methods are synchronous.

use std::sync::{Arc, Mutex};

use axon_api::handler::AxonHandler;
use axon_api::request::{
    CreateEntityRequest, DeleteEntityRequest, GetEntityRequest, UpdateEntityRequest,
};
use axon_core::id::{CollectionId, EntityId};
use axon_storage::adapter::StorageAdapter;

use crate::tools::{ToolDef, ToolError};

/// Build CRUD tools for a collection, wired to a shared handler.
///
/// Returns tool definitions for `{collection}.create`, `.get`, `.patch`, `.delete`.
pub fn build_crud_tools<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<Mutex<AxonHandler<S>>>,
) -> Vec<ToolDef> {
    let col = collection.to_string();
    vec![
        build_create_tool(&col, Arc::clone(&handler)),
        build_get_tool(&col, Arc::clone(&handler)),
        build_patch_tool(&col, Arc::clone(&handler)),
        build_delete_tool(&col, handler),
    ]
}

fn lock_handler<S: StorageAdapter>(handler: &Mutex<AxonHandler<S>>) -> Result<std::sync::MutexGuard<'_, AxonHandler<S>>, ToolError> {
    handler
        .lock()
        .map_err(|e| ToolError::Internal(format!("mutex poisoned: {e}")))
}

fn build_create_tool<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<Mutex<AxonHandler<S>>>,
) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
        name: format!("{col}.create"),
        description: format!("Create a new entity in the {col} collection"),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Entity ID (optional, auto-generated UUIDv7 if omitted)" },
                "data": { "type": "object", "description": "Entity data" }
            },
            "required": ["data"]
        }),
        handler: Box::new(move |args| {
            let id_str = args
                .get("id")
                .and_then(|v| v.as_str())
                .map(EntityId::new)
                .unwrap_or_else(EntityId::generate);
            let data = args
                .get("data")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));

            let mut guard = lock_handler(&handler)?;
            let result = guard.create_entity(CreateEntityRequest {
                collection: CollectionId::new(&col),
                id: id_str,
                data,
                actor: Some("mcp".into()),
                audit_metadata: None,
            });

            match result {
                Ok(resp) => Ok(serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": serde_json::to_string(&resp.entity).unwrap_or_default()
                    }]
                })),
                Err(e) => Err(to_tool_error(e)),
            }
        }),
    }
}

fn build_get_tool<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<Mutex<AxonHandler<S>>>,
) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
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
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::InvalidArgument("missing 'id'".into()))?;

            let guard = lock_handler(&handler)?;
            let result = guard.get_entity(GetEntityRequest {
                collection: CollectionId::new(&col),
                id: EntityId::new(id),
            });

            match result {
                Ok(resp) => Ok(serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": serde_json::to_string(&resp.entity).unwrap_or_default()
                    }]
                })),
                Err(e) => Err(to_tool_error(e)),
            }
        }),
    }
}

fn build_patch_tool<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<Mutex<AxonHandler<S>>>,
) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
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
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::InvalidArgument("missing 'id'".into()))?;
            let data = args
                .get("data")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            let expected_version = args
                .get("expected_version")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| ToolError::InvalidArgument("missing 'expected_version'".into()))?;

            let mut guard = lock_handler(&handler)?;
            let result = guard.update_entity(UpdateEntityRequest {
                collection: CollectionId::new(&col),
                id: EntityId::new(id),
                data,
                expected_version,
                actor: Some("mcp".into()),
                audit_metadata: None,
            });

            match result {
                Ok(resp) => Ok(serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": serde_json::to_string(&resp.entity).unwrap_or_default()
                    }]
                })),
                Err(e) => Err(to_tool_error(e)),
            }
        }),
    }
}

fn build_delete_tool<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<Mutex<AxonHandler<S>>>,
) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
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
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::InvalidArgument("missing 'id'".into()))?;

            let mut guard = lock_handler(&handler)?;
            let result = guard.delete_entity(DeleteEntityRequest {
                collection: CollectionId::new(&col),
                id: EntityId::new(id),
                actor: Some("mcp".into()),
                audit_metadata: None,
                force: false,
            });

            match result {
                Ok(resp) => Ok(serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": format!("Deleted {}/{}", resp.collection, resp.id)
                    }]
                })),
                Err(e) => Err(to_tool_error(e)),
            }
        }),
    }
}

/// Build the `axon.query` tool for GraphQL queries via MCP.
///
/// Accepts a `query` string and optional `variables` object. Validates
/// basic GraphQL syntax (brace matching) before forwarding.
pub fn build_query_tool() -> ToolDef {
    ToolDef {
        name: "axon.query".into(),
        description: "Execute a GraphQL query or mutation against Axon. Accepts a query string and optional variables.".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "GraphQL query or mutation string"
                },
                "variables": {
                    "type": "object",
                    "description": "Optional variables for the query"
                }
            },
            "required": ["query"]
        }),
        handler: Box::new(|args| {
            let query = args
                .get("query")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::InvalidArgument("missing 'query' string".into()))?;

            // Basic syntax validation: check brace balance.
            let open = query.chars().filter(|&c| c == '{').count();
            let close = query.chars().filter(|&c| c == '}').count();
            if open != close {
                return Err(ToolError::InvalidArgument(format!(
                    "GraphQL syntax error: mismatched braces ({{={open}, }}={close})"
                )));
            }

            if query.trim().is_empty() {
                return Err(ToolError::InvalidArgument(
                    "empty query string".into(),
                ));
            }

            let variables = args
                .get("variables")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));

            // In the full implementation, this would execute against the
            // async-graphql schema. For now, return a stub response that
            // confirms the query was parsed and accepted.
            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": serde_json::json!({
                        "data": null,
                        "info": "GraphQL execution stub — query accepted",
                        "query": query,
                        "variables": variables
                    }).to_string()
                }]
            }))
        }),
    }
}

/// Build a `{collection}.aggregate` tool for MCP.
///
/// Accepts structured aggregation requests: function, field, optional filter and group_by.
pub fn build_aggregate_tool(collection: &str) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
        name: format!("{col}.aggregate"),
        description: format!("Run an aggregation query on the {col} collection"),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "function": {
                    "type": "string",
                    "enum": ["count", "sum", "avg", "min", "max"],
                    "description": "Aggregation function"
                },
                "field": {
                    "type": "string",
                    "description": "Field to aggregate"
                },
                "filter": {
                    "type": "object",
                    "description": "Optional filter to restrict entities"
                },
                "group_by": {
                    "type": "string",
                    "description": "Optional field to group results by"
                }
            },
            "required": ["function", "field"]
        }),
        handler: Box::new(move |args| {
            let function = args
                .get("function")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::InvalidArgument("missing 'function'".into()))?;
            let field = args
                .get("field")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::InvalidArgument("missing 'field'".into()))?;

            // Validate function name.
            match function {
                "count" | "sum" | "avg" | "min" | "max" => {}
                other => {
                    return Err(ToolError::InvalidArgument(format!(
                        "unknown aggregation function: {other}"
                    )));
                }
            }

            // Stub response — in full implementation, this would call
            // AxonHandler::aggregate_entities.
            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": serde_json::json!({
                        "collection": col,
                        "function": function,
                        "field": field,
                        "result": null,
                        "info": "aggregation stub — handler integration pending"
                    }).to_string()
                }]
            }))
        }),
    }
}

/// Build `{collection}.link_candidates` tool.
///
/// Returns possible link types for a collection (from schema link definitions).
pub fn build_link_candidates_tool(collection: &str) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
        name: format!("{col}.link_candidates"),
        description: format!("Discover available link types for entities in the {col} collection"),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Entity ID to find link candidates for" }
            },
            "required": ["id"]
        }),
        handler: Box::new(move |args| {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::InvalidArgument("missing 'id'".into()))?;

            // Stub: return empty candidates list.
            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": serde_json::json!({
                        "collection": col,
                        "entity_id": id,
                        "candidates": [],
                        "info": "link candidates stub — schema integration pending"
                    }).to_string()
                }]
            }))
        }),
    }
}

/// Build `{collection}.neighbors` tool.
///
/// Returns linked entities for a given entity.
pub fn build_neighbors_tool(collection: &str) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
        name: format!("{col}.neighbors"),
        description: format!("Find entities linked to a {col} entity"),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Entity ID" },
                "link_type": { "type": "string", "description": "Optional link type filter" },
                "direction": {
                    "type": "string",
                    "enum": ["outbound", "inbound", "both"],
                    "description": "Link direction (default: both)"
                }
            },
            "required": ["id"]
        }),
        handler: Box::new(move |args| {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::InvalidArgument("missing 'id'".into()))?;
            let direction = args
                .get("direction")
                .and_then(|v| v.as_str())
                .unwrap_or("both");

            // Stub response
            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": serde_json::json!({
                        "collection": col,
                        "entity_id": id,
                        "direction": direction,
                        "neighbors": [],
                        "info": "neighbors stub — handler integration pending"
                    }).to_string()
                }]
            }))
        }),
    }
}

fn to_tool_error(err: axon_core::error::AxonError) -> ToolError {
    use axon_core::error::AxonError;
    match err {
        AxonError::NotFound(msg) => ToolError::NotFound(msg),
        AxonError::ConflictingVersion {
            expected,
            actual,
            current_entity,
        } => {
            let entity_json = current_entity
                .as_ref()
                .and_then(|e| serde_json::to_string(e).ok())
                .unwrap_or_else(|| "null".to_string());
            ToolError::Conflict(format!(
                "version conflict: expected {expected}, actual {actual}, current_entity: {entity_json}"
            ))
        }
        AxonError::InvalidArgument(msg) | AxonError::InvalidOperation(msg) => {
            ToolError::InvalidArgument(msg)
        }
        AxonError::SchemaValidation(msg) => ToolError::InvalidArgument(msg),
        AxonError::AlreadyExists(msg) => ToolError::Conflict(msg),
        AxonError::UniqueViolation { field, value } => {
            ToolError::Conflict(format!("unique violation on {field}: {value}"))
        }
        AxonError::Storage(msg) => ToolError::Internal(msg),
        AxonError::Serialization(e) => ToolError::Internal(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon_api::handler::AxonHandler;
    use axon_storage::memory::MemoryStorageAdapter;
    use serde_json::Value;

    fn make_handler() -> Arc<Mutex<AxonHandler<MemoryStorageAdapter>>> {
        Arc::new(Mutex::new(AxonHandler::new(
            MemoryStorageAdapter::default(),
        )))
    }

    #[test]
    fn create_and_get_via_mcp_tools() {
        let handler = make_handler();
        let tools = build_crud_tools("tasks", Arc::clone(&handler));

        // Create
        let create_tool = &tools[0];
        assert_eq!(create_tool.name, "tasks.create");
        let result = (create_tool.handler)(&serde_json::json!({
            "id": "t-001",
            "data": {"title": "Test task"}
        }))
        .unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let entity: Value = serde_json::from_str(text).unwrap();
        assert_eq!(entity["id"], "t-001");
        assert_eq!(entity["data"]["title"], "Test task");

        // Get
        let get_tool = &tools[1];
        assert_eq!(get_tool.name, "tasks.get");
        let result = (get_tool.handler)(&serde_json::json!({"id": "t-001"})).unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let entity: Value = serde_json::from_str(text).unwrap();
        assert_eq!(entity["data"]["title"], "Test task");
    }

    #[test]
    fn patch_via_mcp_tools() {
        let handler = make_handler();
        let tools = build_crud_tools("tasks", Arc::clone(&handler));

        // Create first
        (tools[0].handler)(&serde_json::json!({
            "id": "t-001",
            "data": {"title": "Original"}
        }))
        .unwrap();

        // Patch
        let patch_tool = &tools[2];
        let result = (patch_tool.handler)(&serde_json::json!({
            "id": "t-001",
            "data": {"title": "Updated"},
            "expected_version": 1
        }))
        .unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let entity: Value = serde_json::from_str(text).unwrap();
        assert_eq!(entity["data"]["title"], "Updated");
        assert_eq!(entity["version"], 2);
    }

    #[test]
    fn delete_via_mcp_tools() {
        let handler = make_handler();
        let tools = build_crud_tools("tasks", Arc::clone(&handler));

        // Create
        (tools[0].handler)(&serde_json::json!({
            "id": "t-001",
            "data": {"title": "Delete me"}
        }))
        .unwrap();

        // Delete
        let delete_tool = &tools[3];
        let result = (delete_tool.handler)(&serde_json::json!({"id": "t-001"})).unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("t-001"));

        // Verify deleted
        let get_result = (tools[1].handler)(&serde_json::json!({"id": "t-001"}));
        assert!(get_result.is_err());
    }

    #[test]
    fn version_conflict_returns_current_entity() {
        let handler = make_handler();
        let tools = build_crud_tools("tasks", Arc::clone(&handler));

        // Create
        (tools[0].handler)(&serde_json::json!({
            "id": "t-001",
            "data": {"title": "V1"}
        }))
        .unwrap();

        // Patch with wrong version
        let err = (tools[2].handler)(&serde_json::json!({
            "id": "t-001",
            "data": {"title": "V2"},
            "expected_version": 99
        }))
        .unwrap_err();

        match err {
            ToolError::Conflict(msg) => {
                assert!(msg.contains("version conflict"));
                assert!(msg.contains("current_entity"));
            }
            other => panic!("expected Conflict, got: {other:?}"),
        }
    }

    #[test]
    fn missing_id_returns_invalid_argument() {
        let handler = make_handler();
        let tools = build_crud_tools("tasks", Arc::clone(&handler));

        let err = (tools[1].handler)(&serde_json::json!({})).unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    // ── axon.query tool tests ──────────────────────────────────────────

    #[test]
    fn query_tool_accepts_valid_graphql() {
        let tool = build_query_tool();
        assert_eq!(tool.name, "axon.query");
        let result = (tool.handler)(&serde_json::json!({
            "query": "{ tasks { id title } }"
        }))
        .unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["info"], "GraphQL execution stub — query accepted");
        assert!(parsed["query"].as_str().unwrap().contains("tasks"));
    }

    #[test]
    fn query_tool_rejects_mismatched_braces() {
        let tool = build_query_tool();
        let err = (tool.handler)(&serde_json::json!({
            "query": "{ tasks { id }"
        }))
        .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    #[test]
    fn query_tool_rejects_empty_query() {
        let tool = build_query_tool();
        let err = (tool.handler)(&serde_json::json!({
            "query": ""
        }))
        .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    #[test]
    fn query_tool_rejects_missing_query() {
        let tool = build_query_tool();
        let err = (tool.handler)(&serde_json::json!({})).unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    #[test]
    fn query_tool_passes_variables() {
        let tool = build_query_tool();
        let result = (tool.handler)(&serde_json::json!({
            "query": "query($id: ID!) { task(id: $id) { title } }",
            "variables": {"id": "t-001"}
        }))
        .unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["variables"]["id"], "t-001");
    }

    // ── aggregate tool tests ───────────────────────────────────────────

    #[test]
    fn aggregate_tool_accepts_valid_request() {
        let tool = build_aggregate_tool("tasks");
        assert_eq!(tool.name, "tasks.aggregate");
        let result = (tool.handler)(&serde_json::json!({
            "function": "count",
            "field": "status"
        }))
        .unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["function"], "count");
        assert_eq!(parsed["collection"], "tasks");
    }

    #[test]
    fn aggregate_tool_rejects_unknown_function() {
        let tool = build_aggregate_tool("tasks");
        let err = (tool.handler)(&serde_json::json!({
            "function": "median",
            "field": "x"
        }))
        .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    #[test]
    fn aggregate_tool_requires_function() {
        let tool = build_aggregate_tool("tasks");
        let err = (tool.handler)(&serde_json::json!({"field": "x"})).unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    // ── link tools tests ───────────────────────────────────────────────

    #[test]
    fn link_candidates_tool() {
        let tool = build_link_candidates_tool("tasks");
        assert_eq!(tool.name, "tasks.link_candidates");
        let result = (tool.handler)(&serde_json::json!({"id": "t-001"})).unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["entity_id"], "t-001");
    }

    #[test]
    fn neighbors_tool() {
        let tool = build_neighbors_tool("tasks");
        assert_eq!(tool.name, "tasks.neighbors");
        let result = (tool.handler)(&serde_json::json!({"id": "t-001"})).unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["direction"], "both");
    }

    #[test]
    fn neighbors_tool_with_direction() {
        let tool = build_neighbors_tool("tasks");
        let result = (tool.handler)(&serde_json::json!({
            "id": "t-001",
            "direction": "outbound"
        }))
        .unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["direction"], "outbound");
    }
}
