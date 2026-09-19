//! Integration tests for PostgreSQL mutating routine EXECUTE boundaries.
//!
//! When `AXON_TEST_POSTGRES` is set, it is used as the superadmin DSN.
//! Otherwise the test attempts to start a PostgreSQL container via
//! `testcontainers`. If the container runtime is unavailable, the test is
//! skipped gracefully.

#![allow(clippy::unwrap_used)]

use axon_storage::postgres::{
    tenant_postgres_roles, POSTGRES_MUTATING_ROUTINE_NAME, POSTGRES_MUTATING_ROUTINE_SIGNATURE,
};
use axon_storage::{deprovision_postgres_database, provision_postgres_database, tenant_dsn};
use testcontainers_modules::{
    postgres,
    testcontainers::{runners::SyncRunner, Container},
};

struct TestPgCluster {
    superadmin_dsn: String,
    _container: Option<Container<postgres::Postgres>>,
}

struct TestTenant {
    superadmin_dsn: String,
    name: String,
    dsn: String,
    _cluster: TestPgCluster,
}

impl Drop for TestTenant {
    fn drop(&mut self) {
        let _ = deprovision_postgres_database(&self.superadmin_dsn, &self.name);
    }
}

fn cluster_or_skip(test_name: &str) -> Option<TestPgCluster> {
    if let Ok(dsn) = std::env::var("AXON_TEST_POSTGRES") {
        return Some(TestPgCluster {
            superadmin_dsn: dsn,
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

fn provisioned_tenant(test_name: &str) -> Option<TestTenant> {
    let cluster = cluster_or_skip(test_name)?;
    let name = format!(
        "rt_{}_{:06}",
        std::process::id(),
        unique_suffix() % 1_000_000
    );
    let superadmin_dsn = cluster.superadmin_dsn.clone();
    let _ = deprovision_postgres_database(&superadmin_dsn, &name);
    provision_postgres_database(&superadmin_dsn, &name)
        .expect("tenant database provisioning should succeed");
    let dsn = tenant_dsn(&superadmin_dsn, &name);
    Some(TestTenant {
        superadmin_dsn,
        name,
        dsn,
        _cluster: cluster,
    })
}

fn unique_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock should be after UNIX_EPOCH")
        .as_nanos()
}

fn quote_pg_ident(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    rt.block_on(future)
}

async fn open_pool(dsn: &str) -> sqlx::PgPool {
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(dsn)
        .await
        .expect("tenant database connection should open")
}

async fn set_role(pool: &sqlx::PgPool, role: &str) {
    sqlx::raw_sql(&format!("SET ROLE {}", quote_pg_ident(role)))
        .execute(pool)
        .await
        .expect("SET ROLE should succeed");
}

async fn reset_role(pool: &sqlx::PgPool) {
    sqlx::raw_sql("RESET ROLE")
        .execute(pool)
        .await
        .expect("RESET ROLE should succeed");
}

async fn execute_mutating_routine(
    pool: &sqlx::PgPool,
    tenant_id: &str,
    database_id: &str,
    intent_id: &str,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    sqlx::query(&format!(
        "SELECT {POSTGRES_MUTATING_ROUTINE_NAME}($1, $2, $3, $4, $5, $6, $7)"
    ))
    .bind(tenant_id)
    .bind(database_id)
    .bind(intent_id)
    .bind("approve")
    .bind("approved")
    .bind(123_i64)
    .bind(serde_json::json!({"intent": intent_id}))
    .execute(pool)
    .await
}

fn assert_permission_error(error: &sqlx::Error) {
    let sqlx::Error::Database(database_error) = error else {
        panic!("expected database permission error, got {error:?}");
    };
    assert_eq!(
        database_error.code().as_deref(),
        Some("42501"),
        "expected PostgreSQL insufficient_privilege error, got {database_error:?}"
    );
}

#[test]
fn postgres_mutating_routines_unavailable_to_runtime() {
    let Some(tenant) = provisioned_tenant("postgres_mutating_routines_unavailable_to_runtime")
    else {
        return;
    };
    let roles = tenant_postgres_roles(&tenant.name).expect("tenant roles should derive");

    block_on(async {
        let pool = open_pool(&tenant.dsn).await;
        let routine = format!(
            "public.{}",
            POSTGRES_MUTATING_ROUTINE_SIGNATURE.replace(", ", ",")
        );
        let runtime_can_execute: bool =
            sqlx::query_scalar("SELECT has_function_privilege($1, $2, 'EXECUTE')")
                .bind(&roles.runtime)
                .bind(&routine)
                .fetch_one(&pool)
                .await
                .expect("runtime privilege query should succeed");
        let capability_can_execute: bool =
            sqlx::query_scalar("SELECT has_function_privilege($1, $2, 'EXECUTE')")
                .bind(&roles.capability)
                .bind(&routine)
                .fetch_one(&pool)
                .await
                .expect("capability privilege query should succeed");

        assert!(
            !runtime_can_execute,
            "runtime role must not have EXECUTE on mutating routine"
        );
        assert!(
            capability_can_execute,
            "capability role must retain EXECUTE on mutating routine"
        );
    });
}

#[test]
fn postgres_runtime_role_cannot_execute_mutating_routine() {
    let Some(tenant) = provisioned_tenant("postgres_runtime_role_cannot_execute_mutating_routine")
    else {
        return;
    };
    let roles = tenant_postgres_roles(&tenant.name).expect("tenant roles should derive");

    block_on(async {
        let pool = open_pool(&tenant.dsn).await;
        set_role(&pool, &roles.runtime).await;
        let error = execute_mutating_routine(&pool, "tenant-a", "default", "intent-denied")
            .await
            .expect_err("runtime role must not execute mutating routine");
        assert_permission_error(&error);
        reset_role(&pool).await;
    });
}

#[test]
fn postgres_capability_role_can_execute_mutating_routine() {
    let Some(tenant) = provisioned_tenant("postgres_capability_role_can_execute_mutating_routine")
    else {
        return;
    };
    let roles = tenant_postgres_roles(&tenant.name).expect("tenant roles should derive");

    block_on(async {
        let pool = open_pool(&tenant.dsn).await;
        let intent_id = "intent-allowed";

        set_role(&pool, &roles.capability).await;
        execute_mutating_routine(&pool, "tenant-a", "default", intent_id)
            .await
            .expect("capability role should execute mutating routine");
        reset_role(&pool).await;

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM mutation_intents
             WHERE tenant_id = $1 AND database_id = $2 AND intent_id = $3",
        )
        .bind("tenant-a")
        .bind("default")
        .bind(intent_id)
        .fetch_one(&pool)
        .await
        .expect("mutation_intents row count should be readable");

        assert_eq!(count, 1, "capability routine call should persist mutation");
    });
}
