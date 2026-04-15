#![forbid(unsafe_code)]
//! Storage adapter trait and in-memory implementation for Axon.
//!
//! `axon-storage` defines the `StorageAdapter` trait that abstracts over
//! backing stores (SQLite, PostgreSQL, FoundationDB, etc.) and provides
//! an in-memory implementation for testing and development.

pub mod adapter;
pub mod auth_schema;
pub mod conformance;
pub mod index_builder;
pub mod memory;
pub mod postgres;
pub mod sqlite;

#[cfg(test)]
mod proptest_storage;

pub use adapter::{
    extract_compound_key, extract_index_value, resolve_field_path, CompoundKey, IndexValue,
    OrderedFloat, StorageAdapter,
};
pub use auth_schema::{apply_auth_migrations_postgres, apply_auth_migrations_sqlite};
pub use memory::MemoryStorageAdapter;
pub use postgres::{
    deprovision_postgres_database, provision_postgres_database, tenant_dsn, PostgresStorageAdapter,
};
pub use sqlite::SqliteStorageAdapter;
