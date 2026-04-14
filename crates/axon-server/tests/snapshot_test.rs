//! L6 HTTP contract tests for the atomic snapshot-with-cursor endpoint
//! (FEAT-004 / US-080).
//!
//! Covers:
//! 1. Happy path: entities across multiple collections are returned with an
//!    `audit_cursor` greater than or equal to the number of creates.
//! 2. Collections filter narrows the result set.
//! 3. Race-free cursor: post-snapshot mutations appear in the audit tail but
//!    not in the snapshot's result set; the audit tail filtered by `after_id`
//!    excludes the entries that were in-scope for the snapshot.
//! 4. Empty database returns `entities: []` and `audit_cursor: 0`.
//! 5. Pagination: `limit: 2` over 5 entities returns 2 + token, 2 + token,
//!    1 + `next_page_token: None`.
//!
//! These tests drive the HTTP gateway end-to-end via `axum_test::TestServer`,
//! exercising `POST /snapshot` (the un-prefixed route, which the server
//! registers at both `/snapshot` and `/db/{database}/snapshot`).

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use axon_api::handler::AxonHandler;
use axon_server::gateway::build_router;
use axon_server::tenant_router::TenantRouter;
use axon_storage::adapter::StorageAdapter;
use axon_storage::SqliteStorageAdapter;
use serde_json::{json, Value};
use tokio::sync::Mutex;

fn make_server() -> axum_test::TestServer {
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(handler));
    let app = build_router(tenant_router, "memory", None);
    axum_test::TestServer::new(app)
}

async fn create_collection(server: &axum_test::TestServer, name: &str) {
    let resp = server
        .post(&format!("/collections/{name}"))
        .json(&json!({
            "schema": {
                "collection": name,
                "version": 1,
            }
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::CREATED);
}

async fn create_entity(server: &axum_test::TestServer, collection: &str, id: &str, data: Value) {
    let resp = server
        .post(&format!("/entities/{collection}/{id}"))
        .json(&json!({"data": data}))
        .await;
    resp.assert_status(axum::http::StatusCode::CREATED);
}

/// Acceptance test 1 (happy path): 5 entities across 2 collections are all
/// returned and `audit_cursor` is at least 5 (MemoryAuditLog IDs are
/// monotonic so the cursor is >= the total number of audit entries so far).
#[tokio::test]
async fn snapshot_happy_path_returns_all_entities_and_cursor() {
    let http = make_server();

    // 3 in "tasks", 2 in "notes".
    create_collection(&http, "tasks").await;
    create_collection(&http, "notes").await;
    create_entity(&http, "tasks", "t-001", json!({"title": "task one"})).await;
    create_entity(&http, "tasks", "t-002", json!({"title": "task two"})).await;
    create_entity(&http, "tasks", "t-003", json!({"title": "task three"})).await;
    create_entity(&http, "notes", "n-001", json!({"text": "note one"})).await;
    create_entity(&http, "notes", "n-002", json!({"text": "note two"})).await;

    let resp = http.post("/snapshot").json(&json!({})).await;
    resp.assert_status_ok();
    let body: Value = resp.json();

    let entities = body["entities"].as_array().unwrap();
    assert_eq!(entities.len(), 5, "all 5 entities should be returned");

    // Collect ids to verify every entity is present.
    let mut ids: Vec<&str> = entities.iter().map(|e| e["id"].as_str().unwrap()).collect();
    ids.sort_unstable();
    assert_eq!(ids, vec!["n-001", "n-002", "t-001", "t-002", "t-003"]);

    let cursor = body["audit_cursor"].as_u64().unwrap();
    assert!(
        cursor >= 5,
        "audit_cursor should be >= 5 for 5 creates, got {cursor}"
    );
    assert!(body["next_page_token"].is_null());
}

/// Acceptance test 2: passing `collections: Some(vec!["tasks"])` returns
/// only entities from the tasks collection.
#[tokio::test]
async fn snapshot_collections_filter_narrows_results() {
    let http = make_server();

    create_collection(&http, "tasks").await;
    create_collection(&http, "notes").await;
    create_entity(&http, "tasks", "t-001", json!({"title": "task one"})).await;
    create_entity(&http, "tasks", "t-002", json!({"title": "task two"})).await;
    create_entity(&http, "notes", "n-001", json!({"text": "note one"})).await;

    let resp = http
        .post("/snapshot")
        .json(&json!({"collections": ["tasks"]}))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();

    let entities = body["entities"].as_array().unwrap();
    assert_eq!(entities.len(), 2);
    for e in entities {
        assert_eq!(e["collection"], "tasks");
    }
}

/// Acceptance test 3: the cursor is race-free with respect to subsequent
/// mutations. Entities created after the snapshot appear in the audit tail
/// when querying `after_id=<audit_cursor>`, but the audit entries that the
/// snapshot reflected do NOT appear in that tail.
#[tokio::test]
async fn snapshot_cursor_is_race_free_against_post_snapshot_writes() {
    let http = make_server();

    // Pre-snapshot state.
    create_collection(&http, "tasks").await;
    create_entity(&http, "tasks", "t-001", json!({"title": "one"})).await;
    create_entity(&http, "tasks", "t-002", json!({"title": "two"})).await;

    let snap = http.post("/snapshot").json(&json!({})).await;
    snap.assert_status_ok();
    let snap_body: Value = snap.json();
    let cursor = snap_body["audit_cursor"].as_u64().unwrap();

    // The snapshot captured t-001 and t-002.
    let snap_ids: Vec<&str> = snap_body["entities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_str().unwrap())
        .collect();
    assert!(snap_ids.contains(&"t-001"));
    assert!(snap_ids.contains(&"t-002"));

    // Post-snapshot write — this must appear in the audit tail but not in
    // the snapshot.
    create_entity(&http, "tasks", "t-003", json!({"title": "three"})).await;

    // Audit tail query starting strictly after the snapshot cursor.
    let tail = http.get(&format!("/audit/query?after_id={cursor}")).await;
    tail.assert_status_ok();
    let tail_body: Value = tail.json();
    let tail_entries = tail_body["entries"].as_array().unwrap();

    // The new create must be present.
    let tail_ids: Vec<&str> = tail_entries
        .iter()
        .map(|e| e["entity_id"].as_str().unwrap())
        .collect();
    assert!(
        tail_ids.contains(&"t-003"),
        "post-snapshot write should appear in audit tail"
    );
    // The snapshotted creates must NOT be in the tail (they are at or below
    // the captured cursor).
    assert!(
        !tail_ids.contains(&"t-001"),
        "t-001 was captured by snapshot and should not appear in audit tail"
    );
    assert!(
        !tail_ids.contains(&"t-002"),
        "t-002 was captured by snapshot and should not appear in audit tail"
    );
}

/// Acceptance test 4: snapshotting an empty database returns an empty
/// entity list and `audit_cursor: 0`.
#[tokio::test]
async fn snapshot_empty_database_returns_zero_cursor() {
    let http = make_server();

    let resp = http.post("/snapshot").json(&json!({})).await;
    resp.assert_status_ok();
    let body: Value = resp.json();

    assert!(body["entities"].as_array().unwrap().is_empty());
    assert_eq!(body["audit_cursor"].as_u64().unwrap(), 0);
    assert!(body["next_page_token"].is_null());
}

/// Acceptance test 5: pagination with `limit: 2` over 5 entities returns
/// two entities + page token, two entities + page token, then one entity
/// and `next_page_token: None`.
#[tokio::test]
async fn snapshot_pagination_walks_all_pages() {
    let http = make_server();

    // Five entities spread across two collections so we exercise cross-
    // collection ordering of the page cursor.
    create_collection(&http, "tasks").await;
    create_collection(&http, "notes").await;
    create_entity(&http, "tasks", "t-001", json!({"title": "one"})).await;
    create_entity(&http, "tasks", "t-002", json!({"title": "two"})).await;
    create_entity(&http, "tasks", "t-003", json!({"title": "three"})).await;
    create_entity(&http, "notes", "n-001", json!({"text": "a"})).await;
    create_entity(&http, "notes", "n-002", json!({"text": "b"})).await;

    // Page 1 — 2 entities + token.
    let p1 = http.post("/snapshot").json(&json!({"limit": 2})).await;
    p1.assert_status_ok();
    let p1_body: Value = p1.json();
    let p1_entities = p1_body["entities"].as_array().unwrap();
    assert_eq!(p1_entities.len(), 2);
    let p1_token = p1_body["next_page_token"].as_str().unwrap().to_string();
    let cursor = p1_body["audit_cursor"].as_u64().unwrap();

    // Page 2 — 2 more entities + token. Every page carries the same
    // snapshot `audit_cursor` so consumers can merge pages safely.
    let p2 = http
        .post("/snapshot")
        .json(&json!({"limit": 2, "after_page_token": p1_token}))
        .await;
    p2.assert_status_ok();
    let p2_body: Value = p2.json();
    let p2_entities = p2_body["entities"].as_array().unwrap();
    assert_eq!(p2_entities.len(), 2);
    let p2_token = p2_body["next_page_token"].as_str().unwrap().to_string();
    assert_eq!(p2_body["audit_cursor"].as_u64().unwrap(), cursor);

    // Page 3 — the last entity + no next token.
    let p3 = http
        .post("/snapshot")
        .json(&json!({"limit": 2, "after_page_token": p2_token}))
        .await;
    p3.assert_status_ok();
    let p3_body: Value = p3.json();
    let p3_entities = p3_body["entities"].as_array().unwrap();
    assert_eq!(p3_entities.len(), 1);
    assert!(p3_body["next_page_token"].is_null());
    assert_eq!(p3_body["audit_cursor"].as_u64().unwrap(), cursor);

    // Union of all pages covers all 5 entities without duplication.
    let mut all: Vec<(String, String)> = p1_entities
        .iter()
        .chain(p2_entities.iter())
        .chain(p3_entities.iter())
        .map(|e| {
            (
                e["collection"].as_str().unwrap().to_string(),
                e["id"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    all.sort();
    all.dedup();
    assert_eq!(
        all.len(),
        5,
        "union of all pages should contain all 5 entities exactly once"
    );
}

/// Covers the `/db/{database}/snapshot` route shape referenced in the task
/// prompt. The default tenant router maps any database name back to the
/// single in-memory handler, so we exercise the route with the alias
/// `"default"` to confirm the nested path binds to the same handler.
#[tokio::test]
async fn snapshot_route_is_available_under_db_prefix() {
    let http = make_server();

    create_collection(&http, "tasks").await;
    create_entity(&http, "tasks", "t-001", json!({"title": "hello"})).await;

    let resp = http.post("/db/default/snapshot").json(&json!({})).await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["entities"].as_array().unwrap().len(), 1);
    assert!(body["audit_cursor"].as_u64().unwrap() >= 1);
}
