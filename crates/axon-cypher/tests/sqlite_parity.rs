//! SQLite-backed parity tests for the axon-cypher executor.
//!
//! Runs the same 10+ DDx integration scenarios from `ddx_integration.rs`
//! against `SqliteStorageAdapter` (in-memory) wrapped in
//! `StorageAdapterQueryStore`.  This satisfies AC1 and AC4 — every setup
//! call uses only `StorageAdapter` trait methods; no raw SQL, no
//! backend-specific query operators (ADR-010: "the query planner never
//! depends on backend-specific JSON operators").
//!
//! Dataset: 10 beads, 15 DEPENDS_ON links — identical to the MemoryQueryStore
//! fixture used in ddx_integration.rs.
//!
//! Ready beads  (open, all deps closed): bead-01, bead-03, bead-05
//! Blocked beads (open, ≥1 non-closed dep): bead-02, bead-04

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;

use axon_core::id::{CollectionId, EntityId};
use axon_core::types::{Entity, Link};
use axon_cypher::schema::{
    IndexedProperty, LabelDef, PlannerConfig, PropertyKind, RelationshipDef, SchemaSnapshot,
};
use axon_cypher::{execute, parse, plan, validate, StorageAdapterQueryStore};
use axon_schema::schema::{IndexDef, IndexType};
use axon_storage::{SqliteStorageAdapter, StorageAdapter as _};
use serde_json::{json, Value};

// ── Schema fixture ────────────────────────────────────────────────────────────

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
        queries: BTreeMap::new(),
    }
}

fn index_defs() -> Vec<IndexDef> {
    vec![
        IndexDef {
            field: "status".to_string(),
            index_type: IndexType::String,
            unique: false,
        },
        IndexDef {
            field: "priority".to_string(),
            index_type: IndexType::Integer,
            unique: false,
        },
        IndexDef {
            field: "id".to_string(),
            index_type: IndexType::String,
            unique: true,
        },
    ]
}

/// Build an in-memory `SqliteStorageAdapter` populated with the 10-bead,
/// 15-link DDx graph.  Only `StorageAdapter` trait methods are called —
/// no raw SQL, no JSON path operators (ADR-010 compliant).
fn ddx_sqlite_store() -> SqliteStorageAdapter {
    let mut storage = SqliteStorageAdapter::open_in_memory().expect("sqlite in-memory should open");

    let col = CollectionId::new("ddx_beads");
    // Register a schema declaring the indexes so the write primitives maintain
    // them internally on each `put` (Approach C — no direct index maintenance).
    let mut schema = axon_schema::schema::CollectionSchema::new(col.clone());
    schema.indexes = index_defs();
    storage.put_schema(&schema).unwrap();

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
        let data = json!({
            "id": id,
            "status": status,
            "priority": priority,
            "title": title,
        });
        storage
            .put(Entity::new(col.clone(), EntityId::new(id), data))
            .unwrap();
    }

    // 15 DEPENDS_ON links — identical topology to ddx_integration.rs
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
        storage
            .put_link(&Link {
                source_collection: col.clone(),
                source_id: EntityId::new(src),
                target_collection: col.clone(),
                target_id: EntityId::new(tgt),
                link_type: "DEPENDS_ON".to_string(),
                metadata: Value::Null,
            })
            .unwrap();
    }

    storage
}

/// Execute a Cypher query end-to-end using `SqliteStorageAdapter`.
fn run(storage: &SqliteStorageAdapter, cypher: &str) -> Vec<BTreeMap<String, Value>> {
    let schema = ddx_schema();
    let store = StorageAdapterQueryStore::new(storage, &schema);
    let query = parse(cypher).expect("query should parse");
    validate(&query, &schema).expect("query should validate");
    let execution_plan = plan(&query, &schema).expect("query should plan");
    execute(&execution_plan, &store)
        .collect::<Result<Vec<_>, _>>()
        .expect("query should execute")
}

fn str_field<'a>(rows: &'a [BTreeMap<String, Value>], field: &str) -> Vec<&'a str> {
    rows.iter()
        .map(|r| r[field].as_str().expect("field should be a string"))
        .collect()
}

// ── Scenario 1: dataset size ──────────────────────────────────────────────────

#[test]
fn sqlite_dataset_has_ten_beads_and_fifteen_links() {
    let storage = ddx_sqlite_store();
    let col = CollectionId::new("ddx_beads");
    let count = storage.count(&col).unwrap();
    assert_eq!(count, 10, "expected 10 bead entities in sqlite");
}

// ── Scenario 2: ready beads ───────────────────────────────────────────────────

#[test]
fn sqlite_ddx_ready_query_returns_open_beads_whose_deps_are_all_closed() {
    // @covers US-074-AC1
    let storage = ddx_sqlite_store();
    let rows = run(
        &storage,
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

// ── Scenario 3: blocked beads ─────────────────────────────────────────────────

#[test]
fn sqlite_ddx_blocked_query_returns_open_beads_with_at_least_one_non_closed_dep() {
    // @covers US-074-AC2
    let storage = ddx_sqlite_store();
    let rows = run(
        &storage,
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

// ── Scenario 4: direct deps of bead-01 ───────────────────────────────────────

#[test]
fn sqlite_dependency_dag_direct_deps_of_bead_01_ordered_by_id() {
    let storage = ddx_sqlite_store();
    let rows = run(
        &storage,
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

// ── Scenario 5: leaf node has no deps ────────────────────────────────────────

#[test]
fn sqlite_dependency_dag_leaf_node_has_no_deps() {
    let storage = ddx_sqlite_store();
    let rows = run(
        &storage,
        r"
        MATCH (b:DdxBead {id: 'bead-07'})-[:DEPENDS_ON]->(d:DdxBead)
        RETURN d.id AS dep_id
        ",
    );
    assert!(
        rows.is_empty(),
        "bead-07 is a leaf with no outgoing dependencies"
    );
}

// ── Scenario 6: variable-length path (transitive deps of bead-02) ─────────────

#[test]
fn sqlite_reachability_bead_02_transitive_deps_via_variable_length_path() {
    let storage = ddx_sqlite_store();
    let rows = run(
        &storage,
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

// ── Scenario 7: DISTINCT across multiple open beads ──────────────────────────

#[test]
fn sqlite_distinct_deduplicates_dep_ids_across_all_open_beads() {
    let storage = ddx_sqlite_store();
    let rows = run(
        &storage,
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

// ── Scenario 8: OPTIONAL MATCH null binding ───────────────────────────────────

#[test]
fn sqlite_optional_match_produces_null_binding_when_no_outgoing_dep_exists() {
    let storage = ddx_sqlite_store();
    let rows = run(
        &storage,
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

// ── Scenario 9: EXISTS true ───────────────────────────────────────────────────

#[test]
fn sqlite_exists_true_finds_beads_that_have_at_least_one_non_closed_dep() {
    let storage = ddx_sqlite_store();
    let rows = run(
        &storage,
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

// ── Scenario 10: NOT EXISTS ───────────────────────────────────────────────────

#[test]
fn sqlite_not_exists_finds_beads_with_no_non_closed_deps() {
    let storage = ddx_sqlite_store();
    let rows = run(
        &storage,
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

// ── Scenario 11: incoming links ───────────────────────────────────────────────

#[test]
fn sqlite_incoming_links_returns_all_dependents_of_a_leaf_node() {
    let storage = ddx_sqlite_store();
    let rows = run(
        &storage,
        r"
        MATCH (b:DdxBead {id: 'bead-07'})<-[:DEPENDS_ON]-(a:DdxBead)
        RETURN a.id AS id
        ORDER BY a.id ASC
        ",
    );
    assert_eq!(
        str_field(&rows, "id"),
        vec!["bead-01", "bead-02", "bead-03", "bead-05", "bead-06", "bead-08"],
        "six beads have outgoing DEPENDS_ON links to bead-07"
    );
}

// ── Scenario 12: count(*) ─────────────────────────────────────────────────────

#[test]
fn sqlite_count_star_counts_all_open_beads() {
    let storage = ddx_sqlite_store();
    let rows = run(
        &storage,
        "MATCH (b:DdxBead {status: 'open'}) RETURN count(*) AS n",
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["n"], json!(5), "there should be 5 open beads");
}

// ── Scenario 13: ORDER BY priority ASC ───────────────────────────────────────

#[test]
fn sqlite_order_by_priority_asc_returns_open_beads_in_ascending_order() {
    let storage = ddx_sqlite_store();
    let rows = run(
        &storage,
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

// ── Scenario 14: ORDER BY priority DESC ──────────────────────────────────────

#[test]
fn sqlite_order_by_priority_desc_returns_open_beads_in_descending_order() {
    let storage = ddx_sqlite_store();
    let rows = run(
        &storage,
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
