use std::collections::{HashMap, HashSet, VecDeque};

use axon_audit::entry::{AuditEntry, MutationType};
use axon_audit::log::{AuditLog, MemoryAuditLog};
use axon_core::error::AxonError;
use axon_core::id::CollectionId;
use axon_core::types::{Entity, Link};
use axon_schema::schema::CollectionSchema;
use axon_schema::validation::validate;
use axon_storage::adapter::StorageAdapter;

use crate::request::{
    CreateEntityRequest, CreateLinkRequest, DeleteEntityRequest, GetEntityRequest, TraverseRequest,
    UpdateEntityRequest,
};
use crate::response::{
    CreateEntityResponse, CreateLinkResponse, DeleteEntityResponse, GetEntityResponse,
    TraverseResponse, UpdateEntityResponse,
};

const DEFAULT_MAX_DEPTH: usize = 3;
const MAX_DEPTH_CAP: usize = 10;

/// Core API handler: coordinates storage, schema validation, and audit.
///
/// Holds an in-memory audit log and a per-collection schema registry.
/// Swap `S` for any [`StorageAdapter`] implementation (in-memory or SQLite).
pub struct AxonHandler<S: StorageAdapter> {
    storage: S,
    audit: MemoryAuditLog,
    schemas: HashMap<CollectionId, CollectionSchema>,
}

impl<S: StorageAdapter> AxonHandler<S> {
    pub fn new(storage: S) -> Self {
        Self {
            storage,
            audit: MemoryAuditLog::default(),
            schemas: HashMap::new(),
        }
    }

    /// Register a schema for a collection. Subsequent creates and updates
    /// for that collection will be validated against this schema.
    pub fn register_schema(&mut self, schema: CollectionSchema) {
        self.schemas.insert(schema.collection.clone(), schema);
    }

    /// Returns a reference to the internal audit log (useful in tests).
    pub fn audit_log(&self) -> &MemoryAuditLog {
        &self.audit
    }

    /// Mutable access to the underlying storage adapter (used by simulation framework).
    pub fn storage_mut(&mut self) -> &mut S {
        &mut self.storage
    }

    /// Commits a [`Transaction`] through this handler's storage and audit log.
    pub fn commit_transaction(
        &mut self,
        tx: crate::transaction::Transaction,
        actor: Option<String>,
    ) -> Result<Vec<axon_core::types::Entity>, AxonError> {
        tx.commit(&mut self.storage, &mut self.audit, actor)
    }

    // ── Entity operations ────────────────────────────────────────────────────

    pub fn create_entity(
        &mut self,
        req: CreateEntityRequest,
    ) -> Result<CreateEntityResponse, AxonError> {
        // Schema validation.
        if let Some(schema) = self.schemas.get(&req.collection) {
            validate(schema, &req.data)?;
        }

        let entity = Entity::new(req.collection, req.id, req.data);
        self.storage.put(entity.clone())?;

        // Audit.
        self.audit.append(AuditEntry::new(
            entity.collection.clone(),
            entity.id.clone(),
            entity.version,
            MutationType::Create,
            None,
            Some(entity.data.clone()),
            req.actor,
        ))?;

        Ok(CreateEntityResponse { entity })
    }

    pub fn get_entity(&self, req: GetEntityRequest) -> Result<GetEntityResponse, AxonError> {
        match self.storage.get(&req.collection, &req.id)? {
            Some(entity) => Ok(GetEntityResponse { entity }),
            None => Err(AxonError::NotFound(req.id.to_string())),
        }
    }

    /// Update an entity using optimistic concurrency control (OCC).
    ///
    /// Fails with [`AxonError::ConflictingVersion`] if `expected_version`
    /// does not match the current stored version.
    pub fn update_entity(
        &mut self,
        req: UpdateEntityRequest,
    ) -> Result<UpdateEntityResponse, AxonError> {
        // Schema validation.
        if let Some(schema) = self.schemas.get(&req.collection) {
            validate(schema, &req.data)?;
        }

        // Read current state for the audit `before` snapshot.
        let before = self
            .storage
            .get(&req.collection, &req.id)?
            .map(|e| e.data.clone());

        // OCC write.
        let candidate = Entity {
            collection: req.collection,
            id: req.id,
            version: req.expected_version, // compare_and_swap bumps this to +1
            data: req.data,
        };
        let updated = self
            .storage
            .compare_and_swap(candidate, req.expected_version)?;

        // Audit.
        self.audit.append(AuditEntry::new(
            updated.collection.clone(),
            updated.id.clone(),
            updated.version,
            MutationType::Update,
            before,
            Some(updated.data.clone()),
            req.actor,
        ))?;

        Ok(UpdateEntityResponse { entity: updated })
    }

    pub fn delete_entity(
        &mut self,
        req: DeleteEntityRequest,
    ) -> Result<DeleteEntityResponse, AxonError> {
        // Read current state for the audit `before` snapshot.
        let before = self
            .storage
            .get(&req.collection, &req.id)?
            .map(|e| e.data.clone());

        let version = self
            .storage
            .get(&req.collection, &req.id)?
            .map(|e| e.version)
            .unwrap_or(0);

        self.storage.delete(&req.collection, &req.id)?;

        // Audit (only if the entity actually existed).
        if before.is_some() {
            self.audit.append(AuditEntry::new(
                req.collection.clone(),
                req.id.clone(),
                version,
                MutationType::Delete,
                before,
                None,
                req.actor,
            ))?;
        }

        Ok(DeleteEntityResponse {
            collection: req.collection.to_string(),
            id: req.id.to_string(),
        })
    }

    // ── Link operations ──────────────────────────────────────────────────────

    /// Create a typed link from one entity to another.
    ///
    /// Both source and target must exist in storage; if either is missing,
    /// [`AxonError::NotFound`] is returned.
    pub fn create_link(&mut self, req: CreateLinkRequest) -> Result<CreateLinkResponse, AxonError> {
        // Verify source and target exist.
        if self
            .storage
            .get(&req.source_collection, &req.source_id)?
            .is_none()
        {
            return Err(AxonError::NotFound(format!(
                "source entity {}/{}",
                req.source_collection, req.source_id
            )));
        }
        if self
            .storage
            .get(&req.target_collection, &req.target_id)?
            .is_none()
        {
            return Err(AxonError::NotFound(format!(
                "target entity {}/{}",
                req.target_collection, req.target_id
            )));
        }

        let link = Link {
            source_collection: req.source_collection,
            source_id: req.source_id,
            target_collection: req.target_collection,
            target_id: req.target_id,
            link_type: req.link_type,
            metadata: req.metadata,
        };

        // Store the link as an entity in the internal links collection.
        self.storage.put(link.to_entity())?;

        Ok(CreateLinkResponse { link })
    }

    /// Traverse links from a starting entity using BFS up to `max_depth` hops.
    ///
    /// Returns all reachable entities (excluding the starting entity itself)
    /// in BFS order. Cycles are detected and each entity is visited at most once.
    pub fn traverse(&self, req: TraverseRequest) -> Result<TraverseResponse, AxonError> {
        let max_depth = req
            .max_depth
            .unwrap_or(DEFAULT_MAX_DEPTH)
            .min(MAX_DEPTH_CAP);

        // Load all links once and index them by (source_collection, source_id).
        let all_links = self.load_all_links()?;

        let mut visited: HashSet<(String, String)> = HashSet::new();
        let start_key = (req.collection.to_string(), req.id.to_string());
        visited.insert(start_key);

        // Queue entries: (collection, id, current_depth)
        let mut queue: VecDeque<(CollectionId, axon_core::id::EntityId, usize)> = VecDeque::new();
        queue.push_back((req.collection, req.id, 0));

        let mut result = Vec::new();

        while let Some((col, id, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }

            let neighbors = all_links
                .iter()
                .filter(|l| {
                    l.source_collection == col
                        && l.source_id == id
                        && req
                            .link_type
                            .as_deref()
                            .map_or(true, |lt| l.link_type == lt)
                })
                .collect::<Vec<_>>();

            for link in neighbors {
                let neighbor_key = (
                    link.target_collection.to_string(),
                    link.target_id.to_string(),
                );
                if visited.contains(&neighbor_key) {
                    continue;
                }
                visited.insert(neighbor_key);

                if let Some(entity) = self.storage.get(&link.target_collection, &link.target_id)? {
                    result.push(entity);
                    queue.push_back((
                        link.target_collection.clone(),
                        link.target_id.clone(),
                        depth + 1,
                    ));
                }
            }
        }

        Ok(TraverseResponse { entities: result })
    }

    /// Load all stored links from the internal links collection.
    fn load_all_links(&self) -> Result<Vec<Link>, AxonError> {
        let links_col = Link::links_collection();
        let entities = self.storage.range_scan(&links_col, None, None, None)?;
        Ok(entities.iter().filter_map(Link::from_entity).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon_core::id::{CollectionId, EntityId};
    use axon_schema::schema::EsfDocument;
    use axon_storage::memory::MemoryStorageAdapter;
    use serde_json::json;

    fn handler() -> AxonHandler<MemoryStorageAdapter> {
        AxonHandler::new(MemoryStorageAdapter::default())
    }

    // ── Entity CRUD ──────────────────────────────────────────────────────────

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

    #[test]
    fn update_entity_increments_version() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v1"}),
            actor: None,
        })
        .unwrap();

        let updated = h
            .update_entity(UpdateEntityRequest {
                collection: col.clone(),
                id: id.clone(),
                data: json!({"title": "v2"}),
                expected_version: 1,
                actor: None,
            })
            .unwrap();

        assert_eq!(updated.entity.version, 2);
        assert_eq!(updated.entity.data["title"], "v2");
    }

    #[test]
    fn occ_rejects_stale_version() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v1"}),
            actor: None,
        })
        .unwrap();

        let err = h
            .update_entity(UpdateEntityRequest {
                collection: col,
                id,
                data: json!({"title": "v2"}),
                expected_version: 99, // wrong version
                actor: None,
            })
            .unwrap_err();

        assert!(
            matches!(
                err,
                AxonError::ConflictingVersion {
                    expected: 99,
                    actual: 1
                }
            ),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn delete_entity_removes_it() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "to-delete"}),
            actor: None,
        })
        .unwrap();

        h.delete_entity(DeleteEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            actor: None,
        })
        .unwrap();

        let result = h.get_entity(GetEntityRequest {
            collection: col,
            id,
        });
        assert!(matches!(result, Err(AxonError::NotFound(_))));
    }

    #[test]
    fn create_update_delete_produce_audit_entries() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v1"}),
            actor: Some("agent-1".into()),
        })
        .unwrap();

        h.update_entity(UpdateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v2"}),
            expected_version: 1,
            actor: Some("agent-1".into()),
        })
        .unwrap();

        h.delete_entity(DeleteEntityRequest {
            collection: col,
            id,
            actor: None,
        })
        .unwrap();

        assert_eq!(
            h.audit_log().len(),
            3,
            "expected 3 audit entries (create/update/delete)"
        );
    }

    // ── Schema validation ────────────────────────────────────────────────────

    const TASK_ESF: &str = r#"
esf_version: "1.0"
collection: tasks
entity_schema:
  type: object
  required: [title]
  properties:
    title:
      type: string
    done:
      type: boolean
"#;

    #[test]
    fn schema_validation_rejects_invalid_write() {
        let mut h = handler();
        let schema = EsfDocument::parse(TASK_ESF)
            .unwrap()
            .into_collection_schema();
        h.register_schema(schema);

        // Missing required "title" field.
        let err = h
            .create_entity(CreateEntityRequest {
                collection: CollectionId::new("tasks"),
                id: EntityId::new("t-001"),
                data: json!({"done": false}),
                actor: None,
            })
            .unwrap_err();

        assert!(
            matches!(err, AxonError::SchemaValidation(_)),
            "expected SchemaValidation error, got: {err}"
        );
    }

    #[test]
    fn schema_validation_accepts_valid_write() {
        let mut h = handler();
        let schema = EsfDocument::parse(TASK_ESF)
            .unwrap()
            .into_collection_schema();
        h.register_schema(schema);

        let result = h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("tasks"),
            id: EntityId::new("t-001"),
            data: json!({"title": "My task", "done": false}),
            actor: None,
        });

        assert!(result.is_ok(), "valid entity should be accepted");
    }

    // ── Link operations ──────────────────────────────────────────────────────

    fn make_entity(h: &mut AxonHandler<MemoryStorageAdapter>, col: &str, id: &str) {
        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new(col),
            id: EntityId::new(id),
            data: json!({"name": id}),
            actor: None,
        })
        .unwrap();
    }

    #[test]
    fn link_creation_between_entities() {
        let mut h = handler();
        make_entity(&mut h, "users", "u-001");
        make_entity(&mut h, "tasks", "t-001");

        let resp = h
            .create_link(CreateLinkRequest {
                source_collection: CollectionId::new("users"),
                source_id: EntityId::new("u-001"),
                target_collection: CollectionId::new("tasks"),
                target_id: EntityId::new("t-001"),
                link_type: "assigned-to".into(),
                metadata: json!(null),
                actor: None,
            })
            .unwrap();

        assert_eq!(resp.link.link_type, "assigned-to");
    }

    #[test]
    fn link_to_missing_entity_fails() {
        let mut h = handler();
        make_entity(&mut h, "users", "u-001");

        let err = h
            .create_link(CreateLinkRequest {
                source_collection: CollectionId::new("users"),
                source_id: EntityId::new("u-001"),
                target_collection: CollectionId::new("tasks"),
                target_id: EntityId::new("ghost"),
                link_type: "assigned-to".into(),
                metadata: json!(null),
                actor: None,
            })
            .unwrap_err();

        assert!(matches!(err, AxonError::NotFound(_)));
    }

    #[test]
    fn traversal_follows_links_to_depth_3() {
        let mut h = handler();
        // Chain: a -> b -> c -> d (depth 3 from a reaches d)
        for name in ["a", "b", "c", "d"] {
            make_entity(&mut h, "nodes", name);
        }
        for (src, tgt) in [("a", "b"), ("b", "c"), ("c", "d")] {
            h.create_link(CreateLinkRequest {
                source_collection: CollectionId::new("nodes"),
                source_id: EntityId::new(src),
                target_collection: CollectionId::new("nodes"),
                target_id: EntityId::new(tgt),
                link_type: "next".into(),
                metadata: json!(null),
                actor: None,
            })
            .unwrap();
        }

        let resp = h
            .traverse(TraverseRequest {
                collection: CollectionId::new("nodes"),
                id: EntityId::new("a"),
                link_type: Some("next".into()),
                max_depth: Some(3),
            })
            .unwrap();

        let ids: Vec<_> = resp.entities.iter().map(|e| e.id.as_str()).collect();
        assert!(ids.contains(&"b"), "should reach b");
        assert!(ids.contains(&"c"), "should reach c");
        assert!(ids.contains(&"d"), "should reach d at depth 3");
    }

    #[test]
    fn traversal_does_not_revisit_cycles() {
        let mut h = handler();
        // Ring: a -> b -> a
        for name in ["a", "b"] {
            make_entity(&mut h, "nodes", name);
        }
        for (src, tgt) in [("a", "b"), ("b", "a")] {
            h.create_link(CreateLinkRequest {
                source_collection: CollectionId::new("nodes"),
                source_id: EntityId::new(src),
                target_collection: CollectionId::new("nodes"),
                target_id: EntityId::new(tgt),
                link_type: "edge".into(),
                metadata: json!(null),
                actor: None,
            })
            .unwrap();
        }

        let resp = h
            .traverse(TraverseRequest {
                collection: CollectionId::new("nodes"),
                id: EntityId::new("a"),
                link_type: None,
                max_depth: Some(5),
            })
            .unwrap();

        // Should only see "b" (not "a" again, not infinite loop)
        assert_eq!(resp.entities.len(), 1);
        assert_eq!(resp.entities[0].id.as_str(), "b");
    }
}
