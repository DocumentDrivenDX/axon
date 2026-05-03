//! Minimal schema snapshot used by the validator and planner.
//!
//! This is a stand-in for the eventual integration with `axon-schema`. It
//! captures only what the planner needs: which labels exist, what
//! properties they have (and which are indexed), and what relationship
//! types connect them. The full ESF schema is much richer; the planner
//! does not need the rest.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A snapshot of the schema the planner uses for type-checking and
/// index selection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaSnapshot {
    /// Map from label name (e.g. `"DdxBead"`) to the collection definition.
    pub labels: BTreeMap<String, LabelDef>,
    /// Map from relationship-type name (e.g. `"DEPENDS_ON"`) to its
    /// definition.
    pub relationships: BTreeMap<String, RelationshipDef>,
    /// Planner limits and feature flags applied when compiling queries.
    #[serde(default)]
    pub planner_config: PlannerConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelDef {
    /// The collection ID this label maps to in storage.
    pub collection_name: String,
    /// Estimated number of live entities in this collection.
    pub estimated_count: u64,
    /// Properties declared on this label and their types.
    pub properties: BTreeMap<String, PropertyKind>,
    /// Property paths that have a secondary index per FEAT-013.
    /// Each is a single-field index here; compound indexes can be added
    /// later as a separate map.
    pub indexed_properties: Vec<IndexedProperty>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropertyKind {
    String,
    Integer,
    Float,
    Boolean,
    DateTime,
    /// Any other shape — JSON object, array, etc. Not indexable.
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexedProperty {
    pub property: String,
    pub kind: PropertyKind,
    pub unique: bool,
    /// Estimated rows returned by an equality lookup on this index.
    pub estimated_equality_rows: u64,
    /// Estimated rows returned by a range lookup on this index.
    pub estimated_range_rows: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationshipDef {
    /// Source labels this relationship may connect from.
    pub source_labels: Vec<String>,
    /// Target labels this relationship may connect to.
    pub target_labels: Vec<String>,
}

/// Planner limits for ad-hoc and schema-declared query compilation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannerConfig {
    /// Maximum collection size where an unindexed full scan is accepted.
    pub unindexed_scan_threshold: u64,
    /// Maximum variable-length traversal depth.
    pub depth_cap: u32,
    /// Maximum estimated intermediate rows for ad-hoc plans.
    pub cardinality_budget: u64,
    /// Named queries can opt into larger reviewed plans by disabling this.
    pub enforce_cardinality_budget: bool,
}

impl Default for PlannerConfig {
    fn default() -> Self {
        Self {
            unindexed_scan_threshold: 1_000,
            depth_cap: 10,
            cardinality_budget: 1_000_000,
            enforce_cardinality_budget: true,
        }
    }
}

impl SchemaSnapshot {
    pub fn label(&self, name: &str) -> Option<&LabelDef> {
        self.labels.get(name)
    }

    pub fn relationship(&self, name: &str) -> Option<&RelationshipDef> {
        self.relationships.get(name)
    }

    pub fn has_label(&self, name: &str) -> bool {
        self.labels.contains_key(name)
    }

    pub fn has_relationship(&self, name: &str) -> bool {
        self.relationships.contains_key(name)
    }
}

impl LabelDef {
    pub fn property(&self, name: &str) -> Option<PropertyKind> {
        self.properties.get(name).copied()
    }

    pub fn is_indexed(&self, property: &str) -> bool {
        self.indexed_properties
            .iter()
            .any(|p| p.property == property)
    }

    pub fn unique_index(&self, property: &str) -> bool {
        self.indexed_properties
            .iter()
            .any(|p| p.property == property && p.unique)
    }

    pub fn index(&self, property: &str) -> Option<&IndexedProperty> {
        self.indexed_properties
            .iter()
            .find(|p| p.property == property)
    }
}

#[cfg(test)]
pub mod test_fixtures {
    //! Reusable schema fixtures for tests.

    use super::*;

    /// Schema fixture matching the DDx use case in axon-05c1019d.
    pub fn ddx_beads() -> SchemaSnapshot {
        let mut labels = BTreeMap::new();
        let mut properties = BTreeMap::new();
        properties.insert("id".to_string(), PropertyKind::String);
        properties.insert("status".to_string(), PropertyKind::String);
        properties.insert("priority".to_string(), PropertyKind::Integer);
        properties.insert("updated_at".to_string(), PropertyKind::DateTime);
        properties.insert("title".to_string(), PropertyKind::String);

        labels.insert(
            "DdxBead".to_string(),
            LabelDef {
                collection_name: "ddx_beads".to_string(),
                estimated_count: 10_000,
                properties,
                indexed_properties: vec![
                    IndexedProperty {
                        property: "status".to_string(),
                        kind: PropertyKind::String,
                        unique: false,
                        estimated_equality_rows: 2_500,
                        estimated_range_rows: 7_500,
                    },
                    IndexedProperty {
                        property: "priority".to_string(),
                        kind: PropertyKind::Integer,
                        unique: false,
                        estimated_equality_rows: 500,
                        estimated_range_rows: 5_000,
                    },
                    IndexedProperty {
                        property: "id".to_string(),
                        kind: PropertyKind::String,
                        unique: true,
                        estimated_equality_rows: 1,
                        estimated_range_rows: 10_000,
                    },
                ],
            },
        );

        let mut relationships = BTreeMap::new();
        relationships.insert(
            "DEPENDS_ON".to_string(),
            RelationshipDef {
                source_labels: vec!["DdxBead".to_string()],
                target_labels: vec!["DdxBead".to_string()],
            },
        );

        SchemaSnapshot {
            labels,
            relationships,
            planner_config: PlannerConfig::default(),
        }
    }
}
