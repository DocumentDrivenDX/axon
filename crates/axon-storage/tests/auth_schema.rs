//! Integration tests: auth/tenancy schema migrations on PostgreSQL.
//!
//! Verifies that `apply_auth_migrations_postgres` creates all required tables,
//! is idempotent, enforces the required constraints, and that round-trip
//! insert + SELECT works for every table.
//!
//! When `AXON_TEST_POSTGRES` is set it is used as the superadmin DSN.
//! Otherwise the test attempts to start a PostgreSQL container via
//! `testcontainers`.  If the container runtime is unavailable the tests are
//! skipped gracefully.

#![allow(clippy::unwrap_used)]

use axon_storage::apply_auth_migrations_postgres;
use testcontainers_modules::{
    postgres,
    testcontainers::{runners::SyncRunner, Container},
};
use tokio_postgres::{Client, NoTls};

// ── Infrastructure ────────────────────────────────────────────────────────────

struct TestPg {
    pub dsn: String,
    _container: Option<Container<postgres::Postgres>>,
}

/// Resolve or start a test PostgreSQL cluster.
///
/// Returns `None` if neither `AXON_TEST_POSTGRES` is set nor a container can
/// be started (Docker unavailable), signalling that the test should be skipped.
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

/// Connect to Postgres and apply auth migrations. Returns the client.
fn connect_and_migrate(dsn: &str) -> Client {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");

    rt.block_on(async {
        let (client, connection) = tokio_postgres::connect(dsn, NoTls)
            .await
            .expect("postgres connect should succeed");

        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("postgres connection error: {e}");
            }
        });

        apply_auth_migrations_postgres(&client)
            .await
            .expect("migrations should apply");

        client
    })
}

// Wrapper that runs an async block synchronously using a fresh single-threaded runtime.
macro_rules! pg_test {
    ($test_name:expr, $dsn:expr, |$client:ident| $body:block) => {{
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build");
        rt.block_on(async {
            let ($client, connection) = tokio_postgres::connect($dsn, NoTls)
                .await
                .expect("postgres connect should succeed");
            tokio::spawn(async move {
                if let Err(e) = connection.await {
                    eprintln!("postgres connection error: {e}");
                }
            });
            apply_auth_migrations_postgres(&$client)
                .await
                .expect("migrations should apply");
            $body
        });
    }};
}

// ── Schema existence ──────────────────────────────────────────────────────────

#[test]
fn pg_creates_all_six_auth_tables() {
    let Some(cluster) = cluster_or_skip("pg_creates_all_six_auth_tables") else {
        return;
    };

    pg_test!("pg_creates_all_six_auth_tables", &cluster.dsn, |client| {
        let row = client
            .query_one(
                "SELECT COUNT(*) FROM information_schema.tables
                 WHERE table_schema = 'public'
                   AND table_name IN (
                       'tenants', 'users', 'user_identities', 'tenant_users',
                       'tenant_databases', 'credential_revocations'
                   )",
                &[],
            )
            .await
            .expect("query should succeed");
        let count: i64 = row.get(0);
        assert_eq!(count, 6, "all 6 auth tables should be created");
    });
}

#[test]
fn pg_audit_retention_policies_absent() {
    let Some(cluster) = cluster_or_skip("pg_audit_retention_policies_absent") else {
        return;
    };

    pg_test!("pg_audit_retention_policies_absent", &cluster.dsn, |client| {
        let row = client
            .query_one(
                "SELECT COUNT(*) FROM information_schema.tables
                 WHERE table_schema = 'public'
                   AND table_name = 'audit_retention_policies'",
                &[],
            )
            .await
            .expect("query should succeed");
        let count: i64 = row.get(0);
        assert_eq!(
            count, 0,
            "audit_retention_policies must NOT be created (F1 territory)"
        );
    });
}

#[test]
fn pg_migrations_are_idempotent() {
    let Some(cluster) = cluster_or_skip("pg_migrations_are_idempotent") else {
        return;
    };

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");

    rt.block_on(async {
        let (client, connection) = tokio_postgres::connect(&cluster.dsn, NoTls)
            .await
            .expect("connect should succeed");
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("postgres connection error: {e}");
            }
        });

        apply_auth_migrations_postgres(&client)
            .await
            .expect("first migration should succeed");
        apply_auth_migrations_postgres(&client)
            .await
            .expect("second migration should be idempotent");

        let row = client
            .query_one(
                "SELECT COUNT(*) FROM information_schema.tables
                 WHERE table_schema = 'public'
                   AND table_name IN (
                       'tenants', 'users', 'user_identities', 'tenant_users',
                       'tenant_databases', 'credential_revocations'
                   )",
                &[],
            )
            .await
            .expect("query should succeed");
        let count: i64 = row.get(0);
        assert_eq!(count, 6, "still exactly 6 tables after re-run");
    });
}

// ── Round-trip tests ──────────────────────────────────────────────────────────

#[test]
fn pg_round_trip_tenants() {
    let Some(cluster) = cluster_or_skip("pg_round_trip_tenants") else {
        return;
    };

    pg_test!("pg_round_trip_tenants", &cluster.dsn, |client| {
        client
            .execute(
                "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
                 VALUES ($1, $2, $3, $4, $5)",
                &[
                    &"t-001",
                    &"acme",
                    &"Acme Corp",
                    &1_000_000i64,
                    &1_000_001i64,
                ],
            )
            .await
            .expect("insert should succeed");

        let row = client
            .query_one(
                "SELECT id, name, display_name, created_at_ms, updated_at_ms
                 FROM tenants WHERE id = $1",
                &[&"t-001"],
            )
            .await
            .expect("SELECT should succeed");

        assert_eq!(row.get::<_, &str>(0), "t-001");
        assert_eq!(row.get::<_, &str>(1), "acme");
        assert_eq!(row.get::<_, &str>(2), "Acme Corp");
        assert_eq!(row.get::<_, i64>(3), 1_000_000);
        assert_eq!(row.get::<_, i64>(4), 1_000_001);
    });
}

#[test]
fn pg_round_trip_users() {
    let Some(cluster) = cluster_or_skip("pg_round_trip_users") else {
        return;
    };

    pg_test!("pg_round_trip_users", &cluster.dsn, |client| {
        client
            .execute(
                "INSERT INTO users (id, display_name, email, created_at_ms)
                 VALUES ($1, $2, $3, $4)",
                &[&"u-001", &"Alice", &"alice@example.com", &2_000_000i64],
            )
            .await
            .expect("insert should succeed");

        let row = client
            .query_one(
                "SELECT id, display_name, email, created_at_ms, suspended_at_ms
                 FROM users WHERE id = $1",
                &[&"u-001"],
            )
            .await
            .expect("SELECT should succeed");

        assert_eq!(row.get::<_, &str>(0), "u-001");
        assert_eq!(row.get::<_, &str>(1), "Alice");
        assert_eq!(row.get::<_, Option<&str>>(2), Some("alice@example.com"));
        assert_eq!(row.get::<_, i64>(3), 2_000_000);
        assert!(row.get::<_, Option<i64>>(4).is_none());
    });
}

#[test]
fn pg_round_trip_user_identities() {
    let Some(cluster) = cluster_or_skip("pg_round_trip_user_identities") else {
        return;
    };

    pg_test!("pg_round_trip_user_identities", &cluster.dsn, |client| {
        client
            .execute(
                "INSERT INTO users (id, display_name, created_at_ms) VALUES ($1, $2, $3)",
                &[&"u-001", &"Alice", &1_000_000i64],
            )
            .await
            .unwrap();

        client
            .execute(
                "INSERT INTO user_identities (provider, external_id, user_id, created_at_ms)
                 VALUES ($1, $2, $3, $4)",
                &[&"tailscale", &"alice@tailnet", &"u-001", &3_000_000i64],
            )
            .await
            .expect("insert should succeed");

        let row = client
            .query_one(
                "SELECT provider, external_id, user_id, created_at_ms
                 FROM user_identities WHERE provider = $1 AND external_id = $2",
                &[&"tailscale", &"alice@tailnet"],
            )
            .await
            .expect("SELECT should succeed");

        assert_eq!(row.get::<_, &str>(0), "tailscale");
        assert_eq!(row.get::<_, &str>(1), "alice@tailnet");
        assert_eq!(row.get::<_, &str>(2), "u-001");
        assert_eq!(row.get::<_, i64>(3), 3_000_000);
    });
}

#[test]
fn pg_round_trip_tenant_users() {
    let Some(cluster) = cluster_or_skip("pg_round_trip_tenant_users") else {
        return;
    };

    pg_test!("pg_round_trip_tenant_users", &cluster.dsn, |client| {
        client
            .execute(
                "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
                 VALUES ($1, $2, $3, $4, $5)",
                &[&"t-001", &"acme", &"Acme Corp", &1_000_000i64, &1_000_000i64],
            )
            .await
            .unwrap();
        client
            .execute(
                "INSERT INTO users (id, display_name, created_at_ms) VALUES ($1, $2, $3)",
                &[&"u-001", &"Alice", &1_000_000i64],
            )
            .await
            .unwrap();

        client
            .execute(
                "INSERT INTO tenant_users (tenant_id, user_id, role, added_at_ms)
                 VALUES ($1, $2, $3, $4)",
                &[&"t-001", &"u-001", &"admin", &4_000_000i64],
            )
            .await
            .expect("insert should succeed");

        let row = client
            .query_one(
                "SELECT tenant_id, user_id, role, added_at_ms
                 FROM tenant_users WHERE tenant_id = $1 AND user_id = $2",
                &[&"t-001", &"u-001"],
            )
            .await
            .expect("SELECT should succeed");

        assert_eq!(row.get::<_, &str>(0), "t-001");
        assert_eq!(row.get::<_, &str>(1), "u-001");
        assert_eq!(row.get::<_, &str>(2), "admin");
        assert_eq!(row.get::<_, i64>(3), 4_000_000);
    });
}

#[test]
fn pg_round_trip_tenant_databases() {
    let Some(cluster) = cluster_or_skip("pg_round_trip_tenant_databases") else {
        return;
    };

    pg_test!("pg_round_trip_tenant_databases", &cluster.dsn, |client| {
        client
            .execute(
                "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
                 VALUES ($1, $2, $3, $4, $5)",
                &[&"t-001", &"acme", &"Acme Corp", &1_000_000i64, &1_000_000i64],
            )
            .await
            .unwrap();

        client
            .execute(
                "INSERT INTO tenant_databases (tenant_id, database_name, created_at_ms)
                 VALUES ($1, $2, $3)",
                &[&"t-001", &"orders", &5_000_000i64],
            )
            .await
            .expect("insert should succeed");

        let row = client
            .query_one(
                "SELECT tenant_id, database_name, created_at_ms
                 FROM tenant_databases WHERE tenant_id = $1 AND database_name = $2",
                &[&"t-001", &"orders"],
            )
            .await
            .expect("SELECT should succeed");

        assert_eq!(row.get::<_, &str>(0), "t-001");
        assert_eq!(row.get::<_, &str>(1), "orders");
        assert_eq!(row.get::<_, i64>(2), 5_000_000);
    });
}

#[test]
fn pg_round_trip_credential_revocations() {
    let Some(cluster) = cluster_or_skip("pg_round_trip_credential_revocations") else {
        return;
    };

    pg_test!("pg_round_trip_credential_revocations", &cluster.dsn, |client| {
        client
            .execute(
                "INSERT INTO credential_revocations (jti, revoked_at_ms, revoked_by)
                 VALUES ($1, $2, $3)",
                &[&"jti-abc123", &6_000_000i64, &"u-001"],
            )
            .await
            .expect("insert should succeed");

        let row = client
            .query_one(
                "SELECT jti, revoked_at_ms, revoked_by
                 FROM credential_revocations WHERE jti = $1",
                &[&"jti-abc123"],
            )
            .await
            .expect("SELECT should succeed");

        assert_eq!(row.get::<_, &str>(0), "jti-abc123");
        assert_eq!(row.get::<_, i64>(1), 6_000_000);
        assert_eq!(row.get::<_, Option<&str>>(2), Some("u-001"));
    });
}

// ── Constraint tests ──────────────────────────────────────────────────────────

#[test]
fn pg_unique_tenant_name_rejected_on_duplicate() {
    let Some(cluster) = cluster_or_skip("pg_unique_tenant_name_rejected_on_duplicate") else {
        return;
    };

    pg_test!(
        "pg_unique_tenant_name_rejected_on_duplicate",
        &cluster.dsn,
        |client| {
            client
                .execute(
                    "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
                     VALUES ($1, $2, $3, $4, $5)",
                    &[
                        &"t-001",
                        &"dup-tenant",
                        &"Tenant One",
                        &1_000_000i64,
                        &1_000_000i64,
                    ],
                )
                .await
                .expect("first insert should succeed");

            let result = client
                .execute(
                    "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
                     VALUES ($1, $2, $3, $4, $5)",
                    &[
                        &"t-002",
                        &"dup-tenant",
                        &"Tenant Duplicate",
                        &2_000_000i64,
                        &2_000_000i64,
                    ],
                )
                .await;
            assert!(result.is_err(), "duplicate tenant name should be rejected");
        }
    );
}

#[test]
fn pg_unique_user_identity_rejected_on_duplicate() {
    let Some(cluster) = cluster_or_skip("pg_unique_user_identity_rejected_on_duplicate") else {
        return;
    };

    pg_test!(
        "pg_unique_user_identity_rejected_on_duplicate",
        &cluster.dsn,
        |client| {
            client
                .execute(
                    "INSERT INTO users (id, display_name, created_at_ms) VALUES ($1, $2, $3)",
                    &[&"u-001", &"Alice", &1_000_000i64],
                )
                .await
                .unwrap();

            client
                .execute(
                    "INSERT INTO user_identities (provider, external_id, user_id, created_at_ms)
                     VALUES ($1, $2, $3, $4)",
                    &[&"tailscale", &"dup@tailnet", &"u-001", &1_000_000i64],
                )
                .await
                .expect("first identity should succeed");

            let result = client
                .execute(
                    "INSERT INTO user_identities (provider, external_id, user_id, created_at_ms)
                     VALUES ($1, $2, $3, $4)",
                    &[&"tailscale", &"dup@tailnet", &"u-001", &2_000_000i64],
                )
                .await;
            assert!(
                result.is_err(),
                "duplicate provider+external_id should be rejected"
            );
        }
    );
}

#[test]
fn pg_tenant_user_composite_pk_rejected_on_duplicate() {
    let Some(cluster) =
        cluster_or_skip("pg_tenant_user_composite_pk_rejected_on_duplicate")
    else {
        return;
    };

    pg_test!(
        "pg_tenant_user_composite_pk_rejected_on_duplicate",
        &cluster.dsn,
        |client| {
            client
                .execute(
                    "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
                     VALUES ($1, $2, $3, $4, $5)",
                    &[
                        &"t-001",
                        &"dup-tenant-u",
                        &"Tenant",
                        &1_000_000i64,
                        &1_000_000i64,
                    ],
                )
                .await
                .unwrap();
            client
                .execute(
                    "INSERT INTO users (id, display_name, created_at_ms) VALUES ($1, $2, $3)",
                    &[&"u-001", &"Alice", &1_000_000i64],
                )
                .await
                .unwrap();

            client
                .execute(
                    "INSERT INTO tenant_users (tenant_id, user_id, role, added_at_ms)
                     VALUES ($1, $2, $3, $4)",
                    &[&"t-001", &"u-001", &"admin", &1_000_000i64],
                )
                .await
                .expect("first membership should succeed");

            let result = client
                .execute(
                    "INSERT INTO tenant_users (tenant_id, user_id, role, added_at_ms)
                     VALUES ($1, $2, $3, $4)",
                    &[&"t-001", &"u-001", &"write", &2_000_000i64],
                )
                .await;
            assert!(
                result.is_err(),
                "duplicate tenant+user membership should be rejected"
            );
        }
    );
}
