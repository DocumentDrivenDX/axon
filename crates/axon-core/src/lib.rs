#![forbid(unsafe_code)]
//! Core types, traits, and error definitions for Axon.
//!
//! `axon-core` is the foundational crate that all other Axon crates depend on.
//! It defines the fundamental data model: entities, links, collection identifiers,
//! and the error hierarchy used across the workspace.

pub mod auth;
pub mod clock;
pub mod error;
pub mod guardrails;
pub mod id;
pub mod topology;
pub mod types;

pub use auth::{
    AuthError, CallerIdentity, DatabaseGrant, EntityFilter, GrantRegistry, JwtClaims, Op,
    Tenant, TenantId, TenantMember, TenantRole, User, UserId, UserIdentity, WritePolicy,
};
pub use clock::{Clock, FakeClock, SystemClock};
pub use guardrails::{
    GuardrailsConfig, GuardrailsLayer, RateLimitConfig, RejectionReason, TokenBucket,
};
pub use error::AxonError;
pub use id::{
    CollectionId, EntityId, LinkId, Namespace, QualifiedCollectionId, DEFAULT_DATABASE,
    DEFAULT_SCHEMA,
};
pub use types::{Entity, GateResult, Link, RuleViolation, LINKS_COLLECTION};
