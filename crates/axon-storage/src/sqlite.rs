use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};

use axon_audit::entry::AuditEntry;
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_core::types::Entity;
use axon_schema::schema::{CollectionSchema, CollectionView};

use crate::adapter::StorageAdapter;

/// SQLite-backed storage adapter using an embedded database.
///
/// Schema:
/// ```sql
/// CREATE TABLE entities (
///     collection TEXT NOT NULL,
///     id         TEXT NOT NULL,
///     version    INTEGER NOT NULL,
///     data       TEXT NOT NULL,
///     PRIMARY KEY (collection, id)
/// );
/// ```
///
/// Transactions use SQLite's `BEGIN IMMEDIATE` / `COMMIT` / `ROLLBACK`
/// statements. `BEGIN IMMEDIATE` acquires a write lock up-front, eliminating
/// the TOCTOU window that exists when a read and write are issued as separate
/// statements.
pub struct SqliteStorageAdapter {
    conn: Connection,
    /// `true` while a `BEGIN` has been issued but not yet committed or rolled back.
    in_tx: bool,
}

impl SqliteStorageAdapter {
    /// Opens (or creates) a SQLite database at the given path.
    pub fn open(path: &str) -> Result<Self, AxonError> {
        let conn = Connection::open(path).map_err(|e| AxonError::Storage(e.to_string()))?;
        let adapter = Self { conn, in_tx: false };
        adapter.init_schema()?;
        Ok(adapter)
    }

    /// Opens an in-memory SQLite database (useful for testing).
    pub fn open_in_memory() -> Result<Self, AxonError> {
        let conn = Connection::open_in_memory().map_err(|e| AxonError::Storage(e.to_string()))?;
        let adapter = Self { conn, in_tx: false };
        adapter.init_schema()?;
        Ok(adapter)
    }

    fn init_schema(&self) -> Result<(), AxonError> {
        self.conn
            .execute_batch(
                "PRAGMA foreign_keys = ON;
                CREATE TABLE IF NOT EXISTS entities (
                    collection TEXT NOT NULL,
                    id         TEXT NOT NULL,
                    version    INTEGER NOT NULL,
                    data       TEXT NOT NULL,
                    PRIMARY KEY (collection, id)
                );
                CREATE TABLE IF NOT EXISTS schema_versions (
                    collection  TEXT NOT NULL,
                    version     INTEGER NOT NULL,
                    schema_json TEXT NOT NULL,
                    created_at  INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY (collection, version)
                );
                CREATE TABLE IF NOT EXISTS collections (
                    name TEXT NOT NULL PRIMARY KEY
                );
                CREATE TABLE IF NOT EXISTS collection_views (
                    collection        TEXT NOT NULL PRIMARY KEY
                                      REFERENCES collections(name) ON DELETE CASCADE,
                    version           INTEGER NOT NULL,
                    view_json         TEXT NOT NULL,
                    updated_at_ns     INTEGER NOT NULL,
                    updated_by        TEXT
                );
                CREATE TABLE IF NOT EXISTS audit_log (
                    id             INTEGER PRIMARY KEY AUTOINCREMENT,
                    timestamp_ns   INTEGER NOT NULL,
                    collection     TEXT NOT NULL,
                    entity_id      TEXT NOT NULL,
                    version        INTEGER NOT NULL,
                    mutation       TEXT NOT NULL,
                    actor          TEXT NOT NULL,
                    transaction_id TEXT,
                    entry_json     TEXT NOT NULL
                );",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))
    }

    fn collection_exists(&self, collection: &CollectionId) -> Result<bool, AxonError> {
        let exists: i64 = self
            .conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM collections WHERE name = ?1)",
                params![collection.as_str()],
                |row| row.get(0),
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(exists != 0)
    }

    fn row_to_entity(
        collection: String,
        id: String,
        version: u64,
        data_json: String,
    ) -> Result<Entity, AxonError> {
        let data: serde_json::Value = serde_json::from_str(&data_json)?;
        Ok(Entity {
            collection: CollectionId::new(collection),
            id: EntityId::new(id),
            version,
            data,
            created_at_ns: None,
            updated_at_ns: None,
            created_by: None,
            updated_by: None,
        })
    }
}

// rusqlite::Connection is not Send by default when built without the
// `send_sync` feature. For now we mark the adapter Send + Sync manually
// since callers are expected to use it from a single thread (embedded mode).
// A production multi-threaded adapter would use a connection pool.
#[allow(unsafe_code)]
unsafe impl Send for SqliteStorageAdapter {}
#[allow(unsafe_code)]
unsafe impl Sync for SqliteStorageAdapter {}

impl StorageAdapter for SqliteStorageAdapter {
    fn get(&self, collection: &CollectionId, id: &EntityId) -> Result<Option<Entity>, AxonError> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT collection, id, version, data FROM entities
                 WHERE collection = ?1 AND id = ?2",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let mut rows = stmt
            .query(params![collection.as_str(), id.as_str()])
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        if let Some(row) = rows.next().map_err(|e| AxonError::Storage(e.to_string()))? {
            let entity = Self::row_to_entity(
                row.get(0).map_err(|e| AxonError::Storage(e.to_string()))?,
                row.get(1).map_err(|e| AxonError::Storage(e.to_string()))?,
                row.get::<_, i64>(2)
                    .map_err(|e| AxonError::Storage(e.to_string()))? as u64,
                row.get(3).map_err(|e| AxonError::Storage(e.to_string()))?,
            )?;
            Ok(Some(entity))
        } else {
            Ok(None)
        }
    }

    fn put(&mut self, entity: Entity) -> Result<(), AxonError> {
        let data_json = serde_json::to_string(&entity.data)?;
        self.conn
            .execute(
                "INSERT OR REPLACE INTO entities (collection, id, version, data)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    entity.collection.as_str(),
                    entity.id.as_str(),
                    entity.version as i64,
                    data_json,
                ],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn delete(&mut self, collection: &CollectionId, id: &EntityId) -> Result<(), AxonError> {
        self.conn
            .execute(
                "DELETE FROM entities WHERE collection = ?1 AND id = ?2",
                params![collection.as_str(), id.as_str()],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn count(&self, collection: &CollectionId) -> Result<usize, AxonError> {
        let n: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM entities WHERE collection = ?1",
                params![collection.as_str()],
                |row| row.get(0),
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(n as usize)
    }

    fn range_scan(
        &self,
        collection: &CollectionId,
        start: Option<&EntityId>,
        end: Option<&EntityId>,
        limit: Option<usize>,
    ) -> Result<Vec<Entity>, AxonError> {
        // Build a query with optional start/end bounds. SQLite does not support
        // binding NULL in place of a comparison, so we use conditional predicates.
        let start_str = start.map(|s| s.as_str().to_owned());
        let end_str = end.map(|e| e.as_str().to_owned());
        let limit_val = limit.map(|l| l as i64).unwrap_or(i64::MAX);

        let sql = "SELECT collection, id, version, data FROM entities
                   WHERE collection = ?1
                     AND (?2 IS NULL OR id >= ?2)
                     AND (?3 IS NULL OR id <= ?3)
                   ORDER BY id ASC
                   LIMIT ?4";

        let mut stmt = self
            .conn
            .prepare_cached(sql)
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let rows = stmt
            .query_map(
                params![collection.as_str(), start_str, end_str, limit_val],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let mut entities = Vec::new();
        for row in rows {
            let (col, id, version, data_json) =
                row.map_err(|e| AxonError::Storage(e.to_string()))?;
            entities.push(Self::row_to_entity(col, id, version as u64, data_json)?);
        }
        Ok(entities)
    }

    fn compare_and_swap(
        &mut self,
        entity: Entity,
        expected_version: u64,
    ) -> Result<Entity, AxonError> {
        // Check current version first.
        let current = self.get(&entity.collection, &entity.id)?;
        let actual_version = current.as_ref().map(|e| e.version).unwrap_or(0);

        if actual_version != expected_version {
            return Err(AxonError::ConflictingVersion {
                expected: expected_version,
                actual: actual_version,
                current_entity: current.map(Box::new),
            });
        }

        let new_version = expected_version + 1;
        let data_json = serde_json::to_string(&entity.data)?;

        let changed = self
            .conn
            .execute(
                "UPDATE entities SET version = ?1, data = ?2
                 WHERE collection = ?3 AND id = ?4 AND version = ?5",
                params![
                    new_version as i64,
                    data_json,
                    entity.collection.as_str(),
                    entity.id.as_str(),
                    expected_version as i64,
                ],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        if changed == 0 {
            // A concurrent writer changed the version between our read and write.
            let current_after_race = self.get(&entity.collection, &entity.id)?;
            let actual = current_after_race.as_ref().map(|e| e.version).unwrap_or(0);
            return Err(AxonError::ConflictingVersion {
                expected: expected_version,
                actual,
                current_entity: current_after_race.map(Box::new),
            });
        }

        Ok(Entity {
            version: new_version,
            ..entity
        })
    }

    fn begin_tx(&mut self) -> Result<(), AxonError> {
        if self.in_tx {
            return Err(AxonError::Storage("transaction already active".into()));
        }
        self.conn
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.in_tx = true;
        Ok(())
    }

    fn commit_tx(&mut self) -> Result<(), AxonError> {
        if !self.in_tx {
            return Err(AxonError::Storage("no active transaction".into()));
        }
        self.conn
            .execute_batch("COMMIT")
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.in_tx = false;
        Ok(())
    }

    fn abort_tx(&mut self) -> Result<(), AxonError> {
        if !self.in_tx {
            return Ok(());
        }
        self.conn
            .execute_batch("ROLLBACK")
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.in_tx = false;
        Ok(())
    }

    fn append_audit_entry(&mut self, mut entry: AuditEntry) -> Result<AuditEntry, AxonError> {
        // Assign timestamp if the caller left it at the zero sentinel.
        if entry.timestamp_ns == 0 {
            entry.timestamp_ns = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0);
        }

        // Serialize the full entry. The `id` field will be 0 here; the
        // canonical `id` is the SQLite AUTOINCREMENT rowid stored in the `id`
        // column. Readers reconstruct the entry from `entry_json` and override
        // `id` with the column value.
        let entry_json =
            serde_json::to_string(&entry).map_err(|e| AxonError::Storage(e.to_string()))?;

        self.conn
            .execute(
                "INSERT INTO audit_log
                     (timestamp_ns, collection, entity_id, version, mutation, actor,
                      transaction_id, entry_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    entry.timestamp_ns as i64,
                    entry.collection.as_str(),
                    entry.entity_id.as_str(),
                    entry.version as i64,
                    entry.mutation.to_string(),
                    entry.actor,
                    entry.transaction_id,
                    entry_json,
                ],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        entry.id = self.conn.last_insert_rowid() as u64;
        Ok(entry)
    }

    fn put_schema(&mut self, schema: &CollectionSchema) -> Result<(), AxonError> {
        // Auto-increment: find current max version for this collection.
        let max_version: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_versions WHERE collection = ?1",
                params![schema.collection.as_str()],
                |row| row.get(0),
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        let next_version = max_version + 1;

        let mut versioned = schema.clone();
        versioned.version = next_version as u32;
        let schema_json = serde_json::to_string(&versioned)?;

        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as i64;

        self.conn
            .execute(
                "INSERT INTO schema_versions (collection, version, schema_json, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    schema.collection.as_str(),
                    next_version,
                    schema_json,
                    now_ns,
                ],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn get_schema(&self, collection: &CollectionId) -> Result<Option<CollectionSchema>, AxonError> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT schema_json FROM schema_versions
                 WHERE collection = ?1 ORDER BY version DESC LIMIT 1",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let mut rows = stmt
            .query(params![collection.as_str()])
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        if let Some(row) = rows.next().map_err(|e| AxonError::Storage(e.to_string()))? {
            let json: String = row.get(0).map_err(|e| AxonError::Storage(e.to_string()))?;
            let schema: CollectionSchema = serde_json::from_str(&json)?;
            Ok(Some(schema))
        } else {
            Ok(None)
        }
    }

    fn get_schema_version(
        &self,
        collection: &CollectionId,
        version: u32,
    ) -> Result<Option<CollectionSchema>, AxonError> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT schema_json FROM schema_versions
                 WHERE collection = ?1 AND version = ?2",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let mut rows = stmt
            .query(params![collection.as_str(), version as i64])
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        if let Some(row) = rows.next().map_err(|e| AxonError::Storage(e.to_string()))? {
            let json: String = row.get(0).map_err(|e| AxonError::Storage(e.to_string()))?;
            let schema: CollectionSchema = serde_json::from_str(&json)?;
            Ok(Some(schema))
        } else {
            Ok(None)
        }
    }

    fn list_schema_versions(
        &self,
        collection: &CollectionId,
    ) -> Result<Vec<(u32, u64)>, AxonError> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT version, created_at FROM schema_versions
                 WHERE collection = ?1 ORDER BY version ASC",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let rows = stmt
            .query_map(params![collection.as_str()], |row| {
                Ok((row.get::<_, i64>(0)? as u32, row.get::<_, i64>(1)? as u64))
            })
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let mut result = vec![];
        for row in rows {
            result.push(row.map_err(|e| AxonError::Storage(e.to_string()))?);
        }
        Ok(result)
    }

    fn delete_schema(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        self.conn
            .execute(
                "DELETE FROM schema_versions WHERE collection = ?1",
                params![collection.as_str()],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn put_collection_view(&mut self, view: &CollectionView) -> Result<CollectionView, AxonError> {
        if !self.collection_exists(&view.collection)? {
            return Err(AxonError::InvalidArgument(format!(
                "collection '{}' is not registered",
                view.collection.as_str()
            )));
        }

        let current_version: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(version, 0) FROM collection_views WHERE collection = ?1",
                params![view.collection.as_str()],
                |row| row.get(0),
            )
            .or_else(|_| Ok(0))
            .map_err(|e: rusqlite::Error| AxonError::Storage(e.to_string()))?;
        let next_version = current_version + 1;

        let updated_at_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as i64;

        let mut versioned = view.clone();
        versioned.version = next_version as u32;
        versioned.updated_at_ns = Some(updated_at_ns as u64);
        let view_json = serde_json::to_string(&versioned)?;

        self.conn
            .execute(
                "INSERT INTO collection_views (collection, version, view_json, updated_at_ns, updated_by)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(collection) DO UPDATE SET
                     version = excluded.version,
                     view_json = excluded.view_json,
                     updated_at_ns = excluded.updated_at_ns,
                     updated_by = excluded.updated_by",
                params![
                    view.collection.as_str(),
                    next_version,
                    view_json,
                    updated_at_ns,
                    versioned.updated_by.as_deref(),
                ],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(versioned)
    }

    fn get_collection_view(
        &self,
        collection: &CollectionId,
    ) -> Result<Option<CollectionView>, AxonError> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT view_json FROM collection_views
                 WHERE collection = ?1",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let mut rows = stmt
            .query(params![collection.as_str()])
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        if let Some(row) = rows.next().map_err(|e| AxonError::Storage(e.to_string()))? {
            let json: String = row.get(0).map_err(|e| AxonError::Storage(e.to_string()))?;
            let view: CollectionView = serde_json::from_str(&json)?;
            Ok(Some(view))
        } else {
            Ok(None)
        }
    }

    fn delete_collection_view(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        self.conn
            .execute(
                "DELETE FROM collection_views WHERE collection = ?1",
                params![collection.as_str()],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn register_collection(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO collections (name) VALUES (?1)",
                params![collection.as_str()],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn unregister_collection(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        self.conn
            .execute(
                "DELETE FROM collections WHERE name = ?1",
                params![collection.as_str()],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn list_collections(&self) -> Result<Vec<CollectionId>, AxonError> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT name FROM collections ORDER BY name ASC")
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let mut names = Vec::new();
        for row in rows {
            names.push(CollectionId::new(
                row.map_err(|e| AxonError::Storage(e.to_string()))?,
            ));
        }
        Ok(names)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tasks() -> CollectionId {
        CollectionId::new("tasks")
    }

    fn entity(id: &str) -> Entity {
        Entity::new(tasks(), EntityId::new(id), json!({"title": id}))
    }

    fn store() -> SqliteStorageAdapter {
        SqliteStorageAdapter::open_in_memory().expect("test operation should succeed")
    }

    #[test]
    fn create_entity() {
        let mut s = store();
        s.put(entity("t-001"))
            .expect("test operation should succeed");
        assert_eq!(s.count(&tasks()).expect("test operation should succeed"), 1);
    }

    #[test]
    fn read_entity() {
        let mut s = store();
        s.put(entity("t-001"))
            .expect("test operation should succeed");
        let e = s
            .get(&tasks(), &EntityId::new("t-001"))
            .expect("test operation should succeed")
            .expect("test operation should succeed");
        assert_eq!(e.id.as_str(), "t-001");
        assert_eq!(e.data["title"], "t-001");
        assert_eq!(e.version, 1);
    }

    #[test]
    fn delete_entity() {
        let mut s = store();
        s.put(entity("t-001"))
            .expect("test operation should succeed");
        s.delete(&tasks(), &EntityId::new("t-001"))
            .expect("test operation should succeed");
        assert!(s
            .get(&tasks(), &EntityId::new("t-001"))
            .expect("test operation should succeed")
            .is_none());
    }

    #[test]
    fn range_scan_returns_sorted_results() {
        let mut s = store();
        for i in [3, 1, 2] {
            s.put(entity(&format!("t-00{i}")))
                .expect("test operation should succeed");
        }
        let results = s
            .range_scan(&tasks(), None, None, None)
            .expect("test operation should succeed");
        let ids: Vec<_> = results.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["t-001", "t-002", "t-003"]);
    }

    #[test]
    fn range_scan_with_bounds_and_limit() {
        let mut s = store();
        for i in 1..=5 {
            s.put(entity(&format!("t-00{i}")))
                .expect("test operation should succeed");
        }
        let start = EntityId::new("t-002");
        let end = EntityId::new("t-004");
        let results = s
            .range_scan(&tasks(), Some(&start), Some(&end), Some(2))
            .expect("test operation should succeed");
        let ids: Vec<_> = results.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["t-002", "t-003"]);
    }

    #[test]
    fn update_with_version_check_succeeds() {
        let mut s = store();
        s.put(entity("t-001"))
            .expect("test operation should succeed");
        let updated = s
            .compare_and_swap(entity("t-001"), 1)
            .expect("test operation should succeed");
        assert_eq!(updated.version, 2);
        let stored = s
            .get(&tasks(), &EntityId::new("t-001"))
            .expect("test operation should succeed")
            .expect("test operation should succeed");
        assert_eq!(stored.version, 2);
    }

    #[test]
    fn version_conflict_detected_and_rejected() {
        let mut s = store();
        s.put(entity("t-001"))
            .expect("test operation should succeed");
        let err = s
            .compare_and_swap(entity("t-001"), 99)
            .expect_err("test operation should fail");
        assert!(
            matches!(
                err,
                AxonError::ConflictingVersion {
                    expected: 99,
                    actual: 1,
                    ..
                }
            ),
            "unexpected error: {err}"
        );
        if let AxonError::ConflictingVersion { current_entity, .. } = err {
            let ce =
                current_entity.expect("current_entity must be present on wrong-version conflict");
            assert_eq!(ce.version, 1);
        }
    }

    #[test]
    fn compare_and_swap_missing_entity_rejected() {
        let mut s = store();
        let err = s
            .compare_and_swap(entity("ghost"), 1)
            .expect_err("test operation should fail");
        assert!(
            matches!(
                err,
                AxonError::ConflictingVersion {
                    expected: 1,
                    actual: 0,
                    ..
                }
            ),
            "unexpected error: {err}"
        );
        if let AxonError::ConflictingVersion { current_entity, .. } = err {
            assert!(
                current_entity.is_none(),
                "no entity for missing-entity conflict"
            );
        }
    }

    #[test]
    fn begin_commit_tx_persists_writes() {
        let mut s = store();
        s.begin_tx().expect("test operation should succeed");
        s.put(entity("t-001"))
            .expect("test operation should succeed");
        s.commit_tx().expect("test operation should succeed");
        assert!(s
            .get(&tasks(), &EntityId::new("t-001"))
            .expect("test operation should succeed")
            .is_some());
    }

    #[test]
    fn abort_tx_rolls_back_writes() {
        let mut s = store();
        s.put(entity("t-existing"))
            .expect("test operation should succeed");

        s.begin_tx().expect("test operation should succeed");
        s.put(entity("t-new"))
            .expect("test operation should succeed");
        s.delete(&tasks(), &EntityId::new("t-existing"))
            .expect("test operation should succeed");
        s.abort_tx().expect("test operation should succeed");

        assert!(s
            .get(&tasks(), &EntityId::new("t-new"))
            .expect("test operation should succeed")
            .is_none());
        assert!(s
            .get(&tasks(), &EntityId::new("t-existing"))
            .expect("test operation should succeed")
            .is_some());
    }

    #[test]
    fn begin_tx_rejects_nested_begin() {
        let mut s = store();
        s.begin_tx().expect("test operation should succeed");
        assert!(s.begin_tx().is_err());
        s.abort_tx().expect("test operation should succeed");
    }

    #[test]
    fn commit_tx_requires_active_transaction() {
        let mut s = store();
        assert!(s.commit_tx().is_err());
    }

    #[test]
    fn abort_tx_without_active_tx_is_noop() {
        let mut s = store();
        s.abort_tx().expect("test operation should succeed");
    }

    // ── Schema persistence ───────────────────────────────────────────────────

    #[test]
    fn put_get_schema_roundtrip() {
        use axon_schema::schema::CollectionSchema;
        let mut s = store();
        let col = tasks();
        let schema = CollectionSchema {
            collection: col.clone(),
            description: Some("test schema".into()),
            version: 99, // ignored — auto-increment assigns v1
            entity_schema: Some(json!({"type": "object"})),
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
        };

        s.put_schema(&schema)
            .expect("test operation should succeed");

        let retrieved = s.get_schema(&col).expect("test operation should succeed");
        assert!(retrieved.is_some());
        let retrieved = retrieved.expect("test operation should succeed");
        assert_eq!(retrieved.collection, col);
        assert_eq!(retrieved.version, 1); // auto-incremented
        assert_eq!(retrieved.description.as_deref(), Some("test schema"));
    }

    #[test]
    fn get_schema_missing_returns_none() {
        let s = store();
        assert!(s
            .get_schema(&tasks())
            .expect("test operation should succeed")
            .is_none());
    }

    #[test]
    fn put_schema_overwrites_previous() {
        use axon_schema::schema::CollectionSchema;
        let mut s = store();
        let col = tasks();

        let v1 = CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: None,
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
        };
        let v2 = CollectionSchema {
            collection: col.clone(),
            description: Some("v2".into()),
            version: 2,
            entity_schema: None,
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
        };

        s.put_schema(&v1).expect("test operation should succeed");
        s.put_schema(&v2).expect("test operation should succeed");

        let retrieved = s
            .get_schema(&col)
            .expect("test operation should succeed")
            .expect("test operation should succeed");
        assert_eq!(
            retrieved.version, 2,
            "second put_schema must overwrite the first"
        );
    }

    #[test]
    fn delete_schema_removes_it() {
        use axon_schema::schema::CollectionSchema;
        let mut s = store();
        let col = tasks();
        let schema = CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: None,
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
        };

        s.put_schema(&schema)
            .expect("test operation should succeed");
        assert!(s
            .get_schema(&col)
            .expect("test operation should succeed")
            .is_some());

        s.delete_schema(&col)
            .expect("test operation should succeed");
        assert!(s
            .get_schema(&col)
            .expect("test operation should succeed")
            .is_none());
    }

    // ── Audit co-location ────────────────────────────────────────────────────

    #[test]
    fn append_audit_entry_assigns_id_and_timestamp() {
        use axon_audit::entry::{AuditEntry, MutationType};
        use axon_core::id::{CollectionId, EntityId};
        use serde_json::json;

        let mut s = store();
        let entry = AuditEntry::new(
            CollectionId::new("tasks"),
            EntityId::new("t-001"),
            1,
            MutationType::EntityCreate,
            None,
            Some(json!({"title": "hello"})),
            Some("agent-1".into()),
        );
        assert_eq!(entry.id, 0);
        assert_eq!(entry.timestamp_ns, 0);

        let stored = s
            .append_audit_entry(entry)
            .expect("test operation should succeed");
        assert_eq!(stored.id, 1, "first entry gets id=1");
        assert!(stored.timestamp_ns > 0, "timestamp_ns is assigned");
    }

    #[test]
    fn audit_entry_rolled_back_with_entity_on_abort() {
        use axon_audit::entry::{AuditEntry, MutationType};
        use axon_core::id::EntityId;
        use serde_json::json;

        let mut s = store();

        // Begin a transaction, write an entity and an audit entry, then abort.
        s.begin_tx().expect("test operation should succeed");
        s.put(entity("t-001"))
            .expect("test operation should succeed");
        let entry = AuditEntry::new(
            tasks(),
            EntityId::new("t-001"),
            1,
            MutationType::EntityCreate,
            None,
            Some(json!({"title": "t-001"})),
            None,
        );
        s.append_audit_entry(entry)
            .expect("test operation should succeed");
        s.abort_tx().expect("test operation should succeed");

        // Entity must be absent.
        assert!(s
            .get(&tasks(), &EntityId::new("t-001"))
            .expect("test operation should succeed")
            .is_none());

        // Audit entry must also be absent (rolled back with the transaction).
        let count: i64 = s
            .conn
            .query_row("SELECT COUNT(*) FROM audit_log", [], |r| r.get(0))
            .expect("test operation should succeed");
        assert_eq!(count, 0, "audit entry must be rolled back with the entity");
    }

    #[test]
    fn audit_entry_persists_with_entity_on_commit() {
        use axon_audit::entry::{AuditEntry, MutationType};
        use axon_core::id::EntityId;
        use serde_json::json;

        let mut s = store();

        s.begin_tx().expect("test operation should succeed");
        s.put(entity("t-001"))
            .expect("test operation should succeed");
        let entry = AuditEntry::new(
            tasks(),
            EntityId::new("t-001"),
            1,
            MutationType::EntityCreate,
            None,
            Some(json!({"title": "t-001"})),
            Some("tester".into()),
        );
        s.append_audit_entry(entry)
            .expect("test operation should succeed");
        s.commit_tx().expect("test operation should succeed");

        // Entity must be present.
        assert!(s
            .get(&tasks(), &EntityId::new("t-001"))
            .expect("test operation should succeed")
            .is_some());

        // Audit entry must also be present.
        let count: i64 = s
            .conn
            .query_row("SELECT COUNT(*) FROM audit_log", [], |r| r.get(0))
            .expect("test operation should succeed");
        assert_eq!(
            count, 1,
            "audit entry must persist when transaction commits"
        );
    }
}

// L4 conformance test suite for SqliteStorageAdapter.
crate::storage_conformance_tests!(
    sqlite_conformance,
    SqliteStorageAdapter::open_in_memory().expect("test operation should succeed")
);
