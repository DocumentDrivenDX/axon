//! Federation identity tests — INV-021 primitive level.
//!
//! Covers:
//! a) Fresh upsert creates exactly one user row and one identity row.
//! b) Calling upsert again with the same (provider, external_id) is idempotent.
//! c) 64 concurrent callers with the same identity converge on one user / one
//!    identity (memory and sqlite backends).
//! d) Two different (provider, external_id) pairs produce two distinct users.
//!    NOTE: this bead does NOT implement identity merge — two providers that
//!    belong to the same human produce two separate users here. A later bead
//!    (identity-merge, email-match policy) will link them.
//! e) `resolve_tailscale_identity` delegates to `storage.upsert_user_identity`
//!    with the correct provider / external_id.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use axon_server::auth::TailscaleWhoisResponse;
use axon_server::federation::resolve_tailscale_identity;
use axon_storage::{MemoryStorageAdapter, SqliteStorageAdapter, StorageAdapter};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn fresh_memory() -> MemoryStorageAdapter {
    MemoryStorageAdapter::default()
}

fn fresh_sqlite() -> SqliteStorageAdapter {
    let adapter = SqliteStorageAdapter::open_in_memory().expect("sqlite in-memory should open");
    adapter
        .apply_auth_migrations()
        .expect("auth migrations should apply");
    adapter
}

// ── a) upsert_fresh_creates_user_and_identity ─────────────────────────────────

#[test]
fn upsert_fresh_creates_user_and_identity_memory() {
    let adapter = fresh_memory();
    let user = adapter
        .upsert_user_identity("tailscale", "alice@tailnet", "Alice", Some("alice@tailnet"))
        .expect("first upsert should succeed");
    assert_eq!(user.display_name, "Alice");
    assert_eq!(user.email.as_deref(), Some("alice@tailnet"));
    assert_eq!(adapter.upsert_identity_count(), 1, "user_identities should have 1 row");
    assert_eq!(adapter.upserted_user_count(), 1, "users should have 1 row");
}

#[test]
fn upsert_fresh_creates_user_and_identity_sqlite() {
    let adapter = fresh_sqlite();
    let user = adapter
        .upsert_user_identity("tailscale", "alice@tailnet", "Alice", Some("alice@tailnet"))
        .expect("first upsert should succeed");
    assert_eq!(user.display_name, "Alice");
    assert_eq!(
        adapter.query_identity_count().unwrap(),
        1,
        "user_identities should have 1 row"
    );
    assert_eq!(
        adapter.query_user_count().unwrap(),
        1,
        "users should have 1 row"
    );
}

// ── b) upsert_same_identity_is_idempotent ─────────────────────────────────────

#[test]
fn upsert_same_identity_is_idempotent_memory() {
    let adapter = fresh_memory();
    let u1 = adapter
        .upsert_user_identity("tailscale", "alice@tailnet", "Alice", None)
        .expect("first upsert should succeed");
    let u2 = adapter
        .upsert_user_identity("tailscale", "alice@tailnet", "Alice", None)
        .expect("second upsert should succeed");
    assert_eq!(u1.id, u2.id, "both calls must return the same user_id");
    assert_eq!(adapter.upsert_identity_count(), 1, "user_identities should still have 1 row");
    assert_eq!(adapter.upserted_user_count(), 1, "users should still have 1 row");
}

#[test]
fn upsert_same_identity_is_idempotent_sqlite() {
    let adapter = fresh_sqlite();
    let u1 = adapter
        .upsert_user_identity("tailscale", "alice@tailnet", "Alice", None)
        .expect("first upsert should succeed");
    let u2 = adapter
        .upsert_user_identity("tailscale", "alice@tailnet", "Alice", None)
        .expect("second upsert should succeed");
    assert_eq!(u1.id, u2.id, "both calls must return the same user_id");
    assert_eq!(adapter.query_identity_count().unwrap(), 1);
    assert_eq!(adapter.query_user_count().unwrap(), 1);
}

// ── c) concurrent_upsert_converges ────────────────────────────────────────────

#[test]
fn concurrent_upsert_converges_memory() {
    let adapter = Arc::new(fresh_memory());
    let results: Vec<_> = std::thread::scope(|s| {
        (0..64)
            .map(|_| {
                let adapter = Arc::clone(&adapter);
                s.spawn(move || {
                    adapter
                        .upsert_user_identity("tailscale", "alice@tailnet", "Alice", None)
                        .expect("concurrent upsert should succeed")
                })
            })
            .map(|h| h.join().unwrap())
            .collect()
    });

    let first_id = results[0].id.clone();
    assert!(
        results.iter().all(|u| u.id == first_id),
        "all 64 callers must get the same user_id"
    );
    assert_eq!(adapter.upsert_identity_count(), 1, "user_identities must have exactly 1 row");
    assert_eq!(adapter.upserted_user_count(), 1, "users must have exactly 1 row");
}

#[test]
fn concurrent_upsert_converges_sqlite() {
    let adapter = Arc::new(fresh_sqlite());
    let results: Vec<_> = std::thread::scope(|s| {
        (0..64)
            .map(|_| {
                let adapter = Arc::clone(&adapter);
                s.spawn(move || {
                    adapter
                        .upsert_user_identity("tailscale", "alice@tailnet", "Alice", None)
                        .expect("concurrent upsert should succeed")
                })
            })
            .map(|h| h.join().unwrap())
            .collect()
    });

    let first_id = results[0].id.clone();
    assert!(
        results.iter().all(|u| u.id == first_id),
        "all 64 callers must get the same user_id"
    );
    assert_eq!(adapter.query_identity_count().unwrap(), 1, "user_identities must have 1 row");
    assert_eq!(adapter.query_user_count().unwrap(), 1, "users must have 1 row");
}

// ── d) second_provider_produces_distinct_users ────────────────────────────────

#[test]
fn second_provider_produces_distinct_users_memory() {
    // NOTE: This bead does NOT implement identity merge.
    //
    // Two different (provider, external_id) pairs represent two different
    // external identities and therefore produce two separate Axon users.
    // Linking "tailscale/alice@tailnet" to "oidc/alice@corp.example" as the
    // same human requires an email-match policy or explicit admin action —
    // that is deferred to a later identity-merge bead. For now we assert that
    // the storage layer behaves safely (no crash, no accidental deduplication).
    let adapter = fresh_memory();
    let ua = adapter
        .upsert_user_identity("tailscale", "alice@tailnet", "Alice", None)
        .expect("tailscale upsert should succeed");
    let ub = adapter
        .upsert_user_identity("oidc", "alice@corp.example", "Alice", None)
        .expect("oidc upsert should succeed");
    assert_ne!(ua.id, ub.id, "distinct providers must produce distinct users");
    assert_eq!(adapter.upsert_identity_count(), 2, "user_identities should have 2 rows");
    assert_eq!(adapter.upserted_user_count(), 2, "users should have 2 rows");
}

#[test]
fn second_provider_produces_distinct_users_sqlite() {
    // Same rationale as the memory variant above.
    let adapter = fresh_sqlite();
    let ua = adapter
        .upsert_user_identity("tailscale", "alice@tailnet", "Alice", None)
        .expect("tailscale upsert should succeed");
    let ub = adapter
        .upsert_user_identity("oidc", "alice@corp.example", "Alice", None)
        .expect("oidc upsert should succeed");
    assert_ne!(ua.id, ub.id, "distinct providers must produce distinct users");
    assert_eq!(adapter.query_identity_count().unwrap(), 2);
    assert_eq!(adapter.query_user_count().unwrap(), 2);
}

// ── e) federation_module_wraps_storage_upsert ─────────────────────────────────

#[test]
fn federation_module_wraps_storage_upsert_memory() {
    let adapter = fresh_memory();
    let whois = TailscaleWhoisResponse {
        node_name: "alice-laptop".to_string(),
        user_login: "alice@tailnet".to_string(),
        tags: vec![],
    };

    // First call: provisions a new user.
    let user = resolve_tailscale_identity(&whois, &adapter)
        .expect("federation resolve should succeed");
    // external_id is derived from user_login (non-empty), which becomes display_name.
    assert_eq!(user.display_name, "alice@tailnet");
    assert_eq!(user.email.as_deref(), Some("alice@tailnet"));
    assert_eq!(adapter.upsert_identity_count(), 1);

    // Second call: idempotent — returns the same user.
    let user2 = resolve_tailscale_identity(&whois, &adapter)
        .expect("second federation resolve should succeed");
    assert_eq!(user.id, user2.id, "resolve must be idempotent");
    assert_eq!(adapter.upsert_identity_count(), 1, "still exactly 1 identity row");
}

#[test]
fn federation_module_uses_node_name_when_login_empty() {
    let adapter = fresh_memory();
    let whois = TailscaleWhoisResponse {
        node_name: "agent-worker-1".to_string(),
        user_login: String::new(),
        tags: vec!["tag:axon-agent".to_string()],
    };
    let user = resolve_tailscale_identity(&whois, &adapter)
        .expect("federation resolve for tagged node should succeed");
    // When user_login is empty, node_name is used as the external_id and display_name.
    assert_eq!(user.display_name, "agent-worker-1");
    assert!(user.email.is_none(), "no email for tagged service nodes");
}
