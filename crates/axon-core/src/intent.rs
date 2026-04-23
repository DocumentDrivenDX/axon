use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::id::{CollectionId, EntityId, LinkId};

/// Tenant/database scope bound into a mutation intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationIntentScopeBinding {
    /// Tenant route context active when the intent was previewed.
    pub tenant_id: String,
    /// Database route context active when the intent was previewed.
    pub database_id: String,
}

/// Request subject snapshot bound into a mutation intent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct MutationIntentSubjectBinding {
    /// Stable Axon user ID when a human principal exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    /// Stable service or agent identity when delegated or service-originated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Principal that delegated authority to the agent, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegated_by: Option<String>,
    /// Tenant role resolved at preview time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_role: Option<String>,
    /// Credential that authenticated the preview request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_id: Option<String>,
    /// Version of the credential grant snapshot used for the preview.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant_version: Option<u64>,
    /// Request-scoped policy attributes resolved at preview time.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub attributes: HashMap<String, Value>,
}

/// Entity or link version reviewed during mutation preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PreImageBinding {
    /// Entity version reviewed by the caller or approver.
    Entity {
        /// Collection that stores the entity.
        collection: CollectionId,
        /// Entity ID.
        id: EntityId,
        /// Version observed during preview.
        version: u64,
    },
    /// Link version reviewed by the caller or approver.
    Link {
        /// Collection that stores the link record.
        collection: CollectionId,
        /// Link ID.
        id: LinkId,
        /// Version observed during preview.
        version: u64,
    },
}

/// Canonical operation class for an intent-bound write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationOperationKind {
    CreateEntity,
    UpdateEntity,
    PatchEntity,
    DeleteEntity,
    CreateLink,
    DeleteLink,
    Transaction,
    Transition,
    Rollback,
    Revert,
}

/// Canonical operation metadata bound into a mutation intent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalOperationMetadata {
    /// Operation class represented by the canonical payload.
    pub operation_kind: MutationOperationKind,
    /// Hash of the canonical mutation input, usually prefixed with `sha256:`.
    pub operation_hash: String,
    /// Optional canonical payload retained for diagnostics or tests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_operation: Option<Value>,
}

/// Policy envelope decision captured by mutation preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationIntentDecision {
    Allow,
    NeedsApproval,
    Deny,
}

impl MutationIntentDecision {
    /// Returns whether the decision may produce an executable intent token.
    pub fn can_have_executable_token(&self) -> bool {
        matches!(self, Self::Allow | Self::NeedsApproval)
    }

    /// Stable wire/storage string for this decision.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::NeedsApproval => "needs_approval",
            Self::Deny => "deny",
        }
    }
}

/// Review lifecycle state for a mutation intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState {
    None,
    Pending,
    Approved,
    Rejected,
    Expired,
    Committed,
}

impl ApprovalState {
    /// Stable wire/storage string for this lifecycle state.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Expired => "expired",
            Self::Committed => "committed",
        }
    }
}

/// Human approval route captured from a matching policy envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MutationApprovalRoute {
    /// Role whose members can approve the intent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Whether approval must include a human-readable reason.
    #[serde(default)]
    pub reason_required: bool,
    /// Relative approval deadline in seconds from preview time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline_seconds: Option<u64>,
    /// Whether the requester/delegator is forbidden from approving.
    #[serde(default)]
    pub separation_of_duties: bool,
}

/// Approver-safe diff and policy summary for a mutation intent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct MutationReviewSummary {
    /// Short operator-facing title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Human-readable summary of the proposed change.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    /// Optional risk label or explanation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk: Option<String>,
    /// Records affected by the reviewed operation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affected_records: Vec<PreImageBinding>,
    /// Field paths affected by the reviewed operation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affected_fields: Vec<String>,
    /// Computed diff, redacted to the approver-safe view.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub diff: Value,
    /// Policy rule explanations safe to show to operators or agents.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub policy_explanation: Vec<String>,
}

/// Server-side mutation intent record bound to preview, approval, and commit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MutationIntent {
    /// Stable ID for lookup, approval, audit, and token binding.
    pub intent_id: String,
    /// Tenant/database scope binding.
    #[serde(flatten)]
    pub scope: MutationIntentScopeBinding,
    /// Subject and grant binding captured at preview time.
    pub subject: MutationIntentSubjectBinding,
    /// Collection schema version active during preview.
    pub schema_version: u32,
    /// Policy version active during preview.
    pub policy_version: u32,
    /// Canonical operation metadata.
    #[serde(flatten)]
    pub operation: CanonicalOperationMetadata,
    /// Entity and link versions reviewed during preview.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_images: Vec<PreImageBinding>,
    /// Policy envelope decision.
    pub decision: MutationIntentDecision,
    /// Approval lifecycle state.
    pub approval_state: ApprovalState,
    /// Approval route when the decision is `needs_approval`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_route: Option<MutationApprovalRoute>,
    /// Expiration timestamp in nanoseconds since Unix epoch.
    pub expires_at: u64,
    /// Approver-safe review summary.
    #[serde(default)]
    pub review_summary: MutationReviewSummary,
}

impl MutationIntent {
    /// Returns whether this intent's decision may be executed with a token.
    pub fn can_have_executable_token(&self) -> bool {
        self.decision.can_have_executable_token()
    }
}

/// Opaque mutation intent token returned by allowed or approval-routed previews.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MutationIntentToken(String);

impl MutationIntentToken {
    /// Wraps an opaque token string.
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    /// Returns the opaque token string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Signs and verifies opaque mutation intent tokens with a deployment secret.
#[derive(Clone)]
pub struct MutationIntentTokenSigner {
    deployment_secret: Vec<u8>,
}

impl fmt::Debug for MutationIntentTokenSigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MutationIntentTokenSigner")
            .finish_non_exhaustive()
    }
}

impl MutationIntentTokenSigner {
    /// Creates a signer from deployment-local secret bytes.
    pub fn new(deployment_secret: impl Into<Vec<u8>>) -> Self {
        Self {
            deployment_secret: deployment_secret.into(),
        }
    }

    /// Issues an opaque token for the given intent ID.
    pub fn issue_for_id(&self, intent_id: &str) -> MutationIntentToken {
        let intent_id_part = URL_SAFE_NO_PAD.encode(intent_id.as_bytes());
        let signature = hmac_sha256(&self.deployment_secret, intent_id.as_bytes());
        let signature_part = URL_SAFE_NO_PAD.encode(signature);
        MutationIntentToken::new(format!("{intent_id_part}.{signature_part}"))
    }

    /// Issues an opaque token for a stored mutation intent.
    pub fn issue(&self, intent: &MutationIntent) -> MutationIntentToken {
        self.issue_for_id(&intent.intent_id)
    }

    /// Verifies token shape and HMAC, returning the signed intent ID.
    pub fn verify(
        &self,
        token: &MutationIntentToken,
    ) -> Result<String, MutationIntentTokenLookupError> {
        let (intent_id_part, signature_part) = split_token(token.as_str())?;
        let intent_id_bytes = URL_SAFE_NO_PAD
            .decode(intent_id_part)
            .map_err(|_| MutationIntentTokenLookupError::MalformedToken)?;
        if intent_id_bytes.is_empty() {
            return Err(MutationIntentTokenLookupError::MalformedToken);
        }
        let intent_id = String::from_utf8(intent_id_bytes)
            .map_err(|_| MutationIntentTokenLookupError::MalformedToken)?;

        let signature = URL_SAFE_NO_PAD
            .decode(signature_part)
            .map_err(|_| MutationIntentTokenLookupError::MalformedToken)?;
        let expected = hmac_sha256(&self.deployment_secret, intent_id.as_bytes());
        if signature.len() != expected.len() || !constant_time_eq(&signature, &expected) {
            return Err(MutationIntentTokenLookupError::InvalidSignature);
        }

        Ok(intent_id)
    }

    /// Verifies a token, performs a scope-bound lookup, and checks commit state.
    pub fn resolve_for_commit<F>(
        &self,
        token: &MutationIntentToken,
        expected_scope: &MutationIntentScopeBinding,
        now_ns: u64,
        lookup: F,
    ) -> Result<MutationIntent, MutationIntentTokenLookupError>
    where
        F: FnOnce(&str, &MutationIntentScopeBinding) -> Option<MutationIntent>,
    {
        let intent_id = self.verify(token)?;
        let intent =
            lookup(&intent_id, expected_scope).ok_or(MutationIntentTokenLookupError::NotFound)?;
        if &intent.scope != expected_scope {
            return Err(MutationIntentTokenLookupError::TenantDatabaseMismatch);
        }
        validate_intent_commit_state(&intent, now_ns)?;
        Ok(intent)
    }
}

fn split_token(token: &str) -> Result<(&str, &str), MutationIntentTokenLookupError> {
    let mut parts = token.split('.');
    let Some(intent_id_part) = parts.next() else {
        return Err(MutationIntentTokenLookupError::MalformedToken);
    };
    let Some(signature_part) = parts.next() else {
        return Err(MutationIntentTokenLookupError::MalformedToken);
    };
    if parts.next().is_some() || intent_id_part.is_empty() || signature_part.is_empty() {
        return Err(MutationIntentTokenLookupError::MalformedToken);
    }
    Ok((intent_id_part, signature_part))
}

fn validate_intent_commit_state(
    intent: &MutationIntent,
    now_ns: u64,
) -> Result<(), MutationIntentTokenLookupError> {
    if intent.expires_at <= now_ns || intent.approval_state == ApprovalState::Expired {
        return Err(MutationIntentTokenLookupError::Expired);
    }

    match intent.approval_state {
        ApprovalState::Rejected => return Err(MutationIntentTokenLookupError::Rejected),
        ApprovalState::Committed => return Err(MutationIntentTokenLookupError::AlreadyCommitted),
        _ => {}
    }

    match intent.decision {
        MutationIntentDecision::Deny => Err(MutationIntentTokenLookupError::Unauthorized),
        MutationIntentDecision::NeedsApproval
            if intent.approval_state != ApprovalState::Approved =>
        {
            Err(MutationIntentTokenLookupError::ApprovalRequired)
        }
        MutationIntentDecision::Allow | MutationIntentDecision::NeedsApproval => Ok(()),
    }
}

fn hmac_sha256(secret: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK_LEN: usize = 64;

    let mut key_block = [0_u8; BLOCK_LEN];
    if secret.len() > BLOCK_LEN {
        let digest = Sha256::digest(secret);
        key_block[..digest.len()].copy_from_slice(&digest);
    } else {
        key_block[..secret.len()].copy_from_slice(secret);
    }

    let mut inner_pad = [0x36_u8; BLOCK_LEN];
    let mut outer_pad = [0x5c_u8; BLOCK_LEN];
    for index in 0..BLOCK_LEN {
        inner_pad[index] ^= key_block[index];
        outer_pad[index] ^= key_block[index];
    }

    let mut hasher = Sha256::new();
    hasher.update(inner_pad);
    hasher.update(message);
    let inner = hasher.finalize();

    let mut hasher = Sha256::new();
    hasher.update(outer_pad);
    hasher.update(inner);
    let digest = hasher.finalize();

    let mut output = [0_u8; 32];
    output.copy_from_slice(&digest);
    output
}

fn constant_time_eq(actual: &[u8], expected: &[u8; 32]) -> bool {
    if actual.len() != expected.len() {
        return false;
    }
    let mut diff = 0_u8;
    for (actual_byte, expected_byte) in actual.iter().zip(expected.iter()) {
        diff |= actual_byte ^ expected_byte;
    }
    diff == 0
}

/// Intent plus token for decisions that can legally be committed later.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "ExecutableMutationIntentWire",
    into = "ExecutableMutationIntentWire"
)]
pub struct ExecutableMutationIntent {
    intent: MutationIntent,
    intent_token: MutationIntentToken,
}

impl ExecutableMutationIntent {
    /// Creates an executable token binding for an allowed or approval-routed intent.
    pub fn new(
        intent: MutationIntent,
        intent_token: MutationIntentToken,
    ) -> Result<Self, MutationIntentModelError> {
        if intent.can_have_executable_token() {
            Ok(Self {
                intent,
                intent_token,
            })
        } else {
            Err(MutationIntentModelError::DeniedIntentToken {
                intent_id: intent.intent_id,
            })
        }
    }

    /// Intent record referenced by this executable token.
    pub fn intent(&self) -> &MutationIntent {
        &self.intent
    }

    /// Opaque token that resolves to the intent record.
    pub fn intent_token(&self) -> &MutationIntentToken {
        &self.intent_token
    }

    /// Consumes the wrapper and returns the record plus token.
    pub fn into_parts(self) -> (MutationIntent, MutationIntentToken) {
        (self.intent, self.intent_token)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ExecutableMutationIntentWire {
    intent: MutationIntent,
    intent_token: MutationIntentToken,
}

impl TryFrom<ExecutableMutationIntentWire> for ExecutableMutationIntent {
    type Error = MutationIntentModelError;

    fn try_from(value: ExecutableMutationIntentWire) -> Result<Self, Self::Error> {
        Self::new(value.intent, value.intent_token)
    }
}

impl From<ExecutableMutationIntent> for ExecutableMutationIntentWire {
    fn from(value: ExecutableMutationIntent) -> Self {
        let (intent, intent_token) = value.into_parts();
        Self {
            intent,
            intent_token,
        }
    }
}

/// Domain model validation failures for mutation intent values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationIntentModelError {
    /// A denied preview was incorrectly paired with an executable token.
    DeniedIntentToken { intent_id: String },
}

impl fmt::Display for MutationIntentModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeniedIntentToken { intent_id } => write!(
                f,
                "denied mutation intent '{intent_id}' cannot carry an executable token"
            ),
        }
    }
}

impl Error for MutationIntentModelError {}

/// Failures while resolving or validating an opaque mutation intent token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationIntentTokenLookupError {
    MalformedToken,
    InvalidSignature,
    NotFound,
    TenantDatabaseMismatch,
    Expired,
    Rejected,
    AlreadyCommitted,
    ApprovalRequired,
    Unauthorized,
    GrantVersionStale,
    SchemaVersionStale,
    PolicyVersionStale,
    PreImageStale,
    OperationMismatch,
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::{json, Value};

    use super::*;

    fn sample_subject() -> MutationIntentSubjectBinding {
        let mut attributes = HashMap::new();
        attributes.insert("app_role".into(), json!("finance"));

        MutationIntentSubjectBinding {
            user_id: Some("usr_finance_ops".into()),
            agent_id: Some("agent_ap_reconciler".into()),
            delegated_by: Some("usr_director".into()),
            tenant_role: Some("member".into()),
            credential_id: Some("cred_live".into()),
            grant_version: Some(7),
            attributes,
        }
    }

    fn entity_pre_image() -> PreImageBinding {
        PreImageBinding::Entity {
            collection: CollectionId::new("invoices"),
            id: EntityId::new("inv_001"),
            version: 5,
        }
    }

    fn link_pre_image() -> PreImageBinding {
        PreImageBinding::Link {
            collection: CollectionId::new("__axon_links__"),
            id: LinkId::new("vendors/vendor_001/approves/invoices/inv_001"),
            version: 2,
        }
    }

    fn sample_intent(decision: MutationIntentDecision) -> MutationIntent {
        let approval_route = if decision == MutationIntentDecision::NeedsApproval {
            Some(MutationApprovalRoute {
                role: Some("finance_approver".into()),
                reason_required: true,
                deadline_seconds: Some(86_400),
                separation_of_duties: true,
            })
        } else {
            None
        };
        let approval_state = match decision {
            MutationIntentDecision::NeedsApproval => ApprovalState::Pending,
            MutationIntentDecision::Allow | MutationIntentDecision::Deny => ApprovalState::None,
        };

        MutationIntent {
            intent_id: "mint_01H".into(),
            scope: MutationIntentScopeBinding {
                tenant_id: "acme".into(),
                database_id: "finance".into(),
            },
            subject: sample_subject(),
            schema_version: 12,
            policy_version: 12,
            operation: CanonicalOperationMetadata {
                operation_kind: MutationOperationKind::UpdateEntity,
                operation_hash: "sha256:abc123".into(),
                canonical_operation: Some(json!({
                    "collection": "invoices",
                    "id": "inv_001",
                    "patch": {"amount_cents": 1_250_000}
                })),
            },
            pre_images: vec![entity_pre_image(), link_pre_image()],
            decision,
            approval_state,
            approval_route,
            expires_at: 1_766_273_600_000_000_000,
            review_summary: MutationReviewSummary {
                title: Some("Invoice amount change".into()),
                summary: "Update invoice amount before approval.".into(),
                risk: Some("amount_above_autonomous_limit".into()),
                affected_records: vec![entity_pre_image()],
                affected_fields: vec!["amount_cents".into()],
                diff: json!({
                    "amount_cents": {
                        "before": 900_000,
                        "after": 1_250_000
                    }
                }),
                policy_explanation: vec!["require-approval-large-invoice matched".into()],
            },
        }
    }

    #[test]
    fn mutation_intent_roundtrips_all_adr019_binding_fields() {
        let intent = sample_intent(MutationIntentDecision::NeedsApproval);
        let value = serde_json::to_value(&intent).expect("intent should serialize");

        for field in [
            "intent_id",
            "tenant_id",
            "database_id",
            "subject",
            "schema_version",
            "policy_version",
            "operation_kind",
            "operation_hash",
            "pre_images",
            "decision",
            "approval_state",
            "expires_at",
            "approval_route",
            "review_summary",
        ] {
            assert!(
                value.get(field).is_some(),
                "serialized intent should include {field}: {value}"
            );
        }

        assert_eq!(value["subject"]["user_id"], json!("usr_finance_ops"));
        assert_eq!(value["subject"]["agent_id"], json!("agent_ap_reconciler"));
        assert_eq!(value["subject"]["delegated_by"], json!("usr_director"));
        assert_eq!(value["subject"]["credential_id"], json!("cred_live"));
        assert_eq!(value["subject"]["grant_version"], json!(7));
        assert_eq!(value["pre_images"][0]["kind"], json!("entity"));
        assert_eq!(value["pre_images"][1]["kind"], json!("link"));

        let restored: MutationIntent =
            serde_json::from_value(value).expect("intent should deserialize");
        assert_eq!(restored, intent);
    }

    #[test]
    fn approval_states_model_full_lifecycle() {
        let states = vec![
            ApprovalState::None,
            ApprovalState::Pending,
            ApprovalState::Approved,
            ApprovalState::Rejected,
            ApprovalState::Expired,
            ApprovalState::Committed,
        ];
        let value = serde_json::to_value(&states).expect("states should serialize");
        assert_eq!(
            value,
            json!([
                "none",
                "pending",
                "approved",
                "rejected",
                "expired",
                "committed"
            ])
        );

        let restored: Vec<ApprovalState> =
            serde_json::from_value(value).expect("states should deserialize");
        assert_eq!(restored, states);
    }

    #[test]
    fn executable_intent_roundtrips_for_allowed_decision() {
        let executable = ExecutableMutationIntent::new(
            sample_intent(MutationIntentDecision::Allow),
            MutationIntentToken::new("token.parts"),
        )
        .expect("allowed intent can carry a token");

        let value = serde_json::to_value(&executable).expect("executable should serialize");
        assert_eq!(value["intent_token"], json!("token.parts"));

        let restored: ExecutableMutationIntent =
            serde_json::from_value(value).expect("executable should deserialize");
        assert_eq!(restored.intent_token().as_str(), "token.parts");
        assert_eq!(restored.intent().decision, MutationIntentDecision::Allow);
    }

    #[test]
    fn denied_intent_cannot_carry_executable_token() {
        let err = ExecutableMutationIntent::new(
            sample_intent(MutationIntentDecision::Deny),
            MutationIntentToken::new("token.parts"),
        )
        .expect_err("denied intent should reject executable token");

        assert_eq!(
            err,
            MutationIntentModelError::DeniedIntentToken {
                intent_id: "mint_01H".into()
            }
        );
    }

    #[test]
    fn denied_executable_intent_wire_is_rejected() {
        let value = json!({
            "intent": sample_intent(MutationIntentDecision::Deny),
            "intent_token": "token.parts"
        });

        let err =
            serde_json::from_value::<ExecutableMutationIntent>(value).expect_err("wire is invalid");
        assert!(
            err.to_string().contains("cannot carry an executable token"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn intent_token_signing_uses_adr019_base64url_hmac_format() {
        let signer = MutationIntentTokenSigner::new(b"deployment-secret".to_vec());
        let intent_id = "mint_01H";
        let token = signer.issue_for_id(intent_id);
        let parts: Vec<&str> = token.as_str().split('.').collect();

        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], URL_SAFE_NO_PAD.encode(intent_id.as_bytes()));
        assert!(!token.as_str().contains('='));
        assert_eq!(signer.issue_for_id(intent_id), token);
        assert_eq!(
            signer.verify(&token).expect("token should verify"),
            intent_id
        );
    }

    #[test]
    fn valid_token_lookup_returns_scope_bound_intent() {
        let signer = MutationIntentTokenSigner::new(b"deployment-secret".to_vec());
        let intent = sample_intent(MutationIntentDecision::Allow);
        let token = signer.issue(&intent);
        let expected_scope = intent.scope.clone();
        let mut observed_scope = None;

        let resolved = signer
            .resolve_for_commit(&token, &expected_scope, 1, |intent_id, scope| {
                observed_scope = Some(scope.clone());
                assert_eq!(intent_id, intent.intent_id);
                Some(intent.clone())
            })
            .expect("valid token should resolve");

        assert_eq!(observed_scope, Some(expected_scope));
        assert_eq!(resolved.intent_id, "mint_01H");
    }

    #[test]
    fn bad_hmac_is_rejected() {
        let signer = MutationIntentTokenSigner::new(b"deployment-secret".to_vec());
        let verifier = MutationIntentTokenSigner::new(b"different-secret".to_vec());
        let token = signer.issue_for_id("mint_01H");

        assert_eq!(
            verifier.verify(&token),
            Err(MutationIntentTokenLookupError::InvalidSignature)
        );
    }

    #[test]
    fn malformed_tokens_are_rejected_before_lookup() {
        let signer = MutationIntentTokenSigner::new(b"deployment-secret".to_vec());
        let malformed = [
            "",
            "one-part",
            "too.many.parts",
            ".signature",
            "intent.",
            "not base64.signature",
            "a.not base64",
        ];

        for raw in malformed {
            let token = MutationIntentToken::new(raw);
            assert_eq!(
                signer.verify(&token),
                Err(MutationIntentTokenLookupError::MalformedToken),
                "token should be malformed: {raw}"
            );
        }
    }

    #[test]
    fn wrong_tenant_database_scope_is_rejected_after_lookup() {
        let signer = MutationIntentTokenSigner::new(b"deployment-secret".to_vec());
        let intent = sample_intent(MutationIntentDecision::Allow);
        let token = signer.issue(&intent);
        let wrong_scope = MutationIntentScopeBinding {
            tenant_id: "other-tenant".into(),
            database_id: intent.scope.database_id.clone(),
        };

        let err = signer
            .resolve_for_commit(&token, &wrong_scope, 1, |_intent_id, _scope| {
                Some(intent.clone())
            })
            .expect_err("scope mismatch should fail");

        assert_eq!(err, MutationIntentTokenLookupError::TenantDatabaseMismatch);
    }

    #[test]
    fn expired_token_lookup_is_rejected() {
        let signer = MutationIntentTokenSigner::new(b"deployment-secret".to_vec());
        let mut intent = sample_intent(MutationIntentDecision::Allow);
        intent.expires_at = 99;
        let token = signer.issue(&intent);

        let err = signer
            .resolve_for_commit(&token, &intent.scope, 99, |_intent_id, _scope| {
                Some(intent.clone())
            })
            .expect_err("expired token should fail");

        assert_eq!(err, MutationIntentTokenLookupError::Expired);
    }

    #[test]
    fn token_verification_alone_does_not_authorize_commit() {
        let signer = MutationIntentTokenSigner::new(b"deployment-secret".to_vec());
        let intent = sample_intent(MutationIntentDecision::NeedsApproval);
        let token = signer.issue(&intent);

        assert_eq!(
            signer.verify(&token).expect("signature should verify"),
            intent.intent_id
        );
        let err = signer
            .resolve_for_commit(&token, &intent.scope, 1, |_intent_id, _scope| {
                Some(intent.clone())
            })
            .expect_err("pending approval should not commit");

        assert_eq!(err, MutationIntentTokenLookupError::ApprovalRequired);
    }

    #[test]
    fn token_lookup_errors_roundtrip() {
        let errors = vec![
            MutationIntentTokenLookupError::MalformedToken,
            MutationIntentTokenLookupError::InvalidSignature,
            MutationIntentTokenLookupError::NotFound,
            MutationIntentTokenLookupError::TenantDatabaseMismatch,
            MutationIntentTokenLookupError::Expired,
            MutationIntentTokenLookupError::Rejected,
            MutationIntentTokenLookupError::AlreadyCommitted,
            MutationIntentTokenLookupError::ApprovalRequired,
            MutationIntentTokenLookupError::Unauthorized,
            MutationIntentTokenLookupError::GrantVersionStale,
            MutationIntentTokenLookupError::SchemaVersionStale,
            MutationIntentTokenLookupError::PolicyVersionStale,
            MutationIntentTokenLookupError::PreImageStale,
            MutationIntentTokenLookupError::OperationMismatch,
        ];
        let value = serde_json::to_value(&errors).expect("errors should serialize");
        assert_eq!(
            value,
            json!([
                "malformed_token",
                "invalid_signature",
                "not_found",
                "tenant_database_mismatch",
                "expired",
                "rejected",
                "already_committed",
                "approval_required",
                "unauthorized",
                "grant_version_stale",
                "schema_version_stale",
                "policy_version_stale",
                "pre_image_stale",
                "operation_mismatch"
            ])
        );

        let restored: Vec<MutationIntentTokenLookupError> =
            serde_json::from_value(value).expect("errors should deserialize");
        assert_eq!(restored, errors);
    }

    #[test]
    fn review_summary_defaults_skip_empty_optional_fields() {
        let value = serde_json::to_value(MutationReviewSummary::default())
            .expect("summary should serialize");
        assert_eq!(value, Value::Object(Default::default()));
    }
}
