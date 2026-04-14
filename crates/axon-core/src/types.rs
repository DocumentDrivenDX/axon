use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::id::{CollectionId, EntityId};

/// The name of the internal collection used to store links.
pub const LINKS_COLLECTION: &str = "__axon_links__";

/// The name of the reverse-index collection for efficient inbound-link queries.
///
/// Entries use IDs formatted as `{target_col}/{target_id}/{source_col}/{source_id}/{link_type}`,
/// enabling a targeted prefix scan to find all links pointing at a given entity
/// without a full table scan of the main links collection.
pub const LINKS_REV_COLLECTION: &str = "__axon_links_rev__";

/// A typed directional edge between two entities.
///
/// Links are stored as entities in the [`LINKS_COLLECTION`] pseudo-collection
/// so that the existing `StorageAdapter` trait can persist them without
/// requiring a separate storage interface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Link {
    /// Collection of the source entity.
    pub source_collection: CollectionId,
    /// ID of the source entity.
    pub source_id: EntityId,
    /// Collection of the target entity.
    pub target_collection: CollectionId,
    /// ID of the target entity.
    pub target_id: EntityId,
    /// Semantic label for this edge (e.g., `"belongs-to"`, `"depends-on"`).
    pub link_type: String,
    /// Optional metadata attached to this edge.
    #[serde(default)]
    pub metadata: Value,
}

impl Link {
    /// Computes the canonical storage ID for a link entity.
    ///
    /// Format: `<source_col>/<source_id>/<link_type>/<target_col>/<target_id>`
    pub fn storage_id(
        source_col: &CollectionId,
        source_id: &EntityId,
        link_type: &str,
        target_col: &CollectionId,
        target_id: &EntityId,
    ) -> EntityId {
        EntityId::new(format!(
            "{}/{}/{}/{}/{}",
            source_col, source_id, link_type, target_col, target_id,
        ))
    }

    /// Returns the internal collection that holds all links.
    pub fn links_collection() -> CollectionId {
        CollectionId::new(LINKS_COLLECTION)
    }

    /// Returns the reverse-index collection for inbound-link queries.
    pub fn links_rev_collection() -> CollectionId {
        CollectionId::new(LINKS_REV_COLLECTION)
    }

    /// Computes the reverse-index storage ID for an inbound-link entry.
    ///
    /// Format: `<target_col>/<target_id>/<source_col>/<source_id>/<link_type>`
    ///
    /// IDs with the same `target_col/target_id/` prefix can be found with a
    /// bounded `range_scan`, avoiding a full table scan.
    pub fn rev_storage_id(
        target_col: &CollectionId,
        target_id: &EntityId,
        source_col: &CollectionId,
        source_id: &EntityId,
        link_type: &str,
    ) -> EntityId {
        EntityId::new(format!(
            "{}/{}/{}/{}/{}",
            target_col, target_id, source_col, source_id, link_type,
        ))
    }

    /// Serializes this link into a reverse-index [`Entity`].
    ///
    /// The entity carries no data payload; only its ID is meaningful.
    pub fn to_rev_entity(&self) -> Entity {
        let rev_id = Self::rev_storage_id(
            &self.target_collection,
            &self.target_id,
            &self.source_collection,
            &self.source_id,
            &self.link_type,
        );
        Entity::new(
            Self::links_rev_collection(),
            rev_id,
            serde_json::Value::Null,
        )
    }

    /// Serializes this link into a storage [`Entity`].
    pub fn to_entity(&self) -> Entity {
        let storage_id = Self::storage_id(
            &self.source_collection,
            &self.source_id,
            &self.link_type,
            &self.target_collection,
            &self.target_id,
        );
        Entity::new(
            Self::links_collection(),
            storage_id,
            serde_json::to_value(self).expect("Link is always serializable"),
        )
    }

    /// Deserializes a [`Link`] from a storage entity.
    pub fn from_entity(entity: &Entity) -> Option<Self> {
        serde_json::from_value(entity.data.clone()).ok()
    }
}

/// Result of evaluating a single validation rule against entity data.
///
/// Defined in `axon-core` (not `axon-schema`) so `Entity` can carry
/// materialized gate results as a first-class field (FEAT-019).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleViolation {
    /// Rule name.
    pub rule: String,
    /// Field that failed.
    pub field: String,
    /// Human-readable message.
    pub message: String,
    /// Fix suggestion (if provided).
    pub fix: Option<String>,
    /// Gate this rule belongs to.
    pub gate: Option<String>,
    /// Whether this is advisory-only.
    pub advisory: bool,
    /// Context: which condition triggered the rule.
    pub context: Option<Value>,
}

/// Result of evaluating a single gate for an entity.
///
/// Defined in `axon-core` (not `axon-schema`) so `Entity` can carry
/// materialized gate results as a first-class field (FEAT-019).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateResult {
    /// Gate name.
    pub gate: String,
    /// Whether all rules in the gate (including inherited) pass.
    pub pass: bool,
    /// Violations for this gate.
    pub failures: Vec<RuleViolation>,
}

/// A versioned entity stored in a collection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    /// The collection this entity belongs to.
    pub collection: CollectionId,
    /// Unique identifier within the collection.
    pub id: EntityId,
    /// Monotonically increasing version; starts at 1.
    pub version: u64,
    /// The entity data as an arbitrary JSON object.
    pub data: Value,
    /// Nanoseconds since Unix epoch when the entity was first created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at_ns: Option<u64>,
    /// Nanoseconds since Unix epoch when the entity was last updated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at_ns: Option<u64>,
    /// Actor that created this entity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    /// Actor that last updated this entity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    /// Schema version that validated this entity on create/update.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,
    /// Gate evaluation results materialized at write time (FEAT-019).
    ///
    /// Populated by the handler before storage writes so that stored entities
    /// carry their gate verdicts alongside their data. `#[serde(default)]`
    /// keeps backward compatibility with entities serialized before this
    /// field existed.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub gate_results: HashMap<String, GateResult>,
}

impl Entity {
    pub fn new(collection: CollectionId, id: EntityId, data: Value) -> Self {
        Self {
            collection,
            id,
            version: 1,
            data,
            created_at_ns: None,
            updated_at_ns: None,
            created_by: None,
            updated_by: None,
            schema_version: None,
            gate_results: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn entity_new_starts_at_version_one() {
        let entity = Entity::new(
            CollectionId::new("tasks"),
            EntityId::new("t-001"),
            json!({"title": "hello"}),
        );
        assert_eq!(entity.version, 1);
        assert_eq!(entity.data["title"], "hello");
    }

    #[test]
    fn entity_new_starts_with_empty_gate_results() {
        let entity = Entity::new(
            CollectionId::new("tasks"),
            EntityId::new("t-001"),
            json!({"title": "hello"}),
        );
        assert!(entity.gate_results.is_empty());
    }

    #[test]
    fn entity_gate_results_serde_roundtrip() {
        let mut entity = Entity::new(
            CollectionId::new("beads"),
            EntityId::new("b-1"),
            json!({"bead_type": "invoice"}),
        );
        entity.gate_results.insert(
            "complete".into(),
            GateResult {
                gate: "complete".into(),
                pass: false,
                failures: vec![
                    RuleViolation {
                        rule: "need-desc".into(),
                        field: "description".into(),
                        message: "description is required".into(),
                        fix: Some("set description".into()),
                        gate: Some("complete".into()),
                        advisory: false,
                        context: None,
                    },
                    RuleViolation {
                        rule: "need-owner".into(),
                        field: "owner".into(),
                        message: "owner is required".into(),
                        fix: None,
                        gate: Some("complete".into()),
                        advisory: false,
                        context: Some(json!({"when": "bead_type=invoice"})),
                    },
                ],
            },
        );

        let json = serde_json::to_value(&entity).expect("serialize");
        let roundtripped: Entity = serde_json::from_value(json).expect("deserialize");
        assert_eq!(roundtripped, entity);
        assert_eq!(roundtripped.gate_results.len(), 1);
        assert_eq!(
            roundtripped
                .gate_results
                .get("complete")
                .expect("present")
                .failures
                .len(),
            2
        );
    }

    #[test]
    fn entity_without_gate_results_field_deserializes_to_empty_map() {
        // Emulate an entity persisted before the gate_results field was added.
        let legacy = json!({
            "collection": "tasks",
            "id": "t-001",
            "version": 3,
            "data": {"title": "hello"}
        });
        let entity: Entity = serde_json::from_value(legacy).expect("deserialize legacy");
        assert_eq!(entity.version, 3);
        assert_eq!(entity.data["title"], "hello");
        assert!(entity.gate_results.is_empty());
    }
}
