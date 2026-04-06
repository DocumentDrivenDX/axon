//! Storage adapter trait and in-memory implementation for Axon.
//!
//! `axon-storage` defines the `StorageAdapter` trait that abstracts over
//! backing stores (SQLite, PostgreSQL, FoundationDB, etc.) and provides
//! an in-memory implementation for testing and development.

pub mod adapter;
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
pub use memory::MemoryStorageAdapter;
pub use postgres::PostgresStorageAdapter;
pub use sqlite::SqliteStorageAdapter;
