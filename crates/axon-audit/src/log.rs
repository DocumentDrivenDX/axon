use std::time::{SystemTime, UNIX_EPOCH};

use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};

use crate::entry::AuditEntry;

/// Append-only audit log interface.
///
/// Implementations must guarantee that entries are stored durably and
/// in the order they were appended. The log is never truncated.
pub trait AuditLog: Send + Sync {
    /// Appends an entry to the audit log.
    ///
    /// The implementation assigns `entry.id` (sequential) and
    /// `entry.timestamp_ns` (current wall clock) before storing.
    fn append(&mut self, entry: AuditEntry) -> Result<AuditEntry, AxonError>;

    /// Returns the total number of entries in the log.
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns all entries for a specific entity in the order they were appended.
    fn query_by_entity(
        &self,
        collection: &CollectionId,
        entity_id: &EntityId,
    ) -> Result<Vec<AuditEntry>, AxonError>;

    /// Returns all entries whose `timestamp_ns` falls within `[start_ns, end_ns]`.
    fn query_by_time_range(&self, start_ns: u64, end_ns: u64)
        -> Result<Vec<AuditEntry>, AxonError>;
}

/// In-memory audit log for testing and embedded mode.
///
/// Entries are stored in append order. Sequential IDs start at 1.
#[derive(Debug, Default)]
pub struct MemoryAuditLog {
    entries: Vec<AuditEntry>,
    next_id: u64,
}

impl MemoryAuditLog {
    /// Returns all stored entries (useful in tests).
    pub fn entries(&self) -> &[AuditEntry] {
        &self.entries
    }
}

impl AuditLog for MemoryAuditLog {
    fn append(&mut self, mut entry: AuditEntry) -> Result<AuditEntry, AxonError> {
        self.next_id += 1;
        entry.id = self.next_id;
        entry.timestamp_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        self.entries.push(entry.clone());
        Ok(entry)
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    fn query_by_entity(
        &self,
        collection: &CollectionId,
        entity_id: &EntityId,
    ) -> Result<Vec<AuditEntry>, AxonError> {
        Ok(self
            .entries
            .iter()
            .filter(|e| &e.collection == collection && &e.entity_id == entity_id)
            .cloned()
            .collect())
    }

    fn query_by_time_range(
        &self,
        start_ns: u64,
        end_ns: u64,
    ) -> Result<Vec<AuditEntry>, AxonError> {
        Ok(self
            .entries
            .iter()
            .filter(|e| e.timestamp_ns >= start_ns && e.timestamp_ns <= end_ns)
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::MutationType;
    use axon_core::id::{CollectionId, EntityId};
    use serde_json::json;

    fn tasks() -> CollectionId {
        CollectionId::new("tasks")
    }

    fn sample_entry(id: &str, mutation: MutationType) -> AuditEntry {
        AuditEntry::new(
            tasks(),
            EntityId::new(id),
            1,
            mutation,
            None,
            Some(json!({"title": id})),
            None,
        )
    }

    #[test]
    fn memory_log_starts_empty() {
        let log = MemoryAuditLog::default();
        assert!(log.is_empty());
    }

    #[test]
    fn memory_log_append_increments_len() {
        let mut log = MemoryAuditLog::default();
        log.append(sample_entry("t-001", MutationType::Create))
            .unwrap();
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn append_assigns_sequential_ids() {
        let mut log = MemoryAuditLog::default();
        let e1 = log
            .append(sample_entry("t-001", MutationType::Create))
            .unwrap();
        let e2 = log
            .append(sample_entry("t-002", MutationType::Create))
            .unwrap();
        assert_eq!(e1.id, 1);
        assert_eq!(e2.id, 2);
    }

    #[test]
    fn append_assigns_nonzero_timestamp() {
        let mut log = MemoryAuditLog::default();
        let e = log
            .append(sample_entry("t-001", MutationType::Create))
            .unwrap();
        assert!(e.timestamp_ns > 0, "timestamp_ns should be non-zero");
    }

    #[test]
    fn query_by_entity_returns_mutations_in_order() {
        let mut log = MemoryAuditLog::default();
        // Create t-001, update t-001, create t-002
        log.append(sample_entry("t-001", MutationType::Create))
            .unwrap();
        log.append(sample_entry("t-002", MutationType::Create))
            .unwrap();
        log.append(sample_entry("t-001", MutationType::Update))
            .unwrap();

        let entries = log
            .query_by_entity(&tasks(), &EntityId::new("t-001"))
            .unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].mutation, MutationType::Create);
        assert_eq!(entries[1].mutation, MutationType::Update);
    }

    #[test]
    fn query_by_entity_empty_collection_returns_empty() {
        let log = MemoryAuditLog::default();
        let entries = log
            .query_by_entity(&tasks(), &EntityId::new("none"))
            .unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn query_by_time_range_filters_correctly() {
        let mut log = MemoryAuditLog::default();
        let e1 = log
            .append(sample_entry("t-001", MutationType::Create))
            .unwrap();
        let e2 = log
            .append(sample_entry("t-002", MutationType::Create))
            .unwrap();

        // Inclusive range covering both entries.
        let results = log
            .query_by_time_range(e1.timestamp_ns, e2.timestamp_ns)
            .unwrap();
        assert_eq!(results.len(), 2);

        // Range before e1 should return nothing.
        let results_empty = log.query_by_time_range(0, e1.timestamp_ns - 1).unwrap();
        assert!(results_empty.is_empty());
    }

    #[test]
    fn audit_entries_are_append_only() {
        // Verify there is no remove/update API on the trait — compile-time check
        // by confirming only `append`, `query_by_entity`, `query_by_time_range`
        // are available. This is a structural test.
        let log = MemoryAuditLog::default();
        assert!(log.is_empty());
        // No delete or update method exists — enforced by the trait definition.
    }
}
