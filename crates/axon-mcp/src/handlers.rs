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
}
