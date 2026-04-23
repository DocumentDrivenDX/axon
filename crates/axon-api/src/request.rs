use std::collections::HashMap;

use axon_audit::entry::AuditAttribution;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use axon_core::id::{CollectionId, EntityId};
use axon_schema::schema::CollectionSchema;

/// Request to create a new entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateEntityRequest {
    pub collection: CollectionId,
    pub id: EntityId,
    pub data: Value,
    /// Optional actor identity for the audit log.
    pub actor: Option<String>,
    /// Optional key-value metadata attached to the audit entry (US-009).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_metadata: Option<HashMap<String, String>>,
    /// JWT-derived attribution stamped onto the audit entry (gateway-only; not part of the wire format).
    #[serde(skip)]
    pub attribution: Option<AuditAttribution>,
}

/// Request to read an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetEntityRequest {
    pub collection: CollectionId,
    pub id: EntityId,
}

/// Request to update an existing entity using optimistic concurrency control.
///
/// The write succeeds only if the stored version equals `expected_version`.
/// On conflict, [`axon_core::error::AxonError::ConflictingVersion`] is returned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateEntityRequest {
    pub collection: CollectionId,
    pub id: EntityId,
    /// Replacement data for the entity.
    pub data: Value,
    /// The version the caller believes is current. Must match the stored version.
    pub expected_version: u64,
    pub actor: Option<String>,
    /// Optional key-value metadata attached to the audit entry (US-009).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_metadata: Option<HashMap<String, String>>,
    /// JWT-derived attribution stamped onto the audit entry (gateway-only; not part of the wire format).
    #[serde(skip)]
    pub attribution: Option<AuditAttribution>,
}

/// Request to partially update an entity using RFC 7396 JSON Merge Patch.
///
/// Only the fields present in `patch` are modified; absent fields are preserved.
/// A field set to `null` is removed. The merged result is validated against the
/// schema before writing.
///
/// The write succeeds only if the stored version equals `expected_version`.
/// On conflict, [`axon_core::error::AxonError::ConflictingVersion`] is returned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchEntityRequest {
    pub collection: CollectionId,
    pub id: EntityId,
    /// Partial data to merge into the existing entity (RFC 7396 JSON Merge Patch).
    pub patch: Value,
    /// The version the caller believes is current. Must match the stored version.
    pub expected_version: u64,
    pub actor: Option<String>,
    /// Optional key-value metadata attached to the audit entry (US-009).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_metadata: Option<HashMap<String, String>>,
    /// JWT-derived attribution stamped onto the audit entry (gateway-only; not part of the wire format).
    #[serde(skip)]
    pub attribution: Option<AuditAttribution>,
}

/// Request to delete an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteEntityRequest {
    pub collection: CollectionId,
    pub id: EntityId,
    pub actor: Option<String>,
    /// When `true`, bypass the referential integrity check and delete the
    /// entity even if it has inbound links. Default: `false`.
    #[serde(default)]
    pub force: bool,
    /// Optional key-value metadata attached to the audit entry (US-009).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_metadata: Option<HashMap<String, String>>,
    /// JWT-derived attribution stamped onto the audit entry (gateway-only; not part of the wire format).
    #[serde(skip)]
    pub attribution: Option<AuditAttribution>,
}

/// Request to create a typed link between two entities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateLinkRequest {
    pub source_collection: CollectionId,
    pub source_id: EntityId,
    pub target_collection: CollectionId,
    pub target_id: EntityId,
    /// Semantic label for the edge (e.g., `"belongs-to"`, `"depends-on"`).
    pub link_type: String,
    /// Optional metadata stored on the link.
    #[serde(default)]
    pub metadata: Value,
    pub actor: Option<String>,
    /// JWT-derived attribution stamped onto the audit entry (gateway-only; not part of the wire format).
    #[serde(skip)]
    pub attribution: Option<AuditAttribution>,
}

/// Request to delete a typed link between two entities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteLinkRequest {
    pub source_collection: CollectionId,
    pub source_id: EntityId,
    pub target_collection: CollectionId,
    pub target_id: EntityId,
    /// Semantic label for the edge (e.g., `"belongs-to"`, `"depends-on"`).
    pub link_type: String,
    pub actor: Option<String>,
    /// JWT-derived attribution stamped onto the audit entry (gateway-only; not part of the wire format).
    #[serde(skip)]
    pub attribution: Option<AuditAttribution>,
}

/// Direction for link traversal.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraverseDirection {
    /// Follow outbound links (source → target). Default.
    #[default]
    Forward,
    /// Follow inbound links (target ← source).
    Reverse,
}

/// Request to traverse links from a starting entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraverseRequest {
    /// Starting entity.
    pub collection: CollectionId,
    pub id: EntityId,
    /// Filter traversal to this link type. If `None`, follow all link types.
    pub link_type: Option<String>,
    /// Maximum hop depth (default: 3, capped at 10).
    pub max_depth: Option<usize>,
    /// Direction of traversal: follow outbound or inbound links.
    #[serde(default)]
    pub direction: TraverseDirection,
    /// Optional filter applied to each candidate entity at every hop.
    /// Entities that do not match are excluded (and not traversed further).
    pub hop_filter: Option<FilterNode>,
}

/// Request to check whether a target entity is reachable from a source entity.
///
/// Short-circuits as soon as the target is found, avoiding a full BFS expansion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReachableRequest {
    /// Starting entity.
    pub source_collection: CollectionId,
    pub source_id: EntityId,
    /// Target entity to search for.
    pub target_collection: CollectionId,
    pub target_id: EntityId,
    /// Filter traversal to this link type. If `None`, follow all link types.
    pub link_type: Option<String>,
    /// Maximum hop depth (default: 3, capped at 10).
    pub max_depth: Option<usize>,
    /// Direction of traversal.
    #[serde(default)]
    pub direction: TraverseDirection,
}

/// Request to find link target candidates (US-070, FEAT-020).
///
/// Given a source entity and link type, returns candidate target entities
/// from the target collection with an already-linked indicator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindLinkCandidatesRequest {
    /// Source entity.
    pub source_collection: CollectionId,
    pub source_id: EntityId,
    /// The link type to find candidates for.
    pub link_type: String,
    /// Optional search filter applied to candidate entities.
    pub filter: Option<FilterNode>,
    /// Maximum number of candidates to return.
    pub limit: Option<usize>,
}

/// Request to list an entity's neighbors (US-071, FEAT-020).
///
/// Returns both outbound and inbound linked entities grouped by link type
/// and direction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListNeighborsRequest {
    /// The entity whose neighbors to list.
    pub collection: CollectionId,
    pub id: EntityId,
    /// Filter to a specific link type. If `None`, returns all link types.
    pub link_type: Option<String>,
    /// Filter by direction. If `None`, returns both inbound and outbound.
    pub direction: Option<TraverseDirection>,
}

// ── Audit requests ───────────────────────────────────────────────────────────

/// Request to query the audit log with optional filters and cursor-based pagination.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QueryAuditRequest {
    /// Restrict to entries visible within this database scope.
    ///
    /// This is only set when the transport explicitly selected a current
    /// database via the URL path (e.g. `/tenants/{tenant}/databases/{database}/…`).
    pub database: Option<String>,
    /// Restrict to entries for this collection (single-collection path; kept for
    /// backward compatibility). When `collection_ids` is also provided, the two
    /// are unioned before filtering.
    pub collection: Option<CollectionId>,
    /// Restrict to entries for any of these collections (multi-collection tail, US-079).
    ///
    /// Unioned with `collection` if both are set. When empty and `collection` is `None`,
    /// entries from all collections are returned. Entries are always emitted globally
    /// ordered by `audit_id` ascending so that a single monotonic cursor walks all
    /// requested collections at once.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collection_ids: Vec<CollectionId>,
    /// Restrict to entries for this entity.
    pub entity_id: Option<EntityId>,
    /// Restrict to entries produced by this actor.
    pub actor: Option<String>,
    /// Restrict to entries of this operation type (e.g. `"entity.create"`).
    pub operation: Option<String>,
    /// Restrict to entries carrying this mutation intent ID.
    pub intent_id: Option<String>,
    /// Restrict to entries carrying this approval record ID.
    pub approval_id: Option<String>,
    /// Inclusive start of the time range (nanoseconds since Unix epoch).
    pub since_ns: Option<u64>,
    /// Inclusive end of the time range (nanoseconds since Unix epoch).
    pub until_ns: Option<u64>,
    /// Pagination cursor: last entry ID seen. Omit to start from the beginning.
    pub after_id: Option<u64>,
    /// Maximum number of entries to return per page.
    pub limit: Option<usize>,
}

/// Request to revert an entity to the `before` state recorded in an audit entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevertEntityRequest {
    /// The audit entry whose `before` state should be restored.
    pub audit_entry_id: u64,
    /// Actor performing the revert.
    pub actor: Option<String>,
    /// When `true`, bypass schema validation for the restored state.
    /// Use only when the schema has changed since the audit entry was recorded.
    #[serde(default)]
    pub force: bool,
    /// JWT-derived attribution stamped onto the audit entry (gateway-only; not part of the wire format).
    #[serde(skip)]
    pub attribution: Option<AuditAttribution>,
}

/// Roll an entity back to a prior state recorded in the audit log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackEntityRequest {
    pub collection: CollectionId,
    pub id: EntityId,
    pub target: RollbackEntityTarget,
    /// Optional OCC guard. When omitted, the current stored version is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_version: Option<u64>,
    /// Actor performing the rollback.
    pub actor: Option<String>,
    /// When true, validate and preview the rollback without writing it.
    #[serde(default)]
    pub dry_run: bool,
}

/// How to identify the historical state to restore.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RollbackEntityTarget {
    /// Restore the entity state stored in the audit entry for this entity version.
    Version(u64),
    /// Restore the entity state stored in this specific audit entry.
    AuditEntryId(u64),
}

// ── Entity query requests ─────────────────────────────────────────────────────

/// Comparison operator for a field filter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    /// Field value equals the given value.
    Eq,
    /// Field value does not equal the given value.
    Ne,
    /// Field value is greater than the given value.
    Gt,
    /// Field value is greater than or equal to the given value.
    Gte,
    /// Field value is less than the given value.
    Lt,
    /// Field value is less than or equal to the given value.
    Lte,
    /// Field value is contained in the given array of values.
    In,
    /// String field value contains the given substring (case-sensitive).
    Contains,
}

/// A leaf filter that tests a single field against a value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldFilter {
    /// Dot-separated field path within the entity data (e.g. `"status"`, `"address.city"`).
    pub field: String,
    pub op: FilterOp,
    pub value: serde_json::Value,
}

/// A gate filter: test whether an entity passes a named validation gate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateFilter {
    /// Gate name (e.g. "complete", "review").
    pub gate: String,
    /// If true, match entities that pass this gate; if false, match those that fail.
    pub pass: bool,
}

/// A composable filter node: either a single field test or a boolean combinator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum FilterNode {
    /// Test a single field.
    Field(FieldFilter),
    /// Test a validation gate pass/fail status.
    Gate(GateFilter),
    /// All child filters must match (logical AND).
    And { filters: Vec<FilterNode> },
    /// At least one child filter must match (logical OR).
    Or { filters: Vec<FilterNode> },
}

/// Sort direction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortDirection {
    Asc,
    Desc,
}

/// Sort by a single field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SortField {
    /// Dot-separated field path within entity data.
    pub field: String,
    #[serde(default = "SortDirection::default_asc")]
    pub direction: SortDirection,
}

impl SortDirection {
    fn default_asc() -> Self {
        SortDirection::Asc
    }
}

/// Request to query entities in a collection with optional filtering, sorting,
/// and cursor-based pagination.
///
/// Corresponds to US-011 (FEAT-004): filter, sort, paginate, count.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QueryEntitiesRequest {
    #[serde(default)]
    pub collection: CollectionId,
    /// Optional filter tree. When absent, all entities are returned.
    pub filter: Option<FilterNode>,
    /// Sort order. When empty, entities are returned in entity-ID order.
    #[serde(default)]
    pub sort: Vec<SortField>,
    /// Maximum number of entities to return.
    pub limit: Option<usize>,
    /// Pagination cursor: the last entity ID seen on the previous page.
    /// Omit to start from the beginning.
    pub after_id: Option<EntityId>,
    /// When `true`, return only the count of matching entities without
    /// fetching their full data.
    #[serde(default)]
    pub count_only: bool,
}

// ── Collection lifecycle requests ────────────────────────────────────────────

/// Request to explicitly create a named collection and record the event in the audit log.
///
/// A schema is required at creation time — schemaless collections are not supported (FEAT-001).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCollectionRequest {
    pub name: CollectionId,
    /// The schema that governs entities in this collection.
    /// `schema.collection` must match `name`.
    pub schema: CollectionSchema,
    pub actor: Option<String>,
}

/// Request to drop a collection and all its entities, recording the event in the audit log.
///
/// The caller must set `confirm` to `true` to acknowledge the destructive operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropCollectionRequest {
    pub name: CollectionId,
    pub actor: Option<String>,
    /// Must be `true` to proceed. Prevents accidental drops.
    #[serde(default)]
    pub confirm: bool,
}

/// Request to list all explicitly created collections.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListCollectionsRequest {}

/// Request to describe a single collection (entity count + schema).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DescribeCollectionRequest {
    pub name: CollectionId,
}

/// Request to store or replace a collection markdown template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutCollectionTemplateRequest {
    pub collection: CollectionId,
    pub template: String,
    pub actor: Option<String>,
}

/// Request to retrieve the markdown template for a collection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetCollectionTemplateRequest {
    pub collection: CollectionId,
}

/// Request to delete the markdown template for a collection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteCollectionTemplateRequest {
    pub collection: CollectionId,
    pub actor: Option<String>,
}

// ── Schema requests ──────────────────────────────────────────────────────────

/// Request to store or replace the schema for a collection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutSchemaRequest {
    /// The schema to persist. `schema.collection` must match the target collection.
    pub schema: CollectionSchema,
    /// Optional actor identifier for audit provenance.
    pub actor: Option<String>,
    /// If true, apply even if the change is classified as breaking.
    #[serde(default)]
    pub force: bool,
    /// If true, check compatibility and return the diff without applying.
    #[serde(default)]
    pub dry_run: bool,
}

/// Request to retrieve the schema for a collection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetSchemaRequest {
    pub collection: CollectionId,
}

/// Request to explain the policy decision for a read or mutation preview.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplainPolicyRequest {
    /// Operation to explain: read, create, update, patch, delete, transition,
    /// rollback, transaction, create_link, or delete_link.
    pub operation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collection: Option<CollectionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<EntityId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_version: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_version: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operations: Vec<ExplainPolicyRequest>,
}

/// Request to revalidate all entities in a collection against the current schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevalidateRequest {
    pub collection: CollectionId,
}

// ── Namespace management (US-036) ───────────────────────────────────────────

/// Request to create a schema namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateNamespaceRequest {
    /// Database name.
    pub database: String,
    /// Schema name within the database.
    pub schema: String,
}

/// Request to list schema namespaces within a database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListNamespacesRequest {
    /// Database name.
    pub database: String,
}

/// Request to list collections within a namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListNamespaceCollectionsRequest {
    /// Database name.
    pub database: String,
    /// Schema name within the database.
    pub schema: String,
}

/// Request to drop a schema namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropNamespaceRequest {
    /// Database name.
    pub database: String,
    /// Schema name within the database.
    pub schema: String,
    /// If true, drop all collections within the namespace.
    /// If false, fail if the namespace contains collections.
    #[serde(default)]
    pub force: bool,
}

/// Roll an entire collection back to its state at a given point in time.
///
/// All entity mutations recorded after `timestamp_ns` are reverted.
/// When `dry_run` is `true`, a preview of the affected entities is returned
/// without modifying data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackCollectionRequest {
    pub collection: CollectionId,
    /// Nanoseconds since Unix epoch — the point in time to roll back to.
    pub timestamp_ns: u64,
    /// Actor performing the rollback.
    pub actor: Option<String>,
    /// When true, return a preview without writing changes.
    #[serde(default)]
    pub dry_run: bool,
}

/// Roll back all mutations from a specific transaction.
///
/// All entities affected by the transaction are reverted to their
/// pre-transaction state. When `dry_run` is `true`, a preview of the
/// affected entities is returned without modifying data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackTransactionRequest {
    /// The transaction ID (as recorded in `AuditEntry::transaction_id`).
    pub transaction_id: String,
    /// Actor performing the rollback.
    pub actor: Option<String>,
    /// When true, return a preview without writing changes.
    #[serde(default)]
    pub dry_run: bool,
}

/// Request to diff two schema versions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffSchemaRequest {
    pub collection: CollectionId,
    /// First version to compare.
    pub version_a: u32,
    /// Second version to compare.
    pub version_b: u32,
}

// ── Aggregation requests (US-062, US-063) ───────────────────────────────────

/// Request to count entities, optionally grouped by a field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountEntitiesRequest {
    pub collection: CollectionId,
    /// Optional filter to restrict which entities are counted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterNode>,
    /// Field to group by. If `None`, returns a single total count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_by: Option<String>,
}

/// Aggregation function to apply on a numeric field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AggregateFunction {
    Sum,
    Avg,
    Min,
    Max,
}

/// Request to compute a numeric aggregation over entities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateRequest {
    pub collection: CollectionId,
    /// The aggregation function to apply.
    pub function: AggregateFunction,
    /// The field to aggregate.
    pub field: String,
    /// Optional filter to restrict which entities are included.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterNode>,
    /// Optional field to group by.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_by: Option<String>,
}

// ── Snapshot request (US-080, FEAT-004) ─────────────────────────────────────

/// Request to take a consistent point-in-time snapshot of entities.
///
/// Returns entities from one or more collections as-of the current state plus
/// an `audit_cursor` that captures the audit log high-water mark at snapshot
/// time. Callers can tail the audit log from `audit_cursor` to discover
/// mutations that occurred after the snapshot.
///
/// # V1 caveat
///
/// Multi-page snapshot consistency under concurrent writes is NOT guaranteed
/// by this implementation. `MemoryStorageAdapter` has no storage-level snapshot
/// isolation; concurrent mutations between paginated requests can cause a
/// multi-page snapshot to reflect mixed state (some entities from before a
/// write, others from after). This is acceptable for V1 because the built-in
/// tests are single-threaded. Production-grade multi-page consistency requires
/// storage-level snapshot support and is deferred to a later release.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SnapshotRequest {
    /// Collections to include in the snapshot. `None` means **all** registered
    /// collections visible to the handler.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collections: Option<Vec<CollectionId>>,
    /// Maximum number of entities to return per page. `None` means no limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    /// Opaque pagination cursor returned in a previous response's
    /// `next_page_token`. Omit to start from the beginning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_page_token: Option<String>,
}

// ── Database isolation requests (US-035, FEAT-014) ────────────────────────

/// Request to create a new database (isolated data space).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateDatabaseRequest {
    /// Database name.
    pub name: String,
}

/// Request to drop a database and all its collections.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropDatabaseRequest {
    /// Database name.
    pub name: String,
    /// If true, drop all collections within the database.
    #[serde(default)]
    pub force: bool,
}

/// Request to list all databases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListDatabasesRequest {}

// ── Lifecycle transition request (FEAT-015) ──────────────────────────────────

/// Request to transition an entity through a named lifecycle state machine.
///
/// The lifecycle must be declared in the collection schema (`schema.lifecycles`).
/// The entity's current state is read from `entity.data[lifecycle.field]` and
/// validated against the allowed transitions before writing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionLifecycleRequest {
    pub collection_id: CollectionId,
    pub entity_id: EntityId,
    /// Name of the lifecycle defined in the collection schema.
    pub lifecycle_name: String,
    /// The state the caller wants to transition to.
    pub target_state: String,
    /// The version the caller believes is current (OCC guard).
    pub expected_version: u64,
    /// Optional actor identity for the audit log.
    pub actor: Option<String>,
    /// Optional key-value metadata attached to the audit entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_metadata: Option<HashMap<String, String>>,
    /// JWT-derived attribution stamped onto the audit entry (gateway-only; not part of the wire format).
    #[serde(skip)]
    pub attribution: Option<AuditAttribution>,
}
