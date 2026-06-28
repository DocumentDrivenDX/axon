//! FEAT-008 TXN-05 — serializable auto-capture.
//!
//! Proves the serializable guards work using ONLY the transaction-aware read
//! methods (`tx_get_entity`, `tx_query_entities`) — no manual `record_read` /
//! `record_scan_read`. Reading through the transaction is the recording.

#![allow(clippy::unwrap_used)]

use axon_api::handler::AxonHandler;
use axon_api::request::{CreateEntityRequest, GetEntityRequest, QueryEntitiesRequest};
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
