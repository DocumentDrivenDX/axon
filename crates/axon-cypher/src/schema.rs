//! Minimal schema snapshot used by the validator and planner.
//!
//! This is a stand-in for the eventual integration with `axon-schema`. It
//! captures only what the planner needs: which labels exist, what
//! properties they have (and which are indexed), and what relationship
//! types connect them. The full ESF schema is much richer; the planner
//! does not need the rest.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A schema-declared named query (per FEAT-009 US-074 / US-075).
///
/// Named queries are compiled and policy-validated at `put_schema` time
/// and exposed as typed GraphQL fields and MCP tools on the collection.
/// They bypass ad-hoc cardinality-budget enforcement because they are
/// reviewed offline before activation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedQuery {
    /// Human-readable description, surfaced as GraphQL field doc and MCP
    /// tool description.
    pub description: String,
    /// The openCypher query string (read-only V1 subset per ADR-021).
    pub cypher: String,
    /// Named parameters the query accepts (`$param` references in the cypher).
    #[serde(default)]
    pub parameters: Vec<String>,
}

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
    /// Schema-declared named queries for this collection (per FEAT-009).
    /// Compiled and validated at schema activation time.
    #[serde(default)]
    pub queries: BTreeMap<String, NamedQuery>,
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
    /// Compile all schema-declared named queries into execution plans.
    ///
    /// Each query is parsed, validated against this schema, and planned.
    /// Named queries bypass cardinality-budget enforcement because they are
    /// reviewed at schema-write time, not submitted ad-hoc at runtime.
    ///
    /// Returns a map from query name to compiled [`crate::ExecutionPlan`].
    /// Fails on the first query that does not parse, validate, or plan.
    pub fn activate_named_queries(
        &self,
    ) -> Result<BTreeMap<String, crate::planner::ExecutionPlan>, crate::error::CypherError> {
        // Named queries are validated offline; relax the ad-hoc budget.
        let mut relaxed = self.clone();
        relaxed.planner_config.enforce_cardinality_budget = false;

        let mut plans = BTreeMap::new();
        for (name, named_query) in &self.queries {
            let parsed = crate::parse(&named_query.cypher)?;
            let plan = crate::plan(&parsed, &relaxed)?;
            plans.insert(name.clone(), plan);
        }
        Ok(plans)
    }

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

    const READY_BEADS_CYPHER: &str = "\
MATCH (b:DdxBead {status: 'open'})
WHERE NOT EXISTS {
    MATCH (b)-[:DEPENDS_ON]->(d:DdxBead)
    WHERE d.status <> 'closed'
}
RETURN b
ORDER BY b.priority DESC, b.updated_at DESC";

    const BLOCKED_BEADS_CYPHER: &str = "\
MATCH (b:DdxBead {status: 'open'})
WHERE EXISTS {
    MATCH (b)-[:DEPENDS_ON]->(d:DdxBead)
    WHERE d.status <> 'closed'
}
RETURN b
ORDER BY b.priority DESC, b.updated_at DESC";

    /// Schema fixture matching the DDx use case in axon-05c1019d.
    ///
    /// Declares `ready_beads` and `blocked_beads` named queries per
    /// FEAT-009 US-074 so that `activate_named_queries()` can be called
    /// to verify schema-declared compilation.
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

        let mut queries = BTreeMap::new();
        queries.insert(
            "ready_beads".to_string(),
            NamedQuery {
                description: "Open beads with no open dependencies".to_string(),
                cypher: READY_BEADS_CYPHER.to_string(),
                parameters: Vec::new(),
            },
        );
        queries.insert(
            "blocked_beads".to_string(),
            NamedQuery {
                description: "Open beads blocked by at least one open dependency".to_string(),
                cypher: BLOCKED_BEADS_CYPHER.to_string(),
                parameters: Vec::new(),
            },
        );

        SchemaSnapshot {
            labels,
            relationships,
            planner_config: PlannerConfig::default(),
            queries,
        }
    }

    #[test]
    fn ddx_beads_named_queries_activate_without_error() {
        let schema = ddx_beads();
        let plans = schema
            .activate_named_queries()
            .expect("ddx_beads named queries should activate without error");
        assert_eq!(
            plans.len(),
            2,
            "expected ready_beads and blocked_beads plans"
        );
        assert!(
            plans.contains_key("ready_beads"),
            "ready_beads plan missing"
        );
        assert!(
            plans.contains_key("blocked_beads"),
            "blocked_beads plan missing"
        );
    }
}
