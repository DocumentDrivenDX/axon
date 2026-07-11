#![forbid(unsafe_code)]
//! Storage adapter trait and in-memory implementation for Axon.
//!
//! `axon-storage` defines the `StorageAdapter` trait that abstracts over
//! backing stores (SQLite, PostgreSQL, FoundationDB, etc.) and provides
//! an in-memory implementation for testing and development.

pub mod adapter;
pub mod auth_schema;
pub mod conformance;
pub mod cursor_store;
pub mod index_builder;
pub mod memory;
pub mod postgres;
pub mod sqlite;

#[cfg(test)]
mod proptest_storage;

pub use adapter::{
    content_version_by_scan, extract_compound_key, extract_index_key_bytes, extract_index_value,
    extract_index_values, hash_id_set, hash_id_version_set, resolve_field_path,
    structural_version_by_scan, CompoundKey, IndexValue, OrderedFloat, StorageAdapter,
};
pub use auth_schema::{apply_auth_migrations_postgres, apply_auth_migrations_sqlite};
pub use axon_esf::{coerce_datetime_nanos, encode_compound_index_key, encode_index_value};
pub use cursor_store::{StorageCursorStore, CDC_CURSORS_COLLECTION};
pub use memory::MemoryStorageAdapter;
pub use postgres::{
    deprovision_postgres_database, provision_postgres_database, tenant_dsn, PostgresStorageAdapter,
};
pub use sqlite::{
    // raw_access_compile_fail covers that external crates cannot obtain the raw
    // SQLite connection from this concrete adapter export.
    SqliteStorageAdapter,
};
