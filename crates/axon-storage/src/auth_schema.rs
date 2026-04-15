//! SQL migrations for auth/tenancy tables (FEAT-014, ADR-018).
//!
//! Adds tables for:
//! - tenants: global account boundary
//! - users: global user identities
//! - user_identities: federated identity mapping
//! - tenant_users: M:N tenant membership
//! - tenant_databases: tenant-to-database mapping
//! - credential_revocations: revoked JWTs
//! - audit_retention_policies: retention settings per tenant

/// Apply auth/tenancy migrations to a SQLite storage adapter.
///
/// This is idempotent - running it multiple times on the same database
/// produces the same result without errors or duplicate rows.
pub fn apply_auth_migrations_sqlite(conn: &rusqlite::Connection) -> Result<(), String> {
    // Create tenants table
    // UNIQUE(tenants.name) is enforced by the UNIQUE constraint
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

    // Create users table
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

    // Create user_identities table
    // UNIQUE(user_identities.provider, external_id) is enforced
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

    // Create tenant_users table
    // PK(tenant_users.tenant_id, user_id) is enforced by composite primary key
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

    // Create tenant_databases table
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS tenant_databases (
            tenant_id      TEXT NOT NULL REFERENCES tenants(id),
            database_name  TEXT NOT NULL,
            created_at_ms  INTEGER NOT NULL,
            PRIMARY KEY (tenant_id, database_name)
        )",
    )
    .map_err(|e| format!("tenant_databases table: {e}"))?;

    // Create credential_revocations table
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS credential_revocations (
            jti            TEXT PRIMARY KEY,
            revoked_at_ms  INTEGER NOT NULL,
            revoked_by     TEXT
        )",
    )
    .map_err(|e| format!("credential_revocations table: {e}"))?;

    // Create audit_retention_policies table
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS audit_retention_policies (
            tenant_id            TEXT PRIMARY KEY REFERENCES tenants(id),
            policy               TEXT NOT NULL,
            retain_for_days      INTEGER,
            created_at_ms        INTEGER NOT NULL,
            updated_at_ms        INTEGER NOT NULL
        )",
    )
    .map_err(|e| format!("audit_retention_policies table: {e}"))?;

    Ok(())
}

/// Apply auth/tenancy migrations to a PostgreSQL storage adapter.
///
/// This is idempotent - running it multiple times on the same database
/// produces the same result without errors or duplicate rows.
pub async fn apply_auth_migrations_postgres(client: &tokio_postgres::Client) -> Result<(), String> {
    // Create tenants table
    client
        .batch_execute(
            "CREATE TABLE IF NOT EXISTS tenants (
                id                 TEXT PRIMARY KEY,
                name               TEXT NOT NULL UNIQUE,
                display_name       TEXT NOT NULL,
                created_at_ms      BIGINT NOT NULL,
                updated_at_ms      BIGINT NOT NULL
            )",
        )
        .await
        .map_err(|e| format!("tenants table: {e}"))?;

    // Create users table
    client
        .batch_execute(
            "CREATE TABLE IF NOT EXISTS users (
                id                 TEXT PRIMARY KEY,
                display_name       TEXT NOT NULL,
                email              TEXT,
                created_at_ms      BIGINT NOT NULL,
                suspended_at_ms    BIGINT
            )",
        )
        .await
        .map_err(|e| format!("users table: {e}"))?;

    // Create user_identities table
    // UNIQUE(user_identities.provider, external_id) is enforced
    client
        .batch_execute(
            "CREATE TABLE IF NOT EXISTS user_identities (
                provider       TEXT NOT NULL,
                external_id    TEXT NOT NULL,
                user_id        TEXT NOT NULL REFERENCES users(id),
                created_at_ms  BIGINT NOT NULL,
                PRIMARY KEY (provider, external_id)
            )",
        )
        .await
        .map_err(|e| format!("user_identities table: {e}"))?;

    // Create tenant_users table
    // PK(tenant_users.tenant_id, user_id) is enforced by composite primary key
    client
        .batch_execute(
            "CREATE TABLE IF NOT EXISTS tenant_users (
                tenant_id    TEXT NOT NULL REFERENCES tenants(id),
                user_id      TEXT NOT NULL REFERENCES users(id),
                role         TEXT NOT NULL,
                added_at_ms  BIGINT NOT NULL,
                PRIMARY KEY (tenant_id, user_id)
            )",
        )
        .await
        .map_err(|e| format!("tenant_users table: {e}"))?;

    // Create tenant_databases table
    client
        .batch_execute(
            "CREATE TABLE IF NOT EXISTS tenant_databases (
                tenant_id      TEXT NOT NULL REFERENCES tenants(id),
                database_name  TEXT NOT NULL,
                created_at_ms  BIGINT NOT NULL,
                PRIMARY KEY (tenant_id, database_name)
            )",
        )
        .await
        .map_err(|e| format!("tenant_databases table: {e}"))?;

    // Create credential_revocations table
    client
        .batch_execute(
            "CREATE TABLE IF NOT EXISTS credential_revocations (
                jti            TEXT PRIMARY KEY,
                revoked_at_ms  BIGINT NOT NULL,
                revoked_by     TEXT
            )",
        )
        .await
        .map_err(|e| format!("credential_revocations table: {e}"))?;

    // Create audit_retention_policies table
    client
        .batch_execute(
            "CREATE TABLE IF NOT EXISTS audit_retention_policies (
                tenant_id            TEXT PRIMARY KEY REFERENCES tenants(id),
                policy               TEXT NOT NULL,
                retain_for_days      INTEGER,
                created_at_ms        BIGINT NOT NULL,
                updated_at_ms        BIGINT NOT NULL
            )",
        )
        .await
        .map_err(|e| format!("audit_retention_policies table: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn apply_sqlite_migrations_creates_all_tables() {
        let conn = Connection::open_in_memory().expect("in-memory DB should open");
        apply_auth_migrations_sqlite(&conn).expect("migrations should succeed");

        // Verify tables exist
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN (
                    'tenants', 'users', 'user_identities', 'tenant_users',
                    'tenant_databases', 'credential_revocations', 'audit_retention_policies'
                )",
                [],
                |row| row.get(0),
            )
            .expect("query should succeed");
        assert_eq!(count, 7, "all 7 auth tables should be created");
    }

    #[test]
    fn apply_sqlite_migrations_is_idempotent() {
        let conn = Connection::open_in_memory().expect("in-memory DB should open");
        apply_auth_migrations_sqlite(&conn).expect("first migration should succeed");
        apply_auth_migrations_sqlite(&conn).expect("second migration should be idempotent");

        // Verify no duplicate rows in sqlite_master
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN (
                    'tenants', 'users', 'user_identities', 'tenant_users',
                    'tenant_databases', 'credential_revocations', 'audit_retention_policies'
                )",
                [],
                |row| row.get(0),
            )
            .expect("query should succeed");
        assert_eq!(count, 7, "still exactly 7 tables after re-run");
    }

    #[test]
    fn unique_tenant_name_rejected_on_duplicate() {
        let conn = Connection::open_in_memory().expect("in-memory DB should open");
        apply_auth_migrations_sqlite(&conn).expect("migrations should succeed");

        // Insert first tenant - should succeed
        conn.execute(
            "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
             VALUES (?, ?, ?, ?, ?)",
            rusqlite::params!["t-001", "tenant-1", "Tenant One", 1000000i64, 1000000i64],
        )
        .expect("first tenant insert should succeed");

        // Insert second tenant with same name - should fail
        let result = conn.execute(
            "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
             VALUES (?, ?, ?, ?, ?)",
            rusqlite::params![
                "t-002",
                "tenant-1",
                "Tenant One Duplicate",
                2000000i64,
                2000000i64
            ],
        );
        assert!(result.is_err(), "duplicate tenant name should be rejected");
    }

    #[test]
    fn unique_user_identity_rejected_on_duplicate() {
        let conn = Connection::open_in_memory().expect("in-memory DB should open");
        apply_auth_migrations_sqlite(&conn).expect("migrations should succeed");

        // Insert first user - should succeed
        conn.execute(
            "INSERT INTO users (id, display_name, created_at_ms)
             VALUES (?, ?, ?)",
            rusqlite::params!["u-001", "User One", 1000000i64],
        )
        .expect("first user insert should succeed");

        // Insert first identity for user - should succeed
        conn.execute(
            "INSERT INTO user_identities (provider, external_id, user_id, created_at_ms)
             VALUES (?, ?, ?, ?)",
            rusqlite::params!["tailscale", "alice@tailnet", "u-001", 1000000i64],
        )
        .expect("first identity insert should succeed");

        // Insert second identity with same provider and external_id - should fail
        let result = conn.execute(
            "INSERT INTO user_identities (provider, external_id, user_id, created_at_ms)
             VALUES (?, ?, ?, ?)",
            rusqlite::params!["tailscale", "alice@tailnet", "u-002", 2000000i64],
        );
        assert!(
            result.is_err(),
            "duplicate provider+external_id should be rejected"
        );
    }

    #[test]
    fn tenant_user_composite_pk_rejected_on_duplicate() {
        let conn = Connection::open_in_memory().expect("in-memory DB should open");
        apply_auth_migrations_sqlite(&conn).expect("migrations should succeed");

        // Insert tenants
        conn.execute(
            "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms)
             VALUES (?, ?, ?, ?, ?)",
            rusqlite::params!["t-001", "tenant-1", "Tenant One", 1000000i64, 1000000i64],
        )
        .expect("first tenant insert should succeed");

        // Insert users
        conn.execute(
            "INSERT INTO users (id, display_name, created_at_ms)
             VALUES (?, ?, ?)",
            rusqlite::params!["u-001", "User One", 1000000i64],
        )
        .expect("first user insert should succeed");

        // Insert first membership - should succeed
        conn.execute(
            "INSERT INTO tenant_users (tenant_id, user_id, role, added_at_ms)
             VALUES (?, ?, ?, ?)",
            rusqlite::params!["t-001", "u-001", "admin", 1000000i64],
        )
        .expect("first membership insert should succeed");

        // Insert duplicate membership - should fail
        let result = conn.execute(
            "INSERT INTO tenant_users (tenant_id, user_id, role, added_at_ms)
             VALUES (?, ?, ?, ?)",
            rusqlite::params!["t-001", "u-001", "write", 2000000i64],
        );
        assert!(
            result.is_err(),
            "duplicate tenant+user membership should be rejected"
        );
    }
}
