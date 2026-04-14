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

use rusqlite::{params, Connection};

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
        let set = self.0.read().unwrap();
        set.contains("*") || set.contains(origin)
    }

    /// Returns `true` if the wildcard entry `"*"` is present.
    pub fn is_wildcard(&self) -> bool {
        self.0.read().unwrap().contains("*")
    }

    /// Returns `true` if no origins have been configured.
    pub fn is_empty(&self) -> bool {
        self.0.read().unwrap().is_empty()
    }

    /// Add an origin to the in-memory cache.
    /// The caller is responsible for also persisting to the DB.
    pub fn add_cached(&self, origin: impl Into<String>) {
        self.0.write().unwrap().insert(origin.into());
    }

    /// Remove an origin from the in-memory cache.
    /// Returns `true` if the entry was present.
    /// The caller is responsible for also persisting to the DB.
    pub fn remove_cached(&self, origin: &str) -> bool {
        self.0.write().unwrap().remove(origin)
    }

    /// Snapshot of all configured origins, sorted for stable output.
    pub fn list(&self) -> Vec<String> {
        let mut origins: Vec<String> = self.0.read().unwrap().iter().cloned().collect();
        origins.sort();
        origins
    }

    /// Populate the in-memory cache from a pre-loaded list.
    /// Used during server startup after reading from the database.
    pub fn load_from_entries(&self, origins: Vec<String>) {
        let mut set = self.0.write().unwrap();
        for o in origins {
            set.insert(o);
        }
    }
}

// ── SQLite persistence helpers ────────────────────────────────────────────────

/// Add the `cors_origins` table to an existing control-plane database.
/// Called from `ControlPlaneDb::migrate()`.
pub fn migrate_cors_origins(conn: &Connection) -> Result<(), AxonError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS cors_origins (
             origin     TEXT PRIMARY KEY,
             created_at TEXT NOT NULL
         );",
    )
    .map_err(|e| AxonError::Storage(e.to_string()))
}

/// Load all allowed origins from the database.
pub fn db_list(conn: &Connection) -> Result<Vec<String>, AxonError> {
    let mut stmt = conn
        .prepare("SELECT origin FROM cors_origins ORDER BY origin")
        .map_err(|e| AxonError::Storage(e.to_string()))?;

    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| AxonError::Storage(e.to_string()))?;

    let mut origins = Vec::new();
    for row in rows {
        origins.push(row.map_err(|e| AxonError::Storage(e.to_string()))?);
    }
    Ok(origins)
}

/// Insert an allowed origin (idempotent — no error if already present).
pub fn db_add(conn: &Connection, origin: &str) -> Result<(), AxonError> {
    let now = crate::user_roles::chrono_now();
    conn.execute(
        "INSERT INTO cors_origins (origin, created_at) VALUES (?1, ?2)
         ON CONFLICT(origin) DO NOTHING",
        params![origin, now],
    )
    .map_err(|e| AxonError::Storage(e.to_string()))?;
    Ok(())
}

/// Remove an allowed origin from the database.
/// Returns `true` if a row was deleted.
pub fn db_remove(conn: &Connection, origin: &str) -> Result<bool, AxonError> {
    let n = conn
        .execute(
            "DELETE FROM cors_origins WHERE origin = ?1",
            params![origin],
        )
        .map_err(|e| AxonError::Storage(e.to_string()))?;
    Ok(n > 0)
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

    #[test]
    fn db_round_trip() {
        let conn = Connection::open_in_memory().unwrap();
        migrate_cors_origins(&conn).unwrap();
        db_add(&conn, "https://sindri:5173").unwrap();
        db_add(&conn, "*").unwrap();
        let origins = db_list(&conn).unwrap();
        assert_eq!(origins.len(), 2);
        assert!(origins.contains(&"*".to_string()));
        assert!(origins.contains(&"https://sindri:5173".to_string()));
    }

    #[test]
    fn db_add_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        migrate_cors_origins(&conn).unwrap();
        db_add(&conn, "https://foo:3000").unwrap();
        db_add(&conn, "https://foo:3000").unwrap(); // should not error
        assert_eq!(db_list(&conn).unwrap().len(), 1);
    }

    #[test]
    fn db_remove_returns_true_on_deletion() {
        let conn = Connection::open_in_memory().unwrap();
        migrate_cors_origins(&conn).unwrap();
        db_add(&conn, "https://foo:3000").unwrap();
        assert!(db_remove(&conn, "https://foo:3000").unwrap());
        assert!(db_list(&conn).unwrap().is_empty());
    }

    #[test]
    fn db_remove_returns_false_when_absent() {
        let conn = Connection::open_in_memory().unwrap();
        migrate_cors_origins(&conn).unwrap();
        assert!(!db_remove(&conn, "https://nobody:9000").unwrap());
    }
}
