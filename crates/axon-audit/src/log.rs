use axon_core::error::AxonError;

use crate::entry::AuditEntry;

/// Append-only audit log interface.
///
/// Implementations must guarantee that entries are stored durably and
/// in the order they were appended. The log is never truncated.
pub trait AuditLog: Send + Sync {
    /// Appends an entry to the audit log.
    fn append(&mut self, entry: AuditEntry) -> Result<(), AxonError>;

    /// Returns the total number of entries in the log.
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// In-memory audit log for testing.
#[derive(Debug, Default)]
pub struct MemoryAuditLog {
    entries: Vec<AuditEntry>,
}

impl AuditLog for MemoryAuditLog {
    fn append(&mut self, entry: AuditEntry) -> Result<(), AxonError> {
        self.entries.push(entry);
        Ok(())
    }

    fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::MutationType;
    use axon_core::id::{CollectionId, EntityId};
    use serde_json::json;

    fn sample_entry() -> AuditEntry {
        AuditEntry {
            collection: CollectionId::new("tasks"),
            entity_id: EntityId::new("t-001"),
            version: 1,
            mutation: MutationType::Create,
            data_after: Some(json!({"title": "hello"})),
            actor: None,
        }
    }

    #[test]
    fn memory_log_starts_empty() {
        let log = MemoryAuditLog::default();
        assert!(log.is_empty());
    }

    #[test]
    fn memory_log_append_increments_len() {
        let mut log = MemoryAuditLog::default();
        log.append(sample_entry()).unwrap();
        assert_eq!(log.len(), 1);
    }
}
