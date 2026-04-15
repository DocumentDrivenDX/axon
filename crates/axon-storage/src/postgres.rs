use std::future::Future;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio_postgres::NoTls;

use axon_audit::entry::AuditEntry;
use axon_core::error::AxonError;
use axon_core::id::{
    CollectionId, EntityId, Namespace, QualifiedCollectionId, DEFAULT_DATABASE, DEFAULT_SCHEMA,
};
use axon_core::types::Entity;
use axon_schema::schema::{CollectionSchema, CollectionView};

use crate::adapter::StorageAdapter;

/// PostgreSQL-backed storage adapter.
///
/// Uses the asynchronous `tokio-postgres` crate driven from a dedicated Tokio
/// runtime. Because `StorageAdapter` is synchronous, each database call
/// blocks the calling thread via `handle.block_on(...)`.
///
/// Transactions are handled via `BEGIN` / `COMMIT` / `ROLLBACK` statements.
/// The adapter creates the required tables on initialization if they do not
/// exist.
pub struct PostgresStorageAdapter {
    client: tokio_postgres::Client,
    handle: tokio::runtime::Handle,
    _rt: tokio::runtime::Runtime,
    in_tx: bool,
}

impl PostgresStorageAdapter {
    /// Connect to a PostgreSQL database using a connection string.
    ///
    /// Example: `"host=localhost user=axon dbname=axon"`
    pub fn connect(params: &str) -> Result<Self, AxonError> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        let handle = rt.handle().clone();
        let (client, connection) = handle
            .block_on(tokio_postgres::connect(params, NoTls))
            .map_err(|e| AxonError::Storage(format!("connection failed: {e}")))?;
        handle.spawn(async move {
            if let Err(e) = connection.await {
                tracing::error!("postgres connection error: {e}");
            }
        });
        let mut adapter = Self {
            client,
            handle,
            _rt: rt,
            in_tx: false,
        };
        adapter.init_schema()?;
        Ok(adapter)
    }

    /// Block the current thread on a `tokio_postgres` future, converting any
    /// error into `AxonError::Storage`.
    ///
    /// When called from within a Tokio async task, uses
    /// [`tokio::task::block_in_place`] to temporarily un-enter the async
    /// context before blocking on the adapter's dedicated runtime.  This
    /// prevents the "cannot call `block_on` from within an async context"
    /// panic that would otherwise occur when gateway route handlers drive
    /// storage calls.
    ///
    /// Requires the multi-thread Tokio scheduler (used by the production
    /// server).  Tests that use the default `current_thread` scheduler must
    /// call adapter methods from a `spawn_blocking` closure.
    fn block<T>(
        &self,
        fut: impl Future<Output = Result<T, tokio_postgres::Error>>,
    ) -> Result<T, AxonError> {
        match tokio::runtime::Handle::try_current() {
            Ok(_) => tokio::task::block_in_place(|| self.handle.block_on(fut)),
            Err(_) => self.handle.block_on(fut),
        }
        .map_err(|e| AxonError::Storage(e.to_string()))
    }

    fn init_schema(&mut self) -> Result<(), AxonError> {
        self.block(self.client.batch_execute(
            "CREATE TABLE IF NOT EXISTS databases (
                    name TEXT NOT NULL PRIMARY KEY
                );
                CREATE TABLE IF NOT EXISTS namespaces (
                    database_name TEXT NOT NULL REFERENCES databases(name) ON DELETE CASCADE,
                    name          TEXT NOT NULL,
                    PRIMARY KEY (database_name, name)
                );
                CREATE TABLE IF NOT EXISTS entities (
                    collection    TEXT NOT NULL,
                    database_name TEXT NOT NULL DEFAULT 'default',
                    schema_name   TEXT NOT NULL DEFAULT 'default',
                    id            TEXT NOT NULL,
                    version       BIGINT NOT NULL,
                    data          JSONB NOT NULL,
                    PRIMARY KEY (database_name, schema_name, collection, id)
                );
                CREATE TABLE IF NOT EXISTS schemas (
                    collection    TEXT NOT NULL,
                    database_name TEXT NOT NULL DEFAULT 'default',
                    schema_name   TEXT NOT NULL DEFAULT 'default',
                    version       INTEGER NOT NULL,
                    schema_json   JSONB NOT NULL,
                    created_at_ns BIGINT NOT NULL DEFAULT 0,
                    PRIMARY KEY (database_name, schema_name, collection, version)
                );
                CREATE TABLE IF NOT EXISTS collections (
                    name TEXT NOT NULL,
                    database_name TEXT NOT NULL DEFAULT 'default',
                    schema_name TEXT NOT NULL DEFAULT 'default',
                    PRIMARY KEY (database_name, schema_name, name)
                );
                CREATE TABLE IF NOT EXISTS collection_views (
                    collection    TEXT NOT NULL,
                    database_name TEXT NOT NULL DEFAULT 'default',
                    schema_name   TEXT NOT NULL DEFAULT 'default',
                    version       INTEGER NOT NULL,
                    view_json     JSONB NOT NULL,
                    updated_at_ns BIGINT NOT NULL,
                    updated_by    TEXT,
                    PRIMARY KEY (database_name, schema_name, collection),
                    FOREIGN KEY (database_name, schema_name, collection)
                        REFERENCES collections(database_name, schema_name, name)
                        ON DELETE CASCADE
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
        ))?;
        self.ensure_namespace_catalog_tables()?;
        self.ensure_default_namespace()
    }

    fn collection_exists_in_namespace(
        &self,
        collection: &CollectionId,
        namespace: &Namespace,
    ) -> Result<bool, AxonError> {
        let row = self.block(self.client.query_one(
            "SELECT EXISTS(
                    SELECT 1 FROM collections
                    WHERE name = $1 AND database_name = $2 AND schema_name = $3
                )",
            &[&collection.as_str(), &namespace.database, &namespace.schema],
        ))?;
        Ok(row.get(0))
    }

    fn database_exists(&self, database: &str) -> Result<bool, AxonError> {
        let row = self.block(self.client.query_one(
            "SELECT EXISTS(SELECT 1 FROM databases WHERE name = $1)",
            &[&database],
        ))?;
        Ok(row.get(0))
    }

    fn namespace_exists(&self, namespace: &Namespace) -> Result<bool, AxonError> {
        let row = self.block(self.client.query_one(
            "SELECT EXISTS(
                    SELECT 1 FROM namespaces
                    WHERE database_name = $1 AND name = $2
                )",
            &[&namespace.database, &namespace.schema],
        ))?;
        Ok(row.get(0))
    }

    fn table_pk_columns(&self, table: &str) -> Result<Vec<String>, AxonError> {
        let rows = self.block(self.client.query(
            "SELECT a.attname
                 FROM pg_index i
                 JOIN pg_class t ON t.oid = i.indrelid
                 JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS cols(attnum, ord) ON TRUE
                 JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = cols.attnum
                 WHERE t.relname = $1 AND i.indisprimary
                 ORDER BY cols.ord",
            &[&table],
        ))?;
        Ok(rows.iter().map(|row| row.get::<_, String>(0)).collect())
    }

    fn ensure_namespace_catalog_tables(&mut self) -> Result<(), AxonError> {
        self.block(self.client.batch_execute(
            "ALTER TABLE entities
                     ADD COLUMN IF NOT EXISTS database_name TEXT NOT NULL DEFAULT 'default';
                 ALTER TABLE entities
                     ADD COLUMN IF NOT EXISTS schema_name TEXT NOT NULL DEFAULT 'default';
                 ALTER TABLE collections
                     ADD COLUMN IF NOT EXISTS database_name TEXT NOT NULL DEFAULT 'default';
                 ALTER TABLE collections
                     ADD COLUMN IF NOT EXISTS schema_name TEXT NOT NULL DEFAULT 'default';
                 ALTER TABLE schemas
                     ADD COLUMN IF NOT EXISTS database_name TEXT NOT NULL DEFAULT 'default';
                 ALTER TABLE schemas
                     ADD COLUMN IF NOT EXISTS schema_name TEXT NOT NULL DEFAULT 'default';
                 ALTER TABLE schemas
                     ADD COLUMN IF NOT EXISTS created_at_ns BIGINT NOT NULL DEFAULT 0;
                 ALTER TABLE collection_views
                     ADD COLUMN IF NOT EXISTS database_name TEXT NOT NULL DEFAULT 'default';
                 ALTER TABLE collection_views
                     ADD COLUMN IF NOT EXISTS schema_name TEXT NOT NULL DEFAULT 'default';",
        ))?;

        if self.table_pk_columns("entities")?
            != vec!["database_name", "schema_name", "collection", "id"]
        {
            self.block(self.client.batch_execute(
                "ALTER TABLE entities DROP CONSTRAINT IF EXISTS entities_pkey;
                     ALTER TABLE entities ADD PRIMARY KEY (database_name, schema_name, collection, id);",
            ))?;
        }

        self.block(self.client.execute(
            "UPDATE schemas s
                 SET database_name = c.database_name,
                     schema_name = c.schema_name
                 FROM collections c
                 WHERE s.collection = c.name
                   AND (s.database_name = 'default' OR s.schema_name = 'default')",
            &[],
        ))?;
        self.block(self.client.execute(
            "UPDATE collection_views v
                 SET database_name = c.database_name,
                     schema_name = c.schema_name
                 FROM collections c
                 WHERE v.collection = c.name
                   AND (v.database_name = 'default' OR v.schema_name = 'default')",
            &[],
        ))?;

        if self.table_pk_columns("collections")? != vec!["database_name", "schema_name", "name"] {
            self.block(self.client.batch_execute(
                "ALTER TABLE collection_views DROP CONSTRAINT IF EXISTS collection_views_collection_fkey;
                     ALTER TABLE collection_views DROP CONSTRAINT IF EXISTS collection_views_pkey;
                     ALTER TABLE collections DROP CONSTRAINT IF EXISTS collections_pkey;
                     ALTER TABLE collections ADD PRIMARY KEY (database_name, schema_name, name);",
            ))?;
        }

        if self.table_pk_columns("schemas")?
            != vec!["database_name", "schema_name", "collection", "version"]
        {
            self.block(self.client.batch_execute(
                "ALTER TABLE schemas DROP CONSTRAINT IF EXISTS schemas_pkey;
                     ALTER TABLE schemas
                         ADD PRIMARY KEY (database_name, schema_name, collection, version);",
            ))?;
        }

        if self.table_pk_columns("collection_views")?
            != vec!["database_name", "schema_name", "collection"]
        {
            self.block(self.client.batch_execute(
                "ALTER TABLE collection_views DROP CONSTRAINT IF EXISTS collection_views_pkey;
                     ALTER TABLE collection_views ADD PRIMARY KEY (database_name, schema_name, collection);",
            ))?;
        }

        self.block(self.client.batch_execute(
            "ALTER TABLE collection_views DROP CONSTRAINT IF EXISTS collection_views_collection_fkey;
                 ALTER TABLE collection_views
                     ADD CONSTRAINT collection_views_collection_fkey
                     FOREIGN KEY (database_name, schema_name, collection)
                     REFERENCES collections(database_name, schema_name, name)
                     ON DELETE CASCADE;
                 CREATE INDEX IF NOT EXISTS idx_collections_namespace
                     ON collections (database_name, schema_name, name);",
        ))?;
        Ok(())
    }

    fn registered_collection_namespaces(
        &self,
        collection: &CollectionId,
    ) -> Result<Vec<Namespace>, AxonError> {
        let rows = self.block(self.client.query(
            "SELECT database_name, schema_name FROM collections
                 WHERE name = $1
                 ORDER BY CASE
                     WHEN database_name = 'default' AND schema_name = 'default' THEN 0
                     ELSE 1
                 END,
                 database_name,
                 schema_name",
            &[&collection.as_str()],
        ))?;
        Ok(rows
            .iter()
            .map(|row| Namespace::new(row.get::<_, String>(0), row.get::<_, String>(1)))
            .collect())
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
        let rows = self.block(self.client.query(
            "SELECT name FROM collections
                 WHERE database_name = $1 AND schema_name = $2
                 ORDER BY name ASC",
            &[&namespace.database, &namespace.schema],
        ))?;
        Ok(rows
            .iter()
            .map(|row| {
                QualifiedCollectionId::from_parts(
                    namespace,
                    &CollectionId::new(row.get::<_, String>("name")),
                )
            })
            .collect())
    }

    fn database_collection_keys(
        &self,
        database: &str,
    ) -> Result<Vec<QualifiedCollectionId>, AxonError> {
        let rows = self.block(self.client.query(
            "SELECT schema_name, name FROM collections
                 WHERE database_name = $1
                 ORDER BY schema_name ASC, name ASC",
            &[&database],
        ))?;
        Ok(rows
            .iter()
            .map(|row| {
                QualifiedCollectionId::from_parts(
                    &Namespace::new(database, row.get::<_, String>("schema_name")),
                    &CollectionId::new(row.get::<_, String>("name")),
                )
            })
            .collect())
    }

    fn ensure_default_namespace(&self) -> Result<(), AxonError> {
        self.block(self.client.execute(
            "INSERT INTO databases (name) VALUES ($1) ON CONFLICT DO NOTHING",
            &[&DEFAULT_DATABASE],
        ))?;
        self.block(self.client.execute(
            "INSERT INTO namespaces (database_name, name)
                 VALUES ($1, $2)
                 ON CONFLICT DO NOTHING",
            &[&DEFAULT_DATABASE, &DEFAULT_SCHEMA],
        ))?;
        Ok(())
    }

    fn row_to_entity(row: &tokio_postgres::Row) -> Result<Entity, AxonError> {
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
            schema_version: None,
            gate_results: Default::default(),
        })
    }
}

impl StorageAdapter for PostgresStorageAdapter {
    fn resolve_collection_key(
        &self,
        collection: &CollectionId,
    ) -> Result<QualifiedCollectionId, AxonError> {
        self.resolve_catalog_key(collection)
    }

    fn get(&self, collection: &CollectionId, id: &EntityId) -> Result<Option<Entity>, AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        let rows = self.block(self.client.query(
            "SELECT collection, id, version, data
                 FROM entities
                 WHERE collection = $1 AND database_name = $2 AND schema_name = $3 AND id = $4",
            &[
                &key.collection.as_str(),
                &key.namespace.database,
                &key.namespace.schema,
                &id.as_str(),
            ],
        ))?;

        match rows.first() {
            Some(row) => Ok(Some(Self::row_to_entity(row)?)),
            None => Ok(None),
        }
    }

    fn put(&mut self, entity: Entity) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(&entity.collection)?;
        let data_json = serde_json::to_value(&entity.data)?;
        self.block(self.client.execute(
            "INSERT INTO entities (collection, database_name, schema_name, id, version, data)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (database_name, schema_name, collection, id)
                 DO UPDATE SET version = $5, data = $6",
            &[
                &key.collection.as_str(),
                &key.namespace.database,
                &key.namespace.schema,
                &entity.id.as_str(),
                &(entity.version as i64),
                &data_json,
            ],
        ))?;
        Ok(())
    }

    fn delete(&mut self, collection: &CollectionId, id: &EntityId) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        self.block(self.client.execute(
            "DELETE FROM entities
                 WHERE collection = $1 AND database_name = $2 AND schema_name = $3 AND id = $4",
            &[
                &key.collection.as_str(),
                &key.namespace.database,
                &key.namespace.schema,
                &id.as_str(),
            ],
        ))?;
        Ok(())
    }

    fn count(&self, collection: &CollectionId) -> Result<usize, AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        let row = self.block(self.client.query_one(
            "SELECT COUNT(*) FROM entities
                 WHERE collection = $1 AND database_name = $2 AND schema_name = $3",
            &[
                &key.collection.as_str(),
                &key.namespace.database,
                &key.namespace.schema,
            ],
        ))?;
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
        let key = self.resolve_catalog_key(collection)?;
        let start_str = start.map(|s| s.as_str().to_string());
        let end_str = end.map(|e| e.as_str().to_string());
        let limit_val = limit.map(|l| l as i64);

        let rows = self.block(self.client.query(
            "SELECT collection, id, version, data FROM entities
                 WHERE collection = $1
                   AND database_name = $2
                   AND schema_name = $3
                   AND ($4::text IS NULL OR id >= $4)
                   AND ($5::text IS NULL OR id <= $5)
                 ORDER BY id ASC
                 LIMIT $6",
            &[
                &key.collection.as_str(),
                &key.namespace.database,
                &key.namespace.schema,
                &start_str.as_deref(),
                &end_str.as_deref(),
                &limit_val,
            ],
        ))?;

        rows.iter().map(Self::row_to_entity).collect()
    }

    fn compare_and_swap(
        &mut self,
        entity: Entity,
        expected_version: u64,
    ) -> Result<Entity, AxonError> {
        let key = self.resolve_catalog_key(&entity.collection)?;
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

        let changed = self.block(self.client.execute(
            "UPDATE entities SET version = $3, data = $4
                 WHERE collection = $1 AND database_name = $5 AND schema_name = $6 AND id = $2 AND version = $7",
            &[
                &key.collection.as_str(),
                &entity.id.as_str(),
                &(new_version as i64),
                &data_json,
                &key.namespace.database,
                &key.namespace.schema,
                &(expected_version as i64),
            ],
        ))?;

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
        let data_json = serde_json::to_value(&entity.data)?;
        let changed = self.block(self.client.execute(
            "INSERT INTO entities (collection, database_name, schema_name, id, version, data)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (database_name, schema_name, collection, id) DO NOTHING",
            &[
                &key.collection.as_str(),
                &key.namespace.database,
                &key.namespace.schema,
                &entity.id.as_str(),
                &(entity.version as i64),
                &data_json,
            ],
        ))?;

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
        self.block(self.client.batch_execute("BEGIN"))?;
        self.in_tx = true;
        Ok(())
    }

    fn commit_tx(&mut self) -> Result<(), AxonError> {
        if !self.in_tx {
            return Err(AxonError::Storage("no active transaction".into()));
        }
        self.block(self.client.batch_execute("COMMIT"))?;
        self.in_tx = false;
        Ok(())
    }

    fn abort_tx(&mut self) -> Result<(), AxonError> {
        if !self.in_tx {
            return Ok(());
        }
        self.block(self.client.batch_execute("ROLLBACK"))?;
        self.in_tx = false;
        Ok(())
    }

    fn create_database(&mut self, name: &str) -> Result<(), AxonError> {
        if self.database_exists(name)? {
            return Err(AxonError::AlreadyExists(format!("database '{name}'")));
        }

        self.block(
            self.client
                .execute("INSERT INTO databases (name) VALUES ($1)", &[&name]),
        )?;
        self.block(self.client.execute(
            "INSERT INTO namespaces (database_name, name) VALUES ($1, $2)",
            &[&name, &DEFAULT_SCHEMA],
        ))?;
        Ok(())
    }

    fn list_databases(&self) -> Result<Vec<String>, AxonError> {
        let rows = self.block(
            self.client
                .query("SELECT name FROM databases ORDER BY name ASC", &[]),
        )?;
        Ok(rows.iter().map(|row| row.get("name")).collect())
    }

    fn drop_database(&mut self, name: &str) -> Result<(), AxonError> {
        if !self.database_exists(name)? {
            return Err(AxonError::NotFound(format!("database '{name}'")));
        }

        let doomed = self.database_collection_keys(name)?;
        self.purge_links_for_collections(&doomed)?;
        self.block(
            self.client
                .execute("DELETE FROM entities WHERE database_name = $1", &[&name]),
        )?;
        self.block(self.client.execute(
            "DELETE FROM collection_views WHERE database_name = $1",
            &[&name],
        ))?;
        self.block(
            self.client
                .execute("DELETE FROM schemas WHERE database_name = $1", &[&name]),
        )?;
        self.block(
            self.client
                .execute("DELETE FROM collections WHERE database_name = $1", &[&name]),
        )?;
        self.block(
            self.client
                .execute("DELETE FROM databases WHERE name = $1", &[&name]),
        )?;
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

        self.block(self.client.execute(
            "INSERT INTO namespaces (database_name, name) VALUES ($1, $2)",
            &[&namespace.database, &namespace.schema],
        ))?;
        Ok(())
    }

    fn list_namespaces(&self, database: &str) -> Result<Vec<String>, AxonError> {
        if !self.database_exists(database)? {
            return Err(AxonError::NotFound(format!("database '{database}'")));
        }

        let rows = self.block(self.client.query(
            "SELECT name FROM namespaces
                 WHERE database_name = $1
                 ORDER BY name ASC",
            &[&database],
        ))?;
        Ok(rows.iter().map(|row| row.get("name")).collect())
    }

    fn drop_namespace(&mut self, namespace: &Namespace) -> Result<(), AxonError> {
        if !self.namespace_exists(namespace)? {
            return Err(AxonError::NotFound(format!("namespace '{namespace}'")));
        }

        let doomed = self.namespace_collection_keys(namespace)?;
        self.purge_links_for_collections(&doomed)?;
        self.block(self.client.execute(
            "DELETE FROM entities
                 WHERE database_name = $1 AND schema_name = $2",
            &[&namespace.database, &namespace.schema],
        ))?;
        self.block(self.client.execute(
            "DELETE FROM collection_views
                 WHERE database_name = $1 AND schema_name = $2",
            &[&namespace.database, &namespace.schema],
        ))?;
        self.block(self.client.execute(
            "DELETE FROM schemas
                 WHERE database_name = $1 AND schema_name = $2",
            &[&namespace.database, &namespace.schema],
        ))?;
        self.block(self.client.execute(
            "DELETE FROM collections
                 WHERE database_name = $1 AND schema_name = $2",
            &[&namespace.database, &namespace.schema],
        ))?;
        self.block(self.client.execute(
            "DELETE FROM namespaces
                 WHERE database_name = $1 AND name = $2",
            &[&namespace.database, &namespace.schema],
        ))?;
        Ok(())
    }

    fn list_namespace_collections(
        &self,
        namespace: &Namespace,
    ) -> Result<Vec<CollectionId>, AxonError> {
        if !self.namespace_exists(namespace)? {
            return Err(AxonError::NotFound(format!("namespace '{namespace}'")));
        }

        let rows = self.block(self.client.query(
            "SELECT name FROM collections
                 WHERE database_name = $1 AND schema_name = $2
                 ORDER BY name ASC",
            &[&namespace.database, &namespace.schema],
        ))?;
        Ok(rows
            .iter()
            .map(|row| {
                let name: String = row.get("name");
                CollectionId::new(name)
            })
            .collect())
    }

    fn append_audit_entry(&mut self, mut entry: AuditEntry) -> Result<AuditEntry, AxonError> {
        if entry.timestamp_ns == 0 {
            entry.timestamp_ns = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0);
        }

        let entry_json = serde_json::to_value(&entry)?;

        let row = self.block(self.client.query_one(
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
        ))?;

        let id: i64 = row.get(0);
        entry.id = id as u64;

        Ok(entry)
    }

    fn put_schema(&mut self, schema: &CollectionSchema) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(&schema.collection)?;
        let row = self.block(self.client.query_one(
            "SELECT COALESCE(MAX(version), 0) FROM schemas
                 WHERE collection = $1 AND database_name = $2 AND schema_name = $3",
            &[
                &key.collection.as_str(),
                &key.namespace.database,
                &key.namespace.schema,
            ],
        ))?;
        let next_version = row.get::<_, i32>(0) + 1;
        let created_at_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as i64;

        let mut versioned = schema.clone();
        versioned.collection = key.collection.clone();
        versioned.version = next_version as u32;
        let schema_json = serde_json::to_value(&versioned)?;
        self.block(self.client.execute(
            "INSERT INTO schemas
                    (collection, database_name, schema_name, version, schema_json, created_at_ns)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            &[
                &key.collection.as_str(),
                &key.namespace.database,
                &key.namespace.schema,
                &next_version,
                &schema_json,
                &created_at_ns,
            ],
        ))?;
        Ok(())
    }

    fn get_schema(&self, collection: &CollectionId) -> Result<Option<CollectionSchema>, AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        let rows = self.block(self.client.query(
            "SELECT schema_json FROM schemas
                 WHERE collection = $1 AND database_name = $2 AND schema_name = $3
                 ORDER BY version DESC
                 LIMIT 1",
            &[
                &key.collection.as_str(),
                &key.namespace.database,
                &key.namespace.schema,
            ],
        ))?;

        match rows.first() {
            Some(row) => {
                let schema_json: serde_json::Value = row.get("schema_json");
                let schema: CollectionSchema = serde_json::from_value(schema_json)?;
                Ok(Some(schema))
            }
            None => Ok(None),
        }
    }

    fn get_schema_version(
        &self,
        collection: &CollectionId,
        version: u32,
    ) -> Result<Option<CollectionSchema>, AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        let rows = self.block(self.client.query(
            "SELECT schema_json FROM schemas
                 WHERE collection = $1 AND database_name = $2 AND schema_name = $3 AND version = $4",
            &[
                &key.collection.as_str(),
                &key.namespace.database,
                &key.namespace.schema,
                &(version as i32),
            ],
        ))?;

        match rows.first() {
            Some(row) => {
                let schema_json: serde_json::Value = row.get("schema_json");
                let schema: CollectionSchema = serde_json::from_value(schema_json)?;
                Ok(Some(schema))
            }
            None => Ok(None),
        }
    }

    fn list_schema_versions(
        &self,
        collection: &CollectionId,
    ) -> Result<Vec<(u32, u64)>, AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        let rows = self.block(self.client.query(
            "SELECT version, created_at_ns FROM schemas
                 WHERE collection = $1 AND database_name = $2 AND schema_name = $3
                 ORDER BY version ASC",
            &[
                &key.collection.as_str(),
                &key.namespace.database,
                &key.namespace.schema,
            ],
        ))?;

        Ok(rows
            .into_iter()
            .map(|row| {
                (
                    row.get::<_, i32>("version") as u32,
                    row.get::<_, i64>("created_at_ns") as u64,
                )
            })
            .collect())
    }

    fn delete_schema(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        self.block(self.client.execute(
            "DELETE FROM schemas
                 WHERE collection = $1 AND database_name = $2 AND schema_name = $3",
            &[
                &key.collection.as_str(),
                &key.namespace.database,
                &key.namespace.schema,
            ],
        ))?;
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

        let current_version = self
            .block(self.client.query_opt(
                "SELECT version FROM collection_views
                 WHERE collection = $1 AND database_name = $2 AND schema_name = $3",
                &[
                    &key.collection.as_str(),
                    &key.namespace.database,
                    &key.namespace.schema,
                ],
            ))?
            .map_or(0, |row| row.get::<_, i32>("version"));
        let next_version = current_version + 1;

        let updated_at_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as i64;

        let mut versioned = view.clone();
        versioned.collection = key.collection.clone();
        versioned.version = next_version as u32;
        versioned.updated_at_ns = Some(updated_at_ns as u64);
        let view_json = serde_json::to_value(&versioned)?;

        self.block(self.client.execute(
            "INSERT INTO collection_views
                    (collection, database_name, schema_name, version, view_json, updated_at_ns, updated_by)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)
                 ON CONFLICT (database_name, schema_name, collection) DO UPDATE SET
                     version = EXCLUDED.version,
                     view_json = EXCLUDED.view_json,
                     updated_at_ns = EXCLUDED.updated_at_ns,
                     updated_by = EXCLUDED.updated_by",
            &[
                &key.collection.as_str(),
                &key.namespace.database,
                &key.namespace.schema,
                &next_version,
                &view_json,
                &updated_at_ns,
                &versioned.updated_by,
            ],
        ))?;
        Ok(versioned)
    }

    fn get_collection_view(
        &self,
        collection: &CollectionId,
    ) -> Result<Option<CollectionView>, AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        let rows = self.block(self.client.query(
            "SELECT view_json FROM collection_views
                 WHERE collection = $1 AND database_name = $2 AND schema_name = $3",
            &[
                &key.collection.as_str(),
                &key.namespace.database,
                &key.namespace.schema,
            ],
        ))?;

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
        let key = self.resolve_catalog_key(collection)?;
        self.block(self.client.execute(
            "DELETE FROM collection_views
                 WHERE collection = $1 AND database_name = $2 AND schema_name = $3",
            &[
                &key.collection.as_str(),
                &key.namespace.database,
                &key.namespace.schema,
            ],
        ))?;
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

        self.block(self.client.execute(
            "INSERT INTO collections (name, database_name, schema_name)
                 VALUES ($1, $2, $3)
                 ON CONFLICT DO NOTHING",
            &[&collection.as_str(), &namespace.database, &namespace.schema],
        ))?;
        Ok(())
    }

    fn unregister_collection(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        let raw_collection = collection.as_str();
        let default_namespace = Namespace::default_ns();
        // Upgraded databases may still have a pre-fix collection_views table
        // without the collection -> collections foreign key, and may still
        // carry metadata rows keyed by the original qualified identifier in
        // either the resolved namespace or the old default/default namespace,
        // so clean them up explicitly instead of relying solely on ON DELETE
        // CASCADE.
        self.block(self.client.execute(
            "DELETE FROM collection_views
                 WHERE collection = $1 AND database_name = $2 AND schema_name = $3",
            &[
                &key.collection.as_str(),
                &key.namespace.database,
                &key.namespace.schema,
            ],
        ))?;
        if raw_collection != key.collection.as_str() {
            self.block(self.client.execute(
                "DELETE FROM collection_views
                     WHERE collection = $1
                       AND ((database_name = $2 AND schema_name = $3)
                            OR (database_name = $4 AND schema_name = $5))",
                &[
                    &raw_collection,
                    &key.namespace.database,
                    &key.namespace.schema,
                    &default_namespace.database,
                    &default_namespace.schema,
                ],
            ))?;
        }
        self.block(self.client.execute(
            "DELETE FROM schemas
                 WHERE collection = $1 AND database_name = $2 AND schema_name = $3",
            &[
                &key.collection.as_str(),
                &key.namespace.database,
                &key.namespace.schema,
            ],
        ))?;
        if raw_collection != key.collection.as_str() {
            self.block(self.client.execute(
                "DELETE FROM schemas
                     WHERE collection = $1
                       AND ((database_name = $2 AND schema_name = $3)
                            OR (database_name = $4 AND schema_name = $5))",
                &[
                    &raw_collection,
                    &key.namespace.database,
                    &key.namespace.schema,
                    &default_namespace.database,
                    &default_namespace.schema,
                ],
            ))?;
        }
        self.block(self.client.execute(
            "DELETE FROM collections
                 WHERE name = $1 AND database_name = $2 AND schema_name = $3",
            &[
                &key.collection.as_str(),
                &key.namespace.database,
                &key.namespace.schema,
            ],
        ))?;
        if raw_collection != key.collection.as_str() {
            self.block(self.client.execute(
                "DELETE FROM collections
                     WHERE name = $1
                       AND ((database_name = $2 AND schema_name = $3)
                            OR (database_name = $4 AND schema_name = $5))",
                &[
                    &raw_collection,
                    &key.namespace.database,
                    &key.namespace.schema,
                    &default_namespace.database,
                    &default_namespace.schema,
                ],
            ))?;
        }
        Ok(())
    }

    fn list_collections(&self) -> Result<Vec<CollectionId>, AxonError> {
        let rows = self.block(self.client.query(
            "SELECT name FROM collections
                 ORDER BY database_name ASC, schema_name ASC, name ASC",
            &[],
        ))?;

        Ok(rows
            .iter()
            .map(|row| {
                let name: String = row.get("name");
                CollectionId::new(name)
            })
            .collect())
    }

    fn collection_registered_in_namespace(
        &self,
        collection: &CollectionId,
        namespace: &Namespace,
    ) -> Result<bool, AxonError> {
        self.collection_exists_in_namespace(collection, namespace)
    }
}

// ── Per-tenant PostgreSQL provisioning ───────────────────────────────────────

/// Validate a PostgreSQL database name to prevent SQL injection in DDL.
///
/// PostgreSQL identifiers may contain letters, digits, `_`, and `$`, and
/// must start with a letter or `_`. We apply a conservative subset of that
/// rule here to keep things simple and safe.
fn validate_pg_db_name(name: &str) -> Result<(), AxonError> {
    if name.is_empty() {
        return Err(AxonError::InvalidArgument(
            "database name must not be empty".to_owned(),
        ));
    }
    let first = name.chars().next().expect("non-empty checked above");
    if !first.is_ascii_alphabetic() && first != '_' {
        return Err(AxonError::InvalidArgument(format!(
            "database name '{name}' must start with a letter or underscore"
        )));
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(AxonError::InvalidArgument(format!(
            "database name '{name}' contains invalid characters (only ASCII alphanumeric and _ allowed)"
        )));
    }
    Ok(())
}

/// Derive a per-tenant DSN from a superadmin DSN by replacing/adding the
/// `dbname` key.
///
/// The superadmin DSN uses the `postgres` maintenance database.  We produce a
/// new DSN that targets `axon_{name}`.
///
/// Both libpq keyword–value format (`host=... dbname=...`) and URL format
/// (`postgres://...`) are supported.
pub fn tenant_dsn(superadmin_dsn: &str, tenant_db_name: &str) -> String {
    let target = format!("axon_{tenant_db_name}");
    if superadmin_dsn.starts_with("postgres://") || superadmin_dsn.starts_with("postgresql://") {
        // URL format: replace path component.
        // e.g. postgres://user:pass@host/somedb?sslmode=disable
        if let Some(pos) = superadmin_dsn.find("://") {
            let after_scheme = &superadmin_dsn[pos + 3..];
            // Find the slash that separates authority from path.
            if let Some(slash_pos) = after_scheme.find('/') {
                let scheme_authority = &superadmin_dsn[..pos + 3 + slash_pos];
                let rest = &after_scheme[slash_pos + 1..];
                // Strip existing dbname (everything before '?').
                let query = if let Some(q) = rest.find('?') {
                    &rest[q..]
                } else {
                    ""
                };
                return format!("{scheme_authority}/{target}{query}");
            }
            // No path: append the target database.
            return format!("{superadmin_dsn}/{target}");
        }
        format!("{superadmin_dsn}/{target}")
    } else {
        // Keyword–value format.  Replace existing dbname= or append it.
        let mut parts: Vec<String> = superadmin_dsn
            .split_whitespace()
            .filter(|part| !part.starts_with("dbname="))
            .map(str::to_owned)
            .collect();
        parts.push(format!("dbname={target}"));
        parts.join(" ")
    }
}

/// Create a physical PostgreSQL database named `axon_{name}` using a
/// superadmin connection.
///
/// The `superadmin_dsn` must connect to the `postgres` maintenance database
/// (or any database the superuser can reach) with sufficient privileges to
/// execute `CREATE DATABASE`.
///
/// The database name is validated to prevent SQL injection.
///
/// # Errors
///
/// Returns `AxonError::AlreadyExists` if the database already exists.
/// Returns `AxonError::InvalidArgument` if `name` contains invalid characters.
/// Returns `AxonError::Storage` for other PostgreSQL errors.
pub fn provision_postgres_database(superadmin_dsn: &str, name: &str) -> Result<(), AxonError> {
    validate_pg_db_name(name)?;
    let full_name = format!("axon_{name}");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| AxonError::Storage(e.to_string()))?;
    rt.block_on(async {
        let (client, connection) = tokio_postgres::connect(superadmin_dsn, NoTls)
            .await
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        let row = client
            .query_one(
                "SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1)",
                &[&full_name.as_str()],
            )
            .await
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        let exists: bool = row.get(0);
        if exists {
            return Err(AxonError::AlreadyExists(format!(
                "PostgreSQL database '{full_name}'"
            )));
        }
        // CREATE DATABASE cannot run inside a transaction; use simple_query to
        // avoid the implicit transaction that execute() would open.
        client
            .simple_query(&format!("CREATE DATABASE \"{full_name}\""))
            .await
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    })
}

/// Drop the physical PostgreSQL database named `axon_{name}`.
///
/// The `superadmin_dsn` must have sufficient privileges to execute
/// `DROP DATABASE`.
///
/// # Errors
///
/// Returns `AxonError::NotFound` if the database does not exist.
/// Returns `AxonError::InvalidArgument` if `name` contains invalid characters.
/// Returns `AxonError::Storage` for other PostgreSQL errors.
pub fn deprovision_postgres_database(superadmin_dsn: &str, name: &str) -> Result<(), AxonError> {
    validate_pg_db_name(name)?;
    let full_name = format!("axon_{name}");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| AxonError::Storage(e.to_string()))?;
    rt.block_on(async {
        let (client, connection) = tokio_postgres::connect(superadmin_dsn, NoTls)
            .await
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        let row = client
            .query_one(
                "SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1)",
                &[&full_name.as_str()],
            )
            .await
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        let exists: bool = row.get(0);
        if !exists {
            return Err(AxonError::NotFound(format!(
                "PostgreSQL database '{full_name}'"
            )));
        }
        client
            .simple_query(&format!("DROP DATABASE \"{full_name}\""))
            .await
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    })
}

// The conformance test suite requires PostgreSQL.
// Explicit verification path:
// AXON_TEST_POSTGRES="host=localhost user=axon dbname=axon_test" cargo test -p axon-storage postgres::tests:: -- --nocapture
// When `AXON_TEST_POSTGRES` is unset, these tests attempt to provision PostgreSQL
// via testcontainers and skip cleanly if the container runtime is unavailable.
#[cfg(test)]
mod tests {
    use std::{
        ops::{Deref, DerefMut},
        sync::{Mutex, MutexGuard, OnceLock},
    };

    use super::*;
    use axon_core::types::Link;
    use testcontainers_modules::postgres::Postgres as PgContainer;
    use testcontainers_modules::testcontainers::{runners::SyncRunner, Container};

    struct TestDatabase {
        url: String,
        _container: Option<Container<PgContainer>>,
    }

    enum TestSetupError {
        Skip(String),
        Fail(AxonError),
    }

    impl TestDatabase {
        fn connect() -> Result<Self, TestSetupError> {
            if let Ok(url) = std::env::var("AXON_TEST_POSTGRES") {
                return Ok(Self {
                    url,
                    _container: None,
                });
            }

            let container = PgContainer::default()
                .with_db_name("axon_test")
                .with_user("postgres")
                .with_password("postgres")
                .start()
                .map_err(|error| {
                    TestSetupError::Skip(format!(
                        "AXON_TEST_POSTGRES is unset and PostgreSQL test container startup failed: {error}"
                    ))
                })?;
            let host = container.get_host().map_err(|error| {
                TestSetupError::Fail(AxonError::Storage(format!(
                    "failed to resolve PostgreSQL test container host: {error}"
                )))
            })?;
            let port = container.get_host_port_ipv4(5432).map_err(|error| {
                TestSetupError::Fail(AxonError::Storage(format!(
                    "failed to resolve PostgreSQL test container port: {error}"
                )))
            })?;

            Ok(Self {
                url: format!(
                    "host={host} port={port} user=postgres password=postgres dbname=axon_test"
                ),
                _container: Some(container),
            })
        }

        fn url(&self) -> &str {
            &self.url
        }
    }

    struct TestStore {
        adapter: PostgresStorageAdapter,
        _database: TestDatabase,
    }

    impl Deref for TestStore {
        type Target = PostgresStorageAdapter;

        fn deref(&self) -> &Self::Target {
            &self.adapter
        }
    }

    impl DerefMut for TestStore {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.adapter
        }
    }

    fn postgres_test_guard() -> MutexGuard<'static, ()> {
        static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
        GUARD
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("PostgreSQL test guard lock should not be poisoned")
    }

    fn skip_postgres_test(test_name: &str, reason: &str) {
        tracing::warn!(test = test_name, reason, "skipping PostgreSQL storage test");
    }

    fn database_or_skip(test_name: &str) -> Option<TestDatabase> {
        match TestDatabase::connect() {
            Ok(database) => Some(database),
            Err(TestSetupError::Skip(reason)) => {
                skip_postgres_test(test_name, &reason);
                None
            }
            Err(TestSetupError::Fail(error)) => {
                panic!("PostgreSQL test setup should succeed: {error}");
            }
        }
    }

    fn store() -> Result<TestStore, TestSetupError> {
        let database = TestDatabase::connect()?;
        let adapter =
            PostgresStorageAdapter::connect(database.url()).map_err(TestSetupError::Fail)?;
        // Clean tables for a fresh test.
        adapter
            .block(adapter.client.batch_execute(
                "TRUNCATE entities, schemas, collection_views, collections, namespaces, databases, audit_log RESTART IDENTITY CASCADE",
            ))
            .map_err(TestSetupError::Fail)?;
        adapter
            .ensure_default_namespace()
            .map_err(TestSetupError::Fail)?;
        Ok(TestStore {
            adapter,
            _database: database,
        })
    }

    fn store_or_skip(test_name: &str) -> Option<TestStore> {
        match store() {
            Ok(store) => Some(store),
            Err(TestSetupError::Skip(reason)) => {
                skip_postgres_test(test_name, &reason);
                None
            }
            Err(TestSetupError::Fail(error)) => {
                panic!("PostgreSQL test setup should succeed: {error}");
            }
        }
    }

    fn register_unique_namespaced_collection(
        store: &mut TestStore,
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

    #[cfg(test)]
    fn connect_raw_for_test(url: &str) -> (tokio_postgres::Client, tokio::runtime::Runtime) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        let (client, connection) = rt
            .block_on(tokio_postgres::connect(url, NoTls))
            .expect("test connection");
        rt.spawn(async move {
            let _ = connection.await;
        });
        (client, rt)
    }

    #[test]
    fn postgres_roundtrip_when_available() {
        let _guard = postgres_test_guard();
        let Some(mut s) = store_or_skip("postgres_roundtrip_when_available") else {
            return;
        };

        let col = CollectionId::new("tasks");
        let entity = Entity::new(
            col.clone(),
            EntityId::new("t-001"),
            serde_json::json!({"title": "hello"}),
        );
        s.put(entity).expect("test operation should succeed");
        let got = s
            .get(&col, &EntityId::new("t-001"))
            .expect("test operation should succeed")
            .expect("test operation should succeed");
        assert_eq!(got.data["title"], "hello");
        assert_eq!(got.version, 1);
    }

    #[test]
    fn unregister_collection_cleans_up_legacy_collection_views_when_available() {
        let _guard = postgres_test_guard();
        let Some(database) = database_or_skip(
            "unregister_collection_cleans_up_legacy_collection_views_when_available",
        ) else {
            return;
        };

        let (legacy_client, legacy_rt) = connect_raw_for_test(database.url());
        legacy_rt
            .block_on(async {
                legacy_client
                    .batch_execute(
                        "DROP TABLE IF EXISTS collection_views;
                         DROP TABLE IF EXISTS collections;
                         DROP TABLE IF EXISTS namespaces;
                         DROP TABLE IF EXISTS databases;
                         DROP TABLE IF EXISTS entities;
                         DROP TABLE IF EXISTS schemas;
                         DROP TABLE IF EXISTS audit_log;",
                    )
                    .await?;
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
                    .await?;
                Ok::<_, tokio_postgres::Error>(())
            })
            .expect("legacy schema should be created");

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

        legacy_rt
            .block_on(async {
                legacy_client
                    .execute(
                        "INSERT INTO collections (name) VALUES ($1)",
                        &[&collection.as_str()],
                    )
                    .await?;
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
                    .await?;
                Ok::<_, tokio_postgres::Error>(())
            })
            .expect("legacy data should insert");
        drop(legacy_client);
        drop(legacy_rt);

        let mut adapter = PostgresStorageAdapter::connect(database.url())
            .expect("adapter should connect after upgrade");
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
            .block(adapter.client.query_one(
                "SELECT COUNT(*) FROM collection_views WHERE collection = $1",
                &[&collection.as_str()],
            ))
            .expect("count query should succeed")
            .get(0);
        assert_eq!(
            remaining_views, 0,
            "stale collection view rows must be deleted"
        );
    }

    #[test]
    fn namespace_catalogs_allow_same_name_without_cross_drop() {
        let _guard = postgres_test_guard();
        let Some(mut s) = store_or_skip("namespace_catalogs_allow_same_name_without_cross_drop")
        else {
            return;
        };
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
        let _guard = postgres_test_guard();
        let Some(mut s) = store_or_skip("drop_namespace_purges_entities_for_removed_collections")
        else {
            return;
        };
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
            serde_json::json!({"title": "invoice"}),
        ))
        .expect("billing entity put should succeed");
        s.put(Entity::new(
            ledger.clone(),
            EntityId::new("led-001"),
            serde_json::json!({"title": "ledger"}),
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
        let _guard = postgres_test_guard();
        let Some(mut s) =
            store_or_skip("drop_namespace_keeps_same_named_entities_in_surviving_namespaces")
        else {
            return;
        };
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
            serde_json::json!({"title": "default invoice"}),
        ))
        .expect("default entity put should succeed");
        s.put(Entity::new(
            ledger.clone(),
            EntityId::new("led-billing-001"),
            serde_json::json!({"title": "billing ledger"}),
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
        let _guard = postgres_test_guard();
        let Some(mut s) = store_or_skip("drop_namespace_purges_links_for_removed_collections")
        else {
            return;
        };
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
                serde_json::json!({"title": "invoice"}),
            ),
            Entity::new(
                ledger.clone(),
                EntityId::new("led-001"),
                serde_json::json!({"title": "ledger"}),
            ),
            Entity::new(
                keep.clone(),
                EntityId::new("keep-001"),
                serde_json::json!({"title": "keep"}),
            ),
            Entity::new(
                archive.clone(),
                EntityId::new("arc-001"),
                serde_json::json!({"title": "archive"}),
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
        let _guard = postgres_test_guard();
        let Some(mut s) = store_or_skip("drop_database_purges_entities_for_removed_collections")
        else {
            return;
        };
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
            serde_json::json!({"title": "order"}),
        ))
        .expect("prod default entity put should succeed");
        s.put(Entity::new(
            rollups.clone(),
            EntityId::new("sum-001"),
            serde_json::json!({"title": "rollup"}),
        ))
        .expect("analytics entity put should succeed");
        s.put(Entity::new(
            keep.clone(),
            EntityId::new("keep-001"),
            serde_json::json!({"title": "keep"}),
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
        let _guard = postgres_test_guard();
        let Some(mut s) = store_or_skip("drop_database_purges_links_for_removed_collections")
        else {
            return;
        };
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
                serde_json::json!({"title": "order"}),
            ),
            Entity::new(
                rollups.clone(),
                EntityId::new("sum-001"),
                serde_json::json!({"title": "rollup"}),
            ),
            Entity::new(
                keep.clone(),
                EntityId::new("keep-001"),
                serde_json::json!({"title": "keep"}),
            ),
            Entity::new(
                archive.clone(),
                EntityId::new("arc-001"),
                serde_json::json!({"title": "archive"}),
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
        let _guard = postgres_test_guard();
        let Some(mut s) =
            store_or_skip("drop_database_keeps_same_named_entities_in_surviving_databases")
        else {
            return;
        };
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
            serde_json::json!({"title": "default invoice"}),
        ))
        .expect("default entity put should succeed");
        s.put(Entity::new(
            orders.clone(),
            EntityId::new("ord-prod-001"),
            serde_json::json!({"title": "prod order"}),
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
    fn qualified_entity_identity_isolated_across_namespaces() {
        let _guard = postgres_test_guard();
        let Some(mut s) = store_or_skip("qualified_entity_identity_isolated_across_namespaces")
        else {
            return;
        };
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
            serde_json::json!({"scope": "billing"}),
        ))
        .expect("billing entity put should succeed");
        s.put(Entity::new(
            engineering_invoices.clone(),
            entity_id.clone(),
            serde_json::json!({"scope": "engineering"}),
        ))
        .expect("engineering entity put should succeed");

        assert_eq!(
            s.get(&billing_invoices, &entity_id)
                .expect("billing get should succeed")
                .expect("billing entity should exist")
                .data["scope"],
            serde_json::json!("billing")
        );
        assert_eq!(
            s.get(&engineering_invoices, &entity_id)
                .expect("engineering get should succeed")
                .expect("engineering entity should exist")
                .data["scope"],
            serde_json::json!("engineering")
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
                    serde_json::json!({"scope": "billing-updated"}),
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

    #[test]
    fn qualified_schema_write_is_readable_via_bare_unique_collection() {
        let _guard = postgres_test_guard();
        let Some(mut s) =
            store_or_skip("qualified_schema_write_is_readable_via_bare_unique_collection")
        else {
            return;
        };
        let qualified = CollectionId::new("prod.billing.invoices");
        let (billing, invoices) = register_unique_namespaced_collection(&mut s, &qualified);

        let v1 = CollectionSchema {
            collection: qualified.clone(),
            description: Some("v1".into()),
            version: 99,
            entity_schema: Some(
                serde_json::json!({"type": "object", "properties": {"title": {"type": "string"}}}),
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
                serde_json::json!({"type": "object", "properties": {"amount": {"type": "number"}}}),
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

        let stored_collections: Vec<String> = s
            .block(s.client.query(
                "SELECT collection FROM schemas
                 WHERE database_name = $1 AND schema_name = $2
                 ORDER BY version ASC",
                &[&billing.database, &billing.schema],
            ))
            .expect("schema version query should succeed")
            .into_iter()
            .map(|row| row.get("collection"))
            .collect();
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
        let _guard = postgres_test_guard();
        let Some(mut s) =
            store_or_skip("qualified_collection_view_write_is_readable_via_bare_unique_collection")
        else {
            return;
        };
        let qualified = CollectionId::new("prod.billing.invoices");
        let (billing, invoices) = register_unique_namespaced_collection(&mut s, &qualified);

        let stored = s
            .put_collection_view(&CollectionView::new(qualified, "# {{title}}"))
            .expect("qualified collection view put should succeed");
        assert_eq!(stored.collection, invoices);
        assert_eq!(stored.version, 1);

        let stored_collection: String = s
            .block(s.client.query_one(
                "SELECT collection FROM collection_views
                 WHERE database_name = $1 AND schema_name = $2",
                &[&billing.database, &billing.schema],
            ))
            .expect("stored collection view lookup should succeed")
            .get("collection");
        assert_eq!(stored_collection, "invoices");

        let retrieved = s
            .get_collection_view(&invoices)
            .expect("bare collection view lookup should succeed")
            .expect("collection view should exist");
        assert_eq!(retrieved.collection, invoices);
        assert_eq!(retrieved.markdown_template, "# {{title}}");
        assert_eq!(retrieved.version, 1);
    }

    #[test]
    fn qualified_unregister_collection_removes_normalized_metadata_rows() {
        let _guard = postgres_test_guard();
        let Some(mut s) =
            store_or_skip("qualified_unregister_collection_removes_normalized_metadata_rows")
        else {
            return;
        };
        let qualified = CollectionId::new("prod.billing.invoices");
        let (billing, invoices) = register_unique_namespaced_collection(&mut s, &qualified);

        s.put_schema(&CollectionSchema {
            collection: qualified.clone(),
            description: Some("v1".into()),
            version: 1,
            entity_schema: Some(
                serde_json::json!({"type": "object", "properties": {"title": {"type": "string"}}}),
            ),
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            lifecycles: Default::default(),
        })
        .expect("qualified schema put should succeed");
        s.put_collection_view(&CollectionView::new(qualified.clone(), "# {{title}}"))
            .expect("qualified collection view put should succeed");

        s.unregister_collection(&qualified)
            .expect("qualified unregister should succeed");

        let row_counts = s
            .block(s.client.query_one(
                "SELECT
                    (SELECT COUNT(*) FROM collections
                     WHERE name = $1 AND database_name = $2 AND schema_name = $3) AS collections_count,
                    (SELECT COUNT(*) FROM schemas
                     WHERE collection = $1 AND database_name = $2 AND schema_name = $3) AS schemas_count,
                    (SELECT COUNT(*) FROM collection_views
                     WHERE collection = $1 AND database_name = $2 AND schema_name = $3) AS views_count",
                &[&invoices.as_str(), &billing.database, &billing.schema],
            ))
            .expect("metadata row count query should succeed");

        let collections_count: i64 = row_counts.get("collections_count");
        let schemas_count: i64 = row_counts.get("schemas_count");
        let views_count: i64 = row_counts.get("views_count");

        assert_eq!(collections_count, 0, "collection row should be removed");
        assert_eq!(schemas_count, 0, "schema rows should be removed");
        assert_eq!(views_count, 0, "collection view row should be removed");
    }

    #[test]
    fn qualified_unregister_collection_removes_default_namespaced_legacy_metadata_rows() {
        let _guard = postgres_test_guard();
        let Some(mut s) = store_or_skip(
            "qualified_unregister_collection_removes_default_namespaced_legacy_metadata_rows",
        ) else {
            return;
        };
        let qualified = CollectionId::new("prod.billing.invoices");
        let (billing, invoices) = register_unique_namespaced_collection(&mut s, &qualified);

        let schema = CollectionSchema {
            collection: qualified.clone(),
            description: Some("v1".into()),
            version: 1,
            entity_schema: Some(
                serde_json::json!({"type": "object", "properties": {"title": {"type": "string"}}}),
            ),
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            lifecycles: Default::default(),
        };
        s.put_schema(&schema)
            .expect("qualified schema put should succeed");
        let view = CollectionView::new(qualified.clone(), "# {{title}}");
        s.put_collection_view(&view)
            .expect("qualified collection view put should succeed");

        let legacy_schema_json =
            serde_json::to_value(&schema).expect("legacy schema should serialize");
        let legacy_view_json =
            serde_json::to_value(&view).expect("legacy collection view should serialize");
        s.block(s.client.execute(
            "INSERT INTO collections (name, database_name, schema_name)
                 VALUES ($1, $2, $3)",
            &[&qualified.as_str(), &DEFAULT_DATABASE, &DEFAULT_SCHEMA],
        ))
        .expect("legacy collection row should insert");
        s.block(s.client.execute(
            "INSERT INTO schemas
                    (collection, database_name, schema_name, version, schema_json, created_at_ns)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            &[
                &qualified.as_str(),
                &DEFAULT_DATABASE,
                &DEFAULT_SCHEMA,
                &99_i32,
                &legacy_schema_json,
                &42_i64,
            ],
        ))
        .expect("legacy schema row should insert");
        s.block(s.client.execute(
            "INSERT INTO collection_views
                    (collection, database_name, schema_name, version, view_json, updated_at_ns, updated_by)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
            &[
                &qualified.as_str(),
                &DEFAULT_DATABASE,
                &DEFAULT_SCHEMA,
                &99_i32,
                &legacy_view_json,
                &42_i64,
                &Some("legacy"),
            ],
        ))
        .expect("legacy collection view row should insert");

        let before_unregister: i64 = s
            .block(s.client.query_one(
                "SELECT
                    (SELECT COUNT(*) FROM collections
                     WHERE (name = $1 AND database_name = $2 AND schema_name = $3)
                        OR (name = $4 AND database_name = $5 AND schema_name = $6)) +
                    (SELECT COUNT(*) FROM schemas
                     WHERE (collection = $1 AND database_name = $2 AND schema_name = $3)
                        OR (collection = $4 AND database_name = $5 AND schema_name = $6)) +
                    (SELECT COUNT(*) FROM collection_views
                     WHERE (collection = $1 AND database_name = $2 AND schema_name = $3)
                        OR (collection = $4 AND database_name = $5 AND schema_name = $6))",
                &[
                    &invoices.as_str(),
                    &billing.database,
                    &billing.schema,
                    &qualified.as_str(),
                    &DEFAULT_DATABASE,
                    &DEFAULT_SCHEMA,
                ],
            ))
            .expect("metadata row count before unregister should succeed")
            .get(0);
        assert_eq!(
            before_unregister, 6,
            "test setup should include normalized and legacy rows across all catalog tables"
        );

        s.unregister_collection(&qualified)
            .expect("qualified unregister should succeed");

        let remaining_metadata_rows: i64 = s
            .block(s.client.query_one(
                "SELECT
                    (SELECT COUNT(*) FROM collections
                     WHERE (name = $1 AND database_name = $2 AND schema_name = $3)
                        OR (name = $4 AND database_name = $5 AND schema_name = $6)) +
                    (SELECT COUNT(*) FROM schemas
                     WHERE (collection = $1 AND database_name = $2 AND schema_name = $3)
                        OR (collection = $4 AND database_name = $5 AND schema_name = $6)) +
                    (SELECT COUNT(*) FROM collection_views
                     WHERE (collection = $1 AND database_name = $2 AND schema_name = $3)
                        OR (collection = $4 AND database_name = $5 AND schema_name = $6))",
                &[
                    &invoices.as_str(),
                    &billing.database,
                    &billing.schema,
                    &qualified.as_str(),
                    &DEFAULT_DATABASE,
                    &DEFAULT_SCHEMA,
                ],
            ))
            .expect("metadata row count after unregister should succeed")
            .get(0);
        assert_eq!(
            remaining_metadata_rows, 0,
            "qualified unregister must remove normalized and default-namespaced legacy metadata"
        );
    }
}
