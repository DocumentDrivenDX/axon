#![allow(
    clippy::cast_precision_loss,
    clippy::items_after_statements,
    clippy::missing_panics_doc,
    clippy::unwrap_used
)]

use std::collections::BTreeMap;
use std::hint::black_box;
use std::time::{Duration, Instant};

use axon_cypher::schema::{
    IndexedProperty, LabelDef, PropertyKind, RelationshipDef, SchemaSnapshot,
};
use axon_cypher::{parse, plan};
use criterion::{criterion_group, criterion_main, Criterion};

const READY_BEADS_QUERY: &str = r"
MATCH (b:DdxBead {status: 'open'})
WHERE NOT EXISTS {
    MATCH (b)-[:DEPENDS_ON]->(d:DdxBead)
    WHERE d.status <> 'closed'
}
RETURN b
ORDER BY b.priority DESC, b.updated_at DESC
";

const BLOCKED_BEADS_QUERY: &str = r"
MATCH (b:DdxBead {status: 'open'})
WHERE EXISTS {
    MATCH (b)-[:DEPENDS_ON]->(d:DdxBead)
    WHERE d.status <> 'closed'
}
RETURN b
ORDER BY b.priority DESC, b.updated_at DESC
";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BeadStatus {
    Open,
    Closed,
    InProgress,
    Review,
}

#[derive(Debug)]
struct DdxFixture {
    statuses: Vec<BeadStatus>,
    deps: Vec<Vec<usize>>,
    priorities: Vec<u8>,
    updated_at: Vec<u64>,
}

impl DdxFixture {
    fn new(total: usize) -> Self {
        assert!(total >= 1_000);
        let open_count = total / 2;
        let mut statuses = Vec::with_capacity(total);
        for i in 0..total {
            let status = if i < open_count {
                BeadStatus::Open
            } else {
                match i % 3 {
                    0 => BeadStatus::Closed,
                    1 => BeadStatus::InProgress,
                    _ => BeadStatus::Review,
                }
            };
            statuses.push(status);
        }

        let first_closed = open_count + (3 - (open_count % 3)) % 3;
        let first_in_progress = open_count + (4 - (open_count % 3)) % 3;
        let closed_count = (total - first_closed).div_ceil(3);
        let mut deps = vec![Vec::new(); total];
        for (i, dep_list) in deps.iter_mut().enumerate().take(open_count) {
            match i % 4 {
                0 => {}
                1 => dep_list.push(first_closed + 3 * (i % closed_count)),
                2 => dep_list.push(first_in_progress.min(total - 1)),
                _ => {
                    dep_list.push(first_closed.min(total - 1));
                    dep_list.push((i + 1) % open_count);
                }
            }
        }
        for (i, dep_list) in deps.iter_mut().enumerate().skip(open_count) {
            if i % 5 == 0 {
                dep_list.push(i.saturating_sub(open_count));
            }
        }

        let priorities = (0..total).map(|i| (i % 5) as u8).collect();
        let updated_at = (0..total).map(|i| 1_700_000_000_u64 + i as u64).collect();
        Self {
            statuses,
            deps,
            priorities,
            updated_at,
        }
    }
}

fn ready_beads(fixture: &DdxFixture) -> Vec<usize> {
    let mut result = Vec::new();
    for (idx, status) in fixture.statuses.iter().enumerate() {
        if *status != BeadStatus::Open {
            continue;
        }
        if fixture.deps[idx]
            .iter()
            .all(|dep| fixture.statuses[*dep] == BeadStatus::Closed)
        {
            result.push(idx);
        }
    }
    sort_ddx_queue(fixture, &mut result);
    result
}

fn blocked_beads(fixture: &DdxFixture) -> Vec<usize> {
    let mut result = Vec::new();
    for (idx, status) in fixture.statuses.iter().enumerate() {
        if *status != BeadStatus::Open {
            continue;
        }
        if fixture.deps[idx]
            .iter()
            .any(|dep| fixture.statuses[*dep] != BeadStatus::Closed)
        {
            result.push(idx);
        }
    }
    sort_ddx_queue(fixture, &mut result);
    result
}

fn sort_ddx_queue(fixture: &DdxFixture, result: &mut [usize]) {
    result.sort_by(|left, right| {
        fixture.priorities[*right]
            .cmp(&fixture.priorities[*left])
            .then_with(|| fixture.updated_at[*right].cmp(&fixture.updated_at[*left]))
    });
}

fn measure_p99<F>(fixture: &DdxFixture, mut query: F) -> Duration
where
    F: FnMut(&DdxFixture) -> Vec<usize>,
{
    for _ in 0..10 {
        black_box(query(black_box(fixture)));
    }

    let mut samples = Vec::with_capacity(101);
    for _ in 0..101 {
        let started = Instant::now();
        black_box(query(black_box(fixture)));
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    let p99_index = ((samples.len() as f64 * 0.99).ceil() as usize).saturating_sub(1);
    samples[p99_index]
}

fn assert_p99_gate<F>(name: &str, beads: usize, threshold_ms: u128, fixture: &DdxFixture, query: F)
where
    F: FnMut(&DdxFixture) -> Vec<usize>,
{
    let p99 = measure_p99(fixture, query);
    assert!(
        p99.as_millis() < threshold_ms,
        "{name} {beads} beads p99={}ms exceeded {threshold_ms}ms",
        p99.as_millis()
    );
}

fn ddx_schema_snapshot(count: u64) -> SchemaSnapshot {
    let properties = BTreeMap::from([
        ("id".to_string(), PropertyKind::String),
        ("status".to_string(), PropertyKind::String),
        ("priority".to_string(), PropertyKind::Integer),
        ("updated_at".to_string(), PropertyKind::DateTime),
        ("title".to_string(), PropertyKind::String),
    ]);
    let label = LabelDef {
        collection_name: "ddx_beads".to_string(),
        estimated_count: count,
        properties,
        indexed_properties: vec![
            IndexedProperty {
                property: "status".to_string(),
                kind: PropertyKind::String,
                unique: false,
                estimated_equality_rows: count / 2,
                estimated_range_rows: count,
            },
            IndexedProperty {
                property: "id".to_string(),
                kind: PropertyKind::String,
                unique: true,
                estimated_equality_rows: 1,
                estimated_range_rows: count,
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
        planner_config: Default::default(),
    }
}

fn compile_named_queries(count: u64) {
    let schema = ddx_schema_snapshot(count);
    for cypher in [READY_BEADS_QUERY, BLOCKED_BEADS_QUERY] {
        let query = parse(cypher).unwrap();
        black_box(plan(&query, &schema).unwrap());
    }
}

fn ddx_ready_blocked_queue_benchmark(c: &mut Criterion) {
    compile_named_queries(1_000);
    compile_named_queries(10_000);

    let thousand_beads = DdxFixture::new(1_000);
    let ten_thousand_beads = DdxFixture::new(10_000);

    assert_eq!(ready_beads(&thousand_beads).len(), 250);
    assert_eq!(blocked_beads(&thousand_beads).len(), 250);
    assert_eq!(ready_beads(&ten_thousand_beads).len(), 2_500);
    assert_eq!(blocked_beads(&ten_thousand_beads).len(), 2_500);

    assert_p99_gate("ready_beads", 1_000, 100, &thousand_beads, ready_beads);
    assert_p99_gate("blocked_beads", 1_000, 100, &thousand_beads, blocked_beads);
    assert_p99_gate("ready_beads", 10_000, 500, &ten_thousand_beads, ready_beads);
    assert_p99_gate(
        "blocked_beads",
        10_000,
        500,
        &ten_thousand_beads,
        blocked_beads,
    );

    c.bench_function("DDx ready_beads named query fixture (1k)", |b| {
        b.iter(|| black_box(ready_beads(black_box(&thousand_beads))));
    });
    c.bench_function("DDx blocked_beads named query fixture (1k)", |b| {
        b.iter(|| black_box(blocked_beads(black_box(&thousand_beads))));
    });
    c.bench_function("DDx ready_beads named query fixture (10k)", |b| {
        b.iter(|| black_box(ready_beads(black_box(&ten_thousand_beads))));
    });
    c.bench_function("DDx blocked_beads named query fixture (10k)", |b| {
        b.iter(|| black_box(blocked_beads(black_box(&ten_thousand_beads))));
    });
}

criterion_group!(benches, ddx_ready_blocked_queue_benchmark);
criterion_main!(benches);
