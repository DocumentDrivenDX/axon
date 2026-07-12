use std::ops::Bound;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use axon_audit::entry::AuditEntry;
use axon_audit::log::{AuditPage, AuditQuery};
use axon_core::auth::{CredentialMetadata, RetentionPolicy, TenantDatabase, TenantId, UserId};
use axon_core::error::AxonError;
use axon_core::id::{
    CollectionId, EntityId, Namespace, QualifiedCollectionId, DEFAULT_DATABASE, DEFAULT_SCHEMA,
};
use axon_core::intent::{ApprovalState, MutationIntent};
use axon_core::types::Entity;
use axon_schema::schema::{CollectionSchema, CollectionView};

use crate::adapter::{filter_audit_entries_for_query, CompoundKey, IndexValue, StorageAdapter};

use crate::adapter::prefix_successor;

/// SQLite-backed storage adapter using an embedded database.
///
/// Uses `sqlx::SqlitePool` with a single connection for serialised access.
/// A dedicated `tokio::runtime::Runtime` bridges async sqlx calls into the
/// synchronous `StorageAdapter` trait.
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
    pool: sqlx::SqlitePool,
    /// Owned runtime — only used when no outer tokio context exists (CLI,
    /// embedded mode).  When constructed inside a gateway handler or
    /// `#[tokio::test]`, this is `None` and the caller's runtime is reused.
    rt: Option<tokio::runtime::Runtime>,
    /// A connection held open for the adapter's entire lifetime.
    ///
    /// Only set for shared-cache in-memory databases. A `cache=shared`,
    /// `mode=memory` SQLite database exists only while at least one connection
    /// to it is open; once every connection closes, SQLite destroys it and the
    /// next connection sees an empty database ("no such table: databases").
    /// The pool's `min_connections` setting is best-effort and can momentarily
    /// drop to zero connections during churn, so we pin one connection here
    /// explicitly to guarantee the database survives for as long as the adapter
    /// does. Never used for queries — only held alive.
    _keepalive: Option<sqlx::sqlite::SqliteConnection>,
    /// `true` while a `BEGIN` has been issued but not yet committed or rolled back.
    in_tx: bool,
}

impl SqliteStorageAdapter {
    /// Run an async future, handling both async and non-async caller contexts.
    fn run_on<T>(
        owned_rt: Option<&tokio::runtime::Runtime>,
        fut: impl std::future::Future<Output = T>,
    ) -> T {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
            Err(_) => owned_rt
                .expect("SqliteStorageAdapter: no tokio runtime available")
                .block_on(fut),
        }
    }

    /// Opens (or creates) a SQLite database at the given path.
    pub fn open(path: &str) -> Result<Self, AxonError> {
        let connect = || async {
            sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(1)
                .connect(&format!("sqlite:{}?mode=rwc", path))
                .await
        };
        let (rt, pool) = match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                let pool = tokio::task::block_in_place(|| handle.block_on(connect()))
                    .map_err(|e| AxonError::Storage(e.to_string()))?;
                (None, pool)
            }
            Err(_) => {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| AxonError::Storage(e.to_string()))?;
                let pool = rt
                    .block_on(connect())
                    .map_err(|e| AxonError::Storage(e.to_string()))?;
                (Some(rt), pool)
            }
        };
        let mut adapter = Self {
            pool,
            rt,
            _keepalive: None,
            in_tx: false,
        };
        adapter.init_schema()?;
        crate::adapter::migrate_legacy_link_keys(&mut adapter)?;
        adapter.backfill_indexes_on_open()?;
        Ok(adapter)
    }

    /// Opens an in-memory SQLite database (useful for testing).
    ///
    /// Each call creates an *isolated* in-memory database, but one whose schema
    /// survives connection churn.
    ///
    /// A plain `sqlite::memory:` URI gives every new physical connection its own
    /// empty database. Combined with a pool that can close and re-open
    /// connections (idle reaping, `min_connections(0)`, or `acquire` opening a
    /// fresh connection), the schema created by `init_schema()` can silently
    /// disappear — surfacing as "no such table: databases" under concurrency.
    ///
    /// To make the in-memory database durable we use a shared-cache, named
    /// in-memory URI: every connection addresses the *same* backing database.
    /// A `cache=shared`, `mode=memory` database exists only while at least one
    /// connection to it is open, so we open one dedicated keepalive connection
    /// and hold it in the adapter for its entire lifetime. The pool's
    /// `min_connections` is best-effort and can momentarily drop to zero
    /// connections during churn, which would destroy the database — the
    /// explicit keepalive connection guarantees it never does. The cache name
    /// is unique per adapter instance, preserving test isolation.
    pub fn open_in_memory() -> Result<Self, AxonError> {
        use sqlx::Connection;

        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let uri = format!("sqlite:file:axon-mem-{id}?mode=memory&cache=shared");

        // Open the keepalive connection first so the shared-cache database
        // exists and stays alive before (and after) the pool connects.
        let keepalive_fut = {
            let uri = uri.clone();
            async move { sqlx::sqlite::SqliteConnection::connect(&uri).await }
        };
        let connect = move || {
            let uri = uri.clone();
            async move {
                sqlx::sqlite::SqlitePoolOptions::new()
                    .max_connections(1)
                    .min_connections(1)
                    .idle_timeout(None)
                    .max_lifetime(None)
                    .test_before_acquire(false)
                    .connect(&uri)
                    .await
            }
        };
        let (rt, keepalive, pool) = match tokio::runtime::Handle::try_current() {
            Ok(handle) => tokio::task::block_in_place(|| {
                handle.block_on(async {
                    let keepalive = keepalive_fut
                        .await
                        .map_err(|e| AxonError::Storage(e.to_string()))?;
                    let pool = connect()
                        .await
                        .map_err(|e| AxonError::Storage(e.to_string()))?;
                    Ok::<_, AxonError>((None, keepalive, pool))
                })
            })?,
            Err(_) => {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| AxonError::Storage(e.to_string()))?;
                let (keepalive, pool) = rt.block_on(async {
                    let keepalive = keepalive_fut
                        .await
                        .map_err(|e| AxonError::Storage(e.to_string()))?;
                    let pool = connect()
                        .await
                        .map_err(|e| AxonError::Storage(e.to_string()))?;
                    Ok::<_, AxonError>((keepalive, pool))
                })?;
                (Some(rt), keepalive, pool)
            }
        };
        let mut adapter = Self {
            pool,
            rt,
            _keepalive: Some(keepalive),
            in_tx: false,
        };
        adapter.init_schema()?;
        crate::adapter::migrate_legacy_link_keys(&mut adapter)?;
        adapter.backfill_indexes_on_open()?;
        Ok(adapter)
    }

    /// Apply auth/tenancy schema migrations to this adapter's SQLite connection.
    ///
    /// Creates the `users`, `user_identities`, `tenant_users`,
    /// `credential_revocations`, and related tables. This is idempotent — it
    /// can be called multiple times safely. Must be called before using any
    /// auth-related adapter methods (`upsert_user_identity`, `is_jti_revoked`,
    /// `get_user`, `get_tenant_member`).
    pub fn apply_auth_migrations(&self) -> Result<(), AxonError> {
        crate::auth_schema::apply_auth_migrations_sqlite(&self.pool, self.rt.as_ref())
            .map_err(AxonError::Storage)
    }

    /// Return the total number of rows in the `users` table (for tests).
    ///
    /// Returns `0` when the auth schema has not been applied.
    pub fn query_user_count(&self) -> Result<i64, AxonError> {
        match self.block_on(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users").fetch_one(&self.pool),
        ) {
            Ok(n) => Ok(n),
            Err(e) if e.to_string().contains("no such table") => Ok(0),
            Err(e) => Err(e),
        }
    }

    /// Return the total number of rows in the `user_identities` table (for tests).
    ///
    /// Returns `0` when the auth schema has not been applied.
    pub fn query_identity_count(&self) -> Result<i64, AxonError> {
        match self.block_on(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM user_identities")
                .fetch_one(&self.pool),
        ) {
            Ok(n) => Ok(n),
            Err(e) if e.to_string().contains("no such table") => Ok(0),
            Err(e) => Err(e),
        }
    }

    /// Insert a tenant and user row for test fixture setup.
    ///
    /// Both rows are inserted with `INSERT OR IGNORE` so the call is
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
                "INSERT OR IGNORE INTO tenants \
                 (id, name, display_name, created_at_ms, updated_at_ms) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
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
                "INSERT OR IGNORE INTO users (id, display_name, created_at_ms) \
                 VALUES (?1, ?2, ?3)",
            )
            .bind(user_id)
            .bind(user_id)
            .bind(now)
            .execute(&self.pool),
        )?;
        Ok(())
    }

    /// Insert a user row for test fixture setup (bootstrap tests).
    ///
    /// Uses `INSERT OR IGNORE` so the call is idempotent. Intended for
    /// integration tests that need a valid FK reference in `users` before
    /// calling `ensure_default_tenant`.
    pub fn test_insert_user(&self, user_id: &str) -> Result<(), AxonError> {
        let now = 1_000_000i64;
        self.block_on(
            sqlx::query(
                "INSERT OR IGNORE INTO users (id, display_name, created_at_ms) \
                 VALUES (?1, ?2, ?3)",
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
        let count: i64 = self.block_on(
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM tenant_users \
                 WHERE tenant_id = ?1 AND user_id = ?2",
            )
            .bind(tenant_id)
            .bind(user_id)
            .fetch_one(&self.pool),
        )?;
        Ok(count)
    }

    /// Bridge async sqlx futures into synchronous results.
    ///
    /// When called from outside any tokio runtime, uses the adapter's own
    /// Run an async sqlx future, bridging into the sync StorageAdapter trait.
    fn block_on<T>(
        &self,
        fut: impl std::future::Future<Output = Result<T, sqlx::Error>>,
    ) -> Result<T, AxonError> {
        Self::run_on(self.rt.as_ref(), fut).map_err(|e| AxonError::Storage(e.to_string()))
    }

    fn init_schema(&self) -> Result<(), AxonError> {
        // sqlx doesn't have execute_batch, so we execute each statement separately.
        // However, we can combine them with semicolons in a single raw query for
        // CREATE TABLE IF NOT EXISTS statements.
        self.block_on(sqlx::query("PRAGMA foreign_keys = ON").execute(&self.pool))?;
        self.block_on(
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS databases (
                    name TEXT NOT NULL PRIMARY KEY
                )",
            )
            .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS namespaces (
                    database_name TEXT NOT NULL,
                    name          TEXT NOT NULL,
                    PRIMARY KEY (database_name, name),
                    FOREIGN KEY (database_name) REFERENCES databases(name) ON DELETE CASCADE
                )",
            )
            .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS entities (
                    collection    TEXT NOT NULL,
                    database_name TEXT NOT NULL DEFAULT 'default',
                    schema_name   TEXT NOT NULL DEFAULT 'default',
                    id            TEXT NOT NULL,
                    version       INTEGER NOT NULL,
                    data          TEXT NOT NULL,
                    PRIMARY KEY (database_name, schema_name, collection, id)
                )",
            )
            .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS schema_versions (
                    collection    TEXT NOT NULL,
                    database_name TEXT NOT NULL DEFAULT 'default',
                    schema_name   TEXT NOT NULL DEFAULT 'default',
                    version       INTEGER NOT NULL,
                    schema_json   TEXT NOT NULL,
                    created_at    INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY (database_name, schema_name, collection, version)
                )",
            )
            .execute(&self.pool),
        )?;
        // Persisted single-field secondary index (FEAT-013). `key` holds the
        // canonical order-preserving bytes from `axon_esf::encode_index_value`;
        // SQLite compares BLOBs bytewise (memcmp), so range scans over `key`
        // honour the same ordering the in-memory adapter's typed `IndexValue`
        // `Ord` produces.
        self.block_on(
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS entity_index (
                    database_name TEXT NOT NULL,
                    schema_name   TEXT NOT NULL,
                    collection    TEXT NOT NULL,
                    field         TEXT NOT NULL,
                    key           BLOB NOT NULL,
                    entity_id     TEXT NOT NULL,
                    PRIMARY KEY (database_name, schema_name, collection, field, key, entity_id)
                )",
            )
            .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "CREATE INDEX IF NOT EXISTS idx_entity_index_range
                 ON entity_index (database_name, schema_name, collection, field, key)",
            )
            .execute(&self.pool),
        )?;
        // Persisted compound secondary index (FEAT-013 / US-033). `key` holds the
        // canonical **framed** composite bytes from
        // `axon_esf::encode_compound_index_key` (a `u32` BE length per field).
        // `index_ordinal` is the compound index's position in the collection
        // schema's `compound_indexes` list, matching how the in-memory adapter
        // keys compound indexes by position. SQLite compares BLOBs bytewise, so a
        // leftmost-prefix match is a byte-prefix range over `key`.
        self.block_on(
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS entity_compound_index (
                    database_name TEXT NOT NULL,
                    schema_name   TEXT NOT NULL,
                    collection    TEXT NOT NULL,
                    index_ordinal INTEGER NOT NULL,
                    key           BLOB NOT NULL,
                    entity_id     TEXT NOT NULL,
                    PRIMARY KEY (database_name, schema_name, collection, index_ordinal, key, entity_id)
                )",
            )
            .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "CREATE INDEX IF NOT EXISTS idx_entity_compound_index_range
                 ON entity_compound_index (database_name, schema_name, collection, index_ordinal, key)",
            )
            .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS collections (
                    name          TEXT NOT NULL,
                    database_name TEXT NOT NULL DEFAULT 'default',
                    schema_name   TEXT NOT NULL DEFAULT 'default',
                    PRIMARY KEY (database_name, schema_name, name)
                )",
            )
            .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS collection_views (
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
                )",
            )
            .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS audit_log (
                    id             INTEGER PRIMARY KEY AUTOINCREMENT,
                    timestamp_ns   INTEGER NOT NULL,
                    collection     TEXT NOT NULL,
                    entity_id      TEXT NOT NULL,
                    version        INTEGER NOT NULL,
                    mutation       TEXT NOT NULL,
                    actor          TEXT NOT NULL,
                    transaction_id TEXT,
                    entry_json     TEXT NOT NULL
                )",
            )
            .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS mutation_intents (
                    tenant_id      TEXT NOT NULL,
                    database_id    TEXT NOT NULL,
                    intent_id      TEXT NOT NULL,
                    decision       TEXT NOT NULL,
                    approval_state TEXT NOT NULL,
                    expires_at_ns  INTEGER NOT NULL,
                    intent_json    TEXT NOT NULL,
                    PRIMARY KEY (tenant_id, database_id, intent_id)
                )",
            )
            .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "CREATE INDEX IF NOT EXISTS idx_mutation_intents_pending
                 ON mutation_intents
                    (tenant_id, database_id, approval_state, expires_at_ns, intent_id)",
            )
            .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "CREATE INDEX IF NOT EXISTS idx_mutation_intents_expired
                 ON mutation_intents
                    (tenant_id, database_id, expires_at_ns, approval_state, intent_id)",
            )
            .execute(&self.pool),
        )?;
        self.ensure_namespace_catalog_tables()?;
        self.ensure_default_namespace()
    }

    fn collection_exists_in_namespace(
        &self,
        collection: &CollectionId,
        namespace: &Namespace,
    ) -> Result<bool, AxonError> {
        let exists: i64 = self.block_on(
            sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM collections
                    WHERE name = ?1 AND database_name = ?2 AND schema_name = ?3
                )",
            )
            .bind(collection.as_str())
            .bind(namespace.database.as_str())
            .bind(namespace.schema.as_str())
            .fetch_one(&self.pool),
        )?;
        Ok(exists != 0)
    }

    fn database_exists(&self, database: &str) -> Result<bool, AxonError> {
        let exists: i64 = self.block_on(
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM databases WHERE name = ?1)")
                .bind(database)
                .fetch_one(&self.pool),
        )?;
        Ok(exists != 0)
    }

    fn namespace_exists(&self, namespace: &Namespace) -> Result<bool, AxonError> {
        let exists: i64 = self.block_on(
            sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM namespaces
                    WHERE database_name = ?1 AND name = ?2
                )",
            )
            .bind(namespace.database.as_str())
            .bind(namespace.schema.as_str())
            .fetch_one(&self.pool),
        )?;
        Ok(exists != 0)
    }

    fn table_info(&self, table: &str) -> Result<Vec<(String, i64)>, AxonError> {
        use sqlx::Row;
        let rows = self
            .block_on(sqlx::query(&format!("PRAGMA table_info({table})")).fetch_all(&self.pool))?;
        let mut result = Vec::new();
        for row in rows {
            let name: String = row.get(1);
            let pk: i64 = row.get(5);
            result.push((name, pk));
        }
        Ok(result)
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
        self.block_on(
            sqlx::query("ALTER TABLE collections RENAME TO collections_legacy").execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "CREATE TABLE collections (
                     name          TEXT NOT NULL,
                     database_name TEXT NOT NULL DEFAULT 'default',
                     schema_name   TEXT NOT NULL DEFAULT 'default',
                     PRIMARY KEY (database_name, schema_name, name)
                 )",
            )
            .execute(&self.pool),
        )?;

        match (has_database_name, has_schema_name) {
            (true, true) => self.block_on(
                sqlx::query(
                    "INSERT OR IGNORE INTO collections (name, database_name, schema_name)
                     SELECT name, COALESCE(database_name, 'default'), COALESCE(schema_name, 'default')
                     FROM collections_legacy",
                )
                .execute(&self.pool),
            )?,
            (true, false) => self.block_on(
                sqlx::query(
                    "INSERT OR IGNORE INTO collections (name, database_name, schema_name)
                     SELECT name, COALESCE(database_name, 'default'), 'default'
                     FROM collections_legacy",
                )
                .execute(&self.pool),
            )?,
            (false, true) => self.block_on(
                sqlx::query(
                    "INSERT OR IGNORE INTO collections (name, database_name, schema_name)
                     SELECT name, 'default', COALESCE(schema_name, 'default')
                     FROM collections_legacy",
                )
                .execute(&self.pool),
            )?,
            (false, false) => self.block_on(
                sqlx::query(
                    "INSERT OR IGNORE INTO collections (name, database_name, schema_name)
                     SELECT name, 'default', 'default'
                     FROM collections_legacy",
                )
                .execute(&self.pool),
            )?,
        };
        self.block_on(sqlx::query("DROP TABLE collections_legacy").execute(&self.pool))?;
        Ok(())
    }

    fn rebuild_entities_table(
        &self,
        has_database_name: bool,
        has_schema_name: bool,
    ) -> Result<(), AxonError> {
        self.block_on(
            sqlx::query("ALTER TABLE entities RENAME TO entities_legacy").execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "CREATE TABLE entities (
                     collection    TEXT NOT NULL,
                     database_name TEXT NOT NULL DEFAULT 'default',
                     schema_name   TEXT NOT NULL DEFAULT 'default',
                     id            TEXT NOT NULL,
                     version       INTEGER NOT NULL,
                     data          TEXT NOT NULL,
                     PRIMARY KEY (database_name, schema_name, collection, id)
                 )",
            )
            .execute(&self.pool),
        )?;

        match (has_database_name, has_schema_name) {
            (true, true) => self.block_on(
                sqlx::query(
                    "INSERT OR REPLACE INTO entities
                        (collection, database_name, schema_name, id, version, data)
                     SELECT collection, COALESCE(database_name, 'default'), COALESCE(schema_name, 'default'), id, version, data
                     FROM entities_legacy",
                )
                .execute(&self.pool),
            )?,
            (true, false) => self.block_on(
                sqlx::query(
                    "INSERT OR REPLACE INTO entities
                        (collection, database_name, schema_name, id, version, data)
                     SELECT collection, COALESCE(database_name, 'default'), 'default', id, version, data
                     FROM entities_legacy",
                )
                .execute(&self.pool),
            )?,
            (false, true) => self.block_on(
                sqlx::query(
                    "INSERT OR REPLACE INTO entities
                        (collection, database_name, schema_name, id, version, data)
                     SELECT collection, 'default', COALESCE(schema_name, 'default'), id, version, data
                     FROM entities_legacy",
                )
                .execute(&self.pool),
            )?,
            (false, false) => self.block_on(
                sqlx::query(
                    "INSERT OR REPLACE INTO entities
                        (collection, database_name, schema_name, id, version, data)
                     SELECT collection, 'default', 'default', id, version, data
                     FROM entities_legacy",
                )
                .execute(&self.pool),
            )?,
        };
        self.block_on(sqlx::query("DROP TABLE entities_legacy").execute(&self.pool))?;
        Ok(())
    }

    fn rebuild_schema_versions_table(
        &self,
        has_database_name: bool,
        has_schema_name: bool,
    ) -> Result<(), AxonError> {
        self.block_on(
            sqlx::query("ALTER TABLE schema_versions RENAME TO schema_versions_legacy")
                .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "CREATE TABLE schema_versions (
                     collection    TEXT NOT NULL,
                     database_name TEXT NOT NULL DEFAULT 'default',
                     schema_name   TEXT NOT NULL DEFAULT 'default',
                     version       INTEGER NOT NULL,
                     schema_json   TEXT NOT NULL,
                     created_at    INTEGER NOT NULL DEFAULT 0,
                     PRIMARY KEY (database_name, schema_name, collection, version)
                 )",
            )
            .execute(&self.pool),
        )?;

        match (has_database_name, has_schema_name) {
            (true, true) => self.block_on(
                sqlx::query(
                    "INSERT INTO schema_versions
                        (collection, database_name, schema_name, version, schema_json, created_at)
                     SELECT collection,
                            COALESCE(database_name, 'default'),
                            COALESCE(schema_name, 'default'),
                            version,
                            schema_json,
                            created_at
                     FROM schema_versions_legacy",
                )
                .execute(&self.pool),
            )?,
            (true, false) => self.block_on(
                sqlx::query(
                    "INSERT INTO schema_versions
                        (collection, database_name, schema_name, version, schema_json, created_at)
                     SELECT collection,
                            COALESCE(database_name, 'default'),
                            'default',
                            version,
                            schema_json,
                            created_at
                     FROM schema_versions_legacy",
                )
                .execute(&self.pool),
            )?,
            (false, true) => self.block_on(
                sqlx::query(
                    "INSERT INTO schema_versions
                        (collection, database_name, schema_name, version, schema_json, created_at)
                     SELECT collection,
                            'default',
                            COALESCE(schema_name, 'default'),
                            version,
                            schema_json,
                            created_at
                     FROM schema_versions_legacy",
                )
                .execute(&self.pool),
            )?,
            (false, false) => self.block_on(
                sqlx::query(
                    "INSERT INTO schema_versions
                        (collection, database_name, schema_name, version, schema_json, created_at)
                     SELECT collection, 'default', 'default', version, schema_json, created_at
                     FROM schema_versions_legacy",
                )
                .execute(&self.pool),
            )?,
        };
        self.block_on(sqlx::query("DROP TABLE schema_versions_legacy").execute(&self.pool))?;
        Ok(())
    }

    fn rebuild_collection_views_table(
        &self,
        has_database_name: bool,
        has_schema_name: bool,
    ) -> Result<(), AxonError> {
        self.block_on(
            sqlx::query("ALTER TABLE collection_views RENAME TO collection_views_legacy")
                .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "CREATE TABLE collection_views (
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
                 )",
            )
            .execute(&self.pool),
        )?;

        if has_database_name && has_schema_name {
            self.block_on(
                sqlx::query(
                    "INSERT OR REPLACE INTO collection_views
                        (collection, database_name, schema_name, version, view_json, updated_at_ns, updated_by)
                     SELECT collection,
                            COALESCE(database_name, 'default'),
                            COALESCE(schema_name, 'default'),
                            version,
                            view_json,
                            updated_at_ns,
                            updated_by
                     FROM collection_views_legacy",
                )
                .execute(&self.pool),
            )?;
        } else {
            self.block_on(
                sqlx::query(
                    "INSERT OR REPLACE INTO collection_views
                        (collection, database_name, schema_name, version, view_json, updated_at_ns, updated_by)
                     SELECT v.collection,
                            COALESCE(c.database_name, 'default'),
                            COALESCE(c.schema_name, 'default'),
                            v.version,
                            v.view_json,
                            v.updated_at_ns,
                            v.updated_by
                     FROM collection_views_legacy v
                     LEFT JOIN collections c ON c.name = v.collection",
                )
                .execute(&self.pool),
            )?;
        }
        self.block_on(sqlx::query("DROP TABLE collection_views_legacy").execute(&self.pool))?;
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
            self.block_on(
                sqlx::query(
                    "CREATE INDEX IF NOT EXISTS idx_collections_namespace
                     ON collections (database_name, schema_name, name)",
                )
                .execute(&self.pool),
            )?;
            return Ok(());
        }

        self.block_on(sqlx::query("PRAGMA foreign_keys = OFF").execute(&self.pool))?;

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

        self.block_on(
            sqlx::query(
                "CREATE INDEX IF NOT EXISTS idx_collections_namespace
                 ON collections (database_name, schema_name, name)",
            )
            .execute(&self.pool),
        )?;
        self.block_on(sqlx::query("PRAGMA foreign_keys = ON").execute(&self.pool))?;
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
                 WHERE name = ?1
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
        let namespaces = rows
            .iter()
            .map(|row| {
                let db: String = row.get("database_name");
                let schema: String = row.get("schema_name");
                Namespace::new(db, schema)
            })
            .collect();
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
        use sqlx::Row;
        let rows = self.block_on(
            sqlx::query(
                "SELECT name FROM collections
                 WHERE database_name = ?1 AND schema_name = ?2
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
                QualifiedCollectionId::from_parts(namespace, &CollectionId::new(name))
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
                 WHERE database_name = ?1
                 ORDER BY schema_name ASC, name ASC",
            )
            .bind(database)
            .fetch_all(&self.pool),
        )?;
        Ok(rows
            .iter()
            .map(|row| {
                let schema: String = row.get("schema_name");
                let collection: String = row.get("name");
                QualifiedCollectionId::from_parts(
                    &Namespace::new(database, schema),
                    &CollectionId::new(collection),
                )
            })
            .collect())
    }

    fn ensure_default_namespace(&self) -> Result<(), AxonError> {
        self.block_on(
            sqlx::query("INSERT OR IGNORE INTO databases (name) VALUES (?1)")
                .bind(DEFAULT_DATABASE)
                .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query("INSERT OR IGNORE INTO namespaces (database_name, name) VALUES (?1, ?2)")
                .bind(DEFAULT_DATABASE)
                .bind(DEFAULT_SCHEMA)
                .execute(&self.pool),
        )?;
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

    fn row_to_mutation_intent(row: &sqlx::sqlite::SqliteRow) -> Result<MutationIntent, AxonError> {
        use sqlx::Row;
        let intent_json: String = row.get("intent_json");
        serde_json::from_str(&intent_json).map_err(AxonError::Serialization)
    }

    /// Run a raw SQL query and return the count (as i64).
    /// Used in tests that need to verify row counts directly.
    pub fn query_scalar_i64(&self, sql: &str) -> Result<i64, AxonError> {
        self.block_on(sqlx::query_scalar::<_, i64>(sql).fetch_one(&self.pool))
    }

    // ── Persisted secondary index helpers (FEAT-013) ─────────────────────

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
                 WHERE database_name = ?1 AND schema_name = ?2 AND collection = ?3
                   AND field = ?4 AND entity_id = ?5",
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
                    "INSERT OR REPLACE INTO entity_index
                        (database_name, schema_name, collection, field, key, entity_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
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
        index_ordinal: i64,
        entity_id: &EntityId,
    ) -> Result<(), AxonError> {
        self.block_on(
            sqlx::query(
                "DELETE FROM entity_compound_index
                 WHERE database_name = ?1 AND schema_name = ?2 AND collection = ?3
                   AND index_ordinal = ?4 AND entity_id = ?5",
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
        index_ordinal: i64,
        entity_id: &EntityId,
        framed: &[u8],
    ) -> Result<(), AxonError> {
        self.block_on(
            sqlx::query(
                "INSERT OR REPLACE INTO entity_compound_index
                    (database_name, schema_name, collection, index_ordinal, key, entity_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
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
    // is sufficient (this is why the SQLite primitives, unlike the in-memory
    // ones, do not capture a prior image).

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
        let found: Option<i64> = self.block_on(
            sqlx::query_scalar(
                "SELECT 1 FROM entity_index
                 WHERE database_name = ?1 AND schema_name = ?2 AND collection = ?3
                   AND field = ?4 AND key = ?5 AND entity_id <> ?6
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
            let found: Option<i64> = self.block_on(
                sqlx::query_scalar(
                    "SELECT 1 FROM entity_compound_index
                     WHERE database_name = ?1 AND schema_name = ?2 AND collection = ?3
                       AND index_ordinal = ?4 AND key = ?5 AND entity_id <> ?6
                     LIMIT 1",
                )
                .bind(key.namespace.database.as_str())
                .bind(key.namespace.schema.as_str())
                .bind(key.collection.as_str())
                .bind(ordinal as i64)
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
                self.delete_compound_index_rows(key, ordinal as i64, entity_id)?;
            }
        }

        for (ordinal, idx) in indexes.iter().enumerate() {
            let Ok(Some(framed)) = idx.index_key(new_data) else {
                continue;
            };
            self.insert_compound_index_row(key, ordinal as i64, entity_id, &framed)?;
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
            self.delete_compound_index_rows(key, ordinal as i64, entity_id)?;
        }
        Ok(())
    }

    /// Maintain ALL secondary indexes (single + compound) for a write, looking
    /// up the entity's index defs from its stamped schema version. Returns the
    /// resolved defs so the caller can tell whether any maintenance ran.
    fn maintain_indexes_for_write_at(
        &self,
        key: &QualifiedCollectionId,
        collection: &CollectionId,
        entity_id: &EntityId,
        schema_version: Option<u32>,
        had_old: bool,
        new_data: &serde_json::Value,
    ) -> Result<(), AxonError> {
        let (single, compound) = self.index_defs_for_entity(collection, schema_version)?;
        if single.is_empty() && compound.is_empty() {
            return Ok(());
        }
        self.maintain_single_indexes_at(key, entity_id, had_old, new_data, &single)?;
        self.maintain_compound_indexes_at(key, entity_id, had_old, new_data, &compound)
    }

    /// Issue a `BEGIN IMMEDIATE` only if we don't already own / sit inside a
    /// transaction. Returns `true` when this call started its own transaction
    /// (and is therefore responsible for committing / rolling it back).
    fn begin_if_needed(&mut self) -> Result<bool, AxonError> {
        if self.in_tx {
            return Ok(false);
        }
        self.block_on(sqlx::query("BEGIN IMMEDIATE").execute(&self.pool))?;
        self.in_tx = true;
        Ok(true)
    }

    /// Commit an owned transaction started by [`Self::begin_if_needed`].
    fn commit_owned(&mut self) -> Result<(), AxonError> {
        self.block_on(sqlx::query("COMMIT").execute(&self.pool))?;
        self.in_tx = false;
        Ok(())
    }

    /// Roll back an owned transaction started by [`Self::begin_if_needed`].
    /// Best-effort: clears `in_tx` even if the ROLLBACK itself errors.
    fn rollback_owned(&mut self) {
        let _ = self.block_on(sqlx::query("ROLLBACK").execute(&self.pool));
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
    /// [`crate::extract_index_key_bytes`]. Correct over an empty collection
    /// (nothing to scan) and idempotent (the leading delete clears any prior
    /// rows). Used both when a schema is (re)registered with indexes and for
    /// the open-time backfill of pre-existing entities.
    pub fn reindex_collection(
        &mut self,
        collection: &CollectionId,
        indexes: &[axon_schema::schema::IndexDef],
    ) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        // Clear any existing rows for this collection first.
        self.block_on(
            sqlx::query(
                "DELETE FROM entity_index
                 WHERE database_name = ?1 AND schema_name = ?2 AND collection = ?3",
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
    /// [`axon_schema::schema::CompoundIndexDef::index_key`], inserting one row
    /// per entity whose key is non-sparse. Idempotent and correct over an empty
    /// collection. A type-mismatch in a field is treated as not-indexed (the
    /// `Err` from `index_key` is mapped to "no row"), matching FEAT-013's typed
    /// in-memory adapter semantics.
    pub fn reindex_compound_collection(
        &mut self,
        collection: &CollectionId,
        indexes: &[axon_schema::schema::CompoundIndexDef],
    ) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        self.block_on(
            sqlx::query(
                "DELETE FROM entity_compound_index
                 WHERE database_name = ?1 AND schema_name = ?2 AND collection = ?3",
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
                // A type-mismatch (`Err`) is "not indexed" here; only a
                // non-sparse `Ok(Some(_))` produces a row.
                if let Ok(Some(framed)) = idx.index_key(&entity.data) {
                    self.insert_compound_index_row(&key, ordinal as i64, &entity.id, &framed)?;
                }
            }
        }
        Ok(())
    }

    /// One-time open-time backfill: for every collection whose latest schema
    /// declares single-field indexes but which has no `entity_index` rows yet,
    /// rebuild its index. Guarded so it never redoes work for collections that
    /// already have rows. Correct (and a no-op) over an empty database.
    fn backfill_indexes_on_open(&mut self) -> Result<(), AxonError> {
        let collections = self.list_collections()?;
        for collection in collections {
            let Some(schema) = self.get_schema(&collection)? else {
                continue;
            };
            let key = self.resolve_catalog_key(&collection)?;

            // Single-field index backfill (guarded so it never redoes work).
            if !schema.indexes.is_empty() {
                let existing: i64 = self.block_on(
                    sqlx::query_scalar(
                        "SELECT COUNT(*) FROM entity_index
                         WHERE database_name = ?1 AND schema_name = ?2 AND collection = ?3",
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

            // Compound index backfill (independently guarded).
            if !schema.compound_indexes.is_empty() {
                let existing: i64 = self.block_on(
                    sqlx::query_scalar(
                        "SELECT COUNT(*) FROM entity_compound_index
                         WHERE database_name = ?1 AND schema_name = ?2 AND collection = ?3",
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

impl StorageAdapter for SqliteStorageAdapter {
    fn resolve_collection_key(
        &self,
        collection: &CollectionId,
    ) -> Result<QualifiedCollectionId, AxonError> {
        self.resolve_catalog_key(collection)
    }

    fn get(&self, collection: &CollectionId, id: &EntityId) -> Result<Option<Entity>, AxonError> {
        use sqlx::Row;
        let key = self.resolve_catalog_key(collection)?;
        let row = self.block_on(
            sqlx::query(
                "SELECT collection, id, version, data FROM entities
                 WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3 AND id = ?4",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .bind(id.as_str())
            .fetch_optional(&self.pool),
        )?;

        match row {
            Some(row) => {
                let collection_str: String = row.get("collection");
                let id_str: String = row.get("id");
                let version: i64 = row.get("version");
                let data_str: String = row.get("data");
                let entity = Self::row_to_entity(collection_str, id_str, version as u64, data_str)?;
                Ok(Some(entity))
            }
            None => Ok(None),
        }
    }

    fn put(&mut self, entity: Entity) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(&entity.collection)?;
        let data_json = serde_json::to_string(&entity.data)?;

        // Resolve the entity's index defs up front. With no indexes this is a
        // single entity-write statement, as before — no per-write tx overhead
        // and schemaless writes are unaffected.
        let (single, compound) =
            self.index_defs_for_entity(&entity.collection, entity.schema_version)?;
        if single.is_empty() && compound.is_empty() {
            self.block_on(
                sqlx::query(
                    "INSERT OR REPLACE INTO entities (collection, database_name, schema_name, id, version, data)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )
                .bind(key.collection.as_str())
                .bind(key.namespace.database.as_str())
                .bind(key.namespace.schema.as_str())
                .bind(entity.id.as_str())
                .bind(entity.version as i64)
                .bind(&data_json)
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
                &entity.collection,
                &entity.id,
                entity.schema_version,
                true,
                &entity.data,
            )?;
            self.block_on(
                sqlx::query(
                    "INSERT OR REPLACE INTO entities (collection, database_name, schema_name, id, version, data)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )
                .bind(key.collection.as_str())
                .bind(key.namespace.database.as_str())
                .bind(key.namespace.schema.as_str())
                .bind(entity.id.as_str())
                .bind(entity.version as i64)
                .bind(&data_json)
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
                     WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3 AND id = ?4",
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
                     WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3 AND id = ?4",
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
        let n: i64 = self.block_on(
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM entities
                 WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .fetch_one(&self.pool),
        )?;
        Ok(n as usize)
    }

    /// Native membership signature (ADR-026 phantom guard): fetch the ordered
    /// id-set and hash it via the shared [`crate::hash_id_set`]. Read-only (no
    /// write-path cost); transfers ids only, not full rows. Membership-only —
    /// changes on create/delete, stable across updates and reads. Runs within
    /// the active transaction during commit validation, so it sees the correct
    /// snapshot.
    fn structural_version(&self, collection: &CollectionId) -> Result<u64, AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        let ids: Vec<String> = self.block_on(
            sqlx::query_scalar(
                "SELECT id FROM entities
                 WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3
                 ORDER BY id",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .fetch_all(&self.pool),
        )?;
        Ok(crate::hash_id_set(&ids))
    }

    /// Native content signature (ADR-026 strict guard, FEAT-008 TXN-05
    /// `SerializableStrict`): fetch the ordered `(id, version)` set and hash it
    /// via the shared [`crate::hash_id_version_set`]. Like
    /// [`Self::structural_version`] this is read-only and transfers ids+versions
    /// only (not full row JSON), but it is **version-inclusive**, so it also
    /// changes on in-place updates — catching update-driven predicate skew the
    /// membership signature misses. Runs within the active transaction during
    /// commit validation, so it sees the correct snapshot.
    fn content_version(&self, collection: &CollectionId) -> Result<u64, AxonError> {
        use sqlx::Row;
        let key = self.resolve_catalog_key(collection)?;
        let rows = self.block_on(
            sqlx::query(
                "SELECT id, version FROM entities
                 WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3
                 ORDER BY id",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .fetch_all(&self.pool),
        )?;
        let pairs: Vec<(String, u64)> = rows
            .into_iter()
            .map(|r| {
                let id: String = r.get("id");
                let version: i64 = r.get("version");
                (id, version as u64)
            })
            .collect();
        Ok(crate::hash_id_version_set(&pairs))
    }

    fn range_scan(
        &self,
        collection: &CollectionId,
        start: Option<&EntityId>,
        end: Option<&EntityId>,
        limit: Option<usize>,
    ) -> Result<Vec<Entity>, AxonError> {
        use sqlx::Row;
        let key = self.resolve_catalog_key(collection)?;
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

        let rows = self.block_on(
            sqlx::query(sql)
                .bind(key.collection.as_str())
                .bind(key.namespace.database.as_str())
                .bind(key.namespace.schema.as_str())
                .bind(&start_str)
                .bind(&end_str)
                .bind(limit_val)
                .fetch_all(&self.pool),
        )?;

        let mut entities = Vec::new();
        for row in &rows {
            let col: String = row.get("collection");
            let id: String = row.get("id");
            let version: i64 = row.get("version");
            let data_json: String = row.get("data");
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
        let new_version = expected_version + 1;
        let data_json = serde_json::to_string(&entity.data)?;

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
                    "UPDATE entities SET version = ?1, data = ?2
                     WHERE collection = ?3 AND database_name = ?4 AND schema_name = ?5 AND id = ?6 AND version = ?7",
                )
                .bind(new_version as i64)
                .bind(&data_json)
                .bind(key.collection.as_str())
                .bind(key.namespace.database.as_str())
                .bind(key.namespace.schema.as_str())
                .bind(entity.id.as_str())
                .bind(expected_version as i64)
                .execute(&self.pool),
            )?;

            if res.rows_affected() == 0 {
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
        let data_json = serde_json::to_string(&entity.data)?;

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
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(database_name, schema_name, collection, id) DO NOTHING",
                )
                .bind(key.collection.as_str())
                .bind(key.namespace.database.as_str())
                .bind(key.namespace.schema.as_str())
                .bind(entity.id.as_str())
                .bind(entity.version as i64)
                .bind(&data_json)
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
        self.block_on(sqlx::query("BEGIN IMMEDIATE").execute(&self.pool))?;
        self.in_tx = true;
        Ok(())
    }

    fn commit_tx(&mut self) -> Result<(), AxonError> {
        if !self.in_tx {
            return Err(AxonError::Storage("no active transaction".into()));
        }
        self.block_on(sqlx::query("COMMIT").execute(&self.pool))?;
        self.in_tx = false;
        Ok(())
    }

    fn abort_tx(&mut self) -> Result<(), AxonError> {
        if !self.in_tx {
            return Ok(());
        }
        self.block_on(sqlx::query("ROLLBACK").execute(&self.pool))?;
        self.in_tx = false;
        Ok(())
    }

    fn create_mutation_intent(&mut self, intent: &MutationIntent) -> Result<(), AxonError> {
        let intent_json = serde_json::to_string(intent)?;
        let result = self.block_on(
            sqlx::query(
                "INSERT OR IGNORE INTO mutation_intents
                    (tenant_id, database_id, intent_id, decision, approval_state, expires_at_ns, intent_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
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
                 WHERE tenant_id = ?1 AND database_id = ?2 AND intent_id = ?3",
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
                 WHERE tenant_id = ?1
                   AND database_id = ?2
                   AND approval_state = ?3
                   AND expires_at_ns > ?4
                 ORDER BY expires_at_ns ASC, intent_id ASC
                 LIMIT ?5",
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
                 WHERE tenant_id = ?1
                   AND database_id = ?2
                   AND expires_at_ns <= ?3
                   AND approval_state IN (?4, ?5, ?6)
                 ORDER BY expires_at_ns ASC, intent_id ASC
                 LIMIT ?7",
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
                 WHERE tenant_id = ?1
                   AND database_id = ?2
                   AND approval_state = ?3
                 ORDER BY expires_at_ns ASC, intent_id ASC
                 LIMIT ?4",
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
        let intent_json = serde_json::to_string(&intent)?;
        let result = self.block_on(
            sqlx::query(
                "UPDATE mutation_intents
                 SET approval_state = ?1, intent_json = ?2
                 WHERE tenant_id = ?3
                   AND database_id = ?4
                   AND intent_id = ?5
                   AND approval_state = ?6",
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
            sqlx::query("INSERT INTO databases (name) VALUES (?1)")
                .bind(name)
                .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query("INSERT INTO namespaces (database_name, name) VALUES (?1, ?2)")
                .bind(name)
                .bind(DEFAULT_SCHEMA)
                .execute(&self.pool),
        )?;
        Ok(())
    }

    fn list_databases(&self) -> Result<Vec<String>, AxonError> {
        let databases: Vec<String> = self.block_on(
            sqlx::query_scalar("SELECT name FROM databases ORDER BY name ASC")
                .fetch_all(&self.pool),
        )?;
        Ok(databases)
    }

    fn drop_database(&mut self, name: &str) -> Result<(), AxonError> {
        if !self.database_exists(name)? {
            return Err(AxonError::NotFound(format!("database '{name}'")));
        }

        let doomed = self.database_collection_keys(name)?;
        self.purge_links_for_collections(&doomed)?;
        self.block_on(
            sqlx::query("DELETE FROM entities WHERE database_name = ?1")
                .bind(name)
                .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query("DELETE FROM collection_views WHERE database_name = ?1")
                .bind(name)
                .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query("DELETE FROM schema_versions WHERE database_name = ?1")
                .bind(name)
                .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query("DELETE FROM collections WHERE database_name = ?1")
                .bind(name)
                .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query("DELETE FROM databases WHERE name = ?1")
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
            sqlx::query("INSERT INTO namespaces (database_name, name) VALUES (?1, ?2)")
                .bind(namespace.database.as_str())
                .bind(namespace.schema.as_str())
                .execute(&self.pool),
        )?;
        Ok(())
    }

    fn list_namespaces(&self, database: &str) -> Result<Vec<String>, AxonError> {
        if !self.database_exists(database)? {
            return Err(AxonError::NotFound(format!("database '{database}'")));
        }

        let namespaces: Vec<String> = self.block_on(
            sqlx::query_scalar(
                "SELECT name FROM namespaces
                 WHERE database_name = ?1
                 ORDER BY name ASC",
            )
            .bind(database)
            .fetch_all(&self.pool),
        )?;
        Ok(namespaces)
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
                 WHERE database_name = ?1 AND schema_name = ?2",
            )
            .bind(namespace.database.as_str())
            .bind(namespace.schema.as_str())
            .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "DELETE FROM collection_views
                 WHERE database_name = ?1 AND schema_name = ?2",
            )
            .bind(namespace.database.as_str())
            .bind(namespace.schema.as_str())
            .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "DELETE FROM schema_versions
                 WHERE database_name = ?1 AND schema_name = ?2",
            )
            .bind(namespace.database.as_str())
            .bind(namespace.schema.as_str())
            .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "DELETE FROM collections
                 WHERE database_name = ?1 AND schema_name = ?2",
            )
            .bind(namespace.database.as_str())
            .bind(namespace.schema.as_str())
            .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "DELETE FROM namespaces
                 WHERE database_name = ?1 AND name = ?2",
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
        if !self.namespace_exists(namespace)? {
            return Err(AxonError::NotFound(format!("namespace '{namespace}'")));
        }

        let names: Vec<String> = self.block_on(
            sqlx::query_scalar(
                "SELECT name FROM collections
                 WHERE database_name = ?1 AND schema_name = ?2
                 ORDER BY name ASC",
            )
            .bind(namespace.database.as_str())
            .bind(namespace.schema.as_str())
            .fetch_all(&self.pool),
        )?;
        Ok(names.into_iter().map(CollectionId::new).collect())
    }

    fn append_audit_entry(&mut self, mut entry: AuditEntry) -> Result<AuditEntry, AxonError> {
        use sqlx::Row;

        // Assign timestamp if the caller left it at the zero sentinel.
        if entry.timestamp_ns == 0 {
            entry.timestamp_ns = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0);
        }

        let entry_json =
            serde_json::to_string(&entry).map_err(|e| AxonError::Storage(e.to_string()))?;

        self.block_on(
            sqlx::query(
                "INSERT INTO audit_log
                     (timestamp_ns, collection, entity_id, version, mutation, actor,
                      transaction_id, entry_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )
            .bind(entry.timestamp_ns as i64)
            .bind(entry.collection.as_str())
            .bind(entry.entity_id.as_str())
            .bind(entry.version as i64)
            .bind(entry.mutation.to_string())
            .bind(&entry.actor)
            .bind(&entry.transaction_id)
            .bind(&entry_json)
            .execute(&self.pool),
        )?;

        // Retrieve the last inserted rowid
        let row = self.block_on(sqlx::query("SELECT last_insert_rowid()").fetch_one(&self.pool))?;
        entry.id = row.get::<i64, _>(0) as u64;
        Ok(entry)
    }

    fn supports_durable_audit(&self) -> bool {
        true
    }

    fn audit_len(&self) -> Result<usize, AxonError> {
        let count: i64 = self
            .block_on(sqlx::query_scalar("SELECT COUNT(*) FROM audit_log").fetch_one(&self.pool))?;
        Ok(count as usize)
    }

    fn query_audit_paginated(&self, query: AuditQuery) -> Result<AuditPage, AxonError> {
        use sqlx::Row;

        let after_id = query.after_id.unwrap_or(0) as i64;
        let rows = self.block_on(
            sqlx::query(
                "SELECT id, timestamp_ns, entry_json
                 FROM audit_log
                 WHERE id > ?1
                 ORDER BY id ASC",
            )
            .bind(after_id)
            .fetch_all(&self.pool),
        )?;

        let mut entries = Vec::with_capacity(rows.len());
        for row in rows {
            let id = row.get::<i64, _>("id") as u64;
            let timestamp_ns = row.get::<i64, _>("timestamp_ns") as u64;
            let entry_json = row.get::<String, _>("entry_json");
            let mut entry: AuditEntry = serde_json::from_str(&entry_json)?;
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
                 WHERE id = ?1",
            )
            .bind(id as i64)
            .fetch_optional(&self.pool),
        )?;
        row.map(|row| {
            let row_id = row.get::<i64, _>("id") as u64;
            let timestamp_ns = row.get::<i64, _>("timestamp_ns") as u64;
            let entry_json = row.get::<String, _>("entry_json");
            let mut entry: AuditEntry = serde_json::from_str(&entry_json)?;
            entry.id = row_id;
            entry.timestamp_ns = timestamp_ns;
            Ok(entry)
        })
        .transpose()
    }

    fn put_schema(&mut self, schema: &CollectionSchema) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(&schema.collection)?;
        // Auto-increment: find current max version for this collection.
        let max_version: i64 = self.block_on(
            sqlx::query_scalar(
                "SELECT COALESCE(MAX(version), 0) FROM schema_versions
                 WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .fetch_one(&self.pool),
        )?;
        let next_version = max_version + 1;

        let mut versioned = schema.clone();
        versioned.collection = key.collection.clone();
        versioned.version = next_version as u32;
        let schema_json = serde_json::to_string(&versioned)?;

        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as i64;

        self.block_on(
            sqlx::query(
                "INSERT INTO schema_versions
                    (collection, database_name, schema_name, version, schema_json, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .bind(next_version)
            .bind(&schema_json)
            .bind(now_ns)
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
                "SELECT schema_json FROM schema_versions
                 WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3
                 ORDER BY version DESC LIMIT 1",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .fetch_optional(&self.pool),
        )?;

        match row {
            Some(row) => {
                let json: String = row.get("schema_json");
                let schema: CollectionSchema = serde_json::from_str(&json)?;
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
                "SELECT schema_json FROM schema_versions
                 WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3 AND version = ?4",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .bind(version as i64)
            .fetch_optional(&self.pool),
        )?;

        match row {
            Some(row) => {
                let json: String = row.get("schema_json");
                let schema: CollectionSchema = serde_json::from_str(&json)?;
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
                "SELECT version, created_at FROM schema_versions
                 WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3
                 ORDER BY version ASC",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .fetch_all(&self.pool),
        )?;

        let mut result = vec![];
        for row in &rows {
            let version: i64 = row.get("version");
            let created_at: i64 = row.get("created_at");
            result.push((version as u32, created_at as u64));
        }
        Ok(result)
    }

    fn delete_schema(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        self.block_on(
            sqlx::query(
                "DELETE FROM schema_versions
                 WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .execute(&self.pool),
        )?;
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
            .block_on(
                sqlx::query_scalar(
                    "SELECT COALESCE(version, 0) FROM collection_views
                     WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3",
                )
                .bind(key.collection.as_str())
                .bind(key.namespace.database.as_str())
                .bind(key.namespace.schema.as_str())
                .fetch_one(&self.pool),
            )
            .unwrap_or_default();
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

        self.block_on(
            sqlx::query(
                "INSERT INTO collection_views
                    (collection, database_name, schema_name, version, view_json, updated_at_ns, updated_by)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(database_name, schema_name, collection) DO UPDATE SET
                     version = excluded.version,
                     view_json = excluded.view_json,
                     updated_at_ns = excluded.updated_at_ns,
                     updated_by = excluded.updated_by",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .bind(next_version)
            .bind(&view_json)
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
                 WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3",
            )
            .bind(key.collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .fetch_optional(&self.pool),
        )?;

        match row {
            Some(row) => {
                let json: String = row.get("view_json");
                let view: CollectionView = serde_json::from_str(&json)?;
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
                 WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3",
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
                "INSERT OR IGNORE INTO collections (name, database_name, schema_name)
                 VALUES (?1, ?2, ?3)",
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
        // Older SQLite databases may have `collection_views` without the
        // `ON DELETE CASCADE` foreign key. Delete the dependent row explicitly
        // so upgraded databases do not retain stale collection views.
        self.block_on(
            sqlx::query(
                "DELETE FROM collection_views
                 WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3",
            )
            .bind(collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "DELETE FROM schema_versions
                 WHERE collection = ?1 AND database_name = ?2 AND schema_name = ?3",
            )
            .bind(collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "DELETE FROM collections
                 WHERE name = ?1 AND database_name = ?2 AND schema_name = ?3",
            )
            .bind(collection.as_str())
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .execute(&self.pool),
        )?;
        Ok(())
    }

    fn list_collections(&self) -> Result<Vec<CollectionId>, AxonError> {
        let names: Vec<String> = self.block_on(
            sqlx::query_scalar(
                "SELECT name FROM collections
                 ORDER BY database_name ASC, schema_name ASC, name ASC",
            )
            .fetch_all(&self.pool),
        )?;
        Ok(names.into_iter().map(CollectionId::new).collect())
    }

    fn collection_registered_in_namespace(
        &self,
        collection: &CollectionId,
        namespace: &Namespace,
    ) -> Result<bool, AxonError> {
        self.collection_exists_in_namespace(collection, namespace)
    }

    // ── Persisted secondary index operations (FEAT-013) ──────────────────
    //
    // Keys are the canonical order-preserving bytes from
    // `axon_esf::encode_index_value`, so equality/range/unique lookups behave
    // identically to `MemoryStorageAdapter`. Array (`field[]`) indexes produce
    // one row per scalar item; null/missing/type-mismatch values are skipped
    // (not errors). All writes run through the pool's single connection, so
    // they automatically participate in any active `BEGIN IMMEDIATE`
    // transaction and roll back together on `abort_tx`.

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
                 WHERE database_name = ?1 AND schema_name = ?2 AND collection = ?3
                   AND field = ?4 AND key = ?5
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

        // Build the bound clauses dynamically, omitting unbounded sides. BLOB
        // comparison in SQLite is bytewise (memcmp), matching the canonical
        // order-preserving key encoding.
        let mut sql = String::from(
            "SELECT entity_id, key FROM entity_index
             WHERE database_name = ?1 AND schema_name = ?2 AND collection = ?3 AND field = ?4",
        );
        // An unencodable bound value carries no usable ordering, so we treat
        // that side as unbounded (`None`) rather than erroring.
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
                Bound::Excluded(_) => sql.push_str(" AND key > ?5"),
                _ => sql.push_str(" AND key >= ?5"),
            }
        }
        if upper_bytes.is_some() {
            // Upper placeholder index depends on whether lower was bound.
            let ph = if lower_bytes.is_some() { "?6" } else { "?5" };
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
        let found: Option<i64> = self.block_on(
            sqlx::query_scalar(
                "SELECT 1 FROM entity_index
                 WHERE database_name = ?1 AND schema_name = ?2 AND collection = ?3
                   AND field = ?4 AND key = ?5 AND entity_id <> ?6
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
                 WHERE database_name = ?1 AND schema_name = ?2 AND collection = ?3",
            )
            .bind(key.namespace.database.as_str())
            .bind(key.namespace.schema.as_str())
            .bind(key.collection.as_str())
            .execute(&self.pool),
        )?;
        self.block_on(
            sqlx::query(
                "DELETE FROM entity_compound_index
                 WHERE database_name = ?1 AND schema_name = ?2 AND collection = ?3",
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
        // An unencodable key cannot match any stored row.
        let Ok(Some(framed)) = key.encode_framed() else {
            return Ok(vec![]);
        };
        let ids: Vec<String> = self.block_on(
            sqlx::query_scalar(
                "SELECT entity_id FROM entity_compound_index
                 WHERE database_name = ?1 AND schema_name = ?2 AND collection = ?3
                   AND index_ordinal = ?4 AND key = ?5
                 ORDER BY entity_id ASC",
            )
            .bind(cat.namespace.database.as_str())
            .bind(cat.namespace.schema.as_str())
            .bind(cat.collection.as_str())
            .bind(index_idx as i64)
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
             WHERE database_name = ?1 AND schema_name = ?2 AND collection = ?3
               AND index_ordinal = ?4 AND key >= ?5",
        );
        if upper.is_some() {
            sql.push_str(" AND key < ?6");
        }
        sql.push_str(" ORDER BY key ASC, entity_id ASC");

        let mut query = sqlx::query(&sql)
            .bind(cat.namespace.database.as_str().to_owned())
            .bind(cat.namespace.schema.as_str().to_owned())
            .bind(cat.collection.as_str().to_owned())
            .bind(index_idx as i64)
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

    // ── Auth / tenancy (ADR-018) ─────────────────────────────────────────────

    fn is_jti_revoked(&self, jti: uuid::Uuid) -> Result<bool, axon_core::error::AxonError> {
        match self.block_on(
            sqlx::query("SELECT 1 FROM credential_revocations WHERE jti = ?1")
                .bind(jti.to_string())
                .fetch_optional(&self.pool),
        ) {
            Ok(Some(_)) => Ok(true),
            Ok(None) => Ok(false),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("no such table") {
                    Ok(false)
                } else {
                    Err(e)
                }
            }
        }
    }

    fn get_user(
        &self,
        user_id: axon_core::auth::UserId,
    ) -> Result<Option<axon_core::auth::User>, axon_core::error::AxonError> {
        use sqlx::Row;
        match self.block_on(
            sqlx::query(
                "SELECT id, display_name, email, created_at_ms, suspended_at_ms \
                 FROM users WHERE id = ?1",
            )
            .bind(user_id.as_str())
            .fetch_optional(&self.pool),
        ) {
            Ok(Some(row)) => {
                let user = axon_core::auth::User {
                    id: axon_core::auth::UserId::new(row.get::<String, _>("id")),
                    display_name: row.get("display_name"),
                    email: row.get("email"),
                    created_at_ms: row.get::<i64, _>("created_at_ms") as u64,
                    suspended_at_ms: row
                        .get::<Option<i64>, _>("suspended_at_ms")
                        .map(|v| v as u64),
                };
                Ok(Some(user))
            }
            Ok(None) => Ok(None),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("no such table") {
                    Ok(None)
                } else {
                    Err(e)
                }
            }
        }
    }

    fn get_tenant_member(
        &self,
        tenant_id: axon_core::auth::TenantId,
        user_id: axon_core::auth::UserId,
    ) -> Result<Option<axon_core::auth::TenantMember>, axon_core::error::AxonError> {
        use sqlx::Row;
        match self.block_on(
            sqlx::query(
                "SELECT tenant_id, user_id, role FROM tenant_users \
                 WHERE tenant_id = ?1 AND user_id = ?2",
            )
            .bind(tenant_id.as_str())
            .bind(user_id.as_str())
            .fetch_optional(&self.pool),
        ) {
            Ok(Some(row)) => {
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
            Ok(None) => Ok(None),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("no such table") {
                    Ok(None)
                } else {
                    Err(e)
                }
            }
        }
    }

    fn upsert_user_identity(
        &self,
        provider: &str,
        external_id: &str,
        display_name: &str,
        email: Option<&str>,
    ) -> Result<axon_core::auth::User, AxonError> {
        use sqlx::Row;

        // Check whether this identity already exists.
        let existing_user_id: Option<String> = match self.block_on(
            sqlx::query_scalar(
                "SELECT user_id FROM user_identities WHERE provider = ?1 AND external_id = ?2",
            )
            .bind(provider)
            .bind(external_id)
            .fetch_optional(&self.pool),
        ) {
            Ok(id) => id,
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("no such table") {
                    return Err(AxonError::Storage(
                        "auth schema not applied; call apply_auth_migrations first".into(),
                    ));
                }
                return Err(e);
            }
        };

        let user_id_str = if let Some(id) = existing_user_id {
            id
        } else {
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64;
            let new_id = axon_core::auth::UserId::generate();
            self.block_on(
                sqlx::query(
                    "INSERT INTO users (id, display_name, email, created_at_ms) \
                     VALUES (?1, ?2, ?3, ?4)",
                )
                .bind(new_id.as_str())
                .bind(display_name)
                .bind(email)
                .bind(now_ms)
                .execute(&self.pool),
            )?;
            self.block_on(
                sqlx::query(
                    "INSERT INTO user_identities (provider, external_id, user_id, created_at_ms) \
                     VALUES (?1, ?2, ?3, ?4)",
                )
                .bind(provider)
                .bind(external_id)
                .bind(new_id.as_str())
                .bind(now_ms)
                .execute(&self.pool),
            )?;
            new_id.0
        };

        let row = self.block_on(
            sqlx::query(
                "SELECT id, display_name, email, created_at_ms, suspended_at_ms \
                 FROM users WHERE id = ?1",
            )
            .bind(&user_id_str)
            .fetch_one(&self.pool),
        )?;
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

    fn create_user(
        &self,
        id: &axon_core::auth::UserId,
        display_name: &str,
        email: Option<&str>,
    ) -> Result<axon_core::auth::User, AxonError> {
        use sqlx::Row;
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let result = self.block_on(
            sqlx::query(
                "INSERT INTO users (id, display_name, email, created_at_ms) \
                 VALUES (?1, ?2, ?3, ?4)",
            )
            .bind(id.as_str())
            .bind(display_name)
            .bind(email)
            .bind(now_ms)
            .execute(&self.pool),
        );
        match result {
            Ok(r) => {
                if r.rows_affected() == 0 {
                    return Err(AxonError::AlreadyExists(format!(
                        "user '{}' already exists",
                        id.as_str()
                    )));
                }
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("UNIQUE constraint failed") {
                    return Err(AxonError::AlreadyExists(format!(
                        "user '{}' already exists",
                        id.as_str()
                    )));
                } else if msg.contains("no such table") {
                    return Err(AxonError::Storage(
                        "auth schema not applied; call apply_auth_migrations first".into(),
                    ));
                }
                return Err(e);
            }
        }
        let row = self.block_on(
            sqlx::query(
                "SELECT id, display_name, email, created_at_ms, suspended_at_ms \
                 FROM users WHERE id = ?1",
            )
            .bind(id.as_str())
            .fetch_one(&self.pool),
        )?;
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
        let rows = match self.block_on(
            sqlx::query(
                "SELECT id, display_name, email, created_at_ms, suspended_at_ms \
                 FROM users ORDER BY created_at_ms DESC",
            )
            .fetch_all(&self.pool),
        ) {
            Ok(rows) => rows,
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("no such table") {
                    return Err(AxonError::Storage("auth schema not applied".into()));
                }
                return Err(e);
            }
        };

        let mut users = Vec::new();
        for row in &rows {
            users.push(axon_core::auth::User {
                id: axon_core::auth::UserId::new(row.get::<String, _>("id")),
                display_name: row.get("display_name"),
                email: row.get("email"),
                created_at_ms: row.get::<i64, _>("created_at_ms") as u64,
                suspended_at_ms: row
                    .get::<Option<i64>, _>("suspended_at_ms")
                    .map(|v| v as u64),
            });
        }
        Ok(users)
    }

    fn suspend_user(&self, id: &axon_core::auth::UserId) -> Result<bool, AxonError> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let result = self.block_on(
            sqlx::query("UPDATE users SET suspended_at_ms = ?1 WHERE id = ?2")
                .bind(now_ms)
                .bind(id.as_str())
                .execute(&self.pool),
        )?;
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
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let result = self.block_on(
            sqlx::query(
                "INSERT INTO tenant_users (tenant_id, user_id, role, added_at_ms) \
                 VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT (tenant_id, user_id) DO UPDATE SET role = excluded.role",
            )
            .bind(tenant_id.as_str())
            .bind(user_id.as_str())
            .bind(role_str)
            .bind(now_ms)
            .execute(&self.pool),
        );
        match result {
            Ok(_) => {}
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("no such table") {
                    return Err(AxonError::Storage(
                        "auth schema not applied; call apply_auth_migrations first".into(),
                    ));
                }
                return Err(e);
            }
        }
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
        let result = self.block_on(
            sqlx::query("DELETE FROM tenant_users WHERE tenant_id = ?1 AND user_id = ?2")
                .bind(tenant_id.as_str())
                .bind(user_id.as_str())
                .execute(&self.pool),
        )?;
        Ok(result.rows_affected() > 0)
    }

    fn list_tenant_members(
        &self,
        tenant_id: axon_core::auth::TenantId,
    ) -> Result<Vec<axon_core::auth::TenantMember>, AxonError> {
        use sqlx::Row;
        let rows = match self.block_on(
            sqlx::query(
                "SELECT tenant_id, user_id, role, added_at_ms \
                 FROM tenant_users \
                 WHERE tenant_id = ?1 \
                 ORDER BY added_at_ms ASC",
            )
            .bind(tenant_id.as_str())
            .fetch_all(&self.pool),
        ) {
            Ok(rows) => rows,
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("no such table") {
                    return Err(AxonError::Storage("auth schema not applied".into()));
                }
                return Err(e);
            }
        };

        let mut members = Vec::new();
        for row in &rows {
            let role_str: String = row.get("role");
            let role = match role_str.as_str() {
                "admin" => axon_core::auth::TenantRole::Admin,
                "write" => axon_core::auth::TenantRole::Write,
                _ => axon_core::auth::TenantRole::Read,
            };
            members.push(axon_core::auth::TenantMember {
                tenant_id: axon_core::auth::TenantId::new(row.get::<String, _>("tenant_id")),
                user_id: axon_core::auth::UserId::new(row.get::<String, _>("user_id")),
                role,
            });
        }
        Ok(members)
    }

    fn count_tenants(&self) -> Result<usize, AxonError> {
        match self.block_on(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tenants").fetch_one(&self.pool),
        ) {
            Ok(n) => Ok(n as usize),
            Err(e) if e.to_string().contains("no such table") => Ok(0),
            Err(e) => Err(e),
        }
    }

    fn upsert_default_tenant(&self, name: &str) -> Result<axon_core::auth::TenantId, AxonError> {
        use sqlx::Row;
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let new_id = axon_core::auth::TenantId::generate();
        let result = self.block_on(
            sqlx::query(
                "INSERT INTO tenants (id, name, display_name, created_at_ms, updated_at_ms) \
                 VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT (name) DO NOTHING",
            )
            .bind(new_id.as_str())
            .bind(name)
            .bind(name)
            .bind(now_ms)
            .bind(now_ms)
            .execute(&self.pool),
        );
        match result {
            Ok(_) => {}
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("no such table") {
                    return Err(AxonError::Storage(
                        "auth schema not applied; call apply_auth_migrations first".into(),
                    ));
                }
                return Err(e);
            }
        }
        let row = self.block_on(
            sqlx::query("SELECT id FROM tenants WHERE name = ?1")
                .bind(name)
                .fetch_one(&self.pool),
        )?;
        let id_str: String = row.get("id");
        Ok(axon_core::auth::TenantId::new(id_str))
    }

    fn get_retention_policy(
        &self,
        tenant_id: axon_core::auth::TenantId,
    ) -> Result<Option<RetentionPolicy>, AxonError> {
        use sqlx::Row;
        match self.block_on(
            sqlx::query(
                "SELECT archive_after_seconds, purge_after_seconds \
                 FROM tenant_retention_policies \
                 WHERE tenant_id = ?1",
            )
            .bind(tenant_id.as_str())
            .fetch_optional(&self.pool),
        ) {
            Ok(Some(row)) => Ok(Some(RetentionPolicy {
                archive_after_seconds: row.get::<i64, _>("archive_after_seconds") as u64,
                purge_after_seconds: row
                    .get::<Option<i64>, _>("purge_after_seconds")
                    .map(|v| v as u64),
            })),
            Ok(None) => Ok(None),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("no such table") {
                    Ok(None)
                } else {
                    Err(e)
                }
            }
        }
    }

    fn set_retention_policy(
        &self,
        tenant_id: axon_core::auth::TenantId,
        policy: &RetentionPolicy,
    ) -> Result<(), AxonError> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let result = self.block_on(
            sqlx::query(
                "INSERT INTO tenant_retention_policies \
                 (tenant_id, archive_after_seconds, purge_after_seconds, updated_at_ms) \
                 VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT (tenant_id) DO UPDATE SET \
                 archive_after_seconds = excluded.archive_after_seconds, \
                 purge_after_seconds = excluded.purge_after_seconds, \
                 updated_at_ms = excluded.updated_at_ms",
            )
            .bind(tenant_id.as_str())
            .bind(policy.archive_after_seconds as i64)
            .bind(policy.purge_after_seconds.map(|v| v as i64))
            .bind(now_ms)
            .execute(&self.pool),
        );
        match result {
            Ok(_) => Ok(()),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("no such table") {
                    Err(AxonError::Storage(
                        "auth schema not applied; call apply_auth_migrations first".into(),
                    ))
                } else {
                    Err(e)
                }
            }
        }
    }

    fn list_tenant_databases(
        &self,
        tenant_id: axon_core::auth::TenantId,
    ) -> Result<Vec<TenantDatabase>, AxonError> {
        use sqlx::Row;
        let rows = match self.block_on(
            sqlx::query(
                "SELECT tenant_id, database_name, created_at_ms \
                 FROM tenant_databases \
                 WHERE tenant_id = ?1 \
                 ORDER BY created_at_ms ASC",
            )
            .bind(tenant_id.as_str())
            .fetch_all(&self.pool),
        ) {
            Ok(rows) => rows,
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("no such table") {
                    return Err(AxonError::Storage("auth schema not applied".into()));
                }
                return Err(e);
            }
        };

        let mut dbs = Vec::new();
        for row in &rows {
            dbs.push(TenantDatabase {
                tenant_id: axon_core::auth::TenantId::new(row.get::<String, _>("tenant_id")),
                name: row.get("database_name"),
                created_at_ms: row.get::<i64, _>("created_at_ms") as u64,
            });
        }
        Ok(dbs)
    }

    fn create_tenant_database(
        &self,
        tenant_id: axon_core::auth::TenantId,
        name: &str,
    ) -> Result<TenantDatabase, AxonError> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let result = self.block_on(
            sqlx::query(
                "INSERT INTO tenant_databases (tenant_id, database_name, created_at_ms) \
                 VALUES (?1, ?2, ?3) \
                 ON CONFLICT (tenant_id, database_name) DO NOTHING",
            )
            .bind(tenant_id.as_str())
            .bind(name)
            .bind(now_ms)
            .execute(&self.pool),
        );
        match result {
            Ok(r) => {
                if r.rows_affected() == 0 {
                    return Err(AxonError::AlreadyExists(format!(
                        "database '{}' already exists in tenant '{}'",
                        name,
                        tenant_id.as_str()
                    )));
                }
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("no such table") {
                    return Err(AxonError::Storage(
                        "auth schema not applied; call apply_auth_migrations first".into(),
                    ));
                }
                return Err(e);
            }
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
        let result = self.block_on(
            sqlx::query("DELETE FROM tenant_databases WHERE tenant_id = ?1 AND database_name = ?2")
                .bind(tenant_id.as_str())
                .bind(name)
                .execute(&self.pool),
        )?;
        Ok(result.rows_affected() > 0)
    }

    fn track_credential_issuance(
        &self,
        jti: uuid::Uuid,
        user_id: UserId,
        tenant_id: TenantId,
        issued_at_ms: i64,
        expires_at_ms: i64,
        grants_json: &str,
    ) -> Result<(), AxonError> {
        self.block_on(
            sqlx::query(
                "INSERT INTO credential_issuances \
                 (jti, user_id, tenant_id, issued_at_ms, expires_at_ms, grants_json) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .bind(jti.to_string())
            .bind(user_id.as_str())
            .bind(tenant_id.as_str())
            .bind(issued_at_ms)
            .bind(expires_at_ms)
            .bind(grants_json)
            .execute(&self.pool),
        )?;
        Ok(())
    }

    fn list_credentials(
        &self,
        tenant_id: TenantId,
        user_filter: Option<UserId>,
    ) -> Result<Vec<CredentialMetadata>, AxonError> {
        use sqlx::Row;
        let rows = self.block_on(
            sqlx::query(
                "SELECT ci.jti, ci.user_id, ci.tenant_id, ci.issued_at_ms, ci.expires_at_ms, \
                 ci.grants_json, \
                 CASE WHEN cr.jti IS NOT NULL THEN 1 ELSE 0 END AS revoked \
                 FROM credential_issuances ci \
                 LEFT JOIN credential_revocations cr ON ci.jti = cr.jti \
                 WHERE ci.tenant_id = ?1 \
                 ORDER BY ci.issued_at_ms ASC",
            )
            .bind(tenant_id.as_str())
            .fetch_all(&self.pool),
        )?;

        let mut creds = Vec::new();
        for row in &rows {
            let meta = CredentialMetadata {
                jti: row.get::<String, _>("jti"),
                user_id: UserId::new(row.get::<String, _>("user_id")),
                tenant_id: TenantId::new(row.get::<String, _>("tenant_id")),
                issued_at_ms: row.get::<i64, _>("issued_at_ms"),
                expires_at_ms: row.get::<i64, _>("expires_at_ms"),
                grants_json: row.get::<String, _>("grants_json"),
                revoked: row.get::<i64, _>("revoked") != 0,
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

    fn revoke_credential(&self, jti: uuid::Uuid, revoked_by: UserId) -> Result<(), AxonError> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        self.block_on(
            sqlx::query(
                "INSERT INTO credential_revocations (jti, revoked_at_ms, revoked_by) \
                 VALUES (?1, ?2, ?3) \
                 ON CONFLICT (jti) DO NOTHING",
            )
            .bind(jti.to_string())
            .bind(now_ms)
            .bind(revoked_by.as_str())
            .execute(&self.pool),
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon_core::intent::{
        CanonicalOperationMetadata, MutationIntentDecision, MutationIntentScopeBinding,
        MutationIntentSubjectBinding, MutationOperationKind, MutationReviewSummary,
    };
    use axon_core::types::{Link, LinkKey};
    use serde_json::{json, Value};
    use tempfile::NamedTempFile;

    fn tasks() -> CollectionId {
        CollectionId::new("tasks")
    }

    fn entity(id: &str) -> Entity {
        Entity::new(tasks(), EntityId::new(id), json!({"title": id}))
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
                canonical_operation: Some(json!({"id": intent_id})),
            },
            pre_images: Vec::new(),
            decision: MutationIntentDecision::NeedsApproval,
            approval_state: ApprovalState::Pending,
            approval_route: None,
            expires_at: 2_000,
            review_summary: MutationReviewSummary::default(),
        }
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
        // Use sqlx to create a legacy database for migration testing.
        let file = NamedTempFile::new().expect("test temp db should be created");
        let path = file.path().to_string_lossy().into_owned();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build");
        let pool = rt
            .block_on(
                sqlx::sqlite::SqlitePoolOptions::new()
                    .max_connections(1)
                    .connect(&format!("sqlite:{}?mode=rwc", path)),
            )
            .expect("test temp db connection should be opened");
        rt.block_on(sqlx::query("PRAGMA foreign_keys = ON").execute(&pool))
            .expect("pragma should succeed");
        rt.block_on(
            sqlx::query(
                "CREATE TABLE collections (
                    name TEXT NOT NULL PRIMARY KEY
                )",
            )
            .execute(&pool),
        )
        .expect("legacy collections table should be created");
        rt.block_on(
            sqlx::query(
                "CREATE TABLE collection_views (
                    collection        TEXT NOT NULL PRIMARY KEY,
                    version           INTEGER NOT NULL,
                    view_json         TEXT NOT NULL,
                    updated_at_ns     INTEGER NOT NULL,
                    updated_by        TEXT
                )",
            )
            .execute(&pool),
        )
        .expect("legacy collection_views table should be created");
        let view_json = serde_json::to_string(&CollectionView::new(collection.clone(), template))
            .expect("legacy collection view should serialize");
        rt.block_on(
            sqlx::query("INSERT INTO collections (name) VALUES (?1)")
                .bind(collection.as_str())
                .execute(&pool),
        )
        .expect("legacy collection should be inserted");
        rt.block_on(
            sqlx::query(
                "INSERT INTO collection_views (collection, version, view_json, updated_at_ns, updated_by)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .bind(collection.as_str())
            .bind(1_i64)
            .bind(&view_json)
            .bind(0_i64)
            .bind(Option::<&str>::None)
            .execute(&pool),
        )
        .expect("legacy collection view should be inserted");
        // Close pool before returning so the file can be reopened.
        rt.block_on(pool.close());
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
        for e in [
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
            s.put(e).expect("entity put should succeed");
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
        for e in [
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
            s.put(e).expect("entity put should succeed");
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
    fn mutation_intents_persist_across_reopen() {
        let file = NamedTempFile::new().expect("temp sqlite file should be created");
        let path = file.path().to_str().expect("temp path should be utf-8");

        {
            let mut s = SqliteStorageAdapter::open(path).expect("sqlite file should open");
            s.create_mutation_intent(&intent("mint-reopen"))
                .expect("intent create should succeed");
            s.update_mutation_intent_state(
                "tenant-a",
                "finance",
                "mint-reopen",
                ApprovalState::Pending,
                ApprovalState::Approved,
            )
            .expect("intent state update should succeed");
        }

        let reopened = SqliteStorageAdapter::open(path).expect("sqlite file should reopen");
        let stored = reopened
            .get_mutation_intent("tenant-a", "finance", "mint-reopen")
            .expect("intent lookup should succeed")
            .expect("intent should persist");
        assert_eq!(stored.approval_state, ApprovalState::Approved);
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
    fn native_content_version_is_version_inclusive_unlike_membership() {
        // The native SQLite content_version push-down (ADR-026 strict guard) must
        // move on an in-place update, while the native structural_version
        // (membership) stays put — pinning the strict-vs-plain distinction at the
        // adapter layer, not just on the by-scan default.
        let mut s = store();
        s.put(entity("t-001")).expect("put");
        s.put(entity("t-002")).expect("put");

        let membership_before = s.structural_version(&tasks()).expect("structural");
        let content_before = s.content_version(&tasks()).expect("content");

        // In-place update: membership unchanged, version bumps.
        s.compare_and_swap(entity("t-001"), 1).expect("cas");

        let membership_after = s.structural_version(&tasks()).expect("structural");
        let content_after = s.content_version(&tasks()).expect("content");

        assert_eq!(
            membership_before, membership_after,
            "membership signature must be stable across an in-place update"
        );
        assert_ne!(
            content_before, content_after,
            "content signature must change on an in-place update"
        );
    }

    #[test]
    fn native_content_version_matches_by_scan_default() {
        // The push-down must agree with the generic by-scan default on the same
        // state, so the strict guard is consistent regardless of which path runs.
        let mut s = store();
        s.put(entity("a")).expect("put");
        s.put(entity("b")).expect("put");
        s.compare_and_swap(entity("a"), 1).expect("cas");

        let native = s.content_version(&tasks()).expect("native content");
        let by_scan = crate::content_version_by_scan(&s, &tasks()).expect("by-scan content");
        assert_eq!(native, by_scan, "native content_version must match by-scan");
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
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
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
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };
        let v2 = CollectionSchema {
            collection: col.clone(),
            description: Some("v2".into()),
            version: 2,
            entity_schema: None,
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
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
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
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
        use sqlx::Row;
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
                json!({"type": "object", "properties": {"amount": {"type": "number"}}}),
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

        let stored_collections: Vec<String> = {
            let rows = s
                .block_on(
                    sqlx::query(
                        "SELECT collection FROM schema_versions
                         WHERE database_name = ?1 AND schema_name = ?2
                         ORDER BY version ASC",
                    )
                    .bind(billing.database.as_str())
                    .bind(billing.schema.as_str())
                    .fetch_all(&s.pool),
                )
                .expect("schema version query should succeed");
            rows.iter()
                .map(|row| row.get::<String, _>("collection"))
                .collect()
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
        use sqlx::Row;
        let mut s = store();
        let qualified = CollectionId::new("prod.billing.invoices");
        let (billing, invoices) = register_unique_namespaced_collection(&mut s, &qualified);

        let stored = s
            .put_collection_view(&CollectionView::new(qualified, "# {{title}}"))
            .expect("qualified collection view put should succeed");
        assert_eq!(stored.collection, invoices);
        assert_eq!(stored.version, 1);

        let row = s
            .block_on(
                sqlx::query(
                    "SELECT collection FROM collection_views
                     WHERE database_name = ?1 AND schema_name = ?2",
                )
                .bind(billing.database.as_str())
                .bind(billing.schema.as_str())
                .fetch_one(&s.pool),
            )
            .expect("stored collection view lookup should succeed");
        let stored_collection: String = row.get("collection");
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
        let count = s
            .query_scalar_i64("SELECT COUNT(*) FROM audit_log")
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
        let count = s
            .query_scalar_i64("SELECT COUNT(*) FROM audit_log")
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

    /// Regression: the in-memory database's schema must survive connection
    /// churn. A plain `sqlite::memory:` pool can hand out a freshly-opened
    /// (empty) connection, dropping the `databases` table created by
    /// `init_schema()` and surfacing as "no such table: databases" under
    /// concurrency. The shared-cache + pinned-connection setup must keep the
    /// schema durable across many operations and creations.
    #[test]
    fn in_memory_schema_survives_repeated_database_operations() {
        let mut s = SqliteStorageAdapter::open_in_memory().expect("open in-memory");
        // Each of these touches the `databases` table; a lost schema would
        // surface as "no such table: databases".
        for i in 0..50 {
            let name = format!("db-{i}");
            assert!(!s.database_exists(&name).expect("database_exists must work"));
            s.create_database(&name).expect("create_database must work");
            assert!(s.database_exists(&name).expect("database_exists must work"));
        }
        // 50 created here plus the seeded default database from init_schema.
        assert_eq!(
            s.list_databases().expect("list_databases must work").len(),
            51
        );
    }

    /// Two independent in-memory adapters must NOT share state — the unique
    /// per-instance shared-cache name preserves test isolation.
    #[test]
    fn in_memory_adapters_are_isolated() {
        let mut a = SqliteStorageAdapter::open_in_memory().expect("open a");
        let b = SqliteStorageAdapter::open_in_memory().expect("open b");
        a.create_database("only-in-a")
            .expect("create_database must work");
        assert!(a.database_exists("only-in-a").expect("a sees its own db"));
        assert!(
            !b.database_exists("only-in-a").expect("b query must work"),
            "adapter b must not see adapter a's database (isolation)"
        );
    }

    /// Persisted secondary index tests (FEAT-013). These mirror the in-memory
    /// adapter's `index_tests` to guarantee identical equality/range/unique/
    /// array/null semantics, plus backfill coverage unique to a persisted store.
    mod index_tests {
        use super::*;
        use crate::adapter::IndexValue;
        use axon_schema::schema::{CollectionSchema, IndexDef, IndexType};

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

        /// Register a schema on `tasks()` declaring the given single indexes,
        /// then return the put-driven store. Index maintenance is performed by
        /// the write primitives from the stamped schema.
        fn store_with_indexes(single: Vec<IndexDef>) -> SqliteStorageAdapter {
            let mut store = store();
            let mut schema = CollectionSchema::new(tasks());
            schema.indexes = single;
            store.put_schema(&schema).expect("put_schema");
            store
        }

        #[test]
        fn null_and_type_mismatch_values_are_not_indexed() {
            let mut store = store_with_indexes(vec![status_index()]);
            let col = tasks();
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
            // No rows at all were written for the missing/type-mismatch entities.
            let n = store
                .query_scalar_i64("SELECT COUNT(*) FROM entity_index")
                .expect("count");
            assert_eq!(n, 0, "null/missing/type-mismatch must not be indexed");
        }

        #[test]
        fn nested_field_path_indexing() {
            let idx = IndexDef {
                field: "address.city".into(),
                index_type: IndexType::String,
                unique: false,
            };
            let mut store = store_with_indexes(vec![idx]);
            let col = tasks();
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
            let mut store = store_with_indexes(vec![unique_email_index()]);
            let col = tasks();
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
            let mut store = store_with_indexes(vec![status_index()]);
            let col = tasks();
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
            // Entities written BEFORE the index exists must still be found
            // after the index is built from the existing data.
            let mut store = store();
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
            // Registering a schema that declares indexes backfills existing rows.
            let mut store = store();
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
            let mut store = store();
            store
                .reindex_collection(&tasks(), &[status_index()])
                .expect("reindex over empty collection must succeed");
            let n = store
                .query_scalar_i64("SELECT COUNT(*) FROM entity_index")
                .expect("count");
            assert_eq!(n, 0);
        }
    }

    // ── Compound persisted index tests (FEAT-013 / US-033) ───────────────
    //
    // Mirror the in-memory adapter's `compound_index_tests`: exact lookup,
    // prefix match, sparse skip, unique reject/allow, update removes the old
    // key, drop clears rows, plus a backfill (entities first, then index).
    mod compound_index_tests {
        use super::*;
        use crate::adapter::{CompoundKey, IndexValue};
        use axon_schema::schema::{
            CollectionSchema, CompoundIndexDef, CompoundIndexField, IndexType,
        };

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

        /// Register a schema on `tasks()` declaring the given compound indexes,
        /// then return the put-driven store. Index maintenance is performed by
        /// the write primitives from the stamped schema.
        fn store_with_compound(compound: Vec<CompoundIndexDef>) -> SqliteStorageAdapter {
            let mut store = store();
            let mut schema = CollectionSchema::new(tasks());
            schema.compound_indexes = compound;
            store.put_schema(&schema).expect("put_schema");
            store
        }

        #[test]
        fn compound_index_missing_field_is_sparse() {
            let mut store = store_with_compound(vec![status_priority_index(false)]);
            let col = tasks();
            // Missing `priority` → no compound entry.
            store
                .put(ctask("t-001", json!({"status": "pending"})))
                .expect("put");

            let prefix = CompoundKey(vec![IndexValue::String("pending".into())]);
            let results = store
                .compound_index_prefix(&col, 0, &prefix)
                .expect("prefix");
            assert!(results.is_empty());
            let n = store
                .query_scalar_i64("SELECT COUNT(*) FROM entity_compound_index")
                .expect("count");
            assert_eq!(n, 0);
        }

        #[test]
        fn drop_indexes_clears_compound_rows() {
            let mut store = store_with_compound(vec![status_priority_index(false)]);
            let col = tasks();
            store
                .put(ctask("t-001", json!({"status": "pending", "priority": 1})))
                .expect("put");
            store.drop_indexes(&col).expect("drop");

            let n = store
                .query_scalar_i64("SELECT COUNT(*) FROM entity_compound_index")
                .expect("count");
            assert_eq!(n, 0);
        }

        #[test]
        fn backfill_compound_via_reindex_covers_preexisting_entities() {
            // Entities written BEFORE the compound index exists are found after
            // the index is built from existing data.
            let mut store = store();
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
            // Exactly two rows (t-003 was sparse).
            let n = store
                .query_scalar_i64("SELECT COUNT(*) FROM entity_compound_index")
                .expect("count");
            assert_eq!(n, 2);
        }

        #[test]
        fn backfill_compound_via_put_schema_hook() {
            // Registering a schema declaring a compound index backfills rows.
            let mut store = store();
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

    /// Approach C: write primitives (`put` / `compare_and_swap` / `delete` /
    /// `create_if_absent`) maintain single + compound indexes internally and
    /// atomically, looking up the collection's index defs from its schema. These
    /// tests drive maintenance THROUGH the primitives and assert via the read
    /// methods (`index_lookup` / `compound_index_lookup`); index maintenance is
    /// no longer exposed as a separate public method.
    mod primitive_maintenance_tests {
        use super::*;
        use crate::adapter::{CompoundKey, IndexValue};
        use axon_schema::schema::{
            CollectionSchema, CompoundIndexDef, CompoundIndexField, IndexDef, IndexType,
        };

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
            single: Vec<IndexDef>,
            compound: Vec<CompoundIndexDef>,
        ) -> SqliteStorageAdapter {
            let mut store = store();
            let mut schema = CollectionSchema::new(people());
            schema.indexes = single;
            schema.compound_indexes = compound;
            store.put_schema(&schema).expect("put_schema");
            store
        }

        fn lookup_status(store: &SqliteStorageAdapter, status: &str) -> Vec<EntityId> {
            let mut ids = store
                .index_lookup(&people(), "status", &IndexValue::String(status.into()))
                .expect("index_lookup");
            ids.sort();
            ids
        }

        #[test]
        fn put_new_entity_is_indexed() {
            let mut store = store_with_indexes(vec![status_index()], vec![]);
            store
                .put(person("p-1", json!({"status": "active"})))
                .expect("put");
            assert_eq!(lookup_status(&store, "active"), vec![EntityId::new("p-1")]);
        }

        #[test]
        fn put_replace_moves_index_entry() {
            let mut store = store_with_indexes(vec![status_index()], vec![]);
            store
                .put(person("p-1", json!({"status": "active"})))
                .expect("put");
            // Replace, changing the indexed field.
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
            let mut store = store_with_indexes(vec![status_index()], vec![]);
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
            let mut store = store_with_indexes(vec![status_index()], vec![]);
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
            let mut store = store_with_indexes(vec![status_index()], vec![]);
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
            let mut store = store_with_indexes(vec![], vec![status_priority_compound()]);
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
            let mut store = store_with_indexes(vec![unique_email_index()], vec![]);
            store
                .put(person("p-1", json!({"email": "a@x.com"})))
                .expect("put p-1");
            // p-2 with the same unique email must fail AND not persist.
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
            // The index must still point only at p-1.
            assert_eq!(
                store
                    .index_lookup(&people(), "email", &IndexValue::String("a@x.com".into()))
                    .expect("lookup"),
                vec![EntityId::new("p-1")]
            );
        }

        #[test]
        fn unique_violation_on_cas_does_not_persist_entity() {
            let mut store = store_with_indexes(vec![unique_email_index()], vec![]);
            store
                .put(person("p-1", json!({"email": "a@x.com"})))
                .expect("put p-1");
            store
                .put(person("p-2", json!({"email": "b@x.com"})))
                .expect("put p-2");
            // CAS p-2 to collide with p-1's email → violation, and p-2 unchanged.
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
            // No schema registered → no index maintenance, but the entity persists.
            let mut store = store();
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
            // No index rows were written.
            let n = store
                .query_scalar_i64("SELECT COUNT(*) FROM entity_index")
                .expect("count");
            assert_eq!(n, 0, "schemaless write must not write index rows");
        }

        /// Joined path: a mutation inside an API-style `begin_tx` maintains
        /// indexes, and `abort_tx` rolls back BOTH the entity and its index
        /// entries (validates owned-vs-joined ownership — the joined primitive
        /// must not commit/rollback its parent's tx).
        #[test]
        fn joined_abort_rolls_back_entity_and_index() {
            let mut store = store_with_indexes(vec![status_index()], vec![]);
            store
                .put(person("p-1", json!({"status": "active"})))
                .expect("seed put");

            store.begin_tx().expect("begin_tx");
            // This put joins the outer tx (no nested BEGIN); it maintains indexes.
            store
                .put(person("p-2", json!({"status": "active"})))
                .expect("joined put");
            // Visible within the transaction.
            assert_eq!(
                lookup_status(&store, "active"),
                vec![EntityId::new("p-1"), EntityId::new("p-2")]
            );
            store.abort_tx().expect("abort_tx");

            // After abort, p-2 and its index entry are gone; p-1 remains.
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
            let mut store = store_with_indexes(vec![status_index()], vec![]);
            store.begin_tx().expect("begin_tx");
            store
                .put(person("p-1", json!({"status": "active"})))
                .expect("joined put");
            store.commit_tx().expect("commit_tx");
            assert_eq!(lookup_status(&store, "active"), vec![EntityId::new("p-1")]);
        }
    }

    fn legacy_link_rows() -> (Entity, Entity, Link) {
        let link = Link {
            source_collection: CollectionId::new("src/集合"),
            source_id: EntityId::new("source/one"),
            target_collection: CollectionId::new("dst/集合"),
            target_id: EntityId::new("target/two"),
            link_type: "owns/typed".into(),
            metadata: json!({"legacy": true}),
        };
        let forward_id = EntityId::new(
            [
                link.source_collection.as_str(),
                link.source_id.as_str(),
                link.link_type.as_str(),
                link.target_collection.as_str(),
                link.target_id.as_str(),
            ]
            .join("/"),
        );
        let reverse_id = EntityId::new(
            [
                link.target_collection.as_str(),
                link.target_id.as_str(),
                link.source_collection.as_str(),
                link.source_id.as_str(),
                link.link_type.as_str(),
            ]
            .join("/"),
        );
        let mut forward = Entity::new(
            Link::links_collection(),
            forward_id,
            serde_json::to_value(&link).expect("link serializes"),
        );
        forward.version = 7;
        forward.created_at_ns = Some(11);
        forward.updated_at_ns = Some(22);
        let reverse = Entity::new(Link::links_rev_collection(), reverse_id, Value::Null);
        (forward, reverse, link)
    }

    #[test]
    fn legacy_link_key_migration_sqlite() {
        let mut storage = store();
        let (forward, reverse, link) = legacy_link_rows();
        storage.put(forward).expect("legacy forward seeds");
        storage.put(reverse).expect("legacy reverse seeds");

        crate::adapter::migrate_legacy_link_keys(&mut storage).expect("migration succeeds");
        let migrated = storage
            .get(
                &Link::links_collection(),
                &Link::storage_id(
                    &link.source_collection,
                    &link.source_id,
                    &link.link_type,
                    &link.target_collection,
                    &link.target_id,
                ),
            )
            .expect("typed lookup succeeds")
            .expect("typed forward exists");
        assert_eq!(migrated.version, 7);
        assert_eq!(
            storage
                .list_inbound_links(&link.target_collection, &link.target_id, None)
                .expect("reverse rebuilt"),
            vec![link]
        );
    }

    #[test]
    fn legacy_link_key_crash_resume_sqlite() {
        let mut storage = store();
        let (forward, reverse, link) = legacy_link_rows();
        let legacy_forward_id = forward.id.clone();
        storage.put(forward).expect("legacy forward seeds");
        storage.put(reverse).expect("legacy reverse seeds");

        assert!(crate::adapter::migrate_legacy_link_keys_with_crash(&mut storage).is_err());
        assert!(storage
            .get(&Link::links_collection(), &legacy_forward_id)
            .expect("legacy lookup succeeds")
            .is_some());
        assert!(storage
            .get(
                &Link::links_collection(),
                &LinkKey::forward(&link).entity_id()
            )
            .expect("typed lookup succeeds")
            .is_none());

        crate::adapter::migrate_legacy_link_keys(&mut storage).expect("retry succeeds");
        crate::adapter::migrate_legacy_link_keys(&mut storage).expect("retry is idempotent");
        assert_eq!(
            storage
                .get_link(
                    &link.source_collection,
                    &link.source_id,
                    &link.link_type,
                    &link.target_collection,
                    &link.target_id,
                )
                .expect("typed link lookup"),
            Some(link)
        );
    }

    #[test]
    fn legacy_link_key_migration_fails_closed_sqlite() {
        let mut malformed = store();
        let malformed_id = EntityId::new("not/a/valid/link");
        malformed
            .put(Entity::new(
                Link::links_collection(),
                malformed_id.clone(),
                json!({"not": "a link"}),
            ))
            .expect("malformed row seeds");
        assert!(crate::adapter::migrate_legacy_link_keys(&mut malformed).is_err());
        assert!(malformed
            .get(&Link::links_collection(), &malformed_id)
            .expect("malformed row lookup")
            .is_some());

        let mut duplicate = store();
        let (legacy_forward, legacy_reverse, link) = legacy_link_rows();
        let legacy_id = legacy_forward.id.clone();
        duplicate.put(legacy_forward).expect("legacy forward seeds");
        duplicate.put(legacy_reverse).expect("legacy reverse seeds");
        duplicate
            .put(link.to_entity())
            .expect("typed duplicate seeds");
        assert!(crate::adapter::migrate_legacy_link_keys(&mut duplicate).is_err());
        assert!(duplicate
            .get(&Link::links_collection(), &legacy_id)
            .expect("legacy duplicate lookup")
            .is_some());
        assert!(duplicate
            .get(
                &Link::links_collection(),
                &LinkKey::forward(&link).entity_id()
            )
            .expect("typed duplicate lookup")
            .is_some());

        let mut orphan = store();
        let orphan_id = EntityId::new("orphan/reverse/identity");
        orphan
            .put(Entity::new(
                Link::links_rev_collection(),
                orphan_id.clone(),
                Value::Null,
            ))
            .expect("orphan reverse seeds");
        assert!(crate::adapter::migrate_legacy_link_keys(&mut orphan).is_err());
        assert!(orphan
            .get(&Link::links_rev_collection(), &orphan_id)
            .expect("orphan lookup")
            .is_some());
    }
}

// L4 conformance test suite for SqliteStorageAdapter.
crate::storage_conformance_tests!(
    sqlite_conformance,
    super::SqliteStorageAdapter::open_in_memory().expect("test operation should succeed")
);
