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

/// Apply auth/tenancy migrations to a SQLite connection.
///
/// This is idempotent — running it multiple times on the same database
/// produces the same result without errors or duplicate rows.
pub fn apply_auth_migrations_sqlite(conn: &rusqlite::Connection) -> Result<(), String> {
    // Create tenants table.
    // UNIQUE(name) enforced by the UNIQUE constraint on the name column.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS tenants (
            id                 TEXT PRIMARY KEY,
            name               TEXT NOT NULL UNIQUE,
            display_name       TEXT NOT NULL,
            created_at_ms      INTEGER NOT NULL,
            updated_at_ms      INTEGER NOT NULL
        )",
    )
    .map_err(|e| format!("tenants table: {e}"))?;

    // Create users table.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS users (
            id                 TEXT PRIMARY KEY,
            display_name       TEXT NOT NULL,
            email              TEXT,
            created_at_ms      INTEGER NOT NULL,
            suspended_at_ms    INTEGER
        )",
    )
    .map_err(|e| format!("users table: {e}"))?;

    // Create user_identities table.
    // PRIMARY KEY (provider, external_id) enforces UNIQUE(provider, external_id).
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS user_identities (
            provider       TEXT NOT NULL,
            external_id    TEXT NOT NULL,
            user_id        TEXT NOT NULL REFERENCES users(id),
            created_at_ms  INTEGER NOT NULL,
            PRIMARY KEY (provider, external_id)
        )",
    )
    .map_err(|e| format!("user_identities table: {e}"))?;

    // Create tenant_users table.
    // PRIMARY KEY (tenant_id, user_id) is the composite PK required by ADR-018.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS tenant_users (
            tenant_id    TEXT NOT NULL REFERENCES tenants(id),
            user_id      TEXT NOT NULL REFERENCES users(id),
            role         TEXT NOT NULL,
            added_at_ms  INTEGER NOT NULL,
            PRIMARY KEY (tenant_id, user_id)
        )",
    )
    .map_err(|e| format!("tenant_users table: {e}"))?;

    // Create tenant_databases table.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS tenant_databases (
            tenant_id      TEXT NOT NULL REFERENCES tenants(id),
            database_name  TEXT NOT NULL,
            created_at_ms  INTEGER NOT NULL,
            PRIMARY KEY (tenant_id, database_name)
        )",
    )
    .map_err(|e| format!("tenant_databases table: {e}"))?;

    // Create credential_revocations table.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS credential_revocations (
            jti            TEXT PRIMARY KEY,
            revoked_at_ms  INTEGER NOT NULL,
            revoked_by     TEXT
        )",
    )
    .map_err(|e| format!("credential_revocations table: {e}"))?;

    // Create tenant_retention_policies table (axon-c6908e78).
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS tenant_retention_policies (
            tenant_id                TEXT NOT NULL,
            archive_after_seconds    INTEGER NOT NULL,
            purge_after_seconds      INTEGER,
            updated_at_ms            INTEGER NOT NULL,
            PRIMARY KEY (tenant_id)
        )",
    )
    .map_err(|e| format!("tenant_retention_policies table: {e}"))?;

    // Create credential_issuances table (axon-906b527a).
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS credential_issuances (
            jti            TEXT PRIMARY KEY,
            user_id        TEXT NOT NULL,
            tenant_id      TEXT NOT NULL,
            issued_at_ms   INTEGER NOT NULL,
            expires_at_ms  INTEGER NOT NULL,
            grants_json    TEXT NOT NULL
        )",
    )
    .map_err(|e| format!("credential_issuances table: {e}"))?;

    Ok(())
}

/// Apply auth/tenancy migrations to a PostgreSQL connection pool.
///
/// This is idempotent — running it multiple times on the same database
/// produces the same result without errors or duplicate rows.
pub async fn apply_auth_migrations_postgres(pool: &sqlx::PgPool) -> Result<(), String> {
    // Create tenants table.
    // UNIQUE(name) enforced by the UNIQUE constraint on the name column.
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
    // PRIMARY KEY (provider, external_id) enforces UNIQUE(provider, external_id).
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
    // PRIMARY KEY (tenant_id, user_id) is the composite PK required by ADR-018.
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

    // Create tenant_retention_policies table (axon-c6908e78).
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

    // Create credential_issuances table (axon-906b527a).
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
    use rusqlite::Connection;

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn open_and_migrate() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory DB should open");
        apply_auth_migrations_sqlite(&conn).expect("migrations should succeed");
        conn
    }

    // ── Schema existence ──────────────────────────────────────────────────────

    #[test]
    fn apply_sqlite_migrations_creates_all_tables() {
        let conn = open_and_migrate();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN (
                    'tenants', 'users', 'user_identities', 'tenant_users',
                    'tenant_databases', 'credential_revocations',
                    'tenant_retention_policies', 'credential_issuances'
                )",
                [],
                |row| row.get(0),
            )
            .expect("query should succeed");
        assert_eq!(count, 8, "all 8 auth tables should be created");
    }

    #[test]
    fn audit_retention_policies_table_absent() {
        let conn = open_and_migrate();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='audit_retention_policies'",
                [],
                |row| row.get(0),
            )
            .expect("query should succeed");
        assert_eq!(
            count, 0,
            "audit_retention_policies must NOT be created here (F1 territory)"
        );
    }

    #[test]
    fn apply_sqlite_migrations_is_idempotent() {
        let conn = Connection::open_in_memory().expect("in-memory DB should open");
        apply_auth_migrations_sqlite(&conn).expect("first migration should succeed");
        apply_auth_migrations_sqlite(&conn).expect("second migration should be idempotent");

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN (
                    'tenants', 'users', 'user_identities', 'tenant_users',
                    'tenant_databases', 'credential_revocations',
                    'tenant_retention_policies', 'credential_issuances'
                )",
                [],
                |row| row.get(0),
            )
            .expect("query should succeed");
        assert_eq!(count, 8, "still exactly 8 tables after re-run");
    }

    // ── Round-trip tests ──────────────────────────────────────────────────────

    #[test]
    fn round_trip_tenants() {
        let conn = open_and_migrate();

        conn.execute(
            "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["t-001", "acme", "Acme Corp", 1_000_000i64, 1_000_001i64],
        )
        .expect("insert should succeed");

        let (id, name, display_name, created_at_ms, updated_at_ms): (
            String,
            String,
            String,
            i64,
            i64,
        ) = conn
            .query_row(
                "SELECT id, name, display_name, created_at_ms, updated_at_ms
                 FROM tenants WHERE id = ?1",
                rusqlite::params!["t-001"],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .expect("SELECT should succeed");

        assert_eq!(id, "t-001");
        assert_eq!(name, "acme");
        assert_eq!(display_name, "Acme Corp");
        assert_eq!(created_at_ms, 1_000_000);
        assert_eq!(updated_at_ms, 1_000_001);
    }

    #[test]
    fn round_trip_users() {
        let conn = open_and_migrate();

        conn.execute(
            "INSERT INTO users (id, display_name, email, created_at_ms, suspended_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                "u-001",
                "Alice",
                "alice@example.com",
                2_000_000i64,
                rusqlite::types::Null
            ],
        )
        .expect("insert should succeed");

        let (id, display_name, email, created_at_ms, suspended_at_ms): (
            String,
            String,
            Option<String>,
            i64,
            Option<i64>,
        ) = conn
            .query_row(
                "SELECT id, display_name, email, created_at_ms, suspended_at_ms
                 FROM users WHERE id = ?1",
                rusqlite::params!["u-001"],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .expect("SELECT should succeed");

        assert_eq!(id, "u-001");
        assert_eq!(display_name, "Alice");
        assert_eq!(email.as_deref(), Some("alice@example.com"));
        assert_eq!(created_at_ms, 2_000_000);
        assert!(suspended_at_ms.is_none());
    }

    #[test]
    fn round_trip_user_identities() {
        let conn = open_and_migrate();

        // Need a parent user first.
        conn.execute(
            "INSERT INTO users (id, display_name, created_at_ms) VALUES (?1, ?2, ?3)",
            rusqlite::params!["u-001", "Alice", 1_000_000i64],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO user_identities (provider, external_id, user_id, created_at_ms)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["tailscale", "alice@tailnet", "u-001", 3_000_000i64],
        )
        .expect("insert should succeed");

        let (provider, external_id, user_id, created_at_ms): (String, String, String, i64) = conn
            .query_row(
                "SELECT provider, external_id, user_id, created_at_ms
                 FROM user_identities WHERE provider = ?1 AND external_id = ?2",
                rusqlite::params!["tailscale", "alice@tailnet"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("SELECT should succeed");

        assert_eq!(provider, "tailscale");
        assert_eq!(external_id, "alice@tailnet");
        assert_eq!(user_id, "u-001");
        assert_eq!(created_at_ms, 3_000_000);
    }

    #[test]
    fn round_trip_tenant_users() {
        let conn = open_and_migrate();

        conn.execute(
            "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["t-001", "acme", "Acme Corp", 1_000_000i64, 1_000_000i64],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO users (id, display_name, created_at_ms) VALUES (?1, ?2, ?3)",
            rusqlite::params!["u-001", "Alice", 1_000_000i64],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO tenant_users (tenant_id, user_id, role, added_at_ms)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["t-001", "u-001", "admin", 4_000_000i64],
        )
        .expect("insert should succeed");

        let (tenant_id, user_id, role, added_at_ms): (String, String, String, i64) = conn
            .query_row(
                "SELECT tenant_id, user_id, role, added_at_ms
                 FROM tenant_users WHERE tenant_id = ?1 AND user_id = ?2",
                rusqlite::params!["t-001", "u-001"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("SELECT should succeed");

        assert_eq!(tenant_id, "t-001");
        assert_eq!(user_id, "u-001");
        assert_eq!(role, "admin");
        assert_eq!(added_at_ms, 4_000_000);
    }

    #[test]
    fn round_trip_tenant_databases() {
        let conn = open_and_migrate();

        conn.execute(
            "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["t-001", "acme", "Acme Corp", 1_000_000i64, 1_000_000i64],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO tenant_databases (tenant_id, database_name, created_at_ms)
             VALUES (?1, ?2, ?3)",
            rusqlite::params!["t-001", "orders", 5_000_000i64],
        )
        .expect("insert should succeed");

        let (tenant_id, database_name, created_at_ms): (String, String, i64) = conn
            .query_row(
                "SELECT tenant_id, database_name, created_at_ms
                 FROM tenant_databases WHERE tenant_id = ?1 AND database_name = ?2",
                rusqlite::params!["t-001", "orders"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("SELECT should succeed");

        assert_eq!(tenant_id, "t-001");
        assert_eq!(database_name, "orders");
        assert_eq!(created_at_ms, 5_000_000);
    }

    #[test]
    fn round_trip_credential_revocations() {
        let conn = open_and_migrate();

        conn.execute(
            "INSERT INTO credential_revocations (jti, revoked_at_ms, revoked_by)
             VALUES (?1, ?2, ?3)",
            rusqlite::params!["jti-abc123", 6_000_000i64, "u-001"],
        )
        .expect("insert should succeed");

        let (jti, revoked_at_ms, revoked_by): (String, i64, Option<String>) = conn
            .query_row(
                "SELECT jti, revoked_at_ms, revoked_by
                 FROM credential_revocations WHERE jti = ?1",
                rusqlite::params!["jti-abc123"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("SELECT should succeed");

        assert_eq!(jti, "jti-abc123");
        assert_eq!(revoked_at_ms, 6_000_000);
        assert_eq!(revoked_by.as_deref(), Some("u-001"));
    }

    // ── Constraint tests ──────────────────────────────────────────────────────

    #[test]
    fn unique_tenant_name_rejected_on_duplicate() {
        let conn = open_and_migrate();

        conn.execute(
            "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                "t-001",
                "tenant-1",
                "Tenant One",
                1_000_000i64,
                1_000_000i64
            ],
        )
        .expect("first tenant insert should succeed");

        let result = conn.execute(
            "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                "t-002",
                "tenant-1",
                "Tenant One Duplicate",
                2_000_000i64,
                2_000_000i64
            ],
        );
        assert!(result.is_err(), "duplicate tenant name should be rejected");
    }

    #[test]
    fn unique_user_identity_rejected_on_duplicate() {
        let conn = open_and_migrate();

        conn.execute(
            "INSERT INTO users (id, display_name, created_at_ms) VALUES (?1, ?2, ?3)",
            rusqlite::params!["u-001", "User One", 1_000_000i64],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO user_identities (provider, external_id, user_id, created_at_ms)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["tailscale", "alice@tailnet", "u-001", 1_000_000i64],
        )
        .expect("first identity insert should succeed");

        let result = conn.execute(
            "INSERT INTO user_identities (provider, external_id, user_id, created_at_ms)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["tailscale", "alice@tailnet", "u-001", 2_000_000i64],
        );
        assert!(
            result.is_err(),
            "duplicate provider+external_id should be rejected"
        );
    }

    #[test]
    fn tenant_user_composite_pk_rejected_on_duplicate() {
        let conn = open_and_migrate();

        conn.execute(
            "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                "t-001",
                "tenant-1",
                "Tenant One",
                1_000_000i64,
                1_000_000i64
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO users (id, display_name, created_at_ms) VALUES (?1, ?2, ?3)",
            rusqlite::params!["u-001", "User One", 1_000_000i64],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO tenant_users (tenant_id, user_id, role, added_at_ms)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["t-001", "u-001", "admin", 1_000_000i64],
        )
        .expect("first membership insert should succeed");

        let result = conn.execute(
            "INSERT INTO tenant_users (tenant_id, user_id, role, added_at_ms)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["t-001", "u-001", "write", 2_000_000i64],
        );
        assert!(
            result.is_err(),
            "duplicate tenant+user membership should be rejected"
        );
    }
}
