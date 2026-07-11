use std::future::Future;
use std::ops::Bound;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use axon_audit::entry::AuditEntry;
use axon_audit::log::{AuditPage, AuditQuery};
use axon_core::auth::TenantDatabase;
use axon_core::error::AxonError;
use axon_core::id::{
    CollectionId, EntityId, Namespace, QualifiedCollectionId, DEFAULT_DATABASE, DEFAULT_SCHEMA,
};
use axon_core::intent::{ApprovalState, MutationIntent};
use axon_core::types::Entity;
use axon_schema::schema::{CollectionSchema, CollectionView};
use sqlx::postgres::PgConnectOptions;

use crate::adapter::{
    filter_audit_entries_for_query, prefix_successor, CompoundKey, IndexValue, StorageAdapter,
};

pub const POSTGRES_MUTATING_ROUTINE_NAME: &str = "axon_record_mutation_intent";
pub const POSTGRES_MUTATING_ROUTINE_SIGNATURE: &str =
    "axon_record_mutation_intent(TEXT, TEXT, TEXT, TEXT, TEXT, BIGINT, JSONB)";

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PostgresTenantRoles {
    pub runtime: String,
    pub capability: String,
    pub migration: String,
}

fn pg_connect_options(params: &str) -> Result<PgConnectOptions, AxonError> {
    if params.starts_with("postgres://") || params.starts_with("postgresql://") {
        return PgConnectOptions::from_str(params)
            .map_err(|e| AxonError::Storage(format!("invalid PostgreSQL DSN: {e}")));
    }

    let mut options = PgConnectOptions::new();
    for part in params.split_whitespace() {
        let Some((key, value)) = part.split_once('=') else {
            return Err(AxonError::InvalidArgument(format!(
                "invalid PostgreSQL keyword-value DSN part '{part}'"
            )));
        };
        options = match key {
            "host" => options.host(value),
            "port" => {
                let port = value.parse::<u16>().map_err(|e| {
                    AxonError::InvalidArgument(format!("invalid PostgreSQL port '{value}': {e}"))
                })?;
                options.port(port)
            }
            "user" | "username" => options.username(value),
            "password" => options.password(value),
            "dbname" | "database" => options.database(value),
            _ => options,
        };
    }
    Ok(options)
}

/// PostgreSQL-backed storage adapter.
///
/// Uses `sqlx::PgPool` for connection pooling. Because `StorageAdapter` is
/// synchronous, each database call blocks the calling thread via a dedicated
/// Tokio runtime.
///
/// Transactions are handled via `BEGIN` / `COMMIT` / `ROLLBACK` statements.
/// The adapter creates the required tables on initialization if they do not
/// exist.
pub struct PostgresStorageAdapter {
    pool: sqlx::PgPool,
    /// Owned runtime — only used when no outer tokio context exists.
    /// When constructed inside a gateway handler or `#[tokio::test]`,
    /// this is `None` and the caller's runtime is reused.
    rt: Option<tokio::runtime::Runtime>,
    in_tx: bool,
}

impl PostgresStorageAdapter {
    /// Run an async future, handling both async and non-async caller contexts.
    fn run_on<T>(
        owned_rt: Option<&tokio::runtime::Runtime>,
        fut: impl std::future::Future<Output = T>,
    ) -> T {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
            Err(_) => owned_rt
                .expect("PostgresStorageAdapter: no tokio runtime available")
                .block_on(fut),
        }
    }

    /// Connect to a PostgreSQL database using a connection string.
    ///
    /// Example: `"host=localhost user=axon dbname=axon"` or
    /// `"postgres://axon@localhost/axon"`
    pub fn connect(params: &str) -> Result<Self, AxonError> {
        let options = pg_connect_options(params)?;
        // max_connections(1) ensures that BEGIN / COMMIT / ROLLBACK are issued on
        // the same underlying connection.  StorageAdapter uses &mut self (exclusive
        // access), so a single connection is the correct and sufficient model.
        let pool_opts = sqlx::postgres::PgPoolOptions::new().max_connections(1);
        let (rt, pool) = match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                let pool = tokio::task::block_in_place(|| {
                    handle.block_on(pool_opts.connect_with(options))
                })
                .map_err(|e| AxonError::Storage(format!("connection failed: {e}")))?;
                (None, pool)
            }
            Err(_) => {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                    .map_err(|e| AxonError::Storage(e.to_string()))?;
                let pool = rt
                    .block_on(pool_opts.connect_with(options))
                    .map_err(|e| AxonError::Storage(format!("connection failed: {e}")))?;
                (Some(rt), pool)
            }
        };
        let mut adapter = Self {
            pool,
            rt,
            in_tx: false,
        };
        adapter.init_schema()?;
        adapter.backfill_indexes_on_open()?;
        Ok(adapter)
    }

    /// Block the current thread on an sqlx future, converting any error into
    /// `AxonError::Storage`.
    fn block_on<T>(
        &self,
        fut: impl Future<Output = Result<T, sqlx::Error>>,
    ) -> Result<T, AxonError> {
        Self::run_on(self.rt.as_ref(), fut).map_err(|e| AxonError::Storage(e.to_string()))
    }

    /// Apply auth/tenancy schema migrations to this adapter's PostgreSQL connection.
    ///
    /// Creates the `users`, `user_identities`, `tenant_users`, and related tables.
    /// This is idempotent — safe to call multiple times.
    pub fn apply_auth_migrations(&self) -> Result<(), AxonError> {
        self.block_on(async {
            crate::auth_schema::apply_auth_migrations_postgres(&self.pool)
                .await
                .map_err(sqlx::Error::Protocol)
        })
    }

    /// Insert a tenant and user row for test fixture setup.
    ///
    /// Both rows are inserted with `ON CONFLICT DO NOTHING` so the call is
    /// idempotent.  Intended for integration tests that need valid FK
    /// references in `tenants` and `users` before calling
    /// `upsert_tenant_member`.
    pub fn test_insert_tenant_and_user(
        &self,
        tenant_id: &str,
        user_id: &str,
    ) -> Result<(), AxonError> {
        let now = 1_000_000i64;
        self.block_on(
            sqlx::query(
                "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms) \
                 VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
            )
            .bind(tenant_id)
            .bind(tenant_id)
            .bind(tenant_id)
            .bind(now)
            .bind(now)
            .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "INSERT INTO users (id, display_name, created_at_ms) \
                 VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
            )
            .bind(user_id)
            .bind(user_id)
            .bind(now)
            .execute(&self.pool),
        )?;
        Ok(())
    }

    /// Count tenant_users rows matching (tenant_id, user_id) for test assertions.
    pub fn test_count_tenant_members(
        &self,
        tenant_id: &str,
        user_id: &str,
    ) -> Result<i64, AxonError> {
        let row = self.block_on(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM tenant_users \
                 WHERE tenant_id = $1 AND user_id = $2",
            )
            .bind(tenant_id)
            .bind(user_id)
            .fetch_one(&self.pool),
        )?;
        Ok(row)
    }

    fn init_schema(&mut self) -> Result<(), AxonError> {
        self.block_on(
            sqlx::raw_sql(
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
                );
                CREATE TABLE IF NOT EXISTS mutation_intents (
                    tenant_id      TEXT NOT NULL,
                    database_id    TEXT NOT NULL,
                    intent_id      TEXT NOT NULL,
                    decision       TEXT NOT NULL,
                    approval_state TEXT NOT NULL,
                    expires_at_ns  BIGINT NOT NULL,
                    intent_json    JSONB NOT NULL,
                    PRIMARY KEY (tenant_id, database_id, intent_id)
                );",
            )
            .execute(&self.pool),
        )?;
        self.ensure_postgres_routines()?;
        self.block_on(
            sqlx::raw_sql(
                "CREATE INDEX IF NOT EXISTS idx_mutation_intents_pending
                     ON mutation_intents
                        (tenant_id, database_id, approval_state, expires_at_ns, intent_id);
                 CREATE INDEX IF NOT EXISTS idx_mutation_intents_expired
                     ON mutation_intents
                        (tenant_id, database_id, expires_at_ns, approval_state, intent_id);",
            )
            .execute(&self.pool),
        )?;
        // Persisted single-field secondary index (FEAT-013). `key` holds the
        // canonical order-preserving bytes from `axon_esf::encode_index_value`.
        // Postgres compares `BYTEA` bytewise (no text collation involvement), so
        // range scans over `key` honour the same ordering the in-memory adapter's
        // typed `IndexValue` `Ord` produces — identical to SQLite's BLOB memcmp.
        self.block_on(
            sqlx::raw_sql(
                "CREATE TABLE IF NOT EXISTS entity_index (
                    database_name TEXT NOT NULL,
                    schema_name   TEXT NOT NULL,
                    collection    TEXT NOT NULL,
                    field         TEXT NOT NULL,
                    key           BYTEA NOT NULL,
                    entity_id     TEXT NOT NULL,
                    PRIMARY KEY (database_name, schema_name, collection, field, key, entity_id)
                );
                CREATE INDEX IF NOT EXISTS idx_entity_index_range
                    ON entity_index (database_name, schema_name, collection, field, key);
                CREATE TABLE IF NOT EXISTS entity_compound_index (
                    database_name TEXT NOT NULL,
                    schema_name   TEXT NOT NULL,
                    collection    TEXT NOT NULL,
                    index_ordinal INTEGER NOT NULL,
                    key           BYTEA NOT NULL,
                    entity_id     TEXT NOT NULL,
                    PRIMARY KEY (database_name, schema_name, collection, index_ordinal, key, entity_id)
                );
                CREATE INDEX IF NOT EXISTS idx_entity_compound_index_range
                    ON entity_compound_index (database_name, schema_name, collection, index_ordinal, key);",
            )
            .execute(&self.pool),
        )?;
        self.ensure_namespace_catalog_tables()?;
        self.ensure_default_namespace()
    }

    fn ensure_postgres_routines(&self) -> Result<(), AxonError> {
        self.block_on(
            sqlx::raw_sql(
                "CREATE OR REPLACE FUNCTION axon_record_mutation_intent(
                    p_tenant_id TEXT,
                    p_database_id TEXT,
                    p_intent_id TEXT,
                    p_decision TEXT,
                    p_approval_state TEXT,
                    p_expires_at_ns BIGINT,
                    p_intent_json JSONB
                )
                RETURNS void
                LANGUAGE SQL
                SECURITY DEFINER
                SET search_path = public
                AS '
                    INSERT INTO mutation_intents (
                        tenant_id,
                        database_id,
                        intent_id,
                        decision,
                        approval_state,
                        expires_at_ns,
                        intent_json
                    )
                    VALUES (
                        p_tenant_id,
                        p_database_id,
                        p_intent_id,
                        p_decision,
                        p_approval_state,
                        p_expires_at_ns,
                        p_intent_json
                    )
                    ON CONFLICT (tenant_id, database_id, intent_id)
                    DO UPDATE SET
                        decision = EXCLUDED.decision,
                        approval_state = EXCLUDED.approval_state,
                        expires_at_ns = EXCLUDED.expires_at_ns,
                        intent_json = EXCLUDED.intent_json
                ';
                REVOKE ALL ON FUNCTION axon_record_mutation_intent(
                    TEXT,
                    TEXT,
                    TEXT,
                    TEXT,
                    TEXT,
                    BIGINT,
                    JSONB
                ) FROM PUBLIC;",
            )
            .execute(&self.pool),
        )?;
        Ok(())
    }

    fn collection_exists_in_namespace(
        &self,
        collection: &CollectionId,
        namespace: &Namespace,
    ) -> Result<bool, AxonError> {
        use sqlx::Row;
        let row = self.block_on(
            sqlx::query(
                "SELECT EXISTS(
                    SELECT 1 FROM collections
                    WHERE name = $1 AND database_name = $2 AND schema_name = $3
                )",
            )
            .bind(collection.as_str())
            .bind(namespace.database.as_str())
            .bind(namespace.schema.as_str())
            .fetch_one(&self.pool),
        )?;
        Ok(row.get::<bool, _>(0))
    }

    fn database_exists(&self, database: &str) -> Result<bool, AxonError> {
        use sqlx::Row;
        let row = self.block_on(
            sqlx::query("SELECT EXISTS(SELECT 1 FROM databases WHERE name = $1)")
                .bind(database)
                .fetch_one(&self.pool),
        )?;
        Ok(row.get::<bool, _>(0))
    }

    fn namespace_exists(&self, namespace: &Namespace) -> Result<bool, AxonError> {
        use sqlx::Row;
        let row = self.block_on(
            sqlx::query(
                "SELECT EXISTS(
                    SELECT 1 FROM namespaces
                    WHERE database_name = $1 AND name = $2
                )",
            )
            .bind(namespace.database.as_str())
            .bind(namespace.schema.as_str())
            .fetch_one(&self.pool),
        )?;
        Ok(row.get::<bool, _>(0))
    }

    fn table_pk_columns(&self, table: &str) -> Result<Vec<String>, AxonError> {
        use sqlx::Row;
        let rows = self.block_on(
            sqlx::query(
                "SELECT a.attname
                 FROM pg_index i
                 JOIN pg_class t ON t.oid = i.indrelid
                 JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS cols(attnum, ord) ON TRUE
                 JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = cols.attnum
                 WHERE t.relname = $1 AND i.indisprimary
                 ORDER BY cols.ord",
            )
            .bind(table)
            .fetch_all(&self.pool),
        )?;
        Ok(rows
            .iter()
            .map(|row| row.get::<String, _>("attname"))
            .collect())
    }

    fn ensure_namespace_catalog_tables(&mut self) -> Result<(), AxonError> {
        self.block_on(
            sqlx::raw_sql(
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
            )
            .execute(&self.pool),
        )?;

        if self.table_pk_columns("entities")?
            != vec!["database_name", "schema_name", "collection", "id"]
        {
            self.block_on(
                sqlx::raw_sql(
                    "ALTER TABLE entities DROP CONSTRAINT IF EXISTS entities_pkey;
                     ALTER TABLE entities ADD PRIMARY KEY (database_name, schema_name, collection, id);",
                )
                .execute(&self.pool),
            )?;
        }

        self.block_on(
            sqlx::query(
                "UPDATE schemas s
                 SET database_name = c.database_name,
                     schema_name = c.schema_name
                 FROM collections c
                 WHERE s.collection = c.name
                   AND (s.database_name = 'default' OR s.schema_name = 'default')",
            )
            .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "UPDATE collection_views v
                 SET database_name = c.database_name,
                     schema_name = c.schema_name
                 FROM collections c
                 WHERE v.collection = c.name
                   AND (v.database_name = 'default' OR v.schema_name = 'default')",
            )
            .execute(&self.pool),
        )?;

        if self.table_pk_columns("collections")? != vec!["database_name", "schema_name", "name"] {
            self.block_on(
                sqlx::raw_sql(
                    "ALTER TABLE collection_views DROP CONSTRAINT IF EXISTS collection_views_collection_fkey;
                     ALTER TABLE collection_views DROP CONSTRAINT IF EXISTS collection_views_pkey;
                     ALTER TABLE collections DROP CONSTRAINT IF EXISTS collections_pkey;
                     ALTER TABLE collections ADD PRIMARY KEY (database_name, schema_name, name);",
                )
                .execute(&self.pool),
            )?;
        }

        if self.table_pk_columns("schemas")?
            != vec!["database_name", "schema_name", "collection", "version"]
        {
            self.block_on(
                sqlx::raw_sql(
                    "ALTER TABLE schemas DROP CONSTRAINT IF EXISTS schemas_pkey;
                     ALTER TABLE schemas
                         ADD PRIMARY KEY (database_name, schema_name, collection, version);",
                )
                .execute(&self.pool),
            )?;
        }

        if self.table_pk_columns("collection_views")?
            != vec!["database_name", "schema_name", "collection"]
        {
            self.block_on(
                sqlx::raw_sql(
                    "ALTER TABLE collection_views DROP CONSTRAINT IF EXISTS collection_views_pkey;
                     ALTER TABLE collection_views ADD PRIMARY KEY (database_name, schema_name, collection);",
                )
                .execute(&self.pool),
            )?;
        }

        self.block_on(
            sqlx::raw_sql(
                "ALTER TABLE collection_views DROP CONSTRAINT IF EXISTS collection_views_collection_fkey;
                 ALTER TABLE collection_views
                     ADD CONSTRAINT collection_views_collection_fkey
                     FOREIGN KEY (database_name, schema_name, collection)
                     REFERENCES collections(database_name, schema_name, name)
                     ON DELETE CASCADE;
                 CREATE INDEX IF NOT EXISTS idx_collections_namespace
                     ON collections (database_name, schema_name, name);",
            )
            .execute(&self.pool),
        )?;
        Ok(())
    }

    fn registered_collection_namespaces(
        &self,
        collection: &CollectionId,
    ) -> Result<Vec<Namespace>, AxonError> {
        use sqlx::Row;
        let rows = self.block_on(
            sqlx::query(
                "SELECT database_name, schema_name FROM collections
                 WHERE name = $1
                 ORDER BY CASE
                     WHEN database_name = 'default' AND schema_name = 'default' THEN 0
                     ELSE 1
                 END,
                 database_name,
                 schema_name",
            )
            .bind(collection.as_str())
            .fetch_all(&self.pool),
        )?;
        Ok(rows
            .iter()
            .map(|row| {
                Namespace::new(
                    row.get::<String, _>("database_name"),
                    row.get::<String, _>("schema_name"),
                )
            })
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
        use sqlx::Row;
        let rows = self.block_on(
            sqlx::query(
                "SELECT name FROM collections
                 WHERE database_name = $1 AND schema_name = $2
                 ORDER BY name ASC",
            )
            .bind(namespace.database.as_str())
            .bind(namespace.schema.as_str())
            .fetch_all(&self.pool),
        )?;
        Ok(rows
            .iter()
            .map(|row| {
                QualifiedCollectionId::from_parts(
                    namespace,
                    &CollectionId::new(row.get::<String, _>("name")),
                )
            })
            .collect())
    }

    fn database_collection_keys(
        &self,
        database: &str,
    ) -> Result<Vec<QualifiedCollectionId>, AxonError> {
        use sqlx::Row;
        let rows = self.block_on(
            sqlx::query(
                "SELECT schema_name, name FROM collections
                 WHERE database_name = $1
                 ORDER BY schema_name ASC, name ASC",
            )
            .bind(database)
            .fetch_all(&self.pool),
        )?;
        Ok(rows
            .iter()
            .map(|row| {
                QualifiedCollectionId::from_parts(
                    &Namespace::new(database, row.get::<String, _>("schema_name")),
                    &CollectionId::new(row.get::<String, _>("name")),
                )
            })
            .collect())
    }

    fn ensure_default_namespace(&self) -> Result<(), AxonError> {
        self.block_on(
            sqlx::query("INSERT INTO databases (name) VALUES ($1) ON CONFLICT DO NOTHING")
                .bind(DEFAULT_DATABASE)
                .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "INSERT INTO namespaces (database_name, name)
                 VALUES ($1, $2)
                 ON CONFLICT DO NOTHING",
            )
            .bind(DEFAULT_DATABASE)
            .bind(DEFAULT_SCHEMA)
            .execute(&self.pool),
        )?;
        Ok(())
    }

    fn row_to_entity(row: &sqlx::postgres::PgRow) -> Result<Entity, AxonError> {
        use sqlx::Row;
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

    fn row_to_mutation_intent(row: &sqlx::postgres::PgRow) -> Result<MutationIntent, AxonError> {
        use sqlx::Row;
        let intent_json: serde_json::Value = row.get("intent_json");
        serde_json::from_value(intent_json).map_err(AxonError::Serialization)
    }

    // ── Persisted secondary index helpers (FEAT-013) ────────────────────
    //
    // These mirror the SQLite adapter's private index helpers 1:1 over Postgres
    // types: `BYTEA` keys (compared bytewise, matching the canonical
    // order-preserving encoding), `$N` placeholders, and `ON CONFLICT DO NOTHING`
    // in place of SQLite's `INSERT OR REPLACE`. The key bytes themselves come
    // from the shared encoders, so all three backends store identical bytes.

    /// Delete this entity's `entity_index` rows for a single field.
    fn delete_index_field_rows(
        &self,
        key: &QualifiedCollectionId,
        field: &str,
        entity_id: &EntityId,
    ) -> Result<(), AxonError> {
        self.block_on(
            sqlx::query(
                "DELETE FROM entity_index
                 WHERE database_name = $1 AND schema_name = $2 AND collection = $3
                   AND field = $4 AND entity_id = $5",
            )
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .bind(key.collection.as_str())
            .bind(field)
            .bind(entity_id.as_str())
            .execute(&self.pool),
        )?;
        Ok(())
    }

    /// Insert one `entity_index` row per byte key for a field.
    fn insert_index_rows(
        &self,
        key: &QualifiedCollectionId,
        field: &str,
        entity_id: &EntityId,
        byte_keys: &[Vec<u8>],
    ) -> Result<(), AxonError> {
        for bytes in byte_keys {
            self.block_on(
                sqlx::query(
                    "INSERT INTO entity_index
                        (database_name, schema_name, collection, field, key, entity_id)
                     VALUES ($1, $2, $3, $4, $5, $6)
                     ON CONFLICT DO NOTHING",
                )
                .bind(key.namespace.database.as_str())
                .bind(key.namespace.schema.as_str())
                .bind(key.collection.as_str())
                .bind(field)
                .bind(bytes.as_slice())
                .bind(entity_id.as_str())
                .execute(&self.pool),
            )?;
        }
        Ok(())
    }

    /// Delete this entity's `entity_compound_index` rows for one ordinal.
    fn delete_compound_index_rows(
        &self,
        key: &QualifiedCollectionId,
        index_ordinal: i32,
        entity_id: &EntityId,
    ) -> Result<(), AxonError> {
        self.block_on(
            sqlx::query(
                "DELETE FROM entity_compound_index
                 WHERE database_name = $1 AND schema_name = $2 AND collection = $3
                   AND index_ordinal = $4 AND entity_id = $5",
            )
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .bind(key.collection.as_str())
            .bind(index_ordinal)
            .bind(entity_id.as_str())
            .execute(&self.pool),
        )?;
        Ok(())
    }

    /// Insert one `entity_compound_index` row for a framed composite key.
    fn insert_compound_index_row(
        &self,
        key: &QualifiedCollectionId,
        index_ordinal: i32,
        entity_id: &EntityId,
        framed: &[u8],
    ) -> Result<(), AxonError> {
        self.block_on(
            sqlx::query(
                "INSERT INTO entity_compound_index
                    (database_name, schema_name, collection, index_ordinal, key, entity_id)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT DO NOTHING",
            )
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .bind(key.collection.as_str())
            .bind(index_ordinal)
            .bind(framed)
            .bind(entity_id.as_str())
            .execute(&self.pool),
        )?;
        Ok(())
    }

    // ── Shared index-maintenance helpers (Approach C) ────────────────────
    //
    // These operate on an already-resolved `QualifiedCollectionId` and issue
    // DELETE-by-(field/ordinal, entity_id) + INSERT-new statements. The write
    // primitives (`put`/`compare_and_swap`/`delete`/`create_if_absent`) route
    // through them, so maintenance logic lives in exactly one place. Because
    // deletion is by
    // entity_id (not by recomputed old value), `put`/`cas` never need to read
    // the prior entity for index purposes — delete-by-entity + insert-from-new
    // is sufficient (mirrors the SQLite primitives, which likewise do not
    // capture a prior image).

    /// Whether a unique value already exists for a *different* entity in the
    /// single-field index, against an already-resolved key.
    fn single_unique_conflict_at(
        &self,
        key: &QualifiedCollectionId,
        field: &str,
        value: &IndexValue,
        exclude_entity: &EntityId,
    ) -> Result<bool, AxonError> {
        let Ok(bytes) = value.encode_key() else {
            return Ok(false);
        };
        let found: Option<i32> = self.block_on(
            sqlx::query_scalar(
                "SELECT 1 FROM entity_index
                 WHERE database_name = $1 AND schema_name = $2 AND collection = $3
                   AND field = $4 AND key = $5 AND entity_id <> $6
                 LIMIT 1",
            )
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .bind(key.collection.as_str())
            .bind(field)
            .bind(bytes.as_slice())
            .bind(exclude_entity.as_str())
            .fetch_optional(&self.pool),
        )?;
        Ok(found.is_some())
    }

    /// Maintain single-field index rows for a write, on a resolved key.
    ///
    /// Validate-then-mutate: unique values are checked FIRST (so a violation
    /// returns before any DELETE/INSERT). Then this entity's prior rows are
    /// removed by `(field, entity_id)` when `had_old` and the new byte keys are
    /// inserted. Atomicity is the caller's responsibility (owned/joined tx).
    fn maintain_single_indexes_at(
        &self,
        key: &QualifiedCollectionId,
        entity_id: &EntityId,
        had_old: bool,
        new_data: &serde_json::Value,
        indexes: &[axon_schema::schema::IndexDef],
    ) -> Result<(), AxonError> {
        for idx in indexes.iter().filter(|i| i.unique) {
            let values = crate::extract_index_values(new_data, &idx.field, &idx.index_type);
            for val in &values {
                if self.single_unique_conflict_at(key, &idx.field, val, entity_id)? {
                    return Err(AxonError::UniqueViolation {
                        field: idx.field.clone(),
                        value: val.to_string(),
                    });
                }
            }
        }

        if had_old {
            for idx in indexes {
                self.delete_index_field_rows(key, &idx.field, entity_id)?;
            }
        }

        for idx in indexes {
            let byte_keys = crate::extract_index_key_bytes(new_data, &idx.field, &idx.index_type);
            self.insert_index_rows(key, &idx.field, entity_id, &byte_keys)?;
        }
        Ok(())
    }

    /// Remove all single-field index rows for an entity, on a resolved key.
    fn remove_single_indexes_at(
        &self,
        key: &QualifiedCollectionId,
        entity_id: &EntityId,
        indexes: &[axon_schema::schema::IndexDef],
    ) -> Result<(), AxonError> {
        for idx in indexes {
            self.delete_index_field_rows(key, &idx.field, entity_id)?;
        }
        Ok(())
    }

    /// Maintain compound index rows for a write, on a resolved key.
    ///
    /// Validate-then-mutate per ordinal: a unique compound key is checked before
    /// removing the old rows / inserting the new one. Atomicity is the caller's
    /// responsibility (owned/joined tx).
    fn maintain_compound_indexes_at(
        &self,
        key: &QualifiedCollectionId,
        entity_id: &EntityId,
        had_old: bool,
        new_data: &serde_json::Value,
        indexes: &[axon_schema::schema::CompoundIndexDef],
    ) -> Result<(), AxonError> {
        // Unique pre-check across all ordinals first.
        for (ordinal, idx) in indexes.iter().enumerate().filter(|(_, i)| i.unique) {
            let Ok(Some(framed)) = idx.index_key(new_data) else {
                continue;
            };
            let found: Option<i32> = self.block_on(
                sqlx::query_scalar(
                    "SELECT 1 FROM entity_compound_index
                     WHERE database_name = $1 AND schema_name = $2 AND collection = $3
                       AND index_ordinal = $4 AND key = $5 AND entity_id <> $6
                     LIMIT 1",
                )
                .bind(key.namespace.database.as_str())
                .bind(key.namespace.schema.as_str())
                .bind(key.collection.as_str())
                .bind(ordinal as i32)
                .bind(framed.as_slice())
                .bind(entity_id.as_str())
                .fetch_optional(&self.pool),
            )?;
            if found.is_some() {
                let field_names: Vec<&str> = idx.fields.iter().map(|f| f.field.as_str()).collect();
                let value = match crate::extract_compound_key(new_data, &idx.fields) {
                    Some(ckey) => format!("{ckey:?}"),
                    None => format!("{framed:?}"),
                };
                return Err(AxonError::UniqueViolation {
                    field: field_names.join(", "),
                    value,
                });
            }
        }

        if had_old {
            for (ordinal, _) in indexes.iter().enumerate() {
                self.delete_compound_index_rows(key, ordinal as i32, entity_id)?;
            }
        }

        for (ordinal, idx) in indexes.iter().enumerate() {
            let Ok(Some(framed)) = idx.index_key(new_data) else {
                continue;
            };
            self.insert_compound_index_row(key, ordinal as i32, entity_id, &framed)?;
        }
        Ok(())
    }

    /// Remove all compound index rows for an entity, on a resolved key.
    fn remove_compound_indexes_at(
        &self,
        key: &QualifiedCollectionId,
        entity_id: &EntityId,
        indexes: &[axon_schema::schema::CompoundIndexDef],
    ) -> Result<(), AxonError> {
        for (ordinal, _) in indexes.iter().enumerate() {
            self.delete_compound_index_rows(key, ordinal as i32, entity_id)?;
        }
        Ok(())
    }

    /// Maintain ALL secondary indexes (single + compound) for a write, looking
    /// up the entity's index defs from its stamped schema version.
    fn maintain_indexes_for_write_at(
        &self,
        key: &QualifiedCollectionId,
        single: &[axon_schema::schema::IndexDef],
        compound: &[axon_schema::schema::CompoundIndexDef],
        entity_id: &EntityId,
        had_old: bool,
        new_data: &serde_json::Value,
    ) -> Result<(), AxonError> {
        self.maintain_single_indexes_at(key, entity_id, had_old, new_data, single)?;
        self.maintain_compound_indexes_at(key, entity_id, had_old, new_data, compound)
    }

    /// Issue a `BEGIN` only if we don't already own / sit inside a transaction.
    /// Returns `true` when this call started its own transaction (and is
    /// therefore responsible for committing / rolling it back). Mirrors
    /// `SqliteStorageAdapter::begin_if_needed`.
    fn begin_if_needed(&mut self) -> Result<bool, AxonError> {
        if self.in_tx {
            return Ok(false);
        }
        self.block_on(sqlx::raw_sql("BEGIN").execute(&self.pool))?;
        self.in_tx = true;
        Ok(true)
    }

    /// Commit an owned transaction started by [`Self::begin_if_needed`].
    fn commit_owned(&mut self) -> Result<(), AxonError> {
        self.block_on(sqlx::raw_sql("COMMIT").execute(&self.pool))?;
        self.in_tx = false;
        Ok(())
    }

    /// Roll back an owned transaction started by [`Self::begin_if_needed`].
    /// Best-effort: clears `in_tx` even if the ROLLBACK itself errors.
    fn rollback_owned(&mut self) {
        let _ = self.block_on(sqlx::raw_sql("ROLLBACK").execute(&self.pool));
        self.in_tx = false;
    }

    /// Settle a write primitive's transaction given the ownership flag from
    /// [`Self::begin_if_needed`] and the primitive's body result.
    ///
    /// - `owned == true`: on `Ok` COMMIT, on `Err` ROLLBACK; either way the
    ///   primitive's own tx is fully resolved.
    /// - `owned == false` (joined the API multi-op transaction): NEVER touch the
    ///   tx — on `Ok` do nothing (the outer owner commits), on `Err` just return
    ///   it (the outer owner's `abort_tx` rolls everything back). A joined
    ///   primitive must never commit or roll back its parent's transaction.
    fn finish_owned_tx<T>(
        &mut self,
        owned: bool,
        result: Result<T, AxonError>,
    ) -> Result<T, AxonError> {
        match result {
            Ok(value) => {
                if owned {
                    self.commit_owned()?;
                }
                Ok(value)
            }
            Err(e) => {
                if owned {
                    self.rollback_owned();
                }
                Err(e)
            }
        }
    }

    /// Rebuild all `entity_index` rows for a collection from its entities.
    ///
    /// Drops the collection's existing index rows, then scans every entity
    /// (`range_scan`) and re-derives the byte keys for each declared index via
    /// [`crate::extract_index_key_bytes`]. Correct over an empty collection and
    /// idempotent (the leading delete clears any prior rows). Used both when a
    /// schema is (re)registered with indexes and for the open-time backfill of
    /// pre-existing entities. Mirrors `SqliteStorageAdapter::reindex_collection`.
    pub fn reindex_collection(
        &mut self,
        collection: &CollectionId,
        indexes: &[axon_schema::schema::IndexDef],
    ) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        self.block_on(
            sqlx::query(
                "DELETE FROM entity_index
                 WHERE database_name = $1 AND schema_name = $2 AND collection = $3",
            )
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .bind(key.collection.as_str())
            .execute(&self.pool),
        )?;
        if indexes.is_empty() {
            return Ok(());
        }
        let entities = self.range_scan(collection, None, None, None)?;
        for entity in &entities {
            for idx in indexes {
                let byte_keys =
                    crate::extract_index_key_bytes(&entity.data, &idx.field, &idx.index_type);
                self.insert_index_rows(&key, &idx.field, &entity.id, &byte_keys)?;
            }
        }
        Ok(())
    }

    /// Rebuild all `entity_compound_index` rows for a collection from its
    /// entities (the compound sibling of [`Self::reindex_collection`]).
    ///
    /// Clears the collection's existing compound rows, then for each entity
    /// computes the framed composite key for each compound def (by ordinal) via
    /// [`axon_schema::schema::CompoundIndexDef::index_key`], inserting one row per
    /// entity whose key is non-sparse. A type-mismatch is treated as not-indexed.
    /// Mirrors `SqliteStorageAdapter::reindex_compound_collection`.
    pub fn reindex_compound_collection(
        &mut self,
        collection: &CollectionId,
        indexes: &[axon_schema::schema::CompoundIndexDef],
    ) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        self.block_on(
            sqlx::query(
                "DELETE FROM entity_compound_index
                 WHERE database_name = $1 AND schema_name = $2 AND collection = $3",
            )
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .bind(key.collection.as_str())
            .execute(&self.pool),
        )?;
        if indexes.is_empty() {
            return Ok(());
        }
        let entities = self.range_scan(collection, None, None, None)?;
        for entity in &entities {
            for (ordinal, idx) in indexes.iter().enumerate() {
                if let Ok(Some(framed)) = idx.index_key(&entity.data) {
                    self.insert_compound_index_row(&key, ordinal as i32, &entity.id, &framed)?;
                }
            }
        }
        Ok(())
    }

    /// One-time open-time backfill: for every collection whose latest schema
    /// declares single-field and/or compound indexes but which has no index rows
    /// yet, rebuild its index. Independently guarded per index kind so it never
    /// redoes work for collections that already have rows. Correct (and a no-op)
    /// over an empty database. Mirrors
    /// `SqliteStorageAdapter::backfill_indexes_on_open`.
    fn backfill_indexes_on_open(&mut self) -> Result<(), AxonError> {
        let collections = self.list_collections()?;
        for collection in collections {
            let Some(schema) = self.get_schema(&collection)? else {
                continue;
            };
            let key = self.resolve_catalog_key(&collection)?;

            if !schema.indexes.is_empty() {
                let existing: i64 = self.block_on(
                    sqlx::query_scalar(
                        "SELECT COUNT(*) FROM entity_index
                         WHERE database_name = $1 AND schema_name = $2 AND collection = $3",
                    )
                    .bind(key.namespace.database.as_str())
                    .bind(key.namespace.schema.as_str())
                    .bind(key.collection.as_str())
                    .fetch_one(&self.pool),
                )?;
                if existing == 0 {
                    let indexes = schema.indexes.clone();
                    self.reindex_collection(&collection, &indexes)?;
                }
            }

            if !schema.compound_indexes.is_empty() {
                let existing: i64 = self.block_on(
                    sqlx::query_scalar(
                        "SELECT COUNT(*) FROM entity_compound_index
                         WHERE database_name = $1 AND schema_name = $2 AND collection = $3",
                    )
                    .bind(key.namespace.database.as_str())
                    .bind(key.namespace.schema.as_str())
                    .bind(key.collection.as_str())
                    .fetch_one(&self.pool),
                )?;
                if existing == 0 {
                    let compound_indexes = schema.compound_indexes.clone();
                    self.reindex_compound_collection(&collection, &compound_indexes)?;
                }
            }
        }
        Ok(())
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
        let row = self.block_on(
            sqlx::query(
                "SELECT collection, id, version, data
                 FROM entities
                 WHERE collection = $1 AND database_name = $2 AND schema_name = $3 AND id = $4",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .bind(id.as_str())
            .fetch_optional(&self.pool),
        )?;

        match row {
            Some(ref r) => Ok(Some(Self::row_to_entity(r)?)),
            None => Ok(None),
        }
    }

    fn put(&mut self, entity: Entity) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(&entity.collection)?;
        let data_json = serde_json::to_value(&entity.data)?;

        // Resolve the entity's index defs up front. With no indexes this is a
        // single entity-write statement, as before — no per-write tx overhead
        // and schemaless writes are unaffected.
        let (single, compound) =
            self.index_defs_for_entity(&entity.collection, entity.schema_version)?;
        if single.is_empty() && compound.is_empty() {
            self.block_on(
                sqlx::query(
                    "INSERT INTO entities (collection, database_name, schema_name, id, version, data)
                     VALUES ($1, $2, $3, $4, $5, $6)
                     ON CONFLICT (database_name, schema_name, collection, id)
                     DO UPDATE SET version = $5, data = $6",
                )
                .bind(key.collection.as_str())
                .bind(key.namespace.database.as_str())
                .bind(key.namespace.schema.as_str())
                .bind(entity.id.as_str())
                .bind(entity.version as i64)
                .bind(data_json)
                .execute(&self.pool),
            )?;
            return Ok(());
        }

        // Indexed write: the entity row + index DELETE/INSERTs + unique check
        // must be atomic. Begin our own tx only if not already inside one (the
        // API multi-op transaction's `begin_tx`); a joined primitive never
        // commits or rolls back its parent's tx.
        let owned = self.begin_if_needed()?;
        let result = (|| {
            // No prior-entity read needed: index maintenance deletes by
            // entity_id, so `had_old = true` simply means "clear any prior rows
            // first" (a no-op for a fresh id). Pass true so a replace's stale
            // rows are removed regardless of whether the id pre-existed.
            self.maintain_indexes_for_write_at(
                &key,
                &single,
                &compound,
                &entity.id,
                true,
                &entity.data,
            )?;
            self.block_on(
                sqlx::query(
                    "INSERT INTO entities (collection, database_name, schema_name, id, version, data)
                     VALUES ($1, $2, $3, $4, $5, $6)
                     ON CONFLICT (database_name, schema_name, collection, id)
                     DO UPDATE SET version = $5, data = $6",
                )
                .bind(key.collection.as_str())
                .bind(key.namespace.database.as_str())
                .bind(key.namespace.schema.as_str())
                .bind(entity.id.as_str())
                .bind(entity.version as i64)
                .bind(data_json)
                .execute(&self.pool),
            )?;
            Ok(())
        })();

        self.finish_owned_tx(owned, result)
    }

    fn delete(&mut self, collection: &CollectionId, id: &EntityId) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(collection)?;

        // Resolve index defs for this collection (latest schema) so we know
        // whether maintenance is needed. Delete removes index rows by
        // entity_id, so the entity's stamped schema_version is not required.
        let (single, compound) = self.index_defs_for_entity(collection, None)?;
        if single.is_empty() && compound.is_empty() {
            self.block_on(
                sqlx::query(
                    "DELETE FROM entities
                     WHERE collection = $1 AND database_name = $2 AND schema_name = $3 AND id = $4",
                )
                .bind(key.collection.as_str())
                .bind(key.namespace.database.as_str())
                .bind(key.namespace.schema.as_str())
                .bind(id.as_str())
                .execute(&self.pool),
            )?;
            return Ok(());
        }

        let owned = self.begin_if_needed()?;
        let result = (|| {
            self.remove_single_indexes_at(&key, id, &single)?;
            self.remove_compound_indexes_at(&key, id, &compound)?;
            self.block_on(
                sqlx::query(
                    "DELETE FROM entities
                     WHERE collection = $1 AND database_name = $2 AND schema_name = $3 AND id = $4",
                )
                .bind(key.collection.as_str())
                .bind(key.namespace.database.as_str())
                .bind(key.namespace.schema.as_str())
                .bind(id.as_str())
                .execute(&self.pool),
            )?;
            Ok(())
        })();

        self.finish_owned_tx(owned, result)
    }

    fn count(&self, collection: &CollectionId) -> Result<usize, AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        let count: i64 = self.block_on(
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM entities
                 WHERE collection = $1 AND database_name = $2 AND schema_name = $3",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .fetch_one(&self.pool),
        )?;
        Ok(count as usize)
    }

    /// Native membership signature (ADR-026 phantom guard) pushed fully into the
    /// database: an `md5` over the ordered id-set, transferred as 32 hex chars
    /// regardless of collection size. Read-only (no write-path cost);
    /// membership-only — changes on create/delete, stable across updates and
    /// reads. Runs in the active transaction during commit validation, so it
    /// sees the correct snapshot.
    fn structural_version(&self, collection: &CollectionId) -> Result<u64, AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        let hex: String = self.block_on(
            sqlx::query_scalar(
                "SELECT md5(coalesce(string_agg(id, ',' ORDER BY id), ''))
                 FROM entities
                 WHERE collection = $1 AND database_name = $2 AND schema_name = $3",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .fetch_one(&self.pool),
        )?;
        // Fold the 128-bit md5 into u64 by taking its first 64 bits (16 hex chars).
        Ok(u64::from_str_radix(hex.get(..16).unwrap_or("0"), 16).unwrap_or(0))
    }

    /// Native content signature (ADR-026 strict guard, FEAT-008 TXN-05
    /// `SerializableStrict`) pushed fully into the database: an `md5` over the
    /// ordered `id:version` pairs, transferred as 32 hex chars regardless of
    /// collection size. Like [`Self::structural_version`] this is read-only and
    /// pushes the aggregation into Postgres, but it is **version-inclusive**: an
    /// in-place update bumps a row's `version` and moves the signature, catching
    /// update-driven predicate skew the membership signature misses. Runs in the
    /// active transaction during commit validation, so it sees the correct
    /// snapshot.
    fn content_version(&self, collection: &CollectionId) -> Result<u64, AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        let hex: String = self.block_on(
            sqlx::query_scalar(
                "SELECT md5(coalesce(
                     string_agg(id || ':' || version::text, ',' ORDER BY id), ''))
                 FROM entities
                 WHERE collection = $1 AND database_name = $2 AND schema_name = $3",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .fetch_one(&self.pool),
        )?;
        // Fold the 128-bit md5 into u64 by taking its first 64 bits (16 hex chars).
        Ok(u64::from_str_radix(hex.get(..16).unwrap_or("0"), 16).unwrap_or(0))
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

        let rows = self.block_on(
            sqlx::query(
                "SELECT collection, id, version, data FROM entities
                 WHERE collection = $1
                   AND database_name = $2
                   AND schema_name = $3
                   AND ($4::text IS NULL OR id >= $4)
                   AND ($5::text IS NULL OR id <= $5)
                 ORDER BY id ASC
                 LIMIT $6",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .bind(start_str.as_deref())
            .bind(end_str.as_deref())
            .bind(limit_val)
            .fetch_all(&self.pool),
        )?;

        rows.iter().map(Self::row_to_entity).collect()
    }

    fn compare_and_swap(
        &mut self,
        entity: Entity,
        expected_version: u64,
    ) -> Result<Entity, AxonError> {
        let key = self.resolve_catalog_key(&entity.collection)?;
        let new_version = expected_version + 1;
        let data_json = serde_json::to_value(&entity.data)?;

        let (single, compound) =
            self.index_defs_for_entity(&entity.collection, entity.schema_version)?;
        let has_indexes = !(single.is_empty() && compound.is_empty());

        // Begin our own tx only when index maintenance must be atomic with the
        // entity write AND we are not already joined to an outer API tx. With no
        // indexes the version-guarded UPDATE is already atomic on its own.
        let owned = if has_indexes {
            self.begin_if_needed()?
        } else {
            false
        };

        let result = (|| {
            // Version check (inside the tx when owned/joined, so the read+write
            // sees a consistent snapshot under the write lock).
            let current = self.get(&entity.collection, &entity.id)?;
            let actual_version = current.as_ref().map(|e| e.version).unwrap_or(0);
            if actual_version != expected_version {
                return Err(AxonError::ConflictingVersion {
                    expected: expected_version,
                    actual: actual_version,
                    current_entity: current.map(Box::new),
                });
            }

            // Maintain indexes (and unique-check) before the UPDATE so a unique
            // violation aborts the swap with the entity unwritten. `had_old` is
            // true: cas always replaces an existing row.
            if has_indexes {
                self.maintain_single_indexes_at(&key, &entity.id, true, &entity.data, &single)?;
                self.maintain_compound_indexes_at(&key, &entity.id, true, &entity.data, &compound)?;
            }

            let res = self.block_on(
                sqlx::query(
                    "UPDATE entities SET version = $3, data = $4
                     WHERE collection = $1 AND database_name = $5 AND schema_name = $6 AND id = $2 AND version = $7",
                )
                .bind(key.collection.as_str())
                .bind(entity.id.as_str())
                .bind(new_version as i64)
                .bind(data_json)
                .bind(key.namespace.database.as_str())
                .bind(key.namespace.schema.as_str())
                .bind(expected_version as i64)
                .execute(&self.pool),
            )?;

            if res.rows_affected() == 0 {
                let current_after_race = self.get(&entity.collection, &entity.id)?;
                let actual = current_after_race.as_ref().map(|e| e.version).unwrap_or(0);
                return Err(AxonError::ConflictingVersion {
                    expected: expected_version,
                    actual,
                    current_entity: current_after_race.map(Box::new),
                });
            }

            Ok(Entity {
                collection: key.collection.clone(),
                version: new_version,
                ..entity
            })
        })();

        self.finish_owned_tx(owned, result)
    }

    fn create_if_absent(
        &mut self,
        entity: Entity,
        expected_absent_version: u64,
    ) -> Result<Entity, AxonError> {
        let key = self.resolve_catalog_key(&entity.collection)?;
        let data_json = serde_json::to_value(&entity.data)?;

        let (single, compound) =
            self.index_defs_for_entity(&entity.collection, entity.schema_version)?;
        let has_indexes = !(single.is_empty() && compound.is_empty());

        let owned = if has_indexes {
            self.begin_if_needed()?
        } else {
            false
        };

        let result = (|| {
            let res = self.block_on(
                sqlx::query(
                    "INSERT INTO entities (collection, database_name, schema_name, id, version, data)
                     VALUES ($1, $2, $3, $4, $5, $6)
                     ON CONFLICT (database_name, schema_name, collection, id) DO NOTHING",
                )
                .bind(key.collection.as_str())
                .bind(key.namespace.database.as_str())
                .bind(key.namespace.schema.as_str())
                .bind(entity.id.as_str())
                .bind(entity.version as i64)
                .bind(data_json)
                .execute(&self.pool),
            )?;

            if res.rows_affected() == 0 {
                // No-op: the entity already existed → maintain nothing.
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

            // Fresh insert → maintain indexes. `had_old = false` (no prior row).
            if has_indexes {
                self.maintain_single_indexes_at(&key, &entity.id, false, &entity.data, &single)?;
                self.maintain_compound_indexes_at(
                    &key,
                    &entity.id,
                    false,
                    &entity.data,
                    &compound,
                )?;
            }

            Ok(Entity {
                collection: key.collection.clone(),
                ..entity
            })
        })();

        self.finish_owned_tx(owned, result)
    }

    fn begin_tx(&mut self) -> Result<(), AxonError> {
        if self.in_tx {
            return Err(AxonError::Storage("transaction already active".into()));
        }
        self.block_on(sqlx::raw_sql("BEGIN").execute(&self.pool))?;
        self.in_tx = true;
        Ok(())
    }

    fn commit_tx(&mut self) -> Result<(), AxonError> {
        if !self.in_tx {
            return Err(AxonError::Storage("no active transaction".into()));
        }
        self.block_on(sqlx::raw_sql("COMMIT").execute(&self.pool))?;
        self.in_tx = false;
        Ok(())
    }

    fn abort_tx(&mut self) -> Result<(), AxonError> {
        if !self.in_tx {
            return Ok(());
        }
        self.block_on(sqlx::raw_sql("ROLLBACK").execute(&self.pool))?;
        self.in_tx = false;
        Ok(())
    }

    fn create_mutation_intent(&mut self, intent: &MutationIntent) -> Result<(), AxonError> {
        let intent_json = serde_json::to_value(intent)?;
        let result = self.block_on(
            sqlx::query(
                "INSERT INTO mutation_intents
                    (tenant_id, database_id, intent_id, decision, approval_state, expires_at_ns, intent_json)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)
                 ON CONFLICT DO NOTHING",
            )
            .bind(intent.scope.tenant_id.as_str())
            .bind(intent.scope.database_id.as_str())
            .bind(intent.intent_id.as_str())
            .bind(intent.decision.as_str())
            .bind(intent.approval_state.as_str())
            .bind(intent.expires_at as i64)
            .bind(intent_json)
            .execute(&self.pool),
        )?;
        if result.rows_affected() == 0 {
            return Err(AxonError::AlreadyExists(format!(
                "mutation intent '{}' already exists in tenant '{}' database '{}'",
                intent.intent_id, intent.scope.tenant_id, intent.scope.database_id
            )));
        }
        Ok(())
    }

    fn get_mutation_intent(
        &self,
        tenant_id: &str,
        database_id: &str,
        intent_id: &str,
    ) -> Result<Option<MutationIntent>, AxonError> {
        let row = self.block_on(
            sqlx::query(
                "SELECT intent_json FROM mutation_intents
                 WHERE tenant_id = $1 AND database_id = $2 AND intent_id = $3",
            )
            .bind(tenant_id)
            .bind(database_id)
            .bind(intent_id)
            .fetch_optional(&self.pool),
        )?;
        row.as_ref().map(Self::row_to_mutation_intent).transpose()
    }

    fn list_pending_mutation_intents(
        &self,
        tenant_id: &str,
        database_id: &str,
        now_ns: u64,
        limit: Option<usize>,
    ) -> Result<Vec<MutationIntent>, AxonError> {
        let limit = limit.unwrap_or(i64::MAX as usize) as i64;
        let rows = self.block_on(
            sqlx::query(
                "SELECT intent_json FROM mutation_intents
                 WHERE tenant_id = $1
                   AND database_id = $2
                   AND approval_state = $3
                   AND expires_at_ns > $4
                 ORDER BY expires_at_ns ASC, intent_id ASC
                 LIMIT $5",
            )
            .bind(tenant_id)
            .bind(database_id)
            .bind(ApprovalState::Pending.as_str())
            .bind(now_ns as i64)
            .bind(limit)
            .fetch_all(&self.pool),
        )?;
        rows.iter().map(Self::row_to_mutation_intent).collect()
    }

    fn list_expired_mutation_intents(
        &self,
        tenant_id: &str,
        database_id: &str,
        now_ns: u64,
        limit: Option<usize>,
    ) -> Result<Vec<MutationIntent>, AxonError> {
        let limit = limit.unwrap_or(i64::MAX as usize) as i64;
        let rows = self.block_on(
            sqlx::query(
                "SELECT intent_json FROM mutation_intents
                 WHERE tenant_id = $1
                   AND database_id = $2
                   AND expires_at_ns <= $3
                   AND approval_state IN ($4, $5, $6)
                 ORDER BY expires_at_ns ASC, intent_id ASC
                 LIMIT $7",
            )
            .bind(tenant_id)
            .bind(database_id)
            .bind(now_ns as i64)
            .bind(ApprovalState::None.as_str())
            .bind(ApprovalState::Pending.as_str())
            .bind(ApprovalState::Approved.as_str())
            .bind(limit)
            .fetch_all(&self.pool),
        )?;
        rows.iter().map(Self::row_to_mutation_intent).collect()
    }

    fn list_mutation_intents_by_state(
        &self,
        tenant_id: &str,
        database_id: &str,
        approval_state: ApprovalState,
        limit: Option<usize>,
    ) -> Result<Vec<MutationIntent>, AxonError> {
        let limit = limit.unwrap_or(i64::MAX as usize) as i64;
        let rows = self.block_on(
            sqlx::query(
                "SELECT intent_json FROM mutation_intents
                 WHERE tenant_id = $1
                   AND database_id = $2
                   AND approval_state = $3
                 ORDER BY expires_at_ns ASC, intent_id ASC
                 LIMIT $4",
            )
            .bind(tenant_id)
            .bind(database_id)
            .bind(approval_state.as_str())
            .bind(limit)
            .fetch_all(&self.pool),
        )?;
        rows.iter().map(Self::row_to_mutation_intent).collect()
    }

    fn update_mutation_intent_state(
        &mut self,
        tenant_id: &str,
        database_id: &str,
        intent_id: &str,
        expected: ApprovalState,
        new_state: ApprovalState,
    ) -> Result<MutationIntent, AxonError> {
        let mut intent = self
            .get_mutation_intent(tenant_id, database_id, intent_id)?
            .ok_or_else(|| AxonError::NotFound(format!("mutation intent '{intent_id}'")))?;
        if intent.approval_state != expected {
            return Err(AxonError::InvalidOperation(format!(
                "mutation intent '{intent_id}' state is '{}', expected '{}'",
                intent.approval_state.as_str(),
                expected.as_str()
            )));
        }
        intent.approval_state = new_state;
        let intent_json = serde_json::to_value(&intent)?;
        let result = self.block_on(
            sqlx::query(
                "UPDATE mutation_intents
                 SET approval_state = $1, intent_json = $2
                 WHERE tenant_id = $3
                   AND database_id = $4
                   AND intent_id = $5
                   AND approval_state = $6",
            )
            .bind(intent.approval_state.as_str())
            .bind(intent_json)
            .bind(tenant_id)
            .bind(database_id)
            .bind(intent_id)
            .bind(expected.as_str())
            .execute(&self.pool),
        )?;
        if result.rows_affected() == 0 {
            return Err(AxonError::InvalidOperation(format!(
                "mutation intent '{intent_id}' state changed before transition"
            )));
        }
        Ok(intent)
    }

    fn create_database(&mut self, name: &str) -> Result<(), AxonError> {
        if self.database_exists(name)? {
            return Err(AxonError::AlreadyExists(format!("database '{name}'")));
        }

        self.block_on(
            sqlx::query("INSERT INTO databases (name) VALUES ($1)")
                .bind(name)
                .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query("INSERT INTO namespaces (database_name, name) VALUES ($1, $2)")
                .bind(name)
                .bind(DEFAULT_SCHEMA)
                .execute(&self.pool),
        )?;
        Ok(())
    }

    fn list_databases(&self) -> Result<Vec<String>, AxonError> {
        use sqlx::Row;
        let rows = self.block_on(
            sqlx::query("SELECT name FROM databases ORDER BY name ASC").fetch_all(&self.pool),
        )?;
        Ok(rows
            .iter()
            .map(|row| row.get::<String, _>("name"))
            .collect())
    }

    fn drop_database(&mut self, name: &str) -> Result<(), AxonError> {
        if !self.database_exists(name)? {
            return Err(AxonError::NotFound(format!("database '{name}'")));
        }

        let doomed = self.database_collection_keys(name)?;
        self.purge_links_for_collections(&doomed)?;
        self.block_on(
            sqlx::query("DELETE FROM entities WHERE database_name = $1")
                .bind(name)
                .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query("DELETE FROM collection_views WHERE database_name = $1")
                .bind(name)
                .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query("DELETE FROM schemas WHERE database_name = $1")
                .bind(name)
                .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query("DELETE FROM collections WHERE database_name = $1")
                .bind(name)
                .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query("DELETE FROM databases WHERE name = $1")
                .bind(name)
                .execute(&self.pool),
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

        self.block_on(
            sqlx::query("INSERT INTO namespaces (database_name, name) VALUES ($1, $2)")
                .bind(namespace.database.as_str())
                .bind(namespace.schema.as_str())
                .execute(&self.pool),
        )?;
        Ok(())
    }

    fn list_namespaces(&self, database: &str) -> Result<Vec<String>, AxonError> {
        use sqlx::Row;
        if !self.database_exists(database)? {
            return Err(AxonError::NotFound(format!("database '{database}'")));
        }

        let rows = self.block_on(
            sqlx::query(
                "SELECT name FROM namespaces
                 WHERE database_name = $1
                 ORDER BY name ASC",
            )
            .bind(database)
            .fetch_all(&self.pool),
        )?;
        Ok(rows
            .iter()
            .map(|row| row.get::<String, _>("name"))
            .collect())
    }

    fn drop_namespace(&mut self, namespace: &Namespace) -> Result<(), AxonError> {
        if !self.namespace_exists(namespace)? {
            return Err(AxonError::NotFound(format!("namespace '{namespace}'")));
        }

        let doomed = self.namespace_collection_keys(namespace)?;
        self.purge_links_for_collections(&doomed)?;
        self.block_on(
            sqlx::query(
                "DELETE FROM entities
                 WHERE database_name = $1 AND schema_name = $2",
            )
            .bind(namespace.database.as_str())
            .bind(namespace.schema.as_str())
            .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "DELETE FROM collection_views
                 WHERE database_name = $1 AND schema_name = $2",
            )
            .bind(namespace.database.as_str())
            .bind(namespace.schema.as_str())
            .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "DELETE FROM schemas
                 WHERE database_name = $1 AND schema_name = $2",
            )
            .bind(namespace.database.as_str())
            .bind(namespace.schema.as_str())
            .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "DELETE FROM collections
                 WHERE database_name = $1 AND schema_name = $2",
            )
            .bind(namespace.database.as_str())
            .bind(namespace.schema.as_str())
            .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "DELETE FROM namespaces
                 WHERE database_name = $1 AND name = $2",
            )
            .bind(namespace.database.as_str())
            .bind(namespace.schema.as_str())
            .execute(&self.pool),
        )?;
        Ok(())
    }

    fn list_namespace_collections(
        &self,
        namespace: &Namespace,
    ) -> Result<Vec<CollectionId>, AxonError> {
        use sqlx::Row;
        if !self.namespace_exists(namespace)? {
            return Err(AxonError::NotFound(format!("namespace '{namespace}'")));
        }

        let rows = self.block_on(
            sqlx::query(
                "SELECT name FROM collections
                 WHERE database_name = $1 AND schema_name = $2
                 ORDER BY name ASC",
            )
            .bind(namespace.database.as_str())
            .bind(namespace.schema.as_str())
            .fetch_all(&self.pool),
        )?;
        Ok(rows
            .iter()
            .map(|row| {
                let name: String = row.get("name");
                CollectionId::new(name)
            })
            .collect())
    }

    fn append_audit_entry(&mut self, mut entry: AuditEntry) -> Result<AuditEntry, AxonError> {
        use sqlx::Row;
        if entry.timestamp_ns == 0 {
            entry.timestamp_ns = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0);
        }

        let entry_json = serde_json::to_value(&entry)?;

        let row = self.block_on(
            sqlx::query(
                "INSERT INTO audit_log (timestamp_ns, collection, entity_id, version, mutation, actor, transaction_id, entry_json)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                 RETURNING id",
            )
            .bind(entry.timestamp_ns as i64)
            .bind(entry.collection.as_str())
            .bind(entry.entity_id.as_str())
            .bind(entry.version as i64)
            .bind(entry.mutation.to_string())
            .bind(entry.actor.as_str())
            .bind(entry.transaction_id.as_deref())
            .bind(entry_json)
            .fetch_one(&self.pool),
        )?;

        let id: i64 = row.get(0);
        entry.id = id as u64;

        Ok(entry)
    }

    fn supports_durable_audit(&self) -> bool {
        true
    }

    fn audit_len(&self) -> Result<usize, AxonError> {
        use sqlx::Row;

        let row =
            self.block_on(sqlx::query("SELECT COUNT(*) FROM audit_log").fetch_one(&self.pool))?;
        let count: i64 = row.get(0);
        Ok(count as usize)
    }

    fn query_audit_paginated(&self, query: AuditQuery) -> Result<AuditPage, AxonError> {
        use sqlx::Row;

        let after_id = query.after_id.unwrap_or(0) as i64;
        let rows = self.block_on(
            sqlx::query(
                "SELECT id, timestamp_ns, entry_json
                 FROM audit_log
                 WHERE id > $1
                 ORDER BY id ASC",
            )
            .bind(after_id)
            .fetch_all(&self.pool),
        )?;

        let mut entries = Vec::with_capacity(rows.len());
        for row in rows {
            let id = row.get::<i64, _>("id") as u64;
            let timestamp_ns = row.get::<i64, _>("timestamp_ns") as u64;
            let entry_json = row.get::<serde_json::Value, _>("entry_json");
            let mut entry: AuditEntry = serde_json::from_value(entry_json)?;
            entry.id = id;
            entry.timestamp_ns = timestamp_ns;
            entries.push(entry);
        }

        Ok(filter_audit_entries_for_query(entries, query))
    }

    fn find_audit_by_id(&self, id: u64) -> Result<Option<AuditEntry>, AxonError> {
        use sqlx::Row;

        let row = self.block_on(
            sqlx::query(
                "SELECT id, timestamp_ns, entry_json
                 FROM audit_log
                 WHERE id = $1",
            )
            .bind(id as i64)
            .fetch_optional(&self.pool),
        )?;
        row.map(|row| {
            let row_id = row.get::<i64, _>("id") as u64;
            let timestamp_ns = row.get::<i64, _>("timestamp_ns") as u64;
            let entry_json = row.get::<serde_json::Value, _>("entry_json");
            let mut entry: AuditEntry = serde_json::from_value(entry_json)?;
            entry.id = row_id;
            entry.timestamp_ns = timestamp_ns;
            Ok(entry)
        })
        .transpose()
    }

    // ── Persisted secondary index operations (FEAT-013) ─────────────────
    //
    // Direct mirrors of the SQLite trait-method implementations over Postgres
    // types. `BYTEA` is compared bytewise by Postgres, so range and prefix
    // scans over `key` honour the canonical order-preserving encoding without
    // any text-collation involvement — identical results to SQLite and memory.

    fn index_lookup(
        &self,
        collection: &CollectionId,
        field: &str,
        value: &IndexValue,
    ) -> Result<Vec<EntityId>, AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        // An unencodable lookup value cannot match any stored key.
        let Ok(bytes) = value.encode_key() else {
            return Ok(vec![]);
        };
        let ids: Vec<String> = self.block_on(
            sqlx::query_scalar(
                "SELECT entity_id FROM entity_index
                 WHERE database_name = $1 AND schema_name = $2 AND collection = $3
                   AND field = $4 AND key = $5
                 ORDER BY entity_id ASC",
            )
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .bind(key.collection.as_str())
            .bind(field)
            .bind(bytes.as_slice())
            .fetch_all(&self.pool),
        )?;
        Ok(ids.into_iter().map(EntityId::new).collect())
    }

    fn index_range(
        &self,
        collection: &CollectionId,
        field: &str,
        lower: Bound<&IndexValue>,
        upper: Bound<&IndexValue>,
    ) -> Result<Vec<EntityId>, AxonError> {
        use sqlx::Row;
        let key = self.resolve_catalog_key(collection)?;

        // Build the bound clauses dynamically, omitting unbounded sides. BYTEA
        // comparison in Postgres is bytewise, matching the canonical
        // order-preserving key encoding. Placeholders are assigned in bind order.
        let mut sql = String::from(
            "SELECT entity_id, key FROM entity_index
             WHERE database_name = $1 AND schema_name = $2 AND collection = $3 AND field = $4",
        );
        // An unencodable bound value carries no usable ordering, so we treat that
        // side as unbounded (`None`) rather than erroring.
        let lower_bytes: Option<Vec<u8>> = match lower {
            Bound::Included(v) | Bound::Excluded(v) => v.encode_key().ok(),
            Bound::Unbounded => None,
        };
        let upper_bytes: Option<Vec<u8>> = match upper {
            Bound::Included(v) | Bound::Excluded(v) => v.encode_key().ok(),
            Bound::Unbounded => None,
        };
        if lower_bytes.is_some() {
            match lower {
                Bound::Excluded(_) => sql.push_str(" AND key > $5"),
                _ => sql.push_str(" AND key >= $5"),
            }
        }
        if upper_bytes.is_some() {
            // Upper placeholder index depends on whether lower was bound.
            let ph = if lower_bytes.is_some() { "$6" } else { "$5" };
            match upper {
                Bound::Excluded(_) => {
                    sql.push_str(" AND key < ");
                    sql.push_str(ph);
                }
                _ => {
                    sql.push_str(" AND key <= ");
                    sql.push_str(ph);
                }
            }
        }
        sql.push_str(" ORDER BY key ASC, entity_id ASC");

        let mut query = sqlx::query(&sql)
            .bind(key.namespace.database.as_str().to_owned())
            .bind(key.namespace.schema.as_str().to_owned())
            .bind(key.collection.as_str().to_owned())
            .bind(field.to_owned());
        if let Some(lb) = lower_bytes {
            query = query.bind(lb);
        }
        if let Some(ub) = upper_bytes {
            query = query.bind(ub);
        }
        let rows = self.block_on(query.fetch_all(&self.pool))?;
        let ids = rows
            .iter()
            .map(|row| {
                let id: String = row.get("entity_id");
                EntityId::new(id)
            })
            .collect();
        Ok(ids)
    }

    fn index_unique_conflict(
        &self,
        collection: &CollectionId,
        field: &str,
        value: &IndexValue,
        exclude_entity: &EntityId,
    ) -> Result<bool, AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        let Ok(bytes) = value.encode_key() else {
            return Ok(false);
        };
        let found: Option<i32> = self.block_on(
            sqlx::query_scalar(
                "SELECT 1 FROM entity_index
                 WHERE database_name = $1 AND schema_name = $2 AND collection = $3
                   AND field = $4 AND key = $5 AND entity_id <> $6
                 LIMIT 1",
            )
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .bind(key.collection.as_str())
            .bind(field)
            .bind(bytes.as_slice())
            .bind(exclude_entity.as_str())
            .fetch_optional(&self.pool),
        )?;
        Ok(found.is_some())
    }

    fn drop_indexes(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        self.block_on(
            sqlx::query(
                "DELETE FROM entity_index
                 WHERE database_name = $1 AND schema_name = $2 AND collection = $3",
            )
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .bind(key.collection.as_str())
            .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "DELETE FROM entity_compound_index
                 WHERE database_name = $1 AND schema_name = $2 AND collection = $3",
            )
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .bind(key.collection.as_str())
            .execute(&self.pool),
        )?;
        Ok(())
    }

    // ── Compound index operations (FEAT-013, US-033) ────────────────────

    fn compound_index_lookup(
        &self,
        collection: &CollectionId,
        index_idx: usize,
        key: &CompoundKey,
    ) -> Result<Vec<EntityId>, AxonError> {
        let cat = self.resolve_catalog_key(collection)?;
        let Ok(Some(framed)) = key.encode_framed() else {
            return Ok(vec![]);
        };
        let ids: Vec<String> = self.block_on(
            sqlx::query_scalar(
                "SELECT entity_id FROM entity_compound_index
                 WHERE database_name = $1 AND schema_name = $2 AND collection = $3
                   AND index_ordinal = $4 AND key = $5
                 ORDER BY entity_id ASC",
            )
            .bind(cat.namespace.database.as_str())
            .bind(cat.namespace.schema.as_str())
            .bind(cat.collection.as_str())
            .bind(index_idx as i32)
            .bind(framed.as_slice())
            .fetch_all(&self.pool),
        )?;
        Ok(ids.into_iter().map(EntityId::new).collect())
    }

    fn compound_index_prefix(
        &self,
        collection: &CollectionId,
        index_idx: usize,
        prefix: &CompoundKey,
    ) -> Result<Vec<EntityId>, AxonError> {
        use sqlx::Row;
        let cat = self.resolve_catalog_key(collection)?;
        let Ok(Some(framed)) = prefix.encode_framed() else {
            return Ok(vec![]);
        };
        // Byte-prefix range: key >= P AND key < successor(P). When there is no
        // finite successor (P empty / all 0xFF), omit the upper bound and scan
        // to the end of the ordinal partition.
        let upper = prefix_successor(&framed);
        let mut sql = String::from(
            "SELECT entity_id, key FROM entity_compound_index
             WHERE database_name = $1 AND schema_name = $2 AND collection = $3
               AND index_ordinal = $4 AND key >= $5",
        );
        if upper.is_some() {
            sql.push_str(" AND key < $6");
        }
        sql.push_str(" ORDER BY key ASC, entity_id ASC");

        let mut query = sqlx::query(&sql)
            .bind(cat.namespace.database.as_str().to_owned())
            .bind(cat.namespace.schema.as_str().to_owned())
            .bind(cat.collection.as_str().to_owned())
            .bind(index_idx as i32)
            .bind(framed.clone());
        if let Some(ub) = upper {
            query = query.bind(ub);
        }
        let rows = self.block_on(query.fetch_all(&self.pool))?;
        let ids = rows
            .iter()
            .map(|row| {
                let id: String = row.get("entity_id");
                EntityId::new(id)
            })
            .collect();
        Ok(ids)
    }

    fn put_schema(&mut self, schema: &CollectionSchema) -> Result<(), AxonError> {
        use sqlx::Row;
        let key = self.resolve_catalog_key(&schema.collection)?;
        let row = self.block_on(
            sqlx::query(
                "SELECT COALESCE(MAX(version), 0) FROM schemas
                 WHERE collection = $1 AND database_name = $2 AND schema_name = $3",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .fetch_one(&self.pool),
        )?;
        let next_version = row.get::<i32, _>(0) + 1;
        let created_at_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as i64;

        let mut versioned = schema.clone();
        versioned.collection = key.collection.clone();
        versioned.version = next_version as u32;
        let schema_json = serde_json::to_value(&versioned)?;
        self.block_on(
            sqlx::query(
                "INSERT INTO schemas
                    (collection, database_name, schema_name, version, schema_json, created_at_ns)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .bind(next_version)
            .bind(schema_json)
            .bind(created_at_ns)
            .execute(&self.pool),
        )?;
        // Rebuild persisted secondary indexes from the now-current schema so a
        // newly declared (or changed) index covers entities that predate it
        // (FEAT-013 backfill). `reindex_collection` clears stale rows first, so
        // dropping all indexes from a schema correctly empties the index too.
        let indexes = versioned.indexes.clone();
        self.reindex_collection(&schema.collection, &indexes)?;
        let compound_indexes = versioned.compound_indexes.clone();
        self.reindex_compound_collection(&schema.collection, &compound_indexes)?;
        Ok(())
    }

    fn get_schema(&self, collection: &CollectionId) -> Result<Option<CollectionSchema>, AxonError> {
        use sqlx::Row;
        let key = self.resolve_catalog_key(collection)?;
        let row = self.block_on(
            sqlx::query(
                "SELECT schema_json FROM schemas
                 WHERE collection = $1 AND database_name = $2 AND schema_name = $3
                 ORDER BY version DESC
                 LIMIT 1",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .fetch_optional(&self.pool),
        )?;

        match row {
            Some(r) => {
                let schema_json: serde_json::Value = r.get("schema_json");
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
        use sqlx::Row;
        let key = self.resolve_catalog_key(collection)?;
        let row = self.block_on(
            sqlx::query(
                "SELECT schema_json FROM schemas
                 WHERE collection = $1 AND database_name = $2 AND schema_name = $3 AND version = $4",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .bind(version as i32)
            .fetch_optional(&self.pool),
        )?;

        match row {
            Some(r) => {
                let schema_json: serde_json::Value = r.get("schema_json");
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
        use sqlx::Row;
        let key = self.resolve_catalog_key(collection)?;
        let rows = self.block_on(
            sqlx::query(
                "SELECT version, created_at_ns FROM schemas
                 WHERE collection = $1 AND database_name = $2 AND schema_name = $3
                 ORDER BY version ASC",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .fetch_all(&self.pool),
        )?;

        Ok(rows
            .into_iter()
            .map(|row| {
                (
                    row.get::<i32, _>("version") as u32,
                    row.get::<i64, _>("created_at_ns") as u64,
                )
            })
            .collect())
    }

    fn delete_schema(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        self.block_on(
            sqlx::query(
                "DELETE FROM schemas
                 WHERE collection = $1 AND database_name = $2 AND schema_name = $3",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .execute(&self.pool),
        )?;
        Ok(())
    }

    fn put_collection_view(&mut self, view: &CollectionView) -> Result<CollectionView, AxonError> {
        use sqlx::Row;
        let key = self.resolve_catalog_key(&view.collection)?;
        if !self.collection_exists_in_namespace(&key.collection, &key.namespace)? {
            return Err(AxonError::InvalidArgument(format!(
                "collection '{}' is not registered",
                view.collection.as_str()
            )));
        }

        let current_version = self
            .block_on(
                sqlx::query(
                    "SELECT version FROM collection_views
                     WHERE collection = $1 AND database_name = $2 AND schema_name = $3",
                )
                .bind(key.collection.as_str())
                .bind(key.namespace.database.as_str())
                .bind(key.namespace.schema.as_str())
                .fetch_optional(&self.pool),
            )?
            .map_or(0, |row| row.get::<i32, _>("version"));
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

        self.block_on(
            sqlx::query(
                "INSERT INTO collection_views
                    (collection, database_name, schema_name, version, view_json, updated_at_ns, updated_by)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)
                 ON CONFLICT (database_name, schema_name, collection) DO UPDATE SET
                     version = EXCLUDED.version,
                     view_json = EXCLUDED.view_json,
                     updated_at_ns = EXCLUDED.updated_at_ns,
                     updated_by = EXCLUDED.updated_by",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .bind(next_version)
            .bind(view_json)
            .bind(updated_at_ns)
            .bind(versioned.updated_by.as_deref())
            .execute(&self.pool),
        )?;
        Ok(versioned)
    }

    fn get_collection_view(
        &self,
        collection: &CollectionId,
    ) -> Result<Option<CollectionView>, AxonError> {
        use sqlx::Row;
        let key = self.resolve_catalog_key(collection)?;
        let row = self.block_on(
            sqlx::query(
                "SELECT view_json FROM collection_views
                 WHERE collection = $1 AND database_name = $2 AND schema_name = $3",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .fetch_optional(&self.pool),
        )?;

        match row {
            Some(r) => {
                let view_json: serde_json::Value = r.get("view_json");
                let view: CollectionView = serde_json::from_value(view_json)?;
                Ok(Some(view))
            }
            None => Ok(None),
        }
    }

    fn delete_collection_view(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        self.block_on(
            sqlx::query(
                "DELETE FROM collection_views
                 WHERE collection = $1 AND database_name = $2 AND schema_name = $3",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .execute(&self.pool),
        )?;
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

        self.block_on(
            sqlx::query(
                "INSERT INTO collections (name, database_name, schema_name)
                 VALUES ($1, $2, $3)
                 ON CONFLICT DO NOTHING",
            )
            .bind(collection.as_str())
            .bind(namespace.database.as_str())
            .bind(namespace.schema.as_str())
            .execute(&self.pool),
        )?;
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
        self.block_on(
            sqlx::query(
                "DELETE FROM collection_views
                 WHERE collection = $1 AND database_name = $2 AND schema_name = $3",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .execute(&self.pool),
        )?;
        if raw_collection != key.collection.as_str() {
            self.block_on(
                sqlx::query(
                    "DELETE FROM collection_views
                     WHERE collection = $1
                       AND ((database_name = $2 AND schema_name = $3)
                            OR (database_name = $4 AND schema_name = $5))",
                )
                .bind(raw_collection)
                .bind(key.namespace.database.as_str())
                .bind(key.namespace.schema.as_str())
                .bind(default_namespace.database.as_str())
                .bind(default_namespace.schema.as_str())
                .execute(&self.pool),
            )?;
        }
        self.block_on(
            sqlx::query(
                "DELETE FROM schemas
                 WHERE collection = $1 AND database_name = $2 AND schema_name = $3",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .execute(&self.pool),
        )?;
        if raw_collection != key.collection.as_str() {
            self.block_on(
                sqlx::query(
                    "DELETE FROM schemas
                     WHERE collection = $1
                       AND ((database_name = $2 AND schema_name = $3)
                            OR (database_name = $4 AND schema_name = $5))",
                )
                .bind(raw_collection)
                .bind(key.namespace.database.as_str())
                .bind(key.namespace.schema.as_str())
                .bind(default_namespace.database.as_str())
                .bind(default_namespace.schema.as_str())
                .execute(&self.pool),
            )?;
        }
        self.block_on(
            sqlx::query(
                "DELETE FROM collections
                 WHERE name = $1 AND database_name = $2 AND schema_name = $3",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .execute(&self.pool),
        )?;
        if raw_collection != key.collection.as_str() {
            self.block_on(
                sqlx::query(
                    "DELETE FROM collections
                     WHERE name = $1
                       AND ((database_name = $2 AND schema_name = $3)
                            OR (database_name = $4 AND schema_name = $5))",
                )
                .bind(raw_collection)
                .bind(key.namespace.database.as_str())
                .bind(key.namespace.schema.as_str())
                .bind(default_namespace.database.as_str())
                .bind(default_namespace.schema.as_str())
                .execute(&self.pool),
            )?;
        }
        Ok(())
    }

    fn list_collections(&self) -> Result<Vec<CollectionId>, AxonError> {
        use sqlx::Row;
        let rows = self.block_on(
            sqlx::query(
                "SELECT name FROM collections
                 ORDER BY database_name ASC, schema_name ASC, name ASC",
            )
            .fetch_all(&self.pool),
        )?;

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

    // ── Auth / tenancy (ADR-018) ─────────────────────────────────────────────

    fn is_jti_revoked(&self, jti: uuid::Uuid) -> Result<bool, AxonError> {
        let jti_str = jti.to_string();
        let row = self
            .block_on(
                sqlx::query("SELECT 1 AS one FROM credential_revocations WHERE jti = $1")
                    .bind(&jti_str)
                    .fetch_optional(&self.pool),
            )
            .or_else(|e| {
                // Table may not exist if auth migrations haven't run.
                if e.to_string().contains("does not exist") {
                    Ok(None)
                } else {
                    Err(e)
                }
            })?;
        Ok(row.is_some())
    }

    fn get_user(
        &self,
        user_id: axon_core::auth::UserId,
    ) -> Result<Option<axon_core::auth::User>, AxonError> {
        use sqlx::Row;
        let row = self
            .block_on(
                sqlx::query(
                    "SELECT id, display_name, email, created_at_ms, suspended_at_ms \
                     FROM users WHERE id = $1",
                )
                .bind(user_id.as_str())
                .fetch_optional(&self.pool),
            )
            .or_else(|e| {
                if e.to_string().contains("does not exist") {
                    Ok(None)
                } else {
                    Err(e)
                }
            })?;
        let Some(row) = row else {
            return Ok(None);
        };
        Ok(Some(axon_core::auth::User {
            id: axon_core::auth::UserId::new(row.get::<String, _>("id")),
            display_name: row.get("display_name"),
            email: row.get("email"),
            created_at_ms: row.get::<i64, _>("created_at_ms") as u64,
            suspended_at_ms: row
                .get::<Option<i64>, _>("suspended_at_ms")
                .map(|v| v as u64),
        }))
    }

    fn get_tenant_member(
        &self,
        tenant_id: axon_core::auth::TenantId,
        user_id: axon_core::auth::UserId,
    ) -> Result<Option<axon_core::auth::TenantMember>, AxonError> {
        use sqlx::Row;
        let row = self
            .block_on(
                sqlx::query(
                    "SELECT tenant_id, user_id, role FROM tenant_users \
                     WHERE tenant_id = $1 AND user_id = $2",
                )
                .bind(tenant_id.as_str())
                .bind(user_id.as_str())
                .fetch_optional(&self.pool),
            )
            .or_else(|e| {
                if e.to_string().contains("does not exist") {
                    Ok(None)
                } else {
                    Err(e)
                }
            })?;
        let Some(row) = row else {
            return Ok(None);
        };
        let role_str: String = row.get("role");
        let role = match role_str.as_str() {
            "admin" => axon_core::auth::TenantRole::Admin,
            "write" => axon_core::auth::TenantRole::Write,
            _ => axon_core::auth::TenantRole::Read,
        };
        Ok(Some(axon_core::auth::TenantMember {
            tenant_id: axon_core::auth::TenantId::new(row.get::<String, _>("tenant_id")),
            user_id: axon_core::auth::UserId::new(row.get::<String, _>("user_id")),
            role,
        }))
    }

    fn upsert_user_identity(
        &self,
        provider: &str,
        external_id: &str,
        display_name: &str,
        email: Option<&str>,
    ) -> Result<axon_core::auth::User, AxonError> {
        use sqlx::Row;
        // ADR-018 §6 ON CONFLICT pattern: three statements, no advisory locks.
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let new_user_id = axon_core::auth::UserId::generate();

        // Step 1: try to insert a candidate user row (first caller wins).
        self.block_on(
            sqlx::query(
                "INSERT INTO users (id, display_name, email, created_at_ms) \
                 VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
            )
            .bind(new_user_id.as_str())
            .bind(display_name)
            .bind(email)
            .bind(now_ms)
            .execute(&self.pool),
        )
        .map_err(|e| {
            if e.to_string().contains("does not exist") {
                AxonError::Storage(
                    "auth schema not applied; call apply_auth_migrations_postgres first".into(),
                )
            } else {
                e
            }
        })?;

        // Step 2: claim the identity (first caller wins).
        self.block_on(
            sqlx::query(
                "INSERT INTO user_identities (provider, external_id, user_id, created_at_ms) \
                 VALUES ($1, $2, $3, $4) ON CONFLICT (provider, external_id) DO NOTHING",
            )
            .bind(provider)
            .bind(external_id)
            .bind(new_user_id.as_str())
            .bind(now_ms)
            .execute(&self.pool),
        )
        .or_else(|e| {
            if e.to_string().contains("does not exist") {
                Ok(sqlx::postgres::PgQueryResult::default())
            } else {
                Err(e)
            }
        })?;

        // Step 3: read back the winner's user_id.
        let row = self
            .block_on(
                sqlx::query(
                    "SELECT user_id FROM user_identities WHERE provider = $1 AND external_id = $2",
                )
                .bind(provider)
                .bind(external_id)
                .fetch_one(&self.pool),
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        let winning_user_id: String = row.get("user_id");

        // Step 4: return the full user record.
        let user_row = self
            .block_on(
                sqlx::query(
                    "SELECT id, display_name, email, created_at_ms, suspended_at_ms \
                     FROM users WHERE id = $1",
                )
                .bind(&winning_user_id)
                .fetch_one(&self.pool),
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(axon_core::auth::User {
            id: axon_core::auth::UserId::new(user_row.get::<String, _>("id")),
            display_name: user_row.get("display_name"),
            email: user_row.get("email"),
            created_at_ms: user_row.get::<i64, _>("created_at_ms") as u64,
            suspended_at_ms: user_row
                .get::<Option<i64>, _>("suspended_at_ms")
                .map(|v| v as u64),
        })
    }

    fn create_user(
        &self,
        id: &axon_core::auth::UserId,
        display_name: &str,
        email: Option<&str>,
    ) -> Result<axon_core::auth::User, AxonError> {
        use sqlx::Row;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        self.block_on(
            sqlx::query(
                "INSERT INTO users (id, display_name, email, created_at_ms) \
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(id.as_str())
            .bind(display_name)
            .bind(email)
            .bind(now_ms)
            .execute(&self.pool),
        )
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("duplicate key") || msg.contains("unique") {
                AxonError::AlreadyExists(format!("user '{}' already exists", id.as_str()))
            } else if msg.contains("does not exist") {
                AxonError::Storage(
                    "auth schema not applied; call apply_auth_migrations_postgres first".into(),
                )
            } else {
                AxonError::Storage(msg)
            }
        })?;
        let row = self
            .block_on(
                sqlx::query(
                    "SELECT id, display_name, email, created_at_ms, suspended_at_ms \
                     FROM users WHERE id = $1",
                )
                .bind(id.as_str())
                .fetch_one(&self.pool),
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(axon_core::auth::User {
            id: axon_core::auth::UserId::new(row.get::<String, _>("id")),
            display_name: row.get("display_name"),
            email: row.get("email"),
            created_at_ms: row.get::<i64, _>("created_at_ms") as u64,
            suspended_at_ms: row
                .get::<Option<i64>, _>("suspended_at_ms")
                .map(|v| v as u64),
        })
    }

    fn list_users(&self) -> Result<Vec<axon_core::auth::User>, AxonError> {
        use sqlx::Row;
        let rows = self
            .block_on(
                sqlx::query(
                    "SELECT id, display_name, email, created_at_ms, suspended_at_ms \
                     FROM users ORDER BY created_at_ms DESC",
                )
                .fetch_all(&self.pool),
            )
            .or_else(|e| {
                if e.to_string().contains("does not exist") {
                    Ok(vec![])
                } else {
                    Err(e)
                }
            })
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let users = rows
            .iter()
            .map(|row| axon_core::auth::User {
                id: axon_core::auth::UserId::new(row.get::<String, _>("id")),
                display_name: row.get("display_name"),
                email: row.get("email"),
                created_at_ms: row.get::<i64, _>("created_at_ms") as u64,
                suspended_at_ms: row
                    .get::<Option<i64>, _>("suspended_at_ms")
                    .map(|v| v as u64),
            })
            .collect();
        Ok(users)
    }

    fn suspend_user(&self, id: &axon_core::auth::UserId) -> Result<bool, AxonError> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let result = self
            .block_on(
                sqlx::query("UPDATE users SET suspended_at_ms = $1 WHERE id = $2")
                    .bind(now_ms)
                    .bind(id.as_str())
                    .execute(&self.pool),
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(result.rows_affected() > 0)
    }

    fn upsert_tenant_member(
        &self,
        tenant_id: axon_core::auth::TenantId,
        user_id: axon_core::auth::UserId,
        role: axon_core::auth::TenantRole,
    ) -> Result<axon_core::auth::TenantMember, AxonError> {
        let role_str = match role {
            axon_core::auth::TenantRole::Admin => "admin",
            axon_core::auth::TenantRole::Write => "write",
            axon_core::auth::TenantRole::Read => "read",
        };
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        self.block_on(
            sqlx::query(
                "INSERT INTO tenant_users (tenant_id, user_id, role, added_at_ms) \
                 VALUES ($1, $2, $3, $4) \
                 ON CONFLICT (tenant_id, user_id) DO UPDATE SET role = EXCLUDED.role",
            )
            .bind(tenant_id.as_str())
            .bind(user_id.as_str())
            .bind(role_str)
            .bind(now_ms)
            .execute(&self.pool),
        )
        .map_err(|e| {
            if e.to_string().contains("does not exist") {
                AxonError::Storage(
                    "auth schema not applied; call apply_auth_migrations_postgres first".into(),
                )
            } else {
                AxonError::Storage(e.to_string())
            }
        })?;
        Ok(axon_core::auth::TenantMember {
            tenant_id,
            user_id,
            role,
        })
    }

    fn remove_tenant_member(
        &self,
        tenant_id: axon_core::auth::TenantId,
        user_id: axon_core::auth::UserId,
    ) -> Result<bool, AxonError> {
        let result = self
            .block_on(
                sqlx::query("DELETE FROM tenant_users WHERE tenant_id = $1 AND user_id = $2")
                    .bind(tenant_id.as_str())
                    .bind(user_id.as_str())
                    .execute(&self.pool),
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(result.rows_affected() > 0)
    }

    fn list_tenant_members(
        &self,
        tenant_id: axon_core::auth::TenantId,
    ) -> Result<Vec<axon_core::auth::TenantMember>, AxonError> {
        use sqlx::Row;
        let rows = self
            .block_on(
                sqlx::query(
                    "SELECT tenant_id, user_id, role \
                     FROM tenant_users \
                     WHERE tenant_id = $1 \
                     ORDER BY added_at_ms ASC",
                )
                .bind(tenant_id.as_str())
                .fetch_all(&self.pool),
            )
            .or_else(|e| {
                if e.to_string().contains("does not exist") {
                    Ok(vec![])
                } else {
                    Err(e)
                }
            })?;

        let members = rows
            .into_iter()
            .map(|row| {
                let role_str: String = row.get("role");
                let role = match role_str.as_str() {
                    "admin" => axon_core::auth::TenantRole::Admin,
                    "write" => axon_core::auth::TenantRole::Write,
                    _ => axon_core::auth::TenantRole::Read,
                };
                axon_core::auth::TenantMember {
                    tenant_id: axon_core::auth::TenantId::new(row.get::<String, _>("tenant_id")),
                    user_id: axon_core::auth::UserId::new(row.get::<String, _>("user_id")),
                    role,
                }
            })
            .collect();
        Ok(members)
    }

    fn count_tenants(&self) -> Result<usize, AxonError> {
        let result = self.block_on(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tenants").fetch_one(&self.pool),
        );
        match result {
            Ok(count) => Ok(count as usize),
            Err(e) if e.to_string().contains("does not exist") => Ok(0),
            Err(e) => Err(e),
        }
    }

    fn upsert_default_tenant(&self, name: &str) -> Result<axon_core::auth::TenantId, AxonError> {
        use sqlx::Row;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let new_id = axon_core::auth::TenantId::generate();
        match self.block_on(
            sqlx::query(
                "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms) \
                 VALUES ($1, $2, $3, $4, $5) ON CONFLICT (name) DO NOTHING",
            )
            .bind(new_id.as_str())
            .bind(name)
            .bind(name)
            .bind(now_ms)
            .bind(now_ms)
            .execute(&self.pool),
        ) {
            Ok(_) => {}
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("does not exist") {
                    return Err(AxonError::Storage(
                        "auth schema not applied; call apply_auth_migrations_postgres first".into(),
                    ));
                }
                return Err(e);
            }
        }
        let row = self
            .block_on(
                sqlx::query("SELECT id FROM tenants WHERE name = $1")
                    .bind(name)
                    .fetch_optional(&self.pool),
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        row.map(|r| axon_core::auth::TenantId::new(r.get::<String, _>(0)))
            .ok_or_else(|| AxonError::Storage(format!("tenant '{name}' not found after upsert")))
    }

    fn get_retention_policy(
        &self,
        tenant_id: axon_core::auth::TenantId,
    ) -> Result<Option<axon_core::auth::RetentionPolicy>, AxonError> {
        use sqlx::Row;
        let row = self
            .block_on(
                sqlx::query(
                    "SELECT archive_after_seconds, purge_after_seconds \
                     FROM tenant_retention_policies \
                     WHERE tenant_id = $1",
                )
                .bind(tenant_id.as_str())
                .fetch_optional(&self.pool),
            )
            .or_else(|e| {
                if e.to_string().contains("does not exist") {
                    Ok(None)
                } else {
                    Err(e)
                }
            })?;

        let Some(row) = row else {
            return Ok(None);
        };
        Ok(Some(axon_core::auth::RetentionPolicy {
            archive_after_seconds: row.get::<i64, _>("archive_after_seconds") as u64,
            purge_after_seconds: row
                .get::<Option<i64>, _>("purge_after_seconds")
                .map(|v| v as u64),
        }))
    }

    fn set_retention_policy(
        &self,
        tenant_id: axon_core::auth::TenantId,
        policy: &axon_core::auth::RetentionPolicy,
    ) -> Result<(), AxonError> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        self.block_on(
            sqlx::query(
                "INSERT INTO tenant_retention_policies \
                 (tenant_id, archive_after_seconds, purge_after_seconds, updated_at_ms) \
                 VALUES ($1, $2, $3, $4) \
                 ON CONFLICT (tenant_id) DO UPDATE SET \
                 archive_after_seconds = EXCLUDED.archive_after_seconds, \
                 purge_after_seconds = EXCLUDED.purge_after_seconds, \
                 updated_at_ms = EXCLUDED.updated_at_ms",
            )
            .bind(tenant_id.as_str())
            .bind(policy.archive_after_seconds as i64)
            .bind(policy.purge_after_seconds.map(|v| v as i64))
            .bind(now_ms)
            .execute(&self.pool),
        )
        .map_err(|e| {
            if e.to_string().contains("does not exist") {
                AxonError::Storage(
                    "auth schema not applied; call apply_auth_migrations_postgres first".into(),
                )
            } else {
                AxonError::Storage(e.to_string())
            }
        })?;
        Ok(())
    }

    fn list_tenant_databases(
        &self,
        tenant_id: axon_core::auth::TenantId,
    ) -> Result<Vec<TenantDatabase>, AxonError> {
        use sqlx::Row;
        let rows = self
            .block_on(
                sqlx::query(
                    "SELECT tenant_id, database_name, created_at_ms \
                     FROM tenant_databases \
                     WHERE tenant_id = $1 \
                     ORDER BY created_at_ms ASC",
                )
                .bind(tenant_id.as_str())
                .fetch_all(&self.pool),
            )
            .or_else(|e| {
                if e.to_string().contains("does not exist") {
                    Ok(vec![])
                } else {
                    Err(e)
                }
            })
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let dbs = rows
            .iter()
            .map(|row| TenantDatabase {
                tenant_id: axon_core::auth::TenantId::new(row.get::<String, _>("tenant_id")),
                name: row.get("database_name"),
                created_at_ms: row.get::<i64, _>("created_at_ms") as u64,
            })
            .collect();
        Ok(dbs)
    }

    fn create_tenant_database(
        &self,
        tenant_id: axon_core::auth::TenantId,
        name: &str,
    ) -> Result<TenantDatabase, AxonError> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let result = self
            .block_on(
                sqlx::query(
                    "INSERT INTO tenant_databases (tenant_id, database_name, created_at_ms) \
                     VALUES ($1, $2, $3) \
                     ON CONFLICT (tenant_id, database_name) DO NOTHING",
                )
                .bind(tenant_id.as_str())
                .bind(name)
                .bind(now_ms)
                .execute(&self.pool),
            )
            .map_err(|e| {
                if e.to_string().contains("does not exist") {
                    AxonError::Storage(
                        "auth schema not applied; call apply_auth_migrations_postgres first".into(),
                    )
                } else {
                    AxonError::Storage(e.to_string())
                }
            })?;
        if result.rows_affected() == 0 {
            return Err(AxonError::AlreadyExists(format!(
                "database '{}' already exists in tenant '{}'",
                name,
                tenant_id.as_str()
            )));
        }
        Ok(TenantDatabase {
            tenant_id,
            name: name.to_string(),
            created_at_ms: now_ms as u64,
        })
    }

    fn delete_tenant_database(
        &self,
        tenant_id: axon_core::auth::TenantId,
        name: &str,
    ) -> Result<bool, AxonError> {
        let result = self
            .block_on(
                sqlx::query(
                    "DELETE FROM tenant_databases WHERE tenant_id = $1 AND database_name = $2",
                )
                .bind(tenant_id.as_str())
                .bind(name)
                .execute(&self.pool),
            )
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(result.rows_affected() > 0)
    }

    fn track_credential_issuance(
        &self,
        jti: uuid::Uuid,
        user_id: axon_core::auth::UserId,
        tenant_id: axon_core::auth::TenantId,
        issued_at_ms: i64,
        expires_at_ms: i64,
        grants_json: &str,
    ) -> Result<(), AxonError> {
        let jti_str = jti.to_string();
        self.block_on(
            sqlx::query(
                "INSERT INTO credential_issuances \
                 (jti, user_id, tenant_id, issued_at_ms, expires_at_ms, grants_json) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(&jti_str)
            .bind(user_id.as_str())
            .bind(tenant_id.as_str())
            .bind(issued_at_ms)
            .bind(expires_at_ms)
            .bind(grants_json)
            .execute(&self.pool),
        )
        .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    }

    fn list_credentials(
        &self,
        tenant_id: axon_core::auth::TenantId,
        user_filter: Option<axon_core::auth::UserId>,
    ) -> Result<Vec<axon_core::auth::CredentialMetadata>, AxonError> {
        use sqlx::Row;
        let rows = self
            .block_on(
                sqlx::query(
                    "SELECT ci.jti, ci.user_id, ci.tenant_id, ci.issued_at_ms, ci.expires_at_ms, \
                     ci.grants_json, \
                     CASE WHEN cr.jti IS NOT NULL THEN TRUE ELSE FALSE END AS revoked \
                     FROM credential_issuances ci \
                     LEFT JOIN credential_revocations cr ON ci.jti = cr.jti \
                     WHERE ci.tenant_id = $1 \
                     ORDER BY ci.issued_at_ms ASC",
                )
                .bind(tenant_id.as_str())
                .fetch_all(&self.pool),
            )
            .or_else(|e| {
                if e.to_string().contains("does not exist") {
                    Ok(vec![])
                } else {
                    Err(e)
                }
            })
            .map_err(|e| AxonError::Storage(e.to_string()))?;

        let mut creds = Vec::new();
        for row in rows {
            let meta = axon_core::auth::CredentialMetadata {
                jti: row.get::<String, _>("jti"),
                user_id: axon_core::auth::UserId::new(row.get::<String, _>("user_id")),
                tenant_id: axon_core::auth::TenantId::new(row.get::<String, _>("tenant_id")),
                issued_at_ms: row.get::<i64, _>("issued_at_ms"),
                expires_at_ms: row.get::<i64, _>("expires_at_ms"),
                grants_json: row.get::<String, _>("grants_json"),
                revoked: row.get::<bool, _>("revoked"),
            };
            if let Some(ref uid) = user_filter {
                if &meta.user_id != uid {
                    continue;
                }
            }
            creds.push(meta);
        }
        Ok(creds)
    }

    fn revoke_credential(
        &self,
        jti: uuid::Uuid,
        revoked_by: axon_core::auth::UserId,
    ) -> Result<(), AxonError> {
        let jti_str = jti.to_string();
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        self.block_on(
            sqlx::query(
                "INSERT INTO credential_revocations (jti, revoked_at_ms, revoked_by) \
                 VALUES ($1, $2, $3) \
                 ON CONFLICT (jti) DO NOTHING",
            )
            .bind(&jti_str)
            .bind(now_ms)
            .bind(revoked_by.as_str())
            .execute(&self.pool),
        )
        .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
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

pub fn tenant_postgres_roles(tenant_db_name: &str) -> Result<PostgresTenantRoles, AxonError> {
    validate_pg_db_name(tenant_db_name)?;
    Ok(PostgresTenantRoles {
        runtime: format!("axon_{tenant_db_name}_runtime"),
        capability: format!("axon_{tenant_db_name}_capability"),
        migration: format!("axon_{tenant_db_name}_migration"),
    })
}

fn quote_pg_ident(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

/// Derive a per-tenant DSN from a superadmin DSN by replacing/adding the
/// `dbname` key.
///
/// The superadmin DSN uses the `postgres` maintenance database.  We produce a
/// new DSN that targets `axon_{name}`.
///
/// Both libpq keyword-value format (`host=... dbname=...`) and URL format
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
        // Keyword-value format.  Replace existing dbname= or append it.
        let mut parts: Vec<String> = superadmin_dsn
            .split_whitespace()
            .filter(|part| !part.starts_with("dbname="))
            .map(str::to_owned)
            .collect();
        parts.push(format!("dbname={target}"));
        parts.join(" ")
    }
}

async fn create_postgres_tenant_roles(
    pool: &sqlx::PgPool,
    roles: &PostgresTenantRoles,
) -> Result<(), AxonError> {
    use sqlx::Row;

    for role in [&roles.runtime, &roles.capability, &roles.migration] {
        let row = sqlx::query("SELECT EXISTS(SELECT 1 FROM pg_roles WHERE rolname = $1)")
            .bind(role)
            .fetch_one(pool)
            .await
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        let exists: bool = row.get(0);
        if !exists {
            sqlx::raw_sql(&format!("CREATE ROLE {} NOLOGIN", quote_pg_ident(role)))
                .execute(pool)
                .await
                .map_err(|e| AxonError::Storage(e.to_string()))?;
        }
    }

    Ok(())
}

async fn drop_postgres_tenant_roles(
    pool: &sqlx::PgPool,
    roles: &PostgresTenantRoles,
) -> Result<(), AxonError> {
    for role in [&roles.runtime, &roles.capability, &roles.migration] {
        sqlx::raw_sql(&format!("DROP ROLE IF EXISTS {}", quote_pg_ident(role)))
            .execute(pool)
            .await
            .map_err(|e| AxonError::Storage(e.to_string()))?;
    }

    Ok(())
}

fn apply_postgres_routine_privileges(
    tenant_dsn: &str,
    roles: &PostgresTenantRoles,
) -> Result<(), AxonError> {
    let options = pg_connect_options(tenant_dsn)?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| AxonError::Storage(e.to_string()))?;
    rt.block_on(async {
        let pool = sqlx::PgPool::connect_with(options)
            .await
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        let runtime = quote_pg_ident(&roles.runtime);
        let capability = quote_pg_ident(&roles.capability);
        let migration = quote_pg_ident(&roles.migration);
        let statements = [
            format!("GRANT USAGE ON SCHEMA public TO {runtime}"),
            format!("GRANT USAGE ON SCHEMA public TO {capability}"),
            format!("GRANT USAGE ON SCHEMA public TO {migration}"),
            format!("REVOKE ALL ON FUNCTION {POSTGRES_MUTATING_ROUTINE_SIGNATURE} FROM {runtime}"),
            format!(
                "GRANT EXECUTE ON FUNCTION {POSTGRES_MUTATING_ROUTINE_SIGNATURE} TO {capability}"
            ),
            format!(
                "GRANT EXECUTE ON FUNCTION {POSTGRES_MUTATING_ROUTINE_SIGNATURE} TO {migration}"
            ),
        ];

        for statement in statements {
            sqlx::raw_sql(&statement)
                .execute(&pool)
                .await
                .map_err(|e| AxonError::Storage(e.to_string()))?;
        }

        Ok(())
    })
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
    let roles = tenant_postgres_roles(name)?;
    let options = pg_connect_options(superadmin_dsn)?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| AxonError::Storage(e.to_string()))?;
    rt.block_on(async {
        use sqlx::Row;
        let pool = sqlx::PgPool::connect_with(options)
            .await
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        let row = sqlx::query("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1)")
            .bind(&full_name)
            .fetch_one(&pool)
            .await
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        let exists: bool = row.get(0);
        if exists {
            return Err(AxonError::AlreadyExists(format!(
                "PostgreSQL database '{full_name}'"
            )));
        }
        create_postgres_tenant_roles(&pool, &roles).await?;
        // CREATE DATABASE cannot run inside a transaction; use raw_sql to
        // avoid the implicit transaction that execute() would open.
        sqlx::raw_sql(&format!("CREATE DATABASE \"{full_name}\""))
            .execute(&pool)
            .await
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        Ok(())
    })?;

    let tenant = tenant_dsn(superadmin_dsn, name);
    let adapter = PostgresStorageAdapter::connect(&tenant)?;
    drop(adapter);
    apply_postgres_routine_privileges(&tenant, &roles)
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
    let roles = tenant_postgres_roles(name)?;
    let options = pg_connect_options(superadmin_dsn)?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| AxonError::Storage(e.to_string()))?;
    rt.block_on(async {
        use sqlx::Row;
        let pool = sqlx::PgPool::connect_with(options)
            .await
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        let row = sqlx::query("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1)")
            .bind(&full_name)
            .fetch_one(&pool)
            .await
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        let exists: bool = row.get(0);
        if !exists {
            return Err(AxonError::NotFound(format!(
                "PostgreSQL database '{full_name}'"
            )));
        }
        sqlx::raw_sql(&format!("DROP DATABASE \"{full_name}\""))
            .execute(&pool)
            .await
            .map_err(|e| AxonError::Storage(e.to_string()))?;
        drop_postgres_tenant_roles(&pool, &roles).await?;
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
        sync::{
            atomic::{AtomicU64, Ordering},
            Mutex, MutexGuard, OnceLock,
        },
    };

    use super::*;
    use axon_core::intent::{
        CanonicalOperationMetadata, MutationIntentDecision, MutationIntentScopeBinding,
        MutationIntentSubjectBinding, MutationOperationKind, MutationReviewSummary,
    };
    use axon_core::types::Link;
    use testcontainers_modules::postgres::Postgres as PgContainer;
    use testcontainers_modules::testcontainers::{runners::SyncRunner, Container};

    struct TestDatabase {
        url: String,
        _container: Option<Container<PgContainer>>,
        cleanup: Option<(String, String)>,
    }

    enum TestSetupError {
        Skip(String),
        Fail(AxonError),
    }

    impl TestDatabase {
        fn connect() -> Result<Self, TestSetupError> {
            if let Ok(superadmin_dsn) = std::env::var("AXON_TEST_POSTGRES") {
                static COUNTER: AtomicU64 = AtomicU64::new(0);
                let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
                let database_name = format!("ut_{}_{sequence:06}", std::process::id());
                provision_postgres_database(&superadmin_dsn, &database_name)
                    .map_err(TestSetupError::Fail)?;
                return Ok(Self {
                    url: tenant_dsn(&superadmin_dsn, &database_name),
                    _container: None,
                    cleanup: Some((superadmin_dsn, database_name)),
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
                url: format!("postgres://postgres:postgres@{host}:{port}/axon_test"),
                _container: Some(container),
                cleanup: None,
            })
        }

        fn url(&self) -> &str {
            &self.url
        }
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            if let Some((superadmin_dsn, database_name)) = self.cleanup.take() {
                let _ = force_deprovision_test_database(&superadmin_dsn, &database_name);
            }
        }
    }

    fn force_deprovision_test_database(
        superadmin_dsn: &str,
        database_name: &str,
    ) -> Result<(), AxonError> {
        validate_pg_db_name(database_name)?;
        let full_name = format!("axon_{database_name}");
        let options = pg_connect_options(superadmin_dsn)?;
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| AxonError::Storage(error.to_string()))?;
        rt.block_on(async {
            let pool = sqlx::postgres::PgPoolOptions::new()
                .max_connections(1)
                .connect_with(options)
                .await
                .map_err(|error| AxonError::Storage(error.to_string()))?;
            let result = sqlx::raw_sql(&format!("DROP DATABASE \"{full_name}\" WITH (FORCE)"))
                .execute(&pool)
                .await
                .map_err(|error| AxonError::Storage(error.to_string()));
            pool.close().await;
            result.map(|_| ())
        })
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
            .block_on(
                sqlx::raw_sql(
                    "TRUNCATE entities, schemas, collection_views, collections, namespaces, databases, audit_log, mutation_intents, entity_index, entity_compound_index RESTART IDENTITY CASCADE",
                )
                .execute(&adapter.pool),
            )
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

    fn intent(intent_id: &str) -> MutationIntent {
        MutationIntent {
            intent_id: intent_id.into(),
            scope: MutationIntentScopeBinding {
                tenant_id: "tenant-a".into(),
                database_id: "finance".into(),
            },
            subject: MutationIntentSubjectBinding::default(),
            schema_version: 1,
            policy_version: 1,
            operation: CanonicalOperationMetadata {
                operation_kind: MutationOperationKind::UpdateEntity,
                operation_hash: format!("sha256:{intent_id}"),
                canonical_operation: Some(serde_json::json!({"id": intent_id})),
            },
            pre_images: Vec::new(),
            decision: MutationIntentDecision::NeedsApproval,
            approval_state: ApprovalState::Pending,
            approval_route: None,
            expires_at: 2_000,
            review_summary: MutationReviewSummary::default(),
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
    fn native_content_version_is_version_inclusive_when_available() {
        // The native Postgres content_version push-down (ADR-026 strict guard,
        // md5 over id:version) must move on an in-place update, while the native
        // structural_version (md5 over the id-set) stays put — confirming the
        // pushed-down strict signature catches update-driven predicate skew.
        let _guard = postgres_test_guard();
        let Some(mut s) =
            store_or_skip("native_content_version_is_version_inclusive_when_available")
        else {
            return;
        };

        let col = CollectionId::new("tasks");
        s.put(Entity::new(
            col.clone(),
            EntityId::new("t-001"),
            serde_json::json!({"title": "a"}),
        ))
        .expect("put");
        s.put(Entity::new(
            col.clone(),
            EntityId::new("t-002"),
            serde_json::json!({"title": "b"}),
        ))
        .expect("put");

        let membership_before = s.structural_version(&col).expect("structural");
        let content_before = s.content_version(&col).expect("content");
        // Re-reading without a write must be stable (self-consistent signature).
        assert_eq!(
            content_before,
            s.content_version(&col).expect("content"),
            "content signature must be stable across reads"
        );

        // In-place update: membership unchanged, version bumps.
        s.compare_and_swap(
            Entity::new(
                col.clone(),
                EntityId::new("t-001"),
                serde_json::json!({"title": "a2"}),
            ),
            1,
        )
        .expect("cas");

        assert_eq!(
            membership_before,
            s.structural_version(&col).expect("structural"),
            "membership signature must be stable across an in-place update"
        );
        assert_ne!(
            content_before,
            s.content_version(&col).expect("content"),
            "content signature must change on an in-place update"
        );
    }

    #[test]
    fn mutation_intent_roundtrip_when_available() {
        let _guard = postgres_test_guard();
        let Some(mut s) = store_or_skip("mutation_intent_roundtrip_when_available") else {
            return;
        };

        s.create_mutation_intent(&intent("mint-pg"))
            .expect("intent create should succeed");
        let updated = s
            .update_mutation_intent_state(
                "tenant-a",
                "finance",
                "mint-pg",
                ApprovalState::Pending,
                ApprovalState::Approved,
            )
            .expect("intent state update should succeed");
        assert_eq!(updated.approval_state, ApprovalState::Approved);

        let stored = s
            .get_mutation_intent("tenant-a", "finance", "mint-pg")
            .expect("intent lookup should succeed")
            .expect("intent should exist");
        assert_eq!(stored.approval_state, ApprovalState::Approved);
    }

    #[test]
    fn unregister_collection_cleans_up_legacy_collection_views_when_available() {
        let _guard = postgres_test_guard();
        let Some(database) = database_or_skip(
            "unregister_collection_cleans_up_legacy_collection_views_when_available",
        ) else {
            return;
        };

        // Set up legacy schema using a raw sqlx pool.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        let pool = rt
            .block_on(sqlx::PgPool::connect(database.url()))
            .expect("test connection");
        rt.block_on(async {
            sqlx::raw_sql(
                "DROP TABLE IF EXISTS collection_views;
                 DROP TABLE IF EXISTS collections;
                 DROP TABLE IF EXISTS namespaces;
                 DROP TABLE IF EXISTS databases;
                 DROP TABLE IF EXISTS entities;
                 DROP TABLE IF EXISTS schemas;
                 DROP TABLE IF EXISTS audit_log;",
            )
            .execute(&pool)
            .await
            .expect("drop tables should succeed");
            sqlx::raw_sql(
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
            .execute(&pool)
            .await
            .expect("legacy schema should be created");
        });

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

        rt.block_on(async {
            sqlx::query("INSERT INTO collections (name) VALUES ($1)")
                .bind(collection.as_str())
                .execute(&pool)
                .await
                .expect("collection insert should succeed");
            sqlx::query(
                "INSERT INTO collection_views (collection, version, view_json, updated_at_ns, updated_by)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(collection.as_str())
            .bind(1_i32)
            .bind(&legacy_view_json)
            .bind(42_i64)
            .bind(Some("legacy"))
            .execute(&pool)
            .await
            .expect("legacy data should insert");
        });
        drop(pool);
        drop(rt);

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
            .block_on(
                sqlx::query_scalar("SELECT COUNT(*) FROM collection_views WHERE collection = $1")
                    .bind(collection.as_str())
                    .fetch_one(&adapter.pool),
            )
            .expect("count query should succeed");
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
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
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
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };

        s.put_schema(&v1).expect("schema v1 put should succeed");
        s.put_schema(&v2).expect("schema v2 put should succeed");

        use sqlx::Row;
        let stored_collections: Vec<String> = s
            .block_on(
                sqlx::query(
                    "SELECT collection FROM schemas
                     WHERE database_name = $1 AND schema_name = $2
                     ORDER BY version ASC",
                )
                .bind(billing.database.as_str())
                .bind(billing.schema.as_str())
                .fetch_all(&s.pool),
            )
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

        use sqlx::Row;
        let stored_collection: String = s
            .block_on(
                sqlx::query(
                    "SELECT collection FROM collection_views
                     WHERE database_name = $1 AND schema_name = $2",
                )
                .bind(billing.database.as_str())
                .bind(billing.schema.as_str())
                .fetch_one(&s.pool),
            )
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
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        })
        .expect("qualified schema put should succeed");
        s.put_collection_view(&CollectionView::new(qualified.clone(), "# {{title}}"))
            .expect("qualified collection view put should succeed");

        s.unregister_collection(&qualified)
            .expect("qualified unregister should succeed");

        use sqlx::Row;
        let row_counts = s
            .block_on(
                sqlx::query(
                    "SELECT
                    (SELECT COUNT(*) FROM collections
                     WHERE name = $1 AND database_name = $2 AND schema_name = $3) AS collections_count,
                    (SELECT COUNT(*) FROM schemas
                     WHERE collection = $1 AND database_name = $2 AND schema_name = $3) AS schemas_count,
                    (SELECT COUNT(*) FROM collection_views
                     WHERE collection = $1 AND database_name = $2 AND schema_name = $3) AS views_count",
                )
                .bind(invoices.as_str())
                .bind(billing.database.as_str())
                .bind(billing.schema.as_str())
                .fetch_one(&s.pool),
            )
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
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
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
        s.block_on(
            sqlx::query(
                "INSERT INTO collections (name, database_name, schema_name)
                 VALUES ($1, $2, $3)",
            )
            .bind(qualified.as_str())
            .bind(DEFAULT_DATABASE)
            .bind(DEFAULT_SCHEMA)
            .execute(&s.pool),
        )
        .expect("legacy collection row should insert");
        s.block_on(
            sqlx::query(
                "INSERT INTO schemas
                    (collection, database_name, schema_name, version, schema_json, created_at_ns)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(qualified.as_str())
            .bind(DEFAULT_DATABASE)
            .bind(DEFAULT_SCHEMA)
            .bind(99_i32)
            .bind(&legacy_schema_json)
            .bind(42_i64)
            .execute(&s.pool),
        )
        .expect("legacy schema row should insert");
        s.block_on(
            sqlx::query(
                "INSERT INTO collection_views
                    (collection, database_name, schema_name, version, view_json, updated_at_ns, updated_by)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(qualified.as_str())
            .bind(DEFAULT_DATABASE)
            .bind(DEFAULT_SCHEMA)
            .bind(99_i32)
            .bind(&legacy_view_json)
            .bind(42_i64)
            .bind(Some("legacy"))
            .execute(&s.pool),
        )
        .expect("legacy collection view row should insert");

        use sqlx::Row;
        let before_unregister: i64 = s
            .block_on(
                sqlx::query(
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
                )
                .bind(invoices.as_str())
                .bind(billing.database.as_str())
                .bind(billing.schema.as_str())
                .bind(qualified.as_str())
                .bind(DEFAULT_DATABASE)
                .bind(DEFAULT_SCHEMA)
                .fetch_one(&s.pool),
            )
            .expect("metadata row count before unregister should succeed")
            .get(0);
        assert_eq!(
            before_unregister, 6,
            "test setup should include normalized and legacy rows across all catalog tables"
        );

        s.unregister_collection(&qualified)
            .expect("qualified unregister should succeed");

        let remaining_metadata_rows: i64 = s
            .block_on(
                sqlx::query(
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
                )
                .bind(invoices.as_str())
                .bind(billing.database.as_str())
                .bind(billing.schema.as_str())
                .bind(qualified.as_str())
                .bind(DEFAULT_DATABASE)
                .bind(DEFAULT_SCHEMA)
                .fetch_one(&s.pool),
            )
            .expect("metadata row count after unregister should succeed")
            .get(0);
        assert_eq!(
            remaining_metadata_rows, 0,
            "qualified unregister must remove normalized and default-namespaced legacy metadata"
        );
    }

    // ── Persisted secondary index tests (FEAT-013) ───────────────────────
    //
    // Mirror the SQLite `index_tests` + `compound_index_tests` so all three
    // backends share equality/range/unique/array/null/compound/backfill
    // semantics. Each test runs against a real Postgres (testcontainers or
    // AXON_TEST_POSTGRES) and skips cleanly when none is reachable. The shared
    // serialization guard plus the per-`store()` TRUNCATE keep them isolated.
    mod index_tests {
        use super::*;
        use crate::adapter::IndexValue;
        use axon_schema::schema::{CollectionSchema, IndexDef, IndexType};
        use serde_json::json;
        use std::ops::Bound;

        fn tasks() -> CollectionId {
            CollectionId::new("tasks")
        }

        fn status_index() -> IndexDef {
            IndexDef {
                field: "status".into(),
                index_type: IndexType::String,
                unique: false,
            }
        }

        fn priority_index() -> IndexDef {
            IndexDef {
                field: "priority".into(),
                index_type: IndexType::Integer,
                unique: false,
            }
        }

        fn unique_email_index() -> IndexDef {
            IndexDef {
                field: "email".into(),
                index_type: IndexType::String,
                unique: true,
            }
        }

        fn task(id: &str, data: serde_json::Value) -> Entity {
            Entity::new(tasks(), EntityId::new(id), data)
        }

        /// Count rows in a table for the current connection (test helper).
        fn count_rows(store: &TestStore, table: &str) -> i64 {
            store
                .block_on(
                    sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                        .fetch_one(&store.pool),
                )
                .expect("count query should succeed")
        }

        #[test]
        fn null_and_type_mismatch_values_are_not_indexed() {
            let _guard = postgres_test_guard();
            let Some(mut store) = store_or_skip("null_and_type_mismatch_values_are_not_indexed")
            else {
                return;
            };
            let col = tasks();
            let mut schema = CollectionSchema::new(col.clone());
            schema.indexes = vec![status_index()];
            store.put_schema(&schema).expect("put_schema");

            // Missing field.
            store
                .put(task("t-001", json!({"title": "no status"})))
                .expect("put");
            // Type mismatch: integer for a string index → skipped.
            store
                .put(task("t-002", json!({"status": 42})))
                .expect("put");

            assert!(store
                .index_lookup(&col, "status", &IndexValue::String(String::new()))
                .expect("lookup")
                .is_empty());
            assert_eq!(
                count_rows(&store, "entity_index"),
                0,
                "null/missing/type-mismatch must not be indexed"
            );
        }

        #[test]
        fn nested_field_path_indexing() {
            let _guard = postgres_test_guard();
            let Some(mut store) = store_or_skip("nested_field_path_indexing") else {
                return;
            };
            let col = tasks();
            let mut schema = CollectionSchema::new(col.clone());
            schema.indexes = vec![IndexDef {
                field: "address.city".into(),
                index_type: IndexType::String,
                unique: false,
            }];
            store.put_schema(&schema).expect("put_schema");

            store
                .put(task("t-001", json!({"address": {"city": "NYC"}})))
                .expect("put");
            assert_eq!(
                store
                    .index_lookup(&col, "address.city", &IndexValue::String("NYC".into()))
                    .expect("lookup"),
                vec![EntityId::new("t-001")]
            );
        }

        #[test]
        fn index_unique_conflict_check() {
            let _guard = postgres_test_guard();
            let Some(mut store) = store_or_skip("index_unique_conflict_check") else {
                return;
            };
            let col = tasks();
            let mut schema = CollectionSchema::new(col.clone());
            schema.indexes = vec![unique_email_index()];
            store.put_schema(&schema).expect("put_schema");

            store
                .put(task("u-001", json!({"email": "alice@example.com"})))
                .expect("put");
            assert!(store
                .index_unique_conflict(
                    &col,
                    "email",
                    &IndexValue::String("alice@example.com".into()),
                    &EntityId::new("u-002"),
                )
                .expect("conflict check"));
            assert!(!store
                .index_unique_conflict(
                    &col,
                    "email",
                    &IndexValue::String("alice@example.com".into()),
                    &EntityId::new("u-001"),
                )
                .expect("conflict check"));
        }

        #[test]
        fn abort_tx_rolls_back_index_changes() {
            let _guard = postgres_test_guard();
            let Some(mut store) = store_or_skip("abort_tx_rolls_back_index_changes") else {
                return;
            };
            let col = tasks();
            let mut schema = CollectionSchema::new(col.clone());
            schema.indexes = vec![status_index()];
            store.put_schema(&schema).expect("put_schema");

            store
                .put(task("t-001", json!({"status": "pending"})))
                .expect("put");

            store.begin_tx().expect("begin");
            store
                .put(task("t-001", json!({"status": "done"})))
                .expect("put in tx");
            store.abort_tx().expect("abort");

            assert_eq!(
                store
                    .index_lookup(&col, "status", &IndexValue::String("pending".into()))
                    .expect("lookup"),
                vec![EntityId::new("t-001")]
            );
            assert!(store
                .index_lookup(&col, "status", &IndexValue::String("done".into()))
                .expect("lookup")
                .is_empty());
        }

        #[test]
        fn backfill_via_reindex_collection_covers_preexisting_entities() {
            let _guard = postgres_test_guard();
            let Some(mut store) =
                store_or_skip("backfill_via_reindex_collection_covers_preexisting_entities")
            else {
                return;
            };
            let col = tasks();
            store
                .put(task("t-001", json!({"status": "pending"})))
                .expect("put");
            store
                .put(task("t-002", json!({"status": "done"})))
                .expect("put");
            store
                .put(task("t-003", json!({"priority": 5})))
                .expect("put");

            // No index rows yet.
            assert!(store
                .index_lookup(&col, "status", &IndexValue::String("pending".into()))
                .expect("lookup")
                .is_empty());

            store
                .reindex_collection(&col, &[status_index(), priority_index()])
                .expect("reindex");

            assert_eq!(
                store
                    .index_lookup(&col, "status", &IndexValue::String("pending".into()))
                    .expect("lookup"),
                vec![EntityId::new("t-001")]
            );
            let by_priority = store
                .index_range(&col, "priority", Bound::Unbounded, Bound::Unbounded)
                .expect("range");
            assert_eq!(by_priority, vec![EntityId::new("t-003")]);
        }

        #[test]
        fn backfill_via_put_schema_hook() {
            let _guard = postgres_test_guard();
            let Some(mut store) = store_or_skip("backfill_via_put_schema_hook") else {
                return;
            };
            let col = tasks();
            store
                .put(task("t-001", json!({"status": "pending"})))
                .expect("put");
            store
                .put(task("t-002", json!({"status": "pending"})))
                .expect("put");

            let mut schema = CollectionSchema::new(col.clone());
            schema.indexes = vec![status_index()];
            store.put_schema(&schema).expect("put_schema");

            let mut results = store
                .index_lookup(&col, "status", &IndexValue::String("pending".into()))
                .expect("lookup");
            results.sort();
            assert_eq!(
                results,
                vec![EntityId::new("t-001"), EntityId::new("t-002")]
            );
        }

        #[test]
        fn reindex_empty_collection_is_noop() {
            let _guard = postgres_test_guard();
            let Some(mut store) = store_or_skip("reindex_empty_collection_is_noop") else {
                return;
            };
            store
                .reindex_collection(&tasks(), &[status_index()])
                .expect("reindex over empty collection must succeed");
            assert_eq!(count_rows(&store, "entity_index"), 0);
        }
    }

    // ── Compound persisted index tests (FEAT-013 / US-033) ───────────────
    mod compound_index_tests {
        use super::*;
        use crate::adapter::{CompoundKey, IndexValue};
        use axon_schema::schema::{
            CollectionSchema, CompoundIndexDef, CompoundIndexField, IndexType,
        };
        use serde_json::json;

        fn tasks() -> CollectionId {
            CollectionId::new("tasks")
        }

        fn count_rows(store: &TestStore, table: &str) -> i64 {
            store
                .block_on(
                    sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                        .fetch_one(&store.pool),
                )
                .expect("count query should succeed")
        }

        fn status_priority_index(unique: bool) -> CompoundIndexDef {
            CompoundIndexDef {
                fields: vec![
                    CompoundIndexField {
                        field: "status".into(),
                        index_type: IndexType::String,
                    },
                    CompoundIndexField {
                        field: "priority".into(),
                        index_type: IndexType::Integer,
                    },
                ],
                unique,
            }
        }

        fn ctask(id: &str, data: serde_json::Value) -> Entity {
            Entity::new(tasks(), EntityId::new(id), data)
        }

        #[test]
        fn compound_index_missing_field_is_sparse() {
            let _guard = postgres_test_guard();
            let Some(mut store) = store_or_skip("compound_index_missing_field_is_sparse") else {
                return;
            };
            let col = tasks();
            let mut schema = CollectionSchema::new(col.clone());
            schema.compound_indexes = vec![status_priority_index(false)];
            store.put_schema(&schema).expect("put_schema");

            // Missing `priority` → no compound entry.
            store
                .put(ctask("t-001", json!({"status": "pending"})))
                .expect("put");

            let prefix = CompoundKey(vec![IndexValue::String("pending".into())]);
            let results = store
                .compound_index_prefix(&col, 0, &prefix)
                .expect("prefix");
            assert!(results.is_empty());
            assert_eq!(count_rows(&store, "entity_compound_index"), 0);
        }

        #[test]
        fn drop_indexes_clears_compound_rows() {
            let _guard = postgres_test_guard();
            let Some(mut store) = store_or_skip("drop_indexes_clears_compound_rows") else {
                return;
            };
            let col = tasks();
            let mut schema = CollectionSchema::new(col.clone());
            schema.compound_indexes = vec![status_priority_index(false)];
            store.put_schema(&schema).expect("put_schema");

            store
                .put(ctask("t-001", json!({"status": "pending", "priority": 1})))
                .expect("put");
            store.drop_indexes(&col).expect("drop");

            assert_eq!(count_rows(&store, "entity_compound_index"), 0);
        }

        #[test]
        fn backfill_compound_via_reindex_covers_preexisting_entities() {
            let _guard = postgres_test_guard();
            let Some(mut store) =
                store_or_skip("backfill_compound_via_reindex_covers_preexisting_entities")
            else {
                return;
            };
            let col = tasks();
            store
                .put(ctask("t-001", json!({"status": "pending", "priority": 1})))
                .expect("put");
            store
                .put(ctask("t-002", json!({"status": "pending", "priority": 2})))
                .expect("put");
            // Sparse entity (missing priority) must not produce a row.
            store
                .put(ctask("t-003", json!({"status": "done"})))
                .expect("put");

            let prefix = CompoundKey(vec![IndexValue::String("pending".into())]);
            assert!(store
                .compound_index_prefix(&col, 0, &prefix)
                .expect("prefix")
                .is_empty());

            store
                .reindex_compound_collection(&col, &[status_priority_index(false)])
                .expect("reindex compound");

            let mut results = store
                .compound_index_prefix(&col, 0, &prefix)
                .expect("prefix");
            results.sort();
            assert_eq!(
                results,
                vec![EntityId::new("t-001"), EntityId::new("t-002")]
            );
            assert_eq!(count_rows(&store, "entity_compound_index"), 2);
        }

        #[test]
        fn backfill_compound_via_put_schema_hook() {
            let _guard = postgres_test_guard();
            let Some(mut store) = store_or_skip("backfill_compound_via_put_schema_hook") else {
                return;
            };
            let col = tasks();
            store
                .put(ctask("t-001", json!({"status": "pending", "priority": 1})))
                .expect("put");
            store
                .put(ctask("t-002", json!({"status": "pending", "priority": 2})))
                .expect("put");

            let mut schema = CollectionSchema::new(col.clone());
            schema.compound_indexes = vec![status_priority_index(false)];
            store.put_schema(&schema).expect("put_schema");

            let prefix = CompoundKey(vec![IndexValue::String("pending".into())]);
            let mut results = store
                .compound_index_prefix(&col, 0, &prefix)
                .expect("prefix");
            results.sort();
            assert_eq!(
                results,
                vec![EntityId::new("t-001"), EntityId::new("t-002")]
            );
        }
    }

    // ── Write-primitive index maintenance (Approach C) ───────────────────
    //
    // Mirrors `SqliteStorageAdapter`'s `primitive_maintenance_tests`: the four
    // write primitives maintain single + compound secondary indexes internally
    // and atomically. Each test provisions a real Postgres container (via
    // `store_or_skip`) and exercises the primitives end-to-end.
    mod primitive_maintenance_tests {
        use super::*;
        use crate::adapter::{CompoundKey, IndexValue};
        use axon_schema::schema::{
            CollectionSchema, CompoundIndexDef, CompoundIndexField, IndexDef, IndexType,
        };
        use serde_json::json;

        fn people() -> CollectionId {
            CollectionId::new("people")
        }

        fn person(id: &str, data: serde_json::Value) -> Entity {
            Entity::new(people(), EntityId::new(id), data)
        }

        fn status_index() -> IndexDef {
            IndexDef {
                field: "status".into(),
                index_type: IndexType::String,
                unique: false,
            }
        }

        fn unique_email_index() -> IndexDef {
            IndexDef {
                field: "email".into(),
                index_type: IndexType::String,
                unique: true,
            }
        }

        fn status_priority_compound() -> CompoundIndexDef {
            CompoundIndexDef {
                fields: vec![
                    CompoundIndexField {
                        field: "status".into(),
                        index_type: IndexType::String,
                    },
                    CompoundIndexField {
                        field: "priority".into(),
                        index_type: IndexType::Integer,
                    },
                ],
                unique: false,
            }
        }

        /// Register a schema declaring the given single + compound indexes.
        fn store_with_indexes(
            store: &mut TestStore,
            single: Vec<IndexDef>,
            compound: Vec<CompoundIndexDef>,
        ) {
            let mut schema = CollectionSchema::new(people());
            schema.indexes = single;
            schema.compound_indexes = compound;
            store.put_schema(&schema).expect("put_schema");
        }

        fn lookup_status(store: &TestStore, status: &str) -> Vec<EntityId> {
            let mut ids = store
                .index_lookup(&people(), "status", &IndexValue::String(status.into()))
                .expect("index_lookup");
            ids.sort();
            ids
        }

        fn count_rows(store: &TestStore, table: &str) -> i64 {
            store
                .block_on(
                    sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                        .fetch_one(&store.pool),
                )
                .expect("count query should succeed")
        }

        #[test]
        fn put_new_entity_is_indexed() {
            let _guard = postgres_test_guard();
            let Some(mut store) = store_or_skip("put_new_entity_is_indexed") else {
                return;
            };
            store_with_indexes(&mut store, vec![status_index()], vec![]);
            store
                .put(person("p-1", json!({"status": "active"})))
                .expect("put");
            assert_eq!(lookup_status(&store, "active"), vec![EntityId::new("p-1")]);
        }

        #[test]
        fn put_replace_moves_index_entry() {
            let _guard = postgres_test_guard();
            let Some(mut store) = store_or_skip("put_replace_moves_index_entry") else {
                return;
            };
            store_with_indexes(&mut store, vec![status_index()], vec![]);
            store
                .put(person("p-1", json!({"status": "active"})))
                .expect("put");
            store
                .put(person("p-1", json!({"status": "archived"})))
                .expect("put replace");
            assert!(lookup_status(&store, "active").is_empty(), "old key gone");
            assert_eq!(
                lookup_status(&store, "archived"),
                vec![EntityId::new("p-1")],
                "new key present"
            );
        }

        #[test]
        fn cas_moves_index_entry() {
            let _guard = postgres_test_guard();
            let Some(mut store) = store_or_skip("cas_moves_index_entry") else {
                return;
            };
            store_with_indexes(&mut store, vec![status_index()], vec![]);
            store
                .put(person("p-1", json!({"status": "active"})))
                .expect("put");
            store
                .compare_and_swap(person("p-1", json!({"status": "archived"})), 1)
                .expect("cas");
            assert!(lookup_status(&store, "active").is_empty());
            assert_eq!(
                lookup_status(&store, "archived"),
                vec![EntityId::new("p-1")]
            );
        }

        #[test]
        fn delete_removes_index_entries() {
            let _guard = postgres_test_guard();
            let Some(mut store) = store_or_skip("delete_removes_index_entries") else {
                return;
            };
            store_with_indexes(&mut store, vec![status_index()], vec![]);
            store
                .put(person("p-1", json!({"status": "active"})))
                .expect("put");
            store
                .delete(&people(), &EntityId::new("p-1"))
                .expect("delete");
            assert!(lookup_status(&store, "active").is_empty());
        }

        #[test]
        fn create_if_absent_maintains_and_noop_does_not_duplicate() {
            let _guard = postgres_test_guard();
            let Some(mut store) =
                store_or_skip("create_if_absent_maintains_and_noop_does_not_duplicate")
            else {
                return;
            };
            store_with_indexes(&mut store, vec![status_index()], vec![]);
            store
                .create_if_absent(person("p-1", json!({"status": "active"})), 0)
                .expect("create_if_absent");
            assert_eq!(lookup_status(&store, "active"), vec![EntityId::new("p-1")]);

            // No-op create (already present) must not duplicate the index row.
            let err = store
                .create_if_absent(person("p-1", json!({"status": "active"})), 0)
                .expect_err("second create_if_absent must conflict");
            assert!(matches!(err, AxonError::ConflictingVersion { .. }));
            assert_eq!(
                lookup_status(&store, "active"),
                vec![EntityId::new("p-1")],
                "no duplicate index row after no-op"
            );
        }

        #[test]
        fn compound_index_maintained_through_put() {
            let _guard = postgres_test_guard();
            let Some(mut store) = store_or_skip("compound_index_maintained_through_put") else {
                return;
            };
            store_with_indexes(&mut store, vec![], vec![status_priority_compound()]);
            store
                .put(person("p-1", json!({"status": "active", "priority": 5})))
                .expect("put");
            let key = CompoundKey(vec![
                IndexValue::String("active".into()),
                IndexValue::Integer(5),
            ]);
            assert_eq!(
                store
                    .compound_index_lookup(&people(), 0, &key)
                    .expect("compound_index_lookup"),
                vec![EntityId::new("p-1")]
            );
        }

        #[test]
        fn unique_violation_on_put_does_not_persist_entity() {
            let _guard = postgres_test_guard();
            let Some(mut store) = store_or_skip("unique_violation_on_put_does_not_persist_entity")
            else {
                return;
            };
            store_with_indexes(&mut store, vec![unique_email_index()], vec![]);
            store
                .put(person("p-1", json!({"email": "a@x.com"})))
                .expect("put p-1");
            let err = store
                .put(person("p-2", json!({"email": "a@x.com"})))
                .expect_err("duplicate unique email must fail");
            assert!(matches!(err, AxonError::UniqueViolation { .. }));
            assert!(
                store
                    .get(&people(), &EntityId::new("p-2"))
                    .expect("get")
                    .is_none(),
                "entity must not be persisted after unique violation"
            );
            assert_eq!(
                store
                    .index_lookup(&people(), "email", &IndexValue::String("a@x.com".into()))
                    .expect("lookup"),
                vec![EntityId::new("p-1")]
            );
        }

        #[test]
        fn unique_violation_on_cas_does_not_persist_entity() {
            let _guard = postgres_test_guard();
            let Some(mut store) = store_or_skip("unique_violation_on_cas_does_not_persist_entity")
            else {
                return;
            };
            store_with_indexes(&mut store, vec![unique_email_index()], vec![]);
            store
                .put(person("p-1", json!({"email": "a@x.com"})))
                .expect("put p-1");
            store
                .put(person("p-2", json!({"email": "b@x.com"})))
                .expect("put p-2");
            let err = store
                .compare_and_swap(person("p-2", json!({"email": "a@x.com"})), 1)
                .expect_err("cas to duplicate email must fail");
            assert!(matches!(err, AxonError::UniqueViolation { .. }));
            let stored = store
                .get(&people(), &EntityId::new("p-2"))
                .expect("get")
                .expect("p-2 still present");
            assert_eq!(stored.version, 1, "version unchanged after failed cas");
            assert_eq!(
                stored.data,
                json!({"email": "b@x.com"}),
                "data unchanged after failed cas"
            );
            assert_eq!(
                store
                    .index_lookup(&people(), "email", &IndexValue::String("b@x.com".into()))
                    .expect("lookup"),
                vec![EntityId::new("p-2")],
                "p-2's original index entry intact"
            );
        }

        #[test]
        fn schemaless_put_writes_without_maintenance() {
            let _guard = postgres_test_guard();
            let Some(mut store) = store_or_skip("schemaless_put_writes_without_maintenance") else {
                return;
            };
            let col = CollectionId::new("no_schema");
            store
                .put(Entity::new(
                    col.clone(),
                    EntityId::new("x-1"),
                    json!({"status": "active"}),
                ))
                .expect("schemaless put must succeed");
            let got = store
                .get(&col, &EntityId::new("x-1"))
                .expect("get")
                .expect("entity present");
            assert_eq!(got.data, json!({"status": "active"}));
            assert_eq!(
                count_rows(&store, "entity_index"),
                0,
                "schemaless write must not write index rows"
            );
        }

        /// Joined path: a mutation inside an API-style `begin_tx` maintains
        /// indexes, and `abort_tx` rolls back BOTH the entity and its index
        /// entries (validates owned-vs-joined ownership — the joined primitive
        /// must not commit/rollback its parent's tx).
        #[test]
        fn joined_abort_rolls_back_entity_and_index() {
            let _guard = postgres_test_guard();
            let Some(mut store) = store_or_skip("joined_abort_rolls_back_entity_and_index") else {
                return;
            };
            store_with_indexes(&mut store, vec![status_index()], vec![]);
            store
                .put(person("p-1", json!({"status": "active"})))
                .expect("seed put");

            store.begin_tx().expect("begin_tx");
            // This put joins the outer tx (no nested BEGIN); it maintains indexes.
            store
                .put(person("p-2", json!({"status": "active"})))
                .expect("joined put");
            assert_eq!(
                lookup_status(&store, "active"),
                vec![EntityId::new("p-1"), EntityId::new("p-2")]
            );
            store.abort_tx().expect("abort_tx");

            assert!(store
                .get(&people(), &EntityId::new("p-2"))
                .expect("get")
                .is_none());
            assert_eq!(
                lookup_status(&store, "active"),
                vec![EntityId::new("p-1")],
                "aborted index entry rolled back with the entity"
            );
        }

        /// Joined path commit: a mutation inside `begin_tx` followed by
        /// `commit_tx` durably maintains both the entity and its index entry.
        #[test]
        fn joined_commit_persists_entity_and_index() {
            let _guard = postgres_test_guard();
            let Some(mut store) = store_or_skip("joined_commit_persists_entity_and_index") else {
                return;
            };
            store_with_indexes(&mut store, vec![status_index()], vec![]);
            store.begin_tx().expect("begin_tx");
            store
                .put(person("p-1", json!({"status": "active"})))
                .expect("joined put");
            store.commit_tx().expect("commit_tx");
            assert_eq!(lookup_status(&store, "active"), vec![EntityId::new("p-1")]);
        }
    }
}

// ── L4 backend conformance — PostgresStorageAdapter ──────────────────────────
//
// Each conformance test provisions a fresh `axon_ct_<n>` database so tests are
// fully isolated even when run in parallel.  Databases accumulate in the test
// cluster; they are small and harmless.  For CI the cluster is ephemeral.
//
// Run locally with a real cluster:
//   AXON_TEST_POSTGRES="postgres://postgres:postgres@localhost/postgres" \
//     cargo test -p axon-storage
//
// Or start a throwaway container:
//   docker run -d -p 5432:5432 -e POSTGRES_PASSWORD=postgres postgres:16
//   AXON_TEST_POSTGRES="postgres://postgres:postgres@localhost/postgres" \
//     cargo test -p axon-storage
//
// The tests are silently skipped when neither AXON_TEST_POSTGRES is set nor
// a Docker container can be started — no test failure is emitted.

#[cfg(test)]
fn pg_conformance_superadmin_dsn() -> Option<String> {
    use std::sync::OnceLock;
    use testcontainers_modules::{postgres, testcontainers::runners::SyncRunner};

    // Cache: None means "already decided no Postgres available".
    static DSN: OnceLock<Option<String>> = OnceLock::new();

    DSN.get_or_init(|| {
        if let Ok(dsn) = std::env::var("AXON_TEST_POSTGRES") {
            return Some(dsn);
        }

        let result = postgres::Postgres::default()
            .with_db_name("postgres")
            .with_user("postgres")
            .with_password("postgres")
            .start();

        match result {
            Ok(container) => {
                let host = container
                    .get_host()
                    .expect("container host should be available");
                let port = container
                    .get_host_port_ipv4(5432)
                    .expect("container port should be available");
                // Leak the container so it lives for the whole test binary run.
                Box::leak(Box::new(container));
                Some(format!(
                    "postgres://postgres:postgres@{host}:{port}/postgres"
                ))
            }
            Err(e) => {
                eprintln!(
                    "[L4 conformance] Skipping PostgreSQL tests: container runtime unavailable ({e}). \
                     Set AXON_TEST_POSTGRES to run against a real cluster."
                );
                None
            }
        }
    })
    .clone()
}

/// Provision a fresh `axon_ct_<n>` database and return a connected adapter.
///
/// Returns `None` when no PostgreSQL cluster is reachable (so conformance
/// tests skip gracefully).
#[cfg(test)]
fn pg_conformance_make_adapter() -> Option<PostgresStorageAdapter> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let dsn = pg_conformance_superadmin_dsn()?;
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let db_name = format!("ct_{}_{n:06}", std::process::id());

    provision_postgres_database(&dsn, &db_name)
        .expect("conformance database provision should succeed");

    let tenant = tenant_dsn(&dsn, &db_name);
    PostgresStorageAdapter::connect(&tenant).ok()
}

// Invoke the L4 conformance suite.  Tests skip gracefully when no Postgres
// cluster is reachable.
crate::storage_conformance_tests!(
    maybe: { super::pg_conformance_make_adapter() },
    postgres_conformance
);
