use std::time::{SystemTime, UNIX_EPOCH};

use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};

use crate::entry::{AuditEntry, MutationType};

// ── Query types ──────────────────────────────────────────────────────────────

/// Filter parameters for querying the audit log.
///
/// All fields are optional. Specified fields are ANDed together.
#[derive(Debug, Clone, Default)]
pub struct AuditQuery {
    /// Restrict to entries for this collection.
    pub collection: Option<CollectionId>,
    /// Restrict to entries for this entity.
    pub entity_id: Option<EntityId>,
    /// Restrict to entries produced by this actor.
    pub actor: Option<String>,
    /// Restrict to entries of this mutation type.
    pub operation: Option<MutationType>,
    /// Inclusive start of the timestamp range (nanoseconds since Unix epoch).
    pub since_ns: Option<u64>,
    /// Inclusive end of the timestamp range (nanoseconds since Unix epoch).
    pub until_ns: Option<u64>,
    /// Cursor for pagination: return only entries with `id > after_id`.
    /// `None` means start from the first entry.
    pub after_id: Option<u64>,
    /// Maximum number of entries to return. Defaults to [`DEFAULT_PAGE_SIZE`].
    pub limit: Option<usize>,
}

const DEFAULT_PAGE_SIZE: usize = 100;

/// A page of audit entries returned by [`AuditLog::query_paginated`].
#[derive(Debug, Clone)]
pub struct AuditPage {
    pub entries: Vec<AuditEntry>,
    /// Cursor to fetch the next page. `None` when there are no more results.
    pub next_cursor: Option<u64>,
}

// ── Trait ────────────────────────────────────────────────────────────────────

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

    /// Returns all entries produced by the given actor, in append order.
    fn query_by_actor(&self, actor: &str) -> Result<Vec<AuditEntry>, AxonError>;

    /// Returns all entries with the given mutation type, in append order.
    fn query_by_operation(&self, operation: &MutationType) -> Result<Vec<AuditEntry>, AxonError>;

    /// Looks up a single entry by its sequential ID.
    ///
    /// Returns `Ok(None)` when no entry with that ID exists.
    fn find_by_id(&self, id: u64) -> Result<Option<AuditEntry>, AxonError>;

    /// Executes a multi-field filtered, paginated query.
    ///
    /// The cursor (`after_id`) is the last entry ID seen; the next page begins
    /// at `after_id + 1`. When the returned [`AuditPage::next_cursor`] is `None`
    /// there are no further results.
    fn query_paginated(&self, query: AuditQuery) -> Result<AuditPage, AxonError>;

    /// Replay audit entries from a given cursor, returning CDC envelopes.
    ///
    /// - `after_id = None`: initial snapshot — replays all entries.
    /// - `after_id = Some(id)`: resumable replay from that cursor.
    /// - `limit`: maximum number of events to return per call.
    ///
    /// Returns `(envelopes, next_cursor)` where `next_cursor` is `None`
    /// when all events have been replayed.
    ///
    /// Consumer deduplication: each envelope carries `source.audit_id`
    /// which is globally unique and monotonically increasing.
    fn replay(
        &self,
        after_id: Option<u64>,
        collection: Option<&CollectionId>,
        limit: usize,
    ) -> Result<(Vec<crate::cdc::CdcEnvelope>, Option<u64>), AxonError> {
        let page = self.query_paginated(AuditQuery {
            collection: collection.cloned(),
            after_id,
            limit: Some(limit),
            ..AuditQuery::default()
        })?;

        let envelopes: Vec<crate::cdc::CdcEnvelope> = page
            .entries
            .iter()
            .filter_map(crate::cdc::CdcEnvelope::from_audit_entry)
            .collect();

        Ok((envelopes, page.next_cursor))
    }
}

// ── MemoryAuditLog ───────────────────────────────────────────────────────────

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

    fn query_by_actor(&self, actor: &str) -> Result<Vec<AuditEntry>, AxonError> {
        Ok(self
            .entries
            .iter()
            .filter(|e| e.actor == actor)
            .cloned()
            .collect())
    }

    fn query_by_operation(&self, operation: &MutationType) -> Result<Vec<AuditEntry>, AxonError> {
        Ok(self
            .entries
            .iter()
            .filter(|e| &e.mutation == operation)
            .cloned()
            .collect())
    }

    fn find_by_id(&self, id: u64) -> Result<Option<AuditEntry>, AxonError> {
        Ok(self.entries.iter().find(|e| e.id == id).cloned())
    }

    fn query_paginated(&self, query: AuditQuery) -> Result<AuditPage, AxonError> {
        let limit = query.limit.unwrap_or(DEFAULT_PAGE_SIZE);
        let after_id = query.after_id.unwrap_or(0);

        let mut filtered: Vec<&AuditEntry> = self
            .entries
            .iter()
            .filter(|e| {
                if e.id <= after_id {
                    return false;
                }
                if let Some(col) = &query.collection {
                    if &e.collection != col {
                        return false;
                    }
                }
                if let Some(eid) = &query.entity_id {
                    if &e.entity_id != eid {
                        return false;
                    }
                }
                if let Some(actor) = &query.actor {
                    if &e.actor != actor {
                        return false;
                    }
                }
                if let Some(op) = &query.operation {
                    if &e.mutation != op {
                        return false;
                    }
                }
                if let Some(since) = query.since_ns {
                    if e.timestamp_ns < since {
                        return false;
                    }
                }
                if let Some(until) = query.until_ns {
                    if e.timestamp_ns > until {
                        return false;
                    }
                }
                true
            })
            .collect();

        // Fetch one extra to detect whether a next page exists.
        let has_more = filtered.len() > limit;
        filtered.truncate(limit);

        let next_cursor = if has_more {
            filtered.last().map(|e| e.id)
        } else {
            None
        };

        Ok(AuditPage {
            entries: filtered.into_iter().cloned().collect(),
            next_cursor,
        })
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
        log.append(sample_entry("t-001", MutationType::EntityCreate))
            .unwrap();
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn append_assigns_sequential_ids() {
        let mut log = MemoryAuditLog::default();
        let e1 = log
            .append(sample_entry("t-001", MutationType::EntityCreate))
            .unwrap();
        let e2 = log
            .append(sample_entry("t-002", MutationType::EntityCreate))
            .unwrap();
        assert_eq!(e1.id, 1);
        assert_eq!(e2.id, 2);
    }

    #[test]
    fn append_assigns_nonzero_timestamp() {
        let mut log = MemoryAuditLog::default();
        let e = log
            .append(sample_entry("t-001", MutationType::EntityCreate))
            .unwrap();
        assert!(e.timestamp_ns > 0, "timestamp_ns should be non-zero");
    }

    #[test]
    fn query_by_entity_returns_mutations_in_order() {
        let mut log = MemoryAuditLog::default();
        log.append(sample_entry("t-001", MutationType::EntityCreate))
            .unwrap();
        log.append(sample_entry("t-002", MutationType::EntityCreate))
            .unwrap();
        log.append(sample_entry("t-001", MutationType::EntityUpdate))
            .unwrap();

        let entries = log
            .query_by_entity(&tasks(), &EntityId::new("t-001"))
            .unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].mutation, MutationType::EntityCreate);
        assert_eq!(entries[1].mutation, MutationType::EntityUpdate);
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
            .append(sample_entry("t-001", MutationType::EntityCreate))
            .unwrap();
        let e2 = log
            .append(sample_entry("t-002", MutationType::EntityCreate))
            .unwrap();

        let results = log
            .query_by_time_range(e1.timestamp_ns, e2.timestamp_ns)
            .unwrap();
        assert_eq!(results.len(), 2);

        let results_empty = log.query_by_time_range(0, e1.timestamp_ns - 1).unwrap();
        assert!(results_empty.is_empty());
    }

    #[test]
    fn query_by_actor_filters_correctly() {
        let mut log = MemoryAuditLog::default();
        let mut e = sample_entry("t-001", MutationType::EntityCreate);
        e.actor = "agent-x".into();
        log.append(e).unwrap();
        log.append(sample_entry("t-002", MutationType::EntityCreate))
            .unwrap();

        let results = log.query_by_actor("agent-x").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entity_id.as_str(), "t-001");

        let anonymous = log.query_by_actor("anonymous").unwrap();
        assert_eq!(anonymous.len(), 1);
    }

    #[test]
    fn query_by_operation_filters_correctly() {
        let mut log = MemoryAuditLog::default();
        log.append(sample_entry("t-001", MutationType::EntityCreate))
            .unwrap();
        log.append(sample_entry("t-001", MutationType::EntityUpdate))
            .unwrap();
        log.append(sample_entry("t-001", MutationType::EntityDelete))
            .unwrap();

        let creates = log.query_by_operation(&MutationType::EntityCreate).unwrap();
        assert_eq!(creates.len(), 1);

        let updates = log.query_by_operation(&MutationType::EntityUpdate).unwrap();
        assert_eq!(updates.len(), 1);
    }

    #[test]
    fn find_by_id_returns_correct_entry() {
        let mut log = MemoryAuditLog::default();
        let e1 = log
            .append(sample_entry("t-001", MutationType::EntityCreate))
            .unwrap();
        log.append(sample_entry("t-002", MutationType::EntityCreate))
            .unwrap();

        let found = log.find_by_id(e1.id).unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().entity_id.as_str(), "t-001");
    }

    #[test]
    fn find_by_id_missing_returns_none() {
        let log = MemoryAuditLog::default();
        assert!(log.find_by_id(999).unwrap().is_none());
    }

    #[test]
    fn query_paginated_basic_cursor() {
        let mut log = MemoryAuditLog::default();
        for i in 0..5u32 {
            log.append(sample_entry(
                &format!("t-{i:03}"),
                MutationType::EntityCreate,
            ))
            .unwrap();
        }

        let page1 = log
            .query_paginated(AuditQuery {
                limit: Some(2),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page1.entries.len(), 2);
        assert!(page1.next_cursor.is_some());

        let page2 = log
            .query_paginated(AuditQuery {
                limit: Some(2),
                after_id: page1.next_cursor,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page2.entries.len(), 2);
        assert!(page2.next_cursor.is_some());

        let page3 = log
            .query_paginated(AuditQuery {
                limit: Some(2),
                after_id: page2.next_cursor,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page3.entries.len(), 1);
        assert!(page3.next_cursor.is_none());
    }

    #[test]
    fn query_paginated_filters_by_actor() {
        let mut log = MemoryAuditLog::default();
        for i in 0..4u32 {
            let mut e = sample_entry(&format!("t-{i:03}"), MutationType::EntityCreate);
            e.actor = if i % 2 == 0 {
                "alice".into()
            } else {
                "bob".into()
            };
            log.append(e).unwrap();
        }

        let alice_page = log
            .query_paginated(AuditQuery {
                actor: Some("alice".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(alice_page.entries.len(), 2);
        assert!(alice_page.entries.iter().all(|e| e.actor == "alice"));
    }

    #[test]
    fn audit_entries_are_append_only() {
        let log = MemoryAuditLog::default();
        assert!(log.is_empty());
        // No delete or update method exists — enforced by the trait definition.
    }

    // ── Replay tests (US-075) ──────────────────────────────────────────

    #[test]
    fn replay_initial_snapshot_returns_all_events() {
        let mut log = MemoryAuditLog::default();
        for i in 1..=5 {
            log.append(AuditEntry::new(
                CollectionId::new("tasks"),
                EntityId::new(format!("t-{i:03}")),
                1,
                MutationType::EntityCreate,
                None,
                Some(serde_json::json!({"n": i})),
                Some("agent".into()),
            ))
            .unwrap();
        }

        let (envelopes, cursor) = log.replay(None, None, 100).unwrap();
        assert_eq!(envelopes.len(), 5);
        // cursor should be None since we got all events.
        assert!(cursor.is_none());
    }

    #[test]
    fn replay_resumable_from_cursor() {
        let mut log = MemoryAuditLog::default();
        for i in 1..=10 {
            log.append(AuditEntry::new(
                CollectionId::new("tasks"),
                EntityId::new(format!("t-{i:03}")),
                1,
                MutationType::EntityCreate,
                None,
                Some(serde_json::json!({"n": i})),
                Some("agent".into()),
            ))
            .unwrap();
        }

        // First page: 5 events.
        let (page1, cursor1) = log.replay(None, None, 5).unwrap();
        assert_eq!(page1.len(), 5);
        assert!(cursor1.is_some());

        // Second page from cursor.
        let (page2, cursor2) = log.replay(cursor1, None, 5).unwrap();
        assert_eq!(page2.len(), 5);
        assert!(cursor2.is_none());

        // audit_ids should not overlap.
        let ids1: Vec<u64> = page1.iter().map(|e| e.source.audit_id).collect();
        let ids2: Vec<u64> = page2.iter().map(|e| e.source.audit_id).collect();
        assert!(ids1.iter().max().unwrap() < ids2.iter().min().unwrap());
    }

    #[test]
    fn replay_filters_by_collection() {
        let mut log = MemoryAuditLog::default();
        log.append(AuditEntry::new(
            CollectionId::new("tasks"),
            EntityId::new("t-001"),
            1,
            MutationType::EntityCreate,
            None,
            Some(serde_json::json!({"task": true})),
            Some("a".into()),
        ))
        .unwrap();
        log.append(AuditEntry::new(
            CollectionId::new("users"),
            EntityId::new("u-001"),
            1,
            MutationType::EntityCreate,
            None,
            Some(serde_json::json!({"user": true})),
            Some("a".into()),
        ))
        .unwrap();

        let (envelopes, _) = log
            .replay(None, Some(&CollectionId::new("tasks")), 100)
            .unwrap();
        assert_eq!(envelopes.len(), 1);
        assert_eq!(envelopes[0].source.collection, "tasks");
    }

    #[test]
    fn replay_collection_events_skipped_in_envelopes() {
        let mut log = MemoryAuditLog::default();
        // Collection create events don't produce CDC envelopes.
        log.append(AuditEntry::new(
            CollectionId::new("tasks"),
            EntityId::new(""),
            0,
            MutationType::CollectionCreate,
            None,
            None,
            None,
        ))
        .unwrap();
        log.append(AuditEntry::new(
            CollectionId::new("tasks"),
            EntityId::new("t-001"),
            1,
            MutationType::EntityCreate,
            None,
            Some(serde_json::json!({"x": 1})),
            Some("a".into()),
        ))
        .unwrap();

        let (envelopes, _) = log.replay(None, None, 100).unwrap();
        // Only entity events produce envelopes.
        assert_eq!(envelopes.len(), 1);
    }

    #[test]
    fn replay_dedup_by_audit_id() {
        let mut log = MemoryAuditLog::default();
        for _ in 0..3 {
            log.append(AuditEntry::new(
                CollectionId::new("tasks"),
                EntityId::new("t-001"),
                1,
                MutationType::EntityCreate,
                None,
                Some(serde_json::json!({})),
                None,
            ))
            .unwrap();
        }

        let (envelopes, _) = log.replay(None, None, 100).unwrap();
        // Each envelope has a unique audit_id.
        let ids: Vec<u64> = envelopes.iter().map(|e| e.source.audit_id).collect();
        let unique: std::collections::HashSet<u64> = ids.iter().copied().collect();
        assert_eq!(ids.len(), unique.len());
    }
}
