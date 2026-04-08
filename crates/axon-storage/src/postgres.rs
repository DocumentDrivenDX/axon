use std::cell::RefCell;
use std::time::{SystemTime, UNIX_EPOCH};

use postgres::{Client, NoTls};

use axon_audit::entry::AuditEntry;
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_core::types::Entity;
use axon_schema::schema::{CollectionSchema, CollectionView};

use crate::adapter::StorageAdapter;

/// PostgreSQL-backed storage adapter.
///
/// Uses the synchronous `postgres` crate. The `Client` is wrapped in
/// `RefCell` because `postgres::Client::query` requires `&mut self` but
/// `StorageAdapter::get` and other read methods take `&self`.
///
/// Transactions are handled via `BEGIN` / `COMMIT` / `ROLLBACK` statements.
/// The adapter creates the required tables on initialization if they do not
/// exist.
pub struct PostgresStorageAdapter {
    client: RefCell<Client>,
    in_tx: bool,
}

impl PostgresStorageAdapter {
    /// Connect to a PostgreSQL database using a connection string.
    ///
    /// Example: `"host=localhost user=axon dbname=axon"`
    pub fn connect(params: &str) -> Result<Self, AxonError> {
        let client =
            Client::connect(params, NoTls).map_err(|e| AxonError::Storage(e.to_string()))?;
        let mut adapter = Self {
            client: RefCell::new(client),
            in_tx: false,
        };
        adapter.init_schema()?;
        Ok(adapter)
    }

    fn init_schema(&mut self) -> Result<(), AxonError> {
        self.client
            .borrow_mut()
            .batch_execute(
                "CREATE TABLE IF NOT EXISTS entities (
                    collection TEXT NOT NULL,
                    id         TEXT NOT NULL,
                    version    BIGINT NOT NULL,
                    data       JSONB NOT NULL,
                    PRIMARY KEY (collection, id)
                );
                CREATE TABLE IF NOT EXISTS schemas (
                    collection  TEXT NOT NULL PRIMARY KEY,
                    version     INTEGER NOT NULL,
                    schema_json JSONB NOT NULL
                );
                CREATE TABLE IF NOT EXISTS collections (
                    name TEXT NOT NULL PRIMARY KEY
                );
                CREATE TABLE IF NOT EXISTS collection_views (
                    collection    TEXT NOT NULL PRIMARY KEY
                                  REFERENCES collections(name) ON DELETE CASCADE,
                    version       INTEGER NOT NULL,
                    view_json     JSONB NOT NULL,
                    updated_at_ns BIGINT NOT NULL,
                    updated_by    TEXT
                );
                CREATE TABLE IF NOT EXISTS audit_log (
                    id             BIGSERIAL PRIMARY KEY,
                    timestamp_ns   BIGINT NOT NULL,
                    collection     TEXT NOT NULL,
                    entity_id      TEXT NOT NULL,
                    version        BIGINT NOT NULL,
                    mutation       TEXT NOT NULL,
                    actor          TEXT NOT NULL,
                    transaction_id TEXT,
                    entry_json     JSONB NOT NULL
                );",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))
    }

    fn collection_exists(&self, collection: &CollectionId) -> Result<bool, AxonError> {
        let row = self
            .client
            .borrow_mut()
            .query_one(
                "SELECT EXISTS(SELECT 1 FROM collections WHERE name = $1)",
                &[&collection.as_str()],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(row.get(0))
    }

    fn row_to_entity(row: &postgres::Row) -> Result<Entity, AxonError> {
        let collection: String = row.get("collection");
        let id: String = row.get("id");
        let version: i64 = row.get("version");
        let data: serde_json::Value = row.get("data");
        Ok(Entity {
            collection: CollectionId::new(collection),
            id: EntityId::new(id),
            version: version as u64,
            data,
            created_at_ns: None,
            updated_at_ns: None,
            created_by: None,
            updated_by: None,
        })
    }
}

// PostgreSQL's Client is not Send (it holds a TcpStream), but the StorageAdapter
// trait requires Send + Sync. We use unsafe impl because a single adapter
// instance is only accessed from one thread at a time behind a Mutex.
#[allow(unsafe_code)]
unsafe impl Send for PostgresStorageAdapter {}
#[allow(unsafe_code)]
unsafe impl Sync for PostgresStorageAdapter {}

impl StorageAdapter for PostgresStorageAdapter {
    fn get(&self, collection: &CollectionId, id: &EntityId) -> Result<Option<Entity>, AxonError> {
        let rows = self
            .client.borrow_mut()
            .query(
                "SELECT collection, id, version, data FROM entities WHERE collection = $1 AND id = $2",
                &[&collection.as_str(), &id.as_str()],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        match rows.first() {
            Some(row) => Ok(Some(Self::row_to_entity(row)?)),
            None => Ok(None),
        }
    }

    fn put(&mut self, entity: Entity) -> Result<(), AxonError> {
        let data_json = serde_json::to_value(&entity.data)?;
        self.client
            .borrow_mut()
            .execute(
                "INSERT INTO entities (collection, id, version, data) VALUES ($1, $2, $3, $4)
                 ON CONFLICT (collection, id) DO UPDATE SET version = $3, data = $4",
                &[
                    &entity.collection.as_str(),
                    &entity.id.as_str(),
                    &(entity.version as i64),
                    &data_json,
                ],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn delete(&mut self, collection: &CollectionId, id: &EntityId) -> Result<(), AxonError> {
        self.client
            .borrow_mut()
            .execute(
                "DELETE FROM entities WHERE collection = $1 AND id = $2",
                &[&collection.as_str(), &id.as_str()],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn count(&self, collection: &CollectionId) -> Result<usize, AxonError> {
        let row = self
            .client
            .borrow_mut()
            .query_one(
                "SELECT COUNT(*) FROM entities WHERE collection = $1",
                &[&collection.as_str()],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        let count: i64 = row.get(0);
        Ok(count as usize)
    }

    fn range_scan(
        &self,
        collection: &CollectionId,
        start: Option<&EntityId>,
        end: Option<&EntityId>,
        limit: Option<usize>,
    ) -> Result<Vec<Entity>, AxonError> {
        let start_str = start.map(|s| s.as_str().to_string());
        let end_str = end.map(|e| e.as_str().to_string());
        let limit_val = limit.map(|l| l as i64);

        let rows = self
            .client
            .borrow_mut()
            .query(
                "SELECT collection, id, version, data FROM entities
                 WHERE collection = $1
                   AND ($2::text IS NULL OR id >= $2)
                   AND ($3::text IS NULL OR id <= $3)
                 ORDER BY id ASC
                 LIMIT $4",
                &[
                    &collection.as_str(),
                    &start_str.as_deref(),
                    &end_str.as_deref(),
                    &limit_val,
                ],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        rows.iter().map(Self::row_to_entity).collect()
    }

    fn compare_and_swap(
        &mut self,
        entity: Entity,
        expected_version: u64,
    ) -> Result<Entity, AxonError> {
        // Read current version.
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
        let data_json = serde_json::to_value(&entity.data)?;

        let changed = self
            .client
            .borrow_mut()
            .execute(
                "UPDATE entities SET version = $3, data = $4
                 WHERE collection = $1 AND id = $2 AND version = $5",
                &[
                    &entity.collection.as_str(),
                    &entity.id.as_str(),
                    &(new_version as i64),
                    &data_json,
                    &(expected_version as i64),
                ],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        if changed == 0 {
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
        self.client
            .borrow_mut()
            .batch_execute("BEGIN")
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.in_tx = true;
        Ok(())
    }

    fn commit_tx(&mut self) -> Result<(), AxonError> {
        if !self.in_tx {
            return Err(AxonError::Storage("no active transaction".into()));
        }
        self.client
            .borrow_mut()
            .batch_execute("COMMIT")
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.in_tx = false;
        Ok(())
    }

    fn abort_tx(&mut self) -> Result<(), AxonError> {
        if !self.in_tx {
            return Ok(());
        }
        self.client
            .borrow_mut()
            .batch_execute("ROLLBACK")
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.in_tx = false;
        Ok(())
    }

    fn append_audit_entry(&mut self, mut entry: AuditEntry) -> Result<AuditEntry, AxonError> {
        if entry.timestamp_ns == 0 {
            entry.timestamp_ns = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0);
        }

        let entry_json = serde_json::to_value(&entry)?;

        let row = self
            .client.borrow_mut()
            .query_one(
                "INSERT INTO audit_log (timestamp_ns, collection, entity_id, version, mutation, actor, transaction_id, entry_json)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                 RETURNING id",
                &[
                    &(entry.timestamp_ns as i64),
                    &entry.collection.as_str(),
                    &entry.entity_id.as_str(),
                    &(entry.version as i64),
                    &entry.mutation.to_string().as_str(),
                    &entry.actor.as_str(),
                    &entry.transaction_id.as_deref(),
                    &entry_json,
                ],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let id: i64 = row.get(0);
        entry.id = id as u64;

        Ok(entry)
    }

    fn put_schema(&mut self, schema: &CollectionSchema) -> Result<(), AxonError> {
        let schema_json = serde_json::to_value(schema)?;
        self.client
            .borrow_mut()
            .execute(
                "INSERT INTO schemas (collection, version, schema_json) VALUES ($1, $2, $3)
                 ON CONFLICT (collection) DO UPDATE SET version = $2, schema_json = $3",
                &[
                    &schema.collection.as_str(),
                    &(schema.version as i32),
                    &schema_json,
                ],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn get_schema(&self, collection: &CollectionId) -> Result<Option<CollectionSchema>, AxonError> {
        let rows = self
            .client
            .borrow_mut()
            .query(
                "SELECT schema_json FROM schemas WHERE collection = $1",
                &[&collection.as_str()],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        match rows.first() {
            Some(row) => {
                let schema_json: serde_json::Value = row.get("schema_json");
                let schema: CollectionSchema = serde_json::from_value(schema_json)?;
                Ok(Some(schema))
            }
            None => Ok(None),
        }
    }

    fn delete_schema(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        self.client
            .borrow_mut()
            .execute(
                "DELETE FROM schemas WHERE collection = $1",
                &[&collection.as_str()],
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

        let current_version = self
            .client
            .borrow_mut()
            .query_opt(
                "SELECT version FROM collection_views WHERE collection = $1",
                &[&view.collection.as_str()],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?
            .map_or(0, |row| row.get::<_, i32>("version"));
        let next_version = current_version + 1;

        let updated_at_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as i64;

        let mut versioned = view.clone();
        versioned.version = next_version as u32;
        versioned.updated_at_ns = Some(updated_at_ns as u64);
        let view_json = serde_json::to_value(&versioned)?;

        self.client
            .borrow_mut()
            .execute(
                "INSERT INTO collection_views (collection, version, view_json, updated_at_ns, updated_by)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (collection) DO UPDATE SET
                     version = EXCLUDED.version,
                     view_json = EXCLUDED.view_json,
                     updated_at_ns = EXCLUDED.updated_at_ns,
                     updated_by = EXCLUDED.updated_by",
                &[
                    &view.collection.as_str(),
                    &next_version,
                    &view_json,
                    &updated_at_ns,
                    &versioned.updated_by,
                ],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(versioned)
    }

    fn get_collection_view(
        &self,
        collection: &CollectionId,
    ) -> Result<Option<CollectionView>, AxonError> {
        let rows = self
            .client
            .borrow_mut()
            .query(
                "SELECT view_json FROM collection_views WHERE collection = $1",
                &[&collection.as_str()],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        match rows.first() {
            Some(row) => {
                let view_json: serde_json::Value = row.get("view_json");
                let view: CollectionView = serde_json::from_value(view_json)?;
                Ok(Some(view))
            }
            None => Ok(None),
        }
    }

    fn delete_collection_view(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        self.client
            .borrow_mut()
            .execute(
                "DELETE FROM collection_views WHERE collection = $1",
                &[&collection.as_str()],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn register_collection(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        self.client
            .borrow_mut()
            .execute(
                "INSERT INTO collections (name) VALUES ($1) ON CONFLICT DO NOTHING",
                &[&collection.as_str()],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn unregister_collection(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        // Upgraded databases may still have a pre-fix collection_views table
        // without the collection -> collections foreign key, so clean up the
        // view row explicitly instead of relying solely on ON DELETE CASCADE.
        self.client
            .borrow_mut()
            .execute(
                "DELETE FROM collection_views WHERE collection = $1",
                &[&collection.as_str()],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.client
            .borrow_mut()
            .execute(
                "DELETE FROM collections WHERE name = $1",
                &[&collection.as_str()],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn list_collections(&self) -> Result<Vec<CollectionId>, AxonError> {
        let rows = self
            .client
            .borrow_mut()
            .query("SELECT name FROM collections ORDER BY name ASC", &[])
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        Ok(rows
            .iter()
            .map(|row| {
                let name: String = row.get("name");
                CollectionId::new(name)
            })
            .collect())
    }
}

// The conformance test suite requires a running PostgreSQL instance.
// Run with: AXON_TEST_POSTGRES="host=localhost user=axon dbname=axon_test" cargo test -p axon-storage postgres_conformance
#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard, OnceLock};

    use super::*;

    fn postgres_test_guard() -> MutexGuard<'static, ()> {
        static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
        GUARD
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("PostgreSQL test guard lock should not be poisoned")
    }

    fn pg_url() -> Option<String> {
        match std::env::var("AXON_TEST_POSTGRES") {
            Ok(url) => Some(url),
            Err(_) if std::env::var_os("CI").is_some() => {
                panic!("AXON_TEST_POSTGRES must be set during automated verification")
            }
            Err(_) => None,
        }
    }

    fn reset_test_tables(client: &mut Client) -> Result<(), AxonError> {
        client
            .batch_execute(
                "DROP TABLE IF EXISTS collection_views;
                 DROP TABLE IF EXISTS collections;
                 DROP TABLE IF EXISTS entities;
                 DROP TABLE IF EXISTS schemas;
                 DROP TABLE IF EXISTS audit_log;",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))
    }

    fn store() -> Result<Option<PostgresStorageAdapter>, AxonError> {
        let Some(url) = pg_url() else {
            return Ok(None);
        };
        let adapter = PostgresStorageAdapter::connect(&url)?;
        // Clean tables for a fresh test.
        adapter
            .client
            .borrow_mut()
            .batch_execute(
                "TRUNCATE entities, schemas, collection_views, collections, audit_log RESTART IDENTITY CASCADE",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(Some(adapter))
    }

    #[test]
    fn postgres_roundtrip_when_available() {
        let _guard = postgres_test_guard();
        let Some(mut s) = store().expect("PostgreSQL test setup should succeed") else {
            eprintln!("Skipping PostgreSQL test: AXON_TEST_POSTGRES not set");
            return;
        };

        let col = CollectionId::new("tasks");
        let entity = Entity::new(
            col.clone(),
            EntityId::new("t-001"),
            serde_json::json!({"title": "hello"}),
        );
        s.put(entity).unwrap();
        let got = s.get(&col, &EntityId::new("t-001")).unwrap().unwrap();
        assert_eq!(got.data["title"], "hello");
        assert_eq!(got.version, 1);
    }

    #[test]
    fn unregister_collection_cleans_up_legacy_collection_views_when_available() {
        let _guard = postgres_test_guard();
        let Some(url) = pg_url() else {
            eprintln!("Skipping PostgreSQL test: AXON_TEST_POSTGRES not set");
            return;
        };

        let mut legacy_client =
            Client::connect(&url, NoTls).expect("PostgreSQL test database should be reachable");
        reset_test_tables(&mut legacy_client).expect("test tables should reset cleanly");
        legacy_client
            .batch_execute(
                "CREATE TABLE collections (
                    name TEXT NOT NULL PRIMARY KEY
                );
                CREATE TABLE collection_views (
                    collection    TEXT NOT NULL PRIMARY KEY,
                    version       INTEGER NOT NULL,
                    view_json     JSONB NOT NULL,
                    updated_at_ns BIGINT NOT NULL,
                    updated_by    TEXT
                );",
            )
            .expect("legacy collection metadata schema should be created");

        let collection = CollectionId::new("ephemeral");
        let legacy_view = CollectionView {
            collection: collection.clone(),
            description: Some("legacy view".into()),
            markdown_template: "# {{title}}".into(),
            version: 1,
            updated_at_ns: Some(42),
            updated_by: Some("legacy".into()),
        };
        let legacy_view_json =
            serde_json::to_value(&legacy_view).expect("legacy collection view should serialize");

        legacy_client
            .execute(
                "INSERT INTO collections (name) VALUES ($1)",
                &[&collection.as_str()],
            )
            .expect("legacy collection should insert");
        legacy_client
            .execute(
                "INSERT INTO collection_views (collection, version, view_json, updated_at_ns, updated_by)
                 VALUES ($1, $2, $3, $4, $5)",
                &[
                    &collection.as_str(),
                    &1_i32,
                    &legacy_view_json,
                    &42_i64,
                    &Some("legacy"),
                ],
            )
            .expect("legacy collection view should insert");
        drop(legacy_client);

        let mut adapter =
            PostgresStorageAdapter::connect(&url).expect("adapter should connect after upgrade");
        assert!(
            adapter
                .get_collection_view(&collection)
                .expect("legacy view should be readable after upgrade")
                .is_some(),
            "upgraded adapter should observe the pre-fix collection view"
        );

        adapter
            .unregister_collection(&collection)
            .expect("unregister_collection should succeed on upgraded database");

        assert!(
            adapter
                .get_collection_view(&collection)
                .expect("collection view lookup should succeed after unregister")
                .is_none(),
            "legacy collection view should be removed during unregister"
        );

        let remaining_views: i64 = adapter
            .client
            .borrow_mut()
            .query_one(
                "SELECT COUNT(*) FROM collection_views WHERE collection = $1",
                &[&collection.as_str()],
            )
            .expect("remaining collection views query should succeed")
            .get(0);
        assert_eq!(
            remaining_views, 0,
            "stale collection view rows must be deleted"
        );
    }
}
