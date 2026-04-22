//! CORS allowed-origin registry — persisted in the control-plane SQLite database.
//!
//! [`CorsStore`] is an in-memory write-through cache of allowed origins.  The
//! special entry `"*"` enables wildcard mode (all origins permitted), which is
//! appropriate when the network boundary (Tailscale) is the security layer.
//!
//! Origins are managed via:
//! - `PUT  /control/cors`    (REST)
//! - `DELETE /control/cors`  (REST)
//! - `axon cors add/remove/list` (CLI)

use std::collections::HashSet;
use std::sync::{Arc, RwLock};

use sqlx::sqlite::SqlitePool;
use sqlx::Row;

use axon_core::error::AxonError;

// ── CorsStore ────────────────────────────────────────────────────────────────

/// In-memory write-through cache of allowed CORS origins.
///
/// Cheaply `Clone`d — all clones share the same underlying data.
/// The special entry `"*"` enables wildcard mode.
#[derive(Clone, Default)]
pub struct CorsStore(Arc<RwLock<HashSet<String>>>);

impl CorsStore {
    /// Returns `true` if `origin` is explicitly allowed or wildcard (`*`) is set.
    pub fn is_allowed(&self, origin: &str) -> bool {
        let set = self.0.read().expect("cors store read lock poisoned");
        set.contains("*") || set.contains(origin)
    }

    /// Returns `true` if the wildcard entry `"*"` is present.
    pub fn is_wildcard(&self) -> bool {
        self.0
            .read()
            .expect("cors store read lock poisoned")
            .contains("*")
    }

    /// Returns `true` if no origins have been configured.
    pub fn is_empty(&self) -> bool {
        self.0
            .read()
            .expect("cors store read lock poisoned")
            .is_empty()
    }

    /// Add an origin to the in-memory cache.
    /// The caller is responsible for also persisting to the DB.
    pub fn add_cached(&self, origin: impl Into<String>) {
        self.0
            .write()
            .expect("cors store write lock poisoned")
            .insert(origin.into());
    }

    /// Remove an origin from the in-memory cache.
    /// Returns `true` if the entry was present.
    /// The caller is responsible for also persisting to the DB.
    pub fn remove_cached(&self, origin: &str) -> bool {
        self.0
            .write()
            .expect("cors store write lock poisoned")
            .remove(origin)
    }

    /// Snapshot of all configured origins, sorted for stable output.
    pub fn list(&self) -> Vec<String> {
        let mut origins: Vec<String> = self
            .0
            .read()
            .expect("cors store read lock poisoned")
            .iter()
            .cloned()
            .collect();
        origins.sort();
        origins
    }

    /// Populate the in-memory cache from a pre-loaded list.
    /// Used during server startup after reading from the database.
    pub fn load_from_entries(&self, origins: Vec<String>) {
        let mut set = self.0.write().expect("cors store write lock poisoned");
        for o in origins {
            set.insert(o);
        }
    }
}

// ── SQLite persistence helpers ────────────────────────────────────────────────

/// Add the `cors_origins` table to an existing control-plane database.
/// Called from `ControlPlaneDb::migrate()`.
pub async fn migrate_cors_origins(pool: &SqlitePool) -> Result<(), AxonError> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS cors_origins (
             origin     TEXT PRIMARY KEY,
             created_at TEXT NOT NULL
         )",
    )
    .execute(pool)
    .await
    .map_err(|e| AxonError::Storage(e.to_string()))?;
    Ok(())
}

/// Load all allowed origins from the database.
pub async fn db_list(pool: &SqlitePool) -> Result<Vec<String>, AxonError> {
    let rows = sqlx::query("SELECT origin FROM cors_origins ORDER BY origin")
        .fetch_all(pool)
        .await
        .map_err(|e| AxonError::Storage(e.to_string()))?;

    let origins: Vec<String> = rows.iter().map(|row| row.get("origin")).collect();
    Ok(origins)
}

/// Insert an allowed origin (idempotent — no error if already present).
pub async fn db_add(pool: &SqlitePool, origin: &str) -> Result<(), AxonError> {
    let now = crate::user_roles::chrono_now();
    sqlx::query(
        "INSERT INTO cors_origins (origin, created_at) VALUES (?, ?)
         ON CONFLICT(origin) DO NOTHING",
    )
    .bind(origin)
    .bind(now)
    .execute(pool)
    .await
    .map_err(|e| AxonError::Storage(e.to_string()))?;
    Ok(())
}

/// Remove an allowed origin from the database.
/// Returns `true` if a row was deleted.
pub async fn db_remove(pool: &SqlitePool, origin: &str) -> Result<bool, AxonError> {
    let result = sqlx::query("DELETE FROM cors_origins WHERE origin = ?")
        .bind(origin)
        .execute(pool)
        .await
        .map_err(|e| AxonError::Storage(e.to_string()))?;
    Ok(result.rows_affected() > 0)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn store_with(origins: &[&str]) -> CorsStore {
        let store = CorsStore::default();
        store.load_from_entries(origins.iter().map(|s| (*s).to_string()).collect());
        store
    }

    #[test]
    fn is_allowed_exact_match() {
        let store = store_with(&["https://sindri:5173"]);
        assert!(store.is_allowed("https://sindri:5173"));
        assert!(!store.is_allowed("https://other:5173"));
    }

    #[test]
    fn is_allowed_wildcard() {
        let store = store_with(&["*"]);
        assert!(store.is_allowed("https://anything.example.com"));
    }

    #[test]
    fn is_empty_when_no_origins() {
        let store = CorsStore::default();
        assert!(store.is_empty());
    }

    #[test]
    fn is_not_empty_after_add() {
        let store = CorsStore::default();
        store.add_cached("https://foo:3000");
        assert!(!store.is_empty());
    }

    #[test]
    fn remove_cached_returns_true_when_present() {
        let store = store_with(&["https://foo:3000"]);
        assert!(store.remove_cached("https://foo:3000"));
        assert!(store.is_empty());
    }

    #[test]
    fn remove_cached_returns_false_when_absent() {
        let store = CorsStore::default();
        assert!(!store.remove_cached("https://nobody:9000"));
    }

    #[test]
    fn list_is_sorted() {
        let store = store_with(&["https://z:1", "https://a:2", "https://m:3"]);
        let list = store.list();
        assert_eq!(list, ["https://a:2", "https://m:3", "https://z:1"]);
    }

    #[test]
    fn clone_shares_state() {
        let store = CorsStore::default();
        let clone = store.clone();
        store.add_cached("https://shared:4000");
        assert!(clone.is_allowed("https://shared:4000"));
    }

    async fn test_pool() -> SqlitePool {
        SqlitePool::connect("sqlite::memory:")
            .await
            .expect("open in-memory pool")
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn db_round_trip() {
        let pool = test_pool().await;
        migrate_cors_origins(&pool).await.unwrap();
        db_add(&pool, "https://sindri:5173").await.unwrap();
        db_add(&pool, "*").await.unwrap();
        let origins = db_list(&pool).await.unwrap();
        assert_eq!(origins.len(), 2);
        assert!(origins.contains(&"*".to_string()));
        assert!(origins.contains(&"https://sindri:5173".to_string()));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn db_add_is_idempotent() {
        let pool = test_pool().await;
        migrate_cors_origins(&pool).await.unwrap();
        db_add(&pool, "https://foo:3000").await.unwrap();
        db_add(&pool, "https://foo:3000").await.unwrap(); // should not error
        assert_eq!(db_list(&pool).await.unwrap().len(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn db_remove_returns_true_on_deletion() {
        let pool = test_pool().await;
        migrate_cors_origins(&pool).await.unwrap();
        db_add(&pool, "https://foo:3000").await.unwrap();
        assert!(db_remove(&pool, "https://foo:3000").await.unwrap());
        assert!(db_list(&pool).await.unwrap().is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn db_remove_returns_false_when_absent() {
        let pool = test_pool().await;
        migrate_cors_origins(&pool).await.unwrap();
        assert!(!db_remove(&pool, "https://nobody:9000").await.unwrap());
    }
}
