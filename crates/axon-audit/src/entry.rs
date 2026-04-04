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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEntry {
    /// The collection affected.
    pub collection: CollectionId,
    /// The entity affected.
    pub entity_id: EntityId,
    /// The entity version after this mutation.
    pub version: u64,
    /// The kind of mutation.
    pub mutation: MutationType,
    /// Snapshot of the entity data after the mutation (None for Delete).
    pub data_after: Option<Value>,
    /// Identity of the actor who performed the mutation.
    pub actor: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn audit_entry_create() {
        let entry = AuditEntry {
            collection: CollectionId::new("tasks"),
            entity_id: EntityId::new("t-001"),
            version: 1,
            mutation: MutationType::Create,
            data_after: Some(json!({"title": "hello"})),
            actor: Some("agent-1".into()),
        };
        assert_eq!(entry.mutation, MutationType::Create);
        assert_eq!(entry.version, 1);
    }
}
