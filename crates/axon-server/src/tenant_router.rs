//! Per-tenant database isolation via physical SQLite files or PostgreSQL databases.
//!
//! `TenantRouter` maps database (tenant) names to separate `AxonHandler`
//! instances, each backed by its own SQLite file (SQLite mode) or its own
//! PostgreSQL database (Postgres mode). The "default" database always maps to
//! the handler supplied at construction time.
//!
//! When constructed via [`TenantRouter::single`], ALL database names resolve
//! to the default handler (no filesystem access, no separate tenants).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock};

use axon_api::handler::AxonHandler;
use axon_storage::adapter::StorageAdapter;
use axon_storage::{PostgresStorageAdapter, SqliteStorageAdapter};

/// Shared, async-safe handle to an `AxonHandler` backed by a boxed
/// `StorageAdapter`.  Used by the HTTP gateway for both SQLite and
/// PostgreSQL tenants.
pub type TenantHandler = Arc<Mutex<AxonHandler<Box<dyn StorageAdapter + Send + Sync>>>>;

/// Backend-specific state for the router.
enum RouterBackend {
    /// SQLite mode: tenant databases are separate files in `data_dir`.
    Sqlite {
        data_dir: PathBuf,
        tenants: RwLock<HashMap<String, TenantHandler>>,
        default_handler: TenantHandler,
        single_mode: bool,
    },
    /// PostgreSQL mode: tenant databases are separate PG databases.
    Postgres {
        superadmin_dsn: String,
        tenants: RwLock<HashMap<String, TenantHandler>>,
        default_handler: TenantHandler,
    },
}

/// Routes tenant database names to isolated `AxonHandler` instances.
///
/// Supports two backends:
/// - **SQLite** (default): each tenant gets its own `.db` file under `data_dir`.
/// - **PostgreSQL**: each tenant gets its own `axon_{name}` PG database;
///   a superadmin DSN is used to provision and deprovision databases.
///
/// When constructed via [`TenantRouter::single`], ALL database names resolve
/// to the default SQLite handler (no filesystem access, no separate tenants).
pub struct TenantRouter {
    backend: RouterBackend,
}

impl TenantRouter {
    /// Create a new SQLite router that stores tenant databases under `data_dir`.
    pub fn new(data_dir: PathBuf, default_handler: TenantHandler) -> Self {
        Self {
            backend: RouterBackend::Sqlite {
                data_dir,
                tenants: RwLock::new(HashMap::new()),
                default_handler,
                single_mode: false,
            },
        }
    }

    /// Create a new PostgreSQL router.
    ///
    /// `superadmin_dsn` must be a connection string for a superuser account
    /// that can execute `CREATE DATABASE` / `DROP DATABASE`.  The
    /// `default_handler` is pre-connected to the master database
    /// (`axon_master`) and is returned for the `"default"` database name.
    pub fn new_postgres(superadmin_dsn: String, default_handler: TenantHandler) -> Self {
        Self {
            backend: RouterBackend::Postgres {
                superadmin_dsn,
                tenants: RwLock::new(HashMap::new()),
                default_handler,
            },
        }
    }

    /// Wraps a single handler so that ALL database names resolve to it.
    ///
    /// This is the constructor for tests and single-database deployments
    /// where multi-tenant isolation is not needed.  No filesystem access
    /// occurs for non-default database names.
    pub fn single(handler: TenantHandler) -> Self {
        Self {
            backend: RouterBackend::Sqlite {
                data_dir: PathBuf::new(),
                tenants: RwLock::new(HashMap::new()),
                default_handler: handler,
                single_mode: true,
            },
        }
    }

    /// Return the handler for the default database.
    pub fn default_handler(&self) -> &TenantHandler {
        match &self.backend {
            RouterBackend::Sqlite {
                default_handler, ..
            } => default_handler,
            RouterBackend::Postgres {
                default_handler, ..
            } => default_handler,
        }
    }

    /// Return the directory where tenant SQLite files are stored.
    ///
    /// Returns an empty `PathBuf` in Postgres mode.
    pub fn tenants_dir(&self) -> PathBuf {
        match &self.backend {
            RouterBackend::Sqlite { data_dir, .. } => data_dir.join("tenants"),
            RouterBackend::Postgres { .. } => PathBuf::new(),
        }
    }

    /// Compute the on-disk path for a tenant's SQLite database (SQLite mode only).
    pub fn tenant_db_path(&self, db_name: &str) -> PathBuf {
        self.tenants_dir().join(format!("{db_name}.db"))
    }

    /// Look up or create the handler for `db_name` in SQLite mode.
    ///
    /// - `"default"` always returns the default handler without touching disk.
    /// - Any other name checks the in-memory cache first, then creates a new
    ///   SQLite file at `{data_dir}/tenants/{db_name}.db`, initialises its
    ///   schema, wraps it in an `AxonHandler`, caches it, and returns it.
    ///
    /// # Errors
    ///
    /// Returns `Err` if called in Postgres mode (use `get_or_create_pg` instead).
    pub async fn get_or_create(&self, db_name: &str) -> Result<TenantHandler, String> {
        match &self.backend {
            RouterBackend::Sqlite {
                data_dir,
                tenants,
                default_handler,
                single_mode,
            } => {
                if db_name == "default" || *single_mode {
                    return Ok(Arc::clone(default_handler));
                }

                // Fast path: read lock to check cache.
                {
                    let guard = tenants.read().await;
                    if let Some(handler) = guard.get(db_name) {
                        return Ok(Arc::clone(handler));
                    }
                }

                // Slow path: write lock, double-check, then create.
                let mut guard = tenants.write().await;
                if let Some(handler) = guard.get(db_name) {
                    return Ok(Arc::clone(handler));
                }

                let tenants_dir = data_dir.join("tenants");
                std::fs::create_dir_all(&tenants_dir).map_err(|e| {
                    format!(
                        "failed to create tenants directory {}: {e}",
                        tenants_dir.display()
                    )
                })?;

                let db_path = tenants_dir.join(format!("{db_name}.db"));
                let path_str = db_path.to_str().ok_or_else(|| {
                    format!(
                        "tenant database path is not valid UTF-8: {}",
                        db_path.display()
                    )
                })?;

                let storage = SqliteStorageAdapter::open(path_str)
                    .map_err(|e| format!("failed to open tenant database '{db_name}': {e}"))?;

                let boxed: Box<dyn StorageAdapter + Send + Sync> = Box::new(storage);
                let handler = Arc::new(Mutex::new(AxonHandler::new(boxed)));
                guard.insert(db_name.to_owned(), Arc::clone(&handler));
                Ok(handler)
            }
            RouterBackend::Postgres { .. } => Err(
                "get_or_create is not supported in Postgres mode; use get_or_create_pg".to_owned(),
            ),
        }
    }

    /// Look up or create the PostgreSQL handler for `db_name`.
    ///
    /// - `"default"` returns the master handler without provisioning anything.
    /// - Any other name checks the in-memory cache first, then calls
    ///   [`axon_storage::provision_postgres_database`] to create `axon_{db_name}`,
    ///   opens a new [`PostgresStorageAdapter`] pool against that database, and
    ///   caches the result.
    ///
    /// # Errors
    ///
    /// Returns `Err` if called in SQLite mode, or if the database cannot be
    /// provisioned.
    pub async fn get_or_create_pg(&self, db_name: &str) -> Result<TenantHandler, String> {
        match &self.backend {
            RouterBackend::Postgres {
                superadmin_dsn,
                tenants,
                default_handler,
            } => {
                if db_name == "default" {
                    return Ok(Arc::clone(default_handler));
                }

                // Fast path: read lock to check cache.
                {
                    let guard = tenants.read().await;
                    if let Some(handler) = guard.get(db_name) {
                        return Ok(Arc::clone(handler));
                    }
                }

                // Slow path: write lock, double-check, then create.
                let mut guard = tenants.write().await;
                if let Some(handler) = guard.get(db_name) {
                    return Ok(Arc::clone(handler));
                }

                let superadmin_dsn = superadmin_dsn.clone();
                let db_name_owned = db_name.to_owned();

                // Provision the physical database (may already exist — that's
                // fine, we just open a connection to it).
                let tenant_conn_str = axon_storage::tenant_dsn(&superadmin_dsn, &db_name_owned);

                // Spawn the blocking connect on a dedicated thread so we don't
                // block the async runtime.
                let handler = tokio::task::spawn_blocking(move || {
                    // Attempt to provision; ignore AlreadyExists.
                    match axon_storage::provision_postgres_database(&superadmin_dsn, &db_name_owned) {
                        Ok(()) | Err(axon_core::error::AxonError::AlreadyExists(_)) => {}
                        Err(e) => {
                            return Err(format!(
                                "failed to provision PostgreSQL database 'axon_{db_name_owned}': {e}"
                            ))
                        }
                    }

                    let storage = PostgresStorageAdapter::connect(&tenant_conn_str).map_err(|e| {
                        format!(
                            "failed to connect to tenant PostgreSQL database 'axon_{db_name_owned}': {e}"
                        )
                    })?;
                    let boxed: Box<dyn StorageAdapter + Send + Sync> = Box::new(storage);
                    Ok(Arc::new(Mutex::new(AxonHandler::new(boxed))))
                })
                .await
                .map_err(|e| format!("thread join error while provisioning tenant: {e}"))??;

                guard.insert(db_name.to_owned(), Arc::clone(&handler));
                Ok(handler)
            }
            RouterBackend::Sqlite { .. } => Err(
                "get_or_create_pg is not supported in SQLite mode; use get_or_create".to_owned(),
            ),
        }
    }

    /// Look up or create the handler for `db_name`, dispatching to the
    /// correct backend automatically.
    ///
    /// This is the primary entry point for the HTTP gateway middleware: it
    /// returns a `TenantHandler` regardless of whether the backing store is
    /// SQLite or PostgreSQL.
    pub async fn get_or_create_any(&self, db_name: &str) -> Result<TenantHandler, String> {
        if self.is_postgres() {
            self.get_or_create_pg(db_name).await
        } else {
            self.get_or_create(db_name).await
        }
    }

    /// Return the names of all cached tenant databases (including "default").
    pub async fn list_databases(&self) -> Vec<String> {
        let keys: Vec<String> = match &self.backend {
            RouterBackend::Sqlite { tenants, .. } => tenants.read().await.keys().cloned().collect(),
            RouterBackend::Postgres { tenants, .. } => {
                tenants.read().await.keys().cloned().collect()
            }
        };
        let mut names: Vec<String> = std::iter::once("default".to_owned()).chain(keys).collect();
        names.sort();
        names
    }

    /// Remove a tenant from the cache and clean up its storage.
    ///
    /// - In **SQLite mode**: removes the cached handler and deletes the `.db`
    ///   file from disk.
    /// - In **Postgres mode**: removes the cached handler and calls
    ///   [`axon_storage::deprovision_postgres_database`] to drop the physical
    ///   PostgreSQL database.
    ///
    /// Returns an error if `db_name` is `"default"` (that database cannot be
    /// dropped).
    pub async fn drop_database(&self, db_name: &str) -> Result<(), String> {
        if db_name == "default" {
            return Err("cannot drop the default database".to_owned());
        }

        match &self.backend {
            RouterBackend::Sqlite { tenants, .. } => {
                let mut guard = tenants.write().await;
                guard.remove(db_name);

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
            RouterBackend::Postgres {
                superadmin_dsn,
                tenants,
                ..
            } => {
                let mut guard = tenants.write().await;
                guard.remove(db_name);
                drop(guard);

                let superadmin_dsn = superadmin_dsn.clone();
                let db_name_owned = db_name.to_owned();

                tokio::task::spawn_blocking(move || {
                    match axon_storage::deprovision_postgres_database(
                        &superadmin_dsn,
                        &db_name_owned,
                    ) {
                        Ok(()) => Ok(()),
                        // Already gone — treat as success.
                        Err(axon_core::error::AxonError::NotFound(_)) => Ok(()),
                        Err(e) => Err(format!(
                            "failed to drop PostgreSQL database 'axon_{db_name_owned}': {e}"
                        )),
                    }
                })
                .await
                .map_err(|e| format!("thread join error while dropping tenant: {e}"))?
            }
        }
    }

    /// Return `true` when the router is in PostgreSQL mode.
    pub fn is_postgres(&self) -> bool {
        matches!(self.backend, RouterBackend::Postgres { .. })
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// Create a `TenantRouter` backed by a temporary directory.
    fn make_router(tmp: &Path) -> TenantRouter {
        let default_storage: Box<dyn StorageAdapter + Send + Sync> = Box::new(
            SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite should open"),
        );
        let default_handler = Arc::new(Mutex::new(AxonHandler::new(default_storage)));
        TenantRouter::new(tmp.to_path_buf(), default_handler)
    }

    #[tokio::test]
    async fn get_or_create_default_returns_default_handler() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let router = make_router(tmp.path());

        let handler = router.get_or_create("default").await.expect("default");
        // Should be the same Arc as the default handler.
        let default = router.default_handler();
        assert!(Arc::ptr_eq(&handler, default));
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
            result.expect_err("dropping 'default' should fail").contains("cannot drop the default"),
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
        let storage: Box<dyn StorageAdapter + Send + Sync> = Box::new(
            SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite should open"),
        );
        let handler: TenantHandler = Arc::new(Mutex::new(AxonHandler::new(storage)));
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
