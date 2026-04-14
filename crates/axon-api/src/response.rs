use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use axon_audit::entry::{AuditEntry, FieldDiff};
use axon_core::types::{Entity, Link};
use axon_schema::gates::GateResult;
use axon_schema::rules::RuleViolation;
use axon_schema::schema::CollectionSchema;

/// Response containing a retrieved entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetEntityResponse {
    pub entity: Entity,
}

/// Outcome of requesting an entity rendered through a collection markdown template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GetEntityMarkdownResponse {
    /// Template rendering succeeded.
    Rendered {
        entity: Entity,
        rendered_markdown: String,
    },
    /// Template rendering failed after the entity was fetched.
    RenderFailed { entity: Entity, detail: String },
}

/// Response after successfully creating an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateEntityResponse {
    pub entity: Entity,
    /// Gate pass/fail status for all non-save gates. Empty when no validation rules.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub gates: HashMap<String, GateResult>,
    /// Advisory violations (never block, always reported).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub advisories: Vec<RuleViolation>,
    /// Audit entry ID produced by this write. Used as the resume cursor for
    /// live change subscriptions (FEAT-026). `None` for callers that do not
    /// populate it (pre-FEAT-026 paths).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_id: Option<u64>,
}

/// Response after successfully updating an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateEntityResponse {
    /// The entity at its new version.
    pub entity: Entity,
    /// Gate pass/fail status for all non-save gates. Empty when no validation rules.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub gates: HashMap<String, GateResult>,
    /// Advisory violations (never block, always reported).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub advisories: Vec<RuleViolation>,
    /// Audit entry ID produced by this write. Used as the resume cursor for
    /// live change subscriptions (FEAT-026).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_id: Option<u64>,
}

/// Response after successfully patching an entity (RFC 7396 merge patch).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchEntityResponse {
    /// The entity at its new version.
    pub entity: Entity,
    /// Gate pass/fail status for all non-save gates. Empty when no validation rules.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub gates: HashMap<String, GateResult>,
    /// Advisory violations (never block, always reported).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub advisories: Vec<RuleViolation>,
    /// Audit entry ID produced by this write. Used as the resume cursor for
    /// live change subscriptions (FEAT-026).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_id: Option<u64>,
}

/// Response after successfully deleting an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteEntityResponse {
    pub collection: String,
    pub id: String,
    /// Audit entry ID produced by this delete. `None` when the delete was a
    /// no-op (entity did not exist), otherwise populated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_id: Option<u64>,
}

/// Response after a successful lifecycle transition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionLifecycleResponse {
    /// The entity at its new version after the transition.
    pub entity: Entity,
    /// Audit entry ID produced by the underlying update.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_id: Option<u64>,
}

/// Response after successfully creating a link.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateLinkResponse {
    pub link: Link,
}

/// Response after successfully deleting a link.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteLinkResponse {
    pub source_collection: String,
    pub source_id: String,
    pub target_collection: String,
    pub target_id: String,
    pub link_type: String,
}

/// A single hop in a traversal path, recording the link that was followed
/// and the entity that was reached.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraverseHop {
    /// The link that was traversed to reach this entity.
    pub link: Link,
    /// The entity reached at this hop.
    pub entity: Entity,
}

/// A full path from the starting entity to a discovered entity.
///
/// Each path contains one or more hops; the last hop's entity is the
/// terminal node of that path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraversePath {
    pub hops: Vec<TraverseHop>,
}

/// Response from a link-traversal query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraverseResponse {
    /// Entities reached from the starting entity, in BFS order.
    /// Does not include the starting entity itself.
    pub entities: Vec<Entity>,
    /// All traversal paths discovered. Each path traces one route from
    /// the starting entity to a reachable entity. An entity may appear
    /// at the end of multiple paths if it is reachable via different routes.
    pub paths: Vec<TraversePath>,
    /// All links that were traversed, in BFS order.
    pub links: Vec<Link>,
}

/// Response from a reachability check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReachableResponse {
    /// `true` if the target entity is reachable from the source.
    pub reachable: bool,
    /// The number of hops in the shortest path, if reachable.
    pub depth: Option<usize>,
}

/// A candidate target entity for link creation (US-070).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkCandidate {
    /// The candidate entity.
    pub entity: Entity,
    /// Whether this entity is already linked from the source.
    pub already_linked: bool,
}

/// Response from a find-link-candidates query (US-070, FEAT-020).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindLinkCandidatesResponse {
    /// Target collection for the link type.
    pub target_collection: String,
    /// The link type.
    pub link_type: String,
    /// The cardinality of the link type (from schema), or "unknown".
    pub cardinality: String,
    /// Number of existing links of this type from the source entity.
    pub existing_link_count: usize,
    /// Candidate target entities.
    pub candidates: Vec<LinkCandidate>,
}

/// A group of neighbors for a specific link type and direction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeighborGroup {
    /// The link type (e.g. `"depends-on"`).
    pub link_type: String,
    /// Direction relative to the queried entity.
    pub direction: String,
    /// Linked entities with their data.
    pub entities: Vec<Entity>,
}

/// Response from a list-neighbors query (US-071).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListNeighborsResponse {
    /// Neighbor groups, each for a unique (link_type, direction) combination.
    pub groups: Vec<NeighborGroup>,
    /// Total number of neighbor entities across all groups.
    pub total_count: usize,
}

// ── Audit responses ──────────────────────────────────────────────────────────

/// Response from an audit log query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryAuditResponse {
    pub entries: Vec<AuditEntry>,
    /// Cursor for the next page. `None` when no further results exist.
    pub next_cursor: Option<u64>,
}

/// Response after reverting an entity to a previous state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevertEntityResponse {
    /// The entity at its restored state.
    pub entity: Entity,
    /// The new audit entry produced by the revert operation.
    pub audit_entry: AuditEntry,
}

/// Response from entity-level rollback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RollbackEntityResponse {
    /// The rollback was applied and audited as a new revision.
    Applied {
        entity: Entity,
        audit_entry: AuditEntry,
    },
    /// The rollback was validated and previewed without writing.
    DryRun {
        /// Current stored entity state. `None` when the rollback would recreate
        /// a deleted entity.
        current: Option<Entity>,
        target: Entity,
        diff: HashMap<String, FieldDiff>,
    },
}

/// Per-entity outcome in a collection rollback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackCollectionEntityResult {
    /// The entity ID that was (or would be) rolled back.
    pub id: String,
    /// Whether the rollback for this entity succeeded.
    pub success: bool,
    /// Error detail when `success` is `false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Response from a collection-level point-in-time rollback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackCollectionResponse {
    /// Number of distinct entities affected by mutations after the timestamp.
    pub entities_affected: usize,
    /// Number of entities successfully rolled back (or that would be in dry-run mode).
    pub entities_rolled_back: usize,
    /// Number of entities that failed to roll back.
    pub errors: usize,
    /// When true, no data was modified.
    pub dry_run: bool,
    /// Per-entity details.
    pub details: Vec<RollbackCollectionEntityResult>,
}

/// Per-entity outcome in a transaction rollback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackTransactionEntityResult {
    /// The collection of the affected entity.
    pub collection: String,
    /// The entity ID that was (or would be) rolled back.
    pub id: String,
    /// Whether the rollback for this entity succeeded.
    pub success: bool,
    /// Error detail when `success` is `false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Response from a transaction-level rollback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackTransactionResponse {
    /// The transaction ID that was rolled back.
    pub transaction_id: String,
    /// Number of distinct entities affected by the transaction.
    pub entities_affected: usize,
    /// Number of entities successfully rolled back (or that would be in dry-run mode).
    pub entities_rolled_back: usize,
    /// Number of entities that failed to roll back.
    pub errors: usize,
    /// When true, no data was modified.
    pub dry_run: bool,
    /// Per-entity details.
    pub details: Vec<RollbackTransactionEntityResult>,
}

// ── Entity query response ─────────────────────────────────────────────────────

/// Response from a filtered entity query (US-011 / FEAT-004).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryEntitiesResponse {
    /// Matching entities. Empty when `count_only` was requested.
    pub entities: Vec<Entity>,
    /// Total number of entities that matched the filter (before pagination).
    /// Always populated regardless of `count_only`.
    pub total_count: usize,
    /// Cursor for the next page. `None` when the result set is exhausted.
    pub next_cursor: Option<String>,
}

// ── Collection lifecycle responses ───────────────────────────────────────────

/// Response after creating a collection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCollectionResponse {
    pub name: String,
}

/// Response after dropping a collection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropCollectionResponse {
    pub name: String,
    /// Number of entities that were removed.
    pub entities_removed: usize,
}

/// Summary metadata for a single collection returned by `list_collections`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionMetadata {
    pub name: String,
    /// Number of entities currently stored in the collection.
    pub entity_count: usize,
    /// Schema version, if a schema has been registered.
    pub schema_version: Option<u32>,
    /// Nanoseconds since Unix epoch when the collection was created (from audit log).
    /// `None` if the audit log has no creation entry (e.g. pre-populated storage).
    pub created_at_ns: Option<u64>,
    /// Nanoseconds since Unix epoch of the most recent mutation in this collection.
    /// `None` if the audit log has no entries for this collection.
    pub updated_at_ns: Option<u64>,
}

/// Response from listing all explicitly created collections.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListCollectionsResponse {
    pub collections: Vec<CollectionMetadata>,
}

/// Response from describing a single collection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DescribeCollectionResponse {
    pub name: String,
    pub entity_count: usize,
    /// Full schema, if one has been registered.
    pub schema: Option<CollectionSchema>,
    /// Nanoseconds since Unix epoch when the collection was created.
    pub created_at_ns: Option<u64>,
    /// Nanoseconds since Unix epoch of the most recent mutation.
    pub updated_at_ns: Option<u64>,
}

/// Response after storing or updating a collection markdown template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutCollectionTemplateResponse {
    pub view: axon_schema::schema::CollectionView,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Response after retrieving a collection markdown template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetCollectionTemplateResponse {
    pub view: axon_schema::schema::CollectionView,
}

/// Response after deleting a collection markdown template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteCollectionTemplateResponse {
    pub collection: String,
}

// ── Schema responses ─────────────────────────────────────────────────────────

/// Response after storing or updating a collection schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutSchemaResponse {
    pub schema: CollectionSchema,
    /// Compatibility classification of this schema change.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<axon_schema::Compatibility>,
    /// Field-level diff from the previous schema version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<axon_schema::SchemaDiff>,
    /// True if this was a dry-run (schema was not applied).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dry_run: bool,
}

/// Response after retrieving a collection schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetSchemaResponse {
    pub schema: CollectionSchema,
}

/// A single invalid entity found during revalidation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvalidEntity {
    /// Entity ID.
    pub id: String,
    /// Entity version.
    pub version: u64,
    /// Validation errors.
    pub errors: Vec<String>,
}

// ── Namespace management responses (US-036) ─────────────────────────────────

/// Response after creating a namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateNamespaceResponse {
    pub database: String,
    pub schema: String,
}

/// Response listing schema namespaces within a database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListNamespacesResponse {
    pub database: String,
    pub schemas: Vec<String>,
}

/// Response listing collections in a namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListNamespaceCollectionsResponse {
    pub database: String,
    pub schema: String,
    pub collections: Vec<String>,
}

/// Response after dropping a namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropNamespaceResponse {
    pub database: String,
    pub schema: String,
    /// Number of collections removed (when force=true).
    pub collections_removed: usize,
}

/// Response from revalidating entities against the current schema (US-060).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevalidateResponse {
    /// Total entities scanned.
    pub total_scanned: usize,
    /// Number of valid entities.
    pub valid_count: usize,
    /// Invalid entities with their errors.
    pub invalid: Vec<InvalidEntity>,
}

/// Response from diffing two schema versions (US-061).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffSchemaResponse {
    /// The version pair that was compared.
    pub version_a: u32,
    pub version_b: u32,
    /// The field-level diff.
    pub diff: axon_schema::SchemaDiff,
}

// ── Aggregation responses (US-062) ──────────────────────────────────────────

/// A single group in a COUNT GROUP BY result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountGroup {
    /// The group key value. `null` for entities where the field is absent or null.
    pub key: serde_json::Value,
    /// Number of entities in this group.
    pub count: usize,
}

/// Response from counting entities with optional GROUP BY.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountEntitiesResponse {
    /// Total number of entities matching the filter (across all groups).
    pub total_count: usize,
    /// Groups when `group_by` was specified. Empty otherwise.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<CountGroup>,
}

/// A single group in an aggregation result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateGroup {
    /// The group key value. `null` when no GROUP BY was specified.
    pub key: serde_json::Value,
    /// The aggregated value (always f64 for AVG, may be integer-valued for SUM/MIN/MAX).
    pub value: f64,
    /// Number of non-null values that contributed to this aggregation.
    pub count: usize,
}

/// Response from a numeric aggregation query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateResponse {
    /// Aggregated result. A single entry when no GROUP BY; multiple for GROUP BY.
    pub results: Vec<AggregateGroup>,
}

// ── Database isolation responses (US-035, FEAT-014) ───────────────────────

/// Response from creating a database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateDatabaseResponse {
    pub name: String,
}

/// Response from dropping a database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropDatabaseResponse {
    pub name: String,
    pub collections_removed: usize,
}

/// Response listing all databases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListDatabasesResponse {
    pub databases: Vec<String>,
}

// ── Snapshot response (US-080, FEAT-004) ────────────────────────────────────

/// Response from a consistent point-in-time snapshot (US-080 / FEAT-004).
///
/// `audit_cursor` is the audit log high-water mark captured atomically with
/// the entity scan. Tail the audit log from `audit_cursor` to discover
/// mutations that occurred after this snapshot was taken.
///
/// # V1 caveat
///
/// All pages of a paginated snapshot share the **same** `audit_cursor` (the
/// value captured on the first page). Multi-page consistency under concurrent
/// writes is not guaranteed by this implementation — see
/// [`crate::request::SnapshotRequest`] for details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotResponse {
    /// Entities in this page of the snapshot.
    pub entities: Vec<Entity>,
    /// Audit log high-water mark at snapshot time. Use this as the `after_id`
    /// cursor when tailing the audit log for changes after the snapshot.
    pub audit_cursor: u64,
    /// Opaque cursor to fetch the next page. `None` when the result set is
    /// exhausted.
    pub next_page_token: Option<String>,
}
