#![forbid(unsafe_code)]
//! Auto-generated GraphQL schema from Axon collection schemas (FEAT-015).
//!
//! This crate generates a dynamic `async-graphql` schema from the currently
//! registered collections and their entity schemas. The generated schema
//! supports:
//!
//! - **Queries** (US-048): per-collection entity queries with filter, sort,
//!   and pagination
//! - **Introspection** (US-049): schema reflects collections; adding a
//!   collection updates the GraphQL type
//! - **Mutations** (US-057): per-collection CRUD mutations
//!
//! # Architecture
//!
//! Unlike static `async-graphql` usage, Axon's schema is dynamic — collections
//! can be created and dropped at runtime. We use `async-graphql`'s dynamic
//! schema API to rebuild the schema when collections change.

pub mod aggregation;
pub mod dynamic;
pub mod graph;
pub mod subscriptions;
pub mod types;

pub use dynamic::{
    build_schema, build_schema_with_handler, build_schema_with_handler_and_broker, AxonSchema,
    GraphqlIdempotencyScope, SharedHandler,
};
pub use subscriptions::{BroadcastBroker, ChangeEvent, ChangeFeedBroker, SubscriptionFilter};
