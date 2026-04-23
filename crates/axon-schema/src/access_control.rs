//! Schema-adjacent access-control metadata (FEAT-029 / ADR-019).
//!
//! These types model the authoring surface stored under ESF `access_control`.
//! They intentionally do not evaluate policy. Runtime policy compilation and
//! enforcement consume this typed AST in later layers.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Collection-local access-control policy declared in ESF.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AccessControlPolicy {
    /// Maps authenticated request context into policy subject fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<AccessControlIdentity>,

    /// Row/entity read policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read: Option<OperationPolicy>,

    /// Entity/link creation policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create: Option<OperationPolicy>,

    /// Entity/link update policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update: Option<OperationPolicy>,

    /// Entity/link delete policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete: Option<OperationPolicy>,

    /// Write shorthand policy covering create, update, and delete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write: Option<OperationPolicy>,

    /// Schema/policy administration policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admin: Option<OperationPolicy>,

    /// Field-level read redaction and write rules, keyed by field path.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub fields: HashMap<String, FieldPolicy>,

    /// Lifecycle transition rules: lifecycle field -> transition name -> policy.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub transitions: HashMap<String, HashMap<String, OperationPolicy>>,

    /// Approval/decision envelopes keyed by operation class.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub envelopes: HashMap<PolicyOperation, Vec<PolicyEnvelope>>,
}

/// Identity metadata used by policy predicates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AccessControlIdentity {
    /// Canonical subject mappings, e.g. `user_id: subject.user_id`.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub subject: HashMap<String, String>,

    /// Request-scoped application attributes.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub attributes: HashMap<String, IdentityAttributeSource>,

    /// Backward-compatible shorthand mappings, e.g. `role:
    /// subject.attributes.user_role`.
    #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
    pub aliases: HashMap<String, String>,
}

/// Declarative source for a request-scoped subject attribute.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IdentityAttributeSource {
    /// Source kind, currently authoring examples use `collection`.
    pub from: String,
    /// Collection to read from when `from = collection`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collection: Option<String>,
    /// Entity field matched against the subject.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_field: Option<String>,
    /// Subject field used as the lookup key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_subject: Option<String>,
    /// Entity field whose value becomes the attribute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_field: Option<String>,
}

/// A row/entity operation policy with explicit allow and deny rule lists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OperationPolicy {
    /// Rules that can allow an operation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<PolicyRule>,

    /// Rules that can deny an operation. A matching deny overrides allow.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny: Vec<PolicyRule>,
}

/// Field-level policy for reads and writes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct FieldPolicy {
    /// Field read redaction rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read: Option<FieldAccessPolicy>,

    /// Field write allow/deny rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write: Option<FieldAccessPolicy>,
}

/// Field access rule lists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct FieldAccessPolicy {
    /// Rules that can allow field access.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<FieldPolicyRule>,

    /// Rules that can deny field access.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny: Vec<FieldPolicyRule>,
}

/// Named operation or row rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PolicyRule {
    /// Stable policy rule name used in explanations and denial details.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Subject, field, operation, or boolean predicate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<PolicyPredicate>,

    /// Row/relationship predicate.
    #[serde(default, rename = "where", skip_serializing_if = "Option::is_none")]
    pub where_clause: Option<PolicyPredicate>,
}

/// Named field rule with optional read redaction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct FieldPolicyRule {
    /// Stable policy rule name used in explanations and denial details.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Subject, field, operation, or boolean predicate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<PolicyPredicate>,

    /// Row/relationship predicate.
    #[serde(default, rename = "where", skip_serializing_if = "Option::is_none")]
    pub where_clause: Option<PolicyPredicate>,

    /// Redaction value for denied reads. FEAT-029 examples use JSON `null`.
    #[serde(
        default,
        deserialize_with = "deserialize_optional_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub redact_as: Option<Value>,
}

fn deserialize_optional_value<'de, D>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Value::deserialize(deserializer).map(Some)
}

/// Approval and direct-commit decision envelope for a write operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyEnvelope {
    /// Stable envelope name used in explanations and review summaries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Predicate that selects this envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<PolicyPredicate>,

    /// Decision returned when this envelope matches.
    pub decision: PolicyDecision,

    /// Approval route required when the decision is `needs_approval`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<ApprovalRoute>,
}

/// Policy operation classes from FEAT-029.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyOperation {
    Read,
    Create,
    Update,
    Delete,
    Write,
    Admin,
}

impl PolicyOperation {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::Write => "write",
            Self::Admin => "admin",
        }
    }
}

/// Policy decisions from ADR-019.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyDecision {
    Allow,
    NeedsApproval,
    Deny,
}

/// Human approval routing metadata for `needs_approval` decisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ApprovalRoute {
    /// Role whose members can approve the intent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,

    /// Whether approval must carry a human-readable reason.
    #[serde(default)]
    pub reason_required: bool,

    /// Relative approval deadline in seconds from intent creation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline_seconds: Option<u64>,

    /// Whether the requester/delegator is forbidden from approving.
    #[serde(default)]
    pub separation_of_duties: bool,
}

impl From<PolicyDecision> for axon_core::intent::MutationIntentDecision {
    fn from(decision: PolicyDecision) -> Self {
        match decision {
            PolicyDecision::Allow => Self::Allow,
            PolicyDecision::NeedsApproval => Self::NeedsApproval,
            PolicyDecision::Deny => Self::Deny,
        }
    }
}

impl From<&PolicyDecision> for axon_core::intent::MutationIntentDecision {
    fn from(decision: &PolicyDecision) -> Self {
        match decision {
            PolicyDecision::Allow => Self::Allow,
            PolicyDecision::NeedsApproval => Self::NeedsApproval,
            PolicyDecision::Deny => Self::Deny,
        }
    }
}

impl From<ApprovalRoute> for axon_core::intent::MutationApprovalRoute {
    fn from(route: ApprovalRoute) -> Self {
        Self {
            role: route.role,
            reason_required: route.reason_required,
            deadline_seconds: route.deadline_seconds,
            separation_of_duties: route.separation_of_duties,
        }
    }
}

impl From<&ApprovalRoute> for axon_core::intent::MutationApprovalRoute {
    fn from(route: &ApprovalRoute) -> Self {
        Self {
            role: route.role.clone(),
            reason_required: route.reason_required,
            deadline_seconds: route.deadline_seconds,
            separation_of_duties: route.separation_of_duties,
        }
    }
}

/// Declarative policy predicate grammar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PolicyPredicate {
    /// All sub-predicates must match.
    All { all: Vec<PolicyPredicate> },
    /// Any sub-predicate may match.
    Any { any: Vec<PolicyPredicate> },
    /// Invert a sub-predicate.
    Not { not: Box<PolicyPredicate> },
    /// Subject field comparison.
    Subject {
        subject: String,
        #[serde(flatten)]
        op: PolicyCompareOp,
    },
    /// Entity field comparison.
    Field {
        field: String,
        #[serde(flatten)]
        op: PolicyCompareOp,
    },
    /// Operation class predicate, used by envelopes.
    Operation { operation: PolicyOperation },
    /// Relationship-backed row predicate.
    Related { related: RelationshipPredicate },
    /// Shared-relation row predicate.
    SharesRelation {
        shares_relation: SharesRelationPredicate,
    },
}

/// Comparison operators used by field and subject predicates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyCompareOp {
    Eq(Value),
    Ne(Value),
    In(Vec<Value>),
    NotNull(bool),
    IsNull(bool),
    Gt(Value),
    Gte(Value),
    Lt(Value),
    Lte(Value),
    ContainsSubject(String),
    EqSubject(String),
}

/// Link-backed relationship predicate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelationshipPredicate {
    pub link_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<LinkDirection>,
    pub target_collection: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_policy: Option<PolicyOperation>,
}

/// Link traversal direction for relationship-backed predicates.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkDirection {
    Incoming,
    Outgoing,
}

/// Predicate proving two rows share a configured relation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SharesRelationPredicate {
    pub collection: String,
    pub field: String,
    pub subject_field: String,
    pub target_field: String,
}

#[cfg(test)]
mod tests {
    use axon_core::intent::{MutationApprovalRoute, MutationIntentDecision};
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn policy_decision_and_approval_route_convert_to_intent_types() {
        assert_eq!(
            MutationIntentDecision::from(PolicyDecision::Allow),
            MutationIntentDecision::Allow
        );
        assert_eq!(
            MutationIntentDecision::from(PolicyDecision::NeedsApproval),
            MutationIntentDecision::NeedsApproval
        );
        assert_eq!(
            MutationIntentDecision::from(PolicyDecision::Deny),
            MutationIntentDecision::Deny
        );

        let route = ApprovalRoute {
            role: Some("finance_approver".into()),
            reason_required: true,
            deadline_seconds: Some(3_600),
            separation_of_duties: true,
        };
        assert_eq!(
            MutationApprovalRoute::from(&route),
            MutationApprovalRoute {
                role: Some("finance_approver".into()),
                reason_required: true,
                deadline_seconds: Some(3_600),
                separation_of_duties: true,
            }
        );
    }
}
