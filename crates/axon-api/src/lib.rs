#![forbid(unsafe_code)]
//! HTTP/gRPC API surface for Axon server.
//!
//! `axon-api` defines the request/response types and handler interfaces
//! for the Axon server API. It sits above the storage, schema, and audit
//! layers and coordinates transactional entity operations.

pub mod bead;
pub mod handler;
pub mod intent;
pub mod policy;
pub mod request;
pub mod response;
#[cfg(any(test, feature = "test-fixtures"))]
pub mod test_fixtures;
pub mod transaction;

pub use intent::{
    ApprovalState, CanonicalOperationMetadata, ExecutableMutationIntent, MutationApprovalRoute,
    MutationIntent, MutationIntentDecision, MutationIntentModelError, MutationIntentScopeBinding,
    MutationIntentSubjectBinding, MutationIntentToken, MutationIntentTokenLookupError,
    MutationIntentTokenSigner, MutationOperationKind, MutationReviewSummary, PreImageBinding,
};
pub use policy::{PolicyRequestSnapshot, PolicySubjectSnapshot};
pub use transaction::Transaction;

#[cfg(test)]
mod proptest_api;
