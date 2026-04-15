use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};

use axon_audit::entry::AuditEntry;
use axon_core::error::AxonError;
use axon_core::id::{
    CollectionId, EntityId, Namespace, QualifiedCollectionId, DEFAULT_DATABASE, DEFAULT_SCHEMA,
};
use axon_core::types::Entity;
use axon_schema::schema::{CollectionSchema, CollectionView};

use crate::adapter::StorageAdapter;

/// SQLite-backed storage adapter using an embedded database.
///
/// The `Connection` is wrapped in a `Mutex` to provide the `Send + Sync`
/// bounds required by `StorageAdapter`.
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
    conn: Mutex<Connection>,
    /// `true` while a `BEGIN` has been issued but not yet committed or rolled back.
    in_tx: bool,
}

impl SqliteStorageAdapter {
    /// Opens (or creates) a SQLite database at the given path.
    pub fn open(path: &str) -> Result<Self, AxonError> {
        let conn = Connection::open(path).map_err(|e| AxonError::Storage(e.to_string()))?;
        let adapter = Self {
            conn: Mutex::new(conn),
            in_tx: false,
        };
        adapter.init_schema()?;
        Ok(adapter)
    }

    /// Opens an in-memory SQLite database (useful for testing).
    pub fn open_in_memory() -> Result<Self, AxonError> {
        let conn = Connection::open_in_memory().map_err(|e| AxonError::Storage(e.to_string()))?;
        let adapter = Self {
            conn: Mutex::new(conn),
            in_tx: false,
        };
        adapter.init_schema()?;
        Ok(adapter)
    }

    /// Acquire the inner `Connection` lock, converting a poisoned-mutex error into
    /// an `AxonError::Storage`.
    fn conn(&self) -> Result<MutexGuard<'_, Connection>, AxonError> {
        self.conn
            .lock()
            .map_err(|e| AxonError::Storage(format!("mutex poisoned: {e}")))
    }

    fn init_schema(&self) -> Result<(), AxonError> {
        self.conn()?
            .execute_batch(
                "PRAGMA foreign_keys = ON;
                CREATE TABLE IF NOT EXISTS databases (
                    name TEXT NOT NULL PRIMARY KEY
                );
                CREATE TABLE IF NOT EXISTS namespaces (
                    database_name TEXT NOT NULL,
                    name          TEXT NOT NULL,
                    PRIMARY KEY (database_name, name),
                    FOREIGN KEY (database_name) REFERENCES databases(name) ON DELETE CASCADE
                );
                CREATE TABLE IF NOT EXISTS entities (
                    collection    TEXT NOT NULL,
                    database_name TEXT NOT NULL DEFAULT 'default',
                    schema_name   TEXT NOT NULL DEFAULT 'default',
                    id            TEXT NOT NULL,
                    version       INTEGER NOT NULL,
                    data          TEXT NOT NULL,
                    PRIMARY KEY (database_name, schema_name, collection, id)
                );
                CREATE TABLE IF NOT EXISTS schema_versions (
                    collection    TEXT NOT NULL,
                    database_name TEXT NOT NULL DEFAULT 'default',
                    schema_name   TEXT NOT NULL DEFAULT 'default',
                    version       INTEGER NOT NULL,
                    schema_json   TEXT NOT NULL,
                    created_at    INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY (database_name, schema_name, collection, version)
                );
                CREATE TABLE IF NOT EXISTS collections (
                    name          TEXT NOT NULL,
                    database_name TEXT NOT NULL DEFAULT 'default',
                    schema_name   TEXT NOT NULL DEFAULT 'default',
                    PRIMARY KEY (database_name, schema_name, name)
                );
                CREATE TABLE IF NOT EXISTS collection_views (
                    collection        TEXT NOT NULL,
                    database_name     TEXT NOT NULL DEFAULT 'default',
                    schema_name       TEXT NOT NULL DEFAULT 'default',
                    version           INTEGER NOT NULL,
                    view_json         TEXT NOT NULL,
                    updated_at_ns     INTEGER NOT NULL,
                    updated_by        TEXT,
                    PRIMARY KEY (database_name, schema_name, collection),
                    FOREIGN KEY (database_name, schema_name, collection)
                        REFERENCES collections(database_name, schema_name, name)
                        ON DELETE CASCADE
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
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.ensure_namespace_catalog_tables()?;
        self.ensure_default_namespace()
    }

    fn collection_exists_in_namespace(
        &self,
        collection: &CollectionId,
        namespace: &Namespace,
    ) -> Result<bool, AxonError> {
        let exists: i64 = self
            .conn()?
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM collections
                    WHERE name = ?1 AND database_name = ?2 AND schema_name = ?3
                )",
                params![
                    collection.as_str(),
                    namespace.database.as_str(),
                    namespace.schema.as_str()
                ],
                |row| row.get(0),
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(exists != 0)
    }

    fn database_exists(&self, database: &str) -> Result<bool, AxonError> {
        let exists: i64 = self
            .conn()?
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM databases WHERE name = ?1)",
                params![database],
                |row| row.get(0),
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(exists != 0)
    }

    fn namespace_exists(&self, namespace: &Namespace) -> Result<bool, AxonError> {
        let exists: i64 = self
            .conn()?
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM namespaces
                    WHERE database_name = ?1 AND name = ?2
                )",
                params![namespace.database.as_str(), namespace.schema.as_str()],
                |row| row.get(0),
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(exists != 0)
    }

    fn table_info(&self, table: &str) -> Result<Vec<(String, i64)>, AxonError> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(1)?, row.get::<_, i64>(5)?))
            })
            .map_err(|e| AxonError::Storage(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(rows)
    }

    fn table_columns(&self, table: &str) -> Result<Vec<String>, AxonError> {
        self.table_info(table)
            .map(|rows| rows.into_iter().map(|(name, _)| name).collect())
    }

    fn table_pk_columns(&self, table: &str) -> Result<Vec<String>, AxonError> {
        let mut rows = self.table_info(table)?;
        rows.retain(|(_, pk)| *pk > 0);
        rows.sort_by_key(|(_, pk)| *pk);
        Ok(rows.into_iter().map(|(name, _)| name).collect())
    }

    fn rebuild_collections_table(
        &self,
        has_database_name: bool,
        has_schema_name: bool,
    ) -> Result<(), AxonError> {
        self.conn()?
            .execute_batch(
                "ALTER TABLE collections RENAME TO collections_legacy;
                 CREATE TABLE collections (
                     name          TEXT NOT NULL,
                     database_name TEXT NOT NULL DEFAULT 'default',
                     schema_name   TEXT NOT NULL DEFAULT 'default',
                     PRIMARY KEY (database_name, schema_name, name)
                 );",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let select_sql = match (has_database_name, has_schema_name) {
            (true, true) => {
                "SELECT name, COALESCE(database_name, 'default'), COALESCE(schema_name, 'default')
                 FROM collections_legacy"
            }
            (true, false) => {
                "SELECT name, COALESCE(database_name, 'default'), 'default'
                 FROM collections_legacy"
            }
            (false, true) => {
                "SELECT name, 'default', COALESCE(schema_name, 'default')
                 FROM collections_legacy"
            }
            (false, false) => {
                "SELECT name, 'default', 'default'
                 FROM collections_legacy"
            }
        };

        self.conn()?
            .execute(
                &format!(
                    "INSERT OR IGNORE INTO collections (name, database_name, schema_name) {select_sql}"
                ),
                [],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.conn()?
            .execute("DROP TABLE collections_legacy", [])
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn rebuild_entities_table(
        &self,
        has_database_name: bool,
        has_schema_name: bool,
    ) -> Result<(), AxonError> {
        self.conn()?
            .execute_batch(
                "ALTER TABLE entities RENAME TO entities_legacy;
                 CREATE TABLE entities (
                     collection    TEXT NOT NULL,
                     database_name TEXT NOT NULL DEFAULT 'default',
                     schema_name   TEXT NOT NULL DEFAULT 'default',
                     id            TEXT NOT NULL,
                     version       INTEGER NOT NULL,
                     data          TEXT NOT NULL,
                     PRIMARY KEY (database_name, schema_name, collection, id)
                 );",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let select_sql = match (has_database_name, has_schema_name) {
            (true, true) => {
                "SELECT collection, COALESCE(database_name, 'default'), COALESCE(schema_name, 'default'), id, version, data
                 FROM entities_legacy"
            }
            (true, false) => {
                "SELECT collection, COALESCE(database_name, 'default'), 'default', id, version, data
                 FROM entities_legacy"
            }
            (false, true) => {
                "SELECT collection, 'default', COALESCE(schema_name, 'default'), id, version, data
                 FROM entities_legacy"
            }
            (false, false) => {
                "SELECT collection, 'default', 'default', id, version, data
                 FROM entities_legacy"
            }
        };

        self.conn()?
            .execute(
                &format!(
                    "INSERT OR REPLACE INTO entities (collection, database_name, schema_name, id, version, data) {select_sql}"
                ),
                [],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.conn()?
            .execute("DROP TABLE entities_legacy", [])
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn rebuild_schema_versions_table(
        &self,
        has_database_name: bool,
        has_schema_name: bool,
    ) -> Result<(), AxonError> {
        self.conn()?
            .execute_batch(
                "ALTER TABLE schema_versions RENAME TO schema_versions_legacy;
                 CREATE TABLE schema_versions (
                     collection    TEXT NOT NULL,
                     database_name TEXT NOT NULL DEFAULT 'default',
                     schema_name   TEXT NOT NULL DEFAULT 'default',
                     version       INTEGER NOT NULL,
                     schema_json   TEXT NOT NULL,
                     created_at    INTEGER NOT NULL DEFAULT 0,
                     PRIMARY KEY (database_name, schema_name, collection, version)
                 );",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let select_sql = match (has_database_name, has_schema_name) {
            (true, true) => {
                "SELECT collection,
                        COALESCE(database_name, 'default'),
                        COALESCE(schema_name, 'default'),
                        version,
                        schema_json,
                        created_at
                 FROM schema_versions_legacy"
            }
            (true, false) => {
                "SELECT collection,
                        COALESCE(database_name, 'default'),
                        'default',
                        version,
                        schema_json,
                        created_at
                 FROM schema_versions_legacy"
            }
            (false, true) => {
                "SELECT collection,
                        'default',
                        COALESCE(schema_name, 'default'),
                        version,
                        schema_json,
                        created_at
                 FROM schema_versions_legacy"
            }
            (false, false) => {
                "SELECT collection, 'default', 'default', version, schema_json, created_at
                 FROM schema_versions_legacy"
            }
        };

        self.conn()?
            .execute(
                &format!(
                    "INSERT INTO schema_versions
                        (collection, database_name, schema_name, version, schema_json, created_at)
                     {select_sql}"
                ),
                [],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.conn()?
            .execute("DROP TABLE schema_versions_legacy", [])
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn rebuild_collection_views_table(
        &self,
        has_database_name: bool,
        has_schema_name: bool,
    ) -> Result<(), AxonError> {
        self.conn()?
            .execute_batch(
                "ALTER TABLE collection_views RENAME TO collection_views_legacy;
                 CREATE TABLE collection_views (
                     collection        TEXT NOT NULL,
                     database_name     TEXT NOT NULL DEFAULT 'default',
                     schema_name       TEXT NOT NULL DEFAULT 'default',
                     version           INTEGER NOT NULL,
                     view_json         TEXT NOT NULL,
                     updated_at_ns     INTEGER NOT NULL,
                     updated_by        TEXT,
                     PRIMARY KEY (database_name, schema_name, collection),
                     FOREIGN KEY (database_name, schema_name, collection)
                         REFERENCES collections(database_name, schema_name, name)
                         ON DELETE CASCADE
                 );",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let select_sql = match (has_database_name, has_schema_name) {
            (true, true) => {
                "SELECT collection,
                        COALESCE(database_name, 'default'),
                        COALESCE(schema_name, 'default'),
                        version,
                        view_json,
                        updated_at_ns,
                        updated_by
                 FROM collection_views_legacy"
            }
            _ => {
                "SELECT v.collection,
                        COALESCE(c.database_name, 'default'),
                        COALESCE(c.schema_name, 'default'),
                        v.version,
                        v.view_json,
                        v.updated_at_ns,
                        v.updated_by
                 FROM collection_views_legacy v
                 LEFT JOIN collections c ON c.name = v.collection"
            }
        };

        self.conn()?
            .execute(
                &format!(
                    "INSERT OR REPLACE INTO collection_views
                        (collection, database_name, schema_name, version, view_json, updated_at_ns, updated_by)
                     {select_sql}"
                ),
                [],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.conn()?
            .execute("DROP TABLE collection_views_legacy", [])
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn ensure_namespace_catalog_tables(&self) -> Result<(), AxonError> {
        let entity_columns = self.table_columns("entities")?;
        let collections_columns = self.table_columns("collections")?;
        let schema_columns = self.table_columns("schema_versions")?;
        let view_columns = self.table_columns("collection_views")?;

        let entities_ok = self.table_pk_columns("entities")?
            == vec!["database_name", "schema_name", "collection", "id"];
        let collections_ok =
            self.table_pk_columns("collections")? == vec!["database_name", "schema_name", "name"];
        let schema_ok = self.table_pk_columns("schema_versions")?
            == vec!["database_name", "schema_name", "collection", "version"];
        let views_ok = self.table_pk_columns("collection_views")?
            == vec!["database_name", "schema_name", "collection"];

        let rebuild_entities = !entities_ok;
        let rebuild_collections = !collections_ok;
        let rebuild_schema = !schema_ok;
        let rebuild_views = !views_ok;

        if !(rebuild_entities || rebuild_collections || rebuild_schema || rebuild_views) {
            self.conn()?
                .execute(
                    "CREATE INDEX IF NOT EXISTS idx_collections_namespace
                     ON collections (database_name, schema_name, name)",
                    [],
                )
                .map_err(|e| AxonError::Storage(e.to_string()))?;
            return Ok(());
        }

        self.conn()?
            .execute_batch("PRAGMA foreign_keys = OFF")
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        if rebuild_entities {
            self.rebuild_entities_table(
                entity_columns
                    .iter()
                    .any(|column| column == "database_name"),
                entity_columns.iter().any(|column| column == "schema_name"),
            )?;
        }
        if rebuild_collections {
            self.rebuild_collections_table(
                collections_columns
                    .iter()
                    .any(|column| column == "database_name"),
                collections_columns
                    .iter()
                    .any(|column| column == "schema_name"),
            )?;
        }
        if rebuild_schema {
            self.rebuild_schema_versions_table(
                schema_columns
                    .iter()
                    .any(|column| column == "database_name"),
                schema_columns.iter().any(|column| column == "schema_name"),
            )?;
        }
        if rebuild_views {
            self.rebuild_collection_views_table(
                view_columns.iter().any(|column| column == "database_name"),
                view_columns.iter().any(|column| column == "schema_name"),
            )?;
        }

        self.conn()?
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_collections_namespace
                 ON collections (database_name, schema_name, name)",
                [],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.conn()?
            .execute_batch("PRAGMA foreign_keys = ON")
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn registered_collection_namespaces(
        &self,
        collection: &CollectionId,
    ) -> Result<Vec<Namespace>, AxonError> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT database_name, schema_name FROM collections
                 WHERE name = ?1
                 ORDER BY CASE
                     WHEN database_name = 'default' AND schema_name = 'default' THEN 0
                     ELSE 1
                 END,
                 database_name,
                 schema_name",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        let namespaces = stmt
            .query_map(params![collection.as_str()], |row| {
                Ok(Namespace::new(
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                ))
            })
            .map_err(|e| AxonError::Storage(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(namespaces)
    }

    fn resolve_catalog_key(
        &self,
        collection: &CollectionId,
    ) -> Result<QualifiedCollectionId, AxonError> {
        let (namespace, bare_collection) = Namespace::parse(collection.as_str());
        if bare_collection != collection.as_str() {
            return Ok(QualifiedCollectionId::from_parts(
                &namespace,
                &CollectionId::new(bare_collection),
            ));
        }

        let namespaces = self.registered_collection_namespaces(collection)?;
        match namespaces.as_slice() {
            [] => Ok(QualifiedCollectionId::from_parts(
                &Namespace::default_ns(),
                collection,
            )),
            [namespace] => Ok(QualifiedCollectionId::from_parts(namespace, collection)),
            _ => {
                let default_namespace = Namespace::default_ns();
                if namespaces.contains(&default_namespace) {
                    Ok(QualifiedCollectionId::from_parts(
                        &default_namespace,
                        collection,
                    ))
                } else {
                    Err(AxonError::InvalidArgument(format!(
                        "collection '{}' exists in multiple namespaces; qualify the namespace",
                        collection.as_str()
                    )))
                }
            }
        }
    }

    fn namespace_collection_keys(
        &self,
        namespace: &Namespace,
    ) -> Result<Vec<QualifiedCollectionId>, AxonError> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT name FROM collections
                 WHERE database_name = ?1 AND schema_name = ?2
                 ORDER BY name ASC",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        let rows = stmt
            .query_map(
                params![namespace.database.as_str(), namespace.schema.as_str()],
                |row| {
                    row.get::<_, String>(0).map(|name| {
                        QualifiedCollectionId::from_parts(namespace, &CollectionId::new(name))
                    })
                },
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AxonError::Storage(e.to_string()))
    }

    fn database_collection_keys(
        &self,
        database: &str,
    ) -> Result<Vec<QualifiedCollectionId>, AxonError> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT schema_name, name FROM collections
                 WHERE database_name = ?1
                 ORDER BY schema_name ASC, name ASC",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        let rows = stmt
            .query_map(params![database], |row| {
                let schema: String = row.get(0)?;
                let collection: String = row.get(1)?;
                Ok(QualifiedCollectionId::from_parts(
                    &Namespace::new(database, schema),
                    &CollectionId::new(collection),
                ))
            })
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AxonError::Storage(e.to_string()))
    }

    fn ensure_default_namespace(&self) -> Result<(), AxonError> {
        self.conn()?
            .execute(
                "INSERT OR IGNORE INTO databases (name) VALUES (?1)",
                params![DEFAULT_DATABASE],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.conn()?
            .execute(
                "INSERT OR IGNORE INTO namespaces (database_name, name) VALUES (?1, ?2)",
                params![DEFAULT_DATABASE, DEFAULT_SCHEMA],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
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
            schema_version: None,
            gate_results: Default::default(),
        })
    }
}

impl StorageAdapter for SqliteStorageAdapter {
    fn resolve_collection_key(
        &self,
        collection: &CollectionId,
    ) -> Result<QualifiedCollectionId, AxonError> {
        self.resolve_catalog_key(collection)
    }

    fn get(&self, collection: &CollectionId, id: &EntityId) -> Result<Option<Entity>, AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT collection, id, version, data FROM entities
                 WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3 AND id = ?4",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let mut rows = stmt
            .query(params![
                key.collection.as_str(),
                key.namespace.database.as_str(),
                key.namespace.schema.as_str(),
                id.as_str()
            ])
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
        let key = self.resolve_catalog_key(&entity.collection)?;
        let data_json = serde_json::to_string(&entity.data)?;
        self.conn()?
            .execute(
                "INSERT OR REPLACE INTO entities (collection, database_name, schema_name, id, version, data)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    key.collection.as_str(),
                    key.namespace.database.as_str(),
                    key.namespace.schema.as_str(),
                    entity.id.as_str(),
                    entity.version as i64,
                    data_json,
                ],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn delete(&mut self, collection: &CollectionId, id: &EntityId) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        self.conn()?
            .execute(
                "DELETE FROM entities
                 WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3 AND id = ?4",
                params![
                    key.collection.as_str(),
                    key.namespace.database.as_str(),
                    key.namespace.schema.as_str(),
                    id.as_str()
                ],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn count(&self, collection: &CollectionId) -> Result<usize, AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        let n: i64 = self
            .conn()?
            .query_row(
                "SELECT COUNT(*) FROM entities
                 WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3",
                params![
                    key.collection.as_str(),
                    key.namespace.database.as_str(),
                    key.namespace.schema.as_str()
                ],
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
        let key = self.resolve_catalog_key(collection)?;
        // Build a query with optional start/end bounds. SQLite does not support
        // binding NULL in place of a comparison, so we use conditional predicates.
        let start_str = start.map(|s| s.as_str().to_owned());
        let end_str = end.map(|e| e.as_str().to_owned());
        let limit_val = limit.map(|l| l as i64).unwrap_or(i64::MAX);

        let sql = "SELECT collection, id, version, data FROM entities
                   WHERE collection = ?1
                     AND database_name = ?2
                     AND schema_name = ?3
                     AND (?4 IS NULL OR id >= ?4)
                     AND (?5 IS NULL OR id <= ?5)
                   ORDER BY id ASC
                   LIMIT ?6";

        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(sql)
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let rows = stmt
            .query_map(
                params![
                    key.collection.as_str(),
                    key.namespace.database.as_str(),
                    key.namespace.schema.as_str(),
                    start_str,
                    end_str,
                    limit_val
                ],
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
        let key = self.resolve_catalog_key(&entity.collection)?;
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
            .conn()?
            .execute(
                "UPDATE entities SET version = ?1, data = ?2
                 WHERE collection = ?3 AND database_name = ?4 AND schema_name = ?5 AND id = ?6 AND version = ?7",
                params![
                    new_version as i64,
                    data_json,
                    key.collection.as_str(),
                    key.namespace.database.as_str(),
                    key.namespace.schema.as_str(),
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
            collection: key.collection,
            version: new_version,
            ..entity
        })
    }

    fn create_if_absent(
        &mut self,
        entity: Entity,
        expected_absent_version: u64,
    ) -> Result<Entity, AxonError> {
        let key = self.resolve_catalog_key(&entity.collection)?;
        let data_json = serde_json::to_string(&entity.data)?;
        let changed = self
            .conn()?
            .execute(
                "INSERT INTO entities (collection, database_name, schema_name, id, version, data)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(database_name, schema_name, collection, id) DO NOTHING",
                params![
                    key.collection.as_str(),
                    key.namespace.database.as_str(),
                    key.namespace.schema.as_str(),
                    entity.id.as_str(),
                    entity.version as i64,
                    data_json,
                ],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        if changed == 0 {
            let current = self.get(&entity.collection, &entity.id)?;
            let actual = current
                .as_ref()
                .map(|existing| existing.version)
                .unwrap_or(0);
            return Err(AxonError::ConflictingVersion {
                expected: expected_absent_version,
                actual,
                current_entity: current.map(Box::new),
            });
        }

        Ok(Entity {
            collection: key.collection,
            ..entity
        })
    }

    fn begin_tx(&mut self) -> Result<(), AxonError> {
        if self.in_tx {
            return Err(AxonError::Storage("transaction already active".into()));
        }
        self.conn()?
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.in_tx = true;
        Ok(())
    }

    fn commit_tx(&mut self) -> Result<(), AxonError> {
        if !self.in_tx {
            return Err(AxonError::Storage("no active transaction".into()));
        }
        self.conn()?
            .execute_batch("COMMIT")
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.in_tx = false;
        Ok(())
    }

    fn abort_tx(&mut self) -> Result<(), AxonError> {
        if !self.in_tx {
            return Ok(());
        }
        self.conn()?
            .execute_batch("ROLLBACK")
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.in_tx = false;
        Ok(())
    }

    fn create_database(&mut self, name: &str) -> Result<(), AxonError> {
        if self.database_exists(name)? {
            return Err(AxonError::AlreadyExists(format!("database '{name}'")));
        }

        self.conn()?
            .execute("INSERT INTO databases (name) VALUES (?1)", params![name])
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.conn()?
            .execute(
                "INSERT INTO namespaces (database_name, name) VALUES (?1, ?2)",
                params![name, DEFAULT_SCHEMA],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn list_databases(&self) -> Result<Vec<String>, AxonError> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT name FROM databases ORDER BY name ASC")
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        let databases = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| AxonError::Storage(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(databases)
    }

    fn drop_database(&mut self, name: &str) -> Result<(), AxonError> {
        if !self.database_exists(name)? {
            return Err(AxonError::NotFound(format!("database '{name}'")));
        }

        let doomed = self.database_collection_keys(name)?;
        self.purge_links_for_collections(&doomed)?;
        self.conn()?
            .execute(
                "DELETE FROM entities WHERE database_name = ?1",
                params![name],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.conn()?
            .execute(
                "DELETE FROM collection_views WHERE database_name = ?1",
                params![name],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.conn()?
            .execute(
                "DELETE FROM schema_versions WHERE database_name = ?1",
                params![name],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.conn()?
            .execute(
                "DELETE FROM collections WHERE database_name = ?1",
                params![name],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.conn()?
            .execute("DELETE FROM databases WHERE name = ?1", params![name])
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn create_namespace(&mut self, namespace: &Namespace) -> Result<(), AxonError> {
        if !self.database_exists(&namespace.database)? {
            return Err(AxonError::NotFound(format!(
                "database '{}'",
                namespace.database
            )));
        }
        if self.namespace_exists(namespace)? {
            return Err(AxonError::AlreadyExists(format!("namespace '{namespace}'")));
        }

        self.conn()?
            .execute(
                "INSERT INTO namespaces (database_name, name) VALUES (?1, ?2)",
                params![namespace.database.as_str(), namespace.schema.as_str()],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn list_namespaces(&self, database: &str) -> Result<Vec<String>, AxonError> {
        if !self.database_exists(database)? {
            return Err(AxonError::NotFound(format!("database '{database}'")));
        }

        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT name FROM namespaces
                 WHERE database_name = ?1
                 ORDER BY name ASC",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        let namespaces = stmt
            .query_map(params![database], |row| row.get::<_, String>(0))
            .map_err(|e| AxonError::Storage(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(namespaces)
    }

    fn drop_namespace(&mut self, namespace: &Namespace) -> Result<(), AxonError> {
        if !self.namespace_exists(namespace)? {
            return Err(AxonError::NotFound(format!("namespace '{namespace}'")));
        }

        let doomed = self.namespace_collection_keys(namespace)?;
        self.purge_links_for_collections(&doomed)?;
        self.conn()?
            .execute(
                "DELETE FROM entities
                 WHERE database_name = ?1 AND schema_name = ?2",
                params![namespace.database.as_str(), namespace.schema.as_str()],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.conn()?
            .execute(
                "DELETE FROM collection_views
                 WHERE database_name = ?1 AND schema_name = ?2",
                params![namespace.database.as_str(), namespace.schema.as_str()],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.conn()?
            .execute(
                "DELETE FROM schema_versions
                 WHERE database_name = ?1 AND schema_name = ?2",
                params![namespace.database.as_str(), namespace.schema.as_str()],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.conn()?
            .execute(
                "DELETE FROM collections
                 WHERE database_name = ?1 AND schema_name = ?2",
                params![namespace.database.as_str(), namespace.schema.as_str()],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.conn()?
            .execute(
                "DELETE FROM namespaces
                 WHERE database_name = ?1 AND name = ?2",
                params![namespace.database.as_str(), namespace.schema.as_str()],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn list_namespace_collections(
        &self,
        namespace: &Namespace,
    ) -> Result<Vec<CollectionId>, AxonError> {
        if !self.namespace_exists(namespace)? {
            return Err(AxonError::NotFound(format!("namespace '{namespace}'")));
        }

        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT name FROM collections
                 WHERE database_name = ?1 AND schema_name = ?2
                 ORDER BY name ASC",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        let collections = stmt
            .query_map(
                params![namespace.database.as_str(), namespace.schema.as_str()],
                |row| row.get::<_, String>(0).map(CollectionId::new),
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(collections)
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

        self.conn()?
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

        entry.id = self.conn()?.last_insert_rowid() as u64;
        Ok(entry)
    }

    fn put_schema(&mut self, schema: &CollectionSchema) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(&schema.collection)?;
        // Auto-increment: find current max version for this collection.
        let max_version: i64 = self
            .conn()?
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_versions
                 WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3",
                params![
                    key.collection.as_str(),
                    key.namespace.database.as_str(),
                    key.namespace.schema.as_str()
                ],
                |row| row.get(0),
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        let next_version = max_version + 1;

        let mut versioned = schema.clone();
        versioned.collection = key.collection.clone();
        versioned.version = next_version as u32;
        let schema_json = serde_json::to_string(&versioned)?;

        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as i64;

        self.conn()?
            .execute(
                "INSERT INTO schema_versions
                    (collection, database_name, schema_name, version, schema_json, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    key.collection.as_str(),
                    key.namespace.database.as_str(),
                    key.namespace.schema.as_str(),
                    next_version,
                    schema_json,
                    now_ns,
                ],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn get_schema(&self, collection: &CollectionId) -> Result<Option<CollectionSchema>, AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT schema_json FROM schema_versions
                 WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3
                 ORDER BY version DESC LIMIT 1",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let mut rows = stmt
            .query(params![
                key.collection.as_str(),
                key.namespace.database.as_str(),
                key.namespace.schema.as_str()
            ])
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
        let key = self.resolve_catalog_key(collection)?;
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT schema_json FROM schema_versions
                 WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3 AND version = ?4",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let mut rows = stmt
            .query(params![
                key.collection.as_str(),
                key.namespace.database.as_str(),
                key.namespace.schema.as_str(),
                version as i64
            ])
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
        let key = self.resolve_catalog_key(collection)?;
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT version, created_at FROM schema_versions
                 WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3
                 ORDER BY version ASC",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let rows = stmt
            .query_map(
                params![
                    key.collection.as_str(),
                    key.namespace.database.as_str(),
                    key.namespace.schema.as_str()
                ],
                |row| Ok((row.get::<_, i64>(0)? as u32, row.get::<_, i64>(1)? as u64)),
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let mut result = vec![];
        for row in rows {
            result.push(row.map_err(|e| AxonError::Storage(e.to_string()))?);
        }
        Ok(result)
    }

    fn delete_schema(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        self.conn()?
            .execute(
                "DELETE FROM schema_versions
                 WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3",
                params![
                    key.collection.as_str(),
                    key.namespace.database.as_str(),
                    key.namespace.schema.as_str()
                ],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn put_collection_view(&mut self, view: &CollectionView) -> Result<CollectionView, AxonError> {
        let key = self.resolve_catalog_key(&view.collection)?;
        if !self.collection_exists_in_namespace(&key.collection, &key.namespace)? {
            return Err(AxonError::InvalidArgument(format!(
                "collection '{}' is not registered",
                view.collection.as_str()
            )));
        }

        let current_version: i64 = self
            .conn()?
            .query_row(
                "SELECT COALESCE(version, 0) FROM collection_views
                 WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3",
                params![
                    key.collection.as_str(),
                    key.namespace.database.as_str(),
                    key.namespace.schema.as_str()
                ],
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
        versioned.collection = key.collection.clone();
        versioned.version = next_version as u32;
        versioned.updated_at_ns = Some(updated_at_ns as u64);
        let view_json = serde_json::to_string(&versioned)?;

        self.conn()?
            .execute(
                "INSERT INTO collection_views
                    (collection, database_name, schema_name, version, view_json, updated_at_ns, updated_by)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(database_name, schema_name, collection) DO UPDATE SET
                     version = excluded.version,
                     view_json = excluded.view_json,
                     updated_at_ns = excluded.updated_at_ns,
                     updated_by = excluded.updated_by",
                params![
                    key.collection.as_str(),
                    key.namespace.database.as_str(),
                    key.namespace.schema.as_str(),
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
        let key = self.resolve_catalog_key(collection)?;
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT view_json FROM collection_views
                 WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3",
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let mut rows = stmt
            .query(params![
                key.collection.as_str(),
                key.namespace.database.as_str(),
                key.namespace.schema.as_str()
            ])
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
        let key = self.resolve_catalog_key(collection)?;
        self.conn()?
            .execute(
                "DELETE FROM collection_views
                 WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3",
                params![
                    key.collection.as_str(),
                    key.namespace.database.as_str(),
                    key.namespace.schema.as_str()
                ],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn register_collection_in_namespace(
        &mut self,
        collection: &CollectionId,
        namespace: &Namespace,
    ) -> Result<(), AxonError> {
        if !self.namespace_exists(namespace)? {
            return Err(AxonError::NotFound(format!("namespace '{namespace}'")));
        }

        self.conn()?
            .execute(
                "INSERT OR IGNORE INTO collections (name, database_name, schema_name)
                 VALUES (?1, ?2, ?3)",
                params![
                    collection.as_str(),
                    namespace.database.as_str(),
                    namespace.schema.as_str()
                ],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn unregister_collection(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        // Older SQLite databases may have `collection_views` without the
        // `ON DELETE CASCADE` foreign key. Delete the dependent row explicitly
        // so upgraded databases do not retain stale collection views.
        self.conn()?
            .execute(
                "DELETE FROM collection_views
                 WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3",
                params![
                    collection.as_str(),
                    key.namespace.database.as_str(),
                    key.namespace.schema.as_str()
                ],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.conn()?
            .execute(
                "DELETE FROM schema_versions
                 WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3",
                params![
                    collection.as_str(),
                    key.namespace.database.as_str(),
                    key.namespace.schema.as_str()
                ],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        self.conn()?
            .execute(
                "DELETE FROM collections
                 WHERE name = ?1 AND database_name = ?2 AND schema_name = ?3",
                params![
                    collection.as_str(),
                    key.namespace.database.as_str(),
                    key.namespace.schema.as_str()
                ],
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn list_collections(&self) -> Result<Vec<CollectionId>, AxonError> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT name FROM collections
                 ORDER BY database_name ASC, schema_name ASC, name ASC",
            )
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

    fn collection_registered_in_namespace(
        &self,
        collection: &CollectionId,
        namespace: &Namespace,
    ) -> Result<bool, AxonError> {
        self.collection_exists_in_namespace(collection, namespace)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon_core::types::Link;
    use serde_json::json;
    use tempfile::NamedTempFile;

    fn tasks() -> CollectionId {
        CollectionId::new("tasks")
    }

    fn entity(id: &str) -> Entity {
        Entity::new(tasks(), EntityId::new(id), json!({"title": id}))
    }

    fn store() -> SqliteStorageAdapter {
        SqliteStorageAdapter::open_in_memory().expect("test operation should succeed")
    }

    fn register_unique_namespaced_collection(
        store: &mut SqliteStorageAdapter,
        qualified: &CollectionId,
    ) -> (Namespace, CollectionId) {
        let (namespace, bare_collection) = Namespace::parse(qualified.as_str());
        let bare_collection = CollectionId::new(bare_collection);

        store
            .create_database(namespace.database.as_str())
            .expect("database create should succeed");
        store
            .create_namespace(&namespace)
            .expect("namespace create should succeed");
        store
            .register_collection_in_namespace(&bare_collection, &namespace)
            .expect("collection register should succeed");

        (namespace, bare_collection)
    }

    fn legacy_collection_views_db(collection: &CollectionId, template: &str) -> NamedTempFile {
        let file = NamedTempFile::new().expect("test temp db should be created");
        let conn = Connection::open(file.path()).expect("test temp db connection should be opened");
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
            CREATE TABLE collections (
                name TEXT NOT NULL PRIMARY KEY
            );
            CREATE TABLE collection_views (
                collection        TEXT NOT NULL PRIMARY KEY,
                version           INTEGER NOT NULL,
                view_json         TEXT NOT NULL,
                updated_at_ns     INTEGER NOT NULL,
                updated_by        TEXT
            );",
        )
        .expect("legacy schema should be created");
        let view_json = serde_json::to_string(&CollectionView::new(collection.clone(), template))
            .expect("legacy collection view should serialize");
        conn.execute(
            "INSERT INTO collections (name) VALUES (?1)",
            params![collection.as_str()],
        )
        .expect("legacy collection should be inserted");
        conn.execute(
            "INSERT INTO collection_views (collection, version, view_json, updated_at_ns, updated_by)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![collection.as_str(), 1_i64, view_json, 0_i64, Option::<&str>::None],
        )
        .expect("legacy collection view should be inserted");
        file
    }

    #[test]
    fn create_entity() {
        let mut s = store();
        s.put(entity("t-001"))
            .expect("test operation should succeed");
        assert_eq!(s.count(&tasks()).expect("test operation should succeed"), 1);
    }

    #[test]
    fn default_database_and_namespace_exist_on_open() {
        let s = store();
        assert_eq!(
            s.list_databases().expect("catalog query should succeed"),
            vec!["default".to_string()]
        );
        assert_eq!(
            s.list_namespaces("default")
                .expect("namespace query should succeed"),
            vec!["default".to_string()]
        );
    }

    #[test]
    fn database_and_namespace_catalog_persist_across_reopen() {
        let file = NamedTempFile::new().expect("test temp db should be created");
        let path = file.path().to_string_lossy().into_owned();

        {
            let mut s = SqliteStorageAdapter::open(&path).expect("initial open should succeed");
            s.create_database("prod")
                .expect("database create should succeed");
            s.create_namespace(&Namespace::new("prod", "billing"))
                .expect("namespace create should succeed");
        }

        let reopened = SqliteStorageAdapter::open(&path).expect("reopen should succeed");
        assert!(reopened
            .list_databases()
            .expect("catalog query should succeed")
            .iter()
            .any(|database| database == "prod"));
        assert!(reopened
            .list_namespaces("prod")
            .expect("namespace query should succeed")
            .iter()
            .any(|schema| schema == "billing"));
    }

    #[test]
    fn namespace_catalogs_allow_same_name_without_cross_drop() {
        let mut s = store();
        let invoices = CollectionId::new("invoices");
        let billing = Namespace::new("prod", "billing");
        let engineering = Namespace::new("prod", "engineering");

        s.create_database("prod")
            .expect("database create should succeed");
        s.create_namespace(&billing)
            .expect("billing namespace create should succeed");
        s.create_namespace(&engineering)
            .expect("engineering namespace create should succeed");

        s.register_collection_in_namespace(&invoices, &Namespace::default_ns())
            .expect("default collection register should succeed");
        s.register_collection_in_namespace(&invoices, &billing)
            .expect("billing collection register should succeed");
        s.register_collection_in_namespace(&invoices, &engineering)
            .expect("engineering collection register should succeed");

        assert_eq!(
            s.list_namespace_collections(&billing)
                .expect("billing list should succeed"),
            vec![invoices.clone()]
        );
        assert_eq!(
            s.list_namespace_collections(&engineering)
                .expect("engineering list should succeed"),
            vec![invoices.clone()]
        );

        s.drop_namespace(&billing)
            .expect("billing drop should succeed");
        assert_eq!(
            s.list_namespace_collections(&engineering)
                .expect("engineering list should survive billing drop"),
            vec![invoices.clone()]
        );
        assert_eq!(
            s.list_namespace_collections(&Namespace::default_ns())
                .expect("default list should survive billing drop"),
            vec![invoices.clone()]
        );

        s.drop_database("prod").expect("prod drop should succeed");
        assert_eq!(
            s.list_namespace_collections(&Namespace::default_ns())
                .expect("default list should survive prod drop"),
            vec![invoices]
        );
    }

    #[test]
    fn drop_namespace_purges_entities_for_removed_collections() {
        let mut s = store();
        let billing = Namespace::new("prod", "billing");
        let engineering = Namespace::new("prod", "engineering");
        let invoices = CollectionId::new("invoices");
        let ledger = CollectionId::new("ledger");

        s.create_database("prod")
            .expect("database create should succeed");
        s.create_namespace(&billing)
            .expect("billing namespace create should succeed");
        s.create_namespace(&engineering)
            .expect("engineering namespace create should succeed");
        s.register_collection_in_namespace(&invoices, &billing)
            .expect("billing collection register should succeed");
        s.register_collection_in_namespace(&ledger, &engineering)
            .expect("engineering collection register should succeed");
        s.put(Entity::new(
            invoices.clone(),
            EntityId::new("inv-001"),
            json!({"title": "invoice"}),
        ))
        .expect("billing entity put should succeed");
        s.put(Entity::new(
            ledger.clone(),
            EntityId::new("led-001"),
            json!({"title": "ledger"}),
        ))
        .expect("engineering entity put should succeed");

        s.drop_namespace(&billing)
            .expect("billing drop should succeed");

        assert!(
            s.get(&invoices, &EntityId::new("inv-001"))
                .expect("billing entity lookup should succeed")
                .is_none(),
            "dropped namespace entities must be purged"
        );
        assert!(
            s.get(&ledger, &EntityId::new("led-001"))
                .expect("surviving entity lookup should succeed")
                .is_some(),
            "entities in other namespaces must survive"
        );
    }

    #[test]
    fn drop_namespace_keeps_same_named_entities_in_surviving_namespaces() {
        let mut s = store();
        let billing = Namespace::new("prod", "billing");
        let engineering = Namespace::new("prod", "engineering");
        let invoices = CollectionId::new("invoices");
        let ledger = CollectionId::new("ledger");

        s.create_database("prod")
            .expect("database create should succeed");
        s.create_namespace(&billing)
            .expect("billing namespace create should succeed");
        s.create_namespace(&engineering)
            .expect("engineering namespace create should succeed");
        s.register_collection_in_namespace(&invoices, &Namespace::default_ns())
            .expect("default collection register should succeed");
        s.register_collection_in_namespace(&invoices, &billing)
            .expect("billing collection register should succeed");
        s.register_collection_in_namespace(&invoices, &engineering)
            .expect("engineering collection register should succeed");
        s.register_collection_in_namespace(&ledger, &billing)
            .expect("billing ledger register should succeed");
        s.put(Entity::new(
            invoices.clone(),
            EntityId::new("inv-default-001"),
            json!({"title": "default invoice"}),
        ))
        .expect("default entity put should succeed");
        s.put(Entity::new(
            ledger.clone(),
            EntityId::new("led-billing-001"),
            json!({"title": "billing ledger"}),
        ))
        .expect("billing ledger put should succeed");

        s.drop_namespace(&billing)
            .expect("billing drop should succeed");

        assert!(
            s.get(&invoices, &EntityId::new("inv-default-001"))
                .expect("default entity lookup should succeed")
                .is_some(),
            "same-named entities in surviving namespaces must be preserved"
        );
        assert!(
            s.get(&ledger, &EntityId::new("led-billing-001"))
                .expect("billing ledger lookup should succeed")
                .is_none(),
            "entities in the dropped namespace must be purged"
        );
    }

    #[test]
    fn drop_namespace_purges_links_for_removed_collections() {
        let mut s = store();
        let billing = Namespace::new("prod", "billing");
        let engineering = Namespace::new("prod", "engineering");
        let invoices = CollectionId::new("prod.billing.invoices");
        let ledger = CollectionId::new("prod.engineering.ledger");
        let keep = CollectionId::new("keep");
        let archive = CollectionId::new("archive");

        s.create_database("prod")
            .expect("database create should succeed");
        s.create_namespace(&billing)
            .expect("billing namespace create should succeed");
        s.create_namespace(&engineering)
            .expect("engineering namespace create should succeed");
        s.register_collection_in_namespace(&CollectionId::new("invoices"), &billing)
            .expect("billing collection register should succeed");
        s.register_collection_in_namespace(&CollectionId::new("ledger"), &engineering)
            .expect("engineering collection register should succeed");
        s.register_collection(&keep)
            .expect("default collection register should succeed");
        s.register_collection(&archive)
            .expect("archive collection register should succeed");
        for entity in [
            Entity::new(
                invoices.clone(),
                EntityId::new("inv-001"),
                json!({"title": "invoice"}),
            ),
            Entity::new(
                ledger.clone(),
                EntityId::new("led-001"),
                json!({"title": "ledger"}),
            ),
            Entity::new(
                keep.clone(),
                EntityId::new("keep-001"),
                json!({"title": "keep"}),
            ),
            Entity::new(
                archive.clone(),
                EntityId::new("arc-001"),
                json!({"title": "archive"}),
            ),
        ] {
            s.put(entity).expect("entity put should succeed");
        }

        for link in [
            Link {
                source_collection: invoices.clone(),
                source_id: EntityId::new("inv-001"),
                target_collection: ledger.clone(),
                target_id: EntityId::new("led-001"),
                link_type: "relates-to".into(),
                metadata: serde_json::Value::Null,
            },
            Link {
                source_collection: keep.clone(),
                source_id: EntityId::new("keep-001"),
                target_collection: invoices.clone(),
                target_id: EntityId::new("inv-001"),
                link_type: "references".into(),
                metadata: serde_json::Value::Null,
            },
            Link {
                source_collection: keep.clone(),
                source_id: EntityId::new("keep-001"),
                target_collection: archive.clone(),
                target_id: EntityId::new("arc-001"),
                link_type: "references".into(),
                metadata: serde_json::Value::Null,
            },
        ] {
            s.put_link(&link).expect("link put should succeed");
        }

        s.drop_namespace(&billing)
            .expect("billing drop should succeed");

        assert!(
            s.list_inbound_links(&ledger, &EntityId::new("led-001"), None)
                .expect("ledger inbound links should load")
                .is_empty(),
            "links from removed collections must be purged"
        );
        let keep_links = s
            .list_outbound_links(&keep, &EntityId::new("keep-001"), None)
            .expect("keep outbound links should load");
        assert_eq!(keep_links.len(), 1);
        assert_eq!(keep_links[0].target_collection, archive);
    }

    #[test]
    fn drop_database_purges_entities_for_removed_collections() {
        let mut s = store();
        let analytics = Namespace::new("prod", "analytics");
        let orders = CollectionId::new("orders");
        let rollups = CollectionId::new("rollups");
        let keep = CollectionId::new("keep");

        s.create_database("prod")
            .expect("database create should succeed");
        s.create_namespace(&analytics)
            .expect("analytics namespace create should succeed");
        s.register_collection_in_namespace(&orders, &Namespace::new("prod", "default"))
            .expect("prod default collection register should succeed");
        s.register_collection_in_namespace(&rollups, &analytics)
            .expect("analytics collection register should succeed");
        s.register_collection_in_namespace(&keep, &Namespace::default_ns())
            .expect("default collection register should succeed");
        s.put(Entity::new(
            orders.clone(),
            EntityId::new("ord-001"),
            json!({"title": "order"}),
        ))
        .expect("prod default entity put should succeed");
        s.put(Entity::new(
            rollups.clone(),
            EntityId::new("sum-001"),
            json!({"title": "rollup"}),
        ))
        .expect("analytics entity put should succeed");
        s.put(Entity::new(
            keep.clone(),
            EntityId::new("keep-001"),
            json!({"title": "keep"}),
        ))
        .expect("default entity put should succeed");

        s.drop_database("prod")
            .expect("database drop should succeed");

        assert!(
            s.get(&orders, &EntityId::new("ord-001"))
                .expect("orders lookup should succeed")
                .is_none(),
            "dropped database entities must be purged"
        );
        assert!(
            s.get(&rollups, &EntityId::new("sum-001"))
                .expect("rollups lookup should succeed")
                .is_none(),
            "all namespace entities in the dropped database must be purged"
        );
        assert!(
            s.get(&keep, &EntityId::new("keep-001"))
                .expect("default lookup should succeed")
                .is_some(),
            "entities in other databases must survive"
        );
    }

    #[test]
    fn drop_database_purges_links_for_removed_collections() {
        let mut s = store();
        let analytics = Namespace::new("prod", "analytics");
        let orders = CollectionId::new("prod.default.orders");
        let rollups = CollectionId::new("prod.analytics.rollups");
        let keep = CollectionId::new("keep");
        let archive = CollectionId::new("archive");

        s.create_database("prod")
            .expect("database create should succeed");
        s.create_namespace(&analytics)
            .expect("analytics namespace create should succeed");
        s.register_collection_in_namespace(
            &CollectionId::new("orders"),
            &Namespace::new("prod", "default"),
        )
        .expect("prod default collection register should succeed");
        s.register_collection_in_namespace(&CollectionId::new("rollups"), &analytics)
            .expect("analytics collection register should succeed");
        s.register_collection(&keep)
            .expect("default collection register should succeed");
        s.register_collection(&archive)
            .expect("archive collection register should succeed");
        for entity in [
            Entity::new(
                orders.clone(),
                EntityId::new("ord-001"),
                json!({"title": "order"}),
            ),
            Entity::new(
                rollups.clone(),
                EntityId::new("sum-001"),
                json!({"title": "rollup"}),
            ),
            Entity::new(
                keep.clone(),
                EntityId::new("keep-001"),
                json!({"title": "keep"}),
            ),
            Entity::new(
                archive.clone(),
                EntityId::new("arc-001"),
                json!({"title": "archive"}),
            ),
        ] {
            s.put(entity).expect("entity put should succeed");
        }

        for link in [
            Link {
                source_collection: keep.clone(),
                source_id: EntityId::new("keep-001"),
                target_collection: orders.clone(),
                target_id: EntityId::new("ord-001"),
                link_type: "references".into(),
                metadata: serde_json::Value::Null,
            },
            Link {
                source_collection: rollups.clone(),
                source_id: EntityId::new("sum-001"),
                target_collection: keep.clone(),
                target_id: EntityId::new("keep-001"),
                link_type: "feeds".into(),
                metadata: serde_json::Value::Null,
            },
            Link {
                source_collection: keep.clone(),
                source_id: EntityId::new("keep-001"),
                target_collection: archive.clone(),
                target_id: EntityId::new("arc-001"),
                link_type: "references".into(),
                metadata: serde_json::Value::Null,
            },
        ] {
            s.put_link(&link).expect("link put should succeed");
        }

        s.drop_database("prod")
            .expect("database drop should succeed");

        assert!(
            s.list_inbound_links(&keep, &EntityId::new("keep-001"), Some("feeds"))
                .expect("keep inbound links should load")
                .is_empty(),
            "inbound links from removed databases must be purged"
        );
        let keep_links = s
            .list_outbound_links(&keep, &EntityId::new("keep-001"), None)
            .expect("keep outbound links should load");
        assert_eq!(keep_links.len(), 1);
        assert_eq!(keep_links[0].target_collection, archive);
    }

    #[test]
    fn drop_database_keeps_same_named_entities_in_surviving_databases() {
        let mut s = store();
        let billing = Namespace::new("prod", "billing");
        let invoices = CollectionId::new("invoices");
        let orders = CollectionId::new("orders");

        s.create_database("prod")
            .expect("database create should succeed");
        s.create_namespace(&billing)
            .expect("billing namespace create should succeed");
        s.register_collection_in_namespace(&invoices, &Namespace::default_ns())
            .expect("default collection register should succeed");
        s.register_collection_in_namespace(&invoices, &billing)
            .expect("billing collection register should succeed");
        s.register_collection_in_namespace(&orders, &Namespace::new("prod", "default"))
            .expect("prod orders register should succeed");
        s.put(Entity::new(
            invoices.clone(),
            EntityId::new("inv-default-001"),
            json!({"title": "default invoice"}),
        ))
        .expect("default entity put should succeed");
        s.put(Entity::new(
            orders.clone(),
            EntityId::new("ord-prod-001"),
            json!({"title": "prod order"}),
        ))
        .expect("prod orders put should succeed");

        s.drop_database("prod").expect("prod drop should succeed");

        assert!(
            s.get(&invoices, &EntityId::new("inv-default-001"))
                .expect("default entity lookup should succeed")
                .is_some(),
            "same-named entities in surviving databases must be preserved"
        );
        assert!(
            s.get(&orders, &EntityId::new("ord-prod-001"))
                .expect("dropped database entity lookup should succeed")
                .is_none(),
            "entities in the dropped database must be purged"
        );
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
    fn opening_legacy_db_unregister_collection_removes_stale_collection_view() {
        let collection = CollectionId::new("legacy");
        let file = legacy_collection_views_db(&collection, "# {{title}}");
        let path = file.path().to_string_lossy().into_owned();

        let mut store = SqliteStorageAdapter::open(&path).expect("test operation should succeed");
        assert!(store
            .get_collection_view(&collection)
            .expect("test operation should succeed")
            .is_some());

        store
            .unregister_collection(&collection)
            .expect("test operation should succeed");

        assert!(store
            .get_collection_view(&collection)
            .expect("test operation should succeed")
            .is_none());
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
            lifecycles: Default::default(),
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
            lifecycles: Default::default(),
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
            lifecycles: Default::default(),
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
            lifecycles: Default::default(),
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

    #[test]
    fn qualified_schema_write_is_readable_via_bare_unique_collection() {
        let mut s = store();
        let qualified = CollectionId::new("prod.billing.invoices");
        let (billing, invoices) = register_unique_namespaced_collection(&mut s, &qualified);

        let v1 = CollectionSchema {
            collection: qualified.clone(),
            description: Some("v1".into()),
            version: 99,
            entity_schema: Some(
                json!({"type": "object", "properties": {"title": {"type": "string"}}}),
            ),
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            lifecycles: Default::default(),
        };
        let v2 = CollectionSchema {
            collection: qualified,
            description: Some("v2".into()),
            version: 100,
            entity_schema: Some(
                json!({"type": "object", "properties": {"amount": {"type": "number"}}}),
            ),
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            lifecycles: Default::default(),
        };

        s.put_schema(&v1).expect("schema v1 put should succeed");
        s.put_schema(&v2).expect("schema v2 put should succeed");

        let stored_collections: Vec<String> = {
            let conn = s.conn.lock().expect("lock");
            let mut stmt = conn
                .prepare(
                    "SELECT collection FROM schema_versions
                     WHERE database_name = ?1 AND schema_name = ?2
                     ORDER BY version ASC",
                )
                .expect("schema version query should prepare");
            stmt.query_map(
                params![billing.database.as_str(), billing.schema.as_str()],
                |row| row.get::<_, String>(0),
            )
            .expect("schema version query should succeed")
            .collect::<Result<Vec<_>, _>>()
            .expect("schema version rows should decode")
        };
        assert_eq!(
            stored_collections,
            vec!["invoices".to_string(), "invoices".to_string()]
        );

        let latest = s
            .get_schema(&invoices)
            .expect("latest schema lookup should succeed")
            .expect("latest schema should exist");
        assert_eq!(latest.collection, invoices);
        assert_eq!(latest.version, 2);
        assert_eq!(latest.description.as_deref(), Some("v2"));

        let version_one = s
            .get_schema_version(&invoices, 1)
            .expect("versioned schema lookup should succeed")
            .expect("schema version one should exist");
        assert_eq!(version_one.collection, invoices);
        assert_eq!(version_one.description.as_deref(), Some("v1"));

        assert_eq!(
            s.list_schema_versions(&invoices)
                .expect("schema version list should succeed")
                .into_iter()
                .map(|(version, _)| version)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[test]
    fn qualified_collection_view_write_is_readable_via_bare_unique_collection() {
        let mut s = store();
        let qualified = CollectionId::new("prod.billing.invoices");
        let (billing, invoices) = register_unique_namespaced_collection(&mut s, &qualified);

        let stored = s
            .put_collection_view(&CollectionView::new(qualified, "# {{title}}"))
            .expect("qualified collection view put should succeed");
        assert_eq!(stored.collection, invoices);
        assert_eq!(stored.version, 1);

        let stored_collection: String = s
            .conn
            .lock()
            .expect("lock")
            .query_row(
                "SELECT collection FROM collection_views
                 WHERE database_name = ?1 AND schema_name = ?2",
                params![billing.database.as_str(), billing.schema.as_str()],
                |row| row.get(0),
            )
            .expect("stored collection view lookup should succeed");
        assert_eq!(stored_collection, "invoices");

        let retrieved = s
            .get_collection_view(&invoices)
            .expect("bare collection view lookup should succeed")
            .expect("collection view should exist");
        assert_eq!(retrieved.collection, invoices);
        assert_eq!(retrieved.markdown_template, "# {{title}}");
        assert_eq!(retrieved.version, 1);
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
            .lock()
            .expect("lock")
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
            .lock()
            .expect("lock")
            .query_row("SELECT COUNT(*) FROM audit_log", [], |r| r.get(0))
            .expect("test operation should succeed");
        assert_eq!(
            count, 1,
            "audit entry must persist when transaction commits"
        );
    }

    #[test]
    fn two_part_collection_names_resolve_to_default_database_schema() {
        let mut s = store();
        let billing = Namespace::new("default", "billing");
        let invoices = CollectionId::new("invoices");
        let schema_qualified = CollectionId::new("billing.invoices");
        let fully_qualified = CollectionId::new("default.billing.invoices");
        let entity_id = EntityId::new("inv-001");

        s.create_namespace(&billing)
            .expect("billing namespace create should succeed");
        s.register_collection_in_namespace(&invoices, &billing)
            .expect("billing collection register should succeed");
        s.put(Entity::new(
            schema_qualified.clone(),
            entity_id.clone(),
            json!({"scope": "billing"}),
        ))
        .expect("two-part entity put should succeed");

        assert_eq!(
            s.get(&schema_qualified, &entity_id)
                .expect("two-part get should succeed")
                .expect("two-part entity should exist")
                .data["scope"],
            json!("billing")
        );
        assert_eq!(
            s.get(&fully_qualified, &entity_id)
                .expect("fully qualified get should succeed")
                .expect("fully qualified entity should exist")
                .data["scope"],
            json!("billing")
        );
        assert_eq!(
            s.count(&fully_qualified)
                .expect("fully qualified count should succeed"),
            1
        );
    }

    #[test]
    fn qualified_entity_identity_isolated_across_namespaces() {
        let mut s = store();
        let billing = Namespace::new("prod", "billing");
        let engineering = Namespace::new("prod", "engineering");
        let invoices = CollectionId::new("invoices");
        let billing_invoices = CollectionId::new("prod.billing.invoices");
        let engineering_invoices = CollectionId::new("prod.engineering.invoices");
        let entity_id = EntityId::new("inv-001");

        s.create_database("prod")
            .expect("database create should succeed");
        s.create_namespace(&billing)
            .expect("billing namespace create should succeed");
        s.create_namespace(&engineering)
            .expect("engineering namespace create should succeed");
        s.register_collection_in_namespace(&invoices, &billing)
            .expect("billing collection register should succeed");
        s.register_collection_in_namespace(&invoices, &engineering)
            .expect("engineering collection register should succeed");

        s.put(Entity::new(
            billing_invoices.clone(),
            entity_id.clone(),
            json!({"scope": "billing"}),
        ))
        .expect("billing entity put should succeed");
        s.put(Entity::new(
            engineering_invoices.clone(),
            entity_id.clone(),
            json!({"scope": "engineering"}),
        ))
        .expect("engineering entity put should succeed");

        assert_eq!(
            s.get(&billing_invoices, &entity_id)
                .expect("billing get should succeed")
                .expect("billing entity should exist")
                .data["scope"],
            json!("billing")
        );
        assert_eq!(
            s.get(&engineering_invoices, &entity_id)
                .expect("engineering get should succeed")
                .expect("engineering entity should exist")
                .data["scope"],
            json!("engineering")
        );
        assert_eq!(
            s.count(&billing_invoices)
                .expect("billing count should succeed"),
            1
        );
        assert_eq!(
            s.count(&engineering_invoices)
                .expect("engineering count should succeed"),
            1
        );

        let updated = s
            .compare_and_swap(
                Entity::new(
                    billing_invoices.clone(),
                    entity_id.clone(),
                    json!({"scope": "billing-updated"}),
                ),
                1,
            )
            .expect("billing compare_and_swap should succeed");
        assert_eq!(updated.version, 2);
        assert_eq!(
            s.get(&engineering_invoices, &entity_id)
                .expect("engineering get after billing update should succeed")
                .expect("engineering entity should exist")
                .version,
            1
        );
        assert_eq!(
            s.range_scan(&billing_invoices, None, None, None)
                .expect("billing range scan should succeed")
                .len(),
            1
        );
        assert_eq!(
            s.range_scan(&engineering_invoices, None, None, None)
                .expect("engineering range scan should succeed")
                .len(),
            1
        );

        s.delete(&billing_invoices, &entity_id)
            .expect("billing delete should succeed");
        assert!(
            s.get(&billing_invoices, &entity_id)
                .expect("billing get after delete should succeed")
                .is_none(),
            "billing entity should be removed"
        );
        assert!(
            s.get(&engineering_invoices, &entity_id)
                .expect("engineering get after billing delete should succeed")
                .is_some(),
            "engineering entity should survive"
        );
    }
}

// L4 conformance test suite for SqliteStorageAdapter.
crate::storage_conformance_tests!(
    sqlite_conformance,
    SqliteStorageAdapter::open_in_memory().expect("test operation should succeed")
);
