use std::collections::{HashSet, VecDeque};

use axon_audit::entry::{AuditEntry, MutationType};
use axon_audit::log::{AuditLog, AuditPage, AuditQuery, MemoryAuditLog};
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_core::types::{Entity, Link};
use axon_schema::schema::CollectionSchema;
use axon_schema::validation::{compile_entity_schema, validate, validate_link_metadata};
use axon_storage::adapter::StorageAdapter;

use crate::request::{
    CreateCollectionRequest, CreateEntityRequest, CreateLinkRequest, DeleteEntityRequest,
    DeleteLinkRequest, DescribeCollectionRequest, DropCollectionRequest, FieldFilter, FilterNode,
    FilterOp, GetEntityRequest, GetSchemaRequest, ListCollectionsRequest, PutSchemaRequest,
    QueryAuditRequest, QueryEntitiesRequest, RevertEntityRequest, SortDirection, TraverseRequest,
    UpdateEntityRequest,
};
use crate::response::{
    CollectionMetadata, CreateCollectionResponse, CreateEntityResponse, CreateLinkResponse,
    DeleteEntityResponse, DeleteLinkResponse, DescribeCollectionResponse, DropCollectionResponse,
    GetEntityResponse, GetSchemaResponse, ListCollectionsResponse, PutSchemaResponse,
    QueryAuditResponse, QueryEntitiesResponse, RevertEntityResponse, TraverseResponse,
    UpdateEntityResponse,
};

const DEFAULT_MAX_DEPTH: usize = 3;
const MAX_DEPTH_CAP: usize = 10;

/// Core API handler: coordinates storage, schema validation, and audit.
///
/// Schemas and collection registrations are persisted via the `StorageAdapter`;
/// there is no separate in-memory state. Swap `S` for any [`StorageAdapter`]
/// implementation.
pub struct AxonHandler<S: StorageAdapter> {
    storage: S,
    audit: MemoryAuditLog,
}

impl<S: StorageAdapter> AxonHandler<S> {
    pub fn new(storage: S) -> Self {
        Self {
            storage,
            audit: MemoryAuditLog::default(),
        }
    }

    /// Persist a schema for a collection via the storage adapter.
    ///
    /// Validates the `entity_schema` (if present) before persisting.
    /// Subsequent creates and updates for that collection are validated
    /// against this schema. Replaces any previously stored schema.
    pub fn put_schema(&mut self, schema: CollectionSchema) -> Result<(), AxonError> {
        if let Some(entity_schema) = &schema.entity_schema {
            compile_entity_schema(entity_schema)?;
        }
        self.storage.put_schema(&schema)
    }

    /// Retrieve the persisted schema for a collection, if one exists.
    pub fn get_schema(
        &self,
        collection: &CollectionId,
    ) -> Result<Option<CollectionSchema>, AxonError> {
        self.storage.get_schema(collection)
    }

    /// Register a schema for a collection. Subsequent creates and updates
    /// for that collection will be validated against this schema.
    ///
    /// Deprecated in favour of [`put_schema`]; kept for backwards compatibility
    /// in tests and simulation harness code. Panics on storage or validation errors.
    pub fn register_schema(&mut self, schema: CollectionSchema) {
        self.put_schema(schema)
            .expect("register_schema: storage or validation error");
    }

    /// Returns a reference to the internal audit log (useful in tests).
    pub fn audit_log(&self) -> &MemoryAuditLog {
        &self.audit
    }

    /// Mutable reference to the internal audit log (used by transaction tests).
    pub fn audit_log_mut(&mut self) -> &mut MemoryAuditLog {
        &mut self.audit
    }

    /// Mutable access to the underlying storage adapter (used by simulation framework).
    pub fn storage_mut(&mut self) -> &mut S {
        &mut self.storage
    }

    /// Mutable access to both storage and audit log for transaction commit.
    pub fn storage_and_audit_mut(&mut self) -> (&mut S, &mut MemoryAuditLog) {
        (&mut self.storage, &mut self.audit)
    }

    /// Consume this handler, returning the underlying storage adapter.
    ///
    /// Useful in tests that need to reconstruct a handler from the same storage
    /// to verify that persisted state (e.g. collection registrations) survives.
    pub fn into_storage(self) -> S {
        self.storage
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
        if let Some(schema) = self.storage.get_schema(&req.collection)? {
            validate(&schema, &req.data)?;
        }

        let entity = Entity::new(req.collection, req.id, req.data);
        self.storage.put(entity.clone())?;

        // Audit.
        self.audit.append(AuditEntry::new(
            entity.collection.clone(),
            entity.id.clone(),
            entity.version,
            MutationType::EntityCreate,
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
        if let Some(schema) = self.storage.get_schema(&req.collection)? {
            validate(&schema, &req.data)?;
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
            MutationType::EntityUpdate,
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
        // Referential integrity: reject delete when inbound links exist.
        //
        // Use the reverse-index collection to avoid an O(N_total_links) full
        // table scan.  Reverse-index IDs are formatted as
        // `{target_col}/{target_id}/{source_col}/{source_id}/{link_type}`, so a
        // prefix scan from `{target_col}/{target_id}/` with limit=1 is enough
        // to determine whether any inbound link exists.
        let links_rev_col = Link::links_rev_collection();
        let rev_prefix = format!("{}/{}/", req.collection, req.id);
        let rev_start = EntityId::new(&rev_prefix);
        let rev_candidates =
            self.storage
                .range_scan(&links_rev_col, Some(&rev_start), None, Some(1))?;
        let inbound_count = rev_candidates
            .iter()
            .filter(|e| e.id.as_str().starts_with(&rev_prefix))
            .count();
        if inbound_count > 0 {
            return Err(AxonError::InvalidOperation(format!(
                "entity {}/{} has inbound link(s); delete or re-target those links first",
                req.collection, req.id
            )));
        }

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
                MutationType::EntityDelete,
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

    // ── Entity query ─────────────────────────────────────────────────────────

    /// Query entities in a collection with optional filtering, sorting, and
    /// cursor-based pagination (US-011, FEAT-004).
    ///
    /// V1 uses a full sequential scan; secondary indexes are P1.
    pub fn query_entities(
        &self,
        req: QueryEntitiesRequest,
    ) -> Result<QueryEntitiesResponse, AxonError> {
        // Reject excessively deep filter trees before any evaluation to prevent
        // stack overflows from client-controlled recursion.
        if let Some(ref f) = req.filter {
            let depth = filter_depth(f);
            if depth > MAX_FILTER_DEPTH {
                return Err(AxonError::InvalidArgument(format!(
                    "filter tree depth {depth} exceeds maximum allowed depth {MAX_FILTER_DEPTH}"
                )));
            }
        }

        // Full scan — FEAT-004 notes secondary indexes are P1 for V1.
        let all = self.storage.range_scan(&req.collection, None, None, None)?;

        // Apply filter.
        let mut matched: Vec<Entity> = all
            .into_iter()
            .filter(|e| {
                req.filter
                    .as_ref()
                    .map_or(true, |f| apply_filter(f, &e.data))
            })
            .collect();

        // Sort before pagination so cursors are stable.
        if !req.sort.is_empty() {
            matched.sort_by(|a, b| {
                for sf in &req.sort {
                    let va = get_field_value(&a.data, &sf.field);
                    let vb = get_field_value(&b.data, &sf.field);
                    let cmp = compare_values(va, vb);
                    if cmp != std::cmp::Ordering::Equal {
                        return if sf.direction == SortDirection::Asc {
                            cmp
                        } else {
                            cmp.reverse()
                        };
                    }
                }
                std::cmp::Ordering::Equal
            });
        }

        let total_count = matched.len();

        // Cursor-based pagination: skip everything up to and including after_id.
        if let Some(ref cursor_id) = req.after_id {
            let pos = matched
                .iter()
                .position(|e| &e.id == cursor_id)
                .ok_or_else(|| {
                    AxonError::InvalidArgument(format!(
                        "cursor entity '{}' not found in result set",
                        cursor_id
                    ))
                })?;
            matched = matched.split_off(pos + 1);
        }

        // Apply limit.
        let limit = req.limit.unwrap_or(usize::MAX);
        let has_more = matched.len() > limit;
        if has_more {
            matched.truncate(limit);
        }

        let next_cursor = if has_more {
            matched.last().map(|e| e.id.to_string())
        } else {
            None
        };

        let entities = if req.count_only { vec![] } else { matched };

        Ok(QueryEntitiesResponse {
            entities,
            total_count,
            next_cursor,
        })
    }

    // ── Audit operations ─────────────────────────────────────────────────────

    /// Query the audit log with optional filters and cursor-based pagination.
    pub fn query_audit(&self, req: QueryAuditRequest) -> Result<QueryAuditResponse, AxonError> {
        use axon_audit::entry::MutationType as MT;

        let operation: Option<MT> = match req.operation.as_deref() {
            None => None,
            Some("entity.create") => Some(MT::EntityCreate),
            Some("entity.update") => Some(MT::EntityUpdate),
            Some("entity.delete") => Some(MT::EntityDelete),
            Some("entity.revert") => Some(MT::EntityRevert),
            Some("link.create") => Some(MT::LinkCreate),
            Some("link.delete") => Some(MT::LinkDelete),
            Some("collection.create") => Some(MT::CollectionCreate),
            Some("collection.drop") => Some(MT::CollectionDrop),
            Some("schema.update") => Some(MT::SchemaUpdate),
            Some(unknown) => {
                return Err(AxonError::InvalidOperation(format!(
                    "unknown operation type: {unknown}"
                )))
            }
        };

        let query = AuditQuery {
            collection: req.collection,
            entity_id: req.entity_id,
            actor: req.actor,
            operation,
            since_ns: req.since_ns,
            until_ns: req.until_ns,
            after_id: req.after_id,
            limit: req.limit,
        };

        let page: AuditPage = self.audit.query_paginated(query)?;
        Ok(QueryAuditResponse {
            entries: page.entries,
            next_cursor: page.next_cursor,
        })
    }

    /// Revert an entity to the `before` state recorded in the given audit entry.
    ///
    /// The revert itself produces a new audit entry tagged `EntityRevert` so
    /// the audit log never loses information.
    pub fn revert_entity_to_audit_entry(
        &mut self,
        req: RevertEntityRequest,
    ) -> Result<RevertEntityResponse, AxonError> {
        let source = self
            .audit
            .find_by_id(req.audit_entry_id)?
            .ok_or_else(|| AxonError::NotFound(format!("audit entry {}", req.audit_entry_id)))?;

        let before_data = source.data_before.clone().ok_or_else(|| {
            AxonError::InvalidOperation(format!(
                "audit entry {} has no before state (cannot revert a create)",
                req.audit_entry_id
            ))
        })?;

        // Validate against current schema unless force=true.
        if !req.force {
            if let Some(schema) = self.storage.get_schema(&source.collection)? {
                validate(&schema, &before_data).map_err(|e| {
                    AxonError::SchemaValidation(format!(
                        "before state from audit entry {} does not validate against current schema: {}",
                        req.audit_entry_id, e
                    ))
                })?;
            }
        }

        // Apply the revert: update if entity still exists, recreate if deleted.
        let current = self.storage.get(&source.collection, &source.entity_id)?;
        let data_before_revert = current.as_ref().map(|e| e.data.clone());
        let restored = match current {
            Some(existing) => {
                let candidate = Entity {
                    collection: source.collection.clone(),
                    id: source.entity_id.clone(),
                    version: existing.version,
                    data: before_data.clone(),
                };
                self.storage.compare_and_swap(candidate, existing.version)?
            }
            None => {
                let entity = Entity::new(
                    source.collection.clone(),
                    source.entity_id.clone(),
                    before_data.clone(),
                );
                self.storage.put(entity.clone())?;
                entity
            }
        };

        // Audit the revert.
        let mut revert_entry = AuditEntry::new(
            restored.collection.clone(),
            restored.id.clone(),
            restored.version,
            MutationType::EntityRevert,
            data_before_revert,
            Some(before_data),
            req.actor,
        );
        revert_entry.metadata.insert(
            "reverted_from_entry_id".into(),
            req.audit_entry_id.to_string(),
        );

        let appended = self.audit.append(revert_entry)?;

        Ok(RevertEntityResponse {
            entity: restored,
            audit_entry: appended,
        })
    }

    // ── Collection lifecycle ─────────────────────────────────────────────────

    /// Validate a collection name against naming rules.
    ///
    /// Names must be 1-128 characters, start with a lowercase letter, and
    /// contain only lowercase letters, digits, hyphens, and underscores.
    /// Internal pseudo-collections beginning with `__` are exempt.
    fn validate_collection_name(name: &CollectionId) -> Result<(), AxonError> {
        let s = name.as_str();

        // Internal pseudo-collections are exempt from user-facing naming rules.
        if s.starts_with("__") {
            return Ok(());
        }

        if s.is_empty() || s.len() > 128 {
            return Err(AxonError::InvalidArgument(format!(
                "collection name '{}' must be 1-128 characters",
                s
            )));
        }

        let mut chars = s.chars();
        let first = chars.next().unwrap();
        if !first.is_ascii_lowercase() {
            return Err(AxonError::InvalidArgument(format!(
                "collection name '{}' must start with a lowercase letter",
                s
            )));
        }

        for c in chars {
            if !matches!(c, 'a'..='z' | '0'..='9' | '-' | '_') {
                return Err(AxonError::InvalidArgument(format!(
                    "collection name '{}' contains invalid character '{}'; \
                     only lowercase letters, digits, hyphens, and underscores are allowed",
                    s, c
                )));
            }
        }

        Ok(())
    }

    /// Explicitly register a named collection and record the event in the audit log.
    ///
    /// A schema must be provided at creation time; schemaless collections are not supported.
    ///
    /// Returns [`AxonError::InvalidArgument`] if the name violates naming rules or the schema's
    /// `collection` field does not match `req.name`.
    /// Returns [`AxonError::AlreadyExists`] if the collection has already been created.
    pub fn create_collection(
        &mut self,
        req: CreateCollectionRequest,
    ) -> Result<CreateCollectionResponse, AxonError> {
        Self::validate_collection_name(&req.name)?;

        if req.schema.collection != req.name {
            return Err(AxonError::InvalidArgument(format!(
                "schema.collection '{}' does not match collection name '{}'",
                req.schema.collection, req.name
            )));
        }

        // Validate entity_schema before any mutations so a bad schema never
        // leaves an orphan (schemaless) collection registration.
        if let Some(entity_schema) = &req.schema.entity_schema {
            compile_entity_schema(entity_schema)?;
        }

        let existing = self.storage.list_collections()?;
        if existing.contains(&req.name) {
            return Err(AxonError::AlreadyExists(req.name.to_string()));
        }
        self.storage.register_collection(&req.name)?;
        self.put_schema(req.schema)?;

        self.audit.append(AuditEntry::new(
            req.name.clone(),
            EntityId::new(""),
            0,
            MutationType::CollectionCreate,
            None,
            None,
            req.actor,
        ))?;

        Ok(CreateCollectionResponse {
            name: req.name.to_string(),
        })
    }

    /// Drop a collection, removing all its entities, and record the event in the audit log.
    ///
    /// Returns [`AxonError::NotFound`] if the collection was never created via
    /// [`create_collection`].
    pub fn drop_collection(
        &mut self,
        req: DropCollectionRequest,
    ) -> Result<DropCollectionResponse, AxonError> {
        let existing = self.storage.list_collections()?;
        if !existing.contains(&req.name) {
            return Err(AxonError::NotFound(req.name.to_string()));
        }

        // Remove all entities in the collection.
        let entities = self.storage.range_scan(&req.name, None, None, None)?;
        let count = entities.len();
        for entity in &entities {
            self.storage.delete(&req.name, &entity.id)?;
        }
        self.storage.delete_schema(&req.name)?;
        self.storage.unregister_collection(&req.name)?;

        self.audit.append(AuditEntry::new(
            req.name.clone(),
            EntityId::new(""),
            0,
            MutationType::CollectionDrop,
            None,
            None,
            req.actor,
        ))?;

        Ok(DropCollectionResponse {
            name: req.name.to_string(),
            entities_removed: count,
        })
    }

    /// List all explicitly created collections with summary metadata.
    pub fn list_collections(
        &self,
        _req: ListCollectionsRequest,
    ) -> Result<ListCollectionsResponse, AxonError> {
        // Storage returns names already sorted ascending.
        let names = self.storage.list_collections()?;
        let collections: Vec<CollectionMetadata> = names
            .iter()
            .map(|name| {
                let entity_count = self.storage.count(name).unwrap_or(0);
                let schema_version = self
                    .storage
                    .get_schema(name)
                    .ok()
                    .flatten()
                    .map(|s| s.version);
                CollectionMetadata {
                    name: name.to_string(),
                    entity_count,
                    schema_version,
                }
            })
            .collect();

        Ok(ListCollectionsResponse { collections })
    }

    /// Describe a single collection (entity count + full schema).
    ///
    /// Returns [`AxonError::NotFound`] if the collection was not explicitly created.
    pub fn describe_collection(
        &self,
        req: DescribeCollectionRequest,
    ) -> Result<DescribeCollectionResponse, AxonError> {
        let existing = self.storage.list_collections()?;
        if !existing.contains(&req.name) {
            return Err(AxonError::NotFound(req.name.to_string()));
        }

        let entity_count = self.storage.count(&req.name)?;
        let schema = self.storage.get_schema(&req.name)?;

        Ok(DescribeCollectionResponse {
            name: req.name.to_string(),
            entity_count,
            schema,
        })
    }

    // ── Schema operations ────────────────────────────────────────────────────

    /// Persist or replace the schema for a collection.
    ///
    /// The `schema.collection` field must match the collection name in the
    /// request. Subsequent entity creates and updates will be validated against
    /// this schema.
    pub fn handle_put_schema(
        &mut self,
        req: PutSchemaRequest,
    ) -> Result<PutSchemaResponse, AxonError> {
        let collection = req.schema.collection.clone();
        self.put_schema(req.schema.clone())?;
        self.audit.append(AuditEntry::new(
            collection,
            EntityId::new(""),
            0,
            MutationType::SchemaUpdate,
            None,
            None,
            req.actor,
        ))?;
        Ok(PutSchemaResponse { schema: req.schema })
    }

    /// Retrieve the schema for a collection.
    ///
    /// Returns [`AxonError::NotFound`] if no schema has been stored.
    pub fn handle_get_schema(&self, req: GetSchemaRequest) -> Result<GetSchemaResponse, AxonError> {
        self.storage
            .get_schema(&req.collection)?
            .map(|schema| GetSchemaResponse { schema })
            .ok_or_else(|| {
                AxonError::NotFound(format!("schema for collection '{}'", req.collection))
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

        // Enforce link-type definitions from source collection schema (ADR-002).
        if let Some(schema) = self.storage.get_schema(&req.source_collection)? {
            if !schema.link_types.is_empty() {
                let link_def = schema.link_types.get(&req.link_type).ok_or_else(|| {
                    AxonError::SchemaValidation(format!(
                        "link type '{}' is not declared in collection '{}' schema",
                        req.link_type, req.source_collection
                    ))
                })?;

                // Verify target collection matches the declaration.
                if req.target_collection.as_str() != link_def.target_collection {
                    return Err(AxonError::SchemaValidation(format!(
                        "link type '{}' requires target collection '{}', got '{}'",
                        req.link_type, link_def.target_collection, req.target_collection
                    )));
                }

                // Validate link metadata against metadata_schema if declared.
                if let Some(metadata_schema) = &link_def.metadata_schema {
                    validate_link_metadata(metadata_schema, &req.metadata)?;
                }
            }
        }

        // Reject duplicate (source, target, link_type) triples.
        let link_id = Link::storage_id(
            &req.source_collection,
            &req.source_id,
            &req.link_type,
            &req.target_collection,
            &req.target_id,
        );
        if self
            .storage
            .get(&Link::links_collection(), &link_id)?
            .is_some()
        {
            return Err(AxonError::AlreadyExists(format!(
                "link {}/{}/{}/{}/{}",
                req.source_collection,
                req.source_id,
                req.link_type,
                req.target_collection,
                req.target_id
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

        // Store the link and its reverse-index entry.
        self.storage.put(link.to_rev_entity())?;
        let link_entity = link.to_entity();
        self.storage.put(link_entity.clone())?;

        // Audit: record the link creation.
        self.audit.append(AuditEntry::new(
            link_entity.collection,
            link_entity.id,
            link_entity.version,
            MutationType::LinkCreate,
            None,
            Some(link_entity.data),
            req.actor,
        ))?;

        Ok(CreateLinkResponse { link })
    }

    /// Delete a typed link between two entities.
    ///
    /// Removes both the forward link from `__axon_links__` and the corresponding
    /// reverse-index entry from `__axon_links_rev__`. If the link does not exist,
    /// [`AxonError::NotFound`] is returned.
    pub fn delete_link(&mut self, req: DeleteLinkRequest) -> Result<DeleteLinkResponse, AxonError> {
        let link_id = Link::storage_id(
            &req.source_collection,
            &req.source_id,
            &req.link_type,
            &req.target_collection,
            &req.target_id,
        );

        // Verify the link exists before attempting deletion; capture its data for the audit entry.
        let link_entity = self
            .storage
            .get(&Link::links_collection(), &link_id)?
            .ok_or_else(|| {
                AxonError::NotFound(format!(
                    "link {}/{} --[{}]--> {}/{}",
                    req.source_collection,
                    req.source_id,
                    req.link_type,
                    req.target_collection,
                    req.target_id,
                ))
            })?;

        // Delete the reverse-index entry first, then the forward link.
        let rev_id = Link::rev_storage_id(
            &req.target_collection,
            &req.target_id,
            &req.source_collection,
            &req.source_id,
            &req.link_type,
        );
        self.storage
            .delete(&Link::links_rev_collection(), &rev_id)?;
        self.storage.delete(&Link::links_collection(), &link_id)?;

        // Audit: record the link deletion.
        self.audit.append(AuditEntry::new(
            link_entity.collection,
            link_entity.id,
            link_entity.version,
            MutationType::LinkDelete,
            Some(link_entity.data),
            None,
            req.actor,
        ))?;

        Ok(DeleteLinkResponse {
            source_collection: req.source_collection.to_string(),
            source_id: req.source_id.to_string(),
            target_collection: req.target_collection.to_string(),
            target_id: req.target_id.to_string(),
            link_type: req.link_type,
        })
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

// ── Query filter helpers ──────────────────────────────────────────────────────

/// Maximum allowed nesting depth for a [`FilterNode`] tree.
///
/// Prevents stack overflows from deeply nested client-supplied filter trees.
const MAX_FILTER_DEPTH: usize = 32;

/// Return the maximum nesting depth of a [`FilterNode`] tree (1-based).
///
/// Uses an explicit stack-based iterative traversal to avoid stack overflows
/// on deeply nested client-supplied filter trees.
fn filter_depth(root: &FilterNode) -> usize {
    // Stack entries: (node, depth_of_this_node)
    let mut stack: Vec<(&FilterNode, usize)> = vec![(root, 1)];
    let mut max_depth = 0usize;
    while let Some((node, depth)) = stack.pop() {
        if depth > max_depth {
            max_depth = depth;
        }
        if let FilterNode::And { filters } | FilterNode::Or { filters } = node {
            for child in filters {
                stack.push((child, depth + 1));
            }
        }
    }
    max_depth
}

/// Evaluate a [`FilterNode`] against the entity's JSON data.
fn apply_filter(node: &FilterNode, data: &serde_json::Value) -> bool {
    match node {
        FilterNode::Field(f) => apply_field_filter(f, data),
        FilterNode::And { filters } => filters.iter().all(|f| apply_filter(f, data)),
        FilterNode::Or { filters } => filters.iter().any(|f| apply_filter(f, data)),
    }
}

fn apply_field_filter(f: &FieldFilter, data: &serde_json::Value) -> bool {
    let field_val = get_field_value(data, &f.field);
    match &f.op {
        FilterOp::Eq => values_eq(field_val, Some(&f.value)),
        FilterOp::Ne => !values_eq(field_val, Some(&f.value)),
        FilterOp::Gt => compare_values(field_val, Some(&f.value)) == std::cmp::Ordering::Greater,
        FilterOp::Gte => {
            let ord = compare_values(field_val, Some(&f.value));
            ord == std::cmp::Ordering::Greater || ord == std::cmp::Ordering::Equal
        }
        FilterOp::Lt => compare_values(field_val, Some(&f.value)) == std::cmp::Ordering::Less,
        FilterOp::Lte => {
            let ord = compare_values(field_val, Some(&f.value));
            ord == std::cmp::Ordering::Less || ord == std::cmp::Ordering::Equal
        }
        FilterOp::In => {
            if let serde_json::Value::Array(arr) = &f.value {
                arr.iter().any(|v| values_eq(field_val, Some(v)))
            } else {
                false
            }
        }
        FilterOp::Contains => match (field_val, &f.value) {
            (Some(serde_json::Value::String(s)), serde_json::Value::String(sub)) => {
                s.contains(sub.as_str())
            }
            _ => false,
        },
    }
}

/// Resolve a dot-separated field path into a JSON value, returning `None` if missing.
fn get_field_value<'a>(data: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut cur = data;
    for segment in path.split('.') {
        cur = cur.get(segment)?;
    }
    Some(cur)
}

fn values_eq(a: Option<&serde_json::Value>, b: Option<&serde_json::Value>) -> bool {
    match (a, b) {
        (Some(av), Some(bv)) => av == bv,
        (None, None) => true,
        _ => false,
    }
}

/// Total ordering for JSON values (numbers, strings, booleans, null).
/// Incomparable types (e.g. object vs number) are treated as equal.
fn compare_values(
    a: Option<&serde_json::Value>,
    b: Option<&serde_json::Value>,
) -> std::cmp::Ordering {
    use serde_json::Value;
    use std::cmp::Ordering;
    match (a, b) {
        (Some(Value::Number(an)), Some(Value::Number(bn))) => {
            let af = an.as_f64().unwrap_or(f64::NAN);
            let bf = bn.as_f64().unwrap_or(f64::NAN);
            af.partial_cmp(&bf).unwrap_or(Ordering::Equal)
        }
        (Some(Value::String(as_)), Some(Value::String(bs))) => as_.cmp(bs),
        (Some(Value::Bool(ab)), Some(Value::Bool(bb))) => ab.cmp(bb),
        (Some(Value::Null), Some(Value::Null)) => Ordering::Equal,
        // Null sorts before everything else.
        (Some(Value::Null), Some(_)) => Ordering::Less,
        (Some(_), Some(Value::Null)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        _ => Ordering::Equal,
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
                    actual: 1,
                    ..
                }
            ),
            "unexpected error: {err}"
        );
        // current_entity must carry the stored state so callers can merge and retry (FEAT-004, FEAT-008).
        if let AxonError::ConflictingVersion { current_entity, .. } = err {
            let ce = current_entity.expect("current_entity must be present in conflict response");
            assert_eq!(ce.version, 1);
        }
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
            .into_collection_schema()
            .unwrap();
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
            .into_collection_schema()
            .unwrap();
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
    fn create_link_produces_audit_entry() {
        let mut h = handler();
        make_entity(&mut h, "users", "u-001");
        make_entity(&mut h, "tasks", "t-001");

        // Two audit entries already exist from make_entity calls.
        let before = h.audit_log().len();

        h.create_link(CreateLinkRequest {
            source_collection: CollectionId::new("users"),
            source_id: EntityId::new("u-001"),
            target_collection: CollectionId::new("tasks"),
            target_id: EntityId::new("t-001"),
            link_type: "assigned-to".into(),
            metadata: json!(null),
            actor: Some("agent-1".into()),
        })
        .unwrap();

        assert_eq!(
            h.audit_log().len(),
            before + 1,
            "create_link must produce exactly one audit entry"
        );

        let resp = h
            .query_audit(QueryAuditRequest {
                operation: Some("link.create".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(resp.entries.len(), 1, "exactly one link.create entry");
        let entry = &resp.entries[0];
        assert_eq!(entry.mutation, axon_audit::entry::MutationType::LinkCreate);
        assert_eq!(entry.actor, "agent-1");
        assert!(
            entry.data_before.is_none(),
            "link.create must have no before state"
        );
        assert!(
            entry.data_after.is_some(),
            "link.create must record after state"
        );
    }

    #[test]
    fn delete_link_produces_audit_entry() {
        let mut h = handler();
        make_entity(&mut h, "users", "u-001");
        make_entity(&mut h, "tasks", "t-001");

        h.create_link(CreateLinkRequest {
            source_collection: CollectionId::new("users"),
            source_id: EntityId::new("u-001"),
            target_collection: CollectionId::new("tasks"),
            target_id: EntityId::new("t-001"),
            link_type: "assigned-to".into(),
            metadata: json!(null),
            actor: None,
        })
        .unwrap();

        let before = h.audit_log().len();

        h.delete_link(DeleteLinkRequest {
            source_collection: CollectionId::new("users"),
            source_id: EntityId::new("u-001"),
            target_collection: CollectionId::new("tasks"),
            target_id: EntityId::new("t-001"),
            link_type: "assigned-to".into(),
            actor: Some("agent-2".into()),
        })
        .unwrap();

        assert_eq!(
            h.audit_log().len(),
            before + 1,
            "delete_link must produce exactly one audit entry"
        );

        let resp = h
            .query_audit(QueryAuditRequest {
                operation: Some("link.delete".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(resp.entries.len(), 1, "exactly one link.delete entry");
        let entry = &resp.entries[0];
        assert_eq!(entry.mutation, axon_audit::entry::MutationType::LinkDelete);
        assert_eq!(entry.actor, "agent-2");
        assert!(
            entry.data_before.is_some(),
            "link.delete must record before state"
        );
        assert!(
            entry.data_after.is_none(),
            "link.delete must have no after state"
        );
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

    // ── Audit query ──────────────────────────────────────────────────────────

    #[test]
    fn update_audit_entry_has_diff() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v1", "done": false}),
            actor: None,
        })
        .unwrap();

        h.update_entity(UpdateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v2", "done": false}),
            expected_version: 1,
            actor: None,
        })
        .unwrap();

        let entries = h.audit_log().query_by_entity(&col, &id).unwrap();
        let update_entry = entries
            .iter()
            .find(|e| e.mutation == axon_audit::entry::MutationType::EntityUpdate)
            .unwrap();
        let diff = update_entry
            .diff
            .as_ref()
            .expect("diff should be present on update");
        assert!(
            diff.contains_key("title"),
            "title field should appear in diff"
        );
        assert_eq!(diff["title"].before, Some(json!("v1")));
        assert_eq!(diff["title"].after, Some(json!("v2")));
    }

    #[test]
    fn query_audit_filters_by_actor() {
        let mut h = handler();
        let col = CollectionId::new("tasks");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-001"),
            data: json!({"title": "by alice"}),
            actor: Some("alice".into()),
        })
        .unwrap();

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-002"),
            data: json!({"title": "by bob"}),
            actor: Some("bob".into()),
        })
        .unwrap();

        let resp = h
            .query_audit(QueryAuditRequest {
                actor: Some("alice".into()),
                ..Default::default()
            })
            .unwrap();

        assert_eq!(resp.entries.len(), 1);
        assert_eq!(resp.entries[0].actor, "alice");
    }

    #[test]
    fn query_audit_pagination() {
        let mut h = handler();
        let col = CollectionId::new("tasks");

        for i in 0..5u32 {
            h.create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: EntityId::new(format!("t-{i:03}")),
                data: json!({"title": format!("task {i}")}),
                actor: None,
            })
            .unwrap();
        }

        let page1 = h
            .query_audit(QueryAuditRequest {
                limit: Some(2),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page1.entries.len(), 2);
        assert!(page1.next_cursor.is_some());

        let page2 = h
            .query_audit(QueryAuditRequest {
                limit: Some(2),
                after_id: page1.next_cursor,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page2.entries.len(), 2);

        let page3 = h
            .query_audit(QueryAuditRequest {
                limit: Some(2),
                after_id: page2.next_cursor,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page3.entries.len(), 1);
        assert!(page3.next_cursor.is_none());
    }

    // ── Revert ───────────────────────────────────────────────────────────────

    #[test]
    fn revert_restores_entity_to_before_state() {
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

        h.update_entity(UpdateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v2"}),
            expected_version: 1,
            actor: None,
        })
        .unwrap();

        // Find the update audit entry.
        let entries = h.audit_log().query_by_entity(&col, &id).unwrap();
        let update_entry = entries
            .iter()
            .find(|e| e.mutation == axon_audit::entry::MutationType::EntityUpdate)
            .unwrap();

        let resp = h
            .revert_entity_to_audit_entry(RevertEntityRequest {
                audit_entry_id: update_entry.id,
                actor: Some("admin".into()),
                force: false,
            })
            .unwrap();

        assert_eq!(
            resp.entity.data["title"], "v1",
            "entity should be restored to v1"
        );
        assert_eq!(
            resp.audit_entry.mutation,
            axon_audit::entry::MutationType::EntityRevert
        );
        assert_eq!(
            resp.audit_entry
                .metadata
                .get("reverted_from_entry_id")
                .map(String::as_str),
            Some(&update_entry.id.to_string() as &str)
        );
        // Audit log should have 4 entries: create, update, revert
        assert_eq!(h.audit_log().len(), 3);
    }

    #[test]
    fn revert_missing_audit_entry_returns_not_found() {
        let mut h = handler();
        let err = h
            .revert_entity_to_audit_entry(RevertEntityRequest {
                audit_entry_id: 9999,
                actor: None,
                force: false,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::NotFound(_)));
    }

    #[test]
    fn revert_create_entry_fails_no_before() {
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

        let entries = h.audit_log().query_by_entity(&col, &id).unwrap();
        let create_entry = &entries[0];

        let err = h
            .revert_entity_to_audit_entry(RevertEntityRequest {
                audit_entry_id: create_entry.id,
                actor: None,
                force: false,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::InvalidOperation(_)));
    }

    // ── Collection lifecycle ─────────────────────────────────────────────────

    #[test]
    fn create_and_drop_collection_produce_audit_entries() {
        let mut h = handler();

        h.create_collection(CreateCollectionRequest {
            name: CollectionId::new("widgets"),
            schema: CollectionSchema::new(CollectionId::new("widgets")),
            actor: Some("admin".into()),
        })
        .unwrap();

        // Populate with some entities.
        for i in 0..3u32 {
            h.create_entity(CreateEntityRequest {
                collection: CollectionId::new("widgets"),
                id: EntityId::new(format!("w-{i:03}")),
                data: json!({"name": format!("widget {i}")}),
                actor: None,
            })
            .unwrap();
        }

        let drop_resp = h
            .drop_collection(DropCollectionRequest {
                name: CollectionId::new("widgets"),
                actor: Some("admin".into()),
            })
            .unwrap();

        assert_eq!(drop_resp.entities_removed, 3);

        // Audit log: 1 CollectionCreate + 3 EntityCreate + 1 CollectionDrop = 5.
        assert_eq!(h.audit_log().len(), 5);

        let col_creates = h
            .audit_log()
            .query_by_operation(&axon_audit::entry::MutationType::CollectionCreate)
            .unwrap();
        assert_eq!(col_creates.len(), 1);

        let col_drops = h
            .audit_log()
            .query_by_operation(&axon_audit::entry::MutationType::CollectionDrop)
            .unwrap();
        assert_eq!(col_drops.len(), 1);
    }

    #[test]
    fn create_duplicate_collection_returns_already_exists() {
        let mut h = handler();
        h.create_collection(CreateCollectionRequest {
            name: CollectionId::new("dup"),
            schema: CollectionSchema::new(CollectionId::new("dup")),
            actor: None,
        })
        .unwrap();

        let err = h
            .create_collection(CreateCollectionRequest {
                name: CollectionId::new("dup"),
                schema: CollectionSchema::new(CollectionId::new("dup")),
                actor: None,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::AlreadyExists(_)));
    }

    #[test]
    fn drop_unknown_collection_returns_not_found() {
        let mut h = handler();
        let err = h
            .drop_collection(DropCollectionRequest {
                name: CollectionId::new("ghost"),
                actor: None,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::NotFound(_)));
    }

    // ── Schema binding at collection creation (FEAT-001) ─────────────────────

    #[test]
    fn create_collection_persists_schema() {
        let mut h = handler();
        let col = CollectionId::new("typed-col");
        let schema = CollectionSchema {
            collection: col.clone(),
            description: Some("a typed collection".into()),
            version: 1,
            entity_schema: Some(json!({"type": "object"})),
            link_types: Default::default(),
        };
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: schema.clone(),
            actor: None,
        })
        .unwrap();

        let stored = h
            .get_schema(&col)
            .unwrap()
            .expect("schema must be stored at creation");
        assert_eq!(stored.version, 1);
        assert_eq!(stored.description.as_deref(), Some("a typed collection"));
        assert_eq!(stored.entity_schema, Some(json!({"type": "object"})));
    }

    #[test]
    fn create_collection_rejects_schema_collection_mismatch() {
        let mut h = handler();
        let err = h
            .create_collection(CreateCollectionRequest {
                name: CollectionId::new("foo"),
                schema: CollectionSchema::new(CollectionId::new("bar")),
                actor: None,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::InvalidArgument(_)));
    }

    // ── Collection name validation ───────────────────────────────────────────

    #[test]
    fn create_collection_rejects_empty_name() {
        let mut h = handler();
        let err = h
            .create_collection(CreateCollectionRequest {
                name: CollectionId::new(""),
                schema: CollectionSchema::new(CollectionId::new("")),
                actor: None,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::InvalidArgument(_)));
    }

    #[test]
    fn create_collection_rejects_name_starting_with_digit() {
        let mut h = handler();
        let err = h
            .create_collection(CreateCollectionRequest {
                name: CollectionId::new("1bad"),
                schema: CollectionSchema::new(CollectionId::new("1bad")),
                actor: None,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::InvalidArgument(_)));
    }

    #[test]
    fn create_collection_rejects_name_with_uppercase() {
        let mut h = handler();
        let err = h
            .create_collection(CreateCollectionRequest {
                name: CollectionId::new("Bad-Name"),
                schema: CollectionSchema::new(CollectionId::new("Bad-Name")),
                actor: None,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::InvalidArgument(_)));
    }

    #[test]
    fn create_collection_rejects_name_with_spaces() {
        let mut h = handler();
        let err = h
            .create_collection(CreateCollectionRequest {
                name: CollectionId::new("bad name"),
                schema: CollectionSchema::new(CollectionId::new("bad name")),
                actor: None,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::InvalidArgument(_)));
    }

    #[test]
    fn create_collection_accepts_valid_names() {
        let mut h = handler();
        for name in &["tasks", "my-tasks", "my_tasks", "tasks2", "a"] {
            h.create_collection(CreateCollectionRequest {
                name: CollectionId::new(*name),
                schema: CollectionSchema::new(CollectionId::new(*name)),
                actor: None,
            })
            .unwrap_or_else(|e| panic!("valid name '{}' rejected: {}", name, e));
        }
    }

    #[test]
    fn create_collection_invalid_entity_schema_leaves_no_orphan() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let schema = CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: Some(json!({"type": "bogus"})),
            link_types: Default::default(),
        };

        let err = h
            .create_collection(CreateCollectionRequest {
                name: col,
                schema,
                actor: None,
            })
            .unwrap_err();
        assert!(
            matches!(err, AxonError::SchemaValidation(_)),
            "expected SchemaValidation error, got: {err}"
        );

        // No orphan: the collection must not appear in the registry.
        let resp = h.list_collections(ListCollectionsRequest {}).unwrap();
        assert!(
            resp.collections.is_empty(),
            "orphan collection registered despite invalid schema: {:?}",
            resp.collections
        );
    }

    // ── list_collections ─────────────────────────────────────────────────────

    #[test]
    fn list_collections_empty_when_none_created() {
        let h = handler();
        let resp = h.list_collections(ListCollectionsRequest {}).unwrap();
        assert!(resp.collections.is_empty());
    }

    #[test]
    fn list_collections_returns_created_collections() {
        let mut h = handler();

        for name in &["apples", "bananas", "cherries"] {
            h.create_collection(CreateCollectionRequest {
                name: CollectionId::new(*name),
                schema: CollectionSchema::new(CollectionId::new(*name)),
                actor: None,
            })
            .unwrap();
        }

        // Add two entities to "bananas".
        for i in 0..2u32 {
            h.create_entity(CreateEntityRequest {
                collection: CollectionId::new("bananas"),
                id: EntityId::new(format!("b-{i}")),
                data: json!({"name": format!("b-{i}")}),
                actor: None,
            })
            .unwrap();
        }

        let resp = h.list_collections(ListCollectionsRequest {}).unwrap();
        assert_eq!(resp.collections.len(), 3);

        // Results are sorted by name.
        assert_eq!(resp.collections[0].name, "apples");
        assert_eq!(resp.collections[1].name, "bananas");
        assert_eq!(resp.collections[2].name, "cherries");

        assert_eq!(resp.collections[1].entity_count, 2);
        assert_eq!(resp.collections[0].entity_count, 0);
    }

    #[test]
    fn list_collections_schema_version_reflects_stored_schema() {
        let mut h = handler();

        h.create_collection(CreateCollectionRequest {
            name: CollectionId::new("items"),
            schema: CollectionSchema::new(CollectionId::new("items")),
            actor: None,
        })
        .unwrap();
        h.put_schema(CollectionSchema {
            collection: CollectionId::new("items"),
            description: None,
            version: 5,
            entity_schema: None,
            link_types: Default::default(),
        })
        .unwrap();

        let resp = h.list_collections(ListCollectionsRequest {}).unwrap();
        assert_eq!(resp.collections[0].schema_version, Some(5));
    }

    // ── describe_collection ──────────────────────────────────────────────────

    #[test]
    fn describe_collection_returns_metadata_and_schema() {
        let mut h = handler();

        h.create_collection(CreateCollectionRequest {
            name: CollectionId::new("things"),
            schema: CollectionSchema::new(CollectionId::new("things")),
            actor: None,
        })
        .unwrap();
        h.put_schema(CollectionSchema {
            collection: CollectionId::new("things"),
            description: Some("a thing".into()),
            version: 2,
            entity_schema: None,
            link_types: Default::default(),
        })
        .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("things"),
            id: EntityId::new("t-001"),
            data: json!({}),
            actor: None,
        })
        .unwrap();

        let resp = h
            .describe_collection(DescribeCollectionRequest {
                name: CollectionId::new("things"),
            })
            .unwrap();

        assert_eq!(resp.name, "things");
        assert_eq!(resp.entity_count, 1);
        assert!(resp.schema.is_some());
        assert_eq!(resp.schema.unwrap().version, 2);
    }

    #[test]
    fn describe_collection_not_found_for_unknown() {
        let h = handler();
        let err = h
            .describe_collection(DescribeCollectionRequest {
                name: CollectionId::new("nope"),
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::NotFound(_)));
    }

    // ── Collection durability (hx-31638e63) ──────────────────────────────────

    /// A handler constructed from storage that already has registered
    /// collections correctly reports them via list_collections and
    /// describe_collection — no re-creation required.
    ///
    /// This is the analogue of a SQLite process-restart: the adapter is
    /// durable; only the AxonHandler is freshly constructed.
    #[test]
    fn pre_populated_storage_reports_collections_on_new_handler() {
        use axon_storage::adapter::StorageAdapter as _;
        let mut storage = MemoryStorageAdapter::default();

        // Directly register a collection into storage (simulates a durable
        // backend that was populated before this handler was constructed).
        storage
            .register_collection(&CollectionId::new("tasks"))
            .unwrap();

        let h = AxonHandler::new(storage);
        let resp = h.list_collections(ListCollectionsRequest {}).unwrap();
        assert_eq!(
            resp.collections.len(),
            1,
            "list_collections should see pre-populated collection"
        );
        assert_eq!(resp.collections[0].name, "tasks");

        // describe_collection must not return NotFound.
        h.describe_collection(DescribeCollectionRequest {
            name: CollectionId::new("tasks"),
        })
        .unwrap();
    }

    /// After creating a collection and extracting the storage adapter, a brand-
    /// new AxonHandler built from that same adapter still sees the collection.
    #[test]
    fn collection_survives_handler_reconstruction() {
        // Build the first handler, create a collection, then recover the storage.
        let mut h1 = handler();
        h1.create_collection(CreateCollectionRequest {
            name: CollectionId::new("widgets"),
            schema: CollectionSchema::new(CollectionId::new("widgets")),
            actor: None,
        })
        .unwrap();

        // Extract storage by consuming the first handler.
        let storage = h1.into_storage();

        // Reconstruct a new handler from the same storage.
        let h2 = AxonHandler::new(storage);
        let resp = h2.list_collections(ListCollectionsRequest {}).unwrap();
        assert_eq!(
            resp.collections.len(),
            1,
            "collection must survive handler reconstruction"
        );
        assert_eq!(resp.collections[0].name, "widgets");

        h2.describe_collection(DescribeCollectionRequest {
            name: CollectionId::new("widgets"),
        })
        .unwrap();
    }

    // ── Link deletion ────────────────────────────────────────────────────────

    #[test]
    fn delete_link_removes_forward_and_reverse_entries() {
        let mut h = handler();
        make_entity(&mut h, "users", "u-001");
        make_entity(&mut h, "tasks", "t-001");

        h.create_link(CreateLinkRequest {
            source_collection: CollectionId::new("users"),
            source_id: EntityId::new("u-001"),
            target_collection: CollectionId::new("tasks"),
            target_id: EntityId::new("t-001"),
            link_type: "assigned-to".into(),
            metadata: json!(null),
            actor: None,
        })
        .unwrap();

        // Delete the link.
        let resp = h
            .delete_link(DeleteLinkRequest {
                source_collection: CollectionId::new("users"),
                source_id: EntityId::new("u-001"),
                target_collection: CollectionId::new("tasks"),
                target_id: EntityId::new("t-001"),
                link_type: "assigned-to".into(),
                actor: None,
            })
            .unwrap();

        assert_eq!(resp.link_type, "assigned-to");

        // Forward link must be gone — traversal from u-001 should return nothing.
        let trav = h
            .traverse(TraverseRequest {
                collection: CollectionId::new("users"),
                id: EntityId::new("u-001"),
                link_type: Some("assigned-to".into()),
                max_depth: Some(1),
            })
            .unwrap();
        assert!(trav.entities.is_empty(), "forward link must be removed");

        // Reverse-index must be gone — delete_entity on t-001 must now succeed.
        h.delete_entity(DeleteEntityRequest {
            collection: CollectionId::new("tasks"),
            id: EntityId::new("t-001"),
            actor: None,
        })
        .expect("delete_entity must succeed after reverse-index entry is removed");
    }

    #[test]
    fn delete_link_missing_returns_not_found() {
        let mut h = handler();
        make_entity(&mut h, "users", "u-001");
        make_entity(&mut h, "tasks", "t-001");

        let err = h
            .delete_link(DeleteLinkRequest {
                source_collection: CollectionId::new("users"),
                source_id: EntityId::new("u-001"),
                target_collection: CollectionId::new("tasks"),
                target_id: EntityId::new("t-001"),
                link_type: "assigned-to".into(),
                actor: None,
            })
            .unwrap_err();

        assert!(matches!(err, AxonError::NotFound(_)));
    }

    // ── Link-type enforcement (axon-f48352d5) ────────────────────────────────

    const USERS_ESF_WITH_LINKS: &str = r#"
esf_version: "1.0"
collection: users
entity_schema:
  type: object
  required: [name]
  properties:
    name:
      type: string
link_types:
  assigned-to:
    target_collection: tasks
    cardinality: many-to-many
  mentor:
    target_collection: users
    cardinality: many-to-one
    metadata_schema:
      type: object
      required: [since]
      properties:
        since:
          type: string
"#;

    fn setup_linked_collections(h: &mut AxonHandler<MemoryStorageAdapter>) {
        let schema = EsfDocument::parse(USERS_ESF_WITH_LINKS)
            .unwrap()
            .into_collection_schema()
            .unwrap();
        h.register_schema(schema);

        // Also register a tasks schema (no link_types needed for this test).
        let tasks_schema = CollectionSchema::new(CollectionId::new("tasks"));
        h.register_schema(tasks_schema);
    }

    #[test]
    fn create_link_rejects_undeclared_link_type() {
        let mut h = handler();
        setup_linked_collections(&mut h);
        make_entity(&mut h, "users", "u-001");
        make_entity(&mut h, "tasks", "t-001");

        let err = h
            .create_link(CreateLinkRequest {
                source_collection: CollectionId::new("users"),
                source_id: EntityId::new("u-001"),
                target_collection: CollectionId::new("tasks"),
                target_id: EntityId::new("t-001"),
                link_type: "undeclared-type".into(),
                metadata: json!(null),
                actor: None,
            })
            .unwrap_err();

        assert!(
            matches!(err, AxonError::SchemaValidation(_)),
            "expected SchemaValidation for undeclared link type, got: {err}"
        );
    }

    #[test]
    fn create_link_rejects_wrong_target_collection() {
        let mut h = handler();
        setup_linked_collections(&mut h);
        make_entity(&mut h, "users", "u-001");
        make_entity(&mut h, "users", "u-002");

        // "assigned-to" declares target_collection=tasks, but we target users.
        let err = h
            .create_link(CreateLinkRequest {
                source_collection: CollectionId::new("users"),
                source_id: EntityId::new("u-001"),
                target_collection: CollectionId::new("users"),
                target_id: EntityId::new("u-002"),
                link_type: "assigned-to".into(),
                metadata: json!(null),
                actor: None,
            })
            .unwrap_err();

        assert!(
            matches!(err, AxonError::SchemaValidation(_)),
            "expected SchemaValidation for wrong target collection, got: {err}"
        );
    }

    #[test]
    fn create_link_validates_metadata_against_schema() {
        let mut h = handler();
        setup_linked_collections(&mut h);
        make_entity(&mut h, "users", "u-001");
        make_entity(&mut h, "users", "u-002");

        // "mentor" requires metadata with a "since" field.
        let err = h
            .create_link(CreateLinkRequest {
                source_collection: CollectionId::new("users"),
                source_id: EntityId::new("u-001"),
                target_collection: CollectionId::new("users"),
                target_id: EntityId::new("u-002"),
                link_type: "mentor".into(),
                metadata: json!({}), // missing required "since"
                actor: None,
            })
            .unwrap_err();

        assert!(
            matches!(err, AxonError::SchemaValidation(_)),
            "expected SchemaValidation for invalid metadata, got: {err}"
        );
    }

    #[test]
    fn create_link_accepts_valid_metadata() {
        let mut h = handler();
        setup_linked_collections(&mut h);
        make_entity(&mut h, "users", "u-001");
        make_entity(&mut h, "users", "u-002");

        let resp = h
            .create_link(CreateLinkRequest {
                source_collection: CollectionId::new("users"),
                source_id: EntityId::new("u-001"),
                target_collection: CollectionId::new("users"),
                target_id: EntityId::new("u-002"),
                link_type: "mentor".into(),
                metadata: json!({"since": "2026-01-01"}),
                actor: None,
            })
            .unwrap();

        assert_eq!(resp.link.link_type, "mentor");
    }

    #[test]
    fn create_link_rejects_duplicate_triple() {
        let mut h = handler();
        setup_linked_collections(&mut h);
        make_entity(&mut h, "users", "u-001");
        make_entity(&mut h, "tasks", "t-001");

        h.create_link(CreateLinkRequest {
            source_collection: CollectionId::new("users"),
            source_id: EntityId::new("u-001"),
            target_collection: CollectionId::new("tasks"),
            target_id: EntityId::new("t-001"),
            link_type: "assigned-to".into(),
            metadata: json!(null),
            actor: None,
        })
        .unwrap();

        // Same triple again should fail.
        let err = h
            .create_link(CreateLinkRequest {
                source_collection: CollectionId::new("users"),
                source_id: EntityId::new("u-001"),
                target_collection: CollectionId::new("tasks"),
                target_id: EntityId::new("t-001"),
                link_type: "assigned-to".into(),
                metadata: json!(null),
                actor: None,
            })
            .unwrap_err();

        assert!(
            matches!(err, AxonError::AlreadyExists(_)),
            "expected AlreadyExists for duplicate link triple, got: {err}"
        );
    }

    #[test]
    fn create_link_allows_untyped_collections() {
        // Collections without schemas should still allow links (no enforcement).
        let mut h = handler();
        make_entity(&mut h, "loose", "a");
        make_entity(&mut h, "loose", "b");

        let resp = h
            .create_link(CreateLinkRequest {
                source_collection: CollectionId::new("loose"),
                source_id: EntityId::new("a"),
                target_collection: CollectionId::new("loose"),
                target_id: EntityId::new("b"),
                link_type: "anything".into(),
                metadata: json!(null),
                actor: None,
            })
            .unwrap();

        assert_eq!(resp.link.link_type, "anything");
    }

    #[test]
    fn create_link_allows_schema_without_link_types() {
        // Collections with a schema but no link_types should allow any link.
        let mut h = handler();
        let schema = EsfDocument::parse(TASK_ESF)
            .unwrap()
            .into_collection_schema()
            .unwrap();
        h.register_schema(schema);
        // Create entities that match the tasks schema (requires "title").
        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("tasks"),
            id: EntityId::new("t-001"),
            data: json!({"title": "Task 1"}),
            actor: None,
        })
        .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("tasks"),
            id: EntityId::new("t-002"),
            data: json!({"title": "Task 2"}),
            actor: None,
        })
        .unwrap();

        let resp = h
            .create_link(CreateLinkRequest {
                source_collection: CollectionId::new("tasks"),
                source_id: EntityId::new("t-001"),
                target_collection: CollectionId::new("tasks"),
                target_id: EntityId::new("t-002"),
                link_type: "depends-on".into(),
                metadata: json!(null),
                actor: None,
            })
            .unwrap();

        assert_eq!(resp.link.link_type, "depends-on");
    }

    // ── Entity query / filter (US-011) ────────────────────────────────────────

    fn make_entity_with_data(
        h: &mut AxonHandler<MemoryStorageAdapter>,
        collection: &str,
        id: &str,
        data: serde_json::Value,
    ) {
        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new(collection),
            id: EntityId::new(id),
            data,
            actor: None,
        })
        .unwrap();
    }

    use crate::request::{
        FieldFilter, FilterNode, FilterOp, QueryEntitiesRequest, SortDirection, SortField,
    };

    #[test]
    fn query_no_filter_returns_all() {
        let mut h = handler();
        make_entity_with_data(&mut h, "tasks", "t-1", json!({"status": "open"}));
        make_entity_with_data(&mut h, "tasks", "t-2", json!({"status": "done"}));

        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("tasks"),
                filter: None,
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.total_count, 2);
        assert_eq!(resp.entities.len(), 2);
    }

    #[test]
    fn query_filter_eq() {
        let mut h = handler();
        make_entity_with_data(&mut h, "tasks", "t-1", json!({"status": "open"}));
        make_entity_with_data(&mut h, "tasks", "t-2", json!({"status": "done"}));
        make_entity_with_data(&mut h, "tasks", "t-3", json!({"status": "open"}));

        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("tasks"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "status".into(),
                    op: FilterOp::Eq,
                    value: json!("open"),
                })),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.total_count, 2);
        assert!(resp.entities.iter().all(|e| e.data["status"] == "open"));
    }

    #[test]
    fn query_filter_ne() {
        let mut h = handler();
        make_entity_with_data(&mut h, "tasks", "t-1", json!({"status": "open"}));
        make_entity_with_data(&mut h, "tasks", "t-2", json!({"status": "done"}));

        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("tasks"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "status".into(),
                    op: FilterOp::Ne,
                    value: json!("done"),
                })),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.total_count, 1);
        assert_eq!(resp.entities[0].data["status"], "open");
    }

    #[test]
    fn query_filter_gt_and_lte() {
        let mut h = handler();
        make_entity_with_data(&mut h, "issues", "i-1", json!({"priority": 1}));
        make_entity_with_data(&mut h, "issues", "i-2", json!({"priority": 3}));
        make_entity_with_data(&mut h, "issues", "i-3", json!({"priority": 5}));

        // priority > 2
        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("issues"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "priority".into(),
                    op: FilterOp::Gt,
                    value: json!(2),
                })),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();
        assert_eq!(resp.total_count, 2);

        // priority <= 3
        let resp2 = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("issues"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "priority".into(),
                    op: FilterOp::Lte,
                    value: json!(3),
                })),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();
        assert_eq!(resp2.total_count, 2);
    }

    #[test]
    fn query_filter_in() {
        let mut h = handler();
        make_entity_with_data(&mut h, "tasks", "t-1", json!({"status": "open"}));
        make_entity_with_data(&mut h, "tasks", "t-2", json!({"status": "done"}));
        make_entity_with_data(&mut h, "tasks", "t-3", json!({"status": "in_progress"}));

        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("tasks"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "status".into(),
                    op: FilterOp::In,
                    value: json!(["open", "in_progress"]),
                })),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.total_count, 2);
    }

    #[test]
    fn query_filter_contains() {
        let mut h = handler();
        make_entity_with_data(&mut h, "docs", "d-1", json!({"title": "Hello World"}));
        make_entity_with_data(&mut h, "docs", "d-2", json!({"title": "Goodbye World"}));
        make_entity_with_data(&mut h, "docs", "d-3", json!({"title": "Nothing here"}));

        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("docs"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "title".into(),
                    op: FilterOp::Contains,
                    value: json!("World"),
                })),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.total_count, 2);
    }

    #[test]
    fn query_filter_and_combinator() {
        let mut h = handler();
        make_entity_with_data(
            &mut h,
            "tasks",
            "t-1",
            json!({"status": "open", "assignee": "alice"}),
        );
        make_entity_with_data(
            &mut h,
            "tasks",
            "t-2",
            json!({"status": "open", "assignee": "bob"}),
        );
        make_entity_with_data(
            &mut h,
            "tasks",
            "t-3",
            json!({"status": "done", "assignee": "alice"}),
        );

        // status = "open" AND assignee = "alice"
        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("tasks"),
                filter: Some(FilterNode::And {
                    filters: vec![
                        FilterNode::Field(FieldFilter {
                            field: "status".into(),
                            op: FilterOp::Eq,
                            value: json!("open"),
                        }),
                        FilterNode::Field(FieldFilter {
                            field: "assignee".into(),
                            op: FilterOp::Eq,
                            value: json!("alice"),
                        }),
                    ],
                }),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.total_count, 1);
        assert_eq!(resp.entities[0].data["assignee"], "alice");
        assert_eq!(resp.entities[0].data["status"], "open");
    }

    #[test]
    fn query_filter_or_combinator() {
        let mut h = handler();
        make_entity_with_data(&mut h, "tasks", "t-1", json!({"status": "open"}));
        make_entity_with_data(&mut h, "tasks", "t-2", json!({"status": "done"}));
        make_entity_with_data(&mut h, "tasks", "t-3", json!({"status": "archived"}));

        // status = "open" OR status = "done"
        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("tasks"),
                filter: Some(FilterNode::Or {
                    filters: vec![
                        FilterNode::Field(FieldFilter {
                            field: "status".into(),
                            op: FilterOp::Eq,
                            value: json!("open"),
                        }),
                        FilterNode::Field(FieldFilter {
                            field: "status".into(),
                            op: FilterOp::Eq,
                            value: json!("done"),
                        }),
                    ],
                }),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.total_count, 2);
    }

    #[test]
    fn query_sort_asc_and_desc() {
        let mut h = handler();
        make_entity_with_data(&mut h, "items", "i-1", json!({"priority": 3}));
        make_entity_with_data(&mut h, "items", "i-2", json!({"priority": 1}));
        make_entity_with_data(&mut h, "items", "i-3", json!({"priority": 2}));

        // Sort ascending
        let asc = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("items"),
                filter: None,
                sort: vec![SortField {
                    field: "priority".into(),
                    direction: SortDirection::Asc,
                }],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();
        let priorities_asc: Vec<i64> = asc
            .entities
            .iter()
            .map(|e| e.data["priority"].as_i64().unwrap())
            .collect();
        assert_eq!(priorities_asc, vec![1, 2, 3]);

        // Sort descending
        let desc = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("items"),
                filter: None,
                sort: vec![SortField {
                    field: "priority".into(),
                    direction: SortDirection::Desc,
                }],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();
        let priorities_desc: Vec<i64> = desc
            .entities
            .iter()
            .map(|e| e.data["priority"].as_i64().unwrap())
            .collect();
        assert_eq!(priorities_desc, vec![3, 2, 1]);
    }

    #[test]
    fn query_cursor_pagination() {
        let mut h = handler();
        // Insert 5 entities in a predictable order.
        for i in 1..=5 {
            make_entity_with_data(&mut h, "items", &format!("i-{i:03}"), json!({"n": i}));
        }

        // Page 1: limit=2, no cursor → returns i-001, i-002; next_cursor = "i-002"
        let page1 = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("items"),
                filter: None,
                sort: vec![],
                limit: Some(2),
                after_id: None,
                count_only: false,
            })
            .unwrap();
        assert_eq!(page1.entities.len(), 2);
        assert_eq!(page1.total_count, 5);
        assert!(page1.next_cursor.is_some());

        // Page 2: pick up after cursor from page 1.
        let cursor_id = EntityId::new(page1.next_cursor.as_deref().unwrap());
        let page2 = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("items"),
                filter: None,
                sort: vec![],
                limit: Some(2),
                after_id: Some(cursor_id),
                count_only: false,
            })
            .unwrap();
        assert_eq!(page2.entities.len(), 2);

        // Last page: no further results.
        let cursor_id2 = EntityId::new(page2.next_cursor.as_deref().unwrap());
        let page3 = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("items"),
                filter: None,
                sort: vec![],
                limit: Some(2),
                after_id: Some(cursor_id2),
                count_only: false,
            })
            .unwrap();
        assert_eq!(page3.entities.len(), 1);
        assert!(page3.next_cursor.is_none());
    }

    #[test]
    fn query_cursor_invalid_after_id_returns_error() {
        let mut h = handler();
        for i in 1..=3 {
            make_entity_with_data(&mut h, "items", &format!("i-{i:03}"), json!({"n": i}));
        }

        let result = h.query_entities(QueryEntitiesRequest {
            collection: CollectionId::new("items"),
            filter: None,
            sort: vec![],
            limit: None,
            after_id: Some(EntityId::new("nonexistent-id")),
            count_only: false,
        });

        assert!(
            matches!(result, Err(AxonError::InvalidArgument(_))),
            "expected InvalidArgument for unknown cursor, got {result:?}"
        );
    }

    #[test]
    fn query_count_only() {
        let mut h = handler();
        make_entity_with_data(&mut h, "tasks", "t-1", json!({"status": "open"}));
        make_entity_with_data(&mut h, "tasks", "t-2", json!({"status": "open"}));
        make_entity_with_data(&mut h, "tasks", "t-3", json!({"status": "done"}));

        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("tasks"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "status".into(),
                    op: FilterOp::Eq,
                    value: json!("open"),
                })),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: true,
            })
            .unwrap();

        assert_eq!(resp.total_count, 2);
        assert!(resp.entities.is_empty());
    }

    #[test]
    fn query_dot_path_field() {
        let mut h = handler();
        make_entity_with_data(
            &mut h,
            "contacts",
            "c-1",
            json!({"address": {"city": "Berlin"}}),
        );
        make_entity_with_data(
            &mut h,
            "contacts",
            "c-2",
            json!({"address": {"city": "Paris"}}),
        );

        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("contacts"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "address.city".into(),
                    op: FilterOp::Eq,
                    value: json!("Berlin"),
                })),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.total_count, 1);
        assert_eq!(resp.entities[0].data["address"]["city"], "Berlin");
    }

    // ── FilterNode depth limit tests ──────────────────────────────────────────

    /// Build a left-spine And tree of the given depth.
    fn nested_and(depth: usize) -> FilterNode {
        let leaf = FilterNode::Field(FieldFilter {
            field: "x".into(),
            op: FilterOp::Eq,
            value: json!(1),
        });
        if depth <= 1 {
            return leaf;
        }
        FilterNode::And {
            filters: vec![nested_and(depth - 1)],
        }
    }

    #[test]
    fn filter_depth_at_max_succeeds() {
        let mut h = handler();
        make_entity_with_data(&mut h, "items", "i-1", json!({"x": 1}));

        let result = h.query_entities(QueryEntitiesRequest {
            collection: CollectionId::new("items"),
            filter: Some(nested_and(MAX_FILTER_DEPTH)),
            sort: vec![],
            limit: None,
            after_id: None,
            count_only: false,
        });

        assert!(result.is_ok(), "filter at max depth should succeed");
    }

    #[test]
    fn filter_depth_exceeds_max_returns_invalid_argument() {
        let mut h = handler();
        make_entity_with_data(&mut h, "items", "i-1", json!({"x": 1}));

        let result = h.query_entities(QueryEntitiesRequest {
            collection: CollectionId::new("items"),
            filter: Some(nested_and(MAX_FILTER_DEPTH + 1)),
            sort: vec![],
            limit: None,
            after_id: None,
            count_only: false,
        });

        match result {
            Err(AxonError::InvalidArgument(msg)) => {
                assert!(
                    msg.contains("depth"),
                    "error message should mention depth: {msg}"
                );
            }
            other => panic!("expected InvalidArgument, got {other:?}"),
        }
    }

    #[test]
    fn filter_deeply_nested_no_stack_overflow() {
        // 1000 levels deep — well beyond MAX_FILTER_DEPTH but should not
        // overflow the stack; it must return InvalidArgument instead.
        let h = handler();
        let result = h.query_entities(QueryEntitiesRequest {
            collection: CollectionId::new("items"),
            filter: Some(nested_and(1000)),
            sort: vec![],
            limit: None,
            after_id: None,
            count_only: false,
        });

        assert!(
            matches!(result, Err(AxonError::InvalidArgument(_))),
            "deeply nested filter must return InvalidArgument, not stack overflow"
        );
    }

    #[test]
    fn filter_depth_iterative_100k_deep_no_stack_overflow() {
        // Build a linear chain of depth 100_000. The old recursive implementation
        // would overflow the stack; the iterative implementation must not.
        let leaf = FilterNode::Field(FieldFilter {
            field: "x".to_string(),
            op: FilterOp::Eq,
            value: serde_json::json!(1),
        });
        let mut node = leaf;
        for _ in 0..99_999 {
            node = FilterNode::And {
                filters: vec![node],
            };
        }
        let depth = filter_depth(&node);
        // Avoid recursive Drop stack overflow on the deep tree; the tree is
        // intentionally leaked here — this is test-only and the process exits anyway.
        std::mem::forget(node);
        assert_eq!(
            depth, 100_000,
            "iterative filter_depth must return exact depth for deep tree"
        );
    }

    // ── Schema persistence ───────────────────────────────────────────────────

    #[test]
    fn put_schema_then_get_schema_roundtrip() {
        let mut h = handler();
        let col = CollectionId::new("invoices");
        let schema = axon_schema::schema::CollectionSchema {
            collection: col.clone(),
            description: Some("Invoice collection".into()),
            version: 1,
            entity_schema: Some(json!({"type": "object"})),
            link_types: Default::default(),
        };

        h.put_schema(schema.clone()).unwrap();

        let retrieved = h
            .get_schema(&col)
            .unwrap()
            .expect("schema should be retrievable after put_schema");
        assert_eq!(retrieved.collection, col);
        assert_eq!(retrieved.version, 1);
        assert_eq!(retrieved.description.as_deref(), Some("Invoice collection"));
    }

    #[test]
    fn get_schema_missing_returns_none() {
        let h = handler();
        let result = h.get_schema(&CollectionId::new("nonexistent")).unwrap();
        assert!(result.is_none(), "missing schema should return None");
    }

    #[test]
    fn handle_get_schema_missing_returns_not_found() {
        let h = handler();
        let err = h
            .handle_get_schema(GetSchemaRequest {
                collection: CollectionId::new("nope"),
            })
            .unwrap_err();
        assert!(
            matches!(err, AxonError::NotFound(_)),
            "expected NotFound, got {err:?}"
        );
    }

    #[test]
    fn handle_put_schema_creates_audit_entry() {
        let mut h = handler();
        let col = CollectionId::new("invoices");
        let schema = axon_schema::schema::CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: None,
            link_types: Default::default(),
        };

        h.handle_put_schema(PutSchemaRequest {
            schema,
            actor: Some("alice".into()),
        })
        .unwrap();

        let entries = h
            .audit_log()
            .query_by_operation(&axon_audit::entry::MutationType::SchemaUpdate)
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].collection, col);
        assert_eq!(entries[0].actor, "alice");
    }

    #[test]
    fn put_schema_persists_across_handler_method_calls() {
        // Verify that schema written via put_schema is visible to create_entity validation.
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let schema = EsfDocument::parse(TASK_ESF)
            .unwrap()
            .into_collection_schema()
            .unwrap();

        h.put_schema(schema).unwrap();

        // Invalid entity should be rejected.
        let err = h
            .create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: EntityId::new("t-bad"),
                data: json!({"done": false}), // missing required "title"
                actor: None,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::SchemaValidation(_)));

        // Valid entity should be accepted.
        h.create_entity(CreateEntityRequest {
            collection: col,
            id: EntityId::new("t-good"),
            data: json!({"title": "ok", "done": false}),
            actor: None,
        })
        .unwrap();
    }

    #[test]
    fn put_schema_rejects_invalid_entity_schema() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let schema = axon_schema::schema::CollectionSchema {
            collection: col,
            description: None,
            version: 1,
            entity_schema: Some(json!({"type": "bogus"})),
            link_types: Default::default(),
        };

        let err = h.put_schema(schema).unwrap_err();
        assert!(
            matches!(err, AxonError::SchemaValidation(_)),
            "expected SchemaValidation error, got: {err}"
        );
    }

    #[test]
    fn handle_put_schema_rejects_invalid_json_schema() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let schema = axon_schema::schema::CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: Some(json!({"type": "bogus"})),
            link_types: Default::default(),
        };

        let err = h
            .handle_put_schema(PutSchemaRequest {
                schema,
                actor: None,
            })
            .unwrap_err();
        assert!(
            matches!(err, AxonError::SchemaValidation(_)),
            "expected SchemaValidation error, got: {err}"
        );
    }

    #[test]
    fn handle_put_schema_accepts_valid_json_schema() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let schema = axon_schema::schema::CollectionSchema {
            collection: col,
            description: None,
            version: 1,
            entity_schema: Some(
                json!({"type": "object", "properties": {"title": {"type": "string"}}}),
            ),
            link_types: Default::default(),
        };

        h.handle_put_schema(PutSchemaRequest {
            schema,
            actor: None,
        })
        .unwrap();
    }

    #[test]
    fn drop_collection_removes_schema() {
        let mut h = handler();
        let col = CollectionId::new("invoices");

        // Explicit collection create so drop_collection can find it.
        // Schema version 1 is persisted as part of create_collection.
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema::new(col.clone()),
            actor: None,
        })
        .unwrap();
        assert!(h.get_schema(&col).unwrap().is_some());

        h.drop_collection(DropCollectionRequest {
            name: col.clone(),
            actor: None,
        })
        .unwrap();

        assert!(
            h.get_schema(&col).unwrap().is_none(),
            "schema must be removed when collection is dropped"
        );
    }
}
