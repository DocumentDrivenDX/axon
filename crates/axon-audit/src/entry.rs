use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use axon_core::id::{CollectionId, EntityId};
use axon_core::intent::{MutationIntentDecision, MutationIntentSubjectBinding};

/// The type of mutation recorded in an audit entry.
///
/// Entity operations cover individual entity CRUD; collection and schema
/// operations cover infrastructure-level lifecycle events.
///
/// The `Display` impl produces the FEAT-003 dot-notation format used in API
/// responses and query filters (e.g. `entity.create`, `collection.drop`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationType {
    // ── Entity operations ────────────────────────────────────────────────────
    EntityCreate,
    EntityUpdate,
    EntityDelete,
    /// An entity was reverted to a previous state from an audit entry.
    EntityRevert,
    // ── Link operations ─────────────────────────────────────────────────────
    LinkCreate,
    LinkDelete,
    // ── Collection lifecycle ─────────────────────────────────────────────────
    CollectionCreate,
    CollectionDrop,
    // ── Collection view / template operations ───────────────────────────────
    TemplateCreate,
    TemplateUpdate,
    TemplateDelete,
    // ── Schema operations ────────────────────────────────────────────────────
    SchemaUpdate,
    // ── Guardrail rejections (FEAT-022 / ADR-016) ───────────────────────────
    /// A mutation was rejected by the agent guardrails layer (rate limit or
    /// scope violation). Distinct from regular mutations — no entity state
    /// changed, but operators need to see the rejection in the audit trail.
    GuardrailRejection,
    // ── Mutation intent lifecycle (FEAT-030) ────────────────────────────────
    /// A mutation intent preview was evaluated and recorded.
    IntentPreview,
    /// A pending mutation intent was approved.
    IntentApprove,
    /// A pending mutation intent was rejected.
    IntentReject,
    /// A pending or executable mutation intent expired.
    IntentExpire,
    /// An executable mutation intent was consumed by a committed write.
    IntentCommit,
}

impl std::fmt::Display for MutationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            MutationType::EntityCreate => "entity.create",
            MutationType::EntityUpdate => "entity.update",
            MutationType::EntityDelete => "entity.delete",
            MutationType::EntityRevert => "entity.revert",
            MutationType::LinkCreate => "link.create",
            MutationType::LinkDelete => "link.delete",
            MutationType::CollectionCreate => "collection.create",
            MutationType::CollectionDrop => "collection.drop",
            MutationType::TemplateCreate => "template.create",
            MutationType::TemplateUpdate => "template.update",
            MutationType::TemplateDelete => "template.delete",
            MutationType::SchemaUpdate => "schema.update",
            MutationType::GuardrailRejection => "guardrail_rejection",
            MutationType::IntentPreview => "intent.preview",
            MutationType::IntentApprove => "intent.approve",
            MutationType::IntentReject => "intent.reject",
            MutationType::IntentExpire => "intent.expire",
            MutationType::IntentCommit => "intent.commit",
        };
        f.write_str(s)
    }
}

/// A per-field diff: captures the before and after value for a single key.
///
/// A `None` `before` means the field was added; a `None` `after` means it was removed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldDiff {
    /// Value before the mutation (absent if the field was newly added).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<Value>,
    /// Value after the mutation (absent if the field was removed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<Value>,
}

/// Computes a field-level diff between two JSON objects.
///
/// Only top-level keys that differ between `before` and `after` are included.
/// If either argument is not a JSON object the function returns an empty map —
/// callers should store the full `data_before` / `data_after` for non-object values.
pub fn compute_diff(before: &Value, after: &Value) -> HashMap<String, FieldDiff> {
    let mut diff = HashMap::new();

    let (Some(before_obj), Some(after_obj)) = (before.as_object(), after.as_object()) else {
        return diff;
    };

    let all_keys: HashSet<&String> = before_obj.keys().chain(after_obj.keys()).collect();

    for key in all_keys {
        let b = before_obj.get(key);
        let a = after_obj.get(key);
        if b != a {
            diff.insert(
                key.clone(),
                FieldDiff {
                    before: b.cloned(),
                    after: a.cloned(),
                },
            );
        }
    }

    diff
}

/// ADR-018 attribution tuple: stable identity + authenticating credential.
///
/// Carried on every audit entry that was authenticated via the ADR-018
/// pipeline. Enables post-hoc forensic queries even after renames,
/// suspensions, or revocations.
///
/// Field semantics (ADR-018 Implementation Notes):
///
/// - `user_id` — stable UUID of the User record. Does NOT change across
///   display_name/email edits.
/// - `tenant_id` — the tenant the action took place within. Stable UUID.
/// - `jti` — the JWT credential ID that authenticated this request, when
///   the auth path was JWT. `None` for Tailscale whois synthetic claims
///   and for --no-auth mode.
/// - `auth_method` — which path produced the identity: "jwt", "tailscale",
///   or "no-auth". Stable string matched by the observability envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditAttribution {
    pub user_id: String,   // serialized UserId/Uuid
    pub tenant_id: String, // serialized TenantId/Uuid
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,
    pub auth_method: String,
}

/// Structured lineage metadata for FEAT-030 mutation-intent audit events.
///
/// The legacy `metadata` map remains available for caller-defined tags. This
/// structure carries the stable intent fields operators need for replayable
/// approvals, policy explanations, and lineage queries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MutationIntentAuditMetadata {
    /// Stable mutation intent identifier.
    pub intent_id: String,
    /// Policy decision captured at preview time.
    pub decision: MutationIntentDecision,
    /// Stable approval record identifier, when a human review exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
    /// Policy version active when the decision was produced.
    pub policy_version: u32,
    /// Collection schema version active when the preview was produced.
    pub schema_version: u32,
    /// Subject and grant snapshot bound to the preview request.
    #[serde(default)]
    pub subject_snapshot: MutationIntentSubjectBinding,
    /// Approver identity snapshot, present for approval/rejection decisions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approver: Option<MutationIntentApproverMetadata>,
    /// Human-readable approval or rejection reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// API/tool surface that produced the intent lifecycle event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<MutationIntentAuditOrigin>,
    /// Links to related audit entries, intents, or approval records.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lineage_links: Vec<MutationIntentLineageLink>,
}

/// Approver identity snapshot stored on intent-lineage audit metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MutationIntentApproverMetadata {
    /// Stable user ID for the approver, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    /// Human-readable actor label captured when the decision occurred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    /// Tenant role resolved for the approver.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_role: Option<String>,
    /// Credential used for the approval decision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_id: Option<String>,
}

/// Request surface that originated a mutation-intent lifecycle audit event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationIntentAuditOrigin {
    /// Protocol or application surface that produced the event.
    pub surface: MutationIntentAuditOriginSurface,
    /// MCP or API tool name, when the event came from a tool call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// Request correlation ID, when supplied by the gateway/tool host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Canonical operation hash associated with the intent, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_hash: Option<String>,
}

/// Origin surface for mutation-intent audit events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationIntentAuditOriginSurface {
    Graphql,
    Rest,
    Mcp,
    Cli,
    System,
}

/// Typed relationship from one intent audit event to another lineage object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationIntentLineageLink {
    /// Relationship represented by this link.
    pub relation: MutationIntentLineageRelation,
    /// Related audit entry ID, when the target is already durable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_id: Option<u64>,
    /// Related mutation intent ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent_id: Option<String>,
    /// Related approval record ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
}

/// Relationship kind for intent-lineage links.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationIntentLineageRelation {
    PreviewedBy,
    ApprovedBy,
    RejectedBy,
    ExpiredBy,
    CommittedBy,
    Supersedes,
    RelatedTo,
}

/// A single immutable record in the audit log.
///
/// Fields follow the FEAT-003 specification:
/// <https://github.com/easylabz/axon/docs/helix/01-frame/features/FEAT-003-audit-log.md>
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Sequential, monotonically increasing log entry ID (assigned by the log on append).
    pub id: u64,
    /// Server-assigned timestamp: nanoseconds since Unix epoch.
    pub timestamp_ns: u64,
    /// The collection affected.
    pub collection: CollectionId,
    /// The entity affected (empty for collection-lifecycle entries).
    pub entity_id: EntityId,
    /// The entity version after this mutation.
    pub version: u64,
    /// The kind of mutation.
    pub mutation: MutationType,
    /// Snapshot of the entity data before the mutation (None for creates).
    pub data_before: Option<Value>,
    /// Snapshot of the entity data after the mutation (None for deletes).
    pub data_after: Option<Value>,
    /// Structured field-level diff (populated for entity updates; None otherwise).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<HashMap<String, FieldDiff>>,
    /// Identity of the actor who performed the mutation.
    /// Defaults to "anonymous" when no actor is provided by the caller.
    pub actor: String,
    /// Optional caller-supplied key-value metadata (reason, correlation ID, etc.).
    pub metadata: HashMap<String, String>,
    /// If this mutation was part of a multi-entity transaction, this field
    /// holds the shared transaction identifier. All entries in the same
    /// transaction share the same `transaction_id`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction_id: Option<String>,
    /// ADR-018 attribution tuple. Present when the request was authenticated
    /// via the middleware pipeline. Authoritative for post-hoc forensic
    /// queries; the `actor` field is a human-readable display string that
    /// may drift over time (e.g. after a rename).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribution: Option<AuditAttribution>,
    /// Structured mutation-intent lineage metadata. Optional so existing
    /// entity/link audit entries keep their stable serialized shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent_lineage: Option<Box<MutationIntentAuditMetadata>>,
}

impl AuditEntry {
    /// Convenience constructor. Sets `id` to 0 and `timestamp_ns` to 0;
    /// the [`crate::log::AuditLog`] implementation assigns real values on append.
    ///
    /// For mutations where both `data_before` and `data_after` are `Some`,
    /// a structured field-level `diff` is computed automatically via [`compute_diff`].
    pub fn new(
        collection: CollectionId,
        entity_id: EntityId,
        version: u64,
        mutation: MutationType,
        data_before: Option<Value>,
        data_after: Option<Value>,
        actor: Option<String>,
    ) -> Self {
        let diff = match (&data_before, &data_after) {
            (Some(before), Some(after)) => {
                let d = compute_diff(before, after);
                if d.is_empty() {
                    None
                } else {
                    Some(d)
                }
            }
            _ => None,
        };

        Self {
            id: 0,
            timestamp_ns: 0,
            collection,
            entity_id,
            version,
            mutation,
            data_before,
            data_after,
            diff,
            actor: actor.unwrap_or_else(|| "anonymous".into()),
            metadata: HashMap::new(),
            transaction_id: None,
            attribution: None,
            intent_lineage: None,
        }
    }

    /// Attach caller-supplied key-value metadata to this entry (builder style).
    pub fn with_metadata(mut self, metadata: HashMap<String, String>) -> Self {
        self.metadata = metadata;
        self
    }

    /// Attach an ADR-018 attribution tuple to this entry (builder style).
    ///
    /// Call this after [`AuditEntry::new`] when the request was authenticated
    /// via the middleware pipeline and a [`AuditAttribution`] is available.
    pub fn with_attribution(mut self, attribution: AuditAttribution) -> Self {
        self.attribution = Some(attribution);
        self
    }

    /// Attach structured FEAT-030 mutation-intent lineage metadata.
    pub fn with_intent_lineage(mut self, lineage: MutationIntentAuditMetadata) -> Self {
        self.intent_lineage = Some(Box::new(lineage));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn must_some<'a, T>(value: Option<&'a T>, context: &str) -> &'a T {
        match value {
            Some(value) => value,
            None => panic!("{context}"),
        }
    }

    fn must_owned_some<T>(value: Option<T>, context: &str) -> T {
        match value {
            Some(value) => value,
            None => panic!("{context}"),
        }
    }

    fn must_ok<T, E: std::fmt::Debug>(result: Result<T, E>, context: &str) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("{context}: {error:?}"),
        }
    }

    fn sample_intent_lineage() -> MutationIntentAuditMetadata {
        MutationIntentAuditMetadata {
            intent_id: "intent-123".into(),
            decision: MutationIntentDecision::NeedsApproval,
            approval_id: Some("approval-456".into()),
            policy_version: 12,
            schema_version: 7,
            subject_snapshot: MutationIntentSubjectBinding {
                user_id: Some("user-1".into()),
                agent_id: Some("agent-9".into()),
                delegated_by: Some("delegate-1".into()),
                tenant_role: Some("maintainer".into()),
                credential_id: Some("cred-1".into()),
                grant_version: Some(3),
                attributes: HashMap::from([("risk".into(), json!("high"))]),
            },
            approver: Some(MutationIntentApproverMetadata {
                user_id: Some("approver-1".into()),
                actor: Some("ops-admin".into()),
                tenant_role: Some("admin".into()),
                credential_id: Some("approval-cred".into()),
            }),
            reason: Some("break-glass maintenance".into()),
            origin: Some(MutationIntentAuditOrigin {
                surface: MutationIntentAuditOriginSurface::Mcp,
                tool_name: Some("axon.mutate".into()),
                request_id: Some("req-789".into()),
                operation_hash: Some("sha256:abc".into()),
            }),
            lineage_links: vec![MutationIntentLineageLink {
                relation: MutationIntentLineageRelation::PreviewedBy,
                audit_id: Some(41),
                intent_id: Some("intent-123".into()),
                approval_id: None,
            }],
        }
    }

    #[test]
    fn mutation_type_display_dot_notation() {
        assert_eq!(MutationType::EntityCreate.to_string(), "entity.create");
        assert_eq!(MutationType::EntityUpdate.to_string(), "entity.update");
        assert_eq!(MutationType::EntityDelete.to_string(), "entity.delete");
        assert_eq!(MutationType::EntityRevert.to_string(), "entity.revert");
        assert_eq!(MutationType::LinkCreate.to_string(), "link.create");
        assert_eq!(MutationType::LinkDelete.to_string(), "link.delete");
        assert_eq!(
            MutationType::CollectionCreate.to_string(),
            "collection.create"
        );
        assert_eq!(MutationType::CollectionDrop.to_string(), "collection.drop");
        assert_eq!(MutationType::TemplateCreate.to_string(), "template.create");
        assert_eq!(MutationType::TemplateUpdate.to_string(), "template.update");
        assert_eq!(MutationType::TemplateDelete.to_string(), "template.delete");
        assert_eq!(MutationType::SchemaUpdate.to_string(), "schema.update");
        assert_eq!(
            MutationType::GuardrailRejection.to_string(),
            "guardrail_rejection"
        );
        assert_eq!(MutationType::IntentPreview.to_string(), "intent.preview");
        assert_eq!(MutationType::IntentApprove.to_string(), "intent.approve");
        assert_eq!(MutationType::IntentReject.to_string(), "intent.reject");
        assert_eq!(MutationType::IntentExpire.to_string(), "intent.expire");
        assert_eq!(MutationType::IntentCommit.to_string(), "intent.commit");
    }

    #[test]
    fn audit_entry_create() {
        let entry = AuditEntry::new(
            CollectionId::new("tasks"),
            EntityId::new("t-001"),
            1,
            MutationType::EntityCreate,
            None,
            Some(json!({"title": "hello"})),
            Some("agent-1".into()),
        );
        assert_eq!(entry.mutation, MutationType::EntityCreate);
        assert_eq!(entry.version, 1);
        assert_eq!(entry.actor, "agent-1");
        assert!(entry.data_before.is_none());
    }

    #[test]
    fn audit_entry_anonymous_actor_default() {
        let entry = AuditEntry::new(
            CollectionId::new("tasks"),
            EntityId::new("t-001"),
            1,
            MutationType::EntityCreate,
            None,
            None,
            None,
        );
        assert_eq!(entry.actor, "anonymous");
    }

    #[test]
    fn compute_diff_detects_changed_fields() {
        let before = json!({"title": "v1", "done": false});
        let after = json!({"title": "v2", "done": false});
        let diff = compute_diff(&before, &after);
        assert_eq!(diff.len(), 1);
        let title_diff = must_some(diff.get("title"), "title diff should be present");
        assert_eq!(title_diff.before, Some(json!("v1")));
        assert_eq!(title_diff.after, Some(json!("v2")));
    }

    #[test]
    fn compute_diff_detects_added_fields() {
        let before = json!({"title": "v1"});
        let after = json!({"title": "v1", "done": true});
        let diff = compute_diff(&before, &after);
        assert_eq!(diff.len(), 1);
        let done_diff = must_some(diff.get("done"), "done diff should be present");
        assert_eq!(done_diff.before, None);
        assert_eq!(done_diff.after, Some(json!(true)));
    }

    #[test]
    fn compute_diff_detects_removed_fields() {
        let before = json!({"title": "v1", "done": false});
        let after = json!({"title": "v1"});
        let diff = compute_diff(&before, &after);
        assert_eq!(diff.len(), 1);
        let done_diff = must_some(diff.get("done"), "done diff should be present");
        assert_eq!(done_diff.before, Some(json!(false)));
        assert_eq!(done_diff.after, None);
    }

    #[test]
    fn compute_diff_empty_when_no_change() {
        let v = json!({"title": "v1", "done": false});
        let diff = compute_diff(&v, &v);
        assert!(diff.is_empty());
    }

    #[test]
    fn compute_diff_non_objects_returns_empty() {
        let diff = compute_diff(&json!("string"), &json!(42));
        assert!(diff.is_empty());
    }

    #[test]
    fn audit_entry_new_auto_computes_diff_for_updates() {
        let entry = AuditEntry::new(
            CollectionId::new("tasks"),
            EntityId::new("t-001"),
            2,
            MutationType::EntityUpdate,
            Some(json!({"title": "old", "done": false})),
            Some(json!({"title": "new", "done": false})),
            None,
        );
        let diff = must_owned_some(entry.diff, "diff populated when before+after present");
        assert_eq!(diff.len(), 1, "only 'title' changed");
        assert_eq!(diff["title"].before, Some(json!("old")));
        assert_eq!(diff["title"].after, Some(json!("new")));
    }

    #[test]
    fn audit_entry_new_no_diff_when_data_identical() {
        let entry = AuditEntry::new(
            CollectionId::new("tasks"),
            EntityId::new("t-001"),
            2,
            MutationType::EntityUpdate,
            Some(json!({"title": "same"})),
            Some(json!({"title": "same"})),
            None,
        );
        assert!(entry.diff.is_none(), "no diff when data unchanged");
    }

    #[test]
    fn audit_entry_create_has_no_diff() {
        let entry = AuditEntry::new(
            CollectionId::new("tasks"),
            EntityId::new("t-001"),
            1,
            MutationType::EntityCreate,
            None,
            Some(json!({"title": "new"})),
            None,
        );
        assert!(entry.diff.is_none(), "create has no diff (no before)");
    }

    #[test]
    fn with_metadata_attaches_metadata() {
        let mut meta = HashMap::new();
        meta.insert("reason".into(), "scheduled-cleanup".into());
        let entry = AuditEntry::new(
            CollectionId::new("tasks"),
            EntityId::new("t-001"),
            1,
            MutationType::EntityCreate,
            None,
            None,
            None,
        )
        .with_metadata(meta);
        assert_eq!(entry.metadata["reason"], "scheduled-cleanup");
    }

    #[test]
    fn intent_lineage_round_trips_through_json() {
        let lineage = sample_intent_lineage();
        let entry = AuditEntry::new(
            CollectionId::new("tasks"),
            EntityId::new("t-001"),
            4,
            MutationType::IntentApprove,
            None,
            None,
            Some("ops-admin".into()),
        )
        .with_intent_lineage(lineage.clone());

        let value = must_ok(
            serde_json::to_value(&entry),
            "intent-lineage entry should serialize",
        );
        assert_eq!(value["mutation"], "intent_approve");
        assert_eq!(value["intent_lineage"]["intent_id"], "intent-123");
        assert_eq!(value["intent_lineage"]["decision"], "needs_approval");
        assert_eq!(value["intent_lineage"]["origin"]["surface"], "mcp");
        assert_eq!(
            value["intent_lineage"]["subject_snapshot"]["tenant_role"],
            "maintainer"
        );

        let decoded: AuditEntry = must_ok(
            serde_json::from_value(value),
            "intent-lineage entry should deserialize",
        );
        assert_eq!(decoded.intent_lineage.as_deref(), Some(&lineage));
        assert_eq!(decoded.mutation, MutationType::IntentApprove);
    }

    #[test]
    fn entity_and_link_entries_omit_intent_lineage_by_default() {
        let entry = AuditEntry::new(
            CollectionId::new("links"),
            EntityId::new("l-001"),
            1,
            MutationType::LinkCreate,
            None,
            Some(json!({"source": "a", "target": "b"})),
            None,
        );

        let value = must_ok(
            serde_json::to_value(&entry),
            "plain link entry should serialize",
        );
        assert!(
            value.get("intent_lineage").is_none(),
            "plain link audit JSON should keep the existing shape"
        );

        let decoded: AuditEntry = must_ok(
            serde_json::from_value(value),
            "plain link entry should deserialize without lineage",
        );
        assert!(decoded.intent_lineage.is_none());
        assert_eq!(decoded.mutation, MutationType::LinkCreate);
    }
}
