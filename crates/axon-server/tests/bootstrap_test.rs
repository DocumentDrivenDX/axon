//! Integration tests for zero-tenant auto-bootstrap (axon-e77f0e5b).
//!
//! Covered:
//! - bootstrap_first_call_creates_everything
//! - bootstrap_second_call_returns_same_handle
//! - bootstrap_skips_when_tenants_present
//! - concurrent_bootstrap_converges (memory AND sqlite, N=64)
//! - bootstrap_uses_default_database_name

use std::sync::Arc;

use axon_core::auth::UserId;
use axon_core::error::AxonError;
use axon_server::bootstrap::{ensure_default_tenant, DefaultTenantHandle};
use axon_storage::{MemoryStorageAdapter, SqliteStorageAdapter, StorageAdapter};

// ── helpers ──────────────────────────────────────────────────────────────────

/// Build a memory adapter with a pre-inserted user ready for bootstrap tests.
///
/// Memory has no FK constraints, so no user row setup is required; this
/// function just returns a fresh default adapter.
fn memory_adapter() -> MemoryStorageAdapter {
    MemoryStorageAdapter::default()
}

/// Build an in-memory SQLite adapter with auth migrations applied.
fn sqlite_adapter() -> SqliteStorageAdapter {
    let adapter = SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite should open");
    adapter
        .apply_auth_migrations()
        .expect("auth migrations should apply");
    adapter
}

// ── test 1 ───────────────────────────────────────────────────────────────────

#[test]
fn bootstrap_first_call_creates_everything() {
    // --- memory backend ---
    {
        let storage = memory_adapter();
        let user_id = UserId::generate();

        let handle =
            ensure_default_tenant(&storage, user_id.clone()).expect("first call should succeed");

        assert_eq!(handle.database_name, "default");

        let members = storage
            .list_tenant_members(handle.tenant_id.clone())
            .expect("list_tenant_members should succeed");
        assert_eq!(members.len(), 1, "should have exactly 1 member");
        assert_eq!(members[0].user_id, user_id);

        let dbs = storage
            .list_tenant_databases(handle.tenant_id.clone())
            .expect("list_tenant_databases should succeed");
        assert_eq!(dbs.len(), 1, "should have exactly 1 database");
        assert_eq!(dbs[0].name, "default");

        assert_eq!(
            storage
                .count_tenants()
                .expect("count_tenants should succeed"),
            1,
            "should have exactly 1 tenant"
        );
    }

    // --- sqlite backend ---
    {
        let storage = sqlite_adapter();
        let user_id = UserId::generate();
        // Insert user row to satisfy FK constraint on tenant_users.user_id
        storage
            .test_insert_user(user_id.as_str())
            .expect("test_insert_user should succeed");

        let handle =
            ensure_default_tenant(&storage, user_id.clone()).expect("first call should succeed");

        assert_eq!(handle.database_name, "default");

        let members = storage
            .list_tenant_members(handle.tenant_id.clone())
            .expect("list_tenant_members should succeed");
        assert_eq!(members.len(), 1, "should have exactly 1 member");
        assert_eq!(members[0].user_id, user_id);

        let dbs = storage
            .list_tenant_databases(handle.tenant_id.clone())
            .expect("list_tenant_databases should succeed");
        assert_eq!(dbs.len(), 1, "should have exactly 1 database");
        assert_eq!(dbs[0].name, "default");

        assert_eq!(
            storage
                .count_tenants()
                .expect("count_tenants should succeed"),
            1,
            "should have exactly 1 tenant"
        );
    }
}

// ── test 2 ───────────────────────────────────────────────────────────────────

#[test]
fn bootstrap_second_call_returns_same_handle() {
    // --- memory backend ---
    {
        let storage = memory_adapter();
        let user1 = UserId::generate();
        let user2 = UserId::generate();

        let handle1 =
            ensure_default_tenant(&storage, user1.clone()).expect("first call should succeed");
        let handle2 =
            ensure_default_tenant(&storage, user2.clone()).expect("second call should succeed");

        assert_eq!(
            handle1.tenant_id, handle2.tenant_id,
            "both callers should receive the same tenant_id"
        );
        assert_eq!(handle2.database_name, "default");

        // Should still have exactly 1 tenant (no duplicate row)
        assert_eq!(
            storage.count_tenants().expect("count_tenants"),
            1,
            "still exactly 1 tenant after second call"
        );

        // Both users should be members
        let members = storage
            .list_tenant_members(handle1.tenant_id.clone())
            .expect("list_tenant_members");
        assert_eq!(members.len(), 2, "should have 2 members after two calls");
    }

    // --- sqlite backend ---
    {
        let storage = sqlite_adapter();
        let user1 = UserId::generate();
        let user2 = UserId::generate();

        storage
            .test_insert_user(user1.as_str())
            .expect("insert user1");
        storage
            .test_insert_user(user2.as_str())
            .expect("insert user2");

        let handle1 =
            ensure_default_tenant(&storage, user1.clone()).expect("first call should succeed");
        let handle2 =
            ensure_default_tenant(&storage, user2.clone()).expect("second call should succeed");

        assert_eq!(
            handle1.tenant_id, handle2.tenant_id,
            "both callers should receive the same tenant_id"
        );
        assert_eq!(handle2.database_name, "default");

        assert_eq!(
            storage.count_tenants().expect("count_tenants"),
            1,
            "still exactly 1 tenant after second call"
        );

        let members = storage
            .list_tenant_members(handle1.tenant_id.clone())
            .expect("list_tenant_members");
        assert_eq!(members.len(), 2, "should have 2 members after two calls");
    }
}

// ── test 3 ───────────────────────────────────────────────────────────────────

#[test]
fn bootstrap_skips_when_tenants_present() {
    // --- memory backend ---
    {
        let storage = memory_adapter();
        // Pre-seed with an explicit tenant
        storage
            .upsert_default_tenant("acme")
            .expect("seeding tenant should succeed");

        let user_id = UserId::generate();
        let result = ensure_default_tenant(&storage, user_id);

        match result {
            Err(AxonError::NotFound(_)) => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    // --- sqlite backend ---
    {
        let storage = sqlite_adapter();
        // Pre-seed with an explicit tenant using upsert_default_tenant
        storage
            .upsert_default_tenant("acme")
            .expect("seeding tenant should succeed");

        let user_id = UserId::generate();
        let result = ensure_default_tenant(&storage, user_id);

        match result {
            Err(AxonError::NotFound(_)) => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
    }
}

// ── test 4 ───────────────────────────────────────────────────────────────────

#[test]
fn concurrent_bootstrap_converges() {
    const N: usize = 64;

    // --- memory backend ---
    {
        let storage = Arc::new(memory_adapter());
        let user_ids: Vec<UserId> = (0..N).map(|_| UserId::generate()).collect();

        // Collect handles into a Vec first so all threads start before any join.
        // The needless_collect lint is suppressed because sequential spawn+join
        // would not exercise concurrent behaviour.
        #[allow(clippy::needless_collect)]
        let handles: Vec<_> = user_ids
            .iter()
            .map(|uid| {
                let s = Arc::clone(&storage);
                let u = uid.clone();
                std::thread::spawn(move || ensure_default_tenant(s.as_ref(), u))
            })
            .collect();

        let results: Vec<DefaultTenantHandle> = handles
            .into_iter()
            .map(|h| {
                h.join()
                    .expect("thread should not panic")
                    .expect("ensure_default_tenant should succeed")
            })
            .collect();

        // All callers must get the same tenant_id
        let first_tid = results[0].tenant_id.clone();
        for r in &results {
            assert_eq!(
                r.tenant_id, first_tid,
                "all callers must converge on the same tenant_id"
            );
            assert_eq!(r.database_name, "default");
        }

        // Exactly 1 tenant, 1 database
        assert_eq!(
            storage.count_tenants().expect("count_tenants"),
            1,
            "memory: exactly 1 tenant after concurrent bootstrap"
        );
        let dbs = storage
            .list_tenant_databases(first_tid.clone())
            .expect("list_tenant_databases");
        assert_eq!(dbs.len(), 1, "memory: exactly 1 database");

        // All N users must be members
        let members = storage
            .list_tenant_members(first_tid.clone())
            .expect("list_tenant_members");
        assert_eq!(members.len(), N, "memory: exactly N members");
    }

    // --- sqlite backend ---
    {
        let storage = Arc::new(sqlite_adapter());
        let user_ids: Vec<UserId> = (0..N).map(|_| UserId::generate()).collect();

        // Pre-insert all N users to satisfy FK on tenant_users.user_id
        for uid in &user_ids {
            storage
                .test_insert_user(uid.as_str())
                .expect("test_insert_user should succeed");
        }

        #[allow(clippy::needless_collect)]
        let handles: Vec<_> = user_ids
            .iter()
            .map(|uid| {
                let s = Arc::clone(&storage);
                let u = uid.clone();
                std::thread::spawn(move || ensure_default_tenant(s.as_ref(), u))
            })
            .collect();

        let results: Vec<DefaultTenantHandle> = handles
            .into_iter()
            .map(|h| {
                h.join()
                    .expect("thread should not panic")
                    .expect("ensure_default_tenant should succeed")
            })
            .collect();

        let first_tid = results[0].tenant_id.clone();
        for r in &results {
            assert_eq!(
                r.tenant_id, first_tid,
                "sqlite: all callers must converge on the same tenant_id"
            );
            assert_eq!(r.database_name, "default");
        }

        // Exactly 1 tenant, 1 database
        assert_eq!(
            storage.count_tenants().expect("count_tenants"),
            1,
            "sqlite: exactly 1 tenant after concurrent bootstrap"
        );
        let dbs = storage
            .list_tenant_databases(first_tid.clone())
            .expect("list_tenant_databases");
        assert_eq!(dbs.len(), 1, "sqlite: exactly 1 database");

        // All N users must be members
        let members = storage
            .list_tenant_members(first_tid.clone())
            .expect("list_tenant_members");
        assert_eq!(members.len(), N, "sqlite: exactly N members");
    }
}

// ── test 5 ───────────────────────────────────────────────────────────────────

#[test]
fn bootstrap_uses_default_database_name() {
    let storage = memory_adapter();
    let user_id = UserId::generate();

    let handle =
        ensure_default_tenant(&storage, user_id).expect("ensure_default_tenant should succeed");

    assert_eq!(
        handle.database_name, "default",
        "DefaultTenantHandle::database_name must be \"default\""
    );
}
