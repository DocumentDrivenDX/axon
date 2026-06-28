//! FEAT-008 TXN-05 — serializable auto-capture.
//!
//! Proves the serializable guards work using ONLY the transaction-aware read
//! methods (`tx_get_entity`, `tx_query_entities`) — no manual `record_read` /
//! `record_scan_read`. Reading through the transaction is the recording.

#![allow(clippy::unwrap_used)]

use axon_api::handler::AxonHandler;
use axon_api::request::{
    AggregateFunction, AggregateRequest, CreateEntityRequest, CreateLinkRequest, GetEntityRequest,
    QueryEntitiesRequest, TraverseRequest,
};
use axon_api::transaction::{IsolationLevel, Transaction};
use axon_core::id::{CollectionId, EntityId};
use axon_core::types::Entity;
use axon_storage::memory::MemoryStorageAdapter;
use serde_json::json;

fn guards() -> CollectionId {
    CollectionId::new("guards")
}

fn create(handler: &mut AxonHandler<MemoryStorageAdapter>, id: &str, flag: i64) {
    handler
        .create_entity(CreateEntityRequest {
            collection: guards(),
            id: EntityId::new(id),
            data: json!({ "flag": flag }),
            actor: Some("seed".into()),
            audit_metadata: None,
            attribution: None,
        })
        .expect("seed create");
}

fn get_req(id: &str) -> GetEntityRequest {
    GetEntityRequest {
        collection: guards(),
        id: EntityId::new(id),
    }
}

fn flag0(id: &str) -> Entity {
    Entity::new(guards(), EntityId::new(id), json!({ "flag": 0 }))
}

/// Classic write skew — invariant "X.flag + Y.flag >= 1". Each transaction
/// reads the OTHER guard via `tx_get_entity` (auto-recording the key-addressed
/// read) and clears its own. Under Serializable the second committer aborts;
/// no `record_read` is ever called by hand.
#[test]
fn write_skew_prevented_via_auto_capture_only() {
    let mut h = AxonHandler::new(MemoryStorageAdapter::default());
    create(&mut h, "X", 1);
    create(&mut h, "Y", 1);

    let mut t1 = Transaction::with_isolation(IsolationLevel::Serializable);
    let mut t2 = Transaction::with_isolation(IsolationLevel::Serializable);

    // Both read the other guard BEFORE either commits (auto-captured).
    h.tx_get_entity(&mut t1, get_req("Y")).expect("t1 reads Y");
    h.tx_get_entity(&mut t2, get_req("X")).expect("t2 reads X");

    t1.update(flag0("X"), 1, None).expect("stage X");
    t2.update(flag0("Y"), 1, None).expect("stage Y");

    h.commit_transaction(t1, Some("t1".into()), None)
        .expect("T1 commits");
    let err = h
        .commit_transaction(t2, Some("t2".into()), None)
        .expect_err("T2 must abort: its auto-captured read of X is stale");
    assert!(
        matches!(err, axon_core::error::AxonError::ConflictingVersion { .. }),
        "expected serialization conflict, got: {err}"
    );

    // Invariant held: Y was never cleared.
    let y = h.get_entity(get_req("Y")).expect("get Y").entity;
    assert_eq!(y.data["flag"], 1, "Y must be untouched after T2 aborts");
}

/// Same interleaving under the default Snapshot isolation: auto-capture is a
/// no-op, both commit, and the invariant is violated — proving the recording
/// is free and inert unless Serializable is selected.
#[test]
fn write_skew_allowed_under_snapshot_via_auto_capture() {
    let mut h = AxonHandler::new(MemoryStorageAdapter::default());
    create(&mut h, "X", 1);
    create(&mut h, "Y", 1);

    let mut t1 = Transaction::new(); // Snapshot
    let mut t2 = Transaction::new();
    h.tx_get_entity(&mut t1, get_req("Y")).expect("t1 reads Y");
    h.tx_get_entity(&mut t2, get_req("X")).expect("t2 reads X");
    t1.update(flag0("X"), 1, None).expect("stage X");
    t2.update(flag0("Y"), 1, None).expect("stage Y");

    h.commit_transaction(t1, Some("t1".into()), None)
        .expect("T1");
    h.commit_transaction(t2, Some("t2".into()), None)
        .expect("snapshot allows write skew");

    let x = h.get_entity(get_req("X")).expect("get X").entity;
    let y = h.get_entity(get_req("Y")).expect("get Y").entity;
    assert_eq!(x.data["flag"], 0);
    assert_eq!(
        y.data["flag"], 0,
        "invariant violated under snapshot (documented)"
    );
}

/// Phantom write skew via `tx_query_entities`: each transaction scans the
/// collection (auto-recording the structural version) then inserts a new row.
/// Under Serializable the second committer aborts because the scanned
/// collection's membership changed. No `record_scan_read` by hand.
#[test]
fn phantom_prevented_via_auto_capture_query() {
    let mut h = AxonHandler::new(MemoryStorageAdapter::default());
    create(&mut h, "a", 1);

    let query = || QueryEntitiesRequest {
        collection: guards(),
        filter: None,
        sort: Vec::new(),
        limit: None,
        after_id: None,
        count_only: false,
    };

    let mut t1 = Transaction::with_isolation(IsolationLevel::Serializable);
    let mut t2 = Transaction::with_isolation(IsolationLevel::Serializable);

    h.tx_query_entities(&mut t1, query()).expect("t1 scans");
    h.tx_query_entities(&mut t2, query()).expect("t2 scans");

    t1.create(Entity::new(
        guards(),
        EntityId::new("b"),
        json!({ "flag": 1 }),
    ))
    .expect("stage b");
    t2.create(Entity::new(
        guards(),
        EntityId::new("c"),
        json!({ "flag": 1 }),
    ))
    .expect("stage c");

    h.commit_transaction(t1, Some("t1".into()), None)
        .expect("T1 commits (membership changes)");
    let err = h
        .commit_transaction(t2, Some("t2".into()), None)
        .expect_err("T2 must abort: scanned collection's membership changed");
    assert!(
        matches!(err, axon_core::error::AxonError::ConflictingVersion { .. }),
        "expected phantom serialization conflict, got: {err}"
    );
}

/// Phantom that shifts an aggregate, via `tx_aggregate`: each transaction sums a
/// field over the collection (auto-recording the structural version) then
/// inserts a new matching row. Under Serializable the second committer aborts
/// because the aggregated collection's membership changed.
#[test]
fn aggregate_phantom_prevented_via_auto_capture() {
    let mut h = AxonHandler::new(MemoryStorageAdapter::default());
    create(&mut h, "a", 5);
    create(&mut h, "b", 5);

    let agg = || AggregateRequest {
        collection: guards(),
        function: AggregateFunction::Sum,
        field: "flag".into(),
        filter: None,
        group_by: None,
    };

    let mut t1 = Transaction::with_isolation(IsolationLevel::Serializable);
    let mut t2 = Transaction::with_isolation(IsolationLevel::Serializable);
    h.tx_aggregate(&mut t1, agg()).expect("t1 aggregates");
    h.tx_aggregate(&mut t2, agg()).expect("t2 aggregates");

    t1.create(Entity::new(
        guards(),
        EntityId::new("c"),
        json!({ "flag": 5 }),
    ))
    .expect("stage c");
    t2.create(Entity::new(
        guards(),
        EntityId::new("d"),
        json!({ "flag": 5 }),
    ))
    .expect("stage d");

    h.commit_transaction(t1, Some("t1".into()), None)
        .expect("T1 commits (membership changes)");
    let err = h
        .commit_transaction(t2, Some("t2".into()), None)
        .expect_err("T2 must abort: aggregated collection's membership changed");
    assert!(
        matches!(err, axon_core::error::AxonError::ConflictingVersion { .. }),
        "expected phantom serialization conflict, got: {err}"
    );
}

/// A concurrent change to a traversed member aborts a Serializable transaction,
/// via `tx_traverse` auto-capture (each returned entity is recorded as a
/// key-addressed read). No manual `record_read`.
#[test]
fn traverse_member_change_prevented_via_auto_capture() {
    let mut h = AxonHandler::new(MemoryStorageAdapter::default());
    create(&mut h, "A", 1); // base
    create(&mut h, "B", 1); // traversed target
    h.create_link(CreateLinkRequest {
        source_collection: guards(),
        source_id: EntityId::new("A"),
        target_collection: guards(),
        target_id: EntityId::new("B"),
        link_type: "rel".into(),
        metadata: json!(null),
        actor: Some("seed".into()),
        attribution: None,
    })
    .expect("seed link");

    let trav = || TraverseRequest {
        collection: guards(),
        id: EntityId::new("A"),
        link_type: Some("rel".into()),
        max_depth: Some(2),
        direction: Default::default(),
        hop_filter: None,
    };

    let mut t1 = Transaction::with_isolation(IsolationLevel::Serializable);
    let mut t2 = Transaction::with_isolation(IsolationLevel::Serializable);
    let r1 = h.tx_traverse(&mut t1, trav()).expect("t1 traverses");
    assert!(
        r1.entities.iter().any(|e| e.id.as_str() == "B"),
        "traversal should reach B"
    );
    h.tx_traverse(&mut t2, trav()).expect("t2 traverses"); // both record B@v1

    // T1 mutates the traversed member B (it recorded B@v1, so its own commit
    // validates before applying — no self-abort).
    t1.update(
        Entity::new(guards(), EntityId::new("B"), json!({ "flag": 2 })),
        1,
        None,
    )
    .expect("stage B update");
    h.commit_transaction(t1, Some("t1".into()), None)
        .expect("T1 commits (B -> v2)");

    // T2 stages an unrelated write; its auto-captured read of B is now stale.
    t2.update(
        Entity::new(guards(), EntityId::new("A"), json!({ "flag": 9 })),
        1,
        None,
    )
    .expect("stage A update");
    let err = h
        .commit_transaction(t2, Some("t2".into()), None)
        .expect_err("T2 must abort: a traversed member changed");
    assert!(
        matches!(err, axon_core::error::AxonError::ConflictingVersion { .. }),
        "expected serialization conflict on the traversed member, got: {err}"
    );
}

/// Cypher read footprint via `tx_record_cypher_scan`: a serializable txn records
/// the collections a Cypher query reads; a concurrent insert into a referenced
/// collection aborts the second committer. No manual `record_scan_read`.
#[test]
fn cypher_phantom_prevented_via_auto_capture() {
    let mut h = AxonHandler::new(MemoryStorageAdapter::default());
    create(&mut h, "a", 1);

    let cypher = "MATCH (g:guards) RETURN g";
    let mut t1 = Transaction::with_isolation(IsolationLevel::Serializable);
    let mut t2 = Transaction::with_isolation(IsolationLevel::Serializable);
    h.tx_record_cypher_scan(&mut t1, cypher)
        .expect("t1 records cypher footprint");
    h.tx_record_cypher_scan(&mut t2, cypher)
        .expect("t2 records cypher footprint");

    t1.create(Entity::new(
        guards(),
        EntityId::new("b"),
        json!({ "flag": 1 }),
    ))
    .expect("stage b");
    t2.create(Entity::new(
        guards(),
        EntityId::new("c"),
        json!({ "flag": 1 }),
    ))
    .expect("stage c");

    h.commit_transaction(t1, Some("t1".into()), None)
        .expect("T1 commits (guards membership changes)");
    let err = h
        .commit_transaction(t2, Some("t2".into()), None)
        .expect_err("T2 must abort: a Cypher-referenced collection changed");
    assert!(
        matches!(err, axon_core::error::AxonError::ConflictingVersion { .. }),
        "expected phantom serialization conflict, got: {err}"
    );
}

/// Under Snapshot, the same Cypher footprint recording is inert — both commit.
#[test]
fn cypher_footprint_is_noop_under_snapshot() {
    let mut h = AxonHandler::new(MemoryStorageAdapter::default());
    create(&mut h, "a", 1);
    let cypher = "MATCH (g:guards) RETURN g";

    let mut t1 = Transaction::new();
    let mut t2 = Transaction::new();
    h.tx_record_cypher_scan(&mut t1, cypher).expect("t1");
    h.tx_record_cypher_scan(&mut t2, cypher).expect("t2");
    t1.create(Entity::new(
        guards(),
        EntityId::new("b"),
        json!({ "flag": 1 }),
    ))
    .expect("stage b");
    t2.create(Entity::new(
        guards(),
        EntityId::new("c"),
        json!({ "flag": 1 }),
    ))
    .expect("stage c");
    h.commit_transaction(t1, Some("t1".into()), None)
        .expect("T1");
    h.commit_transaction(t2, Some("t2".into()), None)
        .expect("snapshot ignores the recorded footprint");
}

/// Invalid Cypher surfaces as a rejectable error, not a panic.
#[test]
fn cypher_scan_rejects_invalid_query() {
    let h = AxonHandler::new(MemoryStorageAdapter::default());
    let mut tx = Transaction::with_isolation(IsolationLevel::Serializable);
    let err = h
        .tx_record_cypher_scan(&mut tx, "this is not cypher")
        .expect_err("invalid cypher must error");
    assert!(matches!(
        err,
        axon_core::error::AxonError::InvalidArgument(_)
    ));
}
