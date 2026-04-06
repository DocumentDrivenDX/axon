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
    /// Restrict to entries for this collection.
    pub collection: Option<CollectionId>,
    /// Restrict to entries for this entity.
    pub entity_id: Option<EntityId>,
    /// Restrict to entries produced by this actor.
    pub actor: Option<String>,
    /// Restrict to entries of this operation type (e.g. `"entity.create"`).
    pub operation: Option<String>,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropCollectionRequest {
    pub name: CollectionId,
    pub actor: Option<String>,
}

/// Request to list all explicitly created collections.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListCollectionsRequest {}

/// Request to describe a single collection (entity count + schema).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DescribeCollectionRequest {
    pub name: CollectionId,
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
