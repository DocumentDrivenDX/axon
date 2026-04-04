use axon_core::error::AxonError;
use axon_core::types::Entity;
use axon_storage::adapter::StorageAdapter;

use crate::request::{CreateEntityRequest, GetEntityRequest};
use crate::response::{CreateEntityResponse, GetEntityResponse};

/// Core API handler: coordinates storage, schema validation, and audit.
///
/// In V1 this is a thin coordinator stub. Follow-on issues add schema
/// validation, audit emission, and OCC transaction logic.
pub struct AxonHandler<S: StorageAdapter> {
    storage: S,
}

impl<S: StorageAdapter> AxonHandler<S> {
    pub fn new(storage: S) -> Self {
        Self { storage }
    }

    pub fn create_entity(
        &mut self,
        req: CreateEntityRequest,
    ) -> Result<CreateEntityResponse, AxonError> {
        let entity = Entity::new(req.collection, req.id, req.data);
        self.storage.put(entity.clone())?;
        Ok(CreateEntityResponse { entity })
    }

    pub fn get_entity(&self, req: GetEntityRequest) -> Result<GetEntityResponse, AxonError> {
        match self.storage.get(&req.collection, &req.id)? {
            Some(entity) => Ok(GetEntityResponse { entity }),
            None => Err(AxonError::NotFound(req.id.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon_core::id::{CollectionId, EntityId};
    use axon_storage::memory::MemoryStorageAdapter;
    use serde_json::json;

    fn handler() -> AxonHandler<MemoryStorageAdapter> {
        AxonHandler::new(MemoryStorageAdapter::default())
    }

    #[test]
    fn create_then_get_roundtrip() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        let created = h
            .create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: id.clone(),
                data: json!({"title": "hello"}),
                actor: None,
            })
            .unwrap();
        assert_eq!(created.entity.version, 1);

        let fetched = h
            .get_entity(GetEntityRequest {
                collection: col,
                id,
            })
            .unwrap();
        assert_eq!(fetched.entity.data["title"], "hello");
    }

    #[test]
    fn get_missing_entity_returns_not_found() {
        let h = handler();
        let result = h.get_entity(GetEntityRequest {
            collection: CollectionId::new("tasks"),
            id: EntityId::new("missing"),
        });
        assert!(matches!(result, Err(AxonError::NotFound(_))));
    }
}
