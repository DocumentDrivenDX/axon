//! Background index build lifecycle (US-034, FEAT-013).
//!
//! When a new index is added to an existing collection, existing entities
//! need to be indexed. This module provides:
//!
//! - **Index states**: `Building` -> `Ready` -> `Dropping`
//! - **Double-write**: new writes update both old and new indexes during build
//! - **Background scan**: iterates existing entities to populate the index

use std::collections::HashMap;

use axon_core::id::CollectionId;
use serde::{Deserialize, Serialize};

/// Lifecycle state of an index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IndexState {
    /// Index is being built; double-writes are active but queries may return
    /// incomplete results.
    Building,
    /// Index is fully built and serving queries.
    Ready,
    /// Index is being dropped; no new writes, draining reads.
    Dropping,
}

/// Metadata for a tracked index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexBuildInfo {
    /// The collection this index belongs to.
    pub collection: CollectionId,
    /// Index field path (for single-field) or name (for compound).
    pub index_name: String,
    /// Current lifecycle state.
    pub state: IndexState,
    /// Number of entities that have been indexed so far.
    pub entities_indexed: usize,
    /// Total number of entities to index (set at build start).
    pub entities_total: usize,
}

impl IndexBuildInfo {
    /// Create a new index build info in `Building` state.
    pub fn new(collection: CollectionId, index_name: String, total: usize) -> Self {
        Self {
            collection,
            index_name,
            state: IndexState::Building,
            entities_indexed: 0,
            entities_total: total,
        }
    }

    /// Progress as a fraction (0.0 to 1.0).
    #[allow(clippy::cast_precision_loss)]
    pub fn progress(&self) -> f64 {
        if self.entities_total == 0 {
            return 1.0;
        }
        self.entities_indexed as f64 / self.entities_total as f64
    }

    /// Mark the index as ready.
    pub fn mark_ready(&mut self) {
        self.state = IndexState::Ready;
        self.entities_indexed = self.entities_total;
    }

    /// Mark the index for dropping.
    pub fn mark_dropping(&mut self) {
        self.state = IndexState::Dropping;
    }
}

/// Registry tracking index build state across collections.
///
/// In production, this would be persisted. For the in-memory adapter,
/// it lives alongside the adapter state.
#[derive(Debug, Default)]
pub struct IndexBuildRegistry {
    /// Active builds keyed by (collection, index_name).
    builds: HashMap<(CollectionId, String), IndexBuildInfo>,
}

impl IndexBuildRegistry {
    /// Start building a new index.
    pub fn start_build(
        &mut self,
        collection: CollectionId,
        index_name: String,
        total_entities: usize,
    ) -> &IndexBuildInfo {
        let info = IndexBuildInfo::new(collection.clone(), index_name.clone(), total_entities);
        self.builds
            .entry((collection, index_name))
            .or_insert(info)
    }

    /// Record progress: increment the count of indexed entities.
    pub fn record_progress(
        &mut self,
        collection: &CollectionId,
        index_name: &str,
        count: usize,
    ) {
        if let Some(info) = self.builds.get_mut(&(collection.clone(), index_name.to_string())) {
            info.entities_indexed = (info.entities_indexed + count).min(info.entities_total);
        }
    }

    /// Mark an index as ready (build complete).
    pub fn mark_ready(&mut self, collection: &CollectionId, index_name: &str) {
        if let Some(info) = self.builds.get_mut(&(collection.clone(), index_name.to_string())) {
            info.mark_ready();
        }
    }

    /// Mark an index for dropping.
    pub fn mark_dropping(&mut self, collection: &CollectionId, index_name: &str) {
        if let Some(info) = self.builds.get_mut(&(collection.clone(), index_name.to_string())) {
            info.mark_dropping();
        }
    }

    /// Remove a tracked index (after drop is complete).
    pub fn remove(&mut self, collection: &CollectionId, index_name: &str) {
        self.builds
            .remove(&(collection.clone(), index_name.to_string()));
    }

    /// Get the state of an index.
    pub fn get(&self, collection: &CollectionId, index_name: &str) -> Option<&IndexBuildInfo> {
        self.builds
            .get(&(collection.clone(), index_name.to_string()))
    }

    /// List all builds for a collection.
    pub fn list_for_collection(&self, collection: &CollectionId) -> Vec<&IndexBuildInfo> {
        self.builds
            .iter()
            .filter(|((col, _), _)| col == collection)
            .map(|(_, info)| info)
            .collect()
    }

    /// Check if an index is ready (or does not exist, meaning it was already built).
    pub fn is_ready(&self, collection: &CollectionId, index_name: &str) -> bool {
        match self.get(collection, index_name) {
            Some(info) => info.state == IndexState::Ready,
            None => true, // No tracking = already ready
        }
    }

    /// Check if double-write is needed for a collection (any index building).
    pub fn needs_double_write(&self, collection: &CollectionId) -> bool {
        self.builds
            .iter()
            .any(|((col, _), info)| col == collection && info.state == IndexState::Building)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tasks() -> CollectionId {
        CollectionId::new("tasks")
    }

    #[test]
    fn start_build_creates_building_state() {
        let mut registry = IndexBuildRegistry::default();
        let info = registry.start_build(tasks(), "status".into(), 100);
        assert_eq!(info.state, IndexState::Building);
        assert_eq!(info.entities_total, 100);
        assert_eq!(info.entities_indexed, 0);
    }

    #[test]
    fn progress_starts_at_zero() {
        let info = IndexBuildInfo::new(tasks(), "status".into(), 100);
        assert!((info.progress() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn progress_updates_correctly() {
        let mut registry = IndexBuildRegistry::default();
        registry.start_build(tasks(), "status".into(), 100);
        registry.record_progress(&tasks(), "status", 50);

        let info = registry.get(&tasks(), "status").unwrap();
        assert!((info.progress() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn mark_ready_transitions_state() {
        let mut registry = IndexBuildRegistry::default();
        registry.start_build(tasks(), "status".into(), 100);
        registry.mark_ready(&tasks(), "status");

        let info = registry.get(&tasks(), "status").unwrap();
        assert_eq!(info.state, IndexState::Ready);
        assert_eq!(info.entities_indexed, 100);
    }

    #[test]
    fn mark_dropping_transitions_state() {
        let mut registry = IndexBuildRegistry::default();
        registry.start_build(tasks(), "status".into(), 100);
        registry.mark_ready(&tasks(), "status");
        registry.mark_dropping(&tasks(), "status");

        let info = registry.get(&tasks(), "status").unwrap();
        assert_eq!(info.state, IndexState::Dropping);
    }

    #[test]
    fn remove_deletes_tracking() {
        let mut registry = IndexBuildRegistry::default();
        registry.start_build(tasks(), "status".into(), 100);
        registry.remove(&tasks(), "status");
        assert!(registry.get(&tasks(), "status").is_none());
    }

    #[test]
    fn is_ready_for_untracked_index() {
        let registry = IndexBuildRegistry::default();
        assert!(registry.is_ready(&tasks(), "status"));
    }

    #[test]
    fn is_ready_false_while_building() {
        let mut registry = IndexBuildRegistry::default();
        registry.start_build(tasks(), "status".into(), 100);
        assert!(!registry.is_ready(&tasks(), "status"));
    }

    #[test]
    fn needs_double_write_during_build() {
        let mut registry = IndexBuildRegistry::default();
        registry.start_build(tasks(), "status".into(), 100);
        assert!(registry.needs_double_write(&tasks()));

        registry.mark_ready(&tasks(), "status");
        assert!(!registry.needs_double_write(&tasks()));
    }

    #[test]
    fn list_for_collection_filters() {
        let mut registry = IndexBuildRegistry::default();
        registry.start_build(tasks(), "status".into(), 100);
        registry.start_build(tasks(), "priority".into(), 50);
        registry.start_build(CollectionId::new("users"), "email".into(), 200);

        let task_builds = registry.list_for_collection(&tasks());
        assert_eq!(task_builds.len(), 2);
    }

    #[test]
    fn progress_for_empty_collection() {
        let info = IndexBuildInfo::new(tasks(), "status".into(), 0);
        assert!((info.progress() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn progress_does_not_exceed_total() {
        let mut registry = IndexBuildRegistry::default();
        registry.start_build(tasks(), "status".into(), 10);
        registry.record_progress(&tasks(), "status", 20); // More than total

        let info = registry.get(&tasks(), "status").unwrap();
        assert_eq!(info.entities_indexed, 10); // Capped at total
    }
}
