//! Core types, traits, and error definitions for Axon.
//!
//! `axon-core` is the foundational crate that all other Axon crates depend on.
//! It defines the fundamental data model: entities, links, collection identifiers,
//! and the error hierarchy used across the workspace.

pub mod error;
pub mod id;
pub mod types;

pub use error::AxonError;
pub use id::{CollectionId, EntityId, LinkId, Namespace, DEFAULT_DATABASE, DEFAULT_SCHEMA};
pub use types::{Entity, Link, LINKS_COLLECTION};
