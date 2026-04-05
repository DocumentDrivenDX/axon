//! HTTP/gRPC API surface for Axon server.
//!
//! `axon-api` defines the request/response types and handler interfaces
//! for the Axon server API. It sits above the storage, schema, and audit
//! layers and coordinates transactional entity operations.

pub mod bead;
pub mod handler;
pub mod request;
pub mod response;
pub mod transaction;

pub use transaction::Transaction;

#[cfg(test)]
mod proptest_api;
