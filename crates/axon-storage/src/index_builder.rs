//! Background index build lifecycle (US-034, FEAT-013).
//!
//! When a new index is added to an existing collection, existing entities
//! need to be indexed. This module provides:
//!
//! - **Index states**: `Building` -> `Ready` -> `Dropping`
//! - **Double-write**: new writes update both old and new indexes during build
//! - **Background scan**: iterates existing entities to populate the index
//! - **Completion channel**: [`IndexBuildRegistry::start_build`] hands back a
//!   [`CompletionReceiver`] so callers (including tests) can deterministically
//!   `.wait().await` for a build to finish instead of polling or sleeping.

use std::collections::HashMap;

use axon_core::error::AxonError;
use axon_core::id::CollectionId;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

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

/// Runtime status broadcast over the build completion channel.
///
/// This is a **signalling** enum — it is not persisted and does not replace
/// [`IndexState`]. A failed build leaves [`IndexState`] at `Building` so the
/// caller can choose to drop, retry, or escalate; the failure is surfaced via
/// the channel only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexBuildStatus {
    /// Build is still in progress. This is the channel's initial value.
    Building,
    /// Build completed successfully; the index is ready to serve queries.
    Ready,
    /// Build failed. The string carries a human-readable error message.
    Failed(String),
}

impl IndexBuildStatus {
    /// Returns `true` if the status represents a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, IndexBuildStatus::Ready | IndexBuildStatus::Failed(_))
    }
}

/// Terminal outcome reported by [`CompletionReceiver::wait`].
///
/// The completion channel collapses to an `Ok(BuildOutcome)` on a clean
/// terminal transition; transport-level problems (sender dropped without
/// sending a terminal value) are surfaced as `Err(AxonError)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildOutcome {
    /// Build finished cleanly and the index is live.
    Ready,
    /// Build failed with a human-readable reason. The caller decides whether
    /// to retry, drop, or escalate.
    Failed(String),
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

    /// Mark the index as ready (in-memory state transition only — this does
    /// **not** fire the completion channel; use
    /// [`IndexBuildRegistry::mark_ready`] to transition state _and_ notify
    /// subscribers atomically).
    pub fn mark_ready(&mut self) {
        self.state = IndexState::Ready;
        self.entities_indexed = self.entities_total;
    }

    /// Mark the index for dropping.
    pub fn mark_dropping(&mut self) {
        self.state = IndexState::Dropping;
    }
}

/// Receiver half of a build completion channel.
///
/// Returned from [`IndexBuildRegistry::start_build`] and
/// [`IndexBuildRegistry::subscribe`]. Call [`CompletionReceiver::wait`] to
/// block asynchronously until the build reaches a terminal status. Drop the
/// receiver to detach without waiting.
///
/// Because the underlying channel retains its last value, a receiver that
/// subscribes **after** `mark_ready`/`mark_failed` fired will still observe
/// the terminal status immediately — callers never miss completion.
#[derive(Debug, Clone)]
pub struct CompletionReceiver {
    inner: watch::Receiver<IndexBuildStatus>,
}

impl CompletionReceiver {
    /// Wait for the build to reach a terminal status.
    ///
    /// Returns `Ok(BuildOutcome::Ready)` on successful completion,
    /// `Ok(BuildOutcome::Failed(msg))` on a build failure, or
    /// `Err(AxonError::Storage(..))` if the sender was dropped before
    /// publishing a terminal status (indicates a bug or abrupt registry
    /// teardown).
    pub async fn wait(mut self) -> Result<BuildOutcome, AxonError> {
        loop {
            {
                let current = self.inner.borrow_and_update().clone();
                match current {
                    IndexBuildStatus::Ready => return Ok(BuildOutcome::Ready),
                    IndexBuildStatus::Failed(msg) => return Ok(BuildOutcome::Failed(msg)),
                    IndexBuildStatus::Building => {}
                }
            }
            if self.inner.changed().await.is_err() {
                return Err(AxonError::Storage(
                    "index build sender dropped before completion".to_string(),
                ));
            }
        }
    }

    /// Current status snapshot without awaiting. Useful for progress polling
    /// and debug logging.
    pub fn status(&self) -> IndexBuildStatus {
        self.inner.borrow().clone()
    }
}

/// Registry tracking index build state across collections.
///
/// In production, this would be persisted. For the in-memory adapter,
/// it lives alongside the adapter state. The registry owns the sender half
/// of each build's completion channel, so
/// [`IndexBuildRegistry::mark_ready`] / [`IndexBuildRegistry::mark_failed`]
/// broadcast completion to every subscribed [`CompletionReceiver`].
#[derive(Debug, Default)]
pub struct IndexBuildRegistry {
    /// Active builds keyed by (collection, index_name).
    builds: HashMap<(CollectionId, String), IndexBuildInfo>,
    /// Completion senders, one per active build. Held separately from
    /// `builds` so the stored `IndexBuildInfo` values remain cheap to clone
    /// and don't carry a channel handle around.
    senders: HashMap<(CollectionId, String), watch::Sender<IndexBuildStatus>>,
}

impl IndexBuildRegistry {
    /// Start building a new index.
    ///
    /// Returns a [`CompletionReceiver`] that resolves when the build
    /// transitions to `Ready` or `Failed`. If a build for this key already
    /// exists, the receiver is a fresh subscription to the existing sender
    /// (so callers racing on the same index all observe the same outcome).
    ///
    /// Drop the returned receiver to detach without waiting.
    pub fn start_build(
        &mut self,
        collection: CollectionId,
        index_name: String,
        total_entities: usize,
    ) -> CompletionReceiver {
        let key = (collection.clone(), index_name.clone());
        if let Some(tx) = self.senders.get(&key) {
            return CompletionReceiver {
                inner: tx.subscribe(),
            };
        }
        let (tx, rx) = watch::channel(IndexBuildStatus::Building);
        let info = IndexBuildInfo::new(collection, index_name, total_entities);
        self.builds.insert(key.clone(), info);
        self.senders.insert(key, tx);
        CompletionReceiver { inner: rx }
    }

    /// Record progress: increment the count of indexed entities.
    pub fn record_progress(&mut self, collection: &CollectionId, index_name: &str, count: usize) {
        if let Some(info) = self
            .builds
            .get_mut(&(collection.clone(), index_name.to_string()))
        {
            info.entities_indexed = (info.entities_indexed + count).min(info.entities_total);
        }
    }

    /// Mark an index as ready (build complete) and broadcast
    /// [`IndexBuildStatus::Ready`] on the completion channel.
    pub fn mark_ready(&mut self, collection: &CollectionId, index_name: &str) {
        let key = (collection.clone(), index_name.to_string());
        if let Some(info) = self.builds.get_mut(&key) {
            info.mark_ready();
        }
        if let Some(tx) = self.senders.get(&key) {
            // Ignore send errors: no subscribers just means nobody was
            // waiting, which is a valid case (fire-and-forget builds).
            let _ = tx.send(IndexBuildStatus::Ready);
        }
    }

    /// Mark an index build as failed and broadcast
    /// [`IndexBuildStatus::Failed`] on the completion channel.
    ///
    /// This does **not** mutate [`IndexState`] — a failed build is left in
    /// `Building` so the caller can decide whether to drop, retry, or
    /// escalate. The failure is observable only via the channel.
    pub fn mark_failed(
        &mut self,
        collection: &CollectionId,
        index_name: &str,
        error: impl Into<String>,
    ) {
        let key = (collection.clone(), index_name.to_string());
        if let Some(tx) = self.senders.get(&key) {
            let _ = tx.send(IndexBuildStatus::Failed(error.into()));
        }
    }

    /// Mark an index for dropping.
    pub fn mark_dropping(&mut self, collection: &CollectionId, index_name: &str) {
        if let Some(info) = self
            .builds
            .get_mut(&(collection.clone(), index_name.to_string()))
        {
            info.mark_dropping();
        }
    }

    /// Remove a tracked index (after drop is complete). Drops the sender,
    /// causing any outstanding [`CompletionReceiver::wait`] calls that
    /// haven't already observed a terminal value to resolve with
    /// `Err(AxonError::Storage(..))`.
    pub fn remove(&mut self, collection: &CollectionId, index_name: &str) {
        let key = (collection.clone(), index_name.to_string());
        self.builds.remove(&key);
        self.senders.remove(&key);
    }

    /// Get the state of an index.
    pub fn get(&self, collection: &CollectionId, index_name: &str) -> Option<&IndexBuildInfo> {
        self.builds
            .get(&(collection.clone(), index_name.to_string()))
    }

    /// Subscribe to an existing build's completion channel without starting
    /// a new one. Returns `None` if no build is registered for the key.
    ///
    /// Use this when multiple callers need to wait for the same build that
    /// was started elsewhere.
    pub fn subscribe(
        &self,
        collection: &CollectionId,
        index_name: &str,
    ) -> Option<CompletionReceiver> {
        self.senders
            .get(&(collection.clone(), index_name.to_string()))
            .map(|tx| CompletionReceiver {
                inner: tx.subscribe(),
            })
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
        let rx = registry.start_build(tasks(), "status".into(), 100);
        assert_eq!(rx.status(), IndexBuildStatus::Building);

        let info = registry
            .get(&tasks(), "status")
            .expect("test operation should succeed");
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
        let _rx = registry.start_build(tasks(), "status".into(), 100);
        registry.record_progress(&tasks(), "status", 50);

        let info = registry
            .get(&tasks(), "status")
            .expect("test operation should succeed");
        assert!((info.progress() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn mark_ready_transitions_state() {
        let mut registry = IndexBuildRegistry::default();
        let _rx = registry.start_build(tasks(), "status".into(), 100);
        registry.mark_ready(&tasks(), "status");

        let info = registry
            .get(&tasks(), "status")
            .expect("test operation should succeed");
        assert_eq!(info.state, IndexState::Ready);
        assert_eq!(info.entities_indexed, 100);
    }

    #[test]
    fn mark_dropping_transitions_state() {
        let mut registry = IndexBuildRegistry::default();
        let _rx = registry.start_build(tasks(), "status".into(), 100);
        registry.mark_ready(&tasks(), "status");
        registry.mark_dropping(&tasks(), "status");

        let info = registry
            .get(&tasks(), "status")
            .expect("test operation should succeed");
        assert_eq!(info.state, IndexState::Dropping);
    }

    #[test]
    fn remove_deletes_tracking() {
        let mut registry = IndexBuildRegistry::default();
        let _rx = registry.start_build(tasks(), "status".into(), 100);
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
        let _rx = registry.start_build(tasks(), "status".into(), 100);
        assert!(!registry.is_ready(&tasks(), "status"));
    }

    #[test]
    fn needs_double_write_during_build() {
        let mut registry = IndexBuildRegistry::default();
        let _rx = registry.start_build(tasks(), "status".into(), 100);
        assert!(registry.needs_double_write(&tasks()));

        registry.mark_ready(&tasks(), "status");
        assert!(!registry.needs_double_write(&tasks()));
    }

    #[test]
    fn list_for_collection_filters() {
        let mut registry = IndexBuildRegistry::default();
        let _a = registry.start_build(tasks(), "status".into(), 100);
        let _b = registry.start_build(tasks(), "priority".into(), 50);
        let _c = registry.start_build(CollectionId::new("users"), "email".into(), 200);

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
        let _rx = registry.start_build(tasks(), "status".into(), 10);
        registry.record_progress(&tasks(), "status", 20); // More than total

        let info = registry
            .get(&tasks(), "status")
            .expect("test operation should succeed");
        assert_eq!(info.entities_indexed, 10); // Capped at total
    }

    // -- Completion channel tests (FEAT-013) ---------------------------------

    #[tokio::test]
    async fn completion_receiver_resolves_on_mark_ready() {
        let mut registry = IndexBuildRegistry::default();
        let rx = registry.start_build(tasks(), "status".into(), 100);

        // Spawn the waiter first so it has to park on the channel rather
        // than observe the terminal value synchronously.
        let waiter = tokio::spawn(async move { rx.wait().await });

        tokio::task::yield_now().await;
        registry.mark_ready(&tasks(), "status");

        let outcome = waiter
            .await
            .expect("waiter task panicked")
            .expect("wait should succeed");
        assert_eq!(outcome, BuildOutcome::Ready);
    }

    #[tokio::test]
    async fn completion_receiver_resolves_on_mark_failed() {
        let mut registry = IndexBuildRegistry::default();
        let rx = registry.start_build(tasks(), "status".into(), 100);

        let waiter = tokio::spawn(async move { rx.wait().await });

        tokio::task::yield_now().await;
        registry.mark_failed(&tasks(), "status", "disk full");

        let outcome = waiter
            .await
            .expect("waiter task panicked")
            .expect("wait should succeed");
        assert_eq!(outcome, BuildOutcome::Failed("disk full".to_string()));
    }

    #[tokio::test]
    async fn completion_receiver_observes_already_terminal_status() {
        let mut registry = IndexBuildRegistry::default();
        let rx = registry.start_build(tasks(), "status".into(), 100);
        registry.mark_ready(&tasks(), "status");

        // The build is already Ready before wait() is polled. The watch
        // channel retains its last value, so the caller still observes
        // Ready — and does so without hanging.
        let outcome = tokio::time::timeout(std::time::Duration::from_secs(1), rx.wait())
            .await
            .expect("wait should not hang after mark_ready")
            .expect("wait should succeed");
        assert_eq!(outcome, BuildOutcome::Ready);
    }

    #[tokio::test]
    async fn completion_receiver_errors_when_sender_dropped() {
        let mut registry = IndexBuildRegistry::default();
        let rx = registry.start_build(tasks(), "status".into(), 100);

        // Dropping the whole registry (and thus the sender) before a
        // terminal value is sent must surface as an AxonError::Storage.
        drop(registry);
        let err = rx
            .wait()
            .await
            .expect_err("wait should fail when sender drops");
        match err {
            AxonError::Storage(msg) => {
                assert!(msg.contains("sender dropped"), "got {msg}");
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[tokio::test]
    async fn subscribe_yields_receiver_for_active_build() {
        let mut registry = IndexBuildRegistry::default();
        let _starter = registry.start_build(tasks(), "status".into(), 100);

        let rx = registry
            .subscribe(&tasks(), "status")
            .expect("subscribe should return a receiver for an active build");
        assert_eq!(rx.status(), IndexBuildStatus::Building);

        registry.mark_ready(&tasks(), "status");
        let outcome = rx.wait().await.expect("wait should succeed");
        assert_eq!(outcome, BuildOutcome::Ready);
    }

    #[tokio::test]
    async fn subscribe_returns_none_for_unknown_build() {
        let registry = IndexBuildRegistry::default();
        assert!(registry.subscribe(&tasks(), "status").is_none());
    }

    #[tokio::test]
    async fn background_task_drives_completion_without_sleep() {
        // This test mirrors the shape of a real background index scan:
        // the registry lives behind a Mutex, a spawned task walks the
        // entity set, records progress, and marks ready — the caller
        // awaits the completion receiver instead of polling or sleeping.
        //
        // It doubles as a conversion of any earlier sleep-based test and
        // demonstrates the EAV-style "entries are visible after wait()
        // returns" contract via the registry's progress counter, which
        // is the observable the in-memory adapter uses today.
        use std::sync::Arc;
        use tokio::sync::Mutex;

        const TOTAL: usize = 5;

        let registry = Arc::new(Mutex::new(IndexBuildRegistry::default()));
        let rx = registry
            .lock()
            .await
            .start_build(tasks(), "status".into(), TOTAL);

        let registry_task = Arc::clone(&registry);
        tokio::spawn(async move {
            for _ in 0..TOTAL {
                tokio::task::yield_now().await;
                registry_task
                    .lock()
                    .await
                    .record_progress(&tasks(), "status", 1);
            }
            // Last entry has been written; flip the lifecycle and notify.
            registry_task.lock().await.mark_ready(&tasks(), "status");
        });

        let outcome = rx.wait().await.expect("wait should succeed");
        assert_eq!(outcome, BuildOutcome::Ready);

        // After wait() returns the registry reflects the completed build:
        // state = Ready and all TOTAL entries accounted for (the EAV
        // equivalent for the in-memory registry).
        let guard = registry.lock().await;
        let info = guard.get(&tasks(), "status").expect("build tracked");
        assert_eq!(info.state, IndexState::Ready);
        assert_eq!(info.entities_indexed, TOTAL);
        assert!(guard.is_ready(&tasks(), "status"));
    }

    #[test]
    fn is_terminal_classifies_variants() {
        assert!(!IndexBuildStatus::Building.is_terminal());
        assert!(IndexBuildStatus::Ready.is_terminal());
        assert!(IndexBuildStatus::Failed("x".into()).is_terminal());
    }
}
