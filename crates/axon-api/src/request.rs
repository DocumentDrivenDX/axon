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

/// A composable filter node: either a single field test or a boolean combinator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum FilterNode {
    /// Test a single field.
    Field(FieldFilter),
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCollectionRequest {
    pub name: CollectionId,
    pub actor: Option<String>,
}

/// Request to drop a collection and all its entities, recording the event in the audit log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropCollectionRequest {
    pub name: CollectionId,
    pub actor: Option<String>,
}

// ── Schema requests ──────────────────────────────────────────────────────────

/// Request to store or replace the schema for a collection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutSchemaRequest {
    /// The schema to persist. `schema.collection` must match the target collection.
    pub schema: CollectionSchema,
}

/// Request to retrieve the schema for a collection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetSchemaRequest {
    pub collection: CollectionId,
}
