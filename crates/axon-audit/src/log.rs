use std::time::{SystemTime, UNIX_EPOCH};

use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId, Namespace, DEFAULT_DATABASE};

use crate::entry::{AuditEntry, MutationType};

// ── Query types ──────────────────────────────────────────────────────────────

/// Filter parameters for querying the audit log.
///
/// All fields are optional. Specified fields are ANDed together.
#[derive(Debug, Clone, Default)]
pub struct AuditQuery {
    /// Restrict to entries within this database scope.
    pub database: Option<String>,
    /// Restrict to entries for this collection (single-collection path; kept for backward
    /// compatibility). When `collection_ids` is also non-empty, the two are unioned before
    /// filtering.
    pub collection: Option<CollectionId>,
    /// Restrict to entries for any of these collections (multi-collection tail, US-079).
    ///
    /// When non-empty, the effective collection filter is the union of this set and the
    /// optional single `collection` field. When both are empty / `None`, no collection filter
    /// is applied (entries from all collections are returned). Results are always returned
    /// globally ordered by `audit_id` ascending — never grouped by collection — so that a
    /// single monotonic cursor advances across all requested collections at once.
    pub collection_ids: Vec<CollectionId>,
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

fn collection_is_in_database(collection: &CollectionId, database: &str) -> bool {
    let (namespace, _) = Namespace::parse_with_database(collection.as_str(), DEFAULT_DATABASE);
    namespace.database == database
}

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
///
/// # Ordering Invariant
///
/// All query methods that return multiple entries MUST return them in ascending `audit_id` order.
/// Implementations MUST ensure this invariant holds regardless of how entries were inserted.
/// Persistent backends must emit `ORDER BY id ASC` or equivalent SQL ordering clause.
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

    /// Returns all entries that share the given `transaction_id`, in append order.
    fn query_by_transaction_id(
        &self,
        transaction_id: &str,
    ) -> Result<Vec<AuditEntry>, AxonError>;

    /// Executes a multi-field filtered, paginated query.
    ///
    /// The cursor (`after_id`) is the last entry ID seen; the next page begins
    /// at `after_id + 1`. When the returned [`AuditPage::next_cursor`] is `None`
    /// there are no further results.
    ///
    /// # Ordering
    ///
    /// Returns entries ordered by `audit_id` (the entry's sequential `id`) in ascending order.
    /// Implementations MUST return entries ordered by audit_id ascending. Persistent backends
    /// must emit `ORDER BY id ASC` or equivalent SQL ordering clause.
    fn query_paginated(&self, query: AuditQuery) -> Result<AuditPage, AxonError>;

    /// Returns the set of distinct collection IDs that appear in the audit log.
    ///
    /// Used to validate collection names supplied by callers before querying.
    /// Returns an empty set when the log is empty.
    ///
    /// # Complexity
    ///
    /// The default implementation is O(N) over the full log and must be cached or indexed
    /// in production backends for scalability.
    fn known_collections(&self) -> std::collections::HashSet<CollectionId>;

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

    fn known_collections(&self) -> std::collections::HashSet<CollectionId> {
        self.entries.iter().map(|e| e.collection.clone()).collect()
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

    fn query_by_transaction_id(
        &self,
        transaction_id: &str,
    ) -> Result<Vec<AuditEntry>, AxonError> {
        Ok(self
            .entries
            .iter()
            .filter(|e| e.transaction_id.as_deref() == Some(transaction_id))
            .cloned()
            .collect())
    }

    fn query_paginated(&self, query: AuditQuery) -> Result<AuditPage, AxonError> {
        let limit = query.limit.unwrap_or(DEFAULT_PAGE_SIZE);
        let after_id = query.after_id.unwrap_or(0);

        // Build the effective collection filter as the union of `collection_ids` and the
        // single-collection `collection` field. Empty set means "all collections".
        // Results remain globally ordered by audit_id ascending so that a single monotonic
        // cursor walks all requested collections (FEAT-003 US-079).
        let effective_collections: Vec<&CollectionId> = {
            let mut v: Vec<&CollectionId> = query.collection_ids.iter().collect();
            if let Some(col) = &query.collection {
                if !v.contains(&col) {
                    v.push(col);
                }
            }
            v
        };

        let mut filtered: Vec<&AuditEntry> = self
            .entries
            .iter()
            .filter(|e| {
                if e.id <= after_id {
                    return false;
                }
                if let Some(database) = &query.database {
                    if !collection_is_in_database(&e.collection, database) {
                        return false;
                    }
                }
                if !effective_collections.is_empty()
                    && !effective_collections.contains(&&e.collection)
                {
                    return false;
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

        // Sort entries by audit_id (entry.id) in ascending order before returning.
        // This enforces the ordering invariant regardless of insertion order.
        filtered.sort_by_key(|e| e.id);

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

    fn must_ok<T, E: std::fmt::Debug>(result: Result<T, E>, context: &str) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("{context}: {error:?}"),
        }
    }

    fn must_some<T>(value: Option<T>, context: &str) -> T {
        match value {
            Some(value) => value,
            None => panic!("{context}"),
        }
    }

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
        must_ok(
            log.append(sample_entry("t-001", MutationType::EntityCreate)),
            "append should succeed",
        );
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn append_assigns_sequential_ids() {
        let mut log = MemoryAuditLog::default();
        let e1 = must_ok(
            log.append(sample_entry("t-001", MutationType::EntityCreate)),
            "first append should succeed",
        );
        let e2 = must_ok(
            log.append(sample_entry("t-002", MutationType::EntityCreate)),
            "second append should succeed",
        );
        assert_eq!(e1.id, 1);
        assert_eq!(e2.id, 2);
    }

    #[test]
    fn append_assigns_nonzero_timestamp() {
        let mut log = MemoryAuditLog::default();
        let e = must_ok(
            log.append(sample_entry("t-001", MutationType::EntityCreate)),
            "append should succeed",
        );
        assert!(e.timestamp_ns > 0, "timestamp_ns should be non-zero");
    }

    #[test]
    fn query_by_entity_returns_mutations_in_order() {
        let mut log = MemoryAuditLog::default();
        must_ok(
            log.append(sample_entry("t-001", MutationType::EntityCreate)),
            "first append should succeed",
        );
        must_ok(
            log.append(sample_entry("t-002", MutationType::EntityCreate)),
            "second append should succeed",
        );
        must_ok(
            log.append(sample_entry("t-001", MutationType::EntityUpdate)),
            "third append should succeed",
        );

        let entries = must_ok(
            log.query_by_entity(&tasks(), &EntityId::new("t-001")),
            "entity query should succeed",
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].mutation, MutationType::EntityCreate);
        assert_eq!(entries[1].mutation, MutationType::EntityUpdate);
    }

    #[test]
    fn query_by_entity_empty_collection_returns_empty() {
        let log = MemoryAuditLog::default();
        let entries = must_ok(
            log.query_by_entity(&tasks(), &EntityId::new("none")),
            "empty entity query should succeed",
        );
        assert!(entries.is_empty());
    }

    #[test]
    fn query_by_time_range_filters_correctly() {
        let mut log = MemoryAuditLog::default();
        let e1 = must_ok(
            log.append(sample_entry("t-001", MutationType::EntityCreate)),
            "first append should succeed",
        );
        let e2 = must_ok(
            log.append(sample_entry("t-002", MutationType::EntityCreate)),
            "second append should succeed",
        );

        let results = must_ok(
            log.query_by_time_range(e1.timestamp_ns, e2.timestamp_ns),
            "time range query should succeed",
        );
        assert_eq!(results.len(), 2);

        let results_empty = must_ok(
            log.query_by_time_range(0, e1.timestamp_ns - 1),
            "empty time range query should succeed",
        );
        assert!(results_empty.is_empty());
    }

    #[test]
    fn query_by_actor_filters_correctly() {
        let mut log = MemoryAuditLog::default();
        let mut e = sample_entry("t-001", MutationType::EntityCreate);
        e.actor = "agent-x".into();
        must_ok(
            log.append(e),
            "append for actor-specific entry should succeed",
        );
        must_ok(
            log.append(sample_entry("t-002", MutationType::EntityCreate)),
            "append for anonymous entry should succeed",
        );

        let results = must_ok(log.query_by_actor("agent-x"), "actor query should succeed");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entity_id.as_str(), "t-001");

        let anonymous = must_ok(
            log.query_by_actor("anonymous"),
            "anonymous query should succeed",
        );
        assert_eq!(anonymous.len(), 1);
    }

    #[test]
    fn query_by_operation_filters_correctly() {
        let mut log = MemoryAuditLog::default();
        must_ok(
            log.append(sample_entry("t-001", MutationType::EntityCreate)),
            "create append should succeed",
        );
        must_ok(
            log.append(sample_entry("t-001", MutationType::EntityUpdate)),
            "update append should succeed",
        );
        must_ok(
            log.append(sample_entry("t-001", MutationType::EntityDelete)),
            "delete append should succeed",
        );

        let creates = must_ok(
            log.query_by_operation(&MutationType::EntityCreate),
            "create-operation query should succeed",
        );
        assert_eq!(creates.len(), 1);

        let updates = must_ok(
            log.query_by_operation(&MutationType::EntityUpdate),
            "update-operation query should succeed",
        );
        assert_eq!(updates.len(), 1);
    }

    #[test]
    fn find_by_id_returns_correct_entry() {
        let mut log = MemoryAuditLog::default();
        let e1 = must_ok(
            log.append(sample_entry("t-001", MutationType::EntityCreate)),
            "first append should succeed",
        );
        must_ok(
            log.append(sample_entry("t-002", MutationType::EntityCreate)),
            "second append should succeed",
        );

        let found = must_ok(log.find_by_id(e1.id), "find_by_id should succeed");
        assert!(found.is_some());
        assert_eq!(
            must_some(found, "entry should be present")
                .entity_id
                .as_str(),
            "t-001"
        );
    }

    #[test]
    fn find_by_id_missing_returns_none() {
        let log = MemoryAuditLog::default();
        assert!(must_ok(log.find_by_id(999), "missing find_by_id should succeed").is_none());
    }

    #[test]
    fn query_paginated_basic_cursor() {
        let mut log = MemoryAuditLog::default();
        for i in 0..5u32 {
            must_ok(
                log.append(sample_entry(
                    &format!("t-{i:03}"),
                    MutationType::EntityCreate,
                )),
                "paginated append should succeed",
            );
        }

        let page1 = must_ok(
            log.query_paginated(AuditQuery {
                limit: Some(2),
                ..Default::default()
            }),
            "first page query should succeed",
        );
        assert_eq!(page1.entries.len(), 2);
        assert!(page1.next_cursor.is_some());

        let page2 = must_ok(
            log.query_paginated(AuditQuery {
                limit: Some(2),
                after_id: page1.next_cursor,
                ..Default::default()
            }),
            "second page query should succeed",
        );
        assert_eq!(page2.entries.len(), 2);
        assert!(page2.next_cursor.is_some());

        let page3 = must_ok(
            log.query_paginated(AuditQuery {
                limit: Some(2),
                after_id: page2.next_cursor,
                ..Default::default()
            }),
            "third page query should succeed",
        );
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
            must_ok(log.append(e), "filtered append should succeed");
        }

        let alice_page = must_ok(
            log.query_paginated(AuditQuery {
                actor: Some("alice".into()),
                ..Default::default()
            }),
            "actor-filtered page query should succeed",
        );
        assert_eq!(alice_page.entries.len(), 2);
        assert!(alice_page.entries.iter().all(|e| e.actor == "alice"));
    }

    #[test]
    fn query_paginated_filters_by_database_scope() {
        let mut log = MemoryAuditLog::default();
        must_ok(
            log.append(AuditEntry::new(
                CollectionId::new("tasks"),
                EntityId::new("t-001"),
                1,
                MutationType::EntityCreate,
                None,
                Some(serde_json::json!({"scope": "default"})),
                None,
            )),
            "default-database append should succeed",
        );
        must_ok(
            log.append(AuditEntry::new(
                CollectionId::new("prod.default.tasks"),
                EntityId::new("t-001"),
                1,
                MutationType::EntityCreate,
                None,
                Some(serde_json::json!({"scope": "prod"})),
                None,
            )),
            "prod-database append should succeed",
        );

        let prod_page = must_ok(
            log.query_paginated(AuditQuery {
                database: Some("prod".into()),
                ..Default::default()
            }),
            "database-filtered page query should succeed",
        );
        assert_eq!(prod_page.entries.len(), 1);
        assert_eq!(
            prod_page.entries[0].collection,
            CollectionId::new("prod.default.tasks")
        );

        let default_page = must_ok(
            log.query_paginated(AuditQuery {
                database: Some("default".into()),
                ..Default::default()
            }),
            "default-database page query should succeed",
        );
        assert_eq!(default_page.entries.len(), 1);
        assert_eq!(
            default_page.entries[0].collection,
            CollectionId::new("tasks")
        );
    }

    // ── Multi-collection tail tests (US-079, FEAT-003) ───────────────────

    fn entry_for(collection: &str, entity: &str, mutation: MutationType) -> AuditEntry {
        AuditEntry::new(
            CollectionId::new(collection),
            EntityId::new(entity),
            1,
            mutation,
            None,
            Some(serde_json::json!({"x": 1})),
            None,
        )
    }

    /// Verifies that multi-collection tail queries return entries interleaved by
    /// global `audit_id` order rather than grouped by collection.
    #[test]
    fn multi_collection_tail_returns_entries_globally_ordered_by_id() {
        let mut log = MemoryAuditLog::default();
        must_ok(
            log.append(entry_for("tasks", "t-001", MutationType::EntityCreate)),
            "tasks create append should succeed",
        );
        must_ok(
            log.append(entry_for("beads", "b-001", MutationType::EntityCreate)),
            "beads create append should succeed",
        );
        must_ok(
            log.append(entry_for("tasks", "t-001", MutationType::EntityUpdate)),
            "tasks update append should succeed",
        );

        let page = must_ok(
            log.query_paginated(AuditQuery {
                collection_ids: vec![CollectionId::new("beads"), CollectionId::new("tasks")],
                ..Default::default()
            }),
            "multi-collection query should succeed",
        );

        assert_eq!(page.entries.len(), 3);
        assert_eq!(page.entries[0].id, 1);
        assert_eq!(page.entries[0].collection, CollectionId::new("tasks"));
        assert_eq!(page.entries[1].id, 2);
        assert_eq!(page.entries[1].collection, CollectionId::new("beads"));
        assert_eq!(page.entries[2].id, 3);
        assert_eq!(page.entries[2].collection, CollectionId::new("tasks"));
    }

    #[test]
    fn multi_collection_tail_omit_collections_returns_all() {
        let mut log = MemoryAuditLog::default();
        must_ok(
            log.append(entry_for("tasks", "t-001", MutationType::EntityCreate)),
            "tasks append should succeed",
        );
        must_ok(
            log.append(entry_for("beads", "b-001", MutationType::EntityCreate)),
            "beads append should succeed",
        );

        let page = must_ok(
            log.query_paginated(AuditQuery {
                collection_ids: vec![],
                collection: None,
                ..Default::default()
            }),
            "unfiltered query should succeed",
        );
        assert_eq!(page.entries.len(), 2);
    }

    #[test]
    fn multi_collection_tail_unions_with_single_collection_field() {
        let mut log = MemoryAuditLog::default();
        must_ok(
            log.append(entry_for("tasks", "t-001", MutationType::EntityCreate)),
            "tasks append should succeed",
        );
        must_ok(
            log.append(entry_for("beads", "b-001", MutationType::EntityCreate)),
            "beads append should succeed",
        );
        must_ok(
            log.append(entry_for("users", "u-001", MutationType::EntityCreate)),
            "users append should succeed",
        );

        // collection_ids=["beads"] + collection=Some("tasks") → union = {beads, tasks}
        let page = must_ok(
            log.query_paginated(AuditQuery {
                collection_ids: vec![CollectionId::new("beads")],
                collection: Some(CollectionId::new("tasks")),
                ..Default::default()
            }),
            "union query should succeed",
        );

        assert_eq!(page.entries.len(), 2);
        let collections: Vec<&str> = page.entries.iter().map(|e| e.collection.as_str()).collect();
        assert!(collections.contains(&"tasks"));
        assert!(collections.contains(&"beads"));
        assert!(!collections.contains(&"users"));
    }

    /// Walks 6 interleaved entries across two collections in pages of 2, verifying that
    /// the unified `audit_id` cursor advances monotonically across both collections and
    /// that `next_cursor` returns `None` only after every requested collection has been
    /// fully drained. Covers US-079 AC #3, #4, and #6.
    #[test]
    fn query_paginated_multi_collection_tail() {
        let mut log = MemoryAuditLog::default();
        // Interleave three collections so global audit_id order differs from per-collection order.
        must_ok(
            log.append(entry_for("tasks", "t-001", MutationType::EntityCreate)),
            "append #1 should succeed",
        ); // id=1
        must_ok(
            log.append(entry_for("beads", "b-001", MutationType::EntityCreate)),
            "append #2 should succeed",
        ); // id=2
        must_ok(
            log.append(entry_for("users", "u-001", MutationType::EntityCreate)),
            "append #3 should succeed",
        ); // id=3 (not in the query set)
        must_ok(
            log.append(entry_for("tasks", "t-001", MutationType::EntityUpdate)),
            "append #4 should succeed",
        ); // id=4
        must_ok(
            log.append(entry_for("beads", "b-002", MutationType::EntityCreate)),
            "append #5 should succeed",
        ); // id=5
        must_ok(
            log.append(entry_for("tasks", "t-002", MutationType::EntityCreate)),
            "append #6 should succeed",
        ); // id=6

        let query_set = vec![CollectionId::new("tasks"), CollectionId::new("beads")];

        // Page 1: expect ids 1 (tasks), 2 (beads). Skips users (id=3).
        let page1 = must_ok(
            log.query_paginated(AuditQuery {
                collection_ids: query_set.clone(),
                limit: Some(2),
                ..Default::default()
            }),
            "page 1 query should succeed",
        );
        assert_eq!(page1.entries.len(), 2);
        assert_eq!(page1.entries[0].id, 1);
        assert_eq!(page1.entries[0].collection, CollectionId::new("tasks"));
        assert_eq!(page1.entries[1].id, 2);
        assert_eq!(page1.entries[1].collection, CollectionId::new("beads"));
        let cursor1 = must_some(page1.next_cursor, "page 1 should have a next cursor");
        assert_eq!(cursor1, 2);

        // Page 2: expect ids 4 (tasks update), 5 (beads). Skips users (id=3) because it's
        // not in the query set, demonstrating that the cursor advances globally but the
        // filter still excludes unrequested collections.
        let page2 = must_ok(
            log.query_paginated(AuditQuery {
                collection_ids: query_set.clone(),
                after_id: Some(cursor1),
                limit: Some(2),
                ..Default::default()
            }),
            "page 2 query should succeed",
        );
        assert_eq!(page2.entries.len(), 2);
        assert_eq!(page2.entries[0].id, 4);
        assert_eq!(page2.entries[0].collection, CollectionId::new("tasks"));
        assert_eq!(page2.entries[1].id, 5);
        assert_eq!(page2.entries[1].collection, CollectionId::new("beads"));
        let cursor2 = must_some(page2.next_cursor, "page 2 should have a next cursor");
        assert_eq!(cursor2, 5);

        // Page 3: only id=6 (tasks) remains; cursor should be None (fully drained).
        let page3 = must_ok(
            log.query_paginated(AuditQuery {
                collection_ids: query_set.clone(),
                after_id: Some(cursor2),
                limit: Some(2),
                ..Default::default()
            }),
            "page 3 query should succeed",
        );
        assert_eq!(page3.entries.len(), 1);
        assert_eq!(page3.entries[0].id, 6);
        assert_eq!(page3.entries[0].collection, CollectionId::new("tasks"));
        assert!(
            page3.next_cursor.is_none(),
            "all requested collections drained → next_cursor must be None"
        );

        // Final empty page from the drained cursor — should yield zero entries and no cursor.
        let page4 = must_ok(
            log.query_paginated(AuditQuery {
                collection_ids: query_set,
                after_id: Some(6),
                limit: Some(2),
                ..Default::default()
            }),
            "page 4 (drained) query should succeed",
        );
        assert!(page4.entries.is_empty());
        assert!(page4.next_cursor.is_none());
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
            must_ok(
                log.append(AuditEntry::new(
                    CollectionId::new("tasks"),
                    EntityId::new(format!("t-{i:03}")),
                    1,
                    MutationType::EntityCreate,
                    None,
                    Some(serde_json::json!({"n": i})),
                    Some("agent".into()),
                )),
                "replay append should succeed",
            );
        }

        let (envelopes, cursor) =
            must_ok(log.replay(None, None, 100), "initial replay should succeed");
        assert_eq!(envelopes.len(), 5);
        // cursor should be None since we got all events.
        assert!(cursor.is_none());
    }

    #[test]
    fn replay_resumable_from_cursor() {
        let mut log = MemoryAuditLog::default();
        for i in 1..=10 {
            must_ok(
                log.append(AuditEntry::new(
                    CollectionId::new("tasks"),
                    EntityId::new(format!("t-{i:03}")),
                    1,
                    MutationType::EntityCreate,
                    None,
                    Some(serde_json::json!({"n": i})),
                    Some("agent".into()),
                )),
                "paged replay append should succeed",
            );
        }

        // First page: 5 events.
        let (page1, cursor1) = must_ok(log.replay(None, None, 5), "first replay page should work");
        assert_eq!(page1.len(), 5);
        assert!(cursor1.is_some());

        // Second page from cursor.
        let (page2, cursor2) = must_ok(
            log.replay(cursor1, None, 5),
            "second replay page should work",
        );
        assert_eq!(page2.len(), 5);
        assert!(cursor2.is_none());

        // audit_ids should not overlap.
        let ids1: Vec<u64> = page1.iter().map(|e| e.source.audit_id).collect();
        let ids2: Vec<u64> = page2.iter().map(|e| e.source.audit_id).collect();
        assert!(ids1[ids1.len() - 1] < ids2[0]);
    }

    #[test]
    fn replay_filters_by_collection() {
        let mut log = MemoryAuditLog::default();
        must_ok(
            log.append(AuditEntry::new(
                CollectionId::new("tasks"),
                EntityId::new("t-001"),
                1,
                MutationType::EntityCreate,
                None,
                Some(serde_json::json!({"task": true})),
                Some("a".into()),
            )),
            "task append should succeed",
        );
        must_ok(
            log.append(AuditEntry::new(
                CollectionId::new("users"),
                EntityId::new("u-001"),
                1,
                MutationType::EntityCreate,
                None,
                Some(serde_json::json!({"user": true})),
                Some("a".into()),
            )),
            "user append should succeed",
        );

        let (envelopes, _) = must_ok(
            log.replay(None, Some(&CollectionId::new("tasks")), 100),
            "collection-filtered replay should succeed",
        );
        assert_eq!(envelopes.len(), 1);
        assert_eq!(envelopes[0].source.collection, "tasks");
    }

    #[test]
    fn replay_collection_events_skipped_in_envelopes() {
        let mut log = MemoryAuditLog::default();
        // Collection create events don't produce CDC envelopes.
        must_ok(
            log.append(AuditEntry::new(
                CollectionId::new("tasks"),
                EntityId::new(""),
                0,
                MutationType::CollectionCreate,
                None,
                None,
                None,
            )),
            "collection-create append should succeed",
        );
        must_ok(
            log.append(AuditEntry::new(
                CollectionId::new("tasks"),
                EntityId::new("t-001"),
                1,
                MutationType::EntityCreate,
                None,
                Some(serde_json::json!({"x": 1})),
                Some("a".into()),
            )),
            "entity-create append should succeed",
        );

        let (envelopes, _) = must_ok(log.replay(None, None, 100), "mixed replay should succeed");
        // Only entity events produce envelopes.
        assert_eq!(envelopes.len(), 1);
    }

    #[test]
    fn replay_dedup_by_audit_id() {
        let mut log = MemoryAuditLog::default();
        for _ in 0..3 {
            must_ok(
                log.append(AuditEntry::new(
                    CollectionId::new("tasks"),
                    EntityId::new("t-001"),
                    1,
                    MutationType::EntityCreate,
                    None,
                    Some(serde_json::json!({})),
                    None,
                )),
                "dedup replay append should succeed",
            );
        }

        let (envelopes, _) = must_ok(log.replay(None, None, 100), "dedup replay should succeed");
        // Each envelope has a unique audit_id.
        let ids: Vec<u64> = envelopes.iter().map(|e| e.source.audit_id).collect();
        let unique: std::collections::HashSet<u64> = ids.iter().copied().collect();
        assert_eq!(ids.len(), unique.len());
    }

    // ── Ordering Invariant Tests ─────────────────────────────────────────

    /// Verifies that query_paginated returns entries ordered by audit_id ascending,
    /// even when internal storage is manipulated to be out-of-order.
    #[test]
    fn query_paginated_enforces_audit_id_ordering() {
        // Create a MemoryAuditLog and manipulate its internal state to insert entries
        // with non-sequential audit_id values, simulating an out-of-order scenario.
        let mut log = MemoryAuditLog::default();

        // Create entries with audit_id values that are not in insertion order
        let mut entry1 = AuditEntry::new(
            CollectionId::new("tasks"),
            EntityId::new("t-003"),
            1, // version
            MutationType::EntityCreate,
            None,
            Some(serde_json::json!({"task": "third"})),
            Some("agent".into()),
        );
        entry1.id = 100; // Set id directly to simulate out-of-order insertion

        let mut entry2 = AuditEntry::new(
            CollectionId::new("tasks"),
            EntityId::new("t-001"),
            1, // version
            MutationType::EntityCreate,
            None,
            Some(serde_json::json!({"task": "first"})),
            Some("agent".into()),
        );
        entry2.id = 50; // Set id directly to simulate out-of-order insertion

        let mut entry3 = AuditEntry::new(
            CollectionId::new("tasks"),
            EntityId::new("t-002"),
            1, // version
            MutationType::EntityCreate,
            None,
            Some(serde_json::json!({"task": "second"})),
            Some("agent".into()),
        );
        entry3.id = 75; // Set id directly to simulate out-of-order insertion

        // Insert entries directly into internal storage (out-of-order by audit_id)
        log.entries.push(entry1);
        log.entries.push(entry2);
        log.entries.push(entry3);

        // Query without any filters - should return entries sorted by audit_id (id)
        let page = log
            .query_paginated(AuditQuery {
                limit: Some(10),
                ..Default::default()
            })
            .unwrap();

        // Verify entries are sorted by audit_id (id) ascending
        assert_eq!(page.entries.len(), 3);

        // The entries should be ordered by id (50, 75, 100), NOT insertion order
        assert_eq!(page.entries[0].id, 50);
        assert_eq!(page.entries[1].id, 75);
        assert_eq!(page.entries[2].id, 100);

        // Verify entries are actually ordered by their id field
        let ids: Vec<u64> = page.entries.iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![50, 75, 100]);
    }

    /// Verifies that known_collections returns entries in deterministic order
    /// and demonstrates the O(N) iteration pattern.
    #[test]
    fn known_collections_are_deterministic() {
        let mut log = MemoryAuditLog::default();

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

        log.append(AuditEntry::new(
            CollectionId::new("beads"),
            EntityId::new("b-001"),
            2,
            MutationType::EntityCreate,
            None,
            Some(serde_json::json!({})),
            None,
        ))
        .unwrap();

        log.append(AuditEntry::new(
            CollectionId::new("tasks"),
            EntityId::new("t-002"),
            3,
            MutationType::EntityUpdate,
            None,
            Some(serde_json::json!({})),
            None,
        ))
        .unwrap();

        let collections = log.known_collections();

        // Should contain exactly two collections
        assert_eq!(collections.len(), 2);
        assert!(collections.contains(&CollectionId::new("tasks")));
        assert!(collections.contains(&CollectionId::new("beads")));
    }
}
