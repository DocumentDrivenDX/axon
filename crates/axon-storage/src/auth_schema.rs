//! SQL migrations for auth/tenancy tables (FEAT-014, ADR-018).
//!
//! Adds tables for:
//! - tenants: global account boundary
//! - users: global user identities
//! - user_identities: federated identity mapping
//! - tenant_users: M:N tenant membership
//! - tenant_databases: tenant-to-database mapping
//! - credential_revocations: revoked JWTs
//!
//! Note: `audit_retention_policies` is intentionally NOT included here — it
//! belongs to F1 audit-attribution (a separate bead / feature).

/// Apply auth/tenancy migrations to a SQLite pool (via sqlx).
///
/// This is idempotent — running it multiple times on the same database
/// produces the same result without errors or duplicate rows.
pub fn apply_auth_migrations_sqlite(
    pool: &sqlx::SqlitePool,
    owned_rt: Option<&tokio::runtime::Runtime>,
) -> Result<(), String> {
    let run = |sql: &str, label: &str| -> Result<(), String> {
        let fut = sqlx::query(sql).execute(pool);
        let result = match tokio::runtime::Handle::try_current() {
            Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
            Err(_) => owned_rt
                .expect("no tokio runtime available")
                .block_on(fut),
        };
        result.map_err(|e| format!("{label}: {e}"))?;
        Ok(())
    };

    // Create tenants table.
    run(
        "CREATE TABLE IF NOT EXISTS tenants (
            id                 TEXT PRIMARY KEY,
            name               TEXT NOT NULL UNIQUE,
            display_name       TEXT NOT NULL,
            created_at_ms      INTEGER NOT NULL,
            updated_at_ms      INTEGER NOT NULL
        )",
        "tenants table",
    )?;

    // Create users table.
    run(
        "CREATE TABLE IF NOT EXISTS users (
            id                 TEXT PRIMARY KEY,
            display_name       TEXT NOT NULL,
            email              TEXT,
            created_at_ms      INTEGER NOT NULL,
            suspended_at_ms    INTEGER
        )",
        "users table",
    )?;

    // Create user_identities table.
    run(
        "CREATE TABLE IF NOT EXISTS user_identities (
            provider       TEXT NOT NULL,
            external_id    TEXT NOT NULL,
            user_id        TEXT NOT NULL REFERENCES users(id),
            created_at_ms  INTEGER NOT NULL,
            PRIMARY KEY (provider, external_id)
        )",
        "user_identities table",
    )?;

    // Create tenant_users table.
    run(
        "CREATE TABLE IF NOT EXISTS tenant_users (
            tenant_id    TEXT NOT NULL REFERENCES tenants(id),
            user_id      TEXT NOT NULL REFERENCES users(id),
            role         TEXT NOT NULL,
            added_at_ms  INTEGER NOT NULL,
            PRIMARY KEY (tenant_id, user_id)
        )",
        "tenant_users table",
    )?;

    // Create tenant_databases table.
    run(
        "CREATE TABLE IF NOT EXISTS tenant_databases (
            tenant_id      TEXT NOT NULL REFERENCES tenants(id),
            database_name  TEXT NOT NULL,
            created_at_ms  INTEGER NOT NULL,
            PRIMARY KEY (tenant_id, database_name)
        )",
        "tenant_databases table",
    )?;

    // Create credential_revocations table.
    run(
        "CREATE TABLE IF NOT EXISTS credential_revocations (
            jti            TEXT PRIMARY KEY,
            revoked_at_ms  INTEGER NOT NULL,
            revoked_by     TEXT
        )",
        "credential_revocations table",
    )?;

    // Create tenant_retention_policies table (axon-c6908e78).
    run(
        "CREATE TABLE IF NOT EXISTS tenant_retention_policies (
            tenant_id                TEXT NOT NULL,
            archive_after_seconds    INTEGER NOT NULL,
            purge_after_seconds      INTEGER,
            updated_at_ms            INTEGER NOT NULL,
            PRIMARY KEY (tenant_id)
        )",
        "tenant_retention_policies table",
    )?;

    // Create credential_issuances table (axon-906b527a).
    run(
        "CREATE TABLE IF NOT EXISTS credential_issuances (
            jti            TEXT PRIMARY KEY,
            user_id        TEXT NOT NULL,
            tenant_id      TEXT NOT NULL,
            issued_at_ms   INTEGER NOT NULL,
            expires_at_ms  INTEGER NOT NULL,
            grants_json    TEXT NOT NULL
        )",
        "credential_issuances table",
    )?;

    Ok(())
}

/// Apply auth/tenancy migrations to a PostgreSQL connection.
///
/// This is idempotent — running it multiple times on the same database
/// produces the same result without errors or duplicate rows.
pub async fn apply_auth_migrations_postgres(pool: &sqlx::PgPool) -> Result<(), String> {
    // Create tenants table.
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS tenants (
            id                 TEXT PRIMARY KEY,
            name               TEXT NOT NULL UNIQUE,
            display_name       TEXT NOT NULL,
            created_at_ms      BIGINT NOT NULL,
            updated_at_ms      BIGINT NOT NULL
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("tenants table: {e}"))?;

    // Create users table.
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS users (
            id                 TEXT PRIMARY KEY,
            display_name       TEXT NOT NULL,
            email              TEXT,
            created_at_ms      BIGINT NOT NULL,
            suspended_at_ms    BIGINT
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("users table: {e}"))?;

    // Create user_identities table.
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS user_identities (
            provider       TEXT NOT NULL,
            external_id    TEXT NOT NULL,
            user_id        TEXT NOT NULL REFERENCES users(id),
            created_at_ms  BIGINT NOT NULL,
            PRIMARY KEY (provider, external_id)
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("user_identities table: {e}"))?;

    // Create tenant_users table.
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS tenant_users (
            tenant_id    TEXT NOT NULL REFERENCES tenants(id),
            user_id      TEXT NOT NULL REFERENCES users(id),
            role         TEXT NOT NULL,
            added_at_ms  BIGINT NOT NULL,
            PRIMARY KEY (tenant_id, user_id)
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("tenant_users table: {e}"))?;

    // Create tenant_databases table.
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS tenant_databases (
            tenant_id      TEXT NOT NULL REFERENCES tenants(id),
            database_name  TEXT NOT NULL,
            created_at_ms  BIGINT NOT NULL,
            PRIMARY KEY (tenant_id, database_name)
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("tenant_databases table: {e}"))?;

    // Create credential_revocations table.
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS credential_revocations (
            jti            TEXT PRIMARY KEY,
            revoked_at_ms  BIGINT NOT NULL,
            revoked_by     TEXT
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("credential_revocations table: {e}"))?;

    // Create tenant_retention_policies table.
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS tenant_retention_policies (
            tenant_id                TEXT NOT NULL,
            archive_after_seconds    BIGINT NOT NULL,
            purge_after_seconds      BIGINT,
            updated_at_ms            BIGINT NOT NULL,
            PRIMARY KEY (tenant_id)
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("tenant_retention_policies table: {e}"))?;

    // Create credential_issuances table.
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS credential_issuances (
            jti            TEXT PRIMARY KEY,
            user_id        TEXT NOT NULL,
            tenant_id      TEXT NOT NULL,
            issued_at_ms   BIGINT NOT NULL,
            expires_at_ms  BIGINT NOT NULL,
            grants_json    TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("credential_issuances table: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::Row;

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn open_and_migrate() -> (sqlx::SqlitePool, Option<tokio::runtime::Runtime>) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime should build");
        let pool = rt
            .block_on(
                sqlx::sqlite::SqlitePoolOptions::new()
                    .max_connections(1)
                    .connect("sqlite::memory:"),
            )
            .expect("in-memory DB should open");
        let rt = Some(rt);
        apply_auth_migrations_sqlite(&pool, rt.as_ref()).expect("migrations should succeed");
        (pool, rt)
    }

    fn query_i64(pool: &sqlx::SqlitePool, rt: Option<&tokio::runtime::Runtime>, sql: &str) -> i64 {
        rt.as_ref()
            .expect("runtime required")
            .block_on(sqlx::query_scalar::<_, i64>(sql).fetch_one(pool))
            .expect("query should succeed")
    }

    fn exec(pool: &sqlx::SqlitePool, rt: Option<&tokio::runtime::Runtime>, sql: &str, binds: &[BindVal]) {
        let mut q = sqlx::query(sql);
        for b in binds {
            match b {
                BindVal::Str(s) => q = q.bind(s.as_str()),
                BindVal::I64(n) => q = q.bind(*n),
                BindVal::Null => q = q.bind(Option::<String>::None),
            }
        }
        rt.as_ref().expect("runtime required").block_on(q.execute(pool)).expect("exec should succeed");
    }

    fn exec_result(
        pool: &sqlx::SqlitePool,
        rt: Option<&tokio::runtime::Runtime>,
        sql: &str,
        binds: &[BindVal],
    ) -> Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
        let mut q = sqlx::query(sql);
        for b in binds {
            match b {
                BindVal::Str(s) => q = q.bind(s.as_str()),
                BindVal::I64(n) => q = q.bind(*n),
                BindVal::Null => q = q.bind(Option::<String>::None),
            }
        }
        rt.as_ref().expect("runtime required").block_on(q.execute(pool))
    }

    enum BindVal {
        Str(String),
        I64(i64),
        Null,
    }

    fn s(val: &str) -> BindVal {
        BindVal::Str(val.to_string())
    }
    fn i(val: i64) -> BindVal {
        BindVal::I64(val)
    }

    // ── Schema existence ──────────────────────────────────────────────────────

    #[test]
    fn apply_sqlite_migrations_creates_all_tables() {
        let (pool, rt) = open_and_migrate();

        let count = query_i64(
            &pool,
            rt.as_ref(),
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN (
                'tenants', 'users', 'user_identities', 'tenant_users',
                'tenant_databases', 'credential_revocations',
                'tenant_retention_policies', 'credential_issuances'
            )",
        );
        assert_eq!(count, 8, "all 8 auth tables should be created");
    }

    #[test]
    fn audit_retention_policies_table_absent() {
        let (pool, rt) = open_and_migrate();
        let count = query_i64(
            &pool,
            rt.as_ref(),
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='audit_retention_policies'",
        );
        assert_eq!(
            count, 0,
            "audit_retention_policies must NOT be created here (F1 territory)"
        );
    }

    #[test]
    fn apply_sqlite_migrations_is_idempotent() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime should build");
        let pool = rt
            .block_on(
                sqlx::sqlite::SqlitePoolOptions::new()
                    .max_connections(1)
                    .connect("sqlite::memory:"),
            )
            .expect("in-memory DB should open");
        let rt = Some(rt);
        apply_auth_migrations_sqlite(&pool, rt.as_ref()).expect("first migration should succeed");
        apply_auth_migrations_sqlite(&pool, rt.as_ref()).expect("second migration should be idempotent");

        let count = query_i64(
            &pool,
            rt.as_ref(),
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN (
                'tenants', 'users', 'user_identities', 'tenant_users',
                'tenant_databases', 'credential_revocations',
                'tenant_retention_policies', 'credential_issuances'
            )",
        );
        assert_eq!(count, 8, "still exactly 8 tables after re-run");
    }

    // ── Round-trip tests ──────────────────────────────────────────────────────

    #[test]
    fn round_trip_tenants() {
        let (pool, rt) = open_and_migrate();

        exec(
            &pool,
            rt.as_ref(),
            "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            &[
                s("t-001"),
                s("acme"),
                s("Acme Corp"),
                i(1_000_000),
                i(1_000_001),
            ],
        );

        let row = rt.as_ref().expect("runtime required")
            .block_on(
                sqlx::query(
                    "SELECT id, name, display_name, created_at_ms, updated_at_ms
                     FROM tenants WHERE id = ?1",
                )
                .bind("t-001")
                .fetch_one(&pool),
            )
            .expect("SELECT should succeed");

        assert_eq!(row.get::<String, _>("id"), "t-001");
        assert_eq!(row.get::<String, _>("name"), "acme");
        assert_eq!(row.get::<String, _>("display_name"), "Acme Corp");
        assert_eq!(row.get::<i64, _>("created_at_ms"), 1_000_000);
        assert_eq!(row.get::<i64, _>("updated_at_ms"), 1_000_001);
    }

    #[test]
    fn round_trip_users() {
        let (pool, rt) = open_and_migrate();

        exec(
            &pool,
            rt.as_ref(),
            "INSERT INTO users (id, display_name, email, created_at_ms, suspended_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            &[
                s("u-001"),
                s("Alice"),
                s("alice@example.com"),
                i(2_000_000),
                BindVal::Null,
            ],
        );

        let row = rt.as_ref().expect("runtime required")
            .block_on(
                sqlx::query(
                    "SELECT id, display_name, email, created_at_ms, suspended_at_ms
                     FROM users WHERE id = ?1",
                )
                .bind("u-001")
                .fetch_one(&pool),
            )
            .expect("SELECT should succeed");

        assert_eq!(row.get::<String, _>("id"), "u-001");
        assert_eq!(row.get::<String, _>("display_name"), "Alice");
        assert_eq!(
            row.get::<Option<String>, _>("email").as_deref(),
            Some("alice@example.com")
        );
        assert_eq!(row.get::<i64, _>("created_at_ms"), 2_000_000);
        assert!(row.get::<Option<i64>, _>("suspended_at_ms").is_none());
    }

    #[test]
    fn round_trip_user_identities() {
        let (pool, rt) = open_and_migrate();

        // Need a parent user first.
        exec(
            &pool,
            rt.as_ref(),
            "INSERT INTO users (id, display_name, created_at_ms) VALUES (?1, ?2, ?3)",
            &[s("u-001"), s("Alice"), i(1_000_000)],
        );

        exec(
            &pool,
            rt.as_ref(),
            "INSERT INTO user_identities (provider, external_id, user_id, created_at_ms)
             VALUES (?1, ?2, ?3, ?4)",
            &[s("tailscale"), s("alice@tailnet"), s("u-001"), i(3_000_000)],
        );

        let row = rt.as_ref().expect("runtime required")
            .block_on(
                sqlx::query(
                    "SELECT provider, external_id, user_id, created_at_ms
                     FROM user_identities WHERE provider = ?1 AND external_id = ?2",
                )
                .bind("tailscale")
                .bind("alice@tailnet")
                .fetch_one(&pool),
            )
            .expect("SELECT should succeed");

        assert_eq!(row.get::<String, _>("provider"), "tailscale");
        assert_eq!(row.get::<String, _>("external_id"), "alice@tailnet");
        assert_eq!(row.get::<String, _>("user_id"), "u-001");
        assert_eq!(row.get::<i64, _>("created_at_ms"), 3_000_000);
    }

    #[test]
    fn round_trip_tenant_users() {
        let (pool, rt) = open_and_migrate();

        exec(
            &pool,
            rt.as_ref(),
            "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            &[
                s("t-001"),
                s("acme"),
                s("Acme Corp"),
                i(1_000_000),
                i(1_000_000),
            ],
        );
        exec(
            &pool,
            rt.as_ref(),
            "INSERT INTO users (id, display_name, created_at_ms) VALUES (?1, ?2, ?3)",
            &[s("u-001"), s("Alice"), i(1_000_000)],
        );

        exec(
            &pool,
            rt.as_ref(),
            "INSERT INTO tenant_users (tenant_id, user_id, role, added_at_ms)
             VALUES (?1, ?2, ?3, ?4)",
            &[s("t-001"), s("u-001"), s("admin"), i(4_000_000)],
        );

        let row = rt.as_ref().expect("runtime required")
            .block_on(
                sqlx::query(
                    "SELECT tenant_id, user_id, role, added_at_ms
                     FROM tenant_users WHERE tenant_id = ?1 AND user_id = ?2",
                )
                .bind("t-001")
                .bind("u-001")
                .fetch_one(&pool),
            )
            .expect("SELECT should succeed");

        assert_eq!(row.get::<String, _>("tenant_id"), "t-001");
        assert_eq!(row.get::<String, _>("user_id"), "u-001");
        assert_eq!(row.get::<String, _>("role"), "admin");
        assert_eq!(row.get::<i64, _>("added_at_ms"), 4_000_000);
    }

    #[test]
    fn round_trip_tenant_databases() {
        let (pool, rt) = open_and_migrate();

        exec(
            &pool,
            rt.as_ref(),
            "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            &[
                s("t-001"),
                s("acme"),
                s("Acme Corp"),
                i(1_000_000),
                i(1_000_000),
            ],
        );

        exec(
            &pool,
            rt.as_ref(),
            "INSERT INTO tenant_databases (tenant_id, database_name, created_at_ms)
             VALUES (?1, ?2, ?3)",
            &[s("t-001"), s("orders"), i(5_000_000)],
        );

        let row = rt.as_ref().expect("runtime required")
            .block_on(
                sqlx::query(
                    "SELECT tenant_id, database_name, created_at_ms
                     FROM tenant_databases WHERE tenant_id = ?1 AND database_name = ?2",
                )
                .bind("t-001")
                .bind("orders")
                .fetch_one(&pool),
            )
            .expect("SELECT should succeed");

        assert_eq!(row.get::<String, _>("tenant_id"), "t-001");
        assert_eq!(row.get::<String, _>("database_name"), "orders");
        assert_eq!(row.get::<i64, _>("created_at_ms"), 5_000_000);
    }

    #[test]
    fn round_trip_credential_revocations() {
        let (pool, rt) = open_and_migrate();

        exec(
            &pool,
            rt.as_ref(),
            "INSERT INTO credential_revocations (jti, revoked_at_ms, revoked_by)
             VALUES (?1, ?2, ?3)",
            &[s("jti-abc123"), i(6_000_000), s("u-001")],
        );

        let row = rt.as_ref().expect("runtime required")
            .block_on(
                sqlx::query(
                    "SELECT jti, revoked_at_ms, revoked_by
                     FROM credential_revocations WHERE jti = ?1",
                )
                .bind("jti-abc123")
                .fetch_one(&pool),
            )
            .expect("SELECT should succeed");

        assert_eq!(row.get::<String, _>("jti"), "jti-abc123");
        assert_eq!(row.get::<i64, _>("revoked_at_ms"), 6_000_000);
        assert_eq!(
            row.get::<Option<String>, _>("revoked_by").as_deref(),
            Some("u-001")
        );
    }

    // ── Constraint tests ──────────────────────────────────────────────────────

    #[test]
    fn unique_tenant_name_rejected_on_duplicate() {
        let (pool, rt) = open_and_migrate();

        exec(
            &pool,
            rt.as_ref(),
            "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            &[
                s("t-001"),
                s("tenant-1"),
                s("Tenant One"),
                i(1_000_000),
                i(1_000_000),
            ],
        );

        let result = exec_result(
            &pool,
            rt.as_ref(),
            "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            &[
                s("t-002"),
                s("tenant-1"),
                s("Tenant One Duplicate"),
                i(2_000_000),
                i(2_000_000),
            ],
        );
        assert!(result.is_err(), "duplicate tenant name should be rejected");
    }

    #[test]
    fn unique_user_identity_rejected_on_duplicate() {
        let (pool, rt) = open_and_migrate();

        exec(
            &pool,
            rt.as_ref(),
            "INSERT INTO users (id, display_name, created_at_ms) VALUES (?1, ?2, ?3)",
            &[s("u-001"), s("User One"), i(1_000_000)],
        );

        exec(
            &pool,
            rt.as_ref(),
            "INSERT INTO user_identities (provider, external_id, user_id, created_at_ms)
             VALUES (?1, ?2, ?3, ?4)",
            &[s("tailscale"), s("alice@tailnet"), s("u-001"), i(1_000_000)],
        );

        let result = exec_result(
            &pool,
            rt.as_ref(),
            "INSERT INTO user_identities (provider, external_id, user_id, created_at_ms)
             VALUES (?1, ?2, ?3, ?4)",
            &[s("tailscale"), s("alice@tailnet"), s("u-001"), i(2_000_000)],
        );
        assert!(
            result.is_err(),
            "duplicate provider+external_id should be rejected"
        );
    }

    #[test]
    fn tenant_user_composite_pk_rejected_on_duplicate() {
        let (pool, rt) = open_and_migrate();

        exec(
            &pool,
            rt.as_ref(),
            "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            &[
                s("t-001"),
                s("tenant-1"),
                s("Tenant One"),
                i(1_000_000),
                i(1_000_000),
            ],
        );
        exec(
            &pool,
            rt.as_ref(),
            "INSERT INTO users (id, display_name, created_at_ms) VALUES (?1, ?2, ?3)",
            &[s("u-001"), s("User One"), i(1_000_000)],
        );

        exec(
            &pool,
            rt.as_ref(),
            "INSERT INTO tenant_users (tenant_id, user_id, role, added_at_ms)
             VALUES (?1, ?2, ?3, ?4)",
            &[s("t-001"), s("u-001"), s("admin"), i(1_000_000)],
        );

        let result = exec_result(
            &pool,
            rt.as_ref(),
            "INSERT INTO tenant_users (tenant_id, user_id, role, added_at_ms)
             VALUES (?1, ?2, ?3, ?4)",
            &[s("t-001"), s("u-001"), s("write"), i(2_000_000)],
        );
        assert!(
            result.is_err(),
            "duplicate tenant+user membership should be rejected"
        );
    }
}
