use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use axon_core::id::{CollectionId, EntityId};

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
    // ── Collection lifecycle ─────────────────────────────────────────────────
    CollectionCreate,
    CollectionDrop,
    // ── Schema operations ────────────────────────────────────────────────────
    SchemaUpdate,
}

impl std::fmt::Display for MutationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            MutationType::EntityCreate => "entity.create",
            MutationType::EntityUpdate => "entity.update",
            MutationType::EntityDelete => "entity.delete",
            MutationType::EntityRevert => "entity.revert",
            MutationType::CollectionCreate => "collection.create",
            MutationType::CollectionDrop => "collection.drop",
            MutationType::SchemaUpdate => "schema.update",
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
                if d.is_empty() { None } else { Some(d) }
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
        }
    }

    /// Attach caller-supplied key-value metadata to this entry (builder style).
    pub fn with_metadata(mut self, metadata: HashMap<String, String>) -> Self {
        self.metadata = metadata;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn mutation_type_display_dot_notation() {
        assert_eq!(MutationType::EntityCreate.to_string(), "entity.create");
        assert_eq!(MutationType::EntityUpdate.to_string(), "entity.update");
        assert_eq!(MutationType::EntityDelete.to_string(), "entity.delete");
        assert_eq!(MutationType::EntityRevert.to_string(), "entity.revert");
        assert_eq!(MutationType::CollectionCreate.to_string(), "collection.create");
        assert_eq!(MutationType::CollectionDrop.to_string(), "collection.drop");
        assert_eq!(MutationType::SchemaUpdate.to_string(), "schema.update");
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
        let title_diff = diff.get("title").unwrap();
        assert_eq!(title_diff.before, Some(json!("v1")));
        assert_eq!(title_diff.after, Some(json!("v2")));
    }

    #[test]
    fn compute_diff_detects_added_fields() {
        let before = json!({"title": "v1"});
        let after = json!({"title": "v1", "done": true});
        let diff = compute_diff(&before, &after);
        assert_eq!(diff.len(), 1);
        let done_diff = diff.get("done").unwrap();
        assert_eq!(done_diff.before, None);
        assert_eq!(done_diff.after, Some(json!(true)));
    }

    #[test]
    fn compute_diff_detects_removed_fields() {
        let before = json!({"title": "v1", "done": false});
        let after = json!({"title": "v1"});
        let diff = compute_diff(&before, &after);
        assert_eq!(diff.len(), 1);
        let done_diff = diff.get("done").unwrap();
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
        let diff = entry.diff.expect("diff populated when before+after present");
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
}
