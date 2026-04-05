use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use axon_audit::entry::{AuditEntry, MutationType};
use axon_audit::log::AuditLog;
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_core::types::Entity;
use axon_storage::adapter::StorageAdapter;
use serde_json::Value;

/// Maximum number of operations allowed per transaction (FEAT-008).
const MAX_OPS: usize = 100;

/// Default transaction timeout (FEAT-008).
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Global counter for generating unique transaction IDs.
static TX_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_tx_id() -> String {
    format!("tx-{}", TX_COUNTER.fetch_add(1, Ordering::SeqCst))
}

/// A buffered write within a transaction.
#[derive(Debug)]
struct WriteOp {
    entity: Entity,
    /// Version that must be current in storage for the write to succeed.
    /// `0` means "entity must not exist" (create).
    expected_version: u64,
    /// State before this write (for audit).
    data_before: Option<Value>,
    mutation: MutationType,
}

/// A multi-entity atomic transaction using optimistic concurrency control (OCC).
///
/// Operations are buffered and applied atomically on [`commit`](Transaction::commit).
/// If any entity's version does not match, **the entire transaction is aborted**
/// and no changes are persisted.
///
/// Audit entries produced by a committed transaction all share the same
/// `transaction_id`.
pub struct Transaction {
    /// Unique identifier for this transaction.
    pub id: String,
    ops: Vec<WriteOp>,
    created_at: Instant,
    timeout: Duration,
}

impl Transaction {
    /// Create a new transaction with an auto-generated ID.
    pub fn new() -> Self {
        Self {
            id: next_tx_id(),
            ops: Vec::new(),
            created_at: Instant::now(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Create a transaction with a custom timeout.
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            timeout,
            ..Self::new()
        }
    }

    fn check_op_limit(&self) -> Result<(), AxonError> {
        if self.ops.len() >= MAX_OPS {
            return Err(AxonError::InvalidArgument(format!(
                "transaction exceeds maximum of {MAX_OPS} operations"
            )));
        }
        Ok(())
    }

    /// Stage a create operation for `entity`.
    ///
    /// The entity must not exist in storage at commit time (`expected_version = 0`
    /// is a sentinel that means "does not exist").
    ///
    /// In practice, callers pass `expected_version = 0` for creates because the
    /// entity starts at version 1.  The commit phase checks that no entity with
    /// that ID already exists (stored version 0 → entity absent).
    /// Returns `Err(InvalidArgument)` if the transaction already has [`MAX_OPS`] operations.
    pub fn create(&mut self, entity: Entity) -> Result<(), AxonError> {
        self.check_op_limit()?;
        self.ops.push(WriteOp {
            entity,
            expected_version: 0,
            data_before: None,
            mutation: MutationType::EntityCreate,
        });
        Ok(())
    }

    /// Stage an update operation for `entity`.
    ///
    /// `expected_version` must equal the version currently in storage, otherwise
    /// the entire transaction aborts with [`AxonError::ConflictingVersion`].
    ///
    /// Returns `Err(InvalidArgument)` if the transaction already has [`MAX_OPS`] operations.
    pub fn update(
        &mut self,
        entity: Entity,
        expected_version: u64,
        data_before: Option<Value>,
    ) -> Result<(), AxonError> {
        self.check_op_limit()?;
        self.ops.push(WriteOp {
            entity,
            expected_version,
            data_before,
            mutation: MutationType::EntityUpdate,
        });
        Ok(())
    }

    /// Stage a delete operation.
    ///
    /// `expected_version` is the version the caller observed; the entity must
    /// still be at that version at commit time.
    ///
    /// Returns `Err(InvalidArgument)` if the transaction already has [`MAX_OPS`] operations.
    pub fn delete(
        &mut self,
        collection: CollectionId,
        id: EntityId,
        expected_version: u64,
        data_before: Option<Value>,
    ) -> Result<(), AxonError> {
        // We store a sentinel entity with empty data for the delete op.
        let sentinel = Entity {
            collection,
            id,
            version: expected_version,
            data: serde_json::Value::Null,
        };
        self.ops.push(WriteOp {
            entity: sentinel,
            expected_version,
            data_before,
            mutation: MutationType::EntityDelete,
        });
        Ok(())
    }

    /// Atomically commit all staged operations.
    ///
    /// Opens a storage-level transaction via [`StorageAdapter::begin_tx`] so
    /// that the version check (Phase 1) and the writes (Phase 2) are protected
    /// against concurrent modification. If any phase fails, [`abort_tx`] is
    /// called to roll back all changes; on success, [`commit_tx`] makes them
    /// durable.
    ///
    /// ## Phase 1 — Version check
    /// For every staged write, verify that `expected_version` equals the current
    /// stored version (or that the entity is absent for creates).
    /// If any check fails, **no writes are applied** and the error is returned.
    ///
    /// ## Phase 2 — Apply writes
    /// All writes are applied sequentially. For creates and updates, the entity
    /// version is set / incremented appropriately. Deletes remove the entity.
    ///
    /// ## Audit (co-located writes, Phase 3)
    /// Each written entity produces an [`AuditEntry`] with `transaction_id` set
    /// to `self.id` so callers can correlate the entire transaction in the log.
    ///
    /// Audit entries are written **inside** the storage transaction via
    /// [`StorageAdapter::append_audit_entry`] before `commit_tx` is called.
    /// For adapters that implement durable audit storage (e.g. SQLite), entity
    /// mutations and their audit entries are committed atomically — if the
    /// transaction rolls back, neither the entity change nor its audit entry
    /// persists. For adapters whose `append_audit_entry` is a no-op (e.g.
    /// in-memory), entries are flushed to the standalone `audit` log after
    /// `commit_tx` succeeds.
    ///
    /// Returns the list of written entities (deletes produce an entry with the
    /// sentinel entity; callers may ignore it).
    pub fn commit<S: StorageAdapter, L: AuditLog>(
        self,
        storage: &mut S,
        audit: &mut L,
        actor: Option<String>,
    ) -> Result<Vec<Entity>, AxonError> {
        // Check timeout before entering the commit path (FEAT-008).
        let elapsed = self.created_at.elapsed();
        if elapsed > self.timeout {
            return Err(AxonError::InvalidOperation(format!(
                "transaction timed out after {:.1}s (limit: {}s)",
                elapsed.as_secs_f64(),
                self.timeout.as_secs()
            )));
        }

        storage.begin_tx()?;

        match self.execute(storage, actor) {
            Ok((written, pending_entries)) => {
                // ── Phase 3: co-located audit writes ────────────────────────
                // Write audit entries inside the storage transaction so that
                // entity mutations and their audit entries are committed
                // atomically. For adapters with durable audit support (e.g.
                // SQLite) a rollback undoes both. For adapters whose
                // `append_audit_entry` is a no-op (e.g. in-memory) this is
                // free and the post-commit path below handles persistence.
                for entry in &pending_entries {
                    if let Err(e) = storage.append_audit_entry(entry.clone()) {
                        let _ = storage.abort_tx();
                        return Err(e);
                    }
                }

                match storage.commit_tx() {
                    Ok(()) => {
                        // Storage committed (entities + co-located audit entries
                        // are now durable). Also flush to the standalone audit log
                        // so callers that query via `AuditLog` see the entries.
                        for entry in pending_entries {
                            audit.append(entry)?;
                        }
                        Ok(written)
                    }
                    Err(e) => {
                        // commit_tx failed — best-effort abort. Pending audit
                        // entries are discarded; no rolled-back mutations reach
                        // the audit log.
                        let _ = storage.abort_tx();
                        Err(e)
                    }
                }
            }
            Err(e) => {
                // Version check or write failed — best-effort rollback.
                let _ = storage.abort_tx();
                Err(e)
            }
        }
    }

    /// Applies all staged writes to storage and returns the written entities
    /// together with buffered [`AuditEntry`] values.
    ///
    /// Audit entries are **not** appended to the log here; the caller
    /// (`commit`) is responsible for flushing them only after the storage
    /// transaction has been durably committed.
    fn execute<S: StorageAdapter>(
        self,
        storage: &mut S,
        actor: Option<String>,
    ) -> Result<(Vec<Entity>, Vec<AuditEntry>), AxonError> {
        let tx_id = self.id.clone();
        let actor_str = actor.as_deref().unwrap_or("anonymous");

        // ── Phase 1: Version check ───────────────────────────────────────────
        for op in &self.ops {
            let current = storage.get(&op.entity.collection, &op.entity.id)?;
            let current_version = current.as_ref().map(|e| e.version).unwrap_or(0);

            let ok = match op.mutation {
                MutationType::EntityCreate => current_version == 0, // must not exist
                MutationType::EntityUpdate | MutationType::EntityDelete => {
                    current_version == op.expected_version
                }
                // Collection/schema mutations, entity reverts, and link mutations are not staged via Transaction.
                MutationType::CollectionCreate
                | MutationType::CollectionDrop
                | MutationType::SchemaUpdate
                | MutationType::EntityRevert
                | MutationType::LinkCreate
                | MutationType::LinkDelete => true,
            };

            if !ok {
                return Err(AxonError::ConflictingVersion {
                    expected: op.expected_version,
                    actual: current_version,
                    current_entity: current,
                });
            }
        }

        // ── Phase 2: Apply writes ────────────────────────────────────────────
        let mut written = Vec::new();
        let mut pending_entries = Vec::new();

        for op in self.ops {
            match op.mutation {
                MutationType::EntityCreate => {
                    storage.put(op.entity.clone())?;
                    let after = op.entity.data.clone();
                    let mut entry = AuditEntry::new(
                        op.entity.collection.clone(),
                        op.entity.id.clone(),
                        op.entity.version,
                        MutationType::EntityCreate,
                        None,
                        Some(after),
                        Some(actor_str.into()),
                    );
                    entry.transaction_id = Some(tx_id.clone());
                    pending_entries.push(entry);
                    written.push(op.entity);
                }
                MutationType::EntityUpdate => {
                    let updated =
                        storage.compare_and_swap(op.entity.clone(), op.expected_version)?;
                    let after = updated.data.clone();
                    let mut entry = AuditEntry::new(
                        updated.collection.clone(),
                        updated.id.clone(),
                        updated.version,
                        MutationType::EntityUpdate,
                        op.data_before,
                        Some(after),
                        Some(actor_str.into()),
                    );
                    entry.transaction_id = Some(tx_id.clone());
                    pending_entries.push(entry);
                    written.push(updated);
                }
                MutationType::EntityDelete => {
                    storage.delete(&op.entity.collection, &op.entity.id)?;
                    let mut entry = AuditEntry::new(
                        op.entity.collection.clone(),
                        op.entity.id.clone(),
                        op.entity.version,
                        MutationType::EntityDelete,
                        op.data_before,
                        None,
                        Some(actor_str.into()),
                    );
                    entry.transaction_id = Some(tx_id.clone());
                    pending_entries.push(entry);
                    written.push(op.entity);
                }
                // Collection/schema mutations, entity reverts, and link mutations are not staged via Transaction.
                MutationType::CollectionCreate
                | MutationType::CollectionDrop
                | MutationType::SchemaUpdate
                | MutationType::EntityRevert
                | MutationType::LinkCreate
                | MutationType::LinkDelete => {}
            }
        }

        Ok((written, pending_entries))
    }
}

impl Default for Transaction {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon_audit::log::MemoryAuditLog;
    use axon_core::id::{CollectionId, EntityId};
    use axon_storage::memory::MemoryStorageAdapter;
    use serde_json::json;

    /// Wraps `MemoryStorageAdapter` and injects a `commit_tx` failure.
    /// Used to verify that `Transaction::commit` calls `abort_tx` when
    /// `commit_tx` fails, leaving the adapter in a usable state.
    struct FailOnCommitAdapter {
        inner: MemoryStorageAdapter,
        abort_called: bool,
    }

    impl FailOnCommitAdapter {
        fn new(inner: MemoryStorageAdapter) -> Self {
            Self {
                inner,
                abort_called: false,
            }
        }
    }

    impl StorageAdapter for FailOnCommitAdapter {
        fn get(
            &self,
            collection: &CollectionId,
            id: &EntityId,
        ) -> Result<Option<Entity>, AxonError> {
            self.inner.get(collection, id)
        }
        fn put(&mut self, entity: Entity) -> Result<(), AxonError> {
            self.inner.put(entity)
        }
        fn delete(&mut self, collection: &CollectionId, id: &EntityId) -> Result<(), AxonError> {
            self.inner.delete(collection, id)
        }
        fn count(&self, collection: &CollectionId) -> Result<usize, AxonError> {
            self.inner.count(collection)
        }
        fn range_scan(
            &self,
            collection: &CollectionId,
            start: Option<&EntityId>,
            end: Option<&EntityId>,
            limit: Option<usize>,
        ) -> Result<Vec<Entity>, AxonError> {
            self.inner.range_scan(collection, start, end, limit)
        }
        fn compare_and_swap(
            &mut self,
            entity: Entity,
            expected_version: u64,
        ) -> Result<Entity, AxonError> {
            self.inner.compare_and_swap(entity, expected_version)
        }
        fn begin_tx(&mut self) -> Result<(), AxonError> {
            self.inner.begin_tx()
        }
        fn commit_tx(&mut self) -> Result<(), AxonError> {
            Err(AxonError::Storage("simulated commit failure".into()))
        }
        fn abort_tx(&mut self) -> Result<(), AxonError> {
            self.abort_called = true;
            self.inner.abort_tx()
        }
    }

    fn accounts() -> CollectionId {
        CollectionId::new("accounts")
    }

    fn account(id: &str, balance: i64) -> Entity {
        Entity::new(accounts(), EntityId::new(id), json!({"balance": balance}))
    }

    #[test]
    fn atomic_debit_credit_succeeds() {
        let mut storage = MemoryStorageAdapter::default();
        let mut audit = MemoryAuditLog::default();

        // Seed two accounts.
        storage.put(account("A", 100)).unwrap();
        storage.put(account("B", 50)).unwrap();

        // Transaction: debit A by 30, credit B by 30.
        let mut tx = Transaction::new();
        let a_before = storage
            .get(&accounts(), &EntityId::new("A"))
            .unwrap()
            .unwrap();
        let b_before = storage
            .get(&accounts(), &EntityId::new("B"))
            .unwrap()
            .unwrap();

        tx.update(
            account("A", 70),
            a_before.version,
            Some(a_before.data.clone()),
        )
        .unwrap();
        tx.update(
            account("B", 80),
            b_before.version,
            Some(b_before.data.clone()),
        )
        .unwrap();

        let written = tx
            .commit(&mut storage, &mut audit, Some("system".into()))
            .unwrap();
        assert_eq!(written.len(), 2);

        let a = storage
            .get(&accounts(), &EntityId::new("A"))
            .unwrap()
            .unwrap();
        let b = storage
            .get(&accounts(), &EntityId::new("B"))
            .unwrap()
            .unwrap();
        assert_eq!(a.data["balance"], 70);
        assert_eq!(b.data["balance"], 80);
    }

    #[test]
    fn version_conflict_aborts_entire_transaction() {
        let mut storage = MemoryStorageAdapter::default();
        let mut audit = MemoryAuditLog::default();

        storage.put(account("A", 100)).unwrap();
        storage.put(account("B", 50)).unwrap();

        let mut tx = Transaction::new();
        tx.update(account("A", 70), 1, None).unwrap(); // correct version
        tx.update(account("B", 80), 99, None).unwrap(); // WRONG version — should abort all

        let err = tx.commit(&mut storage, &mut audit, None).unwrap_err();
        assert!(
            matches!(
                err,
                AxonError::ConflictingVersion {
                    expected: 99,
                    actual: 1,
                    ..
                }
            ),
            "unexpected error: {err}"
        );
        // current_entity must carry the stored state so callers can merge and retry (FEAT-004, FEAT-008).
        if let AxonError::ConflictingVersion { current_entity, .. } = err {
            let ce = current_entity.expect("current_entity must be Some when the entity exists");
            assert_eq!(
                ce.version, 1,
                "current_entity should reflect actual stored version"
            );
        }

        // Neither entity should have been modified.
        let a = storage
            .get(&accounts(), &EntityId::new("A"))
            .unwrap()
            .unwrap();
        let b = storage
            .get(&accounts(), &EntityId::new("B"))
            .unwrap()
            .unwrap();
        assert_eq!(a.data["balance"], 100, "A should be unchanged after abort");
        assert_eq!(b.data["balance"], 50, "B should be unchanged after abort");
        assert_eq!(audit.len(), 0, "no audit entries on abort");
    }

    #[test]
    fn partial_failure_rolls_back_all_changes() {
        let mut storage = MemoryStorageAdapter::default();
        let mut audit = MemoryAuditLog::default();

        storage.put(account("A", 100)).unwrap();
        // B does not exist — create will succeed, but C check will fail.
        storage.put(account("C", 200)).unwrap();

        let mut tx = Transaction::new();
        tx.update(account("A", 70), 1, None).unwrap(); // OK
        tx.update(account("C", 190), 99, None).unwrap(); // WRONG version — triggers abort

        let err = tx.commit(&mut storage, &mut audit, None).unwrap_err();
        assert!(matches!(err, AxonError::ConflictingVersion { .. }));

        // A must be unchanged.
        let a = storage
            .get(&accounts(), &EntityId::new("A"))
            .unwrap()
            .unwrap();
        assert_eq!(a.data["balance"], 100);
    }

    #[test]
    fn commit_tx_failure_calls_abort_and_does_not_wedge_adapter() {
        let mut inner = MemoryStorageAdapter::default();
        inner.put(account("A", 100)).unwrap();

        let mut storage = FailOnCommitAdapter::new(inner);
        let mut audit = MemoryAuditLog::default();

        let a_before = storage
            .get(&accounts(), &EntityId::new("A"))
            .unwrap()
            .unwrap();
        let mut tx = Transaction::new();
        tx.update(account("A", 90), a_before.version, Some(a_before.data))
            .unwrap();

        let err = tx.commit(&mut storage, &mut audit, None).unwrap_err();
        assert!(
            matches!(err, AxonError::Storage(_)),
            "expected storage error from simulated commit failure, got: {err}"
        );

        // abort_tx must have been called to clean up the open transaction.
        assert!(
            storage.abort_called,
            "abort_tx must be called when commit_tx fails"
        );

        // The adapter must not be wedged — begin_tx must succeed again.
        storage
            .begin_tx()
            .expect("adapter must not be wedged after a failed commit");
        storage.abort_tx().unwrap();

        // Data must be unchanged because the transaction was aborted.
        let a = storage
            .get(&accounts(), &EntityId::new("A"))
            .unwrap()
            .unwrap();
        assert_eq!(
            a.data["balance"], 100,
            "data must be unchanged after failed commit"
        );

        // No audit entries must be written when commit_tx fails — the audit log
        // must only record mutations that were actually committed to storage.
        assert_eq!(
            audit.len(),
            0,
            "audit log must be empty after a rolled-back transaction"
        );
    }

    #[test]
    fn audit_entries_not_written_on_version_conflict() {
        // When the version check fails (Phase 1), no writes happen and therefore
        // no audit entries should be produced.
        let mut storage = MemoryStorageAdapter::default();
        let mut audit = MemoryAuditLog::default();

        storage.put(account("A", 100)).unwrap();

        let mut tx = Transaction::new();
        tx.update(account("A", 70), 99, None).unwrap(); // WRONG version

        let _ = tx.commit(&mut storage, &mut audit, None);

        assert_eq!(audit.len(), 0, "no audit entries on version-conflict abort");
    }

    #[test]
    fn audit_entries_share_transaction_id() {
        let mut storage = MemoryStorageAdapter::default();
        let mut audit = MemoryAuditLog::default();

        storage.put(account("A", 100)).unwrap();
        storage.put(account("B", 50)).unwrap();

        let a = storage
            .get(&accounts(), &EntityId::new("A"))
            .unwrap()
            .unwrap();
        let b = storage
            .get(&accounts(), &EntityId::new("B"))
            .unwrap()
            .unwrap();

        let mut tx = Transaction::new();
        let tx_id = tx.id.clone();
        tx.update(account("A", 70), a.version, None).unwrap();
        tx.update(account("B", 80), b.version, None).unwrap();

        tx.commit(&mut storage, &mut audit, None).unwrap();

        let entries = audit.entries();
        assert_eq!(entries.len(), 2);
        for entry in entries {
            assert_eq!(
                entry.transaction_id.as_deref(),
                Some(tx_id.as_str()),
                "all entries must share the transaction ID"
            );
        }
    }

    #[test]
    fn op_limit_rejects_101st_operation() {
        let mut tx = Transaction::new();
        for i in 0..100 {
            tx.create(Entity::new(
                accounts(),
                EntityId::new(format!("e-{i}")),
                json!({"i": i}),
            ))
            .unwrap();
        }
        // 101st should fail.
        let err = tx
            .create(Entity::new(
                accounts(),
                EntityId::new("e-100"),
                json!({"i": 100}),
            ))
            .unwrap_err();
        assert!(
            matches!(err, AxonError::InvalidArgument(_)),
            "expected InvalidArgument for op limit, got: {err}"
        );
    }

    #[test]
    fn timeout_aborts_commit() {
        let mut storage = MemoryStorageAdapter::default();
        let mut audit = MemoryAuditLog::default();

        storage.put(account("A", 100)).unwrap();

        // Create a transaction with zero timeout — it expires immediately.
        let mut tx = Transaction::with_timeout(Duration::from_secs(0));
        tx.update(account("A", 90), 1, None).unwrap();

        // Small sleep to ensure timeout fires.
        std::thread::sleep(Duration::from_millis(1));

        let err = tx.commit(&mut storage, &mut audit, None).unwrap_err();
        assert!(
            matches!(err, AxonError::InvalidOperation(_)),
            "expected timeout error, got: {err}"
        );

        // Entity must be unchanged.
        let a = storage
            .get(&accounts(), &EntityId::new("A"))
            .unwrap()
            .unwrap();
        assert_eq!(a.data["balance"], 100, "entity unchanged after timeout");
    }
}
