use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use axon_audit::entry::{
    AuditAttribution, AuditEntry, MutationIntentApproverMetadata, MutationIntentAuditMetadata,
    MutationIntentAuditOrigin, MutationIntentAuditOriginSurface, MutationType,
};
use axon_audit::log::AuditLog;
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
pub use axon_core::intent::*;
use axon_core::types::{Entity, Link};
use axon_storage::adapter::StorageAdapter;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::request::{
    CreateEntityRequest, CreateLinkRequest, DeleteEntityRequest, DeleteLinkRequest,
    PatchEntityRequest, RevertEntityRequest, RollbackCollectionRequest, RollbackEntityRequest,
    RollbackEntityTarget, RollbackTransactionRequest, TransitionLifecycleRequest,
    UpdateEntityRequest,
};
use crate::transaction::{StagedOp, Transaction};

const INTENT_AUDIT_COLLECTION: &str = "__mutation_intents";

/// Review decision metadata supplied by an approver or operator.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MutationIntentReviewMetadata {
    /// Actor who performed the lifecycle transition, when known.
    pub actor: Option<String>,
    /// Human-readable reason attached to approval or rejection.
    pub reason: Option<String>,
    /// Tenant role resolved for the reviewer, when available.
    pub tenant_role: Option<String>,
    /// Credential used for the review decision, when available.
    pub credential_id: Option<String>,
    /// API or tool surface that produced the review decision.
    pub origin: Option<MutationIntentAuditOrigin>,
}

impl MutationIntentReviewMetadata {
    fn has_reason(&self) -> bool {
        self.reason
            .as_deref()
            .is_some_and(|reason| !reason.trim().is_empty())
    }
}

/// Caller-supplied operation captured inside a transaction intent preview.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum CanonicalTransactionOperation {
    /// Create an entity inside a transaction.
    CreateEntity {
        collection: CollectionId,
        id: EntityId,
        data: Value,
    },
    /// Replace an entity inside a transaction.
    UpdateEntity {
        collection: CollectionId,
        id: EntityId,
        data: Value,
        expected_version: u64,
    },
    /// Apply an RFC 7396 merge patch inside a transaction.
    PatchEntity {
        collection: CollectionId,
        id: EntityId,
        patch: Value,
        expected_version: u64,
    },
    /// Delete an entity inside a transaction.
    DeleteEntity {
        collection: CollectionId,
        id: EntityId,
        expected_version: u64,
    },
    /// Create a typed link inside a transaction.
    CreateLink {
        source_collection: CollectionId,
        source_id: EntityId,
        target_collection: CollectionId,
        target_id: EntityId,
        link_type: String,
        metadata: Value,
    },
    /// Delete a typed link inside a transaction.
    DeleteLink {
        source_collection: CollectionId,
        source_id: EntityId,
        target_collection: CollectionId,
        target_id: EntityId,
        link_type: String,
    },
}

/// Canonicalize a mutation operation payload and bind it to a stable SHA-256 hash.
pub fn canonicalize_intent_operation(
    operation_kind: MutationOperationKind,
    operation: Value,
) -> CanonicalOperationMetadata {
    let canonical_operation = canonicalize_json_value(operation);
    let mut hash_envelope = serde_json::Map::new();
    hash_envelope.insert(
        "operation_kind".into(),
        Value::String(operation_kind_wire_name(&operation_kind).into()),
    );
    hash_envelope.insert("operation".into(), canonical_operation.clone());

    let canonical_input = Value::Object(hash_envelope);
    let hash_input = canonical_json_string(&canonical_input);
    let mut hasher = Sha256::new();
    hasher.update(hash_input.as_bytes());
    let digest = hasher.finalize();

    CanonicalOperationMetadata {
        operation_kind,
        operation_hash: format!("sha256:{digest:x}"),
        canonical_operation: Some(canonical_operation),
    }
}

/// Canonical operation metadata for an entity create request.
pub fn canonical_create_entity_operation(
    request: &CreateEntityRequest,
) -> CanonicalOperationMetadata {
    canonicalize_intent_operation(
        MutationOperationKind::CreateEntity,
        json!({
            "collection": &request.collection,
            "id": &request.id,
            "data": &request.data,
        }),
    )
}

/// Canonical operation metadata for an entity update request.
pub fn canonical_update_entity_operation(
    request: &UpdateEntityRequest,
) -> CanonicalOperationMetadata {
    canonicalize_intent_operation(
        MutationOperationKind::UpdateEntity,
        json!({
            "collection": &request.collection,
            "id": &request.id,
            "data": &request.data,
            "expected_version": request.expected_version,
        }),
    )
}

/// Canonical operation metadata for an entity patch request.
pub fn canonical_patch_entity_operation(
    request: &PatchEntityRequest,
) -> CanonicalOperationMetadata {
    canonicalize_intent_operation(
        MutationOperationKind::PatchEntity,
        json!({
            "collection": &request.collection,
            "id": &request.id,
            "patch": &request.patch,
            "expected_version": request.expected_version,
        }),
    )
}

/// Canonical operation metadata for an entity delete request.
pub fn canonical_delete_entity_operation(
    request: &DeleteEntityRequest,
) -> CanonicalOperationMetadata {
    canonicalize_intent_operation(
        MutationOperationKind::DeleteEntity,
        json!({
            "collection": &request.collection,
            "id": &request.id,
            "force": request.force,
        }),
    )
}

/// Canonical operation metadata for a link create request.
pub fn canonical_create_link_operation(request: &CreateLinkRequest) -> CanonicalOperationMetadata {
    canonicalize_intent_operation(
        MutationOperationKind::CreateLink,
        json!({
            "source_collection": &request.source_collection,
            "source_id": &request.source_id,
            "target_collection": &request.target_collection,
            "target_id": &request.target_id,
            "link_type": &request.link_type,
            "metadata": &request.metadata,
        }),
    )
}

/// Canonical operation metadata for a link delete request.
pub fn canonical_delete_link_operation(request: &DeleteLinkRequest) -> CanonicalOperationMetadata {
    canonicalize_intent_operation(
        MutationOperationKind::DeleteLink,
        json!({
            "source_collection": &request.source_collection,
            "source_id": &request.source_id,
            "target_collection": &request.target_collection,
            "target_id": &request.target_id,
            "link_type": &request.link_type,
        }),
    )
}

/// Canonical operation metadata for a lifecycle state transition request.
pub fn canonical_transition_lifecycle_operation(
    request: &TransitionLifecycleRequest,
) -> CanonicalOperationMetadata {
    canonicalize_intent_operation(
        MutationOperationKind::Transition,
        json!({
            "collection": &request.collection_id,
            "id": &request.entity_id,
            "lifecycle_name": &request.lifecycle_name,
            "target_state": &request.target_state,
            "expected_version": request.expected_version,
        }),
    )
}

/// Canonical operation metadata for an entity rollback request.
pub fn canonical_rollback_entity_operation(
    request: &RollbackEntityRequest,
) -> CanonicalOperationMetadata {
    canonicalize_intent_operation(
        MutationOperationKind::Rollback,
        json!({
            "rollback_scope": "entity",
            "collection": &request.collection,
            "id": &request.id,
            "target": rollback_target_value(&request.target),
            "expected_version": request.expected_version,
            "dry_run": request.dry_run,
        }),
    )
}

/// Canonical operation metadata for a collection rollback request.
pub fn canonical_rollback_collection_operation(
    request: &RollbackCollectionRequest,
) -> CanonicalOperationMetadata {
    canonicalize_intent_operation(
        MutationOperationKind::Rollback,
        json!({
            "rollback_scope": "collection",
            "collection": &request.collection,
            "timestamp_ns": request.timestamp_ns,
            "dry_run": request.dry_run,
        }),
    )
}

/// Canonical operation metadata for a transaction rollback request.
pub fn canonical_rollback_transaction_operation(
    request: &RollbackTransactionRequest,
) -> CanonicalOperationMetadata {
    canonicalize_intent_operation(
        MutationOperationKind::Rollback,
        json!({
            "rollback_scope": "transaction",
            "transaction_id": &request.transaction_id,
            "dry_run": request.dry_run,
        }),
    )
}

/// Canonical operation metadata for an entity revert request.
pub fn canonical_revert_entity_operation(
    request: &RevertEntityRequest,
) -> CanonicalOperationMetadata {
    canonicalize_intent_operation(
        MutationOperationKind::Revert,
        json!({
            "audit_entry_id": request.audit_entry_id,
            "force": request.force,
        }),
    )
}

/// Canonical operation metadata for an ordered transaction operation list.
pub fn canonical_transaction_operation(
    operations: &[CanonicalTransactionOperation],
) -> CanonicalOperationMetadata {
    canonicalize_intent_operation(
        MutationOperationKind::Transaction,
        json!({
            "operations": operations,
        }),
    )
}

/// Canonical operation metadata for an in-memory transaction's staged writes.
pub fn canonical_staged_transaction_operation(
    transaction: &Transaction,
) -> CanonicalOperationMetadata {
    let operations: Vec<_> = transaction
        .staged_ops()
        .iter()
        .map(canonical_staged_operation_value)
        .collect();
    canonicalize_intent_operation(
        MutationOperationKind::Transaction,
        json!({
            "operations": operations,
        }),
    )
}

fn canonical_staged_operation_value(operation: &StagedOp) -> Value {
    match operation {
        StagedOp::Entity(operation) => match &operation.mutation {
            MutationType::EntityCreate => json!({
                "op": "create_entity",
                "collection": &operation.entity.collection,
                "id": &operation.entity.id,
                "data": &operation.entity.data,
            }),
            MutationType::EntityUpdate => json!({
                "op": "update_entity",
                "collection": &operation.entity.collection,
                "id": &operation.entity.id,
                "data": &operation.entity.data,
                "expected_version": operation.expected_version,
            }),
            MutationType::EntityDelete => json!({
                "op": "delete_entity",
                "collection": &operation.entity.collection,
                "id": &operation.entity.id,
                "expected_version": operation.expected_version,
            }),
            MutationType::CollectionCreate
            | MutationType::CollectionDrop
            | MutationType::TemplateCreate
            | MutationType::TemplateUpdate
            | MutationType::TemplateDelete
            | MutationType::SchemaUpdate
            | MutationType::EntityRevert
            | MutationType::LinkCreate
            | MutationType::LinkDelete
            | MutationType::GuardrailRejection
            | MutationType::IntentPreview
            | MutationType::IntentApprove
            | MutationType::IntentReject
            | MutationType::IntentExpire
            | MutationType::IntentCommit => json!({
                "op": "unsupported_entity_mutation",
                "mutation": format!("{:?}", operation.mutation),
                "collection": &operation.entity.collection,
                "id": &operation.entity.id,
            }),
        },
        StagedOp::LinkCreate(link) => canonical_link_operation_value("create_link", link, true),
        StagedOp::LinkDelete(link) => canonical_link_operation_value("delete_link", link, false),
    }
}

fn canonical_link_operation_value(op: &str, link: &Link, include_metadata: bool) -> Value {
    let mut operation = serde_json::Map::new();
    operation.insert("op".into(), Value::String(op.into()));
    operation.insert(
        "source_collection".into(),
        Value::String(link.source_collection.to_string()),
    );
    operation.insert(
        "source_id".into(),
        Value::String(link.source_id.to_string()),
    );
    operation.insert(
        "target_collection".into(),
        Value::String(link.target_collection.to_string()),
    );
    operation.insert(
        "target_id".into(),
        Value::String(link.target_id.to_string()),
    );
    operation.insert("link_type".into(), Value::String(link.link_type.clone()));
    if include_metadata {
        operation.insert("metadata".into(), link.metadata.clone());
    }
    Value::Object(operation)
}

fn rollback_target_value(target: &RollbackEntityTarget) -> Value {
    match target {
        RollbackEntityTarget::Version(version) => json!({
            "kind": "version",
            "version": version,
        }),
        RollbackEntityTarget::AuditEntryId(audit_entry_id) => json!({
            "kind": "audit_entry_id",
            "audit_entry_id": audit_entry_id,
        }),
    }
}

fn canonicalize_json_value(value: Value) -> Value {
    match value {
        Value::Array(items) => {
            Value::Array(items.into_iter().map(canonicalize_json_value).collect())
        }
        Value::Object(map) => {
            let mut entries: Vec<_> = map.into_iter().collect();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            let mut sorted = serde_json::Map::new();
            for (key, value) in entries {
                sorted.insert(key, canonicalize_json_value(value));
            }
            Value::Object(sorted)
        }
        primitive => primitive,
    }
}

fn canonical_json_string(value: &Value) -> String {
    let mut output = String::new();
    push_canonical_json(value, &mut output);
    output
}

fn push_canonical_json(value: &Value, output: &mut String) {
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(true) => output.push_str("true"),
        Value::Bool(false) => output.push_str("false"),
        Value::Number(number) => output.push_str(&number.to_string()),
        Value::String(value) => push_json_string(value, output),
        Value::Array(items) => {
            output.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                push_canonical_json(item, output);
            }
            output.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort();
            output.push('{');
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                push_json_string(key, output);
                output.push(':');
                if let Some(item) = map.get(key) {
                    push_canonical_json(item, output);
                }
            }
            output.push('}');
        }
    }
}

fn push_json_string(value: &str, output: &mut String) {
    output.push('"');
    for ch in value.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\t' => output.push_str("\\t"),
            '\n' => output.push_str("\\n"),
            '\u{0c}' => output.push_str("\\f"),
            '\r' => output.push_str("\\r"),
            '\u{00}'..='\u{1f}' => push_json_control_escape(ch, output),
            ch => output.push(ch),
        }
    }
    output.push('"');
}

fn push_json_control_escape(ch: char, output: &mut String) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let code = ch as usize;
    output.push_str("\\u00");
    output.push(HEX[(code >> 4) & 0x0f] as char);
    output.push(HEX[code & 0x0f] as char);
}

fn operation_kind_wire_name(kind: &MutationOperationKind) -> &'static str {
    match kind {
        MutationOperationKind::CreateEntity => "create_entity",
        MutationOperationKind::UpdateEntity => "update_entity",
        MutationOperationKind::PatchEntity => "patch_entity",
        MutationOperationKind::DeleteEntity => "delete_entity",
        MutationOperationKind::CreateLink => "create_link",
        MutationOperationKind::DeleteLink => "delete_link",
        MutationOperationKind::Transaction => "transaction",
        MutationOperationKind::Transition => "transition",
        MutationOperationKind::Rollback => "rollback",
        MutationOperationKind::Revert => "revert",
    }
}

/// Stored preview record plus the optional executable token returned to callers.
#[derive(Debug, Clone, PartialEq)]
pub struct MutationIntentPreviewRecord {
    /// Server-side intent record persisted for lookup, approval, and commit.
    pub intent: MutationIntent,
    /// Opaque token returned for `allow` and `needs_approval` previews.
    pub intent_token: Option<MutationIntentToken>,
}

/// Lifecycle operation attempted against a mutation intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationIntentLifecycleOperation {
    /// Persist a preview record.
    CreatePreview,
    /// Approve a pending intent.
    Approve,
    /// Reject a pending intent.
    Reject,
    /// Expire an uncommitted intent whose deadline has passed.
    Expire,
    /// Mark an executable intent as committed.
    Commit,
}

impl MutationIntentLifecycleOperation {
    fn as_str(self) -> &'static str {
        match self {
            Self::CreatePreview => "create_preview",
            Self::Approve => "approve",
            Self::Reject => "reject",
            Self::Expire => "expire",
            Self::Commit => "commit",
        }
    }
}

/// Failures raised by service-level mutation intent lifecycle helpers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationIntentLifecycleError {
    /// Storage layer rejected the operation.
    Storage(String),
    /// No intent exists for the requested tenant/database scope.
    NotFound { intent_id: String },
    /// A preview record carried a non-canonical initial lifecycle state.
    InvalidPreviewState {
        intent_id: String,
        decision: MutationIntentDecision,
        actual: ApprovalState,
        expected: ApprovalState,
    },
    /// The requested transition is not valid from the current lifecycle state.
    InvalidTransition {
        intent_id: String,
        operation: MutationIntentLifecycleOperation,
        from: ApprovalState,
        to: ApprovalState,
    },
    /// The requested transition is invalid for the intent policy decision.
    InvalidDecisionTransition {
        intent_id: String,
        operation: MutationIntentLifecycleOperation,
        decision: MutationIntentDecision,
    },
    /// The approval route requires a human-readable reason for this transition.
    ReasonRequired {
        intent_id: String,
        operation: MutationIntentLifecycleOperation,
    },
    /// The intent reached its TTL boundary before the requested transition.
    Expired {
        intent_id: String,
        operation: MutationIntentLifecycleOperation,
        expires_at: u64,
        now_ns: u64,
    },
    /// Expiry was requested before the intent's deadline.
    NotExpired {
        intent_id: String,
        expires_at: u64,
        now_ns: u64,
    },
    /// The operation supplied for commit no longer matches the previewed intent.
    IntentMismatch {
        intent_id: String,
        expected_hash: String,
        actual_hash: String,
    },
}

impl fmt::Display for MutationIntentLifecycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(message) => write!(f, "mutation intent storage error: {message}"),
            Self::NotFound { intent_id } => write!(f, "mutation intent '{intent_id}' not found"),
            Self::InvalidPreviewState {
                intent_id,
                decision,
                actual,
                expected,
            } => write!(
                f,
                "mutation intent '{intent_id}' preview decision '{}' requires initial state '{}', got '{}'",
                decision.as_str(),
                expected.as_str(),
                actual.as_str()
            ),
            Self::InvalidTransition {
                intent_id,
                operation,
                from,
                to,
            } => write!(
                f,
                "mutation intent '{intent_id}' cannot {} from '{}' to '{}'",
                operation.as_str(),
                from.as_str(),
                to.as_str()
            ),
            Self::InvalidDecisionTransition {
                intent_id,
                operation,
                decision,
            } => write!(
                f,
                "mutation intent '{intent_id}' with decision '{}' cannot {}",
                decision.as_str(),
                operation.as_str()
            ),
            Self::ReasonRequired {
                intent_id,
                operation,
            } => write!(
                f,
                "mutation intent '{intent_id}' requires a reason to {}",
                operation.as_str()
            ),
            Self::Expired {
                intent_id,
                operation,
                expires_at,
                now_ns,
            } => write!(
                f,
                "mutation intent '{intent_id}' expired at {expires_at}; cannot {} at {now_ns}",
                operation.as_str()
            ),
            Self::NotExpired {
                intent_id,
                expires_at,
                now_ns,
            } => write!(
                f,
                "mutation intent '{intent_id}' expires at {expires_at}, not expired at {now_ns}"
            ),
            Self::IntentMismatch {
                intent_id,
                expected_hash,
                actual_hash,
            } => write!(
                f,
                "mutation intent '{intent_id}' operation mismatch: expected {expected_hash}, got {actual_hash}"
            ),
        }
    }
}

impl Error for MutationIntentLifecycleError {}

impl MutationIntentLifecycleError {
    /// Stable machine-facing error code for GraphQL/MCP/SDK surfaces.
    pub fn error_code(&self) -> &'static str {
        match self {
            Self::Storage(_) => "intent_storage_error",
            Self::NotFound { .. } => "intent_not_found",
            Self::InvalidPreviewState { .. } => "intent_invalid_preview_state",
            Self::InvalidTransition { .. } => "intent_invalid_transition",
            Self::InvalidDecisionTransition { .. } => "intent_invalid_decision_transition",
            Self::ReasonRequired { .. } => "intent_reason_required",
            Self::Expired { .. } => "intent_expired",
            Self::NotExpired { .. } => "intent_not_expired",
            Self::IntentMismatch { .. } => "intent_mismatch",
        }
    }
}

impl From<AxonError> for MutationIntentLifecycleError {
    fn from(value: AxonError) -> Self {
        Self::Storage(value.to_string())
    }
}

/// Current request-time bindings used to revalidate a mutation intent before commit.
#[derive(Debug, Clone, PartialEq)]
pub struct MutationIntentCommitValidationContext {
    /// Subject and grant snapshot resolved for the commit request.
    pub subject: MutationIntentSubjectBinding,
    /// Current collection schema version observed during commit validation.
    pub schema_version: u32,
    /// Current policy version observed during commit validation.
    pub policy_version: u32,
    /// Canonical hash for the operation supplied to commit.
    pub operation_hash: String,
    /// Whether the caller still has the current authorization envelope required to commit.
    pub caller_authorized: bool,
}

/// Inputs for audited commit binding validation.
pub struct MutationIntentCommitValidationAuditRequest<'a> {
    /// Tenant/database scope expected by the intent token.
    pub scope: &'a MutationIntentScopeBinding,
    /// Executable token issued by preview.
    pub token: &'a MutationIntentToken,
    /// Current request-time binding snapshot.
    pub current: &'a MutationIntentCommitValidationContext,
    /// Current time in nanoseconds for TTL validation.
    pub now_ns: u64,
    /// Actor recorded on validation-failure audit entries.
    pub actor: Option<&'a str>,
}

/// A stale binding dimension detected while validating intent commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationIntentStaleDimension {
    /// Stable dimension name, for example `schema_version` or `pre_image`.
    pub dimension: String,
    /// Version or value bound at preview time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    /// Version or value observed at commit time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
    /// Optional record path for entity/link pre-image drift.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// Inputs for consuming a mutation intent by committing its bound transaction.
pub struct MutationIntentTransactionCommitRequest {
    /// Tenant/database scope expected by the intent token.
    pub scope: MutationIntentScopeBinding,
    /// Executable token issued by preview.
    pub token: MutationIntentToken,
    /// Staged transaction to validate and commit.
    pub transaction: Transaction,
    /// Canonical operation being consumed; defaults to the staged transaction hash.
    pub canonical_operation: Option<CanonicalOperationMetadata>,
    /// Current request-time binding snapshot.
    pub current: MutationIntentCommitValidationContext,
    /// Current time in nanoseconds for TTL validation.
    pub now_ns: u64,
    /// Actor recorded on mutation audit entries.
    pub actor: Option<String>,
    /// Optional authenticated attribution recorded on mutation audit entries.
    pub attribution: Option<AuditAttribution>,
}

/// Result of consuming a mutation intent by committing its bound transaction.
#[derive(Debug, Clone, PartialEq)]
pub struct MutationIntentCommitResult {
    /// Intent after it has been marked committed.
    pub intent: MutationIntent,
    /// Entity/link records written by the transaction commit path.
    pub written: Vec<Entity>,
    /// Transaction ID shared by mutation audit entries.
    pub transaction_id: String,
}

/// Commit-time intent validation failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationIntentCommitValidationError {
    /// Token lookup or lifecycle state validation failed.
    Token(MutationIntentTokenLookupError),
    /// Storage failed while loading intent or pre-image state.
    Storage(String),
    /// The caller-supplied operation hash no longer matches the preview.
    IntentMismatch {
        intent_id: String,
        expected_hash: String,
        actual_hash: String,
    },
    /// One or more non-operation binding dimensions changed since preview.
    IntentStale {
        intent_id: String,
        dimensions: Vec<MutationIntentStaleDimension>,
    },
    /// The caller no longer has the current grants/authorization envelope required to commit.
    AuthorizationFailed { intent_id: String, reason: String },
    /// The validated transaction failed while executing.
    CommitFailed { intent_id: String, source: String },
}

impl MutationIntentCommitValidationError {
    /// Stable machine-readable error code for GraphQL, MCP, and API surfaces.
    pub fn error_code(&self) -> &'static str {
        match self {
            Self::Token(error) => token_lookup_error_code(error),
            Self::Storage(_) => "intent_storage_error",
            Self::IntentMismatch { .. } => "intent_mismatch",
            Self::IntentStale { .. } => "intent_stale",
            Self::AuthorizationFailed { .. } => "intent_authorization_failed",
            Self::CommitFailed { .. } => "intent_commit_failed",
        }
    }
}

impl fmt::Display for MutationIntentCommitValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Token(error) => write!(f, "mutation intent token validation failed: {error:?}"),
            Self::Storage(message) => write!(f, "mutation intent storage error: {message}"),
            Self::IntentMismatch {
                intent_id,
                expected_hash,
                actual_hash,
            } => write!(
                f,
                "mutation intent '{intent_id}' operation mismatch: expected {expected_hash}, got {actual_hash}"
            ),
            Self::IntentStale {
                intent_id,
                dimensions,
            } => write!(
                f,
                "mutation intent '{intent_id}' has stale commit bindings: {dimensions:?}"
            ),
            Self::AuthorizationFailed { intent_id, reason } => write!(
                f,
                "mutation intent '{intent_id}' authorization failed before commit: {reason}"
            ),
            Self::CommitFailed { intent_id, source } => {
                write!(f, "mutation intent '{intent_id}' commit failed: {source}")
            }
        }
    }
}

impl Error for MutationIntentCommitValidationError {}

fn token_lookup_error_code(error: &MutationIntentTokenLookupError) -> &'static str {
    match error {
        MutationIntentTokenLookupError::MalformedToken
        | MutationIntentTokenLookupError::InvalidSignature
        | MutationIntentTokenLookupError::NotFound
        | MutationIntentTokenLookupError::TenantDatabaseMismatch => "intent_token_invalid",
        MutationIntentTokenLookupError::Expired => "intent_expired",
        MutationIntentTokenLookupError::Rejected => "intent_rejected",
        MutationIntentTokenLookupError::AlreadyCommitted => "intent_already_committed",
        MutationIntentTokenLookupError::ApprovalRequired => "intent_approval_required",
        MutationIntentTokenLookupError::Unauthorized => "intent_authorization_failed",
        MutationIntentTokenLookupError::GrantVersionStale
        | MutationIntentTokenLookupError::SchemaVersionStale
        | MutationIntentTokenLookupError::PolicyVersionStale
        | MutationIntentTokenLookupError::PreImageStale => "intent_stale",
        MutationIntentTokenLookupError::OperationMismatch => "intent_mismatch",
    }
}

/// Service-level helpers for mutation intent preview, review, expiry, and commit.
#[derive(Debug, Clone)]
pub struct MutationIntentLifecycleService {
    token_signer: MutationIntentTokenSigner,
}

impl MutationIntentLifecycleService {
    /// Create a lifecycle service from the deployment-local token signer.
    pub fn new(token_signer: MutationIntentTokenSigner) -> Self {
        Self { token_signer }
    }

    /// Persist a preview intent record and issue an executable token when legal.
    pub fn create_preview_record<S: StorageAdapter, A: AuditLog>(
        &self,
        storage: &mut S,
        audit: &mut A,
        intent: MutationIntent,
    ) -> Result<MutationIntentPreviewRecord, MutationIntentLifecycleError> {
        self.create_preview_record_with_origin(storage, audit, intent, None)
    }

    /// Persist a preview intent record and attach API/tool origin metadata to the audit entry.
    pub fn create_preview_record_with_origin<S: StorageAdapter, A: AuditLog>(
        &self,
        storage: &mut S,
        audit: &mut A,
        intent: MutationIntent,
        origin: Option<MutationIntentAuditOrigin>,
    ) -> Result<MutationIntentPreviewRecord, MutationIntentLifecycleError> {
        let expected = preview_state_for_decision(&intent.decision);
        if intent.approval_state != expected {
            return Err(MutationIntentLifecycleError::InvalidPreviewState {
                intent_id: intent.intent_id,
                decision: intent.decision,
                actual: intent.approval_state,
                expected,
            });
        }

        storage.create_mutation_intent(&intent)?;
        append_intent_lifecycle_audit(
            audit,
            &intent,
            MutationType::IntentPreview,
            intent_audit_actor(&intent),
            None,
            origin,
        )?;
        let intent_token = intent
            .can_have_executable_token()
            .then(|| self.token_signer.issue(&intent));

        Ok(MutationIntentPreviewRecord {
            intent,
            intent_token,
        })
    }

    /// Approve a pending approval-routed intent.
    pub fn approve<S: StorageAdapter>(
        &self,
        storage: &mut S,
        scope: &MutationIntentScopeBinding,
        intent_id: &str,
        metadata: MutationIntentReviewMetadata,
        now_ns: u64,
    ) -> Result<MutationIntent, MutationIntentLifecycleError> {
        let intent = load_intent(storage, scope, intent_id)?;
        reject_if_expired_or_expire(
            storage,
            scope,
            &intent,
            now_ns,
            MutationIntentLifecycleOperation::Approve,
        )?;
        require_decision(
            &intent,
            MutationIntentDecision::NeedsApproval,
            MutationIntentLifecycleOperation::Approve,
        )?;
        require_state(
            &intent,
            ApprovalState::Pending,
            ApprovalState::Approved,
            MutationIntentLifecycleOperation::Approve,
        )?;
        require_reason(
            &intent,
            &metadata,
            MutationIntentLifecycleOperation::Approve,
        )?;
        update_state(
            storage,
            scope,
            intent_id,
            ApprovalState::Pending,
            ApprovalState::Approved,
        )
    }

    /// Approve a pending approval-routed intent and append a lineage audit record.
    pub fn approve_with_audit<S: StorageAdapter, A: AuditLog>(
        &self,
        storage: &mut S,
        audit: &mut A,
        scope: &MutationIntentScopeBinding,
        intent_id: &str,
        metadata: MutationIntentReviewMetadata,
        now_ns: u64,
    ) -> Result<MutationIntent, MutationIntentLifecycleError> {
        let before = load_intent(storage, scope, intent_id)?;
        let approved = self.approve(storage, scope, intent_id, metadata.clone(), now_ns)?;
        append_intent_review_audit(
            audit,
            &before,
            &approved,
            MutationType::IntentApprove,
            &metadata,
        )?;
        Ok(approved)
    }

    /// Reject a pending approval-routed intent.
    pub fn reject<S: StorageAdapter>(
        &self,
        storage: &mut S,
        scope: &MutationIntentScopeBinding,
        intent_id: &str,
        metadata: MutationIntentReviewMetadata,
        now_ns: u64,
    ) -> Result<MutationIntent, MutationIntentLifecycleError> {
        let intent = load_intent(storage, scope, intent_id)?;
        reject_if_expired_or_expire(
            storage,
            scope,
            &intent,
            now_ns,
            MutationIntentLifecycleOperation::Reject,
        )?;
        require_decision(
            &intent,
            MutationIntentDecision::NeedsApproval,
            MutationIntentLifecycleOperation::Reject,
        )?;
        require_state(
            &intent,
            ApprovalState::Pending,
            ApprovalState::Rejected,
            MutationIntentLifecycleOperation::Reject,
        )?;
        require_reason(&intent, &metadata, MutationIntentLifecycleOperation::Reject)?;
        update_state(
            storage,
            scope,
            intent_id,
            ApprovalState::Pending,
            ApprovalState::Rejected,
        )
    }

    /// Reject a pending approval-routed intent and append a lineage audit record.
    pub fn reject_with_audit<S: StorageAdapter, A: AuditLog>(
        &self,
        storage: &mut S,
        audit: &mut A,
        scope: &MutationIntentScopeBinding,
        intent_id: &str,
        metadata: MutationIntentReviewMetadata,
        now_ns: u64,
    ) -> Result<MutationIntent, MutationIntentLifecycleError> {
        let before = load_intent(storage, scope, intent_id)?;
        let rejected = self.reject(storage, scope, intent_id, metadata.clone(), now_ns)?;
        append_intent_review_audit(
            audit,
            &before,
            &rejected,
            MutationType::IntentReject,
            &metadata,
        )?;
        Ok(rejected)
    }

    /// Expire an uncommitted intent whose deadline has passed.
    pub fn expire<S: StorageAdapter>(
        &self,
        storage: &mut S,
        scope: &MutationIntentScopeBinding,
        intent_id: &str,
        now_ns: u64,
    ) -> Result<MutationIntent, MutationIntentLifecycleError> {
        let intent = load_intent(storage, scope, intent_id)?;
        if intent.expires_at > now_ns {
            return Err(MutationIntentLifecycleError::NotExpired {
                intent_id: intent.intent_id,
                expires_at: intent.expires_at,
                now_ns,
            });
        }

        match intent.approval_state {
            ApprovalState::None | ApprovalState::Pending | ApprovalState::Approved => update_state(
                storage,
                scope,
                intent_id,
                intent.approval_state,
                ApprovalState::Expired,
            ),
            ApprovalState::Rejected | ApprovalState::Expired | ApprovalState::Committed => {
                Err(MutationIntentLifecycleError::InvalidTransition {
                    intent_id: intent.intent_id,
                    operation: MutationIntentLifecycleOperation::Expire,
                    from: intent.approval_state,
                    to: ApprovalState::Expired,
                })
            }
        }
    }

    /// Expire all due intents returned by the storage adapter expiry scan.
    pub fn expire_due<S: StorageAdapter>(
        &self,
        storage: &mut S,
        scope: &MutationIntentScopeBinding,
        now_ns: u64,
        limit: Option<usize>,
    ) -> Result<Vec<MutationIntent>, MutationIntentLifecycleError> {
        let due = storage.list_expired_mutation_intents(
            &scope.tenant_id,
            &scope.database_id,
            now_ns,
            limit,
        )?;
        let mut expired = Vec::with_capacity(due.len());
        for intent in due {
            expired.push(self.expire(storage, scope, &intent.intent_id, now_ns)?);
        }
        Ok(expired)
    }

    /// Expire one due intent and append a lineage audit record for the transition.
    pub fn expire_with_audit<S: StorageAdapter, A: AuditLog>(
        &self,
        storage: &mut S,
        audit: &mut A,
        scope: &MutationIntentScopeBinding,
        intent_id: &str,
        now_ns: u64,
    ) -> Result<MutationIntent, MutationIntentLifecycleError> {
        let before = load_intent(storage, scope, intent_id)?;
        let expired = self.expire(storage, scope, intent_id, now_ns)?;
        append_intent_transition_audit(
            audit,
            &before,
            &expired,
            MutationType::IntentExpire,
            "system",
            None,
        )?;
        Ok(expired)
    }

    /// Expire all due intents and append lineage audit records for each transition.
    pub fn expire_due_with_audit<S: StorageAdapter, A: AuditLog>(
        &self,
        storage: &mut S,
        audit: &mut A,
        scope: &MutationIntentScopeBinding,
        now_ns: u64,
        limit: Option<usize>,
    ) -> Result<Vec<MutationIntent>, MutationIntentLifecycleError> {
        let due = storage.list_expired_mutation_intents(
            &scope.tenant_id,
            &scope.database_id,
            now_ns,
            limit,
        )?;
        let mut expired = Vec::with_capacity(due.len());
        for intent in due {
            let updated = self.expire(storage, scope, &intent.intent_id, now_ns)?;
            append_intent_transition_audit(
                audit,
                &intent,
                &updated,
                MutationType::IntentExpire,
                "system",
                None,
            )?;
            expired.push(updated);
        }
        Ok(expired)
    }

    /// Materialize due expirations, then list currently pending review intents.
    pub fn list_pending<S: StorageAdapter>(
        &self,
        storage: &mut S,
        scope: &MutationIntentScopeBinding,
        now_ns: u64,
        limit: Option<usize>,
    ) -> Result<Vec<MutationIntent>, MutationIntentLifecycleError> {
        self.expire_due(storage, scope, now_ns, None)?;
        storage
            .list_pending_mutation_intents(&scope.tenant_id, &scope.database_id, now_ns, limit)
            .map_err(MutationIntentLifecycleError::from)
    }

    /// Materialize due expirations, then list intents by an explicit lifecycle state.
    pub fn list_by_state<S: StorageAdapter>(
        &self,
        storage: &mut S,
        scope: &MutationIntentScopeBinding,
        approval_state: ApprovalState,
        now_ns: u64,
        limit: Option<usize>,
    ) -> Result<Vec<MutationIntent>, MutationIntentLifecycleError> {
        self.expire_due(storage, scope, now_ns, None)?;
        storage
            .list_mutation_intents_by_state(
                &scope.tenant_id,
                &scope.database_id,
                approval_state,
                limit,
            )
            .map_err(MutationIntentLifecycleError::from)
    }

    /// Validate token, lifecycle state, authorization, and all preview-bound dimensions.
    pub fn validate_commit_bindings<S: StorageAdapter>(
        &self,
        storage: &S,
        scope: &MutationIntentScopeBinding,
        token: &MutationIntentToken,
        current: &MutationIntentCommitValidationContext,
        now_ns: u64,
    ) -> Result<MutationIntent, MutationIntentCommitValidationError> {
        let intent_id = self
            .token_signer
            .verify(token)
            .map_err(MutationIntentCommitValidationError::Token)?;
        let loaded = load_intent(storage, scope, &intent_id).map_err(commit_load_error)?;
        let intent = self
            .token_signer
            .resolve_for_commit(token, scope, now_ns, |resolved_id, expected_scope| {
                (resolved_id == loaded.intent_id && &loaded.scope == expected_scope)
                    .then(|| loaded.clone())
            })
            .map_err(MutationIntentCommitValidationError::Token)?;

        if intent.operation.operation_hash != current.operation_hash {
            return Err(MutationIntentCommitValidationError::IntentMismatch {
                intent_id: intent.intent_id,
                expected_hash: intent.operation.operation_hash,
                actual_hash: current.operation_hash.clone(),
            });
        }

        if !current.caller_authorized {
            return Err(MutationIntentCommitValidationError::AuthorizationFailed {
                intent_id: intent.intent_id,
                reason: "current grants do not authorize intent commit".into(),
            });
        }

        let stale = stale_commit_dimensions(storage, &intent, current)?;
        if !stale.is_empty() {
            return Err(MutationIntentCommitValidationError::IntentStale {
                intent_id: intent.intent_id,
                dimensions: stale,
            });
        }

        Ok(intent)
    }

    /// Validate commit bindings and append audit records for stale/mismatch attempts.
    pub fn validate_commit_bindings_with_audit<S: StorageAdapter, A: AuditLog>(
        &self,
        storage: &S,
        audit: &mut A,
        request: MutationIntentCommitValidationAuditRequest<'_>,
    ) -> Result<MutationIntent, MutationIntentCommitValidationError> {
        match self.validate_commit_bindings(
            storage,
            request.scope,
            request.token,
            request.current,
            request.now_ns,
        ) {
            Ok(intent) => Ok(intent),
            Err(error) => {
                append_commit_validation_failure_audit(
                    audit,
                    storage,
                    request.scope,
                    &error,
                    request.actor,
                )?;
                Err(error)
            }
        }
    }

    /// Validate and execute a transaction intent through the normal atomic transaction path.
    pub fn commit_transaction_intent<S: StorageAdapter, L: AuditLog>(
        &self,
        storage: &mut S,
        audit: &mut L,
        request: MutationIntentTransactionCommitRequest,
    ) -> Result<MutationIntentCommitResult, MutationIntentCommitValidationError> {
        let MutationIntentTransactionCommitRequest {
            scope,
            token,
            transaction,
            canonical_operation,
            current,
            now_ns,
            actor,
            attribution,
        } = request;
        let operation = canonical_operation
            .unwrap_or_else(|| canonical_staged_transaction_operation(&transaction));
        let mut current = current;
        current.operation_hash = operation.operation_hash.clone();
        let intent = self.validate_commit_bindings_with_audit(
            storage,
            audit,
            MutationIntentCommitValidationAuditRequest {
                scope: &scope,
                token: &token,
                current: &current,
                now_ns,
                actor: actor.as_deref(),
            },
        )?;
        let expected_state = commit_expected_state(&intent).map_err(|error| {
            MutationIntentCommitValidationError::CommitFailed {
                intent_id: intent.intent_id.clone(),
                source: error.to_string(),
            }
        })?;
        let intent_id = intent.intent_id.clone();
        let transaction_id = transaction.id.clone();
        let lineage = intent_lifecycle_lineage(&intent, None);

        let (written, committed) = transaction
            .commit_with_storage_hook(
                storage,
                audit,
                actor,
                attribution,
                Some(lineage),
                |storage| {
                    update_state(
                        storage,
                        &scope,
                        &intent_id,
                        expected_state,
                        ApprovalState::Committed,
                    )
                    .map_err(|error| AxonError::InvalidOperation(error.to_string()))
                },
            )
            .map_err(|error| commit_execution_error(&intent.intent_id, error))?;

        Ok(MutationIntentCommitResult {
            intent: committed,
            written,
            transaction_id,
        })
    }

    /// Mark an allowed or approved intent as committed.
    pub fn mark_committed<S: StorageAdapter>(
        &self,
        storage: &mut S,
        scope: &MutationIntentScopeBinding,
        intent_id: &str,
        now_ns: u64,
    ) -> Result<MutationIntent, MutationIntentLifecycleError> {
        Self::mark_committed_internal(storage, scope, intent_id, None, now_ns)
    }

    /// Mark an allowed or approved intent as committed only if the operation hash still matches.
    pub fn mark_committed_with_operation_hash<S: StorageAdapter>(
        &self,
        storage: &mut S,
        scope: &MutationIntentScopeBinding,
        intent_id: &str,
        operation_hash: &str,
        now_ns: u64,
    ) -> Result<MutationIntent, MutationIntentLifecycleError> {
        Self::mark_committed_internal(storage, scope, intent_id, Some(operation_hash), now_ns)
    }

    fn mark_committed_internal<S: StorageAdapter>(
        storage: &mut S,
        scope: &MutationIntentScopeBinding,
        intent_id: &str,
        operation_hash: Option<&str>,
        now_ns: u64,
    ) -> Result<MutationIntent, MutationIntentLifecycleError> {
        let intent = load_intent(storage, scope, intent_id)?;
        reject_if_expired_or_expire(
            storage,
            scope,
            &intent,
            now_ns,
            MutationIntentLifecycleOperation::Commit,
        )?;
        require_operation_hash(&intent, operation_hash)?;

        let expected = commit_expected_state(&intent)?;
        require_state(
            &intent,
            expected.clone(),
            ApprovalState::Committed,
            MutationIntentLifecycleOperation::Commit,
        )?;
        update_state(
            storage,
            scope,
            intent_id,
            expected,
            ApprovalState::Committed,
        )
    }
}

fn commit_expected_state(
    intent: &MutationIntent,
) -> Result<ApprovalState, MutationIntentLifecycleError> {
    match intent.decision {
        MutationIntentDecision::Allow => Ok(ApprovalState::None),
        MutationIntentDecision::NeedsApproval => Ok(ApprovalState::Approved),
        MutationIntentDecision::Deny => {
            Err(MutationIntentLifecycleError::InvalidDecisionTransition {
                intent_id: intent.intent_id.clone(),
                operation: MutationIntentLifecycleOperation::Commit,
                decision: MutationIntentDecision::Deny,
            })
        }
    }
}

fn preview_state_for_decision(decision: &MutationIntentDecision) -> ApprovalState {
    match decision {
        MutationIntentDecision::Allow | MutationIntentDecision::Deny => ApprovalState::None,
        MutationIntentDecision::NeedsApproval => ApprovalState::Pending,
    }
}

fn load_intent<S: StorageAdapter>(
    storage: &S,
    scope: &MutationIntentScopeBinding,
    intent_id: &str,
) -> Result<MutationIntent, MutationIntentLifecycleError> {
    storage
        .get_mutation_intent(&scope.tenant_id, &scope.database_id, intent_id)?
        .ok_or_else(|| MutationIntentLifecycleError::NotFound {
            intent_id: intent_id.to_string(),
        })
}

fn commit_load_error(error: MutationIntentLifecycleError) -> MutationIntentCommitValidationError {
    match error {
        MutationIntentLifecycleError::NotFound { .. } => {
            MutationIntentCommitValidationError::Token(MutationIntentTokenLookupError::NotFound)
        }
        MutationIntentLifecycleError::Storage(message) => {
            MutationIntentCommitValidationError::Storage(message)
        }
        other => MutationIntentCommitValidationError::Storage(other.to_string()),
    }
}

fn stale_commit_dimensions<S: StorageAdapter>(
    storage: &S,
    intent: &MutationIntent,
    current: &MutationIntentCommitValidationContext,
) -> Result<Vec<MutationIntentStaleDimension>, MutationIntentCommitValidationError> {
    let mut stale = Vec::new();

    if intent.subject.grant_version != current.subject.grant_version {
        stale.push(stale_dimension(
            "grant_version",
            intent
                .subject
                .grant_version
                .map(|version| version.to_string()),
            current
                .subject
                .grant_version
                .map(|version| version.to_string()),
            None,
        ));
    }

    if subject_without_grant(&intent.subject) != subject_without_grant(&current.subject) {
        stale.push(stale_dimension(
            "subject",
            Some(subject_stable_string(&intent.subject)),
            Some(subject_stable_string(&current.subject)),
            None,
        ));
    }

    if intent.schema_version != current.schema_version {
        stale.push(stale_dimension(
            "schema_version",
            Some(intent.schema_version.to_string()),
            Some(current.schema_version.to_string()),
            None,
        ));
    }

    if intent.policy_version != current.policy_version {
        stale.push(stale_dimension(
            "policy_version",
            Some(intent.policy_version.to_string()),
            Some(current.policy_version.to_string()),
            None,
        ));
    }

    for pre_image in &intent.pre_images {
        let current_version = current_pre_image_version(storage, pre_image)
            .map_err(|error| MutationIntentCommitValidationError::Storage(error.to_string()))?;
        if current_version != Some(pre_image_version(pre_image)) {
            stale.push(stale_dimension(
                "pre_image",
                Some(pre_image_version(pre_image).to_string()),
                current_version
                    .map(|version| version.to_string())
                    .or_else(|| Some("missing".into())),
                Some(pre_image_path(pre_image)),
            ));
        }
    }

    Ok(stale)
}

fn stale_dimension(
    dimension: impl Into<String>,
    expected: Option<String>,
    actual: Option<String>,
    path: Option<String>,
) -> MutationIntentStaleDimension {
    MutationIntentStaleDimension {
        dimension: dimension.into(),
        expected,
        actual,
        path,
    }
}

fn commit_execution_error(
    intent_id: &str,
    error: AxonError,
) -> MutationIntentCommitValidationError {
    match error {
        AxonError::ConflictingVersion {
            expected,
            actual,
            current_entity,
        } => MutationIntentCommitValidationError::IntentStale {
            intent_id: intent_id.to_string(),
            dimensions: vec![stale_dimension(
                "pre_image",
                Some(expected.to_string()),
                Some(actual.to_string()),
                current_entity
                    .as_ref()
                    .map(|entity| format!("entity:{}/{}", entity.collection, entity.id)),
            )],
        },
        other => MutationIntentCommitValidationError::CommitFailed {
            intent_id: intent_id.to_string(),
            source: other.to_string(),
        },
    }
}

#[derive(Debug, Eq, PartialEq)]
struct SubjectWithoutGrant<'a> {
    user_id: Option<&'a str>,
    agent_id: Option<&'a str>,
    delegated_by: Option<&'a str>,
    tenant_role: Option<&'a str>,
    credential_id: Option<&'a str>,
    attributes: String,
}

fn subject_without_grant(subject: &MutationIntentSubjectBinding) -> SubjectWithoutGrant<'_> {
    SubjectWithoutGrant {
        user_id: subject.user_id.as_deref(),
        agent_id: subject.agent_id.as_deref(),
        delegated_by: subject.delegated_by.as_deref(),
        tenant_role: subject.tenant_role.as_deref(),
        credential_id: subject.credential_id.as_deref(),
        attributes: canonical_json_string(&json!(subject.attributes)),
    }
}

fn subject_stable_string(subject: &MutationIntentSubjectBinding) -> String {
    canonical_json_string(&serde_json::to_value(subject).unwrap_or(Value::Null))
}

fn pre_image_version(pre_image: &PreImageBinding) -> u64 {
    match pre_image {
        PreImageBinding::Entity { version, .. } | PreImageBinding::Link { version, .. } => *version,
    }
}

fn pre_image_path(pre_image: &PreImageBinding) -> String {
    match pre_image {
        PreImageBinding::Entity { collection, id, .. } => format!("entity:{collection}/{id}"),
        PreImageBinding::Link { collection, id, .. } => format!("link:{collection}/{id}"),
    }
}

fn current_pre_image_version<S: StorageAdapter>(
    storage: &S,
    pre_image: &PreImageBinding,
) -> Result<Option<u64>, AxonError> {
    match pre_image {
        PreImageBinding::Entity { collection, id, .. } => storage
            .get(collection, id)
            .map(|entity| entity.map(|e| e.version)),
        PreImageBinding::Link { collection, id, .. } => storage
            .get(collection, &EntityId::new(id.to_string()))
            .map(|entity| entity.map(|e| e.version)),
    }
}

fn update_state<S: StorageAdapter>(
    storage: &mut S,
    scope: &MutationIntentScopeBinding,
    intent_id: &str,
    expected: ApprovalState,
    new_state: ApprovalState,
) -> Result<MutationIntent, MutationIntentLifecycleError> {
    storage
        .update_mutation_intent_state(
            &scope.tenant_id,
            &scope.database_id,
            intent_id,
            expected,
            new_state,
        )
        .map_err(MutationIntentLifecycleError::from)
}

fn reject_if_expired_or_expire<S: StorageAdapter>(
    storage: &mut S,
    scope: &MutationIntentScopeBinding,
    intent: &MutationIntent,
    now_ns: u64,
    operation: MutationIntentLifecycleOperation,
) -> Result<(), MutationIntentLifecycleError> {
    let already_expired = intent.approval_state == ApprovalState::Expired;
    let due = intent.expires_at <= now_ns;
    if !already_expired && due && expirable_state(&intent.approval_state) {
        update_state(
            storage,
            scope,
            &intent.intent_id,
            intent.approval_state.clone(),
            ApprovalState::Expired,
        )?;
    }

    if already_expired || due {
        return Err(MutationIntentLifecycleError::Expired {
            intent_id: intent.intent_id.clone(),
            operation,
            expires_at: intent.expires_at,
            now_ns,
        });
    }
    Ok(())
}

fn expirable_state(state: &ApprovalState) -> bool {
    matches!(
        state,
        ApprovalState::None | ApprovalState::Pending | ApprovalState::Approved
    )
}

fn require_decision(
    intent: &MutationIntent,
    expected: MutationIntentDecision,
    operation: MutationIntentLifecycleOperation,
) -> Result<(), MutationIntentLifecycleError> {
    if intent.decision != expected {
        return Err(MutationIntentLifecycleError::InvalidDecisionTransition {
            intent_id: intent.intent_id.clone(),
            operation,
            decision: intent.decision.clone(),
        });
    }
    Ok(())
}

fn require_state(
    intent: &MutationIntent,
    expected: ApprovalState,
    new_state: ApprovalState,
    operation: MutationIntentLifecycleOperation,
) -> Result<(), MutationIntentLifecycleError> {
    if intent.approval_state != expected {
        return Err(MutationIntentLifecycleError::InvalidTransition {
            intent_id: intent.intent_id.clone(),
            operation,
            from: intent.approval_state.clone(),
            to: new_state,
        });
    }
    Ok(())
}

fn require_operation_hash(
    intent: &MutationIntent,
    operation_hash: Option<&str>,
) -> Result<(), MutationIntentLifecycleError> {
    let Some(operation_hash) = operation_hash else {
        return Ok(());
    };
    if intent.operation.operation_hash != operation_hash {
        return Err(MutationIntentLifecycleError::IntentMismatch {
            intent_id: intent.intent_id.clone(),
            expected_hash: intent.operation.operation_hash.clone(),
            actual_hash: operation_hash.to_string(),
        });
    }
    Ok(())
}

fn require_reason(
    intent: &MutationIntent,
    metadata: &MutationIntentReviewMetadata,
    operation: MutationIntentLifecycleOperation,
) -> Result<(), MutationIntentLifecycleError> {
    if intent
        .approval_route
        .as_ref()
        .is_some_and(|route| route.reason_required)
        && !metadata.has_reason()
    {
        return Err(MutationIntentLifecycleError::ReasonRequired {
            intent_id: intent.intent_id.clone(),
            operation,
        });
    }
    Ok(())
}

fn append_intent_lifecycle_audit<A: AuditLog>(
    audit: &mut A,
    intent: &MutationIntent,
    mutation: MutationType,
    actor: &str,
    reason: Option<String>,
    origin: Option<MutationIntentAuditOrigin>,
) -> Result<AuditEntry, MutationIntentLifecycleError> {
    let data_after = serde_json::to_value(&intent.review_summary)
        .map_err(|error| MutationIntentLifecycleError::Storage(error.to_string()))?;
    append_intent_lifecycle_audit_entry(
        audit,
        IntentAuditEntryParts {
            intent,
            mutation,
            actor,
            lineage: intent_lifecycle_lineage_with_origin(intent, reason, origin),
            data_before: None,
            data_after,
            metadata: preview_audit_metadata(intent),
        },
    )
}

fn preview_audit_metadata(intent: &MutationIntent) -> HashMap<String, String> {
    HashMap::from([
        ("decision".into(), intent.decision.as_str().into()),
        ("schema_version".into(), intent.schema_version.to_string()),
        ("policy_version".into(), intent.policy_version.to_string()),
        (
            "operation_hash".into(),
            intent.operation.operation_hash.clone(),
        ),
        ("expires_at".into(), intent.expires_at.to_string()),
    ])
}

fn append_intent_transition_audit<A: AuditLog>(
    audit: &mut A,
    before: &MutationIntent,
    after: &MutationIntent,
    mutation: MutationType,
    actor: &str,
    reason: Option<String>,
) -> Result<AuditEntry, MutationIntentLifecycleError> {
    let data_before = serde_json::to_value(before)
        .map_err(|error| MutationIntentLifecycleError::Storage(error.to_string()))?;
    let data_after = serde_json::to_value(after)
        .map_err(|error| MutationIntentLifecycleError::Storage(error.to_string()))?;
    append_intent_lifecycle_audit_entry(
        audit,
        IntentAuditEntryParts {
            intent: after,
            mutation,
            actor,
            lineage: intent_lifecycle_lineage(after, reason),
            data_before: Some(data_before),
            data_after,
            metadata: HashMap::new(),
        },
    )
}

fn append_intent_review_audit<A: AuditLog>(
    audit: &mut A,
    before: &MutationIntent,
    after: &MutationIntent,
    mutation: MutationType,
    metadata: &MutationIntentReviewMetadata,
) -> Result<AuditEntry, MutationIntentLifecycleError> {
    let data_before = serde_json::to_value(before)
        .map_err(|error| MutationIntentLifecycleError::Storage(error.to_string()))?;
    let data_after = serde_json::to_value(after)
        .map_err(|error| MutationIntentLifecycleError::Storage(error.to_string()))?;
    let mut entry_metadata = HashMap::new();
    if let Some(reason) = &metadata.reason {
        entry_metadata.insert("reason".into(), reason.clone());
    }
    append_intent_lifecycle_audit_entry(
        audit,
        IntentAuditEntryParts {
            intent: after,
            mutation,
            actor: metadata.actor.as_deref().unwrap_or("anonymous"),
            lineage: intent_review_lineage(after, metadata),
            data_before: Some(data_before),
            data_after,
            metadata: entry_metadata,
        },
    )
}

struct IntentAuditEntryParts<'a> {
    intent: &'a MutationIntent,
    mutation: MutationType,
    actor: &'a str,
    lineage: MutationIntentAuditMetadata,
    data_before: Option<Value>,
    data_after: Value,
    metadata: HashMap<String, String>,
}

fn append_intent_lifecycle_audit_entry<A: AuditLog>(
    audit: &mut A,
    parts: IntentAuditEntryParts<'_>,
) -> Result<AuditEntry, MutationIntentLifecycleError> {
    let entry = AuditEntry::new(
        CollectionId::new(INTENT_AUDIT_COLLECTION),
        EntityId::new(parts.intent.intent_id.clone()),
        0,
        parts.mutation,
        parts.data_before,
        Some(parts.data_after),
        Some(parts.actor.to_string()),
    )
    .with_metadata(parts.metadata)
    .with_intent_lineage(parts.lineage);
    audit
        .append(entry)
        .map_err(MutationIntentLifecycleError::from)
}

fn intent_audit_actor(intent: &MutationIntent) -> &str {
    intent
        .subject
        .user_id
        .as_deref()
        .or(intent.subject.agent_id.as_deref())
        .or(intent.subject.delegated_by.as_deref())
        .unwrap_or("anonymous")
}

fn intent_lifecycle_lineage(
    intent: &MutationIntent,
    reason: Option<String>,
) -> MutationIntentAuditMetadata {
    intent_lifecycle_lineage_with_origin(intent, reason, None)
}

fn intent_lifecycle_lineage_with_origin(
    intent: &MutationIntent,
    reason: Option<String>,
    origin: Option<MutationIntentAuditOrigin>,
) -> MutationIntentAuditMetadata {
    MutationIntentAuditMetadata {
        intent_id: intent.intent_id.clone(),
        decision: intent.decision.clone(),
        approval_id: None,
        policy_version: intent.policy_version,
        schema_version: intent.schema_version,
        subject_snapshot: intent.subject.clone(),
        approver: None,
        reason,
        origin: Some(origin.unwrap_or_else(|| MutationIntentAuditOrigin {
            surface: MutationIntentAuditOriginSurface::System,
            tool_name: None,
            request_id: None,
            operation_hash: Some(intent.operation.operation_hash.clone()),
        })),
        lineage_links: Vec::new(),
    }
}

fn intent_review_lineage(
    intent: &MutationIntent,
    metadata: &MutationIntentReviewMetadata,
) -> MutationIntentAuditMetadata {
    let mut lineage = intent_lifecycle_lineage(intent, metadata.reason.clone());
    lineage.approver = metadata
        .actor
        .as_ref()
        .map(|actor| MutationIntentApproverMetadata {
            user_id: Some(actor.clone()),
            actor: Some(actor.clone()),
            tenant_role: metadata.tenant_role.clone(),
            credential_id: metadata.credential_id.clone(),
        });
    if let Some(origin) = metadata.origin.clone() {
        lineage.origin = Some(origin);
    }
    lineage
}

fn append_commit_validation_failure_audit<S: StorageAdapter, A: AuditLog>(
    audit: &mut A,
    storage: &S,
    scope: &MutationIntentScopeBinding,
    error: &MutationIntentCommitValidationError,
    actor: Option<&str>,
) -> Result<(), MutationIntentCommitValidationError> {
    let Some(intent_id) = commit_validation_failure_intent_id(error) else {
        return Ok(());
    };
    let intent = load_intent(storage, scope, intent_id)
        .map_err(|error| MutationIntentCommitValidationError::Storage(error.to_string()))?;
    let data_after = commit_validation_failure_payload(&intent, error)?;
    let data_before = serde_json::to_value(&intent)
        .map_err(|error| MutationIntentCommitValidationError::Storage(error.to_string()))?;
    let mut metadata = HashMap::new();
    metadata.insert("event".into(), "commit_validation_failed".into());
    metadata.insert("error_code".into(), error.error_code().into());

    append_intent_lifecycle_audit_entry(
        audit,
        IntentAuditEntryParts {
            intent: &intent,
            mutation: MutationType::IntentCommit,
            actor: actor.unwrap_or("anonymous"),
            lineage: intent_lifecycle_lineage(&intent, None),
            data_before: Some(data_before),
            data_after,
            metadata,
        },
    )
    .map_err(|error| MutationIntentCommitValidationError::Storage(error.to_string()))?;
    Ok(())
}

fn commit_validation_failure_intent_id(
    error: &MutationIntentCommitValidationError,
) -> Option<&str> {
    match error {
        MutationIntentCommitValidationError::IntentMismatch { intent_id, .. }
        | MutationIntentCommitValidationError::IntentStale { intent_id, .. } => Some(intent_id),
        MutationIntentCommitValidationError::Token(_)
        | MutationIntentCommitValidationError::Storage(_)
        | MutationIntentCommitValidationError::AuthorizationFailed { .. }
        | MutationIntentCommitValidationError::CommitFailed { .. } => None,
    }
}

fn commit_validation_failure_payload(
    intent: &MutationIntent,
    error: &MutationIntentCommitValidationError,
) -> Result<Value, MutationIntentCommitValidationError> {
    let intent_value = serde_json::to_value(intent)
        .map_err(|error| MutationIntentCommitValidationError::Storage(error.to_string()))?;
    let mut payload = serde_json::Map::new();
    payload.insert(
        "event".into(),
        Value::String("commit_validation_failed".into()),
    );
    payload.insert(
        "error_code".into(),
        Value::String(error.error_code().into()),
    );
    payload.insert("intent".into(), intent_value);

    match error {
        MutationIntentCommitValidationError::IntentMismatch {
            expected_hash,
            actual_hash,
            ..
        } => {
            payload.insert("expected_hash".into(), Value::String(expected_hash.clone()));
            payload.insert("actual_hash".into(), Value::String(actual_hash.clone()));
        }
        MutationIntentCommitValidationError::IntentStale { dimensions, .. } => {
            let stale = serde_json::to_value(dimensions)
                .map_err(|error| MutationIntentCommitValidationError::Storage(error.to_string()))?;
            payload.insert("stale".into(), stale);
        }
        MutationIntentCommitValidationError::Token(_)
        | MutationIntentCommitValidationError::Storage(_)
        | MutationIntentCommitValidationError::AuthorizationFailed { .. }
        | MutationIntentCommitValidationError::CommitFailed { .. } => {}
    }

    Ok(Value::Object(payload))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::*;
    use axon_audit::log::{AuditLog, AuditQuery, MemoryAuditLog};
    use axon_core::id::{CollectionId, EntityId};
    use axon_storage::memory::MemoryStorageAdapter;

    fn scope() -> MutationIntentScopeBinding {
        MutationIntentScopeBinding {
            tenant_id: "acme".into(),
            database_id: "finance".into(),
        }
    }

    fn service() -> MutationIntentLifecycleService {
        MutationIntentLifecycleService::new(MutationIntentTokenSigner::new(b"deployment-secret"))
    }

    fn metadata(reason: Option<&str>) -> MutationIntentReviewMetadata {
        MutationIntentReviewMetadata {
            actor: Some("usr_approver".into()),
            reason: reason.map(str::to_string),
            ..Default::default()
        }
    }

    fn intent(
        intent_id: &str,
        decision: MutationIntentDecision,
        approval_state: ApprovalState,
        expires_at: u64,
        reason_required: bool,
    ) -> MutationIntent {
        let approval_route =
            (decision == MutationIntentDecision::NeedsApproval).then_some(MutationApprovalRoute {
                role: Some("finance_approver".into()),
                reason_required,
                deadline_seconds: Some(3600),
                separation_of_duties: true,
            });

        MutationIntent {
            intent_id: intent_id.into(),
            scope: scope(),
            subject: MutationIntentSubjectBinding {
                user_id: Some("usr_requester".into()),
                agent_id: Some("agent_ap".into()),
                delegated_by: None,
                tenant_role: Some("member".into()),
                credential_id: Some("cred_live".into()),
                grant_version: Some(1),
                attributes: Default::default(),
            },
            schema_version: 7,
            policy_version: 7,
            operation: CanonicalOperationMetadata {
                operation_kind: MutationOperationKind::UpdateEntity,
                operation_hash: format!("sha256:{intent_id}"),
                canonical_operation: Some(json!({
                    "collection": "invoices",
                    "id": "inv-001",
                    "patch": {"amount_cents": 1250000}
                })),
            },
            pre_images: vec![PreImageBinding::Entity {
                collection: CollectionId::new("invoices"),
                id: EntityId::new("inv-001"),
                version: 3,
            }],
            decision,
            approval_state,
            approval_route,
            expires_at,
            review_summary: MutationReviewSummary {
                title: Some("Invoice update".into()),
                summary: "Review invoice amount update.".into(),
                risk: Some("above autonomous limit".into()),
                affected_records: Vec::new(),
                affected_fields: vec!["amount_cents".into()],
                diff: json!({"amount_cents": {"before": 1000, "after": 1250000}}),
                policy_explanation: vec!["large-invoice-update matched".into()],
            },
        }
    }

    fn current_commit_context(intent: &MutationIntent) -> MutationIntentCommitValidationContext {
        MutationIntentCommitValidationContext {
            subject: intent.subject.clone(),
            schema_version: intent.schema_version,
            policy_version: intent.policy_version,
            operation_hash: intent.operation.operation_hash.clone(),
            caller_authorized: true,
        }
    }

    fn seed_pre_image_entity(storage: &mut MemoryStorageAdapter, version: u64) {
        let mut entity = Entity::new(
            CollectionId::new("invoices"),
            EntityId::new("inv-001"),
            json!({"amount_cents": 1000}),
        );
        entity.version = version;
        storage.put(entity).expect("seed entity");
    }

    fn seed_preview(
        storage: &mut MemoryStorageAdapter,
        svc: &MutationIntentLifecycleService,
        intent: MutationIntent,
    ) -> MutationIntentToken {
        svc.create_preview_record(storage, &mut MemoryAuditLog::default(), intent)
            .expect("preview record")
            .intent_token
            .expect("executable token")
    }

    fn accounts() -> CollectionId {
        CollectionId::new("accounts")
    }

    fn vendors() -> CollectionId {
        CollectionId::new("vendors")
    }

    fn account(id: &str, balance: i64) -> Entity {
        Entity::new(accounts(), EntityId::new(id), json!({"balance": balance}))
    }

    fn vendor(id: &str) -> Entity {
        Entity::new(vendors(), EntityId::new(id), json!({"name": id}))
    }

    fn entity_pre_image(entity: &Entity) -> PreImageBinding {
        PreImageBinding::Entity {
            collection: entity.collection.clone(),
            id: entity.id.clone(),
            version: entity.version,
        }
    }

    fn transaction_intent(
        intent_id: &str,
        transaction: &Transaction,
        pre_images: Vec<PreImageBinding>,
    ) -> MutationIntent {
        let mut intent = intent(
            intent_id,
            MutationIntentDecision::Allow,
            ApprovalState::None,
            100,
            false,
        );
        intent.operation = canonical_staged_transaction_operation(transaction);
        intent.pre_images = pre_images;
        intent
    }

    #[test]
    fn commit_validation_accepts_current_bound_dimensions() {
        let mut storage = MemoryStorageAdapter::default();
        let svc = service();
        let intent = intent(
            "mint_commit_valid",
            MutationIntentDecision::Allow,
            ApprovalState::None,
            100,
            false,
        );
        seed_pre_image_entity(&mut storage, 3);
        let token = seed_preview(&mut storage, &svc, intent.clone());

        let resolved = svc
            .validate_commit_bindings(
                &storage,
                &scope(),
                &token,
                &current_commit_context(&intent),
                1,
            )
            .expect("current bindings should validate");

        assert_eq!(resolved.intent_id, "mint_commit_valid");
    }

    #[test]
    fn commit_validation_reports_stale_schema_policy_grant_and_pre_image_details() {
        let mut storage = MemoryStorageAdapter::default();
        let svc = service();
        let intent = intent(
            "mint_commit_stale",
            MutationIntentDecision::Allow,
            ApprovalState::None,
            100,
            false,
        );
        seed_pre_image_entity(&mut storage, 4);
        let token = seed_preview(&mut storage, &svc, intent.clone());
        let mut current = current_commit_context(&intent);
        current.subject.grant_version = Some(2);
        current.schema_version = 8;
        current.policy_version = 9;

        let err = svc
            .validate_commit_bindings(&storage, &scope(), &token, &current, 1)
            .expect_err("stale dimensions should reject commit");

        assert_eq!(err.error_code(), "intent_stale");
        let MutationIntentCommitValidationError::IntentStale { dimensions, .. } = err else {
            panic!("expected stale error");
        };
        assert!(dimensions.iter().any(|dimension| {
            dimension.dimension == "grant_version"
                && dimension.expected.as_deref() == Some("1")
                && dimension.actual.as_deref() == Some("2")
        }));
        assert!(dimensions.iter().any(|dimension| {
            dimension.dimension == "schema_version"
                && dimension.expected.as_deref() == Some("7")
                && dimension.actual.as_deref() == Some("8")
        }));
        assert!(dimensions.iter().any(|dimension| {
            dimension.dimension == "policy_version"
                && dimension.expected.as_deref() == Some("7")
                && dimension.actual.as_deref() == Some("9")
        }));
        assert!(dimensions.iter().any(|dimension| {
            dimension.dimension == "pre_image"
                && dimension.expected.as_deref() == Some("3")
                && dimension.actual.as_deref() == Some("4")
                && dimension.path.as_deref() == Some("entity:invoices/inv-001")
        }));
    }

    #[test]
    fn commit_validation_reports_operation_hash_drift_as_mismatch() {
        let mut storage = MemoryStorageAdapter::default();
        let svc = service();
        let intent = intent(
            "mint_commit_mismatch",
            MutationIntentDecision::Allow,
            ApprovalState::None,
            100,
            false,
        );
        seed_pre_image_entity(&mut storage, 3);
        let token = seed_preview(&mut storage, &svc, intent.clone());
        let mut current = current_commit_context(&intent);
        current.operation_hash = "sha256:different".into();

        let err = svc
            .validate_commit_bindings(&storage, &scope(), &token, &current, 1)
            .expect_err("operation drift should reject commit");

        assert_eq!(err.error_code(), "intent_mismatch");
        let MutationIntentCommitValidationError::IntentMismatch {
            expected_hash,
            actual_hash,
            ..
        } = err
        else {
            panic!("expected mismatch error");
        };
        assert_eq!(expected_hash, intent.operation.operation_hash);
        assert_eq!(actual_hash, "sha256:different");
    }

    #[test]
    fn commit_validation_reports_current_authorization_failure() {
        let mut storage = MemoryStorageAdapter::default();
        let svc = service();
        let intent = intent(
            "mint_commit_auth",
            MutationIntentDecision::Allow,
            ApprovalState::None,
            100,
            false,
        );
        seed_pre_image_entity(&mut storage, 3);
        let token = seed_preview(&mut storage, &svc, intent.clone());
        let mut current = current_commit_context(&intent);
        current.caller_authorized = false;

        let err = svc
            .validate_commit_bindings(&storage, &scope(), &token, &current, 1)
            .expect_err("lost grants should reject commit");

        assert_eq!(err.error_code(), "intent_authorization_failed");
    }

    #[test]
    fn transaction_intent_rejects_stale_entity_before_any_write_or_mutation_audit() {
        let mut storage = MemoryStorageAdapter::default();
        let mut audit = MemoryAuditLog::default();
        let svc = service();
        storage.put(account("A", 100)).expect("seed A");
        storage.put(account("B", 50)).expect("seed B");
        let a_before = storage
            .get(&accounts(), &EntityId::new("A"))
            .expect("read A")
            .expect("A exists");
        let b_before = storage
            .get(&accounts(), &EntityId::new("B"))
            .expect("read B")
            .expect("B exists");
        let mut tx = Transaction::new();
        tx.update(
            account("A", 70),
            a_before.version,
            Some(a_before.data.clone()),
        )
        .expect("stage A");
        tx.update(
            account("B", 80),
            b_before.version,
            Some(b_before.data.clone()),
        )
        .expect("stage B");
        let intent = transaction_intent(
            "mint_tx_stale",
            &tx,
            vec![entity_pre_image(&a_before), entity_pre_image(&b_before)],
        );
        let token = seed_preview(&mut storage, &svc, intent.clone());
        storage
            .compare_and_swap(account("B", 55), b_before.version)
            .expect("make B stale");

        let err = svc
            .commit_transaction_intent(
                &mut storage,
                &mut audit,
                MutationIntentTransactionCommitRequest {
                    scope: scope(),
                    token,
                    transaction: tx,
                    canonical_operation: None,
                    current: current_commit_context(&intent),
                    now_ns: 1,
                    actor: Some("system".into()),
                    attribution: None,
                },
            )
            .expect_err("stale pre-image should reject commit");

        assert_eq!(err.error_code(), "intent_stale");
        let MutationIntentCommitValidationError::IntentStale { dimensions, .. } = err else {
            panic!("expected stale error");
        };
        assert!(dimensions.iter().any(|dimension| {
            dimension.dimension == "pre_image"
                && dimension.expected.as_deref() == Some("1")
                && dimension.actual.as_deref() == Some("2")
                && dimension.path.as_deref() == Some("entity:accounts/B")
        }));
        assert_eq!(
            storage
                .get(&accounts(), &EntityId::new("A"))
                .expect("read A")
                .expect("A exists")
                .data["balance"],
            100
        );
        assert_eq!(
            storage
                .get(&accounts(), &EntityId::new("B"))
                .expect("read B")
                .expect("B exists")
                .data["balance"],
            55
        );
        assert!(audit
            .query_by_operation(&MutationType::EntityUpdate)
            .expect("entity update audit query should pass")
            .is_empty());
        let stale_audit = audit
            .query_paginated(AuditQuery {
                operation: Some(MutationType::IntentCommit),
                intent_id: Some("mint_tx_stale".into()),
                ..AuditQuery::default()
            })
            .expect("stale operational audit should be queryable");
        assert_eq!(stale_audit.entries.len(), 1);
        assert_eq!(
            stale_audit.entries[0].metadata["error_code"],
            "intent_stale"
        );
        assert_eq!(
            storage
                .get_mutation_intent("acme", "finance", "mint_tx_stale")
                .expect("read intent")
                .expect("intent exists")
                .approval_state,
            ApprovalState::None
        );
    }

    #[test]
    fn transaction_intent_rejects_operation_mismatch_before_any_write_or_mutation_audit() {
        let mut storage = MemoryStorageAdapter::default();
        let mut audit = MemoryAuditLog::default();
        let svc = service();
        storage.put(account("A", 100)).expect("seed A");
        let a_before = storage
            .get(&accounts(), &EntityId::new("A"))
            .expect("read A")
            .expect("A exists");
        let mut previewed_tx = Transaction::new();
        previewed_tx
            .update(
                account("A", 70),
                a_before.version,
                Some(a_before.data.clone()),
            )
            .expect("stage preview");
        let intent = transaction_intent(
            "mint_tx_mismatch",
            &previewed_tx,
            vec![entity_pre_image(&a_before)],
        );
        let token = seed_preview(&mut storage, &svc, intent.clone());
        let mut drifted_tx = Transaction::new();
        drifted_tx
            .update(
                account("A", 71),
                a_before.version,
                Some(a_before.data.clone()),
            )
            .expect("stage drift");

        let err = svc
            .commit_transaction_intent(
                &mut storage,
                &mut audit,
                MutationIntentTransactionCommitRequest {
                    scope: scope(),
                    token,
                    transaction: drifted_tx,
                    canonical_operation: None,
                    current: current_commit_context(&intent),
                    now_ns: 1,
                    actor: Some("system".into()),
                    attribution: None,
                },
            )
            .expect_err("drifted transaction should reject commit");

        assert_eq!(err.error_code(), "intent_mismatch");
        assert_eq!(
            storage
                .get(&accounts(), &EntityId::new("A"))
                .expect("read A")
                .expect("A exists")
                .data["balance"],
            100
        );
        assert!(audit
            .query_by_operation(&MutationType::EntityUpdate)
            .expect("entity update audit query should pass")
            .is_empty());
        let mismatch_audit = audit
            .query_paginated(AuditQuery {
                operation: Some(MutationType::IntentCommit),
                intent_id: Some("mint_tx_mismatch".into()),
                ..AuditQuery::default()
            })
            .expect("mismatch operational audit should be queryable");
        assert_eq!(mismatch_audit.entries.len(), 1);
        assert_eq!(
            mismatch_audit.entries[0].metadata["error_code"],
            "intent_mismatch"
        );
    }

    #[test]
    fn transaction_intent_commits_entity_and_link_operations_atomically() {
        let mut storage = MemoryStorageAdapter::default();
        let mut audit = MemoryAuditLog::default();
        let svc = service();
        storage.put(account("inv-001", 100)).expect("seed invoice");
        storage.put(vendor("ven-001")).expect("seed vendor");
        let invoice_before = storage
            .get(&accounts(), &EntityId::new("inv-001"))
            .expect("read invoice")
            .expect("invoice exists");
        let vendor_before = storage
            .get(&vendors(), &EntityId::new("ven-001"))
            .expect("read vendor")
            .expect("vendor exists");
        let link = Link {
            source_collection: vendors(),
            source_id: EntityId::new("ven-001"),
            target_collection: accounts(),
            target_id: EntityId::new("inv-001"),
            link_type: "approves".into(),
            metadata: json!({"route": "finance"}),
        };
        let mut tx = Transaction::new();
        tx.update(
            account("inv-001", 125),
            invoice_before.version,
            Some(invoice_before.data.clone()),
        )
        .expect("stage invoice");
        tx.create_link(link.clone()).expect("stage link");
        let intent = transaction_intent(
            "mint_tx_mixed",
            &tx,
            vec![
                entity_pre_image(&invoice_before),
                entity_pre_image(&vendor_before),
            ],
        );
        let token = seed_preview(&mut storage, &svc, intent.clone());
        let expected_tx_id = tx.id.clone();

        let result = svc
            .commit_transaction_intent(
                &mut storage,
                &mut audit,
                MutationIntentTransactionCommitRequest {
                    scope: scope(),
                    token,
                    transaction: tx,
                    canonical_operation: None,
                    current: current_commit_context(&intent),
                    now_ns: 1,
                    actor: Some("system".into()),
                    attribution: None,
                },
            )
            .expect("mixed transaction intent should commit");

        assert_eq!(result.intent.approval_state, ApprovalState::Committed);
        assert_eq!(result.transaction_id, expected_tx_id);
        assert_eq!(result.written.len(), 2);
        assert_eq!(
            storage
                .get(&accounts(), &EntityId::new("inv-001"))
                .expect("read invoice")
                .expect("invoice exists")
                .data["balance"],
            125
        );
        let link_id = Link::storage_id(
            &vendors(),
            &EntityId::new("ven-001"),
            "approves",
            &accounts(),
            &EntityId::new("inv-001"),
        );
        assert!(storage
            .get(&Link::links_collection(), &link_id)
            .expect("read link")
            .is_some());
        assert_eq!(audit.len(), 2);
        for entry in audit.entries() {
            assert_eq!(
                entry.transaction_id.as_deref(),
                Some(expected_tx_id.as_str())
            );
            assert_eq!(
                entry
                    .intent_lineage
                    .as_deref()
                    .map(|lineage| lineage.intent_id.as_str()),
                Some("mint_tx_mixed")
            );
        }
        let intent_entries = audit
            .query_paginated(AuditQuery {
                intent_id: Some("mint_tx_mixed".into()),
                ..AuditQuery::default()
            })
            .expect("committed mutation lineage should be queryable by intent id");
        assert_eq!(intent_entries.entries.len(), 2);
        let update_entry = intent_entries
            .entries
            .iter()
            .find(|entry| entry.mutation == MutationType::EntityUpdate)
            .expect("intent query should include committed entity update audit");
        let lineage = update_entry
            .intent_lineage
            .as_deref()
            .expect("entity update audit should carry intent lineage");
        assert_eq!(lineage.decision, MutationIntentDecision::Allow);
        assert_eq!(lineage.approval_id, None);
        assert_eq!(lineage.policy_version, 7);
        assert_eq!(lineage.schema_version, 7);
        assert_eq!(
            lineage.subject_snapshot.user_id.as_deref(),
            Some("usr_requester")
        );
        assert_eq!(
            lineage.subject_snapshot.agent_id.as_deref(),
            Some("agent_ap")
        );
        assert_eq!(
            lineage.subject_snapshot.credential_id.as_deref(),
            Some("cred_live")
        );
        let balance_diff = update_entry
            .diff
            .as_ref()
            .and_then(|diff| diff.get("balance"))
            .expect("entity update audit should retain committed field diff");
        assert_eq!(balance_diff.before.as_ref(), Some(&json!(100)));
        assert_eq!(balance_diff.after.as_ref(), Some(&json!(125)));
    }

    #[test]
    fn transaction_intent_version_conflict_aborts_without_partial_audit_or_state_change() {
        let mut storage = MemoryStorageAdapter::default();
        let mut audit = MemoryAuditLog::default();
        let svc = service();
        storage.put(account("A", 100)).expect("seed A");
        storage.put(account("B", 50)).expect("seed B");
        let a_before = storage
            .get(&accounts(), &EntityId::new("A"))
            .expect("read A")
            .expect("A exists");
        let b_before = storage
            .get(&accounts(), &EntityId::new("B"))
            .expect("read B")
            .expect("B exists");
        let mut tx = Transaction::new();
        tx.update(
            account("A", 70),
            a_before.version,
            Some(a_before.data.clone()),
        )
        .expect("stage A");
        tx.update(account("B", 80), 99, Some(b_before.data.clone()))
            .expect("stage conflicting B");
        let intent = transaction_intent(
            "mint_tx_conflict",
            &tx,
            vec![entity_pre_image(&a_before), entity_pre_image(&b_before)],
        );
        let token = seed_preview(&mut storage, &svc, intent.clone());

        let err = svc
            .commit_transaction_intent(
                &mut storage,
                &mut audit,
                MutationIntentTransactionCommitRequest {
                    scope: scope(),
                    token,
                    transaction: tx,
                    canonical_operation: None,
                    current: current_commit_context(&intent),
                    now_ns: 1,
                    actor: Some("system".into()),
                    attribution: None,
                },
            )
            .expect_err("transaction conflict should reject commit");

        assert_eq!(err.error_code(), "intent_stale");
        assert_eq!(
            storage
                .get(&accounts(), &EntityId::new("A"))
                .expect("read A")
                .expect("A exists")
                .data["balance"],
            100
        );
        assert_eq!(
            storage
                .get(&accounts(), &EntityId::new("B"))
                .expect("read B")
                .expect("B exists")
                .data["balance"],
            50
        );
        assert_eq!(audit.len(), 0);
        assert_eq!(
            storage
                .get_mutation_intent("acme", "finance", "mint_tx_conflict")
                .expect("read intent")
                .expect("intent exists")
                .approval_state,
            ApprovalState::None
        );
    }

    #[test]
    fn canonical_operation_hash_is_field_order_independent() {
        let first = canonicalize_intent_operation(
            MutationOperationKind::UpdateEntity,
            json!({
                "id": "inv-001",
                "collection": "invoices",
                "expected_version": 3,
                "data": {
                    "amount_cents": 125000,
                    "currency": "USD"
                }
            }),
        );
        let second = canonicalize_intent_operation(
            MutationOperationKind::UpdateEntity,
            json!({
                "data": {
                    "currency": "USD",
                    "amount_cents": 125000
                },
                "expected_version": 3,
                "collection": "invoices",
                "id": "inv-001"
            }),
        );

        assert_eq!(first.operation_hash, second.operation_hash);
        assert_eq!(first.canonical_operation, second.canonical_operation);
    }

    #[test]
    fn canonical_operation_hash_changes_with_payload_or_kind() {
        let base = canonicalize_intent_operation(
            MutationOperationKind::UpdateEntity,
            json!({
                "collection": "invoices",
                "id": "inv-001",
                "expected_version": 3,
                "data": {"amount_cents": 125000}
            }),
        );
        let changed_payload = canonicalize_intent_operation(
            MutationOperationKind::UpdateEntity,
            json!({
                "collection": "invoices",
                "id": "inv-001",
                "expected_version": 3,
                "data": {"amount_cents": 125001}
            }),
        );
        let changed_kind = canonicalize_intent_operation(
            MutationOperationKind::PatchEntity,
            json!({
                "collection": "invoices",
                "id": "inv-001",
                "expected_version": 3,
                "data": {"amount_cents": 125000}
            }),
        );

        assert_ne!(base.operation_hash, changed_payload.operation_hash);
        assert_ne!(base.operation_hash, changed_kind.operation_hash);
    }

    #[test]
    fn canonical_operation_helpers_cover_supported_operation_kinds() {
        let create = canonical_create_entity_operation(&CreateEntityRequest {
            collection: CollectionId::new("invoices"),
            id: EntityId::new("inv-001"),
            data: json!({"amount_cents": 125000}),
            actor: Some("usr_finance".into()),
            audit_metadata: None,
            attribution: None,
        });
        let update = canonical_update_entity_operation(&UpdateEntityRequest {
            collection: CollectionId::new("invoices"),
            id: EntityId::new("inv-001"),
            data: json!({"amount_cents": 125500}),
            expected_version: 3,
            actor: None,
            audit_metadata: None,
            attribution: None,
        });
        let patch = canonical_patch_entity_operation(&PatchEntityRequest {
            collection: CollectionId::new("invoices"),
            id: EntityId::new("inv-001"),
            patch: json!({"status": "approved"}),
            expected_version: 4,
            actor: None,
            audit_metadata: None,
            attribution: None,
        });
        let delete = canonical_delete_entity_operation(&DeleteEntityRequest {
            collection: CollectionId::new("invoices"),
            id: EntityId::new("inv-001"),
            actor: None,
            force: true,
            audit_metadata: None,
            attribution: None,
        });
        let create_link = canonical_create_link_operation(&CreateLinkRequest {
            source_collection: CollectionId::new("vendors"),
            source_id: EntityId::new("ven-001"),
            target_collection: CollectionId::new("invoices"),
            target_id: EntityId::new("inv-001"),
            link_type: "approves".into(),
            metadata: json!({"route": "finance"}),
            actor: None,
            attribution: None,
        });
        let delete_link = canonical_delete_link_operation(&DeleteLinkRequest {
            source_collection: CollectionId::new("vendors"),
            source_id: EntityId::new("ven-001"),
            target_collection: CollectionId::new("invoices"),
            target_id: EntityId::new("inv-001"),
            link_type: "approves".into(),
            actor: None,
            attribution: None,
        });
        let transition = canonical_transition_lifecycle_operation(&TransitionLifecycleRequest {
            collection_id: CollectionId::new("invoices"),
            entity_id: EntityId::new("inv-001"),
            lifecycle_name: "approval".into(),
            target_state: "approved".into(),
            expected_version: 4,
            actor: None,
            audit_metadata: None,
            attribution: None,
        });
        let rollback_entity = canonical_rollback_entity_operation(&RollbackEntityRequest {
            collection: CollectionId::new("invoices"),
            id: EntityId::new("inv-001"),
            target: RollbackEntityTarget::Version(2),
            expected_version: Some(5),
            actor: None,
            dry_run: true,
        });
        let rollback_collection =
            canonical_rollback_collection_operation(&RollbackCollectionRequest {
                collection: CollectionId::new("invoices"),
                timestamp_ns: 12_345,
                actor: None,
                dry_run: true,
            });
        let rollback_transaction =
            canonical_rollback_transaction_operation(&RollbackTransactionRequest {
                transaction_id: "tx-001".into(),
                actor: None,
                dry_run: true,
            });
        let revert = canonical_revert_entity_operation(&RevertEntityRequest {
            audit_entry_id: 42,
            actor: None,
            force: true,
            attribution: None,
        });
        let transaction = canonical_transaction_operation(&[
            CanonicalTransactionOperation::CreateEntity {
                collection: CollectionId::new("invoices"),
                id: EntityId::new("inv-002"),
                data: json!({"amount_cents": 9000}),
            },
            CanonicalTransactionOperation::PatchEntity {
                collection: CollectionId::new("invoices"),
                id: EntityId::new("inv-001"),
                patch: json!({"status": "paid"}),
                expected_version: 5,
            },
        ]);

        let operations = vec![
            (create, MutationOperationKind::CreateEntity),
            (update, MutationOperationKind::UpdateEntity),
            (patch, MutationOperationKind::PatchEntity),
            (delete, MutationOperationKind::DeleteEntity),
            (create_link, MutationOperationKind::CreateLink),
            (delete_link, MutationOperationKind::DeleteLink),
            (transition, MutationOperationKind::Transition),
            (rollback_entity, MutationOperationKind::Rollback),
            (rollback_collection, MutationOperationKind::Rollback),
            (rollback_transaction, MutationOperationKind::Rollback),
            (revert, MutationOperationKind::Revert),
            (transaction, MutationOperationKind::Transaction),
        ];

        for (operation, kind) in operations {
            assert_eq!(operation.operation_kind, kind);
            assert!(operation.operation_hash.starts_with("sha256:"));
            assert_eq!(operation.operation_hash.len(), "sha256:".len() + 64);
            assert!(operation.canonical_operation.is_some());
        }
    }

    #[test]
    fn staged_transaction_operation_canonicalizes_ordered_writes() {
        let mut transaction = Transaction::new();
        transaction
            .create(axon_core::types::Entity::new(
                CollectionId::new("invoices"),
                EntityId::new("inv-001"),
                json!({"amount_cents": 125000}),
            ))
            .expect("create should stage");
        transaction
            .create_link(Link {
                source_collection: CollectionId::new("vendors"),
                source_id: EntityId::new("ven-001"),
                target_collection: CollectionId::new("invoices"),
                target_id: EntityId::new("inv-001"),
                link_type: "approves".into(),
                metadata: json!({"route": "finance"}),
            })
            .expect("link create should stage");

        let operation = canonical_staged_transaction_operation(&transaction);

        assert_eq!(operation.operation_kind, MutationOperationKind::Transaction);
        assert!(operation.operation_hash.starts_with("sha256:"));
        assert_eq!(
            operation.canonical_operation,
            Some(json!({
                "operations": [
                    {
                        "op": "create_entity",
                        "collection": "invoices",
                        "id": "inv-001",
                        "data": {"amount_cents": 125000}
                    },
                    {
                        "op": "create_link",
                        "source_collection": "vendors",
                        "source_id": "ven-001",
                        "target_collection": "invoices",
                        "target_id": "inv-001",
                        "link_type": "approves",
                        "metadata": {"route": "finance"}
                    }
                ]
            }))
        );
    }

    #[test]
    fn commit_with_operation_hash_rejects_caller_supplied_operation_drift() {
        let mut storage = MemoryStorageAdapter::default();
        let svc = service();
        let previewed_operation = canonical_update_entity_operation(&UpdateEntityRequest {
            collection: CollectionId::new("invoices"),
            id: EntityId::new("inv-001"),
            data: json!({"amount_cents": 125000}),
            expected_version: 3,
            actor: None,
            audit_metadata: None,
            attribution: None,
        });
        let drifted_operation = canonical_update_entity_operation(&UpdateEntityRequest {
            collection: CollectionId::new("invoices"),
            id: EntityId::new("inv-001"),
            data: json!({"amount_cents": 125001}),
            expected_version: 3,
            actor: None,
            audit_metadata: None,
            attribution: None,
        });
        let mut intent = intent(
            "mint_drift",
            MutationIntentDecision::Allow,
            ApprovalState::None,
            100,
            false,
        );
        intent.operation = previewed_operation.clone();
        svc.create_preview_record(&mut storage, &mut MemoryAuditLog::default(), intent)
            .expect("preview should persist");

        let err = svc
            .mark_committed_with_operation_hash(
                &mut storage,
                &scope(),
                "mint_drift",
                &drifted_operation.operation_hash,
                1,
            )
            .expect_err("drifted operation hash should fail");
        assert_eq!(
            err,
            MutationIntentLifecycleError::IntentMismatch {
                intent_id: "mint_drift".into(),
                expected_hash: previewed_operation.operation_hash.clone(),
                actual_hash: drifted_operation.operation_hash,
            }
        );
        assert_eq!(err.error_code(), "intent_mismatch");

        let stored = storage
            .get_mutation_intent("acme", "finance", "mint_drift")
            .expect("storage read should succeed")
            .expect("intent should exist");
        assert_eq!(stored.approval_state, ApprovalState::None);

        let committed = svc
            .mark_committed_with_operation_hash(
                &mut storage,
                &scope(),
                "mint_drift",
                &previewed_operation.operation_hash,
                1,
            )
            .expect("matching operation hash should commit");
        assert_eq!(committed.approval_state, ApprovalState::Committed);
    }

    #[test]
    fn create_preview_records_allowed_intent_with_none_state_and_token() {
        let mut storage = MemoryStorageAdapter::default();
        let mut audit = MemoryAuditLog::default();
        let svc = service();
        let record = svc
            .create_preview_record(
                &mut storage,
                &mut audit,
                intent(
                    "mint_allowed",
                    MutationIntentDecision::Allow,
                    ApprovalState::None,
                    100,
                    false,
                ),
            )
            .expect("allowed preview should persist");

        assert_eq!(record.intent.approval_state, ApprovalState::None);
        assert!(record.intent_token.is_some());

        let stored = storage
            .get_mutation_intent("acme", "finance", "mint_allowed")
            .expect("storage read should succeed")
            .expect("intent should exist");
        assert_eq!(stored.approval_state, ApprovalState::None);
        assert_eq!(stored.decision, MutationIntentDecision::Allow);

        let preview_page = audit
            .query_paginated(AuditQuery {
                operation: Some(MutationType::IntentPreview),
                intent_id: Some("mint_allowed".into()),
                ..AuditQuery::default()
            })
            .expect("preview audit should be queryable by intent id");
        assert_eq!(preview_page.entries.len(), 1);
        let preview_entry = &preview_page.entries[0];
        assert_eq!(
            preview_entry.collection,
            CollectionId::new(INTENT_AUDIT_COLLECTION)
        );
        assert_eq!(preview_entry.entity_id, EntityId::new("mint_allowed"));
        assert_eq!(preview_entry.actor, "usr_requester");
        assert_eq!(
            preview_entry
                .data_after
                .as_ref()
                .expect("preview audit should include review summary only")["summary"],
            "Review invoice amount update."
        );
        assert!(
            preview_entry
                .data_after
                .as_ref()
                .expect("preview audit should include review summary only")
                .get("operation")
                .is_none(),
            "preview audit after payload must not include the full intent"
        );
        assert!(preview_entry.data_before.is_none());
        assert_eq!(preview_entry.metadata["decision"], "allow");
        assert_eq!(preview_entry.metadata["schema_version"], "7");
        assert_eq!(preview_entry.metadata["policy_version"], "7");
        assert_eq!(
            preview_entry.metadata["operation_hash"],
            "sha256:mint_allowed"
        );
        assert_eq!(preview_entry.metadata["expires_at"], "100");
        let lineage = preview_entry
            .intent_lineage
            .as_deref()
            .expect("preview audit should include lineage");
        assert_eq!(lineage.intent_id, "mint_allowed");
        assert_eq!(lineage.policy_version, 7);
        assert_eq!(lineage.schema_version, 7);
        assert_eq!(
            lineage.subject_snapshot.agent_id.as_deref(),
            Some("agent_ap")
        );
    }

    #[test]
    fn create_preview_records_needs_approval_as_pending() {
        let mut storage = MemoryStorageAdapter::default();
        let svc = service();
        let record = svc
            .create_preview_record(
                &mut storage,
                &mut MemoryAuditLog::default(),
                intent(
                    "mint_pending",
                    MutationIntentDecision::NeedsApproval,
                    ApprovalState::Pending,
                    100,
                    true,
                ),
            )
            .expect("approval-routed preview should persist");

        assert_eq!(record.intent.approval_state, ApprovalState::Pending);
        assert!(record.intent_token.is_some());

        let pending = storage
            .list_pending_mutation_intents("acme", "finance", 1, None)
            .expect("pending scan should succeed");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].intent_id, "mint_pending");
    }

    #[test]
    fn create_preview_rejects_noncanonical_initial_state() {
        let mut storage = MemoryStorageAdapter::default();
        let svc = service();
        let err = svc
            .create_preview_record(
                &mut storage,
                &mut MemoryAuditLog::default(),
                intent(
                    "mint_bad",
                    MutationIntentDecision::Allow,
                    ApprovalState::Pending,
                    100,
                    false,
                ),
            )
            .expect_err("allow preview must start at none");

        assert_eq!(
            err,
            MutationIntentLifecycleError::InvalidPreviewState {
                intent_id: "mint_bad".into(),
                decision: MutationIntentDecision::Allow,
                actual: ApprovalState::Pending,
                expected: ApprovalState::None,
            }
        );
    }

    #[test]
    fn approve_and_reject_require_reason_when_route_requires_it() {
        let mut storage = MemoryStorageAdapter::default();
        let svc = service();
        svc.create_preview_record(
            &mut storage,
            &mut MemoryAuditLog::default(),
            intent(
                "mint_approve",
                MutationIntentDecision::NeedsApproval,
                ApprovalState::Pending,
                100,
                true,
            ),
        )
        .expect("preview should persist");

        let err = svc
            .approve(&mut storage, &scope(), "mint_approve", metadata(None), 1)
            .expect_err("missing approval reason should fail");
        assert_eq!(
            err,
            MutationIntentLifecycleError::ReasonRequired {
                intent_id: "mint_approve".into(),
                operation: MutationIntentLifecycleOperation::Approve,
            }
        );

        let approved = svc
            .approve(
                &mut storage,
                &scope(),
                "mint_approve",
                metadata(Some("reviewed diff")),
                1,
            )
            .expect("approval with reason should pass");
        assert_eq!(approved.approval_state, ApprovalState::Approved);

        svc.create_preview_record(
            &mut storage,
            &mut MemoryAuditLog::default(),
            intent(
                "mint_reject",
                MutationIntentDecision::NeedsApproval,
                ApprovalState::Pending,
                100,
                true,
            ),
        )
        .expect("preview should persist");
        let err = svc
            .reject(
                &mut storage,
                &scope(),
                "mint_reject",
                metadata(Some("   ")),
                1,
            )
            .expect_err("blank rejection reason should fail");
        assert_eq!(
            err,
            MutationIntentLifecycleError::ReasonRequired {
                intent_id: "mint_reject".into(),
                operation: MutationIntentLifecycleOperation::Reject,
            }
        );

        let rejected = svc
            .reject(
                &mut storage,
                &scope(),
                "mint_reject",
                metadata(Some("not acceptable")),
                1,
            )
            .expect("rejection with reason should pass");
        assert_eq!(rejected.approval_state, ApprovalState::Rejected);
    }

    #[test]
    fn approve_and_reject_with_audit_record_actor_reason_policy_and_intent() {
        let mut storage = MemoryStorageAdapter::default();
        let mut audit = MemoryAuditLog::default();
        let svc = service();
        svc.create_preview_record(
            &mut storage,
            &mut MemoryAuditLog::default(),
            intent(
                "mint_approve_audit",
                MutationIntentDecision::NeedsApproval,
                ApprovalState::Pending,
                100,
                true,
            ),
        )
        .expect("approval preview should persist");

        let approved = svc
            .approve_with_audit(
                &mut storage,
                &mut audit,
                &scope(),
                "mint_approve_audit",
                metadata(Some("approved after review")),
                1,
            )
            .expect("approval should pass");
        assert_eq!(approved.approval_state, ApprovalState::Approved);

        let approve_page = audit
            .query_paginated(AuditQuery {
                operation: Some(MutationType::IntentApprove),
                intent_id: Some("mint_approve_audit".into()),
                ..AuditQuery::default()
            })
            .expect("approval audit should be queryable");
        assert_eq!(approve_page.entries.len(), 1);
        let approve_entry = &approve_page.entries[0];
        assert_eq!(approve_entry.actor, "usr_approver");
        assert_eq!(approve_entry.metadata["reason"], "approved after review");
        assert_eq!(
            approve_entry
                .data_before
                .as_ref()
                .expect("approval audit should include reviewed pre-image")["approval_state"],
            json!("pending")
        );
        assert_eq!(
            approve_entry
                .data_after
                .as_ref()
                .expect("approval audit should include intent snapshot")["approval_state"],
            json!("approved")
        );
        let approve_lineage = approve_entry
            .intent_lineage
            .as_deref()
            .expect("approval audit should include lineage");
        assert_eq!(approve_lineage.intent_id, "mint_approve_audit");
        assert_eq!(approve_lineage.policy_version, 7);
        assert_eq!(
            approve_lineage.reason.as_deref(),
            Some("approved after review")
        );
        assert_eq!(
            approve_lineage
                .approver
                .as_ref()
                .and_then(|approver| approver.actor.as_deref()),
            Some("usr_approver")
        );

        svc.create_preview_record(
            &mut storage,
            &mut MemoryAuditLog::default(),
            intent(
                "mint_reject_audit",
                MutationIntentDecision::NeedsApproval,
                ApprovalState::Pending,
                100,
                true,
            ),
        )
        .expect("rejection preview should persist");
        let rejected = svc
            .reject_with_audit(
                &mut storage,
                &mut audit,
                &scope(),
                "mint_reject_audit",
                metadata(Some("risk too high")),
                1,
            )
            .expect("rejection should pass");
        assert_eq!(rejected.approval_state, ApprovalState::Rejected);

        let reject_page = audit
            .query_paginated(AuditQuery {
                operation: Some(MutationType::IntentReject),
                intent_id: Some("mint_reject_audit".into()),
                ..AuditQuery::default()
            })
            .expect("rejection audit should be queryable");
        assert_eq!(reject_page.entries.len(), 1);
        let reject_entry = &reject_page.entries[0];
        assert_eq!(reject_entry.actor, "usr_approver");
        assert_eq!(reject_entry.metadata["reason"], "risk too high");
        assert_eq!(
            reject_entry
                .data_before
                .as_ref()
                .expect("rejection audit should include reviewed pre-image")["approval_state"],
            json!("pending")
        );
        assert_eq!(
            reject_entry
                .data_after
                .as_ref()
                .expect("rejection audit should include post-image")["approval_state"],
            json!("rejected")
        );
        let reject_lineage = reject_entry
            .intent_lineage
            .as_deref()
            .expect("rejection audit should include lineage");
        assert_eq!(reject_lineage.intent_id, "mint_reject_audit");
        assert_eq!(reject_lineage.policy_version, 7);
        assert_eq!(reject_lineage.reason.as_deref(), Some("risk too high"));
    }

    #[test]
    fn pending_intent_can_expire_only_after_deadline() {
        let mut storage = MemoryStorageAdapter::default();
        let svc = service();
        svc.create_preview_record(
            &mut storage,
            &mut MemoryAuditLog::default(),
            intent(
                "mint_expire",
                MutationIntentDecision::NeedsApproval,
                ApprovalState::Pending,
                10,
                false,
            ),
        )
        .expect("preview should persist");

        let err = svc
            .expire(&mut storage, &scope(), "mint_expire", 9)
            .expect_err("intent should not expire before deadline");
        assert_eq!(
            err,
            MutationIntentLifecycleError::NotExpired {
                intent_id: "mint_expire".into(),
                expires_at: 10,
                now_ns: 9,
            }
        );

        let expired = svc
            .expire(&mut storage, &scope(), "mint_expire", 10)
            .expect("pending intent should expire at deadline");
        assert_eq!(expired.approval_state, ApprovalState::Expired);
    }

    #[test]
    fn due_intents_are_materialized_as_expired_before_review_or_commit() {
        let mut storage = MemoryStorageAdapter::default();
        let svc = service();
        for (id, decision, state) in [
            (
                "mint_approve_expired",
                MutationIntentDecision::NeedsApproval,
                ApprovalState::Pending,
            ),
            (
                "mint_reject_expired",
                MutationIntentDecision::NeedsApproval,
                ApprovalState::Pending,
            ),
            (
                "mint_commit_expired",
                MutationIntentDecision::Allow,
                ApprovalState::None,
            ),
        ] {
            svc.create_preview_record(
                &mut storage,
                &mut MemoryAuditLog::default(),
                intent(id, decision, state, 10, false),
            )
            .expect("preview should persist");
        }

        let approve = svc
            .approve(
                &mut storage,
                &scope(),
                "mint_approve_expired",
                metadata(Some("too late")),
                10,
            )
            .expect_err("expired approval should fail");
        assert_eq!(
            approve,
            MutationIntentLifecycleError::Expired {
                intent_id: "mint_approve_expired".into(),
                operation: MutationIntentLifecycleOperation::Approve,
                expires_at: 10,
                now_ns: 10,
            }
        );
        assert_eq!(approve.error_code(), "intent_expired");

        let reject = svc
            .reject(
                &mut storage,
                &scope(),
                "mint_reject_expired",
                metadata(Some("too late")),
                11,
            )
            .expect_err("expired rejection should fail");
        assert_eq!(
            reject,
            MutationIntentLifecycleError::Expired {
                intent_id: "mint_reject_expired".into(),
                operation: MutationIntentLifecycleOperation::Reject,
                expires_at: 10,
                now_ns: 11,
            }
        );
        assert_eq!(reject.error_code(), "intent_expired");

        let commit = svc
            .mark_committed(&mut storage, &scope(), "mint_commit_expired", 12)
            .expect_err("expired commit should fail");
        assert_eq!(
            commit,
            MutationIntentLifecycleError::Expired {
                intent_id: "mint_commit_expired".into(),
                operation: MutationIntentLifecycleOperation::Commit,
                expires_at: 10,
                now_ns: 12,
            }
        );
        assert_eq!(commit.error_code(), "intent_expired");

        for id in [
            "mint_approve_expired",
            "mint_reject_expired",
            "mint_commit_expired",
        ] {
            let stored = storage
                .get_mutation_intent("acme", "finance", id)
                .expect("storage read should succeed")
                .expect("intent should exist");
            assert_eq!(stored.approval_state, ApprovalState::Expired);
        }
    }

    #[test]
    fn pending_view_excludes_expired_intents_after_lazy_materialization() {
        let mut storage = MemoryStorageAdapter::default();
        let svc = service();
        svc.create_preview_record(
            &mut storage,
            &mut MemoryAuditLog::default(),
            intent(
                "mint_due_pending",
                MutationIntentDecision::NeedsApproval,
                ApprovalState::Pending,
                10,
                false,
            ),
        )
        .expect("due preview should persist");
        svc.create_preview_record(
            &mut storage,
            &mut MemoryAuditLog::default(),
            intent(
                "mint_live_pending",
                MutationIntentDecision::NeedsApproval,
                ApprovalState::Pending,
                100,
                false,
            ),
        )
        .expect("live preview should persist");

        let pending = svc
            .list_pending(&mut storage, &scope(), 10, None)
            .expect("pending view should succeed");
        let pending_ids: Vec<_> = pending
            .iter()
            .map(|intent| intent.intent_id.as_str())
            .collect();
        assert_eq!(pending_ids, vec!["mint_live_pending"]);

        let expired = svc
            .list_by_state(&mut storage, &scope(), ApprovalState::Expired, 10, None)
            .expect("explicit expired history view should succeed");
        let expired_ids: Vec<_> = expired
            .iter()
            .map(|intent| intent.intent_id.as_str())
            .collect();
        assert_eq!(expired_ids, vec!["mint_due_pending"]);
    }

    #[test]
    fn expire_due_with_audit_records_intent_lineage() {
        let mut storage = MemoryStorageAdapter::default();
        let mut audit = MemoryAuditLog::default();
        let svc = service();
        svc.create_preview_record(
            &mut storage,
            &mut MemoryAuditLog::default(),
            intent(
                "mint_due_audited",
                MutationIntentDecision::NeedsApproval,
                ApprovalState::Pending,
                10,
                false,
            ),
        )
        .expect("preview should persist");

        let expired = svc
            .expire_due_with_audit(&mut storage, &mut audit, &scope(), 10, None)
            .expect("audited expiry scan should succeed");
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].approval_state, ApprovalState::Expired);

        let page = audit
            .query_paginated(AuditQuery {
                operation: Some(MutationType::IntentExpire),
                intent_id: Some("mint_due_audited".into()),
                ..AuditQuery::default()
            })
            .expect("intent expiry audit query should succeed");
        assert_eq!(page.entries.len(), 1);

        let entry = &page.entries[0];
        assert_eq!(entry.collection, CollectionId::new(INTENT_AUDIT_COLLECTION));
        assert_eq!(entry.entity_id, EntityId::new("mint_due_audited"));
        assert_eq!(entry.mutation, MutationType::IntentExpire);
        assert_eq!(
            entry
                .data_before
                .as_ref()
                .expect("expiry audit should include pre-expiry intent snapshot")["approval_state"],
            json!("pending")
        );
        assert_eq!(
            entry
                .data_after
                .as_ref()
                .expect("expiry audit should include intent snapshot")["approval_state"],
            json!("expired")
        );

        let lineage = entry
            .intent_lineage
            .as_deref()
            .expect("expiry audit should include intent lineage");
        assert_eq!(lineage.intent_id, "mint_due_audited");
        assert_eq!(lineage.decision, MutationIntentDecision::NeedsApproval);
        assert_eq!(lineage.policy_version, 7);
        assert_eq!(lineage.schema_version, 7);
        assert_eq!(lineage.reason, None);
        let origin = lineage
            .origin
            .as_ref()
            .expect("expiry lineage should include origin");
        assert_eq!(origin.surface, MutationIntentAuditOriginSurface::System);
        assert_eq!(
            origin.operation_hash.as_deref(),
            Some("sha256:mint_due_audited")
        );
    }

    #[test]
    fn commit_stale_and_mismatch_attempts_are_audited_and_queryable() {
        let mut storage = MemoryStorageAdapter::default();
        let mut audit = MemoryAuditLog::default();
        let svc = service();

        let stale_intent = intent(
            "mint_stale_audit",
            MutationIntentDecision::Allow,
            ApprovalState::None,
            100,
            false,
        );
        seed_pre_image_entity(&mut storage, 4);
        let stale_token = seed_preview(&mut storage, &svc, stale_intent.clone());
        let mut stale_current = current_commit_context(&stale_intent);
        stale_current.policy_version = 8;

        let stale_error = svc
            .validate_commit_bindings_with_audit(
                &storage,
                &mut audit,
                MutationIntentCommitValidationAuditRequest {
                    scope: &scope(),
                    token: &stale_token,
                    current: &stale_current,
                    now_ns: 1,
                    actor: Some("commit-agent"),
                },
            )
            .expect_err("stale bindings should reject commit");
        assert_eq!(stale_error.error_code(), "intent_stale");

        let stale_page = audit
            .query_paginated(AuditQuery {
                operation: Some(MutationType::IntentCommit),
                intent_id: Some("mint_stale_audit".into()),
                ..AuditQuery::default()
            })
            .expect("stale commit audit should be queryable");
        assert_eq!(stale_page.entries.len(), 1);
        let stale_entry = &stale_page.entries[0];
        assert_eq!(stale_entry.actor, "commit-agent");
        assert_eq!(stale_entry.metadata["error_code"], "intent_stale");
        assert_eq!(
            stale_entry
                .data_before
                .as_ref()
                .expect("stale audit should include bound intent pre-image")["operation_hash"],
            stale_intent.operation.operation_hash
        );
        let stale_payload = stale_entry
            .data_after
            .as_ref()
            .expect("stale audit should include failure payload");
        assert_eq!(stale_payload["error_code"], "intent_stale");
        assert!(stale_payload["stale"]
            .as_array()
            .is_some_and(|dimensions| !dimensions.is_empty()));
        let stale_lineage = stale_entry
            .intent_lineage
            .as_deref()
            .expect("stale audit should include lineage");
        assert_eq!(stale_lineage.intent_id, "mint_stale_audit");
        assert_eq!(stale_lineage.policy_version, 7);

        let mismatch_intent = intent(
            "mint_mismatch_audit",
            MutationIntentDecision::Allow,
            ApprovalState::None,
            100,
            false,
        );
        let mismatch_token = seed_preview(&mut storage, &svc, mismatch_intent.clone());
        let mut mismatch_current = current_commit_context(&mismatch_intent);
        mismatch_current.operation_hash = "sha256:drifted".into();

        let mismatch_error = svc
            .validate_commit_bindings_with_audit(
                &storage,
                &mut audit,
                MutationIntentCommitValidationAuditRequest {
                    scope: &scope(),
                    token: &mismatch_token,
                    current: &mismatch_current,
                    now_ns: 1,
                    actor: Some("commit-agent"),
                },
            )
            .expect_err("operation mismatch should reject commit");
        assert_eq!(mismatch_error.error_code(), "intent_mismatch");

        let mismatch_page = audit
            .query_paginated(AuditQuery {
                operation: Some(MutationType::IntentCommit),
                intent_id: Some("mint_mismatch_audit".into()),
                ..AuditQuery::default()
            })
            .expect("mismatch commit audit should be queryable");
        assert_eq!(mismatch_page.entries.len(), 1);
        assert_eq!(
            mismatch_page.entries[0]
                .data_before
                .as_ref()
                .expect("mismatch audit should include bound intent pre-image")["operation_hash"],
            mismatch_intent.operation.operation_hash
        );
        let mismatch_payload = mismatch_page.entries[0]
            .data_after
            .as_ref()
            .expect("mismatch audit should include failure payload");
        assert_eq!(mismatch_payload["error_code"], "intent_mismatch");
        assert_eq!(
            mismatch_payload["expected_hash"],
            mismatch_intent.operation.operation_hash
        );
        assert_eq!(mismatch_payload["actual_hash"], "sha256:drifted");
    }

    #[test]
    fn preview_record_does_not_create_entity_or_link_audit_entries() {
        let mut storage = MemoryStorageAdapter::default();
        let mut audit = MemoryAuditLog::default();
        let svc = service();

        svc.create_preview_record(
            &mut storage,
            &mut audit,
            intent(
                "mint_preview_no_mutation_audit",
                MutationIntentDecision::Allow,
                ApprovalState::None,
                100,
                false,
            ),
        )
        .expect("preview should persist");

        assert_eq!(audit.len(), 1);
        assert_eq!(
            audit.entries()[0].mutation,
            MutationType::IntentPreview,
            "preview should be audited as intent lineage, not entity/link mutation"
        );
        assert!(audit
            .query_by_operation(&MutationType::EntityCreate)
            .expect("entity create audit query should pass")
            .is_empty());
        assert!(audit
            .query_by_operation(&MutationType::EntityUpdate)
            .expect("entity update audit query should pass")
            .is_empty());
        assert!(audit
            .query_by_operation(&MutationType::LinkCreate)
            .expect("link create audit query should pass")
            .is_empty());
    }

    #[test]
    fn committed_intent_is_single_use_and_replay_is_rejected() {
        let mut storage = MemoryStorageAdapter::default();
        let svc = service();
        svc.create_preview_record(
            &mut storage,
            &mut MemoryAuditLog::default(),
            intent(
                "mint_commit",
                MutationIntentDecision::Allow,
                ApprovalState::None,
                100,
                false,
            ),
        )
        .expect("preview should persist");

        let committed = svc
            .mark_committed(&mut storage, &scope(), "mint_commit", 1)
            .expect("allowed intent should commit once");
        assert_eq!(committed.approval_state, ApprovalState::Committed);

        let err = svc
            .mark_committed(&mut storage, &scope(), "mint_commit", 1)
            .expect_err("committed intent should reject replay");
        assert_eq!(
            err,
            MutationIntentLifecycleError::InvalidTransition {
                intent_id: "mint_commit".into(),
                operation: MutationIntentLifecycleOperation::Commit,
                from: ApprovalState::Committed,
                to: ApprovalState::Committed,
            }
        );
    }

    #[test]
    fn rejected_and_expired_intents_cannot_be_committed() {
        let mut storage = MemoryStorageAdapter::default();
        let svc = service();
        svc.create_preview_record(
            &mut storage,
            &mut MemoryAuditLog::default(),
            intent(
                "mint_rejected",
                MutationIntentDecision::NeedsApproval,
                ApprovalState::Pending,
                100,
                false,
            ),
        )
        .expect("preview should persist");
        svc.reject(
            &mut storage,
            &scope(),
            "mint_rejected",
            MutationIntentReviewMetadata::default(),
            1,
        )
        .expect("rejection should pass");

        let err = svc
            .mark_committed(&mut storage, &scope(), "mint_rejected", 1)
            .expect_err("rejected intent should not commit");
        assert_eq!(
            err,
            MutationIntentLifecycleError::InvalidTransition {
                intent_id: "mint_rejected".into(),
                operation: MutationIntentLifecycleOperation::Commit,
                from: ApprovalState::Rejected,
                to: ApprovalState::Committed,
            }
        );

        svc.create_preview_record(
            &mut storage,
            &mut MemoryAuditLog::default(),
            intent(
                "mint_expired",
                MutationIntentDecision::NeedsApproval,
                ApprovalState::Pending,
                1,
                false,
            ),
        )
        .expect("preview should persist");
        svc.expire(&mut storage, &scope(), "mint_expired", 1)
            .expect("expiry should pass");

        let err = svc
            .mark_committed(&mut storage, &scope(), "mint_expired", 1)
            .expect_err("expired intent should not commit");
        assert_eq!(
            err,
            MutationIntentLifecycleError::Expired {
                intent_id: "mint_expired".into(),
                operation: MutationIntentLifecycleOperation::Commit,
                expires_at: 1,
                now_ns: 1,
            }
        );
        assert_eq!(err.error_code(), "intent_expired");
    }

    #[test]
    fn invalid_transition_errors_are_explicit() {
        let mut storage = MemoryStorageAdapter::default();
        let svc = service();
        svc.create_preview_record(
            &mut storage,
            &mut MemoryAuditLog::default(),
            intent(
                "mint_allowed",
                MutationIntentDecision::Allow,
                ApprovalState::None,
                100,
                false,
            ),
        )
        .expect("preview should persist");

        let err = svc
            .approve(&mut storage, &scope(), "mint_allowed", metadata(None), 1)
            .expect_err("allowed intent cannot be approved");
        assert_eq!(
            err,
            MutationIntentLifecycleError::InvalidDecisionTransition {
                intent_id: "mint_allowed".into(),
                operation: MutationIntentLifecycleOperation::Approve,
                decision: MutationIntentDecision::Allow,
            }
        );

        svc.create_preview_record(
            &mut storage,
            &mut MemoryAuditLog::default(),
            intent(
                "mint_pending_commit",
                MutationIntentDecision::NeedsApproval,
                ApprovalState::Pending,
                100,
                false,
            ),
        )
        .expect("preview should persist");

        let err = svc
            .mark_committed(&mut storage, &scope(), "mint_pending_commit", 1)
            .expect_err("pending intent cannot commit");
        assert_eq!(
            err,
            MutationIntentLifecycleError::InvalidTransition {
                intent_id: "mint_pending_commit".into(),
                operation: MutationIntentLifecycleOperation::Commit,
                from: ApprovalState::Pending,
                to: ApprovalState::Committed,
            }
        );
    }

    #[test]
    fn expire_due_marks_due_nonterminal_intents() {
        let mut storage = MemoryStorageAdapter::default();
        let svc = service();
        for id in ["mint_due_a", "mint_due_b"] {
            svc.create_preview_record(
                &mut storage,
                &mut MemoryAuditLog::default(),
                intent(
                    id,
                    MutationIntentDecision::NeedsApproval,
                    ApprovalState::Pending,
                    10,
                    false,
                ),
            )
            .expect("preview should persist");
        }
        svc.create_preview_record(
            &mut storage,
            &mut MemoryAuditLog::default(),
            intent(
                "mint_later",
                MutationIntentDecision::NeedsApproval,
                ApprovalState::Pending,
                100,
                false,
            ),
        )
        .expect("preview should persist");

        let expired = svc
            .expire_due(&mut storage, &scope(), 10, None)
            .expect("expiry scan should pass");
        let expired_ids: Vec<_> = expired
            .iter()
            .map(|intent| intent.intent_id.as_str())
            .collect();

        assert_eq!(expired_ids, vec!["mint_due_a", "mint_due_b"]);
        assert_eq!(
            storage
                .get_mutation_intent("acme", "finance", "mint_later")
                .expect("storage read should succeed")
                .expect("intent should exist")
                .approval_state,
            ApprovalState::Pending
        );
    }
}
