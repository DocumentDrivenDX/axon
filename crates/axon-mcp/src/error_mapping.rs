use axon_core::error::AxonError;

use crate::protocol::McpError;

/// Collapse Axon's richer error taxonomy into the MCP protocol's narrower
/// client-visible categories while preserving caller-actionable distinctions.
pub fn map_axon_error(error: AxonError) -> McpError {
    match error {
        AxonError::NotFound(message) => McpError::NotFound(message),
        AxonError::SchemaValidation(message)
        | AxonError::InvalidArgument(message)
        | AxonError::InvalidOperation(message)
        | AxonError::AlreadyExists(message) => McpError::InvalidParams(message),
        AxonError::UniqueViolation { field, value } => {
            McpError::InvalidParams(format!("unique violation on {field}: {value}"))
        }
        AxonError::ConflictingVersion {
            expected, actual, ..
        } => McpError::InvalidParams(format!(
            "version conflict: expected {expected}, actual {actual}"
        )),
        AxonError::Storage(message) => McpError::Internal(message),
        AxonError::Serialization(error) => McpError::Internal(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::map_axon_error;
    use crate::prompts::get_prompt_from_handler;
    use crate::protocol::McpError;
    use crate::resources::read_resource_from_handler;
    use axon_api::handler::AxonHandler;
    use axon_core::error::AxonError;
    use axon_core::id::{CollectionId, EntityId};
    use axon_core::types::Entity;
    use axon_storage::adapter::StorageAdapter;
    use axon_storage::memory::MemoryStorageAdapter;
    use serde_json::json;

    #[derive(Default)]
    struct FailOnGetAdapter {
        inner: MemoryStorageAdapter,
    }

    impl StorageAdapter for FailOnGetAdapter {
        fn get(
            &self,
            _collection: &CollectionId,
            _id: &EntityId,
        ) -> Result<Option<Entity>, AxonError> {
            Err(AxonError::Storage("simulated get failure".into()))
        }

        fn put(&mut self, entity: Entity) -> Result<(), AxonError> {
            self.inner.put(entity)
        }

        fn delete(&mut self, collection: &CollectionId, id: &EntityId) -> Result<(), AxonError> {
            self.inner.delete(collection, id)
        }

        fn count(&self, collection: &CollectionId) -> Result<usize, AxonError> {
            self.inner.count(collection)
        }

        fn range_scan(
            &self,
            collection: &CollectionId,
            start: Option<&EntityId>,
            end: Option<&EntityId>,
            limit: Option<usize>,
        ) -> Result<Vec<Entity>, AxonError> {
            self.inner.range_scan(collection, start, end, limit)
        }

        fn compare_and_swap(
            &mut self,
            entity: Entity,
            expected_version: u64,
        ) -> Result<Entity, AxonError> {
            self.inner.compare_and_swap(entity, expected_version)
        }
    }

    #[test]
    fn map_axon_error_preserves_mcp_categories() {
        assert!(matches!(
            map_axon_error(AxonError::NotFound("missing".into())),
            McpError::NotFound(message) if message == "missing"
        ));
        assert!(matches!(
            map_axon_error(AxonError::SchemaValidation("bad schema".into())),
            McpError::InvalidParams(message) if message == "bad schema"
        ));
        assert!(matches!(
            map_axon_error(AxonError::InvalidOperation("bad state".into())),
            McpError::InvalidParams(message) if message == "bad state"
        ));
        assert!(matches!(
            map_axon_error(AxonError::Storage("disk offline".into())),
            McpError::Internal(message) if message == "disk offline"
        ));
    }

    #[test]
    fn entity_resource_preserves_internal_failures() {
        let handler = AxonHandler::new(FailOnGetAdapter::default());
        let error = read_resource_from_handler(&handler, "default", "axon://tasks/task-1")
            .expect_err("resource read should surface storage failures");

        assert!(matches!(
            error,
            McpError::Internal(message) if message == "simulated get failure"
        ));
    }

    #[test]
    fn dependency_prompt_preserves_internal_failures() {
        let handler = AxonHandler::new(FailOnGetAdapter::default());
        let error = get_prompt_from_handler(
            &handler,
            "default",
            "axon.dependency_analysis",
            &json!({
                "collection": "tasks",
                "id": "task-1",
            }),
        )
        .expect_err("prompt generation should surface storage failures");

        assert!(matches!(
            error,
            McpError::Internal(message) if message == "simulated get failure"
        ));
    }
}
