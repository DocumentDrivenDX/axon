//! DDx end-to-end integration tests for axon-cypher.
//!
//! Verifies the full parse -> validate -> plan -> execute pipeline using a
//! hand-built 10-bead, 15-link dataset that mirrors the DDx bead graph.
//!
//! Ready beads (open, all deps closed): bead-01, bead-03, bead-05
//! Blocked beads (open, ≥1 non-closed dep): bead-02, bead-04

use axon_cypher::memory_store::QueryLink;
use axon_cypher::schema::{
    IndexedProperty, LabelDef, PlannerConfig, PropertyKind, RelationshipDef, SchemaSnapshot,
};
use axon_cypher::{execute, parse, plan, validate, MemoryQueryStore, QueryEntity};
use serde_json::{json, Value};
use std::collections::BTreeMap;

// ── Fixture helpers ──────────────────────────────────────────────────────────

fn ddx_schema() -> SchemaSnapshot {
    let properties = BTreeMap::from([
        ("id".to_string(), PropertyKind::String),
        ("status".to_string(), PropertyKind::String),
        ("priority".to_string(), PropertyKind::Integer),
        ("updated_at".to_string(), PropertyKind::DateTime),
        ("title".to_string(), PropertyKind::String),
    ]);
    let label = LabelDef {
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
    };
    SchemaSnapshot {
        labels: BTreeMap::from([("DdxBead".to_string(), label)]),
        relationships: BTreeMap::from([(
            "DEPENDS_ON".to_string(),
            RelationshipDef {
                source_labels: vec!["DdxBead".to_string()],
                target_labels: vec!["DdxBead".to_string()],
            },
        )]),
        planner_config: PlannerConfig::default(),
    }
}

/// 10-bead, 15-link in-memory DDx graph.
///
/// Dependency shape (→ = DEPENDS_ON):
///   bead-01 → bead-06(closed), bead-07(closed), bead-10(closed)  [READY]
///   bead-02 → bead-08(in_progress), bead-07, bead-09(review)     [BLOCKED]
///   bead-03 → bead-06(closed), bead-07(closed)                   [READY]
///   bead-04 → bead-09(review), bead-10(closed)                   [BLOCKED]
///   bead-05 → bead-07(closed)                                     [READY]
///   bead-06 → bead-07
///   bead-08 → bead-07, bead-06
///   bead-09 → bead-10
fn ddx_store() -> MemoryQueryStore {
    let mut store = MemoryQueryStore::new();

    let beads: &[(&str, &str, i64, &str)] = &[
        ("bead-01", "open", 5, "alpha"),
        ("bead-02", "open", 4, "beta"),
        ("bead-03", "open", 3, "gamma"),
        ("bead-04", "open", 2, "delta"),
        ("bead-05", "open", 1, "epsilon"),
        ("bead-06", "closed", 3, "zeta"),
        ("bead-07", "closed", 2, "eta"),
        ("bead-08", "in_progress", 4, "theta"),
        ("bead-09", "review", 3, "iota"),
        ("bead-10", "closed", 5, "kappa"),
    ];
    for &(id, status, priority, title) in beads {
        let mut props = BTreeMap::new();
        props.insert("id".to_string(), json!(id));
        props.insert("status".to_string(), json!(status));
        props.insert("priority".to_string(), json!(priority));
        props.insert("title".to_string(), json!(title));
        store.insert_entity(QueryEntity::new(id, ["DdxBead"], props));
    }

    let links: &[(&str, &str)] = &[
        ("bead-01", "bead-06"),
        ("bead-01", "bead-07"),
        ("bead-01", "bead-10"),
        ("bead-02", "bead-08"),
        ("bead-02", "bead-07"),
        ("bead-02", "bead-09"),
        ("bead-03", "bead-06"),
        ("bead-03", "bead-07"),
        ("bead-04", "bead-09"),
        ("bead-04", "bead-10"),
        ("bead-05", "bead-07"),
        ("bead-06", "bead-07"),
        ("bead-08", "bead-07"),
        ("bead-08", "bead-06"),
        ("bead-09", "bead-10"),
    ];
    for &(src, tgt) in links {
        store.insert_link(QueryLink::new(src, "DEPENDS_ON", tgt, BTreeMap::new()));
    }
    store
}

/// Execute a Cypher query end-to-end: parse → validate → plan → execute.
fn run(cypher: &str) -> Vec<BTreeMap<String, Value>> {
    let schema = ddx_schema();
    let query = parse(cypher).expect("query should parse");
    validate(&query, &schema).expect("query should validate");
    let execution_plan = plan(&query, &schema).expect("query should plan");
    execute(&execution_plan, &ddx_store())
        .collect::<Result<Vec<_>, _>>()
        .expect("query should execute")
}

fn str_field<'a>(rows: &'a [BTreeMap<String, Value>], field: &str) -> Vec<&'a str> {
    rows.iter()
        .map(|r| r[field].as_str().expect("field should be a string"))
        .collect()
}

// ── AC1: dataset size ────────────────────────────────────────────────────────

#[test]
fn dataset_has_ten_beads_and_fifteen_links() {
    let store = ddx_store();
    assert_eq!(store.entities_len(), 10, "expected 10 bead entities");
    assert_eq!(store.links_len(), 15, "expected 15 DEPENDS_ON links");
}

// ── AC2: DDx ready / blocked end-to-end ─────────────────────────────────────

#[test]
fn ddx_ready_query_returns_open_beads_whose_deps_are_all_closed() {
    let rows = run(
        r"
        MATCH (b:DdxBead {status: 'open'})
        WHERE NOT EXISTS {
            MATCH (b)-[:DEPENDS_ON]->(d:DdxBead)
            WHERE d.status <> 'closed'
        }
        RETURN b.id AS id
        ORDER BY b.priority DESC
        ",
    );
    assert_eq!(
        str_field(&rows, "id"),
        vec!["bead-01", "bead-03", "bead-05"],
        "ready beads should be bead-01 (prio 5), bead-03 (prio 3), bead-05 (prio 1)"
    );
}

#[test]
fn ddx_blocked_query_returns_open_beads_with_at_least_one_non_closed_dep() {
    let rows = run(
        r"
        MATCH (b:DdxBead {status: 'open'})
        WHERE EXISTS {
            MATCH (b)-[:DEPENDS_ON]->(d:DdxBead)
            WHERE d.status <> 'closed'
        }
        RETURN b.id AS id
        ORDER BY b.priority DESC
        ",
    );
    assert_eq!(
        str_field(&rows, "id"),
        vec!["bead-02", "bead-04"],
        "blocked beads should be bead-02 (prio 4) and bead-04 (prio 2)"
    );
}

// ── AC3: dependency DAG traversal (US-023-style) ─────────────────────────────

#[test]
fn dependency_dag_direct_deps_of_bead_01_ordered_by_id() {
    let rows = run(
        r"
        MATCH (b:DdxBead {id: 'bead-01'})-[:DEPENDS_ON]->(d:DdxBead)
        RETURN d.id AS dep_id
        ORDER BY d.id ASC
        ",
    );
    assert_eq!(
        str_field(&rows, "dep_id"),
        vec!["bead-06", "bead-07", "bead-10"],
        "bead-01 should have exactly three direct dependencies"
    );
}

#[test]
fn dependency_dag_leaf_node_has_no_deps() {
    let rows = run(
        r"
        MATCH (b:DdxBead {id: 'bead-07'})-[:DEPENDS_ON]->(d:DdxBead)
        RETURN d.id AS dep_id
        ",
    );
    assert!(rows.is_empty(), "bead-07 is a leaf with no outgoing dependencies");
}

// ── AC4: reachability (US-025-style) ─────────────────────────────────────────

#[test]
fn reachability_bead_02_transitive_deps_via_variable_length_path() {
    // bead-02 → {bead-08, bead-07, bead-09}          (depth 1)
    //   bead-08 → {bead-07, bead-06}                  (depth 2)
    //   bead-09 → {bead-10}                           (depth 2)
    //   bead-06 → {bead-07}                           (depth 3, duplicate)
    // Unique transitive deps: bead-06, bead-07, bead-08, bead-09, bead-10
    let rows = run(
        r"
        MATCH (b:DdxBead {id: 'bead-02'})-[:DEPENDS_ON*1..3]->(d:DdxBead)
        RETURN DISTINCT d.id AS dep_id
        ORDER BY d.id ASC
        ",
    );
    assert_eq!(
        str_field(&rows, "dep_id"),
        vec!["bead-06", "bead-07", "bead-08", "bead-09", "bead-10"],
        "transitive deps of bead-02 at depth 1-3 should be five unique beads"
    );
}

// ── AC5: clause coverage ─────────────────────────────────────────────────────

#[test]
fn distinct_deduplicates_dep_ids_across_all_open_beads() {
    // Open beads share several dependencies (e.g., bead-07 appears 5 times).
    // DISTINCT should collapse to 5 unique dep IDs.
    let rows = run(
        r"
        MATCH (b:DdxBead {status: 'open'})-[:DEPENDS_ON]->(d:DdxBead)
        RETURN DISTINCT d.id AS dep_id
        ORDER BY d.id ASC
        ",
    );
    assert_eq!(
        str_field(&rows, "dep_id"),
        vec!["bead-06", "bead-07", "bead-08", "bead-09", "bead-10"],
        "DISTINCT should return 5 unique dependency IDs across all open beads"
    );
}

#[test]
fn optional_match_produces_null_binding_when_no_outgoing_dep_exists() {
    // bead-10 has no outgoing DEPENDS_ON links, so d should be null.
    let rows = run(
        r"
        MATCH (b:DdxBead {id: 'bead-10'})
        OPTIONAL MATCH (b:DdxBead)-[:DEPENDS_ON]->(d:DdxBead)
        RETURN b.title AS title, d.id AS dep_id
        ",
    );
    assert_eq!(rows.len(), 1, "should return exactly one row");
    assert_eq!(rows[0]["title"], json!("kappa"));
    assert_eq!(
        rows[0]["dep_id"],
        Value::Null,
        "dep_id should be null when no dependency exists"
    );
}

#[test]
fn exists_true_finds_beads_that_have_at_least_one_non_closed_dep() {
    let rows = run(
        r"
        MATCH (b:DdxBead {status: 'open'})
        WHERE EXISTS {
            MATCH (b)-[:DEPENDS_ON]->(d:DdxBead)
            WHERE d.status <> 'closed'
        }
        RETURN b.id AS id
        ORDER BY b.id ASC
        ",
    );
    assert_eq!(
        str_field(&rows, "id"),
        vec!["bead-02", "bead-04"],
        "EXISTS should match the two blocked beads"
    );
}

#[test]
fn not_exists_finds_beads_with_no_non_closed_deps() {
    let rows = run(
        r"
        MATCH (b:DdxBead {status: 'open'})
        WHERE NOT EXISTS {
            MATCH (b)-[:DEPENDS_ON]->(d:DdxBead)
            WHERE d.status <> 'closed'
        }
        RETURN b.id AS id
        ORDER BY b.id ASC
        ",
    );
    assert_eq!(
        str_field(&rows, "id"),
        vec!["bead-01", "bead-03", "bead-05"],
        "NOT EXISTS should match the three ready beads"
    );
}

#[test]
fn count_star_counts_all_open_beads() {
    let rows = run("MATCH (b:DdxBead {status: 'open'}) RETURN count(*) AS n");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["n"], json!(5), "there should be 5 open beads");
}

#[test]
fn order_by_priority_asc_returns_open_beads_in_ascending_order() {
    let rows = run(
        r"
        MATCH (b:DdxBead {status: 'open'})
        RETURN b.id AS id
        ORDER BY b.priority ASC
        ",
    );
    assert_eq!(
        str_field(&rows, "id"),
        vec!["bead-05", "bead-04", "bead-03", "bead-02", "bead-01"],
        "open beads should be ordered priority 1→5"
    );
}

#[test]
fn order_by_priority_desc_returns_open_beads_in_descending_order() {
    let rows = run(
        r"
        MATCH (b:DdxBead {status: 'open'})
        RETURN b.id AS id
        ORDER BY b.priority DESC
        ",
    );
    assert_eq!(
        str_field(&rows, "id"),
        vec!["bead-01", "bead-02", "bead-03", "bead-04", "bead-05"],
        "open beads should be ordered priority 5→1"
    );
}
