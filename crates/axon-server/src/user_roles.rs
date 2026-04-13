//! Per-principal role registry — Axon's own RBAC store.
//!
//! Tailscale (or any other IdP) supplies the *identity* (a login string such
//! as `"erik@example.com"`).  Axon owns what that identity is *allowed to do*
//! via this registry.
//!
//! The [`UserRoleStore`] is an in-memory write-through cache.  All mutations
//! immediately update both the in-memory map (so in-flight requests see the
//! change within the next identity-cache TTL window) and the control-plane
//! SQLite database (so assignments survive restarts).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use rusqlite::{params, Connection};

use axon_core::error::AxonError;

use crate::auth::Role;

// ── UserRoleStore ─────────────────────────────────────────────────────────────

/// In-memory, write-through cache of `principal → role` assignments.
///
/// Cheaply `Clone`d — all clones share the same underlying data.
/// The store is initialised from the control-plane SQLite database on startup
/// via [`UserRoleStore::load_from_db`].
#[derive(Clone, Default)]
pub struct UserRoleStore(Arc<RwLock<HashMap<String, Role>>>);

impl UserRoleStore {
    /// Look up the role assigned to `login`.  Returns `None` if no explicit
    /// assignment exists; the caller should fall back to tag-based or default
    /// role resolution.
    pub fn get(&self, login: &str) -> Option<Role> {
        self.0.read().unwrap().get(login).cloned()
    }

    /// Assign `role` to `login` in the in-memory cache.
    /// The caller is responsible for also persisting to the DB.
    pub fn set_cached(&self, login: impl Into<String>, role: Role) {
        self.0.write().unwrap().insert(login.into(), role);
    }

    /// Remove the explicit role for `login` from the in-memory cache.
    /// Returns `true` if an entry was present.
    /// The caller is responsible for also persisting to the DB.
    pub fn remove_cached(&self, login: &str) -> bool {
        self.0.write().unwrap().remove(login).is_some()
    }

    /// Snapshot of all current assignments, sorted by login for stable output.
    pub fn list(&self) -> Vec<UserRoleEntry> {
        let mut entries: Vec<_> = self
            .0
            .read()
            .unwrap()
            .iter()
            .map(|(login, role)| UserRoleEntry {
                login: login.clone(),
                role: role.clone(),
            })
            .collect();
        entries.sort_by(|a, b| a.login.cmp(&b.login));
        entries
    }

    /// Populate the in-memory cache from a pre-loaded list of entries.
    /// Used during server startup after reading from the database.
    pub fn load_from_entries(&self, entries: Vec<UserRoleEntry>) {
        let mut map = self.0.write().unwrap();
        for entry in entries {
            map.insert(entry.login, entry.role);
        }
    }
}

/// A single principal→role assignment.
#[derive(Debug, Clone)]
pub struct UserRoleEntry {
    pub login: String,
    pub role: Role,
}

// ── SQLite persistence helpers ────────────────────────────────────────────────
//
// These are free functions that operate on an existing rusqlite `Connection`
// (owned by `ControlPlaneDb`).  They are called by the control-plane routes
// and by the CLI, which each have their own way of obtaining the connection.

/// Add the `user_roles` table to an existing control-plane database.
/// This is called from `ControlPlaneDb::migrate()`.
pub fn migrate_user_roles(conn: &Connection) -> Result<(), AxonError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS user_roles (
             login      TEXT PRIMARY KEY,
             role       TEXT NOT NULL,
             granted_at TEXT NOT NULL
         );",
    )
    .map_err(|e| AxonError::Storage(e.to_string()))
}

/// Load all user-role assignments from the database.
pub fn db_list(conn: &Connection) -> Result<Vec<UserRoleEntry>, AxonError> {
    let mut stmt = conn
        .prepare("SELECT login, role FROM user_roles ORDER BY login")
        .map_err(|e| AxonError::Storage(e.to_string()))?;

    let rows = stmt
        .query_map([], |row| {
            let login: String = row.get(0)?;
            let role_str: String = row.get(1)?;
            Ok((login, role_str))
        })
        .map_err(|e| AxonError::Storage(e.to_string()))?;

    let mut entries = Vec::new();
    for row in rows {
        let (login, role_str) = row.map_err(|e| AxonError::Storage(e.to_string()))?;
        let role = parse_role(&role_str)?;
        entries.push(UserRoleEntry { login, role });
    }
    Ok(entries)
}

/// Upsert a user-role assignment in the database.
pub fn db_set(conn: &Connection, login: &str, role: &Role) -> Result<(), AxonError> {
    let now = chrono_now();
    conn.execute(
        "INSERT INTO user_roles (login, role, granted_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(login) DO UPDATE SET role = excluded.role, granted_at = excluded.granted_at",
        params![login, role_str(role), now],
    )
    .map_err(|e| AxonError::Storage(e.to_string()))?;
    Ok(())
}

/// Remove a user-role assignment from the database.
/// Returns `true` if a row was deleted.
pub fn db_remove(conn: &Connection, login: &str) -> Result<bool, AxonError> {
    let n = conn
        .execute("DELETE FROM user_roles WHERE login = ?1", params![login])
        .map_err(|e| AxonError::Storage(e.to_string()))?;
    Ok(n > 0)
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn role_str(role: &Role) -> &'static str {
    match role {
        Role::Admin => "admin",
        Role::Write => "write",
        Role::Read => "read",
    }
}

fn parse_role(s: &str) -> Result<Role, AxonError> {
    match s {
        "admin" => Ok(Role::Admin),
        "write" => Ok(Role::Write),
        "read" => Ok(Role::Read),
        other => Err(AxonError::InvalidArgument(format!("unknown role: {other}"))),
    }
}

fn chrono_now() -> String {
    // RFC 3339 timestamp without an external dep.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Format as ISO 8601 / RFC 3339 UTC — good enough for audit purposes.
    let (y, mo, d, h, mi, s) = epoch_to_utc(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// Minimal UTC decomposition from a Unix timestamp (no external dep).
fn epoch_to_utc(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let s = secs % 60;
    let mi = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400;

    // Gregorian calendar approximation — accurate from 1970 onwards.
    let mut y = 1970u64;
    let mut rem = days;
    loop {
        let dy = if is_leap(y) { 366 } else { 365 };
        if rem < dy {
            break;
        }
        rem -= dy;
        y += 1;
    }
    let months = if is_leap(y) {
        [31u64, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31u64, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut mo = 1u64;
    for &dm in &months {
        if rem < dm {
            break;
        }
        rem -= dm;
        mo += 1;
    }
    (y, mo, rem + 1, h, mi, s)
}

fn is_leap(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with(entries: &[(&str, Role)]) -> UserRoleStore {
        let store = UserRoleStore::default();
        store.load_from_entries(
            entries
                .iter()
                .map(|(l, r)| UserRoleEntry {
                    login: l.to_string(),
                    role: r.clone(),
                })
                .collect(),
        );
        store
    }

    #[test]
    fn get_returns_assigned_role() {
        let store = store_with(&[("erik@example.com", Role::Admin)]);
        assert_eq!(store.get("erik@example.com"), Some(Role::Admin));
    }

    #[test]
    fn get_returns_none_for_unknown_principal() {
        let store = store_with(&[("erik@example.com", Role::Admin)]);
        assert_eq!(store.get("unknown@example.com"), None);
    }

    #[test]
    fn set_cached_overwrites_existing() {
        let store = store_with(&[("erik@example.com", Role::Read)]);
        store.set_cached("erik@example.com", Role::Write);
        assert_eq!(store.get("erik@example.com"), Some(Role::Write));
    }

    #[test]
    fn remove_cached_returns_true_when_present() {
        let store = store_with(&[("erik@example.com", Role::Admin)]);
        assert!(store.remove_cached("erik@example.com"));
        assert_eq!(store.get("erik@example.com"), None);
    }

    #[test]
    fn remove_cached_returns_false_when_absent() {
        let store = UserRoleStore::default();
        assert!(!store.remove_cached("nobody@example.com"));
    }

    #[test]
    fn list_is_sorted_by_login() {
        let store = store_with(&[
            ("zara@example.com", Role::Read),
            ("alice@example.com", Role::Write),
            ("erik@example.com", Role::Admin),
        ]);
        let logins: Vec<_> = store.list().iter().map(|e| e.login.clone()).collect();
        assert_eq!(logins, ["alice@example.com", "erik@example.com", "zara@example.com"]);
    }

    #[test]
    fn clone_shares_state() {
        let store = UserRoleStore::default();
        let clone = store.clone();
        store.set_cached("erik@example.com", Role::Admin);
        assert_eq!(clone.get("erik@example.com"), Some(Role::Admin));
    }

    #[test]
    fn db_round_trip() {
        let conn = Connection::open_in_memory().unwrap();
        migrate_user_roles(&conn).unwrap();
        db_set(&conn, "erik@example.com", &Role::Admin).unwrap();
        db_set(&conn, "alice@example.com", &Role::Write).unwrap();
        let entries = db_list(&conn).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].login, "alice@example.com");
        assert_eq!(entries[1].login, "erik@example.com");
        assert!(matches!(entries[1].role, Role::Admin));
    }

    #[test]
    fn db_upsert_updates_existing() {
        let conn = Connection::open_in_memory().unwrap();
        migrate_user_roles(&conn).unwrap();
        db_set(&conn, "erik@example.com", &Role::Read).unwrap();
        db_set(&conn, "erik@example.com", &Role::Admin).unwrap();
        let entries = db_list(&conn).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0].role, Role::Admin));
    }

    #[test]
    fn db_remove_returns_true_on_deletion() {
        let conn = Connection::open_in_memory().unwrap();
        migrate_user_roles(&conn).unwrap();
        db_set(&conn, "erik@example.com", &Role::Admin).unwrap();
        assert!(db_remove(&conn, "erik@example.com").unwrap());
        assert!(db_list(&conn).unwrap().is_empty());
    }

    #[test]
    fn db_remove_returns_false_when_absent() {
        let conn = Connection::open_in_memory().unwrap();
        migrate_user_roles(&conn).unwrap();
        assert!(!db_remove(&conn, "nobody@example.com").unwrap());
    }
}
