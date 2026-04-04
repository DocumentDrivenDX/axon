use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use axon_core::id::{CollectionId, EntityId};

/// The type of mutation recorded in an audit entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationType {
    Create,
    Update,
    Delete,
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
    /// The entity affected.
    pub entity_id: EntityId,
    /// The entity version after this mutation.
    pub version: u64,
    /// The kind of mutation.
    pub mutation: MutationType,
    /// Snapshot of the entity data before the mutation (None for Create).
    pub data_before: Option<Value>,
    /// Snapshot of the entity data after the mutation (None for Delete).
    pub data_after: Option<Value>,
    /// Identity of the actor who performed the mutation.
    /// Defaults to "anonymous" when no actor is provided by the caller.
    pub actor: String,
    /// Optional caller-supplied key-value metadata (reason, correlation ID, etc.).
    pub metadata: HashMap<String, String>,
}

impl AuditEntry {
    /// Convenience constructor. Sets `id` to 0 and `timestamp_ns` to 0;
    /// the [`crate::log::AuditLog`] implementation assigns real values on append.
    pub fn new(
        collection: CollectionId,
        entity_id: EntityId,
        version: u64,
        mutation: MutationType,
        data_before: Option<Value>,
        data_after: Option<Value>,
        actor: Option<String>,
    ) -> Self {
        Self {
            id: 0,
            timestamp_ns: 0,
            collection,
            entity_id,
            version,
            mutation,
            data_before,
            data_after,
            actor: actor.unwrap_or_else(|| "anonymous".into()),
            metadata: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn audit_entry_create() {
        let entry = AuditEntry::new(
            CollectionId::new("tasks"),
            EntityId::new("t-001"),
            1,
            MutationType::Create,
            None,
            Some(json!({"title": "hello"})),
            Some("agent-1".into()),
        );
        assert_eq!(entry.mutation, MutationType::Create);
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
            MutationType::Create,
            None,
            None,
            None,
        );
        assert_eq!(entry.actor, "anonymous");
    }
}
