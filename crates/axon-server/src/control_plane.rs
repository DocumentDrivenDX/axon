//! Control-plane database for managing tenants and their provisioned databases.
//!
//! This SQLite database is **separate** from any tenant data store.  It tracks
//! tenant lifecycle: each tenant owns exactly one database, identified by the
//! `db_name` slug generated at creation time.

use sqlx::sqlite::SqlitePool;
use sqlx::Row;

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
///
/// Owns an optional tokio `Runtime` for callers outside an async context
/// (e.g. the production `main`). When constructed inside a `#[tokio::test(flavor = "multi_thread")]`
/// or gateway handler, the existing runtime is reused via `block_in_place`.
pub struct ControlPlaneDb {
    pool: SqlitePool,
    /// Owned runtime — only used when no outer tokio context exists.
    _rt: Option<tokio::runtime::Runtime>,
}

impl ControlPlaneDb {
    /// Run an async future, handling both async and non-async caller contexts.
    ///
    /// When inside a tokio context, uses the caller's runtime via
    /// `block_in_place`.  Otherwise uses the owned runtime.
    fn run_on<T>(
        owned_rt: &Option<tokio::runtime::Runtime>,
        fut: impl std::future::Future<Output = T>,
    ) -> T {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
            Err(_) => owned_rt
                .as_ref()
                .expect("ControlPlaneDb: no tokio runtime available")
                .block_on(fut),
        }
    }

    /// Helper: run an async sqlx future.
    fn block_on<T>(
        &self,
        fut: impl std::future::Future<Output = Result<T, sqlx::Error>>,
    ) -> Result<T, String> {
        Self::run_on(&self._rt, fut).map_err(|e| e.to_string())
    }

    /// Open (or create) a control-plane database at the given file path.
    pub fn open(path: &str) -> Result<Self, AxonError> {
        // If we're outside an async context, create a runtime to own.
        let (rt, pool) = match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                let pool = tokio::task::block_in_place(|| {
                    handle.block_on(SqlitePool::connect(&format!("sqlite:{path}?mode=rwc")))
                })
                .map_err(|e| AxonError::Storage(e.to_string()))?;
                (None, pool)
            }
            Err(_) => {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| AxonError::Storage(e.to_string()))?;
                let pool = rt
                    .block_on(SqlitePool::connect(&format!("sqlite:{path}?mode=rwc")))
                    .map_err(|e| AxonError::Storage(e.to_string()))?;
                (Some(rt), pool)
            }
        };
        let db = Self { pool, _rt: rt };
        db.migrate()?;
        Ok(db)
    }

    /// Open an in-memory database (useful for testing).
    pub fn open_in_memory() -> Result<Self, AxonError> {
        let (rt, pool) = match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                let pool = tokio::task::block_in_place(|| {
                    handle.block_on(SqlitePool::connect("sqlite::memory:"))
                })
                .map_err(|e| AxonError::Storage(e.to_string()))?;
                (None, pool)
            }
            Err(_) => {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| AxonError::Storage(e.to_string()))?;
                let pool = rt
                    .block_on(SqlitePool::connect("sqlite::memory:"))
                    .map_err(|e| AxonError::Storage(e.to_string()))?;
                (Some(rt), pool)
            }
        };
        let db = Self { pool, _rt: rt };
        db.migrate()?;
        Ok(db)
    }

    /// Apply schema migrations.  Idempotent — safe to run on existing databases.
    pub fn migrate(&self) -> Result<(), AxonError> {
        // Step 1: ensure the tenants table exists with at least the base columns.
        self.block_on(
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS tenants (
                     id         TEXT PRIMARY KEY,
                     name       TEXT UNIQUE NOT NULL,
                     db_name    TEXT NOT NULL DEFAULT '',
                     created_at TEXT NOT NULL
                 )",
            )
            .execute(&self.pool),
        )
        .map_err(AxonError::Storage)?;

        // Enable foreign keys via a separate statement.
        self.block_on(sqlx::query("PRAGMA foreign_keys = ON").execute(&self.pool))
            .map_err(AxonError::Storage)?;

        // Step 2: add db_name column to pre-existing tenants tables that lack it.
        // Ignoring the error is safe: if the column already exists the ALTER fails
        // with "duplicate column name", which we treat as a no-op.
        let _ = self.block_on(
            sqlx::query("ALTER TABLE tenants ADD COLUMN db_name TEXT NOT NULL DEFAULT ''")
                .execute(&self.pool),
        );

        // Step 3: drop obsolete junction tables introduced in earlier schema revisions.
        self.block_on(
            sqlx::query("DROP TABLE IF EXISTS tenant_databases").execute(&self.pool),
        )
        .map_err(AxonError::Storage)?;

        self.block_on(sqlx::query("DROP TABLE IF EXISTS nodes").execute(&self.pool))
            .map_err(AxonError::Storage)?;

        Self::run_on(&self._rt, crate::user_roles::migrate_user_roles(&self.pool))?;
        Self::run_on(&self._rt, crate::cors_config::migrate_cors_origins(&self.pool))?;
        Ok(())
    }

    // -- cors_origins ----------------------------------------------------------

    /// List all configured CORS allowed origins.
    pub fn list_cors_origins(&self) -> Result<Vec<String>, AxonError> {
        Self::run_on(&self._rt, crate::cors_config::db_list(&self.pool))
    }

    /// Add (or no-op if already present) a CORS allowed origin.
    pub fn add_cors_origin(&self, origin: &str) -> Result<(), AxonError> {
        Self::run_on(&self._rt, crate::cors_config::db_add(&self.pool, origin))
    }

    /// Remove a CORS allowed origin.  Returns `true` if a row was deleted.
    pub fn remove_cors_origin(&self, origin: &str) -> Result<bool, AxonError> {
        Self::run_on(&self._rt, crate::cors_config::db_remove(&self.pool, origin))
    }

    // -- user_roles ------------------------------------------------------------

    /// List all user-role assignments.
    pub fn list_user_roles(&self) -> Result<Vec<crate::user_roles::UserRoleEntry>, AxonError> {
        Self::run_on(&self._rt, crate::user_roles::db_list(&self.pool))
    }

    /// Upsert a user-role assignment.
    pub fn set_user_role(
        &self,
        login: &str,
        role: &crate::auth::Role,
    ) -> Result<(), AxonError> {
        Self::run_on(&self._rt, crate::user_roles::db_set(&self.pool, login, role))
    }

    /// Remove a user-role assignment.  Returns `true` if a row was deleted.
    pub fn remove_user_role(&self, login: &str) -> Result<bool, AxonError> {
        Self::run_on(&self._rt, crate::user_roles::db_remove(&self.pool, login))
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
        self.block_on(
            sqlx::query(
                "INSERT INTO tenants (id, name, db_name, created_at) VALUES (?, ?, ?, ?)",
            )
            .bind(id)
            .bind(name)
            .bind(db_name)
            .bind(created_at)
            .execute(&self.pool),
        )
        .map_err(AxonError::Storage)?;
        Ok(())
    }

    /// Get a tenant by id.
    pub fn get_tenant(&self, id: &str) -> Result<Tenant, AxonError> {
        let row = self
            .block_on(
                sqlx::query("SELECT id, name, db_name, created_at FROM tenants WHERE id = ?")
                    .bind(id)
                    .fetch_one(&self.pool),
            )
            .map_err(|e| {
                if e.contains("no rows") {
                    AxonError::NotFound(format!("tenant {id}"))
                } else {
                    AxonError::Storage(e)
                }
            })?;

        Ok(Tenant {
            id: row.get("id"),
            name: row.get("name"),
            db_name: row.get("db_name"),
            created_at: row.get("created_at"),
        })
    }

    /// List all tenants, ordered by `created_at`.
    pub fn list_tenants(&self) -> Result<Vec<Tenant>, AxonError> {
        let rows = self
            .block_on(
                sqlx::query(
                    "SELECT id, name, db_name, created_at FROM tenants ORDER BY created_at",
                )
                .fetch_all(&self.pool),
            )
            .map_err(AxonError::Storage)?;

        let tenants = rows
            .iter()
            .map(|row| Tenant {
                id: row.get("id"),
                name: row.get("name"),
                db_name: row.get("db_name"),
                created_at: row.get("created_at"),
            })
            .collect();
        Ok(tenants)
    }

    /// Delete a tenant by id.
    ///
    /// Returns the `db_name` of the deleted tenant so the caller can clean up
    /// the provisioned database file.  Returns `AxonError::NotFound` if the
    /// tenant does not exist.
    pub fn delete_tenant(&self, tenant_id: &str) -> Result<String, AxonError> {
        let tenant = self.get_tenant(tenant_id)?;

        self.block_on(
            sqlx::query("DELETE FROM tenants WHERE id = ?")
                .bind(tenant_id)
                .execute(&self.pool),
        )
        .map_err(AxonError::Storage)?;

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
        let rows = db
            .block_on(
                sqlx::query(
                    "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
                )
                .fetch_all(&db.pool),
            )
            .expect("query");

        let tables: Vec<String> = rows.iter().map(|row| row.get("name")).collect();

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
