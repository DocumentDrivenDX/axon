//! Per-tenant database isolation via physical SQLite files.
//!
//! `TenantRouter` maps database (tenant) names to separate `AxonHandler`
//! instances, each backed by its own SQLite file. The "default" database
//! always maps to the handler supplied at construction time.
//!
//! **V1 scope:** Only SQLite is supported. PostgreSQL tenant routing is
//! deferred. The router is not yet wired into per-request middleware —
//! that is a follow-up task.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock};

use axon_api::handler::AxonHandler;
use axon_storage::SqliteStorageAdapter;

/// A shared, async-safe handle to an `AxonHandler<SqliteStorageAdapter>`.
pub type SharedSqliteHandler = Arc<Mutex<AxonHandler<SqliteStorageAdapter>>>;

/// Routes tenant database names to isolated `AxonHandler` instances.
///
/// Each tenant gets its own SQLite file at `{data_dir}/tenants/{db_name}.db`.
/// The "default" name always returns the handler provided at construction.
pub struct TenantRouter {
    /// Root directory for tenant database files. May be empty when constructed
    /// via [`TenantRouter::single`] (test-only, no filesystem access).
    data_dir: PathBuf,
    /// Cached handlers keyed by database name (excludes "default").
    tenants: RwLock<HashMap<String, SharedSqliteHandler>>,
    /// The handler returned for the "default" database.
    default_handler: SharedSqliteHandler,
}

impl TenantRouter {
    /// Create a new router that stores tenant databases under `data_dir`.
    pub fn new(data_dir: PathBuf, default_handler: SharedSqliteHandler) -> Self {
        Self {
            data_dir,
            tenants: RwLock::new(HashMap::new()),
            default_handler,
        }
    }

    /// Test-only constructor: wraps a single handler as "default" with no
    /// filesystem backing for tenants.
    pub fn single(handler: SharedSqliteHandler) -> Self {
        Self {
            data_dir: PathBuf::new(),
            tenants: RwLock::new(HashMap::new()),
            default_handler: handler,
        }
    }

    /// Return the handler for the default database.
    pub fn default_handler(&self) -> &SharedSqliteHandler {
        &self.default_handler
    }

    /// Return the directory where tenant SQLite files are stored.
    pub fn tenants_dir(&self) -> PathBuf {
        self.data_dir.join("tenants")
    }

    /// Compute the on-disk path for a tenant's SQLite database.
    pub fn tenant_db_path(&self, db_name: &str) -> PathBuf {
        self.tenants_dir().join(format!("{db_name}.db"))
    }

    /// Look up or create the handler for `db_name`.
    ///
    /// - `"default"` always returns the default handler without touching disk.
    /// - Any other name checks the in-memory cache first, then creates a new
    ///   SQLite file at `{data_dir}/tenants/{db_name}.db`, initialises its
    ///   schema, wraps it in an `AxonHandler`, caches it, and returns it.
    pub async fn get_or_create(&self, db_name: &str) -> Result<SharedSqliteHandler, String> {
        if db_name == "default" {
            return Ok(Arc::clone(&self.default_handler));
        }

        // Fast path: read lock to check cache.
        {
            let tenants = self.tenants.read().await;
            if let Some(handler) = tenants.get(db_name) {
                return Ok(Arc::clone(handler));
            }
        }

        // Slow path: write lock, double-check, then create.
        let mut tenants = self.tenants.write().await;
        // Double-check after acquiring write lock (another task may have raced).
        if let Some(handler) = tenants.get(db_name) {
            return Ok(Arc::clone(handler));
        }

        let tenants_dir = self.tenants_dir();
        std::fs::create_dir_all(&tenants_dir).map_err(|e| {
            format!(
                "failed to create tenants directory {}: {e}",
                tenants_dir.display()
            )
        })?;

        let db_path = self.tenant_db_path(db_name);
        let path_str = db_path.to_str().ok_or_else(|| {
            format!(
                "tenant database path is not valid UTF-8: {}",
                db_path.display()
            )
        })?;

        let storage = SqliteStorageAdapter::open(path_str)
            .map_err(|e| format!("failed to open tenant database '{db_name}': {e}"))?;

        let handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
        tenants.insert(db_name.to_owned(), Arc::clone(&handler));
        Ok(handler)
    }

    /// Return the names of all cached tenant databases (including "default").
    pub async fn list_databases(&self) -> Vec<String> {
        let tenants = self.tenants.read().await;
        let mut names: Vec<String> = std::iter::once("default".to_owned())
            .chain(tenants.keys().cloned())
            .collect();
        names.sort();
        names
    }

    /// Remove a tenant from the cache and delete its SQLite file.
    ///
    /// Returns an error if `db_name` is "default" (the default database cannot
    /// be dropped) or if the file cannot be removed.
    pub async fn drop_database(&self, db_name: &str) -> Result<(), String> {
        if db_name == "default" {
            return Err("cannot drop the default database".to_owned());
        }

        let mut tenants = self.tenants.write().await;
        tenants.remove(db_name);

        let db_path = self.tenant_db_path(db_name);
        if db_path.exists() {
            std::fs::remove_file(&db_path).map_err(|e| {
                format!(
                    "failed to remove tenant database file {}: {e}",
                    db_path.display()
                )
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// Create a `TenantRouter` backed by a temporary directory.
    fn make_router(tmp: &Path) -> TenantRouter {
        let default_storage =
            SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite should open");
        let default_handler = Arc::new(Mutex::new(AxonHandler::new(default_storage)));
        TenantRouter::new(tmp.to_path_buf(), default_handler)
    }

    #[tokio::test]
    async fn get_or_create_default_returns_default_handler() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let router = make_router(tmp.path());

        let handler = router.get_or_create("default").await.expect("default");
        // Should be the same Arc as the default handler.
        assert!(Arc::ptr_eq(&handler, router.default_handler()));
    }

    #[tokio::test]
    async fn get_or_create_creates_new_sqlite_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let router = make_router(tmp.path());

        let _handler = router.get_or_create("teamA").await.expect("teamA");

        // The SQLite file should exist on disk.
        let expected_path = tmp.path().join("tenants").join("teamA.db");
        assert!(
            expected_path.exists(),
            "expected tenant db at {}",
            expected_path.display()
        );
    }

    #[tokio::test]
    async fn get_or_create_returns_cached_handler() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let router = make_router(tmp.path());

        let h1 = router.get_or_create("teamA").await.expect("first call");
        let h2 = router.get_or_create("teamA").await.expect("second call");

        assert!(
            Arc::ptr_eq(&h1, &h2),
            "repeated get_or_create should return the same Arc"
        );
    }

    #[tokio::test]
    async fn drop_database_removes_from_cache_and_disk() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let router = make_router(tmp.path());

        let _handler = router.get_or_create("teamB").await.expect("create");
        let db_path = router.tenant_db_path("teamB");
        assert!(db_path.exists(), "file should exist after creation");

        router.drop_database("teamB").await.expect("drop");
        assert!(!db_path.exists(), "file should be removed after drop");

        // Cache should be empty for this tenant.
        let names = router.list_databases().await;
        assert!(
            !names.contains(&"teamB".to_owned()),
            "teamB should not be listed after drop"
        );
    }

    #[tokio::test]
    async fn drop_default_is_rejected() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let router = make_router(tmp.path());

        let result = router.drop_database("default").await;
        assert!(result.is_err(), "dropping 'default' should fail");
        assert!(
            result.unwrap_err().contains("cannot drop the default"),
            "error message should mention 'default'"
        );
    }

    #[tokio::test]
    async fn list_databases_returns_expected_names() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let router = make_router(tmp.path());

        let before = router.list_databases().await;
        assert_eq!(before, vec!["default"], "initial list should be [default]");

        let _h1 = router.get_or_create("alpha").await.expect("alpha");
        let _h2 = router.get_or_create("beta").await.expect("beta");

        let after = router.list_databases().await;
        assert_eq!(
            after,
            vec!["alpha", "beta", "default"],
            "list should be sorted and include all tenants"
        );
    }

    #[tokio::test]
    async fn single_constructor_works() {
        let storage =
            SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite should open");
        let handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
        let router = TenantRouter::single(Arc::clone(&handler));

        let got = router.get_or_create("default").await.expect("default");
        assert!(Arc::ptr_eq(&got, &handler));

        let names = router.list_databases().await;
        assert_eq!(names, vec!["default"]);
    }

    #[tokio::test]
    async fn tenant_db_path_format() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let router = make_router(tmp.path());

        let path = router.tenant_db_path("my_tenant");
        let expected = tmp.path().join("tenants").join("my_tenant.db");
        assert_eq!(path, expected);
    }

    #[tokio::test]
    async fn drop_nonexistent_is_ok() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let router = make_router(tmp.path());

        // Dropping a tenant that was never created should succeed silently.
        router
            .drop_database("phantom")
            .await
            .expect("drop of nonexistent tenant should succeed");
    }
}
