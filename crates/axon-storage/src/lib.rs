//! Storage adapter trait and in-memory implementation for Axon.
//!
//! `axon-storage` defines the `StorageAdapter` trait that abstracts over
//! backing stores (SQLite, PostgreSQL, FoundationDB, etc.) and provides
//! an in-memory implementation for testing and development.

pub mod adapter;
pub mod conformance;
pub mod memory;
pub mod sqlite;

#[cfg(test)]
mod proptest_storage;

pub use adapter::StorageAdapter;
pub use memory::MemoryStorageAdapter;
pub use sqlite::SqliteStorageAdapter;
