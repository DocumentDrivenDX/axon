//! Integration tests: tenant_users M:N membership CRUD
//! (StorageAdapter::upsert_tenant_member + remove_tenant_member).
//!
//! Tests run on three backends:
//!   - MemoryStorageAdapter (no schema/FK constraints)
//!   - SqliteStorageAdapter (in-memory, FK enforced)
//!   - PostgresStorageAdapter (testcontainers; skipped if Docker unavailable)
//!
//! Each test function is a generic closure that accepts any `StorageAdapter`.
//! Backend-specific harness functions set up the adapter and (for SQL backends)
//! pre-insert the required FK rows before delegating to the generic closure.

#![allow(clippy::unwrap_used)]

use axon_core::auth::{TenantId, TenantRole, UserId};
use axon_storage::{
    MemoryStorageAdapter, PostgresStorageAdapter, SqliteStorageAdapter, StorageAdapter,
};
use testcontainers_modules::{
    postgres,
    testcontainers::{runners::SyncRunner, Container},
};

// ── PostgreSQL test infrastructure ──────────────────────────────────────────

struct TestPg {
    pub dsn: String,
    _container: Option<Container<postgres::Postgres>>,
}

/// Resolve or start a test PostgreSQL cluster.
///
/// Returns `None` if neither `AXON_TEST_POSTGRES` is set nor a Docker container
/// can be started, signalling that the test should be skipped.
fn cluster_or_skip(test_name: &str) -> Option<TestPg> {
    if let Ok(dsn) = std::env::var("AXON_TEST_POSTGRES") {
        return Some(TestPg {
            dsn,
            _container: None,
        });
    }

    let result = postgres::Postgres::default()
        .with_db_name("postgres")
        .with_user("postgres")
        .with_password("postgres")
        .start();

    match result {
        Ok(container) => {
            let host = container
                .get_host()
                .expect("container host should be available");
            let port = container
                .get_host_port_ipv4(5432)
                .expect("container port should be available");
            let dsn =
                format!("host={host} port={port} user=postgres password=postgres dbname=postgres");
            Some(TestPg {
                dsn,
                _container: Some(container),
            })
        }
        Err(e) => {
            eprintln!(
                "SKIP {test_name}: container runtime unavailable ({e}); \
                 set AXON_TEST_POSTGRES to run against a real cluster"
            );
            None
        }
    }
}

// ── Generic test logic ────────────────────────────────────────────────────────
//
// Each test scenario is expressed as a closure over a StorageAdapter and
// two convenience functions (count_rows, count_members_via_get) that abstract
// backend-specific row counting.

/// a) upsert with a fresh (tenant_id, user_id) creates a new membership row.
fn scenario_upsert_fresh_creates_membership(
    adapter: &dyn StorageAdapter,
    count_rows: impl Fn(&str, &str) -> i64,
) {
    let tid = TenantId::new("t-fresh");
    let uid = UserId::new("u-fresh");

    let member = adapter
        .upsert_tenant_member(tid.clone(), uid.clone(), TenantRole::Write)
        .expect("upsert should succeed");

    assert_eq!(member.tenant_id, tid);
    assert_eq!(member.user_id, uid);
    assert_eq!(member.role, TenantRole::Write);
    assert_eq!(
        count_rows("t-fresh", "u-fresh"),
        1,
        "should have exactly 1 row"
    );
}

/// b) upserting the same (tenant_id, user_id) pair with a different role
///    updates the role and leaves exactly one row.
fn scenario_upsert_same_pair_updates_role(
    adapter: &dyn StorageAdapter,
    count_rows: impl Fn(&str, &str) -> i64,
) {
    let tid = TenantId::new("t-update");
    let uid = UserId::new("u-update");

    adapter
        .upsert_tenant_member(tid.clone(), uid.clone(), TenantRole::Read)
        .expect("first upsert should succeed");

    let member = adapter
        .upsert_tenant_member(tid.clone(), uid.clone(), TenantRole::Admin)
        .expect("second upsert should succeed");

    assert_eq!(member.role, TenantRole::Admin, "role should be updated");
    assert_eq!(
        count_rows("t-update", "u-update"),
        1,
        "still exactly 1 row after update"
    );

    // Confirm via the read path.
    let fetched = adapter
        .get_tenant_member(tid.clone(), uid.clone())
        .expect("get should succeed")
        .expect("member should exist");
    assert_eq!(fetched.role, TenantRole::Admin);
}

/// c) remove_tenant_member returns true and the row is gone.
fn scenario_remove_existing_returns_true(
    adapter: &dyn StorageAdapter,
    count_rows: impl Fn(&str, &str) -> i64,
) {
    let tid = TenantId::new("t-remove");
    let uid = UserId::new("u-remove");

    adapter
        .upsert_tenant_member(tid.clone(), uid.clone(), TenantRole::Write)
        .expect("upsert should succeed");

    let removed = adapter
        .remove_tenant_member(tid.clone(), uid.clone())
        .expect("remove should succeed");

    assert!(removed, "remove should return true for an existing row");
    assert_eq!(count_rows("t-remove", "u-remove"), 0, "row should be gone");
}

/// d) remove_tenant_member for a never-inserted pair returns false.
fn scenario_remove_missing_returns_false(adapter: &dyn StorageAdapter) {
    let removed = adapter
        .remove_tenant_member(TenantId::new("t-ghost"), UserId::new("u-ghost"))
        .expect("remove should not error on missing row");

    assert!(
        !removed,
        "remove should return false for a non-existent row"
    );
}

/// e) get_tenant_member finds the row created by upsert_tenant_member.
fn scenario_get_tenant_member_after_upsert(adapter: &dyn StorageAdapter) {
    let tid = TenantId::new("t-get");
    let uid = UserId::new("u-get");

    // Before upsert, should not exist.
    let before = adapter
        .get_tenant_member(tid.clone(), uid.clone())
        .expect("get should succeed");
    assert!(before.is_none(), "member should not exist before upsert");

    adapter
        .upsert_tenant_member(tid.clone(), uid.clone(), TenantRole::Read)
        .expect("upsert should succeed");

    // After upsert, should be found.
    let after = adapter
        .get_tenant_member(tid.clone(), uid.clone())
        .expect("get should succeed")
        .expect("member should exist after upsert");

    assert_eq!(after.tenant_id, tid);
    assert_eq!(after.user_id, uid);
    assert_eq!(after.role, TenantRole::Read);
}

// ── Memory backend ────────────────────────────────────────────────────────────
//
// The memory adapter has no FK constraints, so no pre-insertion of tenants/users
// is needed. Just use a fresh default adapter.

#[test]
fn memory_upsert_fresh_creates_membership() {
    let adapter = MemoryStorageAdapter::default();
    scenario_upsert_fresh_creates_membership(&adapter, |tid, uid| {
        adapter.test_count_tenant_members(tid, uid) as i64
    });
}

#[test]
fn memory_upsert_same_pair_updates_role() {
    let adapter = MemoryStorageAdapter::default();
    scenario_upsert_same_pair_updates_role(&adapter, |tid, uid| {
        adapter.test_count_tenant_members(tid, uid) as i64
    });
}

#[test]
fn memory_remove_existing_returns_true() {
    let adapter = MemoryStorageAdapter::default();
    scenario_remove_existing_returns_true(&adapter, |tid, uid| {
        adapter.test_count_tenant_members(tid, uid) as i64
    });
}

#[test]
fn memory_remove_missing_returns_false() {
    let adapter = MemoryStorageAdapter::default();
    scenario_remove_missing_returns_false(&adapter);
}

#[test]
fn memory_get_tenant_member_after_upsert() {
    let adapter = MemoryStorageAdapter::default();
    scenario_get_tenant_member_after_upsert(&adapter);
}

// ── SQLite backend ────────────────────────────────────────────────────────────

/// Build a SQLite adapter with auth schema applied and the given
/// (tenant_id, user_id) pairs pre-inserted for FK compliance.
fn sqlite_adapter_with_pairs(pairs: &[(&str, &str)]) -> SqliteStorageAdapter {
    let adapter = SqliteStorageAdapter::open_in_memory().expect("sqlite in-memory should open");
    adapter
        .apply_auth_migrations()
        .expect("auth migrations should apply");
    for (tid, uid) in pairs {
        adapter
            .test_insert_tenant_and_user(tid, uid)
            .expect("test setup should succeed");
    }
    adapter
}

#[test]
fn sqlite_upsert_fresh_creates_membership() {
    let adapter = sqlite_adapter_with_pairs(&[("t-fresh", "u-fresh")]);
    scenario_upsert_fresh_creates_membership(&adapter, |tid, uid| {
        adapter
            .test_count_tenant_members(tid, uid)
            .expect("count should succeed")
    });
}

#[test]
fn sqlite_upsert_same_pair_updates_role() {
    let adapter = sqlite_adapter_with_pairs(&[("t-update", "u-update")]);
    scenario_upsert_same_pair_updates_role(&adapter, |tid, uid| {
        adapter
            .test_count_tenant_members(tid, uid)
            .expect("count should succeed")
    });
}

#[test]
fn sqlite_remove_existing_returns_true() {
    let adapter = sqlite_adapter_with_pairs(&[("t-remove", "u-remove")]);
    scenario_remove_existing_returns_true(&adapter, |tid, uid| {
        adapter
            .test_count_tenant_members(tid, uid)
            .expect("count should succeed")
    });
}

#[test]
fn sqlite_remove_missing_returns_false() {
    let adapter = sqlite_adapter_with_pairs(&[]);
    scenario_remove_missing_returns_false(&adapter);
}

#[test]
fn sqlite_get_tenant_member_after_upsert() {
    let adapter = sqlite_adapter_with_pairs(&[("t-get", "u-get")]);
    scenario_get_tenant_member_after_upsert(&adapter);
}

// ── PostgreSQL backend ────────────────────────────────────────────────────────

#[test]
fn postgres_upsert_fresh_creates_membership() {
    let Some(cluster) = cluster_or_skip("postgres_upsert_fresh_creates_membership") else {
        return;
    };

    let adapter =
        PostgresStorageAdapter::connect(&cluster.dsn).expect("postgres connect should succeed");
    adapter
        .apply_auth_migrations()
        .expect("auth migrations should apply");
    adapter
        .test_insert_tenant_and_user("t-fresh", "u-fresh")
        .expect("test setup should succeed");

    scenario_upsert_fresh_creates_membership(&adapter, |tid, uid| {
        adapter
            .test_count_tenant_members(tid, uid)
            .expect("count should succeed")
    });
}

#[test]
fn postgres_upsert_same_pair_updates_role() {
    let Some(cluster) = cluster_or_skip("postgres_upsert_same_pair_updates_role") else {
        return;
    };

    let adapter =
        PostgresStorageAdapter::connect(&cluster.dsn).expect("postgres connect should succeed");
    adapter
        .apply_auth_migrations()
        .expect("auth migrations should apply");
    adapter
        .test_insert_tenant_and_user("t-update", "u-update")
        .expect("test setup should succeed");

    scenario_upsert_same_pair_updates_role(&adapter, |tid, uid| {
        adapter
            .test_count_tenant_members(tid, uid)
            .expect("count should succeed")
    });
}

#[test]
fn postgres_remove_existing_returns_true() {
    let Some(cluster) = cluster_or_skip("postgres_remove_existing_returns_true") else {
        return;
    };

    let adapter =
        PostgresStorageAdapter::connect(&cluster.dsn).expect("postgres connect should succeed");
    adapter
        .apply_auth_migrations()
        .expect("auth migrations should apply");
    adapter
        .test_insert_tenant_and_user("t-remove", "u-remove")
        .expect("test setup should succeed");

    scenario_remove_existing_returns_true(&adapter, |tid, uid| {
        adapter
            .test_count_tenant_members(tid, uid)
            .expect("count should succeed")
    });
}

#[test]
fn postgres_remove_missing_returns_false() {
    let Some(cluster) = cluster_or_skip("postgres_remove_missing_returns_false") else {
        return;
    };

    let adapter =
        PostgresStorageAdapter::connect(&cluster.dsn).expect("postgres connect should succeed");
    adapter
        .apply_auth_migrations()
        .expect("auth migrations should apply");

    scenario_remove_missing_returns_false(&adapter);
}

#[test]
fn postgres_get_tenant_member_after_upsert() {
    let Some(cluster) = cluster_or_skip("postgres_get_tenant_member_after_upsert") else {
        return;
    };

    let adapter =
        PostgresStorageAdapter::connect(&cluster.dsn).expect("postgres connect should succeed");
    adapter
        .apply_auth_migrations()
        .expect("auth migrations should apply");
    adapter
        .test_insert_tenant_and_user("t-get", "u-get")
        .expect("test setup should succeed");

    scenario_get_tenant_member_after_upsert(&adapter);
}
