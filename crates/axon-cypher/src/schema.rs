//! Schema module: re-exports `axon-cypher-ast`'s schema types and adds
//! `SchemaSnapshotExt` for compiling named queries (which requires the
//! planner and is therefore not available in the leaf crate).

pub use axon_cypher_ast::schema::*;

use std::collections::BTreeMap;

/// Extension trait that adds named-query compilation to [`SchemaSnapshot`].
///
/// Defined here rather than on the type itself because the implementation
/// depends on `crate::planner::ExecutionPlan`, which lives in `axon-cypher`
/// and cannot be referenced from the leaf crate `axon-cypher-ast`.
pub trait SchemaSnapshotExt {
    /// Compile all schema-declared named queries into execution plans.
    ///
    /// Named queries bypass cardinality-budget enforcement because they are
    /// reviewed at schema-write time, not submitted ad-hoc at runtime.
    ///
    /// Returns a map from query name to compiled [`ExecutionPlan`].
    /// Fails on the first query that does not parse, validate, or plan.
    fn activate_named_queries(
        &self,
    ) -> Result<BTreeMap<String, crate::planner::ExecutionPlan>, crate::error::CypherError>;
}

impl SchemaSnapshotExt for SchemaSnapshot {
    fn activate_named_queries(
        &self,
    ) -> Result<BTreeMap<String, crate::planner::ExecutionPlan>, crate::error::CypherError> {
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
}

#[cfg(test)]
pub mod test_fixtures {
    //! Reusable schema fixtures for axon-cypher tests.
    //!
    //! This module re-declares the DDx fixture with named queries so that
    //! executor tests can call `activate_named_queries()`. The leaf-crate
    //! fixture in `axon-cypher-ast::schema::test_fixtures` omits named
    //! queries because it cannot reference the planner.

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
        use crate::schema::SchemaSnapshotExt;
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
