//! Integration test: per-tenant PostgreSQL database isolation.
//!
//! Verifies that `provision_postgres_database` / `deprovision_postgres_database`
//! create and drop physical PostgreSQL databases, and that two tenants writing
//! entities to their own databases cannot see each other's data.
//!
//! When `AXON_TEST_POSTGRES` is set, it is used as the superadmin DSN.
//! Otherwise the test attempts to start a PostgreSQL container via
//! `testcontainers`.  If the container runtime is unavailable, the test is
//! skipped gracefully.

#![allow(clippy::unwrap_used)]

use axon_core::id::CollectionId;
use axon_core::types::Entity;
use axon_storage::adapter::StorageAdapter;
use axon_storage::{
    deprovision_postgres_database, provision_postgres_database, tenant_dsn, PostgresStorageAdapter,
};
use testcontainers_modules::{
    postgres,
    testcontainers::{runners::SyncRunner, Container},
};

struct TestPgCluster {
    /// Connection string for a superuser on the cluster (no specific database).
    superadmin_dsn: String,
    /// Container handle — kept alive for the duration of the test.
    _container: Option<Container<postgres::Postgres>>,
}

/// Resolve or start a test PostgreSQL cluster.
///
/// Returns `None` if neither `AXON_TEST_POSTGRES` is set nor a container
/// can be started (Docker unavailable), signalling that the test should be
/// skipped.
fn cluster_or_skip(test_name: &str) -> Option<TestPgCluster> {
    if let Ok(dsn) = std::env::var("AXON_TEST_POSTGRES") {
        return Some(TestPgCluster {
            superadmin_dsn: dsn,
            _container: None,
        });
    }

    // Try testcontainers.
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
            Some(TestPgCluster {
                superadmin_dsn: dsn,
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

/// Drop a tenant database ignoring "not found" errors (best-effort cleanup).
fn cleanup_tenant(superadmin_dsn: &str, name: &str) {
    let _ = deprovision_postgres_database(superadmin_dsn, name);
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[test]
fn provision_and_deprovision_round_trip() {
    let Some(cluster) = cluster_or_skip("provision_and_deprovision_round_trip") else {
        return;
    };
    let dsn = &cluster.superadmin_dsn;
    let tenant = "provision_test";

    // Cleanup any leftover from a previous failed run.
    cleanup_tenant(dsn, tenant);

    provision_postgres_database(dsn, tenant).expect("provision should succeed for a new database");

    // Provisioning again should return AlreadyExists.
    let err = provision_postgres_database(dsn, tenant)
        .expect_err("second provision should fail with AlreadyExists");
    assert!(
        matches!(err, axon_core::error::AxonError::AlreadyExists(_)),
        "expected AlreadyExists, got {err:?}"
    );

    // Deprovisioning should succeed.
    deprovision_postgres_database(dsn, tenant).expect("deprovision should succeed");

    // Deprovisioning a second time should return NotFound.
    let err2 = deprovision_postgres_database(dsn, tenant)
        .expect_err("second deprovision should fail with NotFound");
    assert!(
        matches!(err2, axon_core::error::AxonError::NotFound(_)),
        "expected NotFound, got {err2:?}"
    );
}

#[test]
fn tenant_dsn_replaces_dbname_in_key_value_format() {
    let base = "host=localhost port=5432 user=axon password=secret dbname=postgres";
    let result = tenant_dsn(base, "teamA");
    assert!(
        result.contains("dbname=axon_teamA"),
        "expected dbname=axon_teamA in '{result}'"
    );
    assert!(
        !result.contains("dbname=postgres"),
        "old dbname should be replaced"
    );
}

#[test]
fn tenant_dsn_replaces_dbname_in_url_format() {
    let base = "postgres://axon:secret@localhost:5432/postgres";
    let result = tenant_dsn(base, "teamB");
    assert!(
        result.contains("/axon_teamB"),
        "expected /axon_teamB in '{result}'"
    );
    assert!(
        !result.contains("/postgres"),
        "old database path should be replaced"
    );
}

#[test]
fn two_tenant_databases_are_isolated() {
    let Some(cluster) = cluster_or_skip("two_tenant_databases_are_isolated") else {
        return;
    };
    let dsn = &cluster.superadmin_dsn;
    let tenant_a = "isola_tenant_a";
    let tenant_b = "isola_tenant_b";

    // Cleanup any leftover from a previous failed run.
    cleanup_tenant(dsn, tenant_a);
    cleanup_tenant(dsn, tenant_b);

    // Provision both tenant databases.
    provision_postgres_database(dsn, tenant_a).expect("provision tenant_a should succeed");
    provision_postgres_database(dsn, tenant_b).expect("provision tenant_b should succeed");

    // Connect to each tenant database.
    let conn_a = tenant_dsn(dsn, tenant_a);
    let conn_b = tenant_dsn(dsn, tenant_b);

    let mut adapter_a =
        PostgresStorageAdapter::connect(&conn_a).expect("connect to tenant_a should succeed");
    let mut adapter_b =
        PostgresStorageAdapter::connect(&conn_b).expect("connect to tenant_b should succeed");

    let col = CollectionId::new("widgets");

    // Write an entity to tenant_a.
    let entity_a = Entity::new(
        col.clone(),
        axon_core::id::EntityId::new("w-001"),
        serde_json::json!({"owner": "tenant_a", "name": "widget-alpha"}),
    );
    adapter_a
        .put(entity_a.clone())
        .expect("put to tenant_a should succeed");

    // Write a different entity to tenant_b.
    let entity_b = Entity::new(
        col.clone(),
        axon_core::id::EntityId::new("w-001"),
        serde_json::json!({"owner": "tenant_b", "name": "widget-beta"}),
    );
    adapter_b
        .put(entity_b.clone())
        .expect("put to tenant_b should succeed");

    // Read back from tenant_a — should see the tenant_a entity.
    let read_a = adapter_a
        .get(&col, &axon_core::id::EntityId::new("w-001"))
        .expect("get from tenant_a should succeed")
        .expect("entity should exist in tenant_a");
    assert_eq!(
        read_a.data["owner"],
        serde_json::json!("tenant_a"),
        "tenant_a entity should have owner=tenant_a"
    );

    // Read back from tenant_b — should see the tenant_b entity, not tenant_a's.
    let read_b = adapter_b
        .get(&col, &axon_core::id::EntityId::new("w-001"))
        .expect("get from tenant_b should succeed")
        .expect("entity should exist in tenant_b");
    assert_eq!(
        read_b.data["owner"],
        serde_json::json!("tenant_b"),
        "tenant_b entity should have owner=tenant_b, got: {:?}",
        read_b.data
    );

    // tenant_b must not see tenant_a's data and vice-versa (different PG databases
    // means all tables are separate — the same entity ID in each database holds
    // the respective tenant's data).
    assert_ne!(
        read_a.data["name"], read_b.data["name"],
        "tenant_a and tenant_b entities must not share data"
    );

    // Cleanup.
    drop(adapter_a);
    drop(adapter_b);
    cleanup_tenant(dsn, tenant_a);
    cleanup_tenant(dsn, tenant_b);
}
