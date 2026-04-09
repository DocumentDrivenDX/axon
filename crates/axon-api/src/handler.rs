use std::collections::{HashSet, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

use axon_audit::entry::{AuditEntry, MutationType};
use axon_audit::log::{AuditLog, AuditPage, AuditQuery, MemoryAuditLog};
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId, Namespace, DEFAULT_SCHEMA};
use axon_core::types::{Entity, Link};
use axon_schema::gates::evaluate_gates;
use axon_schema::schema::CollectionSchema;
use axon_schema::validation::{compile_entity_schema, validate, validate_link_metadata};
use axon_storage::adapter::{extract_index_value, resolve_field_path, StorageAdapter};

use crate::request::{
    AggregateFunction, AggregateRequest, CountEntitiesRequest, CreateCollectionRequest,
    CreateDatabaseRequest, CreateEntityRequest, CreateLinkRequest, CreateNamespaceRequest,
    DeleteEntityRequest, DeleteLinkRequest, DescribeCollectionRequest, DiffSchemaRequest,
    DropCollectionRequest, DropDatabaseRequest, DropNamespaceRequest, FieldFilter, FilterNode,
    FilterOp, GetEntityRequest, GetSchemaRequest, ListCollectionsRequest, ListDatabasesRequest,
    ListNamespaceCollectionsRequest, ListNamespacesRequest, PatchEntityRequest, PutSchemaRequest,
    QueryAuditRequest, QueryEntitiesRequest, ReachableRequest, RevalidateRequest,
    RevertEntityRequest, SortDirection, TraverseDirection, TraverseRequest, UpdateEntityRequest,
};
use crate::response::{
    AggregateGroup, AggregateResponse, CollectionMetadata, CountEntitiesResponse, CountGroup,
    CreateCollectionResponse, CreateDatabaseResponse, CreateEntityResponse, CreateLinkResponse,
    CreateNamespaceResponse, DeleteEntityResponse, DeleteLinkResponse, DescribeCollectionResponse,
    DiffSchemaResponse, DropCollectionResponse, DropDatabaseResponse, DropNamespaceResponse,
    GetEntityMarkdownResponse, GetEntityResponse, GetSchemaResponse, InvalidEntity,
    ListCollectionsResponse, ListDatabasesResponse, ListNamespaceCollectionsResponse,
    ListNamespacesResponse, PatchEntityResponse, PutSchemaResponse, QueryAuditResponse,
    QueryEntitiesResponse, ReachableResponse, RevalidateResponse, RevertEntityResponse,
    TraverseHop, TraversePath, TraverseResponse, UpdateEntityResponse,
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
    fn present_entity(requested: &CollectionId, mut entity: Entity) -> Entity {
        entity.collection = requested.clone();
        entity
    }

    fn present_entities(requested: &CollectionId, entities: Vec<Entity>) -> Vec<Entity> {
        entities
            .into_iter()
            .map(|entity| Self::present_entity(requested, entity))
            .collect()
    }

    fn present_schema(requested: &CollectionId, mut schema: CollectionSchema) -> CollectionSchema {
        schema.collection = requested.clone();
        schema
    }

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

        // Validate index declarations (FEAT-013).
        for idx in &schema.indexes {
            if idx.field.is_empty() {
                return Err(AxonError::SchemaValidation(
                    "index declaration has an empty field path".into(),
                ));
            }
        }

        // Validate rule definitions (US-069).
        if !schema.validation_rules.is_empty() {
            let rule_errors = axon_schema::rules::validate_rule_definitions(
                &schema.validation_rules,
                schema.entity_schema.as_ref(),
            );
            if !rule_errors.is_empty() {
                let msgs: Vec<String> = rule_errors.iter().map(|e| e.to_string()).collect();
                return Err(AxonError::SchemaValidation(format!(
                    "invalid validation rules: {}",
                    msgs.join("; ")
                )));
            }
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

    /// Returns a reference to the internal audit log (useful in tests).
    pub fn audit_log(&self) -> &MemoryAuditLog {
        &self.audit
    }

    /// Mutable reference to the internal audit log (used by transaction tests).
    pub fn audit_log_mut(&mut self) -> &mut MemoryAuditLog {
        &mut self.audit
    }

    /// Read-only access to the underlying storage adapter.
    pub fn storage_ref(&self) -> &S {
        &self.storage
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
        let schema = self.storage.get_schema(&req.collection)?;
        if let Some(schema) = &schema {
            validate(schema, &req.data)?;
        }

        // Gate evaluation (ESF Layer 5).
        let gate_eval = if let Some(schema) = &schema {
            if schema.validation_rules.is_empty() {
                None
            } else {
                let eval = evaluate_gates(&schema.validation_rules, &schema.gates, &req.data);
                // Save gate blocks persistence.
                if !eval.save_passes() {
                    return Err(AxonError::SchemaValidation(format!(
                        "save gate failed: {}",
                        eval.save_violations
                            .iter()
                            .map(|v| v.message.as_str())
                            .collect::<Vec<_>>()
                            .join("; ")
                    )));
                }
                Some(eval)
            }
        } else {
            None
        };

        // Unique index constraint check (FEAT-013, US-032).
        if let Some(ref s) = schema {
            check_unique_constraints(&self.storage, &req.collection, &req.id, &req.data, s)?;
        }

        let now = now_ns();
        let mut entity = Entity::new(req.collection, req.id, req.data);
        entity.created_at_ns = Some(now);
        entity.updated_at_ns = Some(now);
        entity.created_by = req.actor.clone();
        entity.updated_by = req.actor.clone();
        self.storage.put(entity.clone())?;

        // Index maintenance (FEAT-013).
        if let Some(ref s) = schema {
            if !s.indexes.is_empty() {
                self.storage.update_indexes(
                    &entity.collection,
                    &entity.id,
                    None,
                    &entity.data,
                    &s.indexes,
                )?;
            }
            if !s.compound_indexes.is_empty() {
                self.storage.update_compound_indexes(
                    &entity.collection,
                    &entity.id,
                    None,
                    &entity.data,
                    &s.compound_indexes,
                )?;
            }
        }

        // Audit.
        let mut audit_entry = AuditEntry::new(
            entity.collection.clone(),
            entity.id.clone(),
            entity.version,
            MutationType::EntityCreate,
            None,
            Some(entity.data.clone()),
            req.actor,
        );
        if let Some(meta) = req.audit_metadata {
            audit_entry = audit_entry.with_metadata(meta);
        }
        self.audit.append(audit_entry)?;

        let (gates, advisories) = match gate_eval {
            Some(eval) => {
                // Materialize gate results to storage (FEAT-019, US-067).
                if !eval.gate_results.is_empty() {
                    let gate_bools: std::collections::HashMap<String, bool> = eval
                        .gate_results
                        .iter()
                        .map(|(name, gr)| (name.clone(), gr.pass))
                        .collect();
                    self.storage
                        .put_gate_results(&entity.collection, &entity.id, &gate_bools)?;
                }
                (eval.gate_results, eval.advisories)
            }
            None => (Default::default(), Vec::new()),
        };

        Ok(CreateEntityResponse {
            entity,
            gates,
            advisories,
        })
    }

    pub fn get_entity(&self, req: GetEntityRequest) -> Result<GetEntityResponse, AxonError> {
        match self.storage.get(&req.collection, &req.id)? {
            Some(entity) => Ok(GetEntityResponse {
                entity: Self::present_entity(&req.collection, entity),
            }),
            None => Err(AxonError::NotFound(req.id.to_string())),
        }
    }

    /// Render a single entity using the collection's stored markdown template.
    ///
    /// Returns [`AxonError::InvalidArgument`] when the collection has no view,
    /// keeping the HTTP surface aligned with FEAT-026's `400 Bad Request`
    /// contract for `format=markdown` without a template.
    pub fn get_entity_markdown(
        &self,
        collection: &CollectionId,
        id: &EntityId,
    ) -> Result<GetEntityMarkdownResponse, AxonError> {
        let entity = self
            .storage
            .get(collection, id)?
            .ok_or_else(|| AxonError::NotFound(id.to_string()))?;
        let view = self
            .storage
            .get_collection_view(collection)?
            .ok_or_else(|| {
                AxonError::InvalidArgument(format!(
                    "collection '{}' has no markdown template defined",
                    collection
                ))
            })?;

        Ok(
            match axon_render::render(&entity, &view.markdown_template) {
                Ok(rendered_markdown) => GetEntityMarkdownResponse::Rendered {
                    entity: Self::present_entity(collection, entity),
                    rendered_markdown,
                },
                Err(error) => GetEntityMarkdownResponse::RenderFailed {
                    entity: Self::present_entity(collection, entity),
                    detail: format!(
                        "failed to render markdown for collection '{}': {error}",
                        collection
                    ),
                },
            },
        )
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
        let schema = self.storage.get_schema(&req.collection)?;
        if let Some(schema) = &schema {
            validate(schema, &req.data)?;
        }

        // Gate evaluation (ESF Layer 5).
        let gate_eval = if let Some(schema) = &schema {
            if schema.validation_rules.is_empty() {
                None
            } else {
                let eval = evaluate_gates(&schema.validation_rules, &schema.gates, &req.data);
                if !eval.save_passes() {
                    return Err(AxonError::SchemaValidation(format!(
                        "save gate failed: {}",
                        eval.save_violations
                            .iter()
                            .map(|v| v.message.as_str())
                            .collect::<Vec<_>>()
                            .join("; ")
                    )));
                }
                Some(eval)
            }
        } else {
            None
        };

        // Unique index constraint check (FEAT-013, US-032).
        if let Some(ref s) = schema {
            check_unique_constraints(&self.storage, &req.collection, &req.id, &req.data, s)?;
        }

        // Read current state for the audit `before` snapshot and metadata preservation.
        let existing = self.storage.get(&req.collection, &req.id)?;
        let before = existing.as_ref().map(|e| e.data.clone());

        // OCC write: preserve created_at/created_by, update updated_at/updated_by.
        let candidate = Entity {
            collection: req.collection.clone(),
            id: req.id,
            version: req.expected_version, // compare_and_swap bumps this to +1
            data: req.data,
            created_at_ns: existing.as_ref().and_then(|e| e.created_at_ns),
            updated_at_ns: Some(now_ns()),
            created_by: existing.as_ref().and_then(|e| e.created_by.clone()),
            updated_by: req.actor.clone(),
        };
        let stored = self
            .storage
            .compare_and_swap(candidate, req.expected_version)?;

        // Index maintenance (FEAT-013).
        if let Some(ref s) = schema {
            if !s.indexes.is_empty() {
                self.storage.update_indexes(
                    &req.collection,
                    &stored.id,
                    before.as_ref(),
                    &stored.data,
                    &s.indexes,
                )?;
            }
            if !s.compound_indexes.is_empty() {
                self.storage.update_compound_indexes(
                    &req.collection,
                    &stored.id,
                    before.as_ref(),
                    &stored.data,
                    &s.compound_indexes,
                )?;
            }
        }

        let updated = Self::present_entity(&req.collection, stored);

        // Audit.
        let mut audit_entry = AuditEntry::new(
            updated.collection.clone(),
            updated.id.clone(),
            updated.version,
            MutationType::EntityUpdate,
            before,
            Some(updated.data.clone()),
            req.actor,
        );
        if let Some(meta) = req.audit_metadata {
            audit_entry = audit_entry.with_metadata(meta);
        }
        self.audit.append(audit_entry)?;

        let (gates, advisories) = match gate_eval {
            Some(eval) => {
                // Materialize gate results to storage (FEAT-019, US-067).
                if !eval.gate_results.is_empty() {
                    let gate_bools: std::collections::HashMap<String, bool> = eval
                        .gate_results
                        .iter()
                        .map(|(name, gr)| (name.clone(), gr.pass))
                        .collect();
                    self.storage
                        .put_gate_results(&req.collection, &updated.id, &gate_bools)?;
                }
                (eval.gate_results, eval.advisories)
            }
            None => (Default::default(), Vec::new()),
        };

        Ok(UpdateEntityResponse {
            entity: updated,
            gates,
            advisories,
        })
    }

    /// Partially update an entity using RFC 7396 JSON Merge Patch.
    ///
    /// Reads the current entity, applies the merge patch, validates the result
    /// against the schema, and writes via OCC (`compare_and_swap`).
    pub fn patch_entity(
        &mut self,
        req: PatchEntityRequest,
    ) -> Result<PatchEntityResponse, AxonError> {
        // Read current entity.
        let existing = self
            .storage
            .get(&req.collection, &req.id)?
            .ok_or_else(|| AxonError::NotFound(req.id.to_string()))?;

        // Apply RFC 7396 merge patch.
        let mut merged = existing.data.clone();
        json_merge_patch(&mut merged, &req.patch);

        // Schema validation on the merged result.
        let schema = self.storage.get_schema(&req.collection)?;
        if let Some(schema) = &schema {
            validate(schema, &merged)?;
        }

        // Gate evaluation (ESF Layer 5).
        let gate_eval = if let Some(schema) = &schema {
            if schema.validation_rules.is_empty() {
                None
            } else {
                let eval = evaluate_gates(&schema.validation_rules, &schema.gates, &merged);
                if !eval.save_passes() {
                    return Err(AxonError::SchemaValidation(format!(
                        "save gate failed: {}",
                        eval.save_violations
                            .iter()
                            .map(|v| v.message.as_str())
                            .collect::<Vec<_>>()
                            .join("; ")
                    )));
                }
                Some(eval)
            }
        } else {
            None
        };

        // Unique index constraint check (FEAT-013, US-032).
        if let Some(ref s) = schema {
            check_unique_constraints(&self.storage, &req.collection, &req.id, &merged, s)?;
        }

        let before = Some(existing.data.clone());

        // OCC write.
        let candidate = Entity {
            collection: req.collection.clone(),
            id: req.id,
            version: req.expected_version,
            data: merged,
            created_at_ns: existing.created_at_ns,
            updated_at_ns: Some(now_ns()),
            created_by: existing.created_by,
            updated_by: req.actor.clone(),
        };
        let stored = self
            .storage
            .compare_and_swap(candidate, req.expected_version)?;

        // Index maintenance (FEAT-013).
        if let Some(ref s) = schema {
            if !s.indexes.is_empty() {
                self.storage.update_indexes(
                    &req.collection,
                    &stored.id,
                    before.as_ref(),
                    &stored.data,
                    &s.indexes,
                )?;
            }
            if !s.compound_indexes.is_empty() {
                self.storage.update_compound_indexes(
                    &req.collection,
                    &stored.id,
                    before.as_ref(),
                    &stored.data,
                    &s.compound_indexes,
                )?;
            }
        }

        let updated = Self::present_entity(&req.collection, stored);

        // Audit.
        let mut audit_entry = AuditEntry::new(
            updated.collection.clone(),
            updated.id.clone(),
            updated.version,
            MutationType::EntityUpdate,
            before,
            Some(updated.data.clone()),
            req.actor,
        );
        if let Some(meta) = req.audit_metadata {
            audit_entry = audit_entry.with_metadata(meta);
        }
        self.audit.append(audit_entry)?;

        let (gates, advisories) = match gate_eval {
            Some(eval) => {
                // Materialize gate results to storage (FEAT-019, US-067).
                if !eval.gate_results.is_empty() {
                    let gate_bools: std::collections::HashMap<String, bool> = eval
                        .gate_results
                        .iter()
                        .map(|(name, gr)| (name.clone(), gr.pass))
                        .collect();
                    self.storage
                        .put_gate_results(&req.collection, &updated.id, &gate_bools)?;
                }
                (eval.gate_results, eval.advisories)
            }
            None => (Default::default(), Vec::new()),
        };

        Ok(PatchEntityResponse {
            entity: updated,
            gates,
            advisories,
        })
    }

    pub fn delete_entity(
        &mut self,
        req: DeleteEntityRequest,
    ) -> Result<DeleteEntityResponse, AxonError> {
        // Referential integrity: reject delete when inbound links exist
        // (unless `force` is set).
        if !req.force {
            let links_rev_col = Link::links_rev_collection();
            let rev_prefix = format!("{}/{}/", req.collection, req.id);
            let rev_start = EntityId::new(&rev_prefix);
            let rev_candidates =
                self.storage
                    .range_scan(&links_rev_col, Some(&rev_start), None, Some(1))?;
            let has_inbound = rev_candidates
                .iter()
                .any(|e| e.id.as_str().starts_with(&rev_prefix));
            if has_inbound {
                return Err(AxonError::InvalidOperation(format!(
                    "entity {}/{} has inbound link(s); delete or re-target those links first, or use force=true",
                    req.collection, req.id
                )));
            }
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

        // Remove index entries before deleting (FEAT-013).
        if let Some(ref data) = before {
            if let Ok(Some(schema)) = self.storage.get_schema(&req.collection) {
                if !schema.indexes.is_empty() {
                    self.storage.remove_index_entries(
                        &req.collection,
                        &req.id,
                        data,
                        &schema.indexes,
                    )?;
                }
                if !schema.compound_indexes.is_empty() {
                    self.storage.remove_compound_index_entries(
                        &req.collection,
                        &req.id,
                        data,
                        &schema.compound_indexes,
                    )?;
                }
            }
        }

        self.storage.delete(&req.collection, &req.id)?;
        // Clean up materialized gate results (FEAT-019).
        self.storage.delete_gate_results(&req.collection, &req.id)?;

        // Audit (only if the entity actually existed).
        if before.is_some() {
            let mut audit_entry = AuditEntry::new(
                req.collection.clone(),
                req.id.clone(),
                version,
                MutationType::EntityDelete,
                before,
                None,
                req.actor,
            );
            if let Some(meta) = req.audit_metadata {
                audit_entry = audit_entry.with_metadata(meta);
            }
            self.audit.append(audit_entry)?;
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

        // Try index-accelerated lookup (FEAT-013) before falling back to scan.
        let schema = self.storage.get_schema(&req.collection)?;
        let index_candidates = try_index_lookup(
            &self.storage,
            &req.collection,
            req.filter.as_ref(),
            schema.as_ref(),
        );

        let all = if let Some(entity_ids) = index_candidates {
            // Fetch entities by ID from the index results.
            let mut entities = Vec::with_capacity(entity_ids.len());
            for eid in &entity_ids {
                if let Some(e) = self.storage.get(&req.collection, eid)? {
                    entities.push(e);
                }
            }
            entities
        } else {
            // Fallback: full scan.
            self.storage.range_scan(&req.collection, None, None, None)?
        };

        // Pre-compute gate evaluations if any gate filters are present.
        let needs_gates = req.filter.as_ref().is_some_and(has_gate_filter);

        // Apply filter (even if we used an index, there may be additional
        // filter predicates or gate filters that need post-filtering).
        let mut matched: Vec<Entity> = all
            .into_iter()
            .filter(|e| {
                req.filter.as_ref().map_or(true, |f| {
                    if needs_gates {
                        if let Some(ref s) = schema {
                            let eval = evaluate_gates(&s.validation_rules, &s.gates, &e.data);
                            apply_filter_with_gates(f, &e.data, Some(&eval))
                        } else {
                            apply_filter(f, &e.data)
                        }
                    } else {
                        apply_filter(f, &e.data)
                    }
                })
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

        let entities = if req.count_only {
            vec![]
        } else {
            Self::present_entities(&req.collection, matched)
        };

        Ok(QueryEntitiesResponse {
            entities,
            total_count,
            next_cursor,
        })
    }

    // ── Aggregation operations (US-062) ────────────────────────────────────────

    /// Count entities with optional filter and GROUP BY.
    pub fn count_entities(
        &self,
        req: CountEntitiesRequest,
    ) -> Result<CountEntitiesResponse, AxonError> {
        // Try index-accelerated lookup (FEAT-013).
        let schema = self.storage.get_schema(&req.collection)?;
        let index_candidates = try_index_lookup(
            &self.storage,
            &req.collection,
            req.filter.as_ref(),
            schema.as_ref(),
        );

        let all = if let Some(entity_ids) = index_candidates {
            let mut entities = Vec::with_capacity(entity_ids.len());
            for eid in &entity_ids {
                if let Some(e) = self.storage.get(&req.collection, eid)? {
                    entities.push(e);
                }
            }
            entities
        } else {
            self.storage.range_scan(&req.collection, None, None, None)?
        };

        // Apply filter (post-filter for remaining predicates).
        let matched: Vec<&Entity> = all
            .iter()
            .filter(|e| {
                req.filter
                    .as_ref()
                    .map_or(true, |f| apply_filter(f, &e.data))
            })
            .collect();

        let total_count = matched.len();

        // Group by field, if requested.
        let groups = if let Some(ref field) = req.group_by {
            let mut group_map: std::collections::BTreeMap<String, usize> =
                std::collections::BTreeMap::new();
            for entity in &matched {
                let key = get_field_value(&entity.data, field)
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let key_str = match &key {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Null => "null".into(),
                    other => other.to_string(),
                };
                *group_map.entry(key_str).or_insert(0) += 1;
            }
            group_map
                .into_iter()
                .map(|(key_str, count)| {
                    let key = if key_str == "null" {
                        serde_json::Value::Null
                    } else {
                        serde_json::Value::String(key_str)
                    };
                    CountGroup { key, count }
                })
                .collect()
        } else {
            vec![]
        };

        Ok(CountEntitiesResponse {
            total_count,
            groups,
        })
    }

    /// Compute a numeric aggregation (SUM, AVG, MIN, MAX) over entities.
    pub fn aggregate(&self, req: AggregateRequest) -> Result<AggregateResponse, AxonError> {
        // Try index-accelerated lookup (FEAT-013).
        let schema = self.storage.get_schema(&req.collection)?;
        let index_candidates = try_index_lookup(
            &self.storage,
            &req.collection,
            req.filter.as_ref(),
            schema.as_ref(),
        );

        let all = if let Some(entity_ids) = index_candidates {
            let mut entities = Vec::with_capacity(entity_ids.len());
            for eid in &entity_ids {
                if let Some(e) = self.storage.get(&req.collection, eid)? {
                    entities.push(e);
                }
            }
            entities
        } else {
            self.storage.range_scan(&req.collection, None, None, None)?
        };

        // Apply filter (post-filter for remaining predicates).
        let matched: Vec<&Entity> = all
            .iter()
            .filter(|e| {
                req.filter
                    .as_ref()
                    .map_or(true, |f| apply_filter(f, &e.data))
            })
            .collect();

        if let Some(ref group_by) = req.group_by {
            // Group by field, then aggregate per group.
            let mut groups: std::collections::BTreeMap<String, Vec<f64>> =
                std::collections::BTreeMap::new();
            for entity in &matched {
                let group_key = get_field_value(&entity.data, group_by)
                    .map(|v| match v {
                        serde_json::Value::String(s) => s.clone(),
                        serde_json::Value::Null => "null".into(),
                        other => other.to_string(),
                    })
                    .unwrap_or_else(|| "null".into());
                let val = get_field_value(&entity.data, &req.field).and_then(|v| v.as_f64());
                if let Some(n) = val {
                    groups.entry(group_key).or_default().push(n);
                } else {
                    // Ensure the group exists even if this entity has null for the agg field.
                    groups.entry(group_key).or_default();
                }
            }

            let results = groups
                .into_iter()
                .filter(|(_, vals)| !vals.is_empty())
                .map(|(key_str, vals)| {
                    let value = compute_aggregate(&req.function, &vals);
                    let key = if key_str == "null" {
                        serde_json::Value::Null
                    } else {
                        serde_json::Value::String(key_str)
                    };
                    AggregateGroup {
                        key,
                        value,
                        count: vals.len(),
                    }
                })
                .collect();

            Ok(AggregateResponse { results })
        } else {
            // No GROUP BY — aggregate all matching.
            let values: Vec<f64> = matched
                .iter()
                .filter_map(|e| get_field_value(&e.data, &req.field).and_then(|v| v.as_f64()))
                .collect();

            // Check if we tried to aggregate but found no numeric values and entities exist.
            if values.is_empty() && !matched.is_empty() {
                // Check if the field exists but is non-numeric.
                let has_non_numeric = matched.iter().any(|e| {
                    get_field_value(&e.data, &req.field)
                        .is_some_and(|v| !v.is_number() && !v.is_null())
                });
                if has_non_numeric {
                    return Err(AxonError::InvalidArgument(format!(
                        "field '{}' is not numeric",
                        req.field
                    )));
                }
            }

            if values.is_empty() {
                return Ok(AggregateResponse { results: vec![] });
            }

            let value = compute_aggregate(&req.function, &values);
            Ok(AggregateResponse {
                results: vec![AggregateGroup {
                    key: serde_json::Value::Null,
                    value,
                    count: values.len(),
                }],
            })
        }
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
                    created_at_ns: existing.created_at_ns,
                    updated_at_ns: Some(now_ns()),
                    created_by: existing.created_by.clone(),
                    updated_by: req.actor.clone(),
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
        let Some(first) = chars.next() else {
            return Err(AxonError::InvalidArgument(format!(
                "collection name '{}' must be 1-128 characters",
                s
            )));
        };
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

        let default_namespace = Namespace::default_ns();
        if self
            .storage
            .collection_registered_in_namespace(&req.name, &default_namespace)?
        {
            return Err(AxonError::AlreadyExists(req.name.to_string()));
        }
        self.storage
            .register_collection_in_namespace(&req.name, &default_namespace)?;
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
        if !req.confirm {
            return Err(AxonError::InvalidArgument(
                "drop_collection requires confirm=true to acknowledge the destructive operation"
                    .into(),
            ));
        }
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
        self.storage.delete_collection_view(&req.name)?;
        self.storage.unregister_collection(&req.name)?;

        let mut drop_meta = std::collections::HashMap::new();
        drop_meta.insert("entities_removed".into(), count.to_string());
        self.audit.append(
            AuditEntry::new(
                req.name.clone(),
                EntityId::new(""),
                0,
                MutationType::CollectionDrop,
                None,
                None,
                req.actor,
            )
            .with_metadata(drop_meta),
        )?;

        Ok(DropCollectionResponse {
            name: req.name.to_string(),
            entities_removed: count,
        })
    }

    fn append_collection_drop_audit_entries(
        &mut self,
        collections: &[CollectionId],
    ) -> Result<(), AxonError> {
        for collection in collections {
            self.audit.append(AuditEntry::new(
                collection.clone(),
                EntityId::new(""),
                0,
                MutationType::CollectionDrop,
                None,
                None,
                None,
            ))?;
        }
        Ok(())
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
                let (created_at_ns, updated_at_ns) =
                    self.collection_timestamps(name).unwrap_or((None, None));
                CollectionMetadata {
                    name: name.to_string(),
                    entity_count,
                    schema_version,
                    created_at_ns,
                    updated_at_ns,
                }
            })
            .collect();

        Ok(ListCollectionsResponse { collections })
    }

    /// Describe a single collection (entity count + full schema + timestamps).
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
        let (created_at_ns, updated_at_ns) = self
            .collection_timestamps(&req.name)
            .unwrap_or((None, None));

        Ok(DescribeCollectionResponse {
            name: req.name.to_string(),
            entity_count,
            schema,
            created_at_ns,
            updated_at_ns,
        })
    }

    /// Derive created_at and updated_at timestamps for a collection from the
    /// audit log. Returns `(created_at_ns, updated_at_ns)`.
    fn collection_timestamps(
        &self,
        collection: &CollectionId,
    ) -> Result<(Option<u64>, Option<u64>), AxonError> {
        let page = self.audit.query_paginated(AuditQuery {
            collection: Some(collection.clone()),
            ..Default::default()
        })?;
        let created_at_ns = page.entries.first().map(|e| e.timestamp_ns);
        let updated_at_ns = page.entries.last().map(|e| e.timestamp_ns);
        Ok((created_at_ns, updated_at_ns))
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

        // Compatibility check against existing schema.
        let existing = self.storage.get_schema(&collection)?;
        let old_entity_schema = existing.as_ref().and_then(|s| s.entity_schema.as_ref());
        let new_entity_schema = req.schema.entity_schema.as_ref();
        let diff = axon_schema::diff_schemas(old_entity_schema, new_entity_schema);
        let compatibility = axon_schema::classify(&diff);

        // Dry-run: return classification without applying.
        if req.dry_run {
            return Ok(PutSchemaResponse {
                schema: req.schema,
                compatibility: Some(compatibility),
                diff: Some(diff),
                dry_run: true,
            });
        }

        // Breaking changes require force flag.
        if compatibility == axon_schema::Compatibility::Breaking && !req.force {
            return Err(AxonError::InvalidOperation(format!(
                "schema change is breaking ({}). Use force=true to apply. Changes: {}",
                diff.changes.len(),
                diff.changes
                    .iter()
                    .map(|c| c.description.as_str())
                    .collect::<Vec<_>>()
                    .join("; "),
            )));
        }

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
        Ok(PutSchemaResponse {
            schema: req.schema,
            compatibility: Some(compatibility),
            diff: Some(diff),
            dry_run: false,
        })
    }

    /// Retrieve the schema for a collection.
    ///
    /// Returns [`AxonError::NotFound`] if no schema has been stored.
    pub fn handle_get_schema(&self, req: GetSchemaRequest) -> Result<GetSchemaResponse, AxonError> {
        self.storage
            .get_schema(&req.collection)?
            .map(|schema| GetSchemaResponse {
                schema: Self::present_schema(&req.collection, schema),
            })
            .ok_or_else(|| {
                AxonError::NotFound(format!("schema for collection '{}'", req.collection))
            })
    }

    /// Revalidate all entities in a collection against the current schema (US-060).
    ///
    /// Scans all entities and reports which ones fail validation, including
    /// the entity ID, version, and specific errors.
    pub fn revalidate(&self, req: RevalidateRequest) -> Result<RevalidateResponse, AxonError> {
        let schema = self.storage.get_schema(&req.collection)?.ok_or_else(|| {
            AxonError::NotFound(format!("schema for collection '{}'", req.collection))
        })?;

        let all = self.storage.range_scan(&req.collection, None, None, None)?;
        let total_scanned = all.len();
        let mut invalid = Vec::new();

        for entity in &all {
            if let Err(errs) = axon_schema::validate_entity(&schema, &entity.data) {
                invalid.push(InvalidEntity {
                    id: entity.id.to_string(),
                    version: entity.version,
                    errors: errs.0.iter().map(|e| e.to_string()).collect(),
                });
            }
        }

        let valid_count = total_scanned - invalid.len();

        Ok(RevalidateResponse {
            total_scanned,
            valid_count,
            invalid,
        })
    }

    // ── Namespace management (US-036) ───────────────────────────────────────

    /// Create a schema namespace (database.schema).
    pub fn create_namespace(
        &mut self,
        req: CreateNamespaceRequest,
    ) -> Result<CreateNamespaceResponse, AxonError> {
        self.storage
            .create_namespace(&Namespace::new(&req.database, &req.schema))?;
        Ok(CreateNamespaceResponse {
            database: req.database,
            schema: req.schema,
        })
    }

    /// List schemas within a database.
    pub fn list_namespaces(
        &self,
        req: ListNamespacesRequest,
    ) -> Result<ListNamespacesResponse, AxonError> {
        let schemas = self.storage.list_namespaces(&req.database)?;
        Ok(ListNamespacesResponse {
            database: req.database,
            schemas,
        })
    }

    /// List collections within a namespace.
    pub fn list_namespace_collections(
        &self,
        req: ListNamespaceCollectionsRequest,
    ) -> Result<ListNamespaceCollectionsResponse, AxonError> {
        let collections = self
            .storage
            .list_namespace_collections(&Namespace::new(&req.database, &req.schema))?
            .into_iter()
            .map(|collection| collection.to_string())
            .collect();
        Ok(ListNamespaceCollectionsResponse {
            database: req.database,
            schema: req.schema,
            collections,
        })
    }

    /// Drop a namespace. Fails if non-empty unless force is set.
    pub fn drop_namespace(
        &mut self,
        req: DropNamespaceRequest,
    ) -> Result<DropNamespaceResponse, AxonError> {
        if req.schema == DEFAULT_SCHEMA {
            return Err(AxonError::InvalidOperation(format!(
                "schema '{}' cannot be dropped",
                req.schema
            )));
        }

        let namespace = Namespace::new(&req.database, &req.schema);
        let collections = self.storage.list_namespace_collections(&namespace)?;
        let count = collections.len();
        if count > 0 && !req.force {
            return Err(AxonError::InvalidOperation(format!(
                "namespace '{}' contains {} collections: {}. Use force=true to drop",
                namespace,
                count,
                collections
                    .iter()
                    .take(5)
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }

        self.storage.drop_namespace(&namespace)?;
        self.append_collection_drop_audit_entries(&collections)?;
        Ok(DropNamespaceResponse {
            database: req.database,
            schema: req.schema,
            collections_removed: count,
        })
    }

    // ── Database isolation (US-035, FEAT-014) ───────────────────────────

    /// Create a new database (isolated data space).
    ///
    /// A database is a namespace prefix that groups collections.
    /// Collections in different databases are invisible to each other.
    pub fn create_database(
        &mut self,
        req: CreateDatabaseRequest,
    ) -> Result<CreateDatabaseResponse, AxonError> {
        self.storage.create_database(&req.name)?;
        Ok(CreateDatabaseResponse { name: req.name })
    }

    /// Drop a database and all its collections.
    ///
    /// Removes all namespaces with the given database prefix and their
    /// collections. Audit entries are retained but the data is purged.
    pub fn drop_database(
        &mut self,
        req: DropDatabaseRequest,
    ) -> Result<DropDatabaseResponse, AxonError> {
        if !self.storage.list_databases()?.contains(&req.name) {
            return Err(AxonError::NotFound(format!("database '{}'", req.name)));
        }

        let schemas = self.storage.list_namespaces(&req.name)?;
        let mut collections = Vec::new();
        for schema in &schemas {
            collections.extend(
                self.storage
                    .list_namespace_collections(&Namespace::new(&req.name, schema))?,
            );
        }
        let total_collections = collections.len();

        if total_collections > 0 && !req.force {
            return Err(AxonError::InvalidOperation(format!(
                "database '{}' contains {total_collections} collections. Use force=true to drop",
                req.name
            )));
        }

        self.storage.drop_database(&req.name)?;
        self.append_collection_drop_audit_entries(&collections)?;

        Ok(DropDatabaseResponse {
            name: req.name,
            collections_removed: total_collections,
        })
    }

    /// List all databases.
    pub fn list_databases(
        &self,
        _req: ListDatabasesRequest,
    ) -> Result<ListDatabasesResponse, AxonError> {
        Ok(ListDatabasesResponse {
            databases: self.storage.list_databases()?,
        })
    }

    /// Diff two schema versions for a collection (US-061).
    ///
    /// Retrieves both versions from storage and produces a field-level diff.
    pub fn diff_schema_versions(
        &self,
        req: DiffSchemaRequest,
    ) -> Result<DiffSchemaResponse, AxonError> {
        let schema_a = self
            .storage
            .get_schema_version(&req.collection, req.version_a)?
            .ok_or_else(|| {
                AxonError::NotFound(format!(
                    "schema version {} for collection '{}'",
                    req.version_a, req.collection
                ))
            })?;
        let schema_b = self
            .storage
            .get_schema_version(&req.collection, req.version_b)?
            .ok_or_else(|| {
                AxonError::NotFound(format!(
                    "schema version {} for collection '{}'",
                    req.version_b, req.collection
                ))
            })?;

        let diff = axon_schema::diff_schemas(
            schema_a.entity_schema.as_ref(),
            schema_b.entity_schema.as_ref(),
        );

        Ok(DiffSchemaResponse {
            version_a: req.version_a,
            version_b: req.version_b,
            diff,
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

                // Enforce cardinality constraints.
                use axon_schema::Cardinality;
                match link_def.cardinality {
                    Cardinality::OneToOne | Cardinality::ManyToOne => {
                        // Source can have at most one outgoing link of this type.
                        let prefix = format!(
                            "{}/{}/{}/",
                            req.source_collection, req.source_id, req.link_type
                        );
                        let start = EntityId::new(&prefix);
                        let existing = self.storage.range_scan(
                            &Link::links_collection(),
                            Some(&start),
                            None,
                            Some(1),
                        )?;
                        let has_outgoing =
                            existing.iter().any(|e| e.id.as_str().starts_with(&prefix));
                        if has_outgoing {
                            return Err(AxonError::SchemaValidation(format!(
                                "cardinality violation: source {}/{} already has a '{}' link \
                                 ({:?} allows at most one outgoing)",
                                req.source_collection,
                                req.source_id,
                                req.link_type,
                                link_def.cardinality
                            )));
                        }
                    }
                    Cardinality::OneToMany | Cardinality::ManyToMany => {}
                }
                match link_def.cardinality {
                    Cardinality::OneToOne | Cardinality::OneToMany => {
                        // Target can have at most one inbound link of this type.
                        // Scan the reverse-index: {target_col}/{target_id}/.../{link_type}
                        let rev_col = Link::links_rev_collection();
                        let prefix = format!("{}/{}/", req.target_collection, req.target_id);
                        let start = EntityId::new(&prefix);
                        let candidates =
                            self.storage
                                .range_scan(&rev_col, Some(&start), None, None)?;
                        let has_inbound = candidates.iter().any(|e| {
                            let id = e.id.as_str();
                            id.starts_with(&prefix) && id.ends_with(&format!("/{}", req.link_type))
                        });
                        if has_inbound {
                            return Err(AxonError::SchemaValidation(format!(
                                "cardinality violation: target {}/{} already has an inbound '{}' link \
                                 ({:?} allows at most one inbound)",
                                req.target_collection,
                                req.target_id,
                                req.link_type,
                                link_def.cardinality
                            )));
                        }
                    }
                    Cardinality::ManyToOne | Cardinality::ManyToMany => {}
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
    /// in BFS order. Supports forward (outbound) and reverse (inbound) traversal,
    /// per-hop entity filtering, and path/link metadata reporting.
    pub fn traverse(&self, req: TraverseRequest) -> Result<TraverseResponse, AxonError> {
        let max_depth = req
            .max_depth
            .unwrap_or(DEFAULT_MAX_DEPTH)
            .min(MAX_DEPTH_CAP);

        let all_links = self.load_all_links()?;
        let reverse = req.direction == TraverseDirection::Reverse;

        let mut visited: HashSet<(String, String)> = HashSet::new();
        let start_key = (req.collection.to_string(), req.id.to_string());
        visited.insert(start_key);

        // Queue entries: (collection, id, current_depth, path_so_far)
        let mut queue: VecDeque<(CollectionId, EntityId, usize, Vec<TraverseHop>)> =
            VecDeque::new();
        queue.push_back((req.collection, req.id, 0, Vec::new()));

        let mut entities = Vec::new();
        let mut paths = Vec::new();
        let mut links_traversed = Vec::new();

        while let Some((col, id, depth, path)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }

            let neighbors: Vec<&Link> = if reverse {
                all_links
                    .iter()
                    .filter(|l| {
                        l.target_collection == col
                            && l.target_id == id
                            && req
                                .link_type
                                .as_deref()
                                .map_or(true, |lt| l.link_type == lt)
                    })
                    .collect()
            } else {
                all_links
                    .iter()
                    .filter(|l| {
                        l.source_collection == col
                            && l.source_id == id
                            && req
                                .link_type
                                .as_deref()
                                .map_or(true, |lt| l.link_type == lt)
                    })
                    .collect()
            };

            for link in neighbors {
                let (next_col, next_id) = if reverse {
                    (&link.source_collection, &link.source_id)
                } else {
                    (&link.target_collection, &link.target_id)
                };

                let neighbor_key = (next_col.to_string(), next_id.to_string());
                if visited.contains(&neighbor_key) {
                    continue;
                }

                if let Some(entity) = self.storage.get(next_col, next_id)? {
                    // Apply hop filter if present.
                    if let Some(ref filter) = req.hop_filter {
                        if !apply_filter(filter, &entity.data) {
                            continue;
                        }
                    }

                    visited.insert(neighbor_key);
                    links_traversed.push(link.clone());

                    let mut hop_path = path.clone();
                    hop_path.push(TraverseHop {
                        link: link.clone(),
                        entity: entity.clone(),
                    });

                    paths.push(TraversePath {
                        hops: hop_path.clone(),
                    });
                    entities.push(entity);
                    queue.push_back((next_col.clone(), next_id.clone(), depth + 1, hop_path));
                }
            }
        }

        Ok(TraverseResponse {
            entities,
            paths,
            links: links_traversed,
        })
    }

    /// Check whether a target entity is reachable from a source entity.
    ///
    /// Short-circuits BFS as soon as the target is found, returning `true`
    /// and the hop depth. More efficient than a full `traverse()` when only
    /// connectivity matters.
    pub fn reachable(&self, req: ReachableRequest) -> Result<ReachableResponse, AxonError> {
        let max_depth = req
            .max_depth
            .unwrap_or(DEFAULT_MAX_DEPTH)
            .min(MAX_DEPTH_CAP);

        let all_links = self.load_all_links()?;
        let reverse = req.direction == TraverseDirection::Reverse;
        let target_key = (req.target_collection.to_string(), req.target_id.to_string());

        let mut visited: HashSet<(String, String)> = HashSet::new();
        let start_key = (req.source_collection.to_string(), req.source_id.to_string());

        // Check trivial case: source == target.
        if start_key == target_key {
            return Ok(ReachableResponse {
                reachable: true,
                depth: Some(0),
            });
        }

        visited.insert(start_key);

        let mut queue: VecDeque<(CollectionId, EntityId, usize)> = VecDeque::new();
        queue.push_back((req.source_collection, req.source_id, 0));

        while let Some((col, id, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }

            let neighbors: Vec<&Link> = if reverse {
                all_links
                    .iter()
                    .filter(|l| {
                        l.target_collection == col
                            && l.target_id == id
                            && req
                                .link_type
                                .as_deref()
                                .map_or(true, |lt| l.link_type == lt)
                    })
                    .collect()
            } else {
                all_links
                    .iter()
                    .filter(|l| {
                        l.source_collection == col
                            && l.source_id == id
                            && req
                                .link_type
                                .as_deref()
                                .map_or(true, |lt| l.link_type == lt)
                    })
                    .collect()
            };

            for link in neighbors {
                let (next_col, next_id) = if reverse {
                    (&link.source_collection, &link.source_id)
                } else {
                    (&link.target_collection, &link.target_id)
                };

                let neighbor_key = (next_col.to_string(), next_id.to_string());
                if visited.contains(&neighbor_key) {
                    continue;
                }

                // Short-circuit: found the target (check before consuming the key).
                if neighbor_key == target_key {
                    return Ok(ReachableResponse {
                        reachable: true,
                        depth: Some(depth + 1),
                    });
                }

                visited.insert(neighbor_key);

                queue.push_back((next_col.clone(), next_id.clone(), depth + 1));
            }
        }

        Ok(ReachableResponse {
            reachable: false,
            depth: None,
        })
    }

    /// Find candidate target entities for a link type (US-070, FEAT-020).
    ///
    /// Returns entities from the target collection with an already-linked
    /// indicator, cardinality info, and existing link count.
    pub fn find_link_candidates(
        &self,
        req: crate::request::FindLinkCandidatesRequest,
    ) -> Result<crate::response::FindLinkCandidatesResponse, AxonError> {
        // Verify source entity exists.
        if self
            .storage
            .get(&req.source_collection, &req.source_id)?
            .is_none()
        {
            return Err(AxonError::NotFound(format!(
                "{}/{}",
                req.source_collection, req.source_id
            )));
        }

        // Look up link type definition from source schema.
        let source_schema = self.storage.get_schema(&req.source_collection)?;
        let link_def = source_schema
            .as_ref()
            .and_then(|s| s.link_types.get(&req.link_type));

        let target_collection = link_def
            .map(|d| CollectionId::new(&d.target_collection))
            .unwrap_or_else(|| req.source_collection.clone());

        let cardinality_str = link_def
            .map(|d| {
                format!("{:?}", d.cardinality)
                    .to_lowercase()
                    .replace("to", "-to-")
            })
            .unwrap_or_else(|| "unknown".into());

        // Get all existing links of this type from the source.
        let all_links = self.load_all_links()?;
        let existing_targets: HashSet<String> = all_links
            .iter()
            .filter(|l| {
                l.source_collection == req.source_collection
                    && l.source_id == req.source_id
                    && l.link_type == req.link_type
            })
            .map(|l| l.target_id.to_string())
            .collect();
        let existing_link_count = existing_targets.len();

        // Fetch candidate entities from the target collection (FEAT-013 index acceleration).
        let target_schema = self.storage.get_schema(&target_collection)?;
        let index_candidates = try_index_lookup(
            &self.storage,
            &target_collection,
            req.filter.as_ref(),
            target_schema.as_ref(),
        );
        let all_targets = if let Some(entity_ids) = index_candidates {
            let mut entities = Vec::with_capacity(entity_ids.len());
            for eid in &entity_ids {
                if let Some(e) = self.storage.get(&target_collection, eid)? {
                    entities.push(e);
                }
            }
            entities
        } else {
            self.storage
                .range_scan(&target_collection, None, None, None)?
        };

        // Filter and collect candidates.
        let limit = req.limit.unwrap_or(50);
        let candidates: Vec<crate::response::LinkCandidate> = all_targets
            .into_iter()
            .filter(|e| {
                req.filter
                    .as_ref()
                    .map_or(true, |f| apply_filter(f, &e.data))
            })
            .take(limit)
            .map(|e| {
                let already_linked = existing_targets.contains(e.id.as_str());
                crate::response::LinkCandidate {
                    entity: e,
                    already_linked,
                }
            })
            .collect();

        Ok(crate::response::FindLinkCandidatesResponse {
            target_collection: target_collection.to_string(),
            link_type: req.link_type,
            cardinality: cardinality_str,
            existing_link_count,
            candidates,
        })
    }

    /// List an entity's neighbors: outbound + inbound linked entities
    /// grouped by link type and direction (US-071, FEAT-020).
    pub fn list_neighbors(
        &self,
        req: crate::request::ListNeighborsRequest,
    ) -> Result<crate::response::ListNeighborsResponse, AxonError> {
        use std::collections::BTreeMap;

        // Verify entity exists.
        if self.storage.get(&req.collection, &req.id)?.is_none() {
            return Err(AxonError::NotFound(format!(
                "{}/{}",
                req.collection, req.id
            )));
        }

        let all_links = self.load_all_links()?;

        // group key: (link_type, direction)
        let mut groups: BTreeMap<(String, String), Vec<Entity>> = BTreeMap::new();

        let include_outbound = req
            .direction
            .as_ref()
            .map_or(true, |d| *d == TraverseDirection::Forward);
        let include_inbound = req
            .direction
            .as_ref()
            .map_or(true, |d| *d == TraverseDirection::Reverse);

        for link in &all_links {
            let type_filter_ok = req
                .link_type
                .as_deref()
                .map_or(true, |lt| link.link_type == lt);
            if !type_filter_ok {
                continue;
            }

            // Outbound: this entity is the source.
            if include_outbound
                && link.source_collection == req.collection
                && link.source_id == req.id
            {
                let key = (link.link_type.clone(), "outbound".to_string());
                if let Some(target) = self.storage.get(&link.target_collection, &link.target_id)? {
                    groups.entry(key).or_default().push(target);
                }
            }

            // Inbound: this entity is the target.
            if include_inbound
                && link.target_collection == req.collection
                && link.target_id == req.id
            {
                let key = (link.link_type.clone(), "inbound".to_string());
                if let Some(source) = self.storage.get(&link.source_collection, &link.source_id)? {
                    groups.entry(key).or_default().push(source);
                }
            }
        }

        let mut total_count = 0;
        let result_groups: Vec<crate::response::NeighborGroup> = groups
            .into_iter()
            .map(|((link_type, direction), entities)| {
                total_count += entities.len();
                crate::response::NeighborGroup {
                    link_type,
                    direction,
                    entities,
                }
            })
            .collect();

        Ok(crate::response::ListNeighborsResponse {
            groups: result_groups,
            total_count,
        })
    }

    /// Load all stored links from the internal links collection.
    fn load_all_links(&self) -> Result<Vec<Link>, AxonError> {
        let links_col = Link::links_collection();
        let entities = self.storage.range_scan(&links_col, None, None, None)?;
        Ok(entities.iter().filter_map(Link::from_entity).collect())
    }
}

// ── Index-accelerated query planner (FEAT-013) ─────────────────────────────────

/// Attempt to use a secondary index to satisfy a filter.
///
/// Returns `Some(entity_ids)` if the filter can be satisfied by an index lookup.
/// Returns `None` to indicate the caller should fall back to a full scan.
///
/// Currently handles:
/// - Single `FieldFilter` with `Eq` op when the field has a declared index
/// - Single `FieldFilter` with `Gt`/`Gte`/`Lt`/`Lte` op for range queries
/// - `And` of equality filters where any single field has an index (picks first)
fn try_index_lookup<S: StorageAdapter>(
    storage: &S,
    collection: &CollectionId,
    filter: Option<&FilterNode>,
    schema: Option<&CollectionSchema>,
) -> Option<Vec<EntityId>> {
    let filter = filter?;
    let schema = schema?;
    if schema.indexes.is_empty() {
        return None;
    }

    match filter {
        FilterNode::Field(f) => {
            // Find an index matching this field.
            let idx = schema.indexes.iter().find(|i| i.field == f.field)?;
            let val = axon_storage::extract_index_value(&f.value, &idx.index_type)?;

            match f.op {
                FilterOp::Eq => storage.index_lookup(collection, &f.field, &val).ok(),
                FilterOp::Gt => storage
                    .index_range(
                        collection,
                        &f.field,
                        std::ops::Bound::Excluded(&val),
                        std::ops::Bound::Unbounded,
                    )
                    .ok(),
                FilterOp::Gte => storage
                    .index_range(
                        collection,
                        &f.field,
                        std::ops::Bound::Included(&val),
                        std::ops::Bound::Unbounded,
                    )
                    .ok(),
                FilterOp::Lt => storage
                    .index_range(
                        collection,
                        &f.field,
                        std::ops::Bound::Unbounded,
                        std::ops::Bound::Excluded(&val),
                    )
                    .ok(),
                FilterOp::Lte => storage
                    .index_range(
                        collection,
                        &f.field,
                        std::ops::Bound::Unbounded,
                        std::ops::Bound::Included(&val),
                    )
                    .ok(),
                _ => None, // Ne, In, Contains — fall back to scan
            }
        }
        FilterNode::And { filters } => {
            // Try to find at least one equality sub-filter with an index.
            for sub in filters {
                if let FilterNode::Field(f) = sub {
                    if f.op == FilterOp::Eq {
                        if let Some(idx) = schema.indexes.iter().find(|i| i.field == f.field) {
                            if let Some(val) =
                                axon_storage::extract_index_value(&f.value, &idx.index_type)
                            {
                                // Use this index; remaining filters applied post-fetch.
                                return storage.index_lookup(collection, &f.field, &val).ok();
                            }
                        }
                    }
                }
            }
            None
        }
        _ => None, // Or, Gate — fall back to scan
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
///
/// `gate_eval` is an optional pre-computed gate evaluation for the entity.
/// When `None`, any `Gate` filter nodes evaluate to `false`.
fn apply_filter(node: &FilterNode, data: &serde_json::Value) -> bool {
    apply_filter_with_gates(node, data, None)
}

fn apply_filter_with_gates(
    node: &FilterNode,
    data: &serde_json::Value,
    gate_eval: Option<&axon_schema::GateEvaluation>,
) -> bool {
    match node {
        FilterNode::Field(f) => apply_field_filter(f, data),
        FilterNode::Gate(g) => gate_eval
            .and_then(|eval| eval.gate_results.get(&g.gate))
            .is_some_and(|result| result.pass == g.pass),
        FilterNode::And { filters } => filters
            .iter()
            .all(|f| apply_filter_with_gates(f, data, gate_eval)),
        FilterNode::Or { filters } => filters
            .iter()
            .any(|f| apply_filter_with_gates(f, data, gate_eval)),
    }
}

/// Check if a filter tree contains any gate filter nodes.
fn has_gate_filter(node: &FilterNode) -> bool {
    match node {
        FilterNode::Gate(_) => true,
        FilterNode::Field(_) => false,
        FilterNode::And { filters } | FilterNode::Or { filters } => {
            filters.iter().any(has_gate_filter)
        }
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

/// Compute an aggregate function over a non-empty slice of f64 values.
#[allow(clippy::cast_precision_loss)]
fn compute_aggregate(func: &AggregateFunction, values: &[f64]) -> f64 {
    match func {
        AggregateFunction::Sum => values.iter().sum(),
        AggregateFunction::Avg => values.iter().sum::<f64>() / values.len() as f64,
        AggregateFunction::Min => values.iter().copied().fold(f64::INFINITY, f64::min),
        AggregateFunction::Max => values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    }
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

/// Apply an RFC 7396 JSON Merge Patch to a target value.
///
/// - Object values are recursively merged.
/// - `null` values remove the key from the target.
/// - Non-object patches replace the target entirely.
fn json_merge_patch(target: &mut serde_json::Value, patch: &serde_json::Value) {
    use serde_json::Value;
    if let Value::Object(patch_map) = patch {
        if !target.is_object() {
            *target = Value::Object(serde_json::Map::new());
        }
        if let Value::Object(target_map) = target {
            for (key, value) in patch_map {
                if value.is_null() {
                    target_map.remove(key);
                } else {
                    let entry = target_map.entry(key.clone()).or_insert(Value::Null);
                    json_merge_patch(entry, value);
                }
            }
        }
    } else {
        *target = patch.clone();
    }
}

/// Check unique index constraints for an entity's data before write.
///
/// Iterates over all unique indexes in the schema and checks whether any other
/// entity in the collection already has the same indexed value.
fn check_unique_constraints<S: StorageAdapter>(
    storage: &S,
    collection: &CollectionId,
    entity_id: &EntityId,
    data: &serde_json::Value,
    schema: &CollectionSchema,
) -> Result<(), AxonError> {
    for idx in &schema.indexes {
        if !idx.unique {
            continue;
        }
        if let Some(json_val) = resolve_field_path(data, &idx.field) {
            if let Some(val) = extract_index_value(json_val, &idx.index_type) {
                if storage.index_unique_conflict(collection, &idx.field, &val, entity_id)? {
                    return Err(AxonError::UniqueViolation {
                        field: idx.field.clone(),
                        value: json_val.to_string(),
                    });
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::manual_string_new, clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fmt::Display;

    use axon_core::id::{CollectionId, EntityId};
    use axon_schema::schema::{CollectionView, EsfDocument};
    use axon_storage::adapter::StorageAdapter;
    use axon_storage::memory::MemoryStorageAdapter;
    use serde_json::json;

    fn handler() -> AxonHandler<MemoryStorageAdapter> {
        AxonHandler::new(MemoryStorageAdapter::default())
    }

    fn register_prod_billing_and_engineering_collection(
        h: &mut AxonHandler<MemoryStorageAdapter>,
        collection: &str,
    ) -> (CollectionId, CollectionId) {
        use crate::request::{CreateDatabaseRequest, CreateNamespaceRequest};
        use axon_core::id::Namespace;

        let bare = CollectionId::new(collection);
        let billing = CollectionId::new(format!("prod.billing.{collection}"));
        let engineering = CollectionId::new(format!("prod.engineering.{collection}"));

        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        for schema in ["billing", "engineering"] {
            h.create_namespace(CreateNamespaceRequest {
                database: "prod".into(),
                schema: schema.into(),
            })
            .unwrap();
        }
        h.storage_mut()
            .register_collection_in_namespace(&bare, &Namespace::new("prod", "billing"))
            .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(&bare, &Namespace::new("prod", "engineering"))
            .unwrap();

        (billing, engineering)
    }

    fn ok_or_panic<T, E: Display>(result: Result<T, E>, context: &str) -> T {
        match result {
            Ok(value) => value,
            Err(err) => panic!("{context}: {err}"),
        }
    }

    fn err_or_panic<T, E: Display>(result: Result<T, E>, context: &str) -> E {
        match result {
            Ok(_) => panic!("{context}: expected error"),
            Err(err) => err,
        }
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
                audit_metadata: None,
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
    fn get_entity_markdown_renders_collection_view() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        ok_or_panic(
            h.create_collection(CreateCollectionRequest {
                name: col.clone(),
                schema: CollectionSchema::new(col.clone()),
                actor: None,
            }),
            "creating collection for markdown render test",
        );
        ok_or_panic(
            h.create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: id.clone(),
                data: json!({"title": "hello", "status": "open"}),
                actor: None,
                audit_metadata: None,
            }),
            "creating entity for markdown render test",
        );
        ok_or_panic(
            h.storage_mut().put_collection_view(&CollectionView::new(
                col.clone(),
                "# {{title}}\n\nStatus: {{status}}",
            )),
            "storing collection view for markdown render test",
        );

        let rendered = ok_or_panic(
            h.get_entity_markdown(&col, &id),
            "rendering markdown with collection view",
        );

        match rendered {
            GetEntityMarkdownResponse::Rendered {
                rendered_markdown, ..
            } => {
                assert_eq!(rendered_markdown, "# hello\n\nStatus: open");
            }
            GetEntityMarkdownResponse::RenderFailed { .. } => {
                panic!("expected markdown render to succeed")
            }
        }
    }

    #[test]
    fn get_entity_markdown_requires_collection_view() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        ok_or_panic(
            h.create_collection(CreateCollectionRequest {
                name: col.clone(),
                schema: CollectionSchema::new(col.clone()),
                actor: None,
            }),
            "creating collection for missing template test",
        );
        ok_or_panic(
            h.create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: id.clone(),
                data: json!({"title": "hello"}),
                actor: None,
                audit_metadata: None,
            }),
            "creating entity for missing template test",
        );

        let error = err_or_panic(
            h.get_entity_markdown(&col, &id),
            "rendering markdown without collection view",
        );

        assert!(matches!(error, AxonError::InvalidArgument(_)));
        assert!(error
            .to_string()
            .contains("has no markdown template defined"));
    }

    #[test]
    fn get_entity_markdown_preserves_entity_on_render_failure() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        ok_or_panic(
            h.create_collection(CreateCollectionRequest {
                name: col.clone(),
                schema: CollectionSchema::new(col.clone()),
                actor: None,
            }),
            "creating collection for render failure test",
        );
        ok_or_panic(
            h.create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: id.clone(),
                data: json!({"title": "hello"}),
                actor: None,
                audit_metadata: None,
            }),
            "creating entity for render failure test",
        );
        ok_or_panic(
            h.storage_mut()
                .put_collection_view(&CollectionView::new(col.clone(), "{{#title}")),
            "storing invalid collection view for render failure test",
        );

        let rendered = ok_or_panic(
            h.get_entity_markdown(&col, &id),
            "rendering markdown with invalid template",
        );

        match rendered {
            GetEntityMarkdownResponse::RenderFailed { entity, detail } => {
                assert_eq!(entity.id, id);
                assert_eq!(entity.data["title"], "hello");
                assert!(detail.contains("failed to render markdown"));
            }
            GetEntityMarkdownResponse::Rendered { .. } => {
                panic!("expected markdown render to fail")
            }
        }
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
            audit_metadata: None,
        })
        .unwrap();

        let updated = h
            .update_entity(UpdateEntityRequest {
                collection: col.clone(),
                id: id.clone(),
                data: json!({"title": "v2"}),
                expected_version: 1,
                actor: None,
                audit_metadata: None,
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
            audit_metadata: None,
        })
        .unwrap();

        let err = h
            .update_entity(UpdateEntityRequest {
                collection: col,
                id,
                data: json!({"title": "v2"}),
                expected_version: 99, // wrong version
                actor: None,
                audit_metadata: None,
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
            audit_metadata: None,
        })
        .unwrap();

        h.delete_entity(DeleteEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            actor: None,
            audit_metadata: None,
            force: false,
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
            audit_metadata: None,
        })
        .unwrap();

        h.update_entity(UpdateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v2"}),
            expected_version: 1,
            actor: Some("agent-1".into()),
            audit_metadata: None,
        })
        .unwrap();

        h.delete_entity(DeleteEntityRequest {
            collection: col,
            id,
            actor: None,
            audit_metadata: None,
            force: false,
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
        h.put_schema(schema).unwrap();

        // Missing required "title" field.
        let err = h
            .create_entity(CreateEntityRequest {
                collection: CollectionId::new("tasks"),
                id: EntityId::new("t-001"),
                data: json!({"done": false}),
                actor: None,
                audit_metadata: None,
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
        h.put_schema(schema).unwrap();

        let result = h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("tasks"),
            id: EntityId::new("t-001"),
            data: json!({"title": "My task", "done": false}),
            actor: None,
            audit_metadata: None,
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
            audit_metadata: None,
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
                direction: TraverseDirection::Forward,
                hop_filter: None,
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
                direction: TraverseDirection::Forward,
                hop_filter: None,
            })
            .unwrap();

        // Should only see "b" (not "a" again, not infinite loop)
        assert_eq!(resp.entities.len(), 1);
        assert_eq!(resp.entities[0].id.as_str(), "b");
    }

    #[test]
    fn traversal_reverse_follows_inbound_links() {
        let mut h = handler();
        // Chain: a -> b -> c. Reverse from c should reach b, then a.
        for name in ["a", "b", "c"] {
            make_entity(&mut h, "nodes", name);
        }
        for (src, tgt) in [("a", "b"), ("b", "c")] {
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
                id: EntityId::new("c"),
                link_type: Some("next".into()),
                max_depth: Some(3),
                direction: TraverseDirection::Reverse,
                hop_filter: None,
            })
            .unwrap();

        let ids: Vec<_> = resp.entities.iter().map(|e| e.id.as_str()).collect();
        assert!(ids.contains(&"b"), "reverse from c should reach b");
        assert!(ids.contains(&"a"), "reverse from c should reach a");
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn traversal_returns_paths_and_links() {
        let mut h = handler();
        // Chain: a -> b -> c
        for name in ["a", "b", "c"] {
            make_entity(&mut h, "nodes", name);
        }
        for (src, tgt) in [("a", "b"), ("b", "c")] {
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
                direction: TraverseDirection::Forward,
                hop_filter: None,
            })
            .unwrap();

        // Two entities reachable, two links traversed, two paths.
        assert_eq!(resp.entities.len(), 2);
        assert_eq!(resp.links.len(), 2);
        assert_eq!(resp.paths.len(), 2);

        // Path to b has 1 hop, path to c has 2 hops.
        let path_to_b = resp
            .paths
            .iter()
            .find(|p| p.hops.last().unwrap().entity.id.as_str() == "b")
            .expect("path to b");
        assert_eq!(path_to_b.hops.len(), 1);

        let path_to_c = resp
            .paths
            .iter()
            .find(|p| p.hops.last().unwrap().entity.id.as_str() == "c")
            .expect("path to c");
        assert_eq!(path_to_c.hops.len(), 2);

        // Each hop carries the link that was traversed.
        assert_eq!(path_to_c.hops[0].link.link_type, "next");
        assert_eq!(path_to_c.hops[0].entity.id.as_str(), "b");
        assert_eq!(path_to_c.hops[1].entity.id.as_str(), "c");
    }

    #[test]
    fn traversal_hop_filter_excludes_entities() {
        let mut h = handler();
        // Chain: a -> b -> c. b has status "inactive", c has "active".
        make_entity(&mut h, "nodes", "a");
        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("nodes"),
            id: EntityId::new("b"),
            data: json!({"status": "inactive"}),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("nodes"),
            id: EntityId::new("c"),
            data: json!({"status": "active"}),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();

        for (src, tgt) in [("a", "b"), ("b", "c")] {
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

        // Filter: only entities where status == "active".
        let resp = h
            .traverse(TraverseRequest {
                collection: CollectionId::new("nodes"),
                id: EntityId::new("a"),
                link_type: None,
                max_depth: Some(5),
                direction: TraverseDirection::Forward,
                hop_filter: Some(FilterNode::Field(FieldFilter {
                    field: "status".into(),
                    op: FilterOp::Eq,
                    value: json!("active"),
                })),
            })
            .unwrap();

        // b is excluded by hop_filter, so traversal stops at b and never reaches c.
        assert!(
            resp.entities.is_empty(),
            "no entities match the hop filter at depth 1"
        );
    }

    #[test]
    fn reachable_returns_true_when_path_exists() {
        let mut h = handler();
        for name in ["a", "b", "c"] {
            make_entity(&mut h, "nodes", name);
        }
        for (src, tgt) in [("a", "b"), ("b", "c")] {
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
            .reachable(ReachableRequest {
                source_collection: CollectionId::new("nodes"),
                source_id: EntityId::new("a"),
                target_collection: CollectionId::new("nodes"),
                target_id: EntityId::new("c"),
                link_type: Some("next".into()),
                max_depth: Some(5),
                direction: TraverseDirection::Forward,
            })
            .unwrap();

        assert!(resp.reachable);
        assert_eq!(resp.depth, Some(2));
    }

    #[test]
    fn reachable_returns_false_when_no_path() {
        let mut h = handler();
        for name in ["a", "b", "c"] {
            make_entity(&mut h, "nodes", name);
        }
        // Only a -> b, no path from a to c.
        h.create_link(CreateLinkRequest {
            source_collection: CollectionId::new("nodes"),
            source_id: EntityId::new("a"),
            target_collection: CollectionId::new("nodes"),
            target_id: EntityId::new("b"),
            link_type: "next".into(),
            metadata: json!(null),
            actor: None,
        })
        .unwrap();

        let resp = h
            .reachable(ReachableRequest {
                source_collection: CollectionId::new("nodes"),
                source_id: EntityId::new("a"),
                target_collection: CollectionId::new("nodes"),
                target_id: EntityId::new("c"),
                link_type: Some("next".into()),
                max_depth: Some(5),
                direction: TraverseDirection::Forward,
            })
            .unwrap();

        assert!(!resp.reachable);
        assert_eq!(resp.depth, None);
    }

    #[test]
    fn reachable_same_entity_returns_depth_zero() {
        let mut h = handler();
        make_entity(&mut h, "nodes", "a");

        let resp = h
            .reachable(ReachableRequest {
                source_collection: CollectionId::new("nodes"),
                source_id: EntityId::new("a"),
                target_collection: CollectionId::new("nodes"),
                target_id: EntityId::new("a"),
                link_type: None,
                max_depth: Some(5),
                direction: TraverseDirection::Forward,
            })
            .unwrap();

        assert!(resp.reachable);
        assert_eq!(resp.depth, Some(0));
    }

    #[test]
    fn reachable_reverse_finds_inbound_path() {
        let mut h = handler();
        for name in ["a", "b", "c"] {
            make_entity(&mut h, "nodes", name);
        }
        for (src, tgt) in [("a", "b"), ("b", "c")] {
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

        // Reverse from c should reach a in 2 hops.
        let resp = h
            .reachable(ReachableRequest {
                source_collection: CollectionId::new("nodes"),
                source_id: EntityId::new("c"),
                target_collection: CollectionId::new("nodes"),
                target_id: EntityId::new("a"),
                link_type: Some("next".into()),
                max_depth: Some(5),
                direction: TraverseDirection::Reverse,
            })
            .unwrap();

        assert!(resp.reachable);
        assert_eq!(resp.depth, Some(2));
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
            audit_metadata: None,
        })
        .unwrap();

        h.update_entity(UpdateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v2", "done": false}),
            expected_version: 1,
            actor: None,
            audit_metadata: None,
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
            audit_metadata: None,
        })
        .unwrap();

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-002"),
            data: json!({"title": "by bob"}),
            actor: Some("bob".into()),
            audit_metadata: None,
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
                audit_metadata: None,
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
            audit_metadata: None,
        })
        .unwrap();

        h.update_entity(UpdateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v2"}),
            expected_version: 1,
            actor: None,
            audit_metadata: None,
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
            audit_metadata: None,
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
                audit_metadata: None,
            })
            .unwrap();
        }

        let drop_resp = h
            .drop_collection(DropCollectionRequest {
                name: CollectionId::new("widgets"),
                actor: Some("admin".into()),
                confirm: true,
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
                confirm: true,
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
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
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
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
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
                audit_metadata: None,
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
        // Auto-increment: create_collection stores v1, this put_schema stores v2.
        h.put_schema(CollectionSchema {
            collection: CollectionId::new("items"),
            description: None,
            version: 99, // ignored — auto-increment assigns v2
            entity_schema: None,
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
        })
        .unwrap();

        let resp = h.list_collections(ListCollectionsRequest {}).unwrap();
        assert_eq!(resp.collections[0].schema_version, Some(2));
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
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
        })
        .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("things"),
            id: EntityId::new("t-001"),
            data: json!({}),
            actor: None,
            audit_metadata: None,
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
        // Timestamp fields populated from audit log (FEAT-001).
        assert!(
            resp.created_at_ns.is_some(),
            "created_at_ns should be populated from audit log"
        );
        assert!(
            resp.updated_at_ns.is_some(),
            "updated_at_ns should be populated from audit log"
        );
        assert!(
            resp.updated_at_ns.unwrap() >= resp.created_at_ns.unwrap(),
            "updated_at_ns should be >= created_at_ns"
        );
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
                direction: TraverseDirection::Forward,
                hop_filter: None,
            })
            .unwrap();
        assert!(trav.entities.is_empty(), "forward link must be removed");

        // Reverse-index must be gone — delete_entity on t-001 must now succeed.
        h.delete_entity(DeleteEntityRequest {
            collection: CollectionId::new("tasks"),
            id: EntityId::new("t-001"),
            actor: None,
            audit_metadata: None,
            force: false,
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
  manager:
    target_collection: users
    cardinality: one-to-one
"#;

    fn setup_linked_collections(h: &mut AxonHandler<MemoryStorageAdapter>) {
        let schema = EsfDocument::parse(USERS_ESF_WITH_LINKS)
            .unwrap()
            .into_collection_schema()
            .unwrap();
        h.put_schema(schema).unwrap();

        // Also register a tasks schema (no link_types needed for this test).
        let tasks_schema = CollectionSchema::new(CollectionId::new("tasks"));
        h.put_schema(tasks_schema).unwrap();
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
        h.put_schema(schema).unwrap();
        // Create entities that match the tasks schema (requires "title").
        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("tasks"),
            id: EntityId::new("t-001"),
            data: json!({"title": "Task 1"}),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("tasks"),
            id: EntityId::new("t-002"),
            data: json!({"title": "Task 2"}),
            actor: None,
            audit_metadata: None,
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

    // ── Cardinality enforcement (axon-7ac24886) ──────────────────────────────

    #[test]
    fn create_link_enforces_many_to_one_source_limit() {
        let mut h = handler();
        setup_linked_collections(&mut h);
        make_entity(&mut h, "users", "u-001");
        make_entity(&mut h, "users", "u-002");
        make_entity(&mut h, "users", "u-003");

        // "mentor" is many-to-one: source can have at most one outgoing mentor link.
        h.create_link(CreateLinkRequest {
            source_collection: CollectionId::new("users"),
            source_id: EntityId::new("u-001"),
            target_collection: CollectionId::new("users"),
            target_id: EntityId::new("u-002"),
            link_type: "mentor".into(),
            metadata: json!({"since": "2026-01-01"}),
            actor: None,
        })
        .unwrap();

        // Second mentor link from same source should fail.
        let err = h
            .create_link(CreateLinkRequest {
                source_collection: CollectionId::new("users"),
                source_id: EntityId::new("u-001"),
                target_collection: CollectionId::new("users"),
                target_id: EntityId::new("u-003"),
                link_type: "mentor".into(),
                metadata: json!({"since": "2026-02-01"}),
                actor: None,
            })
            .unwrap_err();

        assert!(
            matches!(err, AxonError::SchemaValidation(_)),
            "expected cardinality violation, got: {err}"
        );
    }

    #[test]
    fn create_link_enforces_one_to_one_both_directions() {
        let mut h = handler();
        setup_linked_collections(&mut h);
        make_entity(&mut h, "users", "u-001");
        make_entity(&mut h, "users", "u-002");
        make_entity(&mut h, "users", "u-003");

        // "manager" is one-to-one: at most one outgoing AND one inbound.
        h.create_link(CreateLinkRequest {
            source_collection: CollectionId::new("users"),
            source_id: EntityId::new("u-001"),
            target_collection: CollectionId::new("users"),
            target_id: EntityId::new("u-002"),
            link_type: "manager".into(),
            metadata: json!(null),
            actor: None,
        })
        .unwrap();

        // Second outgoing from u-001 should fail (source limit).
        let err = h
            .create_link(CreateLinkRequest {
                source_collection: CollectionId::new("users"),
                source_id: EntityId::new("u-001"),
                target_collection: CollectionId::new("users"),
                target_id: EntityId::new("u-003"),
                link_type: "manager".into(),
                metadata: json!(null),
                actor: None,
            })
            .unwrap_err();
        assert!(
            matches!(err, AxonError::SchemaValidation(_)),
            "expected source cardinality violation, got: {err}"
        );

        // Second inbound to u-002 from different source should fail (target limit).
        let err = h
            .create_link(CreateLinkRequest {
                source_collection: CollectionId::new("users"),
                source_id: EntityId::new("u-003"),
                target_collection: CollectionId::new("users"),
                target_id: EntityId::new("u-002"),
                link_type: "manager".into(),
                metadata: json!(null),
                actor: None,
            })
            .unwrap_err();
        assert!(
            matches!(err, AxonError::SchemaValidation(_)),
            "expected target cardinality violation, got: {err}"
        );
    }

    #[test]
    fn create_link_allows_many_to_many_without_limit() {
        let mut h = handler();
        setup_linked_collections(&mut h);
        make_entity(&mut h, "users", "u-001");
        make_entity(&mut h, "tasks", "t-001");
        make_entity(&mut h, "tasks", "t-002");
        make_entity(&mut h, "tasks", "t-003");

        // "assigned-to" is many-to-many: no limits.
        for tid in ["t-001", "t-002", "t-003"] {
            h.create_link(CreateLinkRequest {
                source_collection: CollectionId::new("users"),
                source_id: EntityId::new("u-001"),
                target_collection: CollectionId::new("tasks"),
                target_id: EntityId::new(tid),
                link_type: "assigned-to".into(),
                metadata: json!(null),
                actor: None,
            })
            .unwrap();
        }
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
            audit_metadata: None,
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
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
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
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
        };

        h.handle_put_schema(PutSchemaRequest {
            schema,
            actor: Some("alice".into()),
            force: false,
            dry_run: false,
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
                audit_metadata: None,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::SchemaValidation(_)));

        // Valid entity should be accepted.
        h.create_entity(CreateEntityRequest {
            collection: col,
            id: EntityId::new("t-good"),
            data: json!({"title": "ok", "done": false}),
            actor: None,
            audit_metadata: None,
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
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
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
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
        };

        let err = h
            .handle_put_schema(PutSchemaRequest {
                schema,
                actor: None,
                force: false,
                dry_run: false,
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
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
        };

        h.handle_put_schema(PutSchemaRequest {
            schema,
            actor: None,
            force: false,
            dry_run: false,
        })
        .unwrap();
    }

    #[test]
    fn put_schema_breaking_change_rejected_without_force() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let v1 = CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "required": ["title"],
                "properties": {
                    "title": {"type": "string"},
                    "status": {"type": "string", "enum": ["draft", "active"]}
                }
            })),
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
        };
        h.handle_put_schema(PutSchemaRequest {
            schema: v1,
            actor: None,
            force: false,
            dry_run: false,
        })
        .unwrap();

        // Breaking change: add required field
        let v2 = CollectionSchema {
            collection: col,
            description: None,
            version: 2,
            entity_schema: Some(json!({
                "type": "object",
                "required": ["title", "assignee"],
                "properties": {
                    "title": {"type": "string"},
                    "status": {"type": "string", "enum": ["draft", "active"]},
                    "assignee": {"type": "string"}
                }
            })),
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
        };
        let err = h
            .handle_put_schema(PutSchemaRequest {
                schema: v2,
                actor: None,
                force: false,
                dry_run: false,
            })
            .unwrap_err();
        assert!(
            matches!(err, AxonError::InvalidOperation(_)),
            "breaking change without force should be rejected, got: {err:?}"
        );
    }

    #[test]
    fn put_schema_breaking_change_accepted_with_force() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let v1 = CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "required": ["title"],
                "properties": {"title": {"type": "string"}}
            })),
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
        };
        h.handle_put_schema(PutSchemaRequest {
            schema: v1,
            actor: None,
            force: false,
            dry_run: false,
        })
        .unwrap();

        // Breaking: add required field, with force=true
        let v2 = CollectionSchema {
            collection: col,
            description: None,
            version: 2,
            entity_schema: Some(json!({
                "type": "object",
                "required": ["title", "priority"],
                "properties": {
                    "title": {"type": "string"},
                    "priority": {"type": "integer"}
                }
            })),
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
        };
        let resp = h
            .handle_put_schema(PutSchemaRequest {
                schema: v2,
                actor: Some("admin".into()),
                force: true,
                dry_run: false,
            })
            .unwrap();
        assert_eq!(
            resp.compatibility,
            Some(axon_schema::Compatibility::Breaking)
        );
        assert!(!resp.dry_run);
    }

    #[test]
    fn put_schema_dry_run_does_not_apply() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let v1 = CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "required": ["title"],
                "properties": {"title": {"type": "string"}}
            })),
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
        };
        h.handle_put_schema(PutSchemaRequest {
            schema: v1,
            actor: None,
            force: false,
            dry_run: false,
        })
        .unwrap();

        // Dry-run breaking change
        let v2 = CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 2,
            entity_schema: Some(json!({
                "type": "object",
                "required": ["title", "owner"],
                "properties": {
                    "title": {"type": "string"},
                    "owner": {"type": "string"}
                }
            })),
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
        };
        let resp = h
            .handle_put_schema(PutSchemaRequest {
                schema: v2,
                actor: None,
                force: false,
                dry_run: true,
            })
            .unwrap();
        assert!(resp.dry_run);
        assert_eq!(
            resp.compatibility,
            Some(axon_schema::Compatibility::Breaking)
        );

        // Schema should still be v1
        let stored = h.get_schema(&col).unwrap().unwrap();
        assert_eq!(stored.version, 1);
    }

    #[test]
    fn put_schema_compatible_change_succeeds_without_force() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let v1 = CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "required": ["title"],
                "properties": {"title": {"type": "string"}}
            })),
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
        };
        h.handle_put_schema(PutSchemaRequest {
            schema: v1,
            actor: None,
            force: false,
            dry_run: false,
        })
        .unwrap();

        // Compatible: add optional field
        let v2 = CollectionSchema {
            collection: col,
            description: None,
            version: 2,
            entity_schema: Some(json!({
                "type": "object",
                "required": ["title"],
                "properties": {
                    "title": {"type": "string"},
                    "tags": {"type": "array", "items": {"type": "string"}}
                }
            })),
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
        };
        let resp = h
            .handle_put_schema(PutSchemaRequest {
                schema: v2,
                actor: None,
                force: false,
                dry_run: false,
            })
            .unwrap();
        assert_eq!(
            resp.compatibility,
            Some(axon_schema::Compatibility::Compatible)
        );
        assert!(!resp.dry_run);
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
            confirm: true,
        })
        .unwrap();

        assert!(
            h.get_schema(&col).unwrap().is_none(),
            "schema must be removed when collection is dropped"
        );
    }

    #[test]
    fn drop_collection_removes_collection_view_and_resets_version_on_recreate() {
        let mut h = handler();
        let col = CollectionId::new("notes");

        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema::new(col.clone()),
            actor: None,
        })
        .unwrap();

        let initial_view = h
            .storage_mut()
            .put_collection_view(&CollectionView::new(col.clone(), "# {{title}}"))
            .unwrap();
        assert_eq!(initial_view.version, 1);

        h.drop_collection(DropCollectionRequest {
            name: col.clone(),
            actor: None,
            confirm: true,
        })
        .unwrap();

        assert!(
            h.storage.get_collection_view(&col).unwrap().is_none(),
            "collection view must be removed when collection is dropped"
        );

        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema::new(col.clone()),
            actor: None,
        })
        .unwrap();
        assert!(
            h.storage.get_collection_view(&col).unwrap().is_none(),
            "recreated collections must not inherit a prior collection view"
        );

        let recreated_view = h
            .storage_mut()
            .put_collection_view(&CollectionView::new(col.clone(), "# {{summary}}"))
            .unwrap();
        assert_eq!(recreated_view.version, 1);
        assert_eq!(recreated_view.markdown_template, "# {{summary}}");
    }

    // ── Validation gate integration tests (US-067) ──────────────────────

    fn handler_with_gated_schema() -> AxonHandler<MemoryStorageAdapter> {
        use axon_schema::rules::{
            ConditionOp, RequirementOp, RuleCondition, RuleRequirement, ValidationRule,
        };
        use axon_schema::schema::GateDef;
        use std::collections::HashMap;

        let mut h = handler();
        let col = CollectionId::new("items");

        // Create collection first.
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema::new(col.clone()),
            actor: None,
        })
        .unwrap();

        // Schema with save, complete, review gates and advisory.
        let schema = CollectionSchema {
            collection: col,
            description: None,
            version: 1,
            entity_schema: None,
            link_types: Default::default(),
            gates: HashMap::from([
                (
                    "complete".into(),
                    GateDef {
                        description: Some("Ready for processing".into()),
                        includes: vec![],
                    },
                ),
                (
                    "review".into(),
                    GateDef {
                        description: Some("Ready for review".into()),
                        includes: vec!["complete".into()],
                    },
                ),
            ]),
            validation_rules: vec![
                // Save gate: bead_type required.
                ValidationRule {
                    name: "need-type".into(),
                    gate: Some("save".into()),
                    advisory: false,
                    when: None,
                    require: RuleRequirement {
                        field: "bead_type".into(),
                        op: RequirementOp::NotNull(true),
                    },
                    message: "bead_type is required".into(),
                    fix: Some("Set bead_type".into()),
                },
                // Complete gate: description required.
                ValidationRule {
                    name: "need-desc".into(),
                    gate: Some("complete".into()),
                    advisory: false,
                    when: None,
                    require: RuleRequirement {
                        field: "description".into(),
                        op: RequirementOp::NotNull(true),
                    },
                    message: "Description required for completion".into(),
                    fix: Some("Add a description".into()),
                },
                // Complete gate: conditional - bugs need priority.
                ValidationRule {
                    name: "bugs-need-priority".into(),
                    gate: Some("complete".into()),
                    advisory: false,
                    when: Some(RuleCondition::Field {
                        field: "bead_type".into(),
                        op: ConditionOp::Eq(serde_json::json!("bug")),
                    }),
                    require: RuleRequirement {
                        field: "priority".into(),
                        op: RequirementOp::NotNull(true),
                    },
                    message: "Bugs must have priority".into(),
                    fix: Some("Set priority (0-4)".into()),
                },
                // Review gate: acceptance required.
                ValidationRule {
                    name: "need-acceptance".into(),
                    gate: Some("review".into()),
                    advisory: false,
                    when: None,
                    require: RuleRequirement {
                        field: "acceptance".into(),
                        op: RequirementOp::NotNull(true),
                    },
                    message: "Acceptance criteria required for review".into(),
                    fix: Some("Add acceptance criteria".into()),
                },
                // Advisory: recommend tags.
                ValidationRule {
                    name: "recommend-tags".into(),
                    gate: None,
                    advisory: true,
                    when: None,
                    require: RuleRequirement {
                        field: "tags".into(),
                        op: RequirementOp::NotNull(true),
                    },
                    message: "Consider adding tags".into(),
                    fix: Some("Add tags for categorization".into()),
                },
            ],
            indexes: Default::default(),
            compound_indexes: Default::default(),
        };
        h.put_schema(schema).unwrap();
        h
    }

    #[test]
    fn save_gate_blocks_create() {
        let mut h = handler_with_gated_schema();
        // Missing bead_type → save gate blocks.
        let result = h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("items"),
            id: EntityId::new("g-1"),
            data: json!({"title": "Test"}),
            actor: None,
            audit_metadata: None,
        });
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("save gate failed"), "got: {err}");
        assert!(err.contains("bead_type is required"), "got: {err}");
    }

    #[test]
    fn custom_gate_allows_save_reports_failures() {
        let mut h = handler_with_gated_schema();
        // Has bead_type (save passes) but missing description (complete gate fails).
        let resp = h
            .create_entity(CreateEntityRequest {
                collection: CollectionId::new("items"),
                id: EntityId::new("g-2"),
                data: json!({"bead_type": "task"}),
                actor: None,
                audit_metadata: None,
            })
            .unwrap();

        // Entity was saved.
        assert_eq!(resp.entity.data["bead_type"], "task");

        // Complete gate fails.
        let complete = resp.gates.get("complete").unwrap();
        assert!(!complete.pass);
        assert!(complete.failures.iter().any(|f| f.rule == "need-desc"));

        // Review gate also fails (inherits complete).
        let review = resp.gates.get("review").unwrap();
        assert!(!review.pass);
        assert!(review.failures.iter().any(|f| f.rule == "need-desc"));
        assert!(review.failures.iter().any(|f| f.rule == "need-acceptance"));
    }

    #[test]
    fn advisory_reported_in_response() {
        let mut h = handler_with_gated_schema();
        let resp = h
            .create_entity(CreateEntityRequest {
                collection: CollectionId::new("items"),
                id: EntityId::new("g-3"),
                data: json!({"bead_type": "task"}),
                actor: None,
                audit_metadata: None,
            })
            .unwrap();

        assert_eq!(resp.advisories.len(), 1);
        assert_eq!(resp.advisories[0].rule, "recommend-tags");
        assert!(resp.advisories[0].advisory);
    }

    #[test]
    fn all_gates_pass_when_all_fields_present() {
        let mut h = handler_with_gated_schema();
        let resp = h
            .create_entity(CreateEntityRequest {
                collection: CollectionId::new("items"),
                id: EntityId::new("g-4"),
                data: json!({
                    "bead_type": "task",
                    "description": "Something",
                    "acceptance": "Tests pass",
                    "tags": ["core"]
                }),
                actor: None,
                audit_metadata: None,
            })
            .unwrap();

        // All gates pass.
        for gate in resp.gates.values() {
            assert!(gate.pass, "gate {} should pass", gate.gate);
        }
        // No advisories.
        assert!(resp.advisories.is_empty());
    }

    #[test]
    fn save_gate_blocks_update() {
        let mut h = handler_with_gated_schema();
        // Create with valid data.
        let resp = h
            .create_entity(CreateEntityRequest {
                collection: CollectionId::new("items"),
                id: EntityId::new("g-5"),
                data: json!({"bead_type": "task"}),
                actor: None,
                audit_metadata: None,
            })
            .unwrap();

        // Update removing bead_type → save gate blocks.
        let result = h.update_entity(UpdateEntityRequest {
            collection: CollectionId::new("items"),
            id: EntityId::new("g-5"),
            data: json!({"title": "Updated"}),
            expected_version: resp.entity.version,
            actor: None,
            audit_metadata: None,
        });
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("save gate failed"));
    }

    #[test]
    fn update_reports_gate_status() {
        let mut h = handler_with_gated_schema();
        let create_resp = h
            .create_entity(CreateEntityRequest {
                collection: CollectionId::new("items"),
                id: EntityId::new("g-6"),
                data: json!({"bead_type": "bug"}),
                actor: None,
                audit_metadata: None,
            })
            .unwrap();

        // Update with description but no priority (bug needs priority for complete gate).
        let resp = h
            .update_entity(UpdateEntityRequest {
                collection: CollectionId::new("items"),
                id: EntityId::new("g-6"),
                data: json!({
                    "bead_type": "bug",
                    "description": "A bug"
                }),
                expected_version: create_resp.entity.version,
                actor: None,
                audit_metadata: None,
            })
            .unwrap();

        let complete = resp.gates.get("complete").unwrap();
        assert!(!complete.pass);
        assert!(complete
            .failures
            .iter()
            .any(|f| f.rule == "bugs-need-priority"));
    }

    #[test]
    fn gate_inclusion_review_inherits_complete_failures() {
        let mut h = handler_with_gated_schema();
        let resp = h
            .create_entity(CreateEntityRequest {
                collection: CollectionId::new("items"),
                id: EntityId::new("g-7"),
                data: json!({"bead_type": "task"}),
                actor: None,
                audit_metadata: None,
            })
            .unwrap();

        // Review gate should contain complete-gate failures too.
        let review = resp.gates.get("review").unwrap();
        let failure_rules: Vec<&str> = review.failures.iter().map(|f| f.rule.as_str()).collect();
        assert!(
            failure_rules.contains(&"need-desc"),
            "review should inherit complete's need-desc failure"
        );
        assert!(
            failure_rules.contains(&"need-acceptance"),
            "review should have its own need-acceptance failure"
        );
    }

    #[test]
    fn gate_definitions_registered_on_schema_save() {
        let h = handler_with_gated_schema();
        let schema = h.get_schema(&CollectionId::new("items")).unwrap().unwrap();
        assert!(schema.gates.contains_key("complete"));
        assert!(schema.gates.contains_key("review"));
        assert_eq!(schema.gates["review"].includes, vec!["complete"]);
    }

    // ── Aggregation tests (US-062) ──────────────────────────────────────

    fn handler_with_entities() -> AxonHandler<MemoryStorageAdapter> {
        let mut h = handler();
        let col = CollectionId::new("beads");
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema::new(col.clone()),
            actor: None,
        })
        .unwrap();

        // Create entities with various statuses and types.
        let items = vec![
            json!({"bead_type": "task", "status": "draft"}),
            json!({"bead_type": "task", "status": "draft"}),
            json!({"bead_type": "task", "status": "pending"}),
            json!({"bead_type": "bug", "status": "pending"}),
            json!({"bead_type": "bug", "status": "done"}),
            json!({"bead_type": "epic"}), // missing status
        ];
        for (i, data) in items.into_iter().enumerate() {
            h.create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: EntityId::new(format!("b-{i}")),
                data,
                actor: None,
                audit_metadata: None,
            })
            .unwrap();
        }
        h
    }

    #[test]
    fn count_without_group_by_returns_total() {
        let h = handler_with_entities();
        let resp = h
            .count_entities(CountEntitiesRequest {
                collection: CollectionId::new("beads"),
                filter: None,
                group_by: None,
            })
            .unwrap();
        assert_eq!(resp.total_count, 6);
        assert!(resp.groups.is_empty());
    }

    #[test]
    fn count_group_by_status() {
        let h = handler_with_entities();
        let resp = h
            .count_entities(CountEntitiesRequest {
                collection: CollectionId::new("beads"),
                filter: None,
                group_by: Some("status".into()),
            })
            .unwrap();
        assert_eq!(resp.total_count, 6);

        // Should have groups for draft, pending, done, and null (missing status).
        assert!(!resp.groups.is_empty());

        let draft_count = resp
            .groups
            .iter()
            .find(|g| g.key == json!("draft"))
            .map(|g| g.count)
            .unwrap_or(0);
        assert_eq!(draft_count, 2);

        let pending_count = resp
            .groups
            .iter()
            .find(|g| g.key == json!("pending"))
            .map(|g| g.count)
            .unwrap_or(0);
        assert_eq!(pending_count, 2);

        let done_count = resp
            .groups
            .iter()
            .find(|g| g.key == json!("done"))
            .map(|g| g.count)
            .unwrap_or(0);
        assert_eq!(done_count, 1);

        // Null group for the entity missing status.
        let null_count = resp
            .groups
            .iter()
            .find(|g| g.key.is_null())
            .map(|g| g.count)
            .unwrap_or(0);
        assert_eq!(null_count, 1);
    }

    #[test]
    fn count_with_filter() {
        let h = handler_with_entities();
        let resp = h
            .count_entities(CountEntitiesRequest {
                collection: CollectionId::new("beads"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "bead_type".into(),
                    op: FilterOp::Eq,
                    value: json!("task"),
                })),
                group_by: None,
            })
            .unwrap();
        assert_eq!(resp.total_count, 3);
    }

    #[test]
    fn count_with_filter_and_group_by() {
        let h = handler_with_entities();
        let resp = h
            .count_entities(CountEntitiesRequest {
                collection: CollectionId::new("beads"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "bead_type".into(),
                    op: FilterOp::Eq,
                    value: json!("task"),
                })),
                group_by: Some("status".into()),
            })
            .unwrap();
        assert_eq!(resp.total_count, 3);

        let draft = resp.groups.iter().find(|g| g.key == json!("draft"));
        assert_eq!(draft.unwrap().count, 2);

        let pending = resp.groups.iter().find(|g| g.key == json!("pending"));
        assert_eq!(pending.unwrap().count, 1);
    }

    #[test]
    fn count_empty_collection() {
        let mut h = handler();
        let col = CollectionId::new("empty");
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema::new(col.clone()),
            actor: None,
        })
        .unwrap();

        let resp = h
            .count_entities(CountEntitiesRequest {
                collection: col,
                filter: None,
                group_by: Some("status".into()),
            })
            .unwrap();
        assert_eq!(resp.total_count, 0);
        assert!(resp.groups.is_empty());
    }

    // ── Namespace management tests (US-036) ───────────────────────────────

    #[test]
    fn create_namespace() {
        use crate::request::{CreateDatabaseRequest, CreateNamespaceRequest};
        let mut h = handler();
        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        let resp = h
            .create_namespace(CreateNamespaceRequest {
                database: "prod".into(),
                schema: "billing".into(),
            })
            .unwrap();
        assert_eq!(resp.database, "prod");
        assert_eq!(resp.schema, "billing");
    }

    #[test]
    fn create_duplicate_namespace_fails() {
        use crate::request::{CreateDatabaseRequest, CreateNamespaceRequest};
        let mut h = handler();
        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
        })
        .unwrap();
        let result = h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
        });
        assert!(result.is_err());
    }

    #[test]
    fn list_namespace_collections_empty() {
        use crate::request::{
            CreateDatabaseRequest, CreateNamespaceRequest, ListNamespaceCollectionsRequest,
        };
        let mut h = handler();
        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
        })
        .unwrap();
        let resp = h
            .list_namespace_collections(ListNamespaceCollectionsRequest {
                database: "prod".into(),
                schema: "billing".into(),
            })
            .unwrap();
        assert!(resp.collections.is_empty());
    }

    #[test]
    fn drop_empty_namespace() {
        use crate::request::{CreateDatabaseRequest, CreateNamespaceRequest, DropNamespaceRequest};
        let mut h = handler();
        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
        })
        .unwrap();
        let resp = h
            .drop_namespace(DropNamespaceRequest {
                database: "prod".into(),
                schema: "billing".into(),
                force: false,
            })
            .unwrap();
        assert_eq!(resp.collections_removed, 0);
    }

    #[test]
    fn drop_nonempty_namespace_without_force_fails() {
        use crate::request::{CreateDatabaseRequest, CreateNamespaceRequest, DropNamespaceRequest};
        use axon_core::id::{CollectionId, Namespace};
        let mut h = handler();
        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
        })
        .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(
                &CollectionId::new("invoices"),
                &Namespace::new("prod", "billing"),
            )
            .unwrap();

        let result = h.drop_namespace(DropNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
            force: false,
        });
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invoices"));
    }

    #[test]
    fn drop_nonempty_namespace_with_force() {
        use crate::request::{CreateDatabaseRequest, CreateNamespaceRequest, DropNamespaceRequest};
        use axon_core::id::{CollectionId, Namespace};
        let mut h = handler();
        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
        })
        .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(
                &CollectionId::new("invoices"),
                &Namespace::new("prod", "billing"),
            )
            .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(
                &CollectionId::new("receipts"),
                &Namespace::new("prod", "billing"),
            )
            .unwrap();

        let resp = h
            .drop_namespace(DropNamespaceRequest {
                database: "prod".into(),
                schema: "billing".into(),
                force: true,
            })
            .unwrap();
        assert_eq!(resp.collections_removed, 2);
    }

    #[test]
    fn create_collection_in_default_namespace_allows_same_name_elsewhere() {
        use crate::request::{
            CreateDatabaseRequest, CreateNamespaceRequest, ListNamespaceCollectionsRequest,
        };
        use axon_core::id::{CollectionId, Namespace};

        let mut h = handler();
        let invoices = CollectionId::new("invoices");

        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
        })
        .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(&invoices, &Namespace::new("prod", "billing"))
            .unwrap();

        h.create_collection(CreateCollectionRequest {
            name: invoices.clone(),
            schema: CollectionSchema::new(invoices.clone()),
            actor: None,
        })
        .unwrap();

        assert_eq!(
            h.list_namespace_collections(ListNamespaceCollectionsRequest {
                database: "default".into(),
                schema: "default".into(),
            })
            .unwrap()
            .collections,
            vec!["invoices".to_string()]
        );
        assert_eq!(
            h.list_namespace_collections(ListNamespaceCollectionsRequest {
                database: "prod".into(),
                schema: "billing".into(),
            })
            .unwrap()
            .collections,
            vec!["invoices".to_string()]
        );
    }

    #[test]
    fn drop_namespace_force_preserves_same_name_in_other_namespace() {
        use crate::request::{
            CreateDatabaseRequest, CreateNamespaceRequest, DropNamespaceRequest,
            ListNamespaceCollectionsRequest,
        };
        use axon_core::id::{CollectionId, Namespace};

        let mut h = handler();
        let invoices = CollectionId::new("invoices");

        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        for schema in ["billing", "engineering"] {
            h.create_namespace(CreateNamespaceRequest {
                database: "prod".into(),
                schema: schema.into(),
            })
            .unwrap();
        }

        h.storage_mut()
            .register_collection_in_namespace(&invoices, &Namespace::new("prod", "billing"))
            .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(&invoices, &Namespace::new("prod", "engineering"))
            .unwrap();

        h.drop_namespace(DropNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
            force: true,
        })
        .unwrap();

        assert_eq!(
            h.list_namespace_collections(ListNamespaceCollectionsRequest {
                database: "prod".into(),
                schema: "engineering".into(),
            })
            .unwrap()
            .collections,
            vec!["invoices".to_string()]
        );
    }

    #[test]
    fn drop_namespace_with_force_records_collection_drop_audits() {
        use crate::request::{CreateDatabaseRequest, CreateNamespaceRequest, DropNamespaceRequest};
        use axon_core::id::Namespace;
        use axon_core::types::Entity;

        let mut h = handler();
        let billing = Namespace::new("prod", "billing");
        let invoices = CollectionId::new("invoices");
        let receipts = CollectionId::new("receipts");
        let keep = CollectionId::new("keep");

        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
        })
        .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(&invoices, &billing)
            .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(&receipts, &billing)
            .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(&keep, &Namespace::default_ns())
            .unwrap();
        h.storage_mut()
            .put(Entity::new(
                invoices.clone(),
                EntityId::new("inv-001"),
                json!({"title": "invoice"}),
            ))
            .unwrap();
        h.storage_mut()
            .put(Entity::new(
                receipts.clone(),
                EntityId::new("rcpt-001"),
                json!({"title": "receipt"}),
            ))
            .unwrap();
        h.storage_mut()
            .put(Entity::new(
                keep.clone(),
                EntityId::new("keep-001"),
                json!({"title": "keep"}),
            ))
            .unwrap();

        let resp = h
            .drop_namespace(DropNamespaceRequest {
                database: "prod".into(),
                schema: "billing".into(),
                force: true,
            })
            .unwrap();
        assert_eq!(resp.collections_removed, 2);
        assert!(h
            .storage
            .get(&invoices, &EntityId::new("inv-001"))
            .unwrap()
            .is_none());
        assert!(h
            .storage
            .get(&receipts, &EntityId::new("rcpt-001"))
            .unwrap()
            .is_none());
        assert!(h
            .storage
            .get(&keep, &EntityId::new("keep-001"))
            .unwrap()
            .is_some());

        let drops = h
            .audit_log()
            .query_by_operation(&axon_audit::entry::MutationType::CollectionDrop)
            .unwrap();
        assert_eq!(drops.len(), 2);
        let dropped: std::collections::BTreeSet<_> = drops
            .iter()
            .map(|entry| entry.collection.to_string())
            .collect();
        assert_eq!(
            dropped,
            std::collections::BTreeSet::from(["invoices".to_string(), "receipts".to_string()])
        );
    }

    #[test]
    fn drop_namespace_with_force_removes_links_for_deleted_collections() {
        use crate::request::{
            CreateDatabaseRequest, CreateEntityRequest, CreateLinkRequest, CreateNamespaceRequest,
            DropNamespaceRequest, ListNeighborsRequest,
        };
        use axon_core::id::Namespace;

        let mut h = handler();
        let billing_invoice = CollectionId::new("prod.billing.invoices");
        let engineering_ledger = CollectionId::new("prod.engineering.ledger");
        let keep = CollectionId::new("keep");
        let archive = CollectionId::new("archive");

        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        for schema in ["billing", "engineering"] {
            h.create_namespace(CreateNamespaceRequest {
                database: "prod".into(),
                schema: schema.into(),
            })
            .unwrap();
        }
        h.storage_mut()
            .register_collection_in_namespace(
                &CollectionId::new("invoices"),
                &Namespace::new("prod", "billing"),
            )
            .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(
                &CollectionId::new("ledger"),
                &Namespace::new("prod", "engineering"),
            )
            .unwrap();
        h.storage_mut().register_collection(&keep).unwrap();
        h.storage_mut().register_collection(&archive).unwrap();

        for (collection, id) in [
            (billing_invoice.clone(), "inv-001"),
            (engineering_ledger.clone(), "led-001"),
            (keep.clone(), "keep-001"),
            (archive.clone(), "arc-001"),
        ] {
            h.create_entity(CreateEntityRequest {
                collection,
                id: EntityId::new(id),
                data: json!({ "title": id }),
                actor: None,
                audit_metadata: None,
            })
            .unwrap();
        }

        for request in [
            CreateLinkRequest {
                source_collection: billing_invoice.clone(),
                source_id: EntityId::new("inv-001"),
                target_collection: engineering_ledger.clone(),
                target_id: EntityId::new("led-001"),
                link_type: "relates-to".into(),
                metadata: serde_json::Value::Null,
                actor: None,
            },
            CreateLinkRequest {
                source_collection: keep.clone(),
                source_id: EntityId::new("keep-001"),
                target_collection: billing_invoice.clone(),
                target_id: EntityId::new("inv-001"),
                link_type: "references".into(),
                metadata: serde_json::Value::Null,
                actor: None,
            },
            CreateLinkRequest {
                source_collection: keep.clone(),
                source_id: EntityId::new("keep-001"),
                target_collection: archive.clone(),
                target_id: EntityId::new("arc-001"),
                link_type: "references".into(),
                metadata: serde_json::Value::Null,
                actor: None,
            },
        ] {
            h.create_link(request).unwrap();
        }

        h.drop_namespace(DropNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
            force: true,
        })
        .unwrap();

        let keep_neighbors = h
            .list_neighbors(ListNeighborsRequest {
                collection: keep.clone(),
                id: EntityId::new("keep-001"),
                link_type: None,
                direction: None,
            })
            .unwrap();
        assert_eq!(keep_neighbors.total_count, 1);
        assert_eq!(
            keep_neighbors.groups[0].entities[0].id,
            EntityId::new("arc-001")
        );

        let ledger_neighbors = h
            .list_neighbors(ListNeighborsRequest {
                collection: engineering_ledger,
                id: EntityId::new("led-001"),
                link_type: None,
                direction: None,
            })
            .unwrap();
        assert_eq!(ledger_neighbors.total_count, 0);
    }

    #[test]
    fn default_namespace_exists_on_startup() {
        use crate::request::ListNamespaceCollectionsRequest;
        let h = handler();
        let resp = h
            .list_namespace_collections(ListNamespaceCollectionsRequest {
                database: "default".into(),
                schema: "default".into(),
            })
            .unwrap();
        assert_eq!(resp.database, "default");
        assert_eq!(resp.schema, "default");
    }

    // ── Revalidation tests (US-060) ───────────────────────────────────────

    #[test]
    fn revalidate_all_valid() {
        use crate::request::RevalidateRequest;

        let mut h = handler();
        let col = CollectionId::new("rv-test");
        let schema = CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "required": ["title"],
                "properties": {
                    "title": {"type": "string"}
                }
            })),
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
        };
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema,
            actor: None,
        })
        .unwrap();

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("rv-1"),
            data: json!({"title": "valid"}),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();

        let resp = h.revalidate(RevalidateRequest { collection: col }).unwrap();
        assert_eq!(resp.total_scanned, 1);
        assert_eq!(resp.valid_count, 1);
        assert!(resp.invalid.is_empty());
    }

    #[test]
    fn revalidate_finds_invalid_after_schema_tightened() {
        use crate::request::RevalidateRequest;

        let mut h = handler();
        let col = CollectionId::new("rv-test-2");

        // Loose schema first.
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema::new(col.clone()),
            actor: None,
        })
        .unwrap();

        // Create entities with no constraints.
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("rv-2"),
            data: json!({"title": "valid"}),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("rv-3"),
            data: json!({"no_title": true}),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();

        // Now tighten the schema.
        h.handle_put_schema(PutSchemaRequest {
            schema: CollectionSchema {
                collection: col.clone(),
                description: None,
                version: 2,
                entity_schema: Some(json!({
                    "type": "object",
                    "required": ["title"],
                    "properties": {
                        "title": {"type": "string"}
                    }
                })),
                link_types: Default::default(),
                gates: Default::default(),
                validation_rules: Default::default(),
                indexes: Default::default(),
                compound_indexes: Default::default(),
            },
            actor: None,
            force: true,
            dry_run: false,
        })
        .unwrap();

        let resp = h.revalidate(RevalidateRequest { collection: col }).unwrap();
        assert_eq!(resp.total_scanned, 2);
        assert_eq!(resp.valid_count, 1);
        assert_eq!(resp.invalid.len(), 1);
        assert_eq!(resp.invalid[0].id, "rv-3");
        assert!(!resp.invalid[0].errors.is_empty());
    }

    #[test]
    fn revalidate_empty_collection() {
        use crate::request::RevalidateRequest;

        let mut h = handler();
        let col = CollectionId::new("rv-empty");
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema {
                collection: col.clone(),
                description: None,
                version: 1,
                entity_schema: Some(json!({"type": "object"})),
                link_types: Default::default(),
                gates: Default::default(),
                validation_rules: Default::default(),
                indexes: Default::default(),
                compound_indexes: Default::default(),
            },
            actor: None,
        })
        .unwrap();

        let resp = h.revalidate(RevalidateRequest { collection: col }).unwrap();
        assert_eq!(resp.total_scanned, 0);
        assert_eq!(resp.valid_count, 0);
        assert!(resp.invalid.is_empty());
    }

    // ── Gate filter tests (US-074b) ───────────────────────────────────────

    #[test]
    fn query_gate_filter_pass_true() {
        use crate::request::GateFilter;

        let mut h = handler_with_gated_schema();
        let col = CollectionId::new("items");

        // Create entities: one with description (complete gate passes), one without.
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("gf-1"),
            data: json!({
                "bead_type": "task",
                "description": "complete",
                "acceptance": "yes",
                "tags": ["x"]
            }),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("gf-2"),
            data: json!({"bead_type": "task"}), // missing description
            actor: None,
            audit_metadata: None,
        })
        .unwrap();

        // Query: gate.complete = true.
        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: col.clone(),
                filter: Some(FilterNode::Gate(GateFilter {
                    gate: "complete".into(),
                    pass: true,
                })),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();
        assert_eq!(resp.total_count, 1);
        assert_eq!(resp.entities[0].id, EntityId::new("gf-1"));
    }

    #[test]
    fn query_gate_filter_pass_false() {
        use crate::request::GateFilter;

        let mut h = handler_with_gated_schema();
        let col = CollectionId::new("items");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("gf-3"),
            data: json!({
                "bead_type": "task",
                "description": "done",
                "acceptance": "yes",
                "tags": ["x"]
            }),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("gf-4"),
            data: json!({"bead_type": "task"}),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();

        // Query: gate.complete = false.
        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: col.clone(),
                filter: Some(FilterNode::Gate(GateFilter {
                    gate: "complete".into(),
                    pass: false,
                })),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();
        assert_eq!(resp.total_count, 1);
        assert_eq!(resp.entities[0].id, EntityId::new("gf-4"));
    }

    #[test]
    fn gate_filter_combines_with_field_filter() {
        use crate::request::GateFilter;

        let mut h = handler_with_gated_schema();
        let col = CollectionId::new("items");

        // Two passing entities, different types.
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("gf-5"),
            data: json!({
                "bead_type": "task",
                "description": "done",
                "acceptance": "yes",
                "tags": ["x"]
            }),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("gf-6"),
            data: json!({
                "bead_type": "bug",
                "description": "done",
                "priority": 1,
                "acceptance": "yes",
                "tags": ["y"]
            }),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("gf-7"),
            data: json!({"bead_type": "task"}), // fails complete
            actor: None,
            audit_metadata: None,
        })
        .unwrap();

        // gate.complete = true AND bead_type = "task"
        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: col.clone(),
                filter: Some(FilterNode::And {
                    filters: vec![
                        FilterNode::Gate(GateFilter {
                            gate: "complete".into(),
                            pass: true,
                        }),
                        FilterNode::Field(FieldFilter {
                            field: "bead_type".into(),
                            op: FilterOp::Eq,
                            value: json!("task"),
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
        assert_eq!(resp.entities[0].id, EntityId::new("gf-5"));
    }

    #[test]
    fn gate_filter_no_rules_returns_empty() {
        // Collection without validation rules: gate filters return no results.
        let mut h = handler();
        let col = CollectionId::new("norules");
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema::new(col.clone()),
            actor: None,
        })
        .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("nr-1"),
            data: json!({"title": "test"}),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();

        use crate::request::GateFilter;
        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: col,
                filter: Some(FilterNode::Gate(GateFilter {
                    gate: "complete".into(),
                    pass: true,
                })),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();
        assert_eq!(resp.total_count, 0, "no gate results without rules");
    }

    // ── Schema diff tests (US-061) ────────────────────────────────────────

    #[test]
    fn diff_schema_versions_shows_added_fields() {
        use crate::request::DiffSchemaRequest;

        let mut h = handler();
        let col = CollectionId::new("diff-test");

        // Create collection with v1 schema (title only).
        let v1_schema = CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string"}
                }
            })),
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
        };
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: v1_schema,
            actor: None,
        })
        .unwrap();

        // v2: title + description.
        let v2_schema = CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 2,
            entity_schema: Some(json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string"},
                    "description": {"type": "string"}
                }
            })),
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
        };
        h.handle_put_schema(PutSchemaRequest {
            schema: v2_schema,
            actor: None,
            force: false,
            dry_run: false,
        })
        .unwrap();

        // Diff v1 vs v2: v1 is version 1 from create_collection, v2 is version 2.
        let resp = h
            .diff_schema_versions(DiffSchemaRequest {
                collection: col,
                version_a: 1,
                version_b: 2,
            })
            .unwrap();

        assert_eq!(resp.version_a, 1);
        assert_eq!(resp.version_b, 2);
        assert!(
            resp.diff.changes.iter().any(|c| c.path == "description"),
            "should show description was added: {:?}",
            resp.diff.changes
        );
    }

    #[test]
    fn diff_nonexistent_version_returns_error() {
        use crate::request::DiffSchemaRequest;

        let mut h = handler();
        let col = CollectionId::new("diff-test-2");
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema::new(col.clone()),
            actor: None,
        })
        .unwrap();

        let result = h.diff_schema_versions(DiffSchemaRequest {
            collection: col,
            version_a: 1,
            version_b: 99,
        });
        assert!(result.is_err());
    }

    #[test]
    fn diff_non_adjacent_versions() {
        use crate::request::DiffSchemaRequest;

        let mut h = handler();
        let col = CollectionId::new("diff-test-3");

        // v1: title.
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema {
                collection: col.clone(),
                description: None,
                version: 1,
                entity_schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "title": {"type": "string"}
                    }
                })),
                link_types: Default::default(),
                gates: Default::default(),
                validation_rules: Default::default(),
                indexes: Default::default(),
                compound_indexes: Default::default(),
            },
            actor: None,
        })
        .unwrap();

        // v2: title + desc.
        h.handle_put_schema(PutSchemaRequest {
            schema: CollectionSchema {
                collection: col.clone(),
                description: None,
                version: 2,
                entity_schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "title": {"type": "string"},
                        "description": {"type": "string"}
                    }
                })),
                link_types: Default::default(),
                gates: Default::default(),
                validation_rules: Default::default(),
                indexes: Default::default(),
                compound_indexes: Default::default(),
            },
            actor: None,
            force: false,
            dry_run: false,
        })
        .unwrap();

        // v3: title + desc + priority.
        h.handle_put_schema(PutSchemaRequest {
            schema: CollectionSchema {
                collection: col.clone(),
                description: None,
                version: 3,
                entity_schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "title": {"type": "string"},
                        "description": {"type": "string"},
                        "priority": {"type": "integer"}
                    }
                })),
                link_types: Default::default(),
                gates: Default::default(),
                validation_rules: Default::default(),
                indexes: Default::default(),
                compound_indexes: Default::default(),
            },
            actor: None,
            force: false,
            dry_run: false,
        })
        .unwrap();

        // Diff v1 to v3 (non-adjacent).
        let resp = h
            .diff_schema_versions(DiffSchemaRequest {
                collection: col,
                version_a: 1,
                version_b: 3,
            })
            .unwrap();

        let paths: Vec<&str> = resp.diff.changes.iter().map(|c| c.path.as_str()).collect();
        assert!(
            paths.contains(&"description"),
            "should show description added"
        );
        assert!(paths.contains(&"priority"), "should show priority added");
    }

    // ── Numeric aggregation tests (US-063) ──────────────────────────────

    fn handler_with_numeric_entities() -> AxonHandler<MemoryStorageAdapter> {
        let mut h = handler();
        let col = CollectionId::new("invoices");
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema::new(col.clone()),
            actor: None,
        })
        .unwrap();

        let items = vec![
            json!({"amount": 100, "status": "draft", "priority": 1}),
            json!({"amount": 200, "status": "draft", "priority": 2}),
            json!({"amount": 300, "status": "pending", "priority": 1}),
            json!({"amount": 50, "status": "pending"}), // no priority
            json!({"status": "done", "title": "no-amount"}), // no amount
        ];
        for (i, data) in items.into_iter().enumerate() {
            h.create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: EntityId::new(format!("inv-{i}")),
                data,
                actor: None,
                audit_metadata: None,
            })
            .unwrap();
        }
        h
    }

    #[test]
    fn aggregate_sum() {
        let h = handler_with_numeric_entities();
        let resp = h
            .aggregate(AggregateRequest {
                collection: CollectionId::new("invoices"),
                function: AggregateFunction::Sum,
                field: "amount".into(),
                filter: None,
                group_by: None,
            })
            .unwrap();
        assert_eq!(resp.results.len(), 1);
        assert!((resp.results[0].value - 650.0).abs() < f64::EPSILON);
        assert_eq!(resp.results[0].count, 4); // 4 entities have amount
    }

    #[test]
    fn aggregate_avg_returns_float() {
        let h = handler_with_numeric_entities();
        let resp = h
            .aggregate(AggregateRequest {
                collection: CollectionId::new("invoices"),
                function: AggregateFunction::Avg,
                field: "amount".into(),
                filter: None,
                group_by: None,
            })
            .unwrap();
        assert_eq!(resp.results.len(), 1);
        assert!((resp.results[0].value - 162.5).abs() < f64::EPSILON); // 650/4
    }

    #[test]
    fn aggregate_min_max() {
        let h = handler_with_numeric_entities();
        let min_resp = h
            .aggregate(AggregateRequest {
                collection: CollectionId::new("invoices"),
                function: AggregateFunction::Min,
                field: "amount".into(),
                filter: None,
                group_by: None,
            })
            .unwrap();
        assert!((min_resp.results[0].value - 50.0).abs() < f64::EPSILON);

        let max_resp = h
            .aggregate(AggregateRequest {
                collection: CollectionId::new("invoices"),
                function: AggregateFunction::Max,
                field: "amount".into(),
                filter: None,
                group_by: None,
            })
            .unwrap();
        assert!((max_resp.results[0].value - 300.0).abs() < f64::EPSILON);
    }

    #[test]
    fn aggregate_group_by() {
        let h = handler_with_numeric_entities();
        let resp = h
            .aggregate(AggregateRequest {
                collection: CollectionId::new("invoices"),
                function: AggregateFunction::Avg,
                field: "priority".into(),
                filter: None,
                group_by: Some("status".into()),
            })
            .unwrap();

        // draft: avg(1,2) = 1.5
        let draft = resp.results.iter().find(|g| g.key == json!("draft"));
        assert!(draft.is_some());
        assert!((draft.unwrap().value - 1.5).abs() < f64::EPSILON);

        // pending: avg(1) = 1.0 (only one entity has priority)
        let pending = resp.results.iter().find(|g| g.key == json!("pending"));
        assert!(pending.is_some());
        assert!((pending.unwrap().value - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn aggregate_non_numeric_field_returns_error() {
        let h = handler_with_numeric_entities();
        let result = h.aggregate(AggregateRequest {
            collection: CollectionId::new("invoices"),
            function: AggregateFunction::Sum,
            field: "status".into(),
            filter: None,
            group_by: None,
        });
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("not numeric"),
            "should report type error"
        );
    }

    #[test]
    fn aggregate_null_excluded() {
        let h = handler_with_numeric_entities();
        // Priority has nulls for some entities.
        let resp = h
            .aggregate(AggregateRequest {
                collection: CollectionId::new("invoices"),
                function: AggregateFunction::Sum,
                field: "priority".into(),
                filter: None,
                group_by: None,
            })
            .unwrap();
        // Only 3 entities have priority: 1 + 2 + 1 = 4
        assert!((resp.results[0].value - 4.0).abs() < f64::EPSILON);
        assert_eq!(resp.results[0].count, 3);
    }

    // ── Secondary index tests (FEAT-013, US-031) ────────────────────────

    fn setup_indexed_collection() -> AxonHandler<MemoryStorageAdapter> {
        use axon_schema::schema::{IndexDef, IndexType};

        let mut h = AxonHandler::new(MemoryStorageAdapter::default());

        let schema = CollectionSchema {
            collection: CollectionId::new("tasks"),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "properties": {
                    "status": { "type": "string" },
                    "priority": { "type": "integer" }
                }
            })),
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: vec![
                IndexDef {
                    field: "status".into(),
                    index_type: IndexType::String,
                    unique: false,
                },
                IndexDef {
                    field: "priority".into(),
                    index_type: IndexType::Integer,
                    unique: false,
                },
            ],
            compound_indexes: Default::default(),
        };

        h.create_collection(CreateCollectionRequest {
            name: CollectionId::new("tasks"),
            schema,
            actor: Some("test".into()),
        })
        .unwrap();

        // Insert test entities.
        for (id, status, priority) in &[
            ("t-001", "pending", 1),
            ("t-002", "pending", 2),
            ("t-003", "done", 3),
            ("t-004", "done", 1),
            ("t-005", "in_progress", 2),
        ] {
            h.create_entity(CreateEntityRequest {
                collection: CollectionId::new("tasks"),
                id: EntityId::new(*id),
                data: json!({"status": status, "priority": priority}),
                actor: None,
                audit_metadata: None,
            })
            .unwrap();
        }

        h
    }

    #[test]
    fn index_equality_query_returns_matching_entities() {
        let h = setup_indexed_collection();

        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("tasks"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "status".into(),
                    op: FilterOp::Eq,
                    value: json!("pending"),
                })),
                sort: vec![],
                after_id: None,
                limit: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.entities.len(), 2);
        let ids: Vec<&str> = resp.entities.iter().map(|e| e.id.as_str()).collect();
        assert!(ids.contains(&"t-001"));
        assert!(ids.contains(&"t-002"));
    }

    #[test]
    fn index_range_query_gt() {
        let h = setup_indexed_collection();

        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("tasks"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "priority".into(),
                    op: FilterOp::Gt,
                    value: json!(1),
                })),
                sort: vec![],
                after_id: None,
                limit: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.entities.len(), 3);
        // priority > 1: t-002 (2), t-003 (3), t-005 (2)
    }

    #[test]
    fn non_indexed_field_falls_back_to_scan() {
        let h = setup_indexed_collection();

        // Filter on a field that has no index.
        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("tasks"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "nonexistent".into(),
                    op: FilterOp::Eq,
                    value: json!("value"),
                })),
                sort: vec![],
                after_id: None,
                limit: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.entities.len(), 0);
    }

    #[test]
    fn and_filter_uses_index_for_one_field() {
        let h = setup_indexed_collection();

        // AND filter: status=pending AND priority=2
        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("tasks"),
                filter: Some(FilterNode::And {
                    filters: vec![
                        FilterNode::Field(FieldFilter {
                            field: "status".into(),
                            op: FilterOp::Eq,
                            value: json!("pending"),
                        }),
                        FilterNode::Field(FieldFilter {
                            field: "priority".into(),
                            op: FilterOp::Eq,
                            value: json!(2),
                        }),
                    ],
                }),
                sort: vec![],
                after_id: None,
                limit: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.entities.len(), 1);
        assert_eq!(resp.entities[0].id.as_str(), "t-002");
    }

    #[test]
    fn index_maintenance_on_update() {
        let mut h = setup_indexed_collection();

        // Update t-001 status from pending to done.
        h.update_entity(UpdateEntityRequest {
            collection: CollectionId::new("tasks"),
            id: EntityId::new("t-001"),
            data: json!({"status": "done", "priority": 1}),
            expected_version: 1,
            actor: None,
            audit_metadata: None,
        })
        .unwrap();

        // Query for pending — should now only return t-002.
        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("tasks"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "status".into(),
                    op: FilterOp::Eq,
                    value: json!("pending"),
                })),
                sort: vec![],
                after_id: None,
                limit: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.entities.len(), 1);
        assert_eq!(resp.entities[0].id.as_str(), "t-002");
    }

    #[test]
    fn namespaced_update_preserves_qualified_collection_for_indexes_and_audit() {
        use axon_schema::schema::{IndexDef, IndexType};
        use axon_storage::adapter::IndexValue;

        let mut h = handler();
        let (billing, engineering) =
            register_prod_billing_and_engineering_collection(&mut h, "invoices");
        let entity_id = EntityId::new("inv-001");

        let indexed_schema = |collection: CollectionId| CollectionSchema {
            collection,
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "properties": {
                    "status": { "type": "string" }
                }
            })),
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: vec![IndexDef {
                field: "status".into(),
                index_type: IndexType::String,
                unique: false,
            }],
            compound_indexes: Default::default(),
        };

        h.put_schema(indexed_schema(billing.clone())).unwrap();
        h.put_schema(indexed_schema(engineering.clone())).unwrap();

        for collection in [billing.clone(), engineering.clone()] {
            h.create_entity(CreateEntityRequest {
                collection,
                id: entity_id.clone(),
                data: json!({ "status": "pending" }),
                actor: None,
                audit_metadata: None,
            })
            .unwrap();
        }

        let response = h
            .update_entity(UpdateEntityRequest {
                collection: billing.clone(),
                id: entity_id.clone(),
                data: json!({ "status": "paid" }),
                expected_version: 1,
                actor: Some("agent-1".into()),
                audit_metadata: None,
            })
            .unwrap();

        assert_eq!(response.entity.collection, billing);
        assert_eq!(
            h.storage_mut()
                .index_lookup(&billing, "status", &IndexValue::String("paid".into()))
                .unwrap(),
            vec![entity_id.clone()]
        );
        assert!(h
            .storage_mut()
            .index_lookup(&billing, "status", &IndexValue::String("pending".into()))
            .unwrap()
            .is_empty());
        assert_eq!(
            h.storage_mut()
                .index_lookup(
                    &engineering,
                    "status",
                    &IndexValue::String("pending".into())
                )
                .unwrap(),
            vec![entity_id.clone()]
        );

        let audit = h.audit_log().query_by_entity(&billing, &entity_id).unwrap();
        assert_eq!(audit.len(), 2);
        assert_eq!(audit[1].collection, billing);
        assert_eq!(
            h.get_entity(GetEntityRequest {
                collection: billing.clone(),
                id: entity_id.clone(),
            })
            .unwrap()
            .entity
            .collection,
            billing
        );
    }

    #[test]
    fn index_maintenance_on_delete() {
        let mut h = setup_indexed_collection();

        h.delete_entity(DeleteEntityRequest {
            collection: CollectionId::new("tasks"),
            id: EntityId::new("t-001"),
            actor: None,
            audit_metadata: None,
            force: false,
        })
        .unwrap();

        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("tasks"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "status".into(),
                    op: FilterOp::Eq,
                    value: json!("pending"),
                })),
                sort: vec![],
                after_id: None,
                limit: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.entities.len(), 1);
        assert_eq!(resp.entities[0].id.as_str(), "t-002");
    }

    #[test]
    fn schema_rejects_empty_index_field() {
        use axon_schema::schema::{IndexDef, IndexType};

        let mut h = AxonHandler::new(MemoryStorageAdapter::default());

        let schema = CollectionSchema {
            collection: CollectionId::new("bad"),
            description: None,
            version: 1,
            entity_schema: None,
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: vec![IndexDef {
                field: "".into(),
                index_type: IndexType::String,
                unique: false,
            }],
            compound_indexes: Default::default(),
        };

        let err = h.put_schema(schema).unwrap_err();
        assert!(
            matches!(err, AxonError::SchemaValidation(_)),
            "expected SchemaValidation, got: {err}"
        );
    }

    // ── Unique index enforcement tests (US-032) ─────────────────────────

    fn setup_unique_indexed_collection() -> AxonHandler<MemoryStorageAdapter> {
        use axon_schema::schema::{IndexDef, IndexType};

        let mut h = AxonHandler::new(MemoryStorageAdapter::default());

        let schema = CollectionSchema {
            collection: CollectionId::new("users"),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "properties": {
                    "email": { "type": "string" },
                    "name": { "type": "string" }
                }
            })),
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: vec![IndexDef {
                field: "email".into(),
                index_type: IndexType::String,
                unique: true,
            }],
            compound_indexes: Default::default(),
        };

        h.create_collection(CreateCollectionRequest {
            name: CollectionId::new("users"),
            schema,
            actor: Some("test".into()),
        })
        .unwrap();

        h
    }

    #[test]
    fn unique_index_rejects_duplicate_on_create() {
        let mut h = setup_unique_indexed_collection();

        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("users"),
            id: EntityId::new("u-001"),
            data: json!({"email": "alice@example.com", "name": "Alice"}),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();

        let err = h
            .create_entity(CreateEntityRequest {
                collection: CollectionId::new("users"),
                id: EntityId::new("u-002"),
                data: json!({"email": "alice@example.com", "name": "Bob"}),
                actor: None,
                audit_metadata: None,
            })
            .unwrap_err();

        match &err {
            AxonError::UniqueViolation { field, value } => {
                assert_eq!(field, "email");
                assert!(value.contains("alice@example.com"), "value: {value}");
            }
            other => panic!("expected UniqueViolation, got: {other}"),
        }
    }

    #[test]
    fn unique_index_allows_different_values() {
        let mut h = setup_unique_indexed_collection();

        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("users"),
            id: EntityId::new("u-001"),
            data: json!({"email": "alice@example.com"}),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();

        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("users"),
            id: EntityId::new("u-002"),
            data: json!({"email": "bob@example.com"}),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();
    }

    #[test]
    fn unique_index_allows_update_same_entity() {
        let mut h = setup_unique_indexed_collection();

        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("users"),
            id: EntityId::new("u-001"),
            data: json!({"email": "alice@example.com", "name": "Alice"}),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();

        // Update name but keep same email — should succeed.
        h.update_entity(UpdateEntityRequest {
            collection: CollectionId::new("users"),
            id: EntityId::new("u-001"),
            data: json!({"email": "alice@example.com", "name": "Alice Smith"}),
            expected_version: 1,
            actor: None,
            audit_metadata: None,
        })
        .unwrap();
    }

    #[test]
    fn unique_index_rejects_duplicate_on_update() {
        let mut h = setup_unique_indexed_collection();

        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("users"),
            id: EntityId::new("u-001"),
            data: json!({"email": "alice@example.com"}),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();

        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("users"),
            id: EntityId::new("u-002"),
            data: json!({"email": "bob@example.com"}),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();

        // Try to update u-002 to have alice's email.
        let err = h
            .update_entity(UpdateEntityRequest {
                collection: CollectionId::new("users"),
                id: EntityId::new("u-002"),
                data: json!({"email": "alice@example.com"}),
                expected_version: 1,
                actor: None,
                audit_metadata: None,
            })
            .unwrap_err();

        assert!(
            matches!(err, AxonError::UniqueViolation { .. }),
            "expected UniqueViolation, got: {err}"
        );
    }

    #[test]
    fn unique_index_freed_after_delete() {
        let mut h = setup_unique_indexed_collection();

        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("users"),
            id: EntityId::new("u-001"),
            data: json!({"email": "alice@example.com"}),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();

        h.delete_entity(DeleteEntityRequest {
            collection: CollectionId::new("users"),
            id: EntityId::new("u-001"),
            actor: None,
            audit_metadata: None,
            force: false,
        })
        .unwrap();

        // After delete, the email should be available.
        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("users"),
            id: EntityId::new("u-002"),
            data: json!({"email": "alice@example.com"}),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();
    }

    // ── List neighbors tests (US-071, FEAT-020) ─────────────────────────

    fn setup_neighbor_graph() -> AxonHandler<MemoryStorageAdapter> {
        let mut h = AxonHandler::new(MemoryStorageAdapter::default());

        // Create two collections.
        for name in &["tasks", "users"] {
            h.create_collection(CreateCollectionRequest {
                name: CollectionId::new(*name),
                schema: CollectionSchema::new(CollectionId::new(*name)),
                actor: Some("test".into()),
            })
            .unwrap();
        }

        // Create entities.
        for (col, id) in &[
            ("tasks", "t-001"),
            ("tasks", "t-002"),
            ("tasks", "t-003"),
            ("users", "u-001"),
        ] {
            h.create_entity(CreateEntityRequest {
                collection: CollectionId::new(*col),
                id: EntityId::new(*id),
                data: json!({"title": id}),
                actor: None,
                audit_metadata: None,
            })
            .unwrap();
        }

        // Create links: t-001 --depends-on--> t-002, t-001 --depends-on--> t-003
        // u-001 --assigned-to--> t-001
        h.create_link(CreateLinkRequest {
            source_collection: CollectionId::new("tasks"),
            source_id: EntityId::new("t-001"),
            target_collection: CollectionId::new("tasks"),
            target_id: EntityId::new("t-002"),
            link_type: "depends-on".into(),
            metadata: serde_json::Value::Null,
            actor: None,
        })
        .unwrap();

        h.create_link(CreateLinkRequest {
            source_collection: CollectionId::new("tasks"),
            source_id: EntityId::new("t-001"),
            target_collection: CollectionId::new("tasks"),
            target_id: EntityId::new("t-003"),
            link_type: "depends-on".into(),
            metadata: serde_json::Value::Null,
            actor: None,
        })
        .unwrap();

        h.create_link(CreateLinkRequest {
            source_collection: CollectionId::new("users"),
            source_id: EntityId::new("u-001"),
            target_collection: CollectionId::new("tasks"),
            target_id: EntityId::new("t-001"),
            link_type: "assigned-to".into(),
            metadata: serde_json::Value::Null,
            actor: None,
        })
        .unwrap();

        h
    }

    #[test]
    fn list_neighbors_returns_outbound_and_inbound() {
        let h = setup_neighbor_graph();

        let resp = h
            .list_neighbors(crate::request::ListNeighborsRequest {
                collection: CollectionId::new("tasks"),
                id: EntityId::new("t-001"),
                link_type: None,
                direction: None,
            })
            .unwrap();

        // t-001 has 2 outbound depends-on and 1 inbound assigned-to.
        assert_eq!(resp.total_count, 3);
        assert_eq!(resp.groups.len(), 2); // depends-on/outbound + assigned-to/inbound

        let outbound = resp
            .groups
            .iter()
            .find(|g| g.direction == "outbound" && g.link_type == "depends-on")
            .unwrap();
        assert_eq!(outbound.entities.len(), 2);

        let inbound = resp
            .groups
            .iter()
            .find(|g| g.direction == "inbound" && g.link_type == "assigned-to")
            .unwrap();
        assert_eq!(inbound.entities.len(), 1);
    }

    #[test]
    fn list_neighbors_filter_by_direction() {
        let h = setup_neighbor_graph();

        // Only outbound.
        let resp = h
            .list_neighbors(crate::request::ListNeighborsRequest {
                collection: CollectionId::new("tasks"),
                id: EntityId::new("t-001"),
                link_type: None,
                direction: Some(TraverseDirection::Forward),
            })
            .unwrap();

        assert_eq!(resp.total_count, 2); // only outbound depends-on
        assert!(resp.groups.iter().all(|g| g.direction == "outbound"));
    }

    #[test]
    fn list_neighbors_filter_by_link_type() {
        let h = setup_neighbor_graph();

        let resp = h
            .list_neighbors(crate::request::ListNeighborsRequest {
                collection: CollectionId::new("tasks"),
                id: EntityId::new("t-001"),
                link_type: Some("assigned-to".into()),
                direction: None,
            })
            .unwrap();

        // Only the inbound assigned-to from u-001.
        assert_eq!(resp.total_count, 1);
        assert_eq!(resp.groups.len(), 1);
        assert_eq!(resp.groups[0].link_type, "assigned-to");
    }

    #[test]
    fn list_neighbors_entity_not_found() {
        let h = setup_neighbor_graph();

        let err = h
            .list_neighbors(crate::request::ListNeighborsRequest {
                collection: CollectionId::new("tasks"),
                id: EntityId::new("ghost"),
                link_type: None,
                direction: None,
            })
            .unwrap_err();

        assert!(matches!(err, AxonError::NotFound(_)));
    }

    #[test]
    fn list_neighbors_entity_with_no_links() {
        let h = setup_neighbor_graph();

        let resp = h
            .list_neighbors(crate::request::ListNeighborsRequest {
                collection: CollectionId::new("tasks"),
                id: EntityId::new("t-003"),
                link_type: None,
                direction: None,
            })
            .unwrap();

        // t-003 has 1 inbound depends-on from t-001.
        assert_eq!(resp.total_count, 1);
        assert_eq!(resp.groups[0].direction, "inbound");
    }

    #[test]
    fn list_neighbors_includes_entity_data() {
        let h = setup_neighbor_graph();

        let resp = h
            .list_neighbors(crate::request::ListNeighborsRequest {
                collection: CollectionId::new("tasks"),
                id: EntityId::new("t-001"),
                link_type: Some("depends-on".into()),
                direction: Some(TraverseDirection::Forward),
            })
            .unwrap();

        assert_eq!(resp.total_count, 2);
        for entity in &resp.groups[0].entities {
            assert!(
                entity.data.get("title").is_some(),
                "entity data should be included"
            );
        }
    }

    // ── Find link candidates tests (US-070, FEAT-020) ───────────────────

    #[test]
    fn find_link_candidates_returns_target_entities() {
        let h = setup_neighbor_graph();

        let resp = h
            .find_link_candidates(crate::request::FindLinkCandidatesRequest {
                source_collection: CollectionId::new("tasks"),
                source_id: EntityId::new("t-001"),
                link_type: "depends-on".into(),
                filter: None,
                limit: None,
            })
            .unwrap();

        // Target collection defaults to source collection (no schema link def).
        assert_eq!(resp.target_collection, "tasks");
        // t-001 has 2 existing depends-on links.
        assert_eq!(resp.existing_link_count, 2);
        // All 3 tasks are candidates (including t-001 itself).
        assert!(resp.candidates.len() >= 3);
    }

    #[test]
    fn find_link_candidates_marks_already_linked() {
        let h = setup_neighbor_graph();

        let resp = h
            .find_link_candidates(crate::request::FindLinkCandidatesRequest {
                source_collection: CollectionId::new("tasks"),
                source_id: EntityId::new("t-001"),
                link_type: "depends-on".into(),
                filter: None,
                limit: None,
            })
            .unwrap();

        let t002 = resp
            .candidates
            .iter()
            .find(|c| c.entity.id.as_str() == "t-002")
            .unwrap();
        assert!(t002.already_linked, "t-002 is linked");

        let t001 = resp
            .candidates
            .iter()
            .find(|c| c.entity.id.as_str() == "t-001")
            .unwrap();
        assert!(!t001.already_linked, "t-001 is not linked to itself");
    }

    #[test]
    fn find_link_candidates_with_filter() {
        let h = setup_neighbor_graph();

        let resp = h
            .find_link_candidates(crate::request::FindLinkCandidatesRequest {
                source_collection: CollectionId::new("tasks"),
                source_id: EntityId::new("t-001"),
                link_type: "depends-on".into(),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "title".into(),
                    op: FilterOp::Eq,
                    value: json!("t-003"),
                })),
                limit: None,
            })
            .unwrap();

        assert_eq!(resp.candidates.len(), 1);
        assert_eq!(resp.candidates[0].entity.id.as_str(), "t-003");
    }

    #[test]
    fn find_link_candidates_with_limit() {
        let h = setup_neighbor_graph();

        let resp = h
            .find_link_candidates(crate::request::FindLinkCandidatesRequest {
                source_collection: CollectionId::new("tasks"),
                source_id: EntityId::new("t-001"),
                link_type: "depends-on".into(),
                filter: None,
                limit: Some(1),
            })
            .unwrap();

        assert_eq!(resp.candidates.len(), 1);
    }

    #[test]
    fn find_link_candidates_source_not_found() {
        let h = setup_neighbor_graph();

        let err = h
            .find_link_candidates(crate::request::FindLinkCandidatesRequest {
                source_collection: CollectionId::new("tasks"),
                source_id: EntityId::new("ghost"),
                link_type: "depends-on".into(),
                filter: None,
                limit: None,
            })
            .unwrap_err();

        assert!(matches!(err, AxonError::NotFound(_)));
    }

    // ── Database isolation tests (US-035) ──────────────────────────────

    #[test]
    fn create_database() {
        use crate::request::CreateDatabaseRequest;
        let mut h = handler();
        let resp = h
            .create_database(CreateDatabaseRequest {
                name: "mydb".into(),
            })
            .unwrap();
        assert_eq!(resp.name, "mydb");
    }

    #[test]
    fn create_duplicate_database_fails() {
        use crate::request::CreateDatabaseRequest;
        let mut h = handler();
        h.create_database(CreateDatabaseRequest {
            name: "mydb".into(),
        })
        .unwrap();
        let err = h
            .create_database(CreateDatabaseRequest {
                name: "mydb".into(),
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::AlreadyExists(_)));
    }

    #[test]
    fn list_databases_includes_default() {
        use crate::request::ListDatabasesRequest;
        let h = handler();
        let resp = h.list_databases(ListDatabasesRequest {}).unwrap();
        assert!(resp.databases.contains(&"default".to_string()));
    }

    #[test]
    fn list_databases_after_create() {
        use crate::request::{CreateDatabaseRequest, ListDatabasesRequest};
        let mut h = handler();
        h.create_database(CreateDatabaseRequest {
            name: "analytics".into(),
        })
        .unwrap();
        let resp = h.list_databases(ListDatabasesRequest {}).unwrap();
        assert!(resp.databases.contains(&"analytics".to_string()));
        assert!(resp.databases.contains(&"default".to_string()));
    }

    #[test]
    fn drop_empty_database() {
        use crate::request::{CreateDatabaseRequest, DropDatabaseRequest, ListDatabasesRequest};
        let mut h = handler();
        h.create_database(CreateDatabaseRequest {
            name: "temp".into(),
        })
        .unwrap();
        let resp = h
            .drop_database(DropDatabaseRequest {
                name: "temp".into(),
                force: false,
            })
            .unwrap();
        assert_eq!(resp.name, "temp");
        assert_eq!(resp.collections_removed, 0);

        let dbs = h.list_databases(ListDatabasesRequest {}).unwrap();
        assert!(!dbs.databases.contains(&"temp".to_string()));
    }

    #[test]
    fn drop_nonexistent_database_fails() {
        use crate::request::DropDatabaseRequest;
        let mut h = handler();
        let err = h
            .drop_database(DropDatabaseRequest {
                name: "nope".into(),
                force: false,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::NotFound(_)));
    }

    #[test]
    fn drop_nonempty_database_requires_force() {
        use crate::request::{CreateDatabaseRequest, CreateNamespaceRequest, DropDatabaseRequest};
        use axon_core::id::{CollectionId, Namespace};
        let mut h = handler();
        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        // Add a second schema namespace to the same database.
        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "analytics".into(),
        })
        .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(
                &CollectionId::new("tasks"),
                &Namespace::new("prod", "default"),
            )
            .unwrap();

        let err = h
            .drop_database(DropDatabaseRequest {
                name: "prod".into(),
                force: false,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::InvalidOperation(_)));

        // Force drop succeeds and removes all schemas.
        let resp = h
            .drop_database(DropDatabaseRequest {
                name: "prod".into(),
                force: true,
            })
            .unwrap();
        assert_eq!(resp.collections_removed, 1);
    }

    #[test]
    fn drop_database_with_force_records_collection_drop_audits() {
        use crate::request::{CreateDatabaseRequest, CreateNamespaceRequest, DropDatabaseRequest};
        use axon_core::id::Namespace;
        use axon_core::types::Entity;

        let mut h = handler();
        let analytics = Namespace::new("prod", "analytics");
        let orders = CollectionId::new("orders");
        let rollups = CollectionId::new("rollups");
        let keep = CollectionId::new("keep");

        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "analytics".into(),
        })
        .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(&orders, &Namespace::new("prod", "default"))
            .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(&rollups, &analytics)
            .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(&keep, &Namespace::default_ns())
            .unwrap();
        h.storage_mut()
            .put(Entity::new(
                orders.clone(),
                EntityId::new("ord-001"),
                json!({"title": "order"}),
            ))
            .unwrap();
        h.storage_mut()
            .put(Entity::new(
                rollups.clone(),
                EntityId::new("sum-001"),
                json!({"title": "rollup"}),
            ))
            .unwrap();
        h.storage_mut()
            .put(Entity::new(
                keep.clone(),
                EntityId::new("keep-001"),
                json!({"title": "keep"}),
            ))
            .unwrap();

        let resp = h
            .drop_database(DropDatabaseRequest {
                name: "prod".into(),
                force: true,
            })
            .unwrap();
        assert_eq!(resp.collections_removed, 2);
        assert!(h
            .storage
            .get(&orders, &EntityId::new("ord-001"))
            .unwrap()
            .is_none());
        assert!(h
            .storage
            .get(&rollups, &EntityId::new("sum-001"))
            .unwrap()
            .is_none());
        assert!(h
            .storage
            .get(&keep, &EntityId::new("keep-001"))
            .unwrap()
            .is_some());

        let drops = h
            .audit_log()
            .query_by_operation(&axon_audit::entry::MutationType::CollectionDrop)
            .unwrap();
        assert_eq!(drops.len(), 2);
        let dropped: std::collections::BTreeSet<_> = drops
            .iter()
            .map(|entry| entry.collection.to_string())
            .collect();
        assert_eq!(
            dropped,
            std::collections::BTreeSet::from(["orders".to_string(), "rollups".to_string()])
        );
    }

    #[test]
    fn drop_database_with_force_removes_links_for_deleted_collections() {
        use crate::request::{
            CreateDatabaseRequest, CreateEntityRequest, CreateLinkRequest, CreateNamespaceRequest,
            DropDatabaseRequest, ListNeighborsRequest,
        };
        use axon_core::id::Namespace;

        let mut h = handler();
        let orders = CollectionId::new("prod.default.orders");
        let rollups = CollectionId::new("prod.analytics.rollups");
        let keep = CollectionId::new("keep");
        let archive = CollectionId::new("archive");

        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "analytics".into(),
        })
        .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(
                &CollectionId::new("orders"),
                &Namespace::new("prod", "default"),
            )
            .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(
                &CollectionId::new("rollups"),
                &Namespace::new("prod", "analytics"),
            )
            .unwrap();
        h.storage_mut().register_collection(&keep).unwrap();
        h.storage_mut().register_collection(&archive).unwrap();

        for (collection, id) in [
            (orders.clone(), "ord-001"),
            (rollups.clone(), "sum-001"),
            (keep.clone(), "keep-001"),
            (archive.clone(), "arc-001"),
        ] {
            h.create_entity(CreateEntityRequest {
                collection,
                id: EntityId::new(id),
                data: json!({ "title": id }),
                actor: None,
                audit_metadata: None,
            })
            .unwrap();
        }

        for request in [
            CreateLinkRequest {
                source_collection: keep.clone(),
                source_id: EntityId::new("keep-001"),
                target_collection: orders.clone(),
                target_id: EntityId::new("ord-001"),
                link_type: "references".into(),
                metadata: serde_json::Value::Null,
                actor: None,
            },
            CreateLinkRequest {
                source_collection: rollups.clone(),
                source_id: EntityId::new("sum-001"),
                target_collection: keep.clone(),
                target_id: EntityId::new("keep-001"),
                link_type: "feeds".into(),
                metadata: serde_json::Value::Null,
                actor: None,
            },
            CreateLinkRequest {
                source_collection: keep.clone(),
                source_id: EntityId::new("keep-001"),
                target_collection: archive.clone(),
                target_id: EntityId::new("arc-001"),
                link_type: "references".into(),
                metadata: serde_json::Value::Null,
                actor: None,
            },
        ] {
            h.create_link(request).unwrap();
        }

        h.drop_database(DropDatabaseRequest {
            name: "prod".into(),
            force: true,
        })
        .unwrap();

        let keep_neighbors = h
            .list_neighbors(ListNeighborsRequest {
                collection: keep.clone(),
                id: EntityId::new("keep-001"),
                link_type: None,
                direction: None,
            })
            .unwrap();
        assert_eq!(keep_neighbors.total_count, 1);
        assert_eq!(
            keep_neighbors.groups[0].entities[0].id,
            EntityId::new("arc-001")
        );

        let archive_neighbors = h
            .list_neighbors(ListNeighborsRequest {
                collection: archive,
                id: EntityId::new("arc-001"),
                link_type: None,
                direction: None,
            })
            .unwrap();
        assert_eq!(archive_neighbors.total_count, 1);
        assert_eq!(
            archive_neighbors.groups[0].entities[0].id,
            EntityId::new("keep-001")
        );
    }

    // ── Audit metadata (US-009) ─────────────────────────────────────────────

    #[test]
    fn create_entity_passes_audit_metadata() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let mut meta = std::collections::HashMap::new();
        meta.insert("reason".into(), "batch-import".into());
        meta.insert("session_id".into(), "sess-42".into());

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-001"),
            data: json!({"title": "hello"}),
            actor: Some("agent-1".into()),
            audit_metadata: Some(meta),
        })
        .unwrap();

        let audit = h
            .query_audit(crate::request::QueryAuditRequest {
                collection: Some(col),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(audit.entries.len(), 1);
        assert_eq!(audit.entries[0].metadata["reason"], "batch-import");
        assert_eq!(audit.entries[0].metadata["session_id"], "sess-42");
    }

    #[test]
    fn update_entity_passes_audit_metadata() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-001"),
            data: json!({"title": "hello"}),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();

        let mut meta = std::collections::HashMap::new();
        meta.insert("ticket".into(), "PROJ-123".into());
        h.update_entity(UpdateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-001"),
            data: json!({"title": "updated"}),
            expected_version: 1,
            actor: None,
            audit_metadata: Some(meta),
        })
        .unwrap();

        let audit = h
            .query_audit(crate::request::QueryAuditRequest {
                collection: Some(col),
                ..Default::default()
            })
            .unwrap();
        // Second entry is the update.
        let update_entry = &audit.entries[1];
        assert_eq!(update_entry.metadata["ticket"], "PROJ-123");
    }

    #[test]
    fn delete_entity_passes_audit_metadata() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-001"),
            data: json!({"title": "hello"}),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();

        let mut meta = std::collections::HashMap::new();
        meta.insert("reason".into(), "cleanup".into());
        h.delete_entity(DeleteEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-001"),
            actor: None,
            force: false,
            audit_metadata: Some(meta),
        })
        .unwrap();

        let audit = h
            .query_audit(crate::request::QueryAuditRequest {
                collection: Some(col),
                ..Default::default()
            })
            .unwrap();
        let delete_entry = &audit.entries[1];
        assert_eq!(delete_entry.metadata["reason"], "cleanup");
    }

    #[test]
    fn audit_metadata_is_optional() {
        let mut h = handler();
        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("tasks"),
            id: EntityId::new("t-001"),
            data: json!({"title": "hello"}),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();

        let audit = h
            .query_audit(crate::request::QueryAuditRequest {
                collection: Some(CollectionId::new("tasks")),
                ..Default::default()
            })
            .unwrap();
        assert!(audit.entries[0].metadata.is_empty());
    }

    // ── Drop collection confirmation (US-003) ───────────────────────────────

    #[test]
    fn drop_collection_requires_confirm() {
        use axon_schema::schema::CollectionSchema;
        let mut h = handler();
        let col = CollectionId::new("tasks");
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema {
                collection: col.clone(),
                description: None,
                version: 1,
                entity_schema: None,
                link_types: Default::default(),
                gates: Default::default(),
                validation_rules: Default::default(),
                indexes: Default::default(),
                compound_indexes: Default::default(),
            },
            actor: None,
        })
        .unwrap();

        // Without confirm=true, drop is rejected.
        let err = h
            .drop_collection(DropCollectionRequest {
                name: col.clone(),
                actor: None,
                confirm: false,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::InvalidArgument(_)));

        // With confirm=true, drop succeeds.
        h.drop_collection(DropCollectionRequest {
            name: col.clone(),
            actor: None,
            confirm: true,
        })
        .unwrap();

        // Collection is gone.
        assert!(h.storage.list_collections().unwrap().is_empty());
    }

    #[test]
    fn drop_collection_audit_includes_entity_count() {
        use axon_schema::schema::CollectionSchema;
        let mut h = handler();
        let col = CollectionId::new("widgets");
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema {
                collection: col.clone(),
                description: None,
                version: 1,
                entity_schema: None,
                link_types: Default::default(),
                gates: Default::default(),
                validation_rules: Default::default(),
                indexes: Default::default(),
                compound_indexes: Default::default(),
            },
            actor: None,
        })
        .unwrap();
        // Add 3 entities.
        for i in 0..3 {
            h.create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: EntityId::new(format!("w-{i}")),
                data: json!({"name": format!("widget-{i}")}),
                actor: None,
                audit_metadata: None,
            })
            .unwrap();
        }

        h.drop_collection(DropCollectionRequest {
            name: col.clone(),
            actor: None,
            confirm: true,
        })
        .unwrap();

        let audit = h
            .query_audit(crate::request::QueryAuditRequest {
                collection: Some(col),
                ..Default::default()
            })
            .unwrap();
        // Last entry is the drop.
        let drop_entry = audit.entries.last().unwrap();
        assert_eq!(drop_entry.metadata["entities_removed"], "3");
    }

    // ── Patch entity / merge-patch (US-012) ─────────────────────────────────

    #[test]
    fn patch_entity_merges_fields() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-001"),
            data: json!({"title": "hello", "status": "draft", "priority": 3}),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();

        let resp = h
            .patch_entity(PatchEntityRequest {
                collection: col.clone(),
                id: EntityId::new("t-001"),
                patch: json!({"status": "active"}),
                expected_version: 1,
                actor: None,
                audit_metadata: None,
            })
            .unwrap();

        // Status changed, title and priority preserved.
        assert_eq!(resp.entity.data["status"], "active");
        assert_eq!(resp.entity.data["title"], "hello");
        assert_eq!(resp.entity.data["priority"], 3);
        assert_eq!(resp.entity.version, 2);
    }

    #[test]
    fn patch_entity_removes_null_fields() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-001"),
            data: json!({"title": "hello", "notes": "some notes"}),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();

        let resp = h
            .patch_entity(PatchEntityRequest {
                collection: col.clone(),
                id: EntityId::new("t-001"),
                patch: json!({"notes": null}),
                expected_version: 1,
                actor: None,
                audit_metadata: None,
            })
            .unwrap();

        assert_eq!(resp.entity.data["title"], "hello");
        assert!(resp.entity.data.get("notes").is_none());
    }

    #[test]
    fn patch_entity_adds_new_fields() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-001"),
            data: json!({"title": "hello"}),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();

        let resp = h
            .patch_entity(PatchEntityRequest {
                collection: col.clone(),
                id: EntityId::new("t-001"),
                patch: json!({"assignee": "agent-1"}),
                expected_version: 1,
                actor: None,
                audit_metadata: None,
            })
            .unwrap();

        assert_eq!(resp.entity.data["title"], "hello");
        assert_eq!(resp.entity.data["assignee"], "agent-1");
    }

    #[test]
    fn patch_entity_occ_conflict() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-001"),
            data: json!({"title": "hello"}),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();

        let err = h
            .patch_entity(PatchEntityRequest {
                collection: col.clone(),
                id: EntityId::new("t-001"),
                patch: json!({"title": "changed"}),
                expected_version: 99,
                actor: None,
                audit_metadata: None,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::ConflictingVersion { .. }));
    }

    #[test]
    fn patch_entity_not_found() {
        let mut h = handler();
        let err = h
            .patch_entity(PatchEntityRequest {
                collection: CollectionId::new("tasks"),
                id: EntityId::new("ghost"),
                patch: json!({"title": "changed"}),
                expected_version: 1,
                actor: None,
                audit_metadata: None,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::NotFound(_)));
    }

    #[test]
    fn patch_entity_creates_audit_entry() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-001"),
            data: json!({"title": "hello", "status": "draft"}),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();

        h.patch_entity(PatchEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-001"),
            patch: json!({"status": "active"}),
            expected_version: 1,
            actor: Some("agent-1".into()),
            audit_metadata: None,
        })
        .unwrap();

        let audit = h
            .query_audit(crate::request::QueryAuditRequest {
                collection: Some(col),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(audit.entries.len(), 2);
        let patch_entry = &audit.entries[1];
        assert_eq!(patch_entry.actor, "agent-1");
        // Before had status=draft, after has status=active.
        assert_eq!(patch_entry.data_before.as_ref().unwrap()["status"], "draft");
        assert_eq!(patch_entry.data_after.as_ref().unwrap()["status"], "active");
    }

    #[test]
    fn namespaced_patch_preserves_qualified_collection_for_gate_results_and_audit() {
        use axon_schema::rules::{RequirementOp, RuleRequirement, ValidationRule};
        use axon_schema::schema::GateDef;

        let mut h = handler();
        let (billing, engineering) =
            register_prod_billing_and_engineering_collection(&mut h, "invoices");
        let entity_id = EntityId::new("inv-001");

        let gated_schema = |collection: CollectionId| CollectionSchema {
            collection,
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "properties": {
                    "bead_type": { "type": "string" },
                    "description": { "type": "string" }
                }
            })),
            link_types: Default::default(),
            gates: std::collections::HashMap::from([(
                "complete".into(),
                GateDef {
                    description: Some("Ready for completion".into()),
                    includes: vec![],
                },
            )]),
            validation_rules: vec![
                ValidationRule {
                    name: "need-type".into(),
                    gate: Some("save".into()),
                    advisory: false,
                    when: None,
                    require: RuleRequirement {
                        field: "bead_type".into(),
                        op: RequirementOp::NotNull(true),
                    },
                    message: "bead_type is required".into(),
                    fix: None,
                },
                ValidationRule {
                    name: "need-desc".into(),
                    gate: Some("complete".into()),
                    advisory: false,
                    when: None,
                    require: RuleRequirement {
                        field: "description".into(),
                        op: RequirementOp::NotNull(true),
                    },
                    message: "description is required".into(),
                    fix: None,
                },
            ],
            indexes: Default::default(),
            compound_indexes: Default::default(),
        };

        h.put_schema(gated_schema(billing.clone())).unwrap();
        h.create_entity(CreateEntityRequest {
            collection: billing.clone(),
            id: entity_id.clone(),
            data: json!({ "bead_type": "invoice" }),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: engineering.clone(),
            id: entity_id.clone(),
            data: json!({ "bead_type": "invoice" }),
            actor: None,
            audit_metadata: None,
        })
        .unwrap();

        let response = h
            .patch_entity(PatchEntityRequest {
                collection: billing.clone(),
                id: entity_id.clone(),
                patch: json!({ "description": "ready" }),
                expected_version: 1,
                actor: Some("agent-1".into()),
                audit_metadata: None,
            })
            .unwrap();

        assert_eq!(response.entity.collection, billing);
        assert!(response.gates["complete"].pass);
        assert_eq!(
            h.storage_mut()
                .get_gate_results(&billing, &entity_id)
                .unwrap()
                .unwrap()
                .get("complete"),
            Some(&true)
        );
        assert_eq!(
            h.audit_log().query_by_entity(&billing, &entity_id).unwrap()[1].collection,
            billing
        );
        assert_eq!(
            h.get_entity(GetEntityRequest {
                collection: billing.clone(),
                id: entity_id.clone(),
            })
            .unwrap()
            .entity
            .collection,
            billing
        );
        assert!(h
            .storage_mut()
            .get_gate_results(&engineering, &entity_id)
            .unwrap()
            .is_none());
    }

    #[test]
    fn json_merge_patch_rfc7396_nested() {
        use serde_json::json;
        let mut target = json!({"a": {"b": 1, "c": 2}, "d": 3});
        let patch = json!({"a": {"b": null, "e": 4}});
        json_merge_patch(&mut target, &patch);
        assert_eq!(target, json!({"a": {"c": 2, "e": 4}, "d": 3}));
    }
}
