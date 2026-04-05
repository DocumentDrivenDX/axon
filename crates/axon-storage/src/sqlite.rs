use rusqlite::{params, Connection};

use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_core::types::Entity;
use axon_schema::schema::CollectionSchema;

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
                "CREATE TABLE IF NOT EXISTS entities (
                    collection TEXT NOT NULL,
                    id         TEXT NOT NULL,
                    version    INTEGER NOT NULL,
                    data       TEXT NOT NULL,
                    PRIMARY KEY (collection, id)
                );
                CREATE TABLE IF NOT EXISTS schemas (
                    collection  TEXT NOT NULL PRIMARY KEY,
                    version     INTEGER NOT NULL,
                    schema_json TEXT NOT NULL
                );",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))
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
        })
    }
}

// rusqlite::Connection is not Send by default when built without the
// `send_sync` feature. For now we mark the adapter Send + Sync manually
// since callers are expected to use it from a single thread (embedded mode).
// A production multi-threaded adapter would use a connection pool.
unsafe impl Send for SqliteStorageAdapter {}
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
            let actual = self
                .get(&entity.collection, &entity.id)?
                .map(|e| e.version)
                .unwrap_or(0);
            return Err(AxonError::ConflictingVersion {
                expected: expected_version,
                actual,
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

    fn put_schema(&mut self, schema: &CollectionSchema) -> Result<(), AxonError> {
        let schema_json = serde_json::to_string(schema)?;
        self.conn
            .execute(
                "INSERT OR REPLACE INTO schemas (collection, version, schema_json)
                 VALUES (?1, ?2, ?3)",
                params![
                    schema.collection.as_str(),
                    schema.version as i64,
                    schema_json
                ],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn get_schema(&self, collection: &CollectionId) -> Result<Option<CollectionSchema>, AxonError> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT schema_json FROM schemas WHERE collection = ?1")
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

    fn delete_schema(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        self.conn
            .execute(
                "DELETE FROM schemas WHERE collection = ?1",
                params![collection.as_str()],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
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
        SqliteStorageAdapter::open_in_memory().unwrap()
    }

    #[test]
    fn create_entity() {
        let mut s = store();
        s.put(entity("t-001")).unwrap();
        assert_eq!(s.count(&tasks()).unwrap(), 1);
    }

    #[test]
    fn read_entity() {
        let mut s = store();
        s.put(entity("t-001")).unwrap();
        let e = s.get(&tasks(), &EntityId::new("t-001")).unwrap().unwrap();
        assert_eq!(e.id.as_str(), "t-001");
        assert_eq!(e.data["title"], "t-001");
        assert_eq!(e.version, 1);
    }

    #[test]
    fn delete_entity() {
        let mut s = store();
        s.put(entity("t-001")).unwrap();
        s.delete(&tasks(), &EntityId::new("t-001")).unwrap();
        assert!(s.get(&tasks(), &EntityId::new("t-001")).unwrap().is_none());
    }

    #[test]
    fn range_scan_returns_sorted_results() {
        let mut s = store();
        for i in [3, 1, 2] {
            s.put(entity(&format!("t-00{i}"))).unwrap();
        }
        let results = s.range_scan(&tasks(), None, None, None).unwrap();
        let ids: Vec<_> = results.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["t-001", "t-002", "t-003"]);
    }

    #[test]
    fn range_scan_with_bounds_and_limit() {
        let mut s = store();
        for i in 1..=5 {
            s.put(entity(&format!("t-00{i}"))).unwrap();
        }
        let start = EntityId::new("t-002");
        let end = EntityId::new("t-004");
        let results = s
            .range_scan(&tasks(), Some(&start), Some(&end), Some(2))
            .unwrap();
        let ids: Vec<_> = results.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["t-002", "t-003"]);
    }

    #[test]
    fn update_with_version_check_succeeds() {
        let mut s = store();
        s.put(entity("t-001")).unwrap();
        let updated = s.compare_and_swap(entity("t-001"), 1).unwrap();
        assert_eq!(updated.version, 2);
        let stored = s.get(&tasks(), &EntityId::new("t-001")).unwrap().unwrap();
        assert_eq!(stored.version, 2);
    }

    #[test]
    fn version_conflict_detected_and_rejected() {
        let mut s = store();
        s.put(entity("t-001")).unwrap();
        let err = s.compare_and_swap(entity("t-001"), 99).unwrap_err();
        assert!(
            matches!(
                err,
                AxonError::ConflictingVersion {
                    expected: 99,
                    actual: 1
                }
            ),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn compare_and_swap_missing_entity_rejected() {
        let mut s = store();
        let err = s.compare_and_swap(entity("ghost"), 1).unwrap_err();
        assert!(
            matches!(
                err,
                AxonError::ConflictingVersion {
                    expected: 1,
                    actual: 0
                }
            ),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn begin_commit_tx_persists_writes() {
        let mut s = store();
        s.begin_tx().unwrap();
        s.put(entity("t-001")).unwrap();
        s.commit_tx().unwrap();
        assert!(s.get(&tasks(), &EntityId::new("t-001")).unwrap().is_some());
    }

    #[test]
    fn abort_tx_rolls_back_writes() {
        let mut s = store();
        s.put(entity("t-existing")).unwrap();

        s.begin_tx().unwrap();
        s.put(entity("t-new")).unwrap();
        s.delete(&tasks(), &EntityId::new("t-existing")).unwrap();
        s.abort_tx().unwrap();

        assert!(s.get(&tasks(), &EntityId::new("t-new")).unwrap().is_none());
        assert!(s
            .get(&tasks(), &EntityId::new("t-existing"))
            .unwrap()
            .is_some());
    }

    #[test]
    fn begin_tx_rejects_nested_begin() {
        let mut s = store();
        s.begin_tx().unwrap();
        assert!(s.begin_tx().is_err());
        s.abort_tx().unwrap();
    }

    #[test]
    fn commit_tx_requires_active_transaction() {
        let mut s = store();
        assert!(s.commit_tx().is_err());
    }

    #[test]
    fn abort_tx_without_active_tx_is_noop() {
        let mut s = store();
        s.abort_tx().unwrap();
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
            version: 2,
            entity_schema: Some(json!({"type": "object"})),
        };

        s.put_schema(&schema).unwrap();

        let retrieved = s.get_schema(&col).unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.collection, col);
        assert_eq!(retrieved.version, 2);
        assert_eq!(retrieved.description.as_deref(), Some("test schema"));
    }

    #[test]
    fn get_schema_missing_returns_none() {
        let s = store();
        assert!(s.get_schema(&tasks()).unwrap().is_none());
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
        };
        let v2 = CollectionSchema {
            collection: col.clone(),
            description: Some("v2".into()),
            version: 2,
            entity_schema: None,
        };

        s.put_schema(&v1).unwrap();
        s.put_schema(&v2).unwrap();

        let retrieved = s.get_schema(&col).unwrap().unwrap();
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
        };

        s.put_schema(&schema).unwrap();
        assert!(s.get_schema(&col).unwrap().is_some());

        s.delete_schema(&col).unwrap();
        assert!(s.get_schema(&col).unwrap().is_none());
    }
}
