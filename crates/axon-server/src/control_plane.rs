//! Control-plane database for managing tenants and their provisioned databases.
//!
//! This SQLite database is **separate** from any tenant data store.  It tracks
//! tenant lifecycle: each tenant owns exactly one database, identified by the
//! `db_name` slug generated at creation time.

use rusqlite::{params, Connection};

use axon_core::error::AxonError;

// ---------------------------------------------------------------------------
// Row types
// ---------------------------------------------------------------------------

/// A registered tenant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tenant {
    pub id: String,
    pub name: String,
    /// Slug used as the tenant's SQLite filename / PostgreSQL database name.
    pub db_name: String,
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// ControlPlaneDb
// ---------------------------------------------------------------------------

/// Handle to the control-plane SQLite database.
pub struct ControlPlaneDb {
    conn: Connection,
}

impl ControlPlaneDb {
    /// Open (or create) a control-plane database at the given file path.
    pub fn open(path: &str) -> Result<Self, AxonError> {
        let conn = Connection::open(path).map_err(|e| AxonError::Storage(e.to_string()))?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    /// Open an in-memory database (useful for testing).
    pub fn open_in_memory() -> Result<Self, AxonError> {
        let conn = Connection::open_in_memory().map_err(|e| AxonError::Storage(e.to_string()))?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    /// Apply schema migrations.  Idempotent — safe to run on existing databases.
    pub fn migrate(&self) -> Result<(), AxonError> {
        // Step 1: ensure the tenants table exists with at least the base columns.
        self.conn
            .execute_batch(
                "PRAGMA foreign_keys = ON;

                 CREATE TABLE IF NOT EXISTS tenants (
                     id         TEXT PRIMARY KEY,
                     name       TEXT UNIQUE NOT NULL,
                     db_name    TEXT NOT NULL DEFAULT '',
                     created_at TEXT NOT NULL
                 );",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        // Step 2: add db_name column to pre-existing tenants tables that lack it.
        // Ignoring the error is safe: if the column already exists the ALTER fails
        // with "duplicate column name", which we treat as a no-op.
        let _ = self.conn.execute(
            "ALTER TABLE tenants ADD COLUMN db_name TEXT NOT NULL DEFAULT ''",
            [],
        );

        // Step 3: drop obsolete junction tables introduced in earlier schema revisions.
        self.conn
            .execute_batch(
                "DROP TABLE IF EXISTS tenant_databases;
                 DROP TABLE IF EXISTS nodes;",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        crate::user_roles::migrate_user_roles(&self.conn)?;
        crate::cors_config::migrate_cors_origins(&self.conn)?;
        Ok(())
    }

    // -- cors_origins ----------------------------------------------------------

    /// List all configured CORS allowed origins.
    pub fn list_cors_origins(&self) -> Result<Vec<String>, AxonError> {
        crate::cors_config::db_list(&self.conn)
    }

    /// Add (or no-op if already present) a CORS allowed origin.
    pub fn add_cors_origin(&self, origin: &str) -> Result<(), AxonError> {
        crate::cors_config::db_add(&self.conn, origin)
    }

    /// Remove a CORS allowed origin.  Returns `true` if a row was deleted.
    pub fn remove_cors_origin(&self, origin: &str) -> Result<bool, AxonError> {
        crate::cors_config::db_remove(&self.conn, origin)
    }

    // -- user_roles ------------------------------------------------------------

    /// List all user-role assignments.
    pub fn list_user_roles(&self) -> Result<Vec<crate::user_roles::UserRoleEntry>, AxonError> {
        crate::user_roles::db_list(&self.conn)
    }

    /// Upsert a user-role assignment.
    pub fn set_user_role(
        &self,
        login: &str,
        role: &crate::auth::Role,
    ) -> Result<(), AxonError> {
        crate::user_roles::db_set(&self.conn, login, role)
    }

    /// Remove a user-role assignment.  Returns `true` if a row was deleted.
    pub fn remove_user_role(&self, login: &str) -> Result<bool, AxonError> {
        crate::user_roles::db_remove(&self.conn, login)
    }

    // -- tenants ---------------------------------------------------------------

    /// Create a new tenant with a pre-generated `db_name` slug.
    pub fn create_tenant(
        &self,
        id: &str,
        name: &str,
        db_name: &str,
        created_at: &str,
    ) -> Result<(), AxonError> {
        self.conn
            .execute(
                "INSERT INTO tenants (id, name, db_name, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![id, name, db_name, created_at],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    /// Get a tenant by id.
    pub fn get_tenant(&self, id: &str) -> Result<Tenant, AxonError> {
        self.conn
            .query_row(
                "SELECT id, name, db_name, created_at FROM tenants WHERE id = ?1",
                params![id],
                |row| {
                    Ok(Tenant {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        db_name: row.get(2)?,
                        created_at: row.get(3)?,
                    })
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => AxonError::NotFound(format!("tenant {id}")),
                other => AxonError::Storage(other.to_string()),
            })
    }

    /// List all tenants, ordered by `created_at`.
    pub fn list_tenants(&self) -> Result<Vec<Tenant>, AxonError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, db_name, created_at FROM tenants ORDER BY created_at")
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(Tenant {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    db_name: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let mut tenants = Vec::new();
        for row in rows {
            tenants.push(row.map_err(|e| AxonError::Storage(e.to_string()))?);
        }
        Ok(tenants)
    }

    /// Delete a tenant by id.
    ///
    /// Returns the `db_name` of the deleted tenant so the caller can clean up
    /// the provisioned database file.  Returns `AxonError::NotFound` if the
    /// tenant does not exist.
    pub fn delete_tenant(&self, tenant_id: &str) -> Result<String, AxonError> {
        let tenant = self.get_tenant(tenant_id)?;

        self.conn
            .execute("DELETE FROM tenants WHERE id = ?1", params![tenant_id])
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        Ok(tenant.db_name)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn setup() -> ControlPlaneDb {
        ControlPlaneDb::open_in_memory().expect("open in-memory control-plane db")
    }

    // -- migration ----------------------------------------------------------

    #[test]
    fn migrate_is_idempotent() {
        let db = setup();
        db.migrate().expect("second migrate should succeed");
    }

    #[test]
    fn tables_exist_after_migration() {
        let db = setup();
        let tables: Vec<String> = db
            .conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            )
            .expect("prepare")
            .query_map([], |row| row.get(0))
            .expect("query")
            .filter_map(Result::ok)
            .collect();

        assert!(tables.contains(&"tenants".to_string()));
        // Old tables must not be present.
        assert!(!tables.contains(&"nodes".to_string()));
        assert!(!tables.contains(&"tenant_databases".to_string()));
    }

    // -- tenants ------------------------------------------------------------

    #[test]
    fn create_and_get_tenant() {
        let db = setup();
        db.create_tenant("t1", "Acme Corp", "acme-corp", "2026-01-01T00:00:00Z")
            .expect("create tenant");

        let t = db.get_tenant("t1").expect("get tenant");
        assert_eq!(t.id, "t1");
        assert_eq!(t.name, "Acme Corp");
        assert_eq!(t.db_name, "acme-corp");
        assert_eq!(t.created_at, "2026-01-01T00:00:00Z");
    }

    #[test]
    fn get_missing_tenant_returns_not_found() {
        let db = setup();
        let err = db.get_tenant("nonexistent").unwrap_err();
        assert!(
            matches!(err, AxonError::NotFound(_)),
            "expected NotFound, got {err:?}"
        );
    }

    #[test]
    fn duplicate_tenant_name_fails() {
        let db = setup();
        db.create_tenant("t1", "Acme", "acme-abc12345", "2026-01-01T00:00:00Z")
            .expect("first create");
        let err = db
            .create_tenant("t2", "Acme", "acme-def67890", "2026-01-02T00:00:00Z")
            .unwrap_err();
        assert!(
            matches!(err, AxonError::Storage(_)),
            "expected Storage error, got {err:?}"
        );
    }

    #[test]
    fn list_tenants_ordered() {
        let db = setup();
        db.create_tenant("t2", "Beta", "beta", "2026-02-01T00:00:00Z")
            .expect("create");
        db.create_tenant("t1", "Alpha", "alpha", "2026-01-01T00:00:00Z")
            .expect("create");
        let tenants = db.list_tenants().expect("list");
        assert_eq!(tenants.len(), 2);
        assert_eq!(tenants[0].name, "Alpha");
        assert_eq!(tenants[1].name, "Beta");
    }

    // -- delete_tenant --------------------------------------------------------

    #[test]
    fn delete_tenant_returns_db_name() {
        let db = setup();
        db.create_tenant("t1", "Acme", "acme", "2026-01-01T00:00:00Z")
            .expect("create tenant");

        let db_name = db.delete_tenant("t1").expect("delete");
        assert_eq!(db_name, "acme");

        let err = db.get_tenant("t1").unwrap_err();
        assert!(matches!(err, AxonError::NotFound(_)));
    }

    #[test]
    fn delete_nonexistent_tenant_returns_not_found() {
        let db = setup();
        let err = db.delete_tenant("no-such-id").unwrap_err();
        assert!(
            matches!(err, AxonError::NotFound(_)),
            "expected NotFound, got {err:?}"
        );
    }
}
