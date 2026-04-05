//! L5 Performance Benchmarks (BM-001 through BM-010)
//!
//! All benchmarks use criterion and run against the in-memory storage backend.
//! Targets from the test plan (p99 latency):
//!   BM-001: Single entity read        < 5 ms
//!   BM-002: Single entity write       < 10 ms
//!   BM-003: Multi-entity transaction  < 20 ms
//!   BM-004: Collection scan           < 100 ms
//!   BM-005: Audit log append overhead < 2 ms
//!   BM-006: Link traversal (3 hops)   < 50 ms
//!   BM-007: Aggregation (10K)         < 500 ms
//!   BM-008: Concurrent writers (100)  — linear scaling
//!   BM-009: Schema validation         < 1 ms
//!   BM-010: Audit query               < 100 ms

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use serde_json::json;

use axon_api::handler::AxonHandler;
use axon_api::request::*;
use axon_api::transaction::Transaction;
use axon_audit::log::MemoryAuditLog;
use axon_core::id::{CollectionId, EntityId};
use axon_core::types::Entity;
use axon_schema::schema::CollectionSchema;
use axon_schema::EsfDocument;
use axon_storage::adapter::StorageAdapter;
use axon_storage::memory::MemoryStorageAdapter;

fn col(name: &str) -> CollectionId {
    CollectionId::new(name)
}

fn eid(id: &str) -> EntityId {
    EntityId::new(id)
}

// ── BM-001: Single entity read ──────────────────────────────────────────────

fn bm_001_single_entity_read(c: &mut Criterion) {
    let mut h = AxonHandler::new(MemoryStorageAdapter::default());

    // Seed entities for random lookups.
    for i in 0..10_000 {
        h.create_entity(CreateEntityRequest {
            collection: col("items"),
            id: eid(&format!("item-{i:05}")),
            data: json!({"name": format!("Item {i}"), "value": i}),
            actor: None,
        })
        .unwrap();
    }

    let mut i = 0_u64;
    c.bench_function("BM-001: single entity read", |b| {
        b.iter(|| {
            let id = format!("item-{:05}", i % 10_000);
            i += 1;
            black_box(
                h.get_entity(GetEntityRequest {
                    collection: col("items"),
                    id: eid(&id),
                })
                .unwrap(),
            );
        })
    });
}

// ── BM-002: Single entity write ─────────────────────────────────────────────

fn bm_002_single_entity_write(c: &mut Criterion) {
    let mut h = AxonHandler::new(MemoryStorageAdapter::default());

    // Register a schema for validation overhead.
    let esf = r#"
esf_version: "1.0"
collection: items
entity_schema:
  type: object
  required: [name, value]
  properties:
    name:
      type: string
    value:
      type: number
"#;
    let schema = EsfDocument::parse(esf)
        .unwrap()
        .into_collection_schema()
        .unwrap();
    h.register_schema(schema);

    let mut i = 0_u64;
    c.bench_function("BM-002: single entity write", |b| {
        b.iter(|| {
            let id = format!("w-{i:08}");
            i += 1;
            black_box(
                h.create_entity(CreateEntityRequest {
                    collection: col("items"),
                    id: eid(&id),
                    data: json!({"name": format!("Item {i}"), "value": i}),
                    actor: Some("bench".into()),
                })
                .unwrap(),
            );
        })
    });
}

// ── BM-003: Multi-entity transaction (5 ops) ───────────────────────────────

fn bm_003_multi_entity_transaction(c: &mut Criterion) {
    let mut storage = MemoryStorageAdapter::default();
    let mut audit = MemoryAuditLog::default();

    // Seed 1000 accounts.
    for i in 0..1000 {
        storage
            .put(Entity::new(
                col("accounts"),
                eid(&format!("acct-{i:04}")),
                json!({"balance": 10000}),
            ))
            .unwrap();
    }

    let mut round = 0_u64;
    c.bench_function("BM-003: multi-entity transaction (5 ops)", |b| {
        b.iter(|| {
            let base = (round * 5) % 1000;
            round += 1;

            let mut tx = Transaction::new();
            for j in 0..5 {
                let idx = (base + j) % 1000;
                let id = format!("acct-{idx:04}");
                let current = storage.get(&col("accounts"), &eid(&id)).unwrap().unwrap();
                tx.update(
                    Entity::new(
                        col("accounts"),
                        eid(&id),
                        json!({"balance": current.data["balance"].as_i64().unwrap() - 1}),
                    ),
                    current.version,
                    Some(current.data),
                );
            }
            black_box(
                tx.commit(&mut storage, &mut audit, Some("bench".into()))
                    .unwrap(),
            );
        })
    });
}

// ── BM-004: Collection scan (1,000 entities) ────────────────────────────────

fn bm_004_collection_scan(c: &mut Criterion) {
    let mut h = AxonHandler::new(MemoryStorageAdapter::default());

    for i in 0..1_000 {
        h.create_entity(CreateEntityRequest {
            collection: col("invoices"),
            id: eid(&format!("inv-{i:04}")),
            data: json!({"amount": i * 100, "status": if i % 3 == 0 { "paid" } else { "open" }}),
            actor: None,
        })
        .unwrap();
    }

    c.bench_function("BM-004: collection scan (1K entities, filter+sort)", |b| {
        b.iter(|| {
            black_box(
                h.query_entities(QueryEntitiesRequest {
                    collection: col("invoices"),
                    filter: Some(FilterNode::Field(FieldFilter {
                        field: "status".into(),
                        op: FilterOp::Eq,
                        value: json!("open"),
                    })),
                    sort: vec![SortField {
                        field: "amount".into(),
                        direction: SortDirection::Desc,
                    }],
                    limit: Some(50),
                    ..Default::default()
                })
                .unwrap(),
            );
        })
    });
}

// ── BM-005: Audit log append overhead ───────────────────────────────────────

fn bm_005_audit_append_overhead(c: &mut Criterion) {
    // Measure write-with-audit vs write-without-audit.
    // We use the handler (which always audits) as the "with audit" path.
    let mut h = AxonHandler::new(MemoryStorageAdapter::default());

    let mut i = 0_u64;
    c.bench_function("BM-005: entity write with audit", |b| {
        b.iter(|| {
            let id = format!("a-{i:08}");
            i += 1;
            black_box(
                h.create_entity(CreateEntityRequest {
                    collection: col("things"),
                    id: eid(&id),
                    data: json!({"v": i}),
                    actor: Some("bench".into()),
                })
                .unwrap(),
            );
        })
    });
}

// ── BM-006: Link traversal (3 hops) ────────────────────────────────────────

fn bm_006_link_traversal(c: &mut Criterion) {
    let mut h = AxonHandler::new(MemoryStorageAdapter::default());

    // Build a tree: root → 5 children → 5 grandchildren each → 5 great-grandchildren each
    h.create_entity(CreateEntityRequest {
        collection: col("nodes"),
        id: eid("root"),
        data: json!({"level": 0}),
        actor: None,
    })
    .unwrap();

    for i in 0..5 {
        let child = format!("c-{i}");
        h.create_entity(CreateEntityRequest {
            collection: col("nodes"),
            id: eid(&child),
            data: json!({"level": 1}),
            actor: None,
        })
        .unwrap();
        h.create_link(CreateLinkRequest {
            source_collection: col("nodes"),
            source_id: eid("root"),
            target_collection: col("nodes"),
            target_id: eid(&child),
            link_type: "contains".into(),
            metadata: json!(null),
            actor: None,
        })
        .unwrap();

        for j in 0..5 {
            let grandchild = format!("gc-{i}-{j}");
            h.create_entity(CreateEntityRequest {
                collection: col("nodes"),
                id: eid(&grandchild),
                data: json!({"level": 2}),
                actor: None,
            })
            .unwrap();
            h.create_link(CreateLinkRequest {
                source_collection: col("nodes"),
                source_id: eid(&child),
                target_collection: col("nodes"),
                target_id: eid(&grandchild),
                link_type: "contains".into(),
                metadata: json!(null),
                actor: None,
            })
            .unwrap();

            for k in 0..5 {
                let ggchild = format!("ggc-{i}-{j}-{k}");
                h.create_entity(CreateEntityRequest {
                    collection: col("nodes"),
                    id: eid(&ggchild),
                    data: json!({"level": 3}),
                    actor: None,
                })
                .unwrap();
                h.create_link(CreateLinkRequest {
                    source_collection: col("nodes"),
                    source_id: eid(&grandchild),
                    target_collection: col("nodes"),
                    target_id: eid(&ggchild),
                    link_type: "contains".into(),
                    metadata: json!(null),
                    actor: None,
                })
                .unwrap();
            }
        }
    }

    c.bench_function("BM-006: link traversal (3 hops, 155 nodes)", |b| {
        b.iter(|| {
            black_box(
                h.traverse(TraverseRequest {
                    collection: col("nodes"),
                    id: eid("root"),
                    link_type: Some("contains".into()),
                    max_depth: Some(3),
                })
                .unwrap(),
            );
        })
    });
}

// ── BM-007: Aggregation (10K entities) ──────────────────────────────────────

fn bm_007_aggregation(c: &mut Criterion) {
    let mut h = AxonHandler::new(MemoryStorageAdapter::default());

    let statuses = ["draft", "submitted", "approved", "paid"];
    let categories = ["A", "B", "C"];
    for i in 0..10_000 {
        h.create_entity(CreateEntityRequest {
            collection: col("invoices"),
            id: eid(&format!("inv-{i:05}")),
            data: json!({
                "amount": (i % 1000) * 10,
                "status": statuses[i % 4],
                "category": categories[i % 3]
            }),
            actor: None,
        })
        .unwrap();
    }

    c.bench_function("BM-007: aggregation (10K entities, filter+sum)", |b| {
        b.iter(|| {
            let results = h
                .query_entities(QueryEntitiesRequest {
                    collection: col("invoices"),
                    filter: Some(FilterNode::Field(FieldFilter {
                        field: "status".into(),
                        op: FilterOp::Eq,
                        value: json!("approved"),
                    })),
                    ..Default::default()
                })
                .unwrap();
            // Simulate aggregation: sum amounts.
            let total: i64 = results
                .entities
                .iter()
                .map(|e| e.data["amount"].as_i64().unwrap_or(0))
                .sum();
            black_box(total);
        })
    });
}

// ── BM-008: Concurrent writers (simulated) ──────────────────────────────────

fn bm_008_concurrent_writers(c: &mut Criterion) {
    // Simulate 100 agents writing to different entities sequentially
    // (true concurrency would need threads, but this measures per-write throughput).
    let mut storage = MemoryStorageAdapter::default();
    let mut audit = MemoryAuditLog::default();

    for i in 0..100 {
        storage
            .put(Entity::new(
                col("tasks"),
                eid(&format!("task-{i:03}")),
                json!({"status": "ready", "agent": null}),
            ))
            .unwrap();
    }

    let mut round = 0_u64;
    c.bench_function("BM-008: 100 sequential writer claims", |b| {
        b.iter(|| {
            for i in 0..100 {
                let id = format!("task-{i:03}");
                let current = storage.get(&col("tasks"), &eid(&id)).unwrap().unwrap();
                let mut tx = Transaction::new();
                tx.update(
                    Entity::new(
                        col("tasks"),
                        eid(&id),
                        json!({"status": "in_progress", "agent": format!("agent-{}", round)}),
                    ),
                    current.version,
                    Some(current.data),
                );
                tx.commit(&mut storage, &mut audit, Some(format!("agent-{round}")))
                    .unwrap();
            }
            round += 1;
        })
    });
}

// ── BM-009: Schema validation ───────────────────────────────────────────────

fn bm_009_schema_validation(c: &mut Criterion) {
    let esf = r#"
esf_version: "1.0"
collection: complex
entity_schema:
  type: object
  required: [name, email, age, address, status]
  properties:
    name:
      type: string
    email:
      type: string
    age:
      type: integer
      minimum: 0
      maximum: 150
    address:
      type: object
      required: [street, city, country]
      properties:
        street:
          type: string
        city:
          type: string
        state:
          type: string
        country:
          type: string
        zip:
          type: string
    status:
      type: string
      enum: [active, inactive, pending, suspended]
    tags:
      type: array
      items:
        type: string
    metadata:
      type: object
      properties:
        created_at:
          type: string
        source:
          type: string
        priority:
          type: integer
    notes:
      type: string
    score:
      type: number
      minimum: 0
      maximum: 100
"#;

    let schema = EsfDocument::parse(esf)
        .unwrap()
        .into_collection_schema()
        .unwrap();

    let valid_entity = json!({
        "name": "Jane Doe",
        "email": "jane@example.com",
        "age": 32,
        "address": {
            "street": "123 Main St",
            "city": "Springfield",
            "state": "IL",
            "country": "US",
            "zip": "62701"
        },
        "status": "active",
        "tags": ["vip", "enterprise"],
        "metadata": {
            "created_at": "2026-01-01",
            "source": "web",
            "priority": 1
        },
        "notes": "Important customer",
        "score": 95.5
    });

    c.bench_function("BM-009: schema validation (20 fields, 2 levels)", |b| {
        b.iter(|| {
            black_box(axon_schema::validate(&schema, &valid_entity).unwrap());
        })
    });
}

// ── BM-010: Audit query (single entity, 100 mutations) ─────────────────────

fn bm_010_audit_query(c: &mut Criterion) {
    let mut h = AxonHandler::new(MemoryStorageAdapter::default());

    // Create entity and mutate it 100 times.
    h.create_entity(CreateEntityRequest {
        collection: col("logs"),
        id: eid("e-001"),
        data: json!({"counter": 0}),
        actor: Some("setup".into()),
    })
    .unwrap();

    for i in 1..=100 {
        h.update_entity(UpdateEntityRequest {
            collection: col("logs"),
            id: eid("e-001"),
            data: json!({"counter": i}),
            expected_version: i as u64,
            actor: Some(format!("agent-{i}")),
        })
        .unwrap();
    }

    c.bench_function("BM-010: audit query (single entity, 100 mutations)", |b| {
        b.iter(|| {
            black_box(
                h.query_audit(QueryAuditRequest {
                    collection: Some(col("logs")),
                    entity_id: Some(eid("e-001")),
                    ..Default::default()
                })
                .unwrap(),
            );
        })
    });
}

criterion_group!(
    benches,
    bm_001_single_entity_read,
    bm_002_single_entity_write,
    bm_003_multi_entity_transaction,
    bm_004_collection_scan,
    bm_005_audit_append_overhead,
    bm_006_link_traversal,
    bm_007_aggregation,
    bm_008_concurrent_writers,
    bm_009_schema_validation,
    bm_010_audit_query,
);
criterion_main!(benches);
