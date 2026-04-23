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
    canonical_create_entity_operation, canonical_create_link_operation,
    canonical_delete_entity_operation, canonical_delete_link_operation,
    canonical_patch_entity_operation, canonical_revert_entity_operation,
    canonical_rollback_collection_operation, canonical_rollback_entity_operation,
    canonical_rollback_transaction_operation, canonical_staged_transaction_operation,
    canonical_transaction_operation, canonical_transition_lifecycle_operation,
    canonical_update_entity_operation, canonicalize_intent_operation, ApprovalState,
    CanonicalOperationMetadata, CanonicalTransactionOperation, ExecutableMutationIntent,
    MutationApprovalRoute, MutationIntent, MutationIntentCommitResult,
    MutationIntentCommitValidationContext, MutationIntentCommitValidationError,
    MutationIntentDecision, MutationIntentLifecycleError, MutationIntentLifecycleOperation,
    MutationIntentLifecycleService, MutationIntentModelError, MutationIntentPreviewRecord,
    MutationIntentReviewMetadata, MutationIntentScopeBinding, MutationIntentStaleDimension,
    MutationIntentSubjectBinding, MutationIntentToken, MutationIntentTokenLookupError,
    MutationIntentTokenSigner, MutationIntentTransactionCommitRequest, MutationOperationKind,
    MutationReviewSummary, PreImageBinding,
};
pub use policy::{PolicyRequestSnapshot, PolicySubjectSnapshot};
pub use transaction::Transaction;

#[cfg(test)]
mod proptest_api;
