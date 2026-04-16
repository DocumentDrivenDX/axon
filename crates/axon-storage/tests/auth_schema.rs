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
use sqlx::Row;
use testcontainers_modules::{
    postgres,
    testcontainers::{runners::SyncRunner, Container},
};

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
            let dsn = format!("postgres://postgres:postgres@{host}:{port}/postgres");
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

// Wrapper that runs an async block synchronously using a fresh single-threaded runtime.
macro_rules! pg_test {
    ($test_name:expr, $dsn:expr, |$pool:ident| $body:block) => {{
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build");
        rt.block_on(async {
            let $pool = sqlx::PgPool::connect($dsn)
                .await
                .expect("postgres connect should succeed");
            apply_auth_migrations_postgres(&$pool)
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

    pg_test!("pg_creates_all_six_auth_tables", &cluster.dsn, |pool| {
        let row = sqlx::query(
            "SELECT COUNT(*) FROM information_schema.tables
             WHERE table_schema = 'public'
               AND table_name IN (
                   'tenants', 'users', 'user_identities', 'tenant_users',
                   'tenant_databases', 'credential_revocations'
               )",
        )
        .fetch_one(&pool)
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

    pg_test!("pg_audit_retention_policies_absent", &cluster.dsn, |pool| {
        let row = sqlx::query(
            "SELECT COUNT(*) FROM information_schema.tables
             WHERE table_schema = 'public'
               AND table_name = 'audit_retention_policies'",
        )
        .fetch_one(&pool)
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
        let pool = sqlx::PgPool::connect(&cluster.dsn)
            .await
            .expect("connect should succeed");

        apply_auth_migrations_postgres(&pool)
            .await
            .expect("first migration should succeed");
        apply_auth_migrations_postgres(&pool)
            .await
            .expect("second migration should be idempotent");

        let row = sqlx::query(
            "SELECT COUNT(*) FROM information_schema.tables
             WHERE table_schema = 'public'
               AND table_name IN (
                   'tenants', 'users', 'user_identities', 'tenant_users',
                   'tenant_databases', 'credential_revocations'
               )",
        )
        .fetch_one(&pool)
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

    pg_test!("pg_round_trip_tenants", &cluster.dsn, |pool| {
        sqlx::query(
            "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind("t-001")
        .bind("acme")
        .bind("Acme Corp")
        .bind(1_000_000i64)
        .bind(1_000_001i64)
        .execute(&pool)
        .await
        .expect("insert should succeed");

        let row = sqlx::query(
            "SELECT id, name, display_name, created_at_ms, updated_at_ms
             FROM tenants WHERE id = $1",
        )
        .bind("t-001")
        .fetch_one(&pool)
        .await
        .expect("SELECT should succeed");

        assert_eq!(row.get::<String, _>(0), "t-001");
        assert_eq!(row.get::<String, _>(1), "acme");
        assert_eq!(row.get::<String, _>(2), "Acme Corp");
        assert_eq!(row.get::<i64, _>(3), 1_000_000);
        assert_eq!(row.get::<i64, _>(4), 1_000_001);
    });
}

#[test]
fn pg_round_trip_users() {
    let Some(cluster) = cluster_or_skip("pg_round_trip_users") else {
        return;
    };

    pg_test!("pg_round_trip_users", &cluster.dsn, |pool| {
        sqlx::query(
            "INSERT INTO users (id, display_name, email, created_at_ms)
             VALUES ($1, $2, $3, $4)",
        )
        .bind("u-001")
        .bind("Alice")
        .bind("alice@example.com")
        .bind(2_000_000i64)
        .execute(&pool)
        .await
        .expect("insert should succeed");

        let row = sqlx::query(
            "SELECT id, display_name, email, created_at_ms, suspended_at_ms
             FROM users WHERE id = $1",
        )
        .bind("u-001")
        .fetch_one(&pool)
        .await
        .expect("SELECT should succeed");

        assert_eq!(row.get::<String, _>(0), "u-001");
        assert_eq!(row.get::<String, _>(1), "Alice");
        assert_eq!(
            row.get::<Option<String>, _>(2),
            Some("alice@example.com".to_string())
        );
        assert_eq!(row.get::<i64, _>(3), 2_000_000);
        assert!(row.get::<Option<i64>, _>(4).is_none());
    });
}

#[test]
fn pg_round_trip_user_identities() {
    let Some(cluster) = cluster_or_skip("pg_round_trip_user_identities") else {
        return;
    };

    pg_test!("pg_round_trip_user_identities", &cluster.dsn, |pool| {
        sqlx::query("INSERT INTO users (id, display_name, created_at_ms) VALUES ($1, $2, $3)")
            .bind("u-001")
            .bind("Alice")
            .bind(1_000_000i64)
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO user_identities (provider, external_id, user_id, created_at_ms)
             VALUES ($1, $2, $3, $4)",
        )
        .bind("tailscale")
        .bind("alice@tailnet")
        .bind("u-001")
        .bind(3_000_000i64)
        .execute(&pool)
        .await
        .expect("insert should succeed");

        let row = sqlx::query(
            "SELECT provider, external_id, user_id, created_at_ms
             FROM user_identities WHERE provider = $1 AND external_id = $2",
        )
        .bind("tailscale")
        .bind("alice@tailnet")
        .fetch_one(&pool)
        .await
        .expect("SELECT should succeed");

        assert_eq!(row.get::<String, _>(0), "tailscale");
        assert_eq!(row.get::<String, _>(1), "alice@tailnet");
        assert_eq!(row.get::<String, _>(2), "u-001");
        assert_eq!(row.get::<i64, _>(3), 3_000_000);
    });
}

#[test]
fn pg_round_trip_tenant_users() {
    let Some(cluster) = cluster_or_skip("pg_round_trip_tenant_users") else {
        return;
    };

    pg_test!("pg_round_trip_tenant_users", &cluster.dsn, |pool| {
        sqlx::query(
            "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind("t-001")
        .bind("acme")
        .bind("Acme Corp")
        .bind(1_000_000i64)
        .bind(1_000_000i64)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO users (id, display_name, created_at_ms) VALUES ($1, $2, $3)")
            .bind("u-001")
            .bind("Alice")
            .bind(1_000_000i64)
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO tenant_users (tenant_id, user_id, role, added_at_ms)
             VALUES ($1, $2, $3, $4)",
        )
        .bind("t-001")
        .bind("u-001")
        .bind("admin")
        .bind(4_000_000i64)
        .execute(&pool)
        .await
        .expect("insert should succeed");

        let row = sqlx::query(
            "SELECT tenant_id, user_id, role, added_at_ms
             FROM tenant_users WHERE tenant_id = $1 AND user_id = $2",
        )
        .bind("t-001")
        .bind("u-001")
        .fetch_one(&pool)
        .await
        .expect("SELECT should succeed");

        assert_eq!(row.get::<String, _>(0), "t-001");
        assert_eq!(row.get::<String, _>(1), "u-001");
        assert_eq!(row.get::<String, _>(2), "admin");
        assert_eq!(row.get::<i64, _>(3), 4_000_000);
    });
}

#[test]
fn pg_round_trip_tenant_databases() {
    let Some(cluster) = cluster_or_skip("pg_round_trip_tenant_databases") else {
        return;
    };

    pg_test!("pg_round_trip_tenant_databases", &cluster.dsn, |pool| {
        sqlx::query(
            "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind("t-001")
        .bind("acme")
        .bind("Acme Corp")
        .bind(1_000_000i64)
        .bind(1_000_000i64)
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO tenant_databases (tenant_id, database_name, created_at_ms)
             VALUES ($1, $2, $3)",
        )
        .bind("t-001")
        .bind("orders")
        .bind(5_000_000i64)
        .execute(&pool)
        .await
        .expect("insert should succeed");

        let row = sqlx::query(
            "SELECT tenant_id, database_name, created_at_ms
             FROM tenant_databases WHERE tenant_id = $1 AND database_name = $2",
        )
        .bind("t-001")
        .bind("orders")
        .fetch_one(&pool)
        .await
        .expect("SELECT should succeed");

        assert_eq!(row.get::<String, _>(0), "t-001");
        assert_eq!(row.get::<String, _>(1), "orders");
        assert_eq!(row.get::<i64, _>(2), 5_000_000);
    });
}

#[test]
fn pg_round_trip_credential_revocations() {
    let Some(cluster) = cluster_or_skip("pg_round_trip_credential_revocations") else {
        return;
    };

    pg_test!(
        "pg_round_trip_credential_revocations",
        &cluster.dsn,
        |pool| {
            sqlx::query(
                "INSERT INTO credential_revocations (jti, revoked_at_ms, revoked_by)
             VALUES ($1, $2, $3)",
            )
            .bind("jti-abc123")
            .bind(6_000_000i64)
            .bind("u-001")
            .execute(&pool)
            .await
            .expect("insert should succeed");

            let row = sqlx::query(
                "SELECT jti, revoked_at_ms, revoked_by
             FROM credential_revocations WHERE jti = $1",
            )
            .bind("jti-abc123")
            .fetch_one(&pool)
            .await
            .expect("SELECT should succeed");

            assert_eq!(row.get::<String, _>(0), "jti-abc123");
            assert_eq!(row.get::<i64, _>(1), 6_000_000);
            assert_eq!(row.get::<Option<String>, _>(2), Some("u-001".to_string()));
        }
    );
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
        |pool| {
            sqlx::query(
                "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind("t-001")
            .bind("dup-tenant")
            .bind("Tenant One")
            .bind(1_000_000i64)
            .bind(1_000_000i64)
            .execute(&pool)
            .await
            .expect("first insert should succeed");

            let result = sqlx::query(
                "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind("t-002")
            .bind("dup-tenant")
            .bind("Tenant Duplicate")
            .bind(2_000_000i64)
            .bind(2_000_000i64)
            .execute(&pool)
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
        |pool| {
            sqlx::query("INSERT INTO users (id, display_name, created_at_ms) VALUES ($1, $2, $3)")
                .bind("u-001")
                .bind("Alice")
                .bind(1_000_000i64)
                .execute(&pool)
                .await
                .unwrap();

            sqlx::query(
                "INSERT INTO user_identities (provider, external_id, user_id, created_at_ms)
                 VALUES ($1, $2, $3, $4)",
            )
            .bind("tailscale")
            .bind("dup@tailnet")
            .bind("u-001")
            .bind(1_000_000i64)
            .execute(&pool)
            .await
            .expect("first identity should succeed");

            let result = sqlx::query(
                "INSERT INTO user_identities (provider, external_id, user_id, created_at_ms)
                 VALUES ($1, $2, $3, $4)",
            )
            .bind("tailscale")
            .bind("dup@tailnet")
            .bind("u-001")
            .bind(2_000_000i64)
            .execute(&pool)
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
    let Some(cluster) = cluster_or_skip("pg_tenant_user_composite_pk_rejected_on_duplicate") else {
        return;
    };

    pg_test!(
        "pg_tenant_user_composite_pk_rejected_on_duplicate",
        &cluster.dsn,
        |pool| {
            sqlx::query(
                "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind("t-001")
            .bind("dup-tenant-u")
            .bind("Tenant")
            .bind(1_000_000i64)
            .bind(1_000_000i64)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query("INSERT INTO users (id, display_name, created_at_ms) VALUES ($1, $2, $3)")
                .bind("u-001")
                .bind("Alice")
                .bind(1_000_000i64)
                .execute(&pool)
                .await
                .unwrap();

            sqlx::query(
                "INSERT INTO tenant_users (tenant_id, user_id, role, added_at_ms)
                 VALUES ($1, $2, $3, $4)",
            )
            .bind("t-001")
            .bind("u-001")
            .bind("admin")
            .bind(1_000_000i64)
            .execute(&pool)
            .await
            .expect("first membership should succeed");

            let result = sqlx::query(
                "INSERT INTO tenant_users (tenant_id, user_id, role, added_at_ms)
                 VALUES ($1, $2, $3, $4)",
            )
            .bind("t-001")
            .bind("u-001")
            .bind("write")
            .bind(2_000_000i64)
            .execute(&pool)
            .await;
            assert!(
                result.is_err(),
                "duplicate tenant+user membership should be rejected"
            );
        }
    );
}
