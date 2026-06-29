use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use axon_audit::entry::{AuditAttribution, AuditEntry, MutationIntentAuditMetadata, MutationType};
use axon_audit::log::AuditLog;
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_core::types::{Entity, Link};
use axon_schema::validation::validate;
use axon_storage::adapter::StorageAdapter;
use serde_json::Value;

/// Maximum number of operations allowed per transaction (FEAT-008).
const MAX_OPS: usize = 100;

/// Maximum number of tracked reads allowed per transaction (FEAT-008 TXN-05).
///
/// Bounds the per-commit read-set validation cost so a Serializable transaction
/// cannot force an unbounded number of extra storage reads at commit time.
const MAX_READS: usize = 100;

/// Default transaction timeout (FEAT-008).
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Effective isolation level for a transaction (FEAT-008 TXN-05).
///
/// The level is fixed at construction and is inspectable per transaction via
/// [`Transaction::isolation_level`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IsolationLevel {
    /// Snapshot isolation — the V1 default.
    ///
    /// Write-set optimistic concurrency control: every staged write must match
    /// the version currently in storage. Prevents dirty reads, non-repeatable
    /// reads, and lost updates. **Does not detect write skew** — two
    /// transactions that read disjoint records and write into each other's read
    /// set can both commit.
    #[default]
    Snapshot,
    /// Serializable for key-addressed read sets.
    ///
    /// In addition to the snapshot-isolation write-set checks, every entity
    /// recorded via [`Transaction::record_read`] is validated to still be at its
    /// observed version at commit (first-committer-wins on reads). This prevents
    /// write skew whose invariant is expressed over **specific entities read by
    /// id**.
    ///
    /// Predicate/phantom reads recorded via [`Transaction::record_scan_read`] are
    /// additionally validated against the scanned collection's **membership
    /// signature** ([`StorageAdapter::structural_version`](axon_storage::StorageAdapter::structural_version)),
    /// so a concurrent **insert/delete** to a scanned collection aborts (ADR-026).
    ///
    /// Scope limit (honesty): the scan guard is **membership-only** — it does
    /// **not** catch *update-driven* predicate skew (a concurrent in-place update
    /// that flips a predicate without changing the id-set, e.g.
    /// `status: open → closed`). Use [`SerializableStrict`](Self::SerializableStrict)
    /// for that, at a higher abort rate.
    Serializable,
    /// Serializable with a **content** signature for predicate/phantom reads.
    ///
    /// Like [`Serializable`](Self::Serializable) for key-addressed reads, but
    /// scan reads are validated against the scanned collection's
    /// [`content_version`](axon_storage::StorageAdapter::content_version) — a hash
    /// of `(id, version)` pairs — instead of the membership signature. This
    /// catches **update-driven** predicate skew (any concurrent create, delete,
    /// *or* in-place update to a scanned collection aborts), closing the gap
    /// `Serializable` leaves.
    ///
    /// It is intentionally **conservative**: it over-aborts on concurrent updates
    /// to non-matching rows in a scanned collection (table-granular). Precise,
    /// minimal-abort serializability needs full SSI (FEAT-008 TXN-05, ADR-026) and
    /// remains future work. Opt in only for invariants over mutable predicates.
    SerializableStrict,
}

impl IsolationLevel {
    /// Stable machine-readable name for inspection and audit/log surfaces.
    pub fn as_str(&self) -> &'static str {
        match self {
            IsolationLevel::Snapshot => "snapshot",
            IsolationLevel::Serializable => "serializable",
            IsolationLevel::SerializableStrict => "serializable_strict",
        }
    }

    /// Whether this level performs serializable read-set validation at commit
    /// (both [`Serializable`](Self::Serializable) and
    /// [`SerializableStrict`](Self::SerializableStrict)).
    pub fn is_serializable(&self) -> bool {
        matches!(
            self,
            IsolationLevel::Serializable | IsolationLevel::SerializableStrict
        )
    }
}

/// An entity version observed during a transaction, tracked for Serializable
/// read-set validation (FEAT-008 TXN-05). `observed_version == 0` records that
/// the entity was observed **absent**, so a concurrent create is detected.
#[derive(Debug, Clone)]
pub(crate) struct ReadRef {
    pub(crate) collection: CollectionId,
    pub(crate) id: EntityId,
    pub(crate) observed_version: u64,
}

/// A predicate/scan read observed during a transaction, tracked for
/// Serializable phantom validation (FEAT-008 TXN-05, ADR-026).
///
/// Records the [`StorageAdapter::structural_version`] of a scanned collection at
/// read time. At commit the structural version is re-checked; a change means a
/// concurrent create/delete touched the scanned collection (a possible phantom),
/// so the transaction aborts first-committer-wins.
#[derive(Debug, Clone)]
pub(crate) struct ScanReadRef {
    pub(crate) collection: CollectionId,
    pub(crate) observed_structural_version: u64,
}

/// Global counter for generating unique transaction IDs.
static TX_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_tx_id() -> String {
    format!("tx-{}", TX_COUNTER.fetch_add(1, Ordering::SeqCst))
}

/// A buffered write within a transaction.
#[derive(Debug)]
pub(crate) struct WriteOp {
    pub(crate) entity: Entity,
    /// Version that must be current in storage for the write to succeed.
    /// `0` means "entity must not exist" (create).
    pub(crate) expected_version: u64,
    /// State before this write (for audit).
    pub(crate) data_before: Option<Value>,
    pub(crate) mutation: MutationType,
}

#[derive(Debug)]
pub(crate) enum StagedOp {
    Entity(WriteOp),
    LinkCreate(Link),
    LinkDelete(Link),
}

/// A multi-entity atomic transaction using optimistic concurrency control (OCC).
///
/// Operations are buffered and applied atomically on [`commit`](Transaction::commit).
/// If any entity's version does not match, **the entire transaction is aborted**
/// and no changes are persisted.
///
/// Audit entries produced by a committed transaction all share the same
/// `transaction_id`.
///
/// ## Isolation (FEAT-008 TXN-05)
///
/// The default level is [`IsolationLevel::Snapshot`] (write-set OCC). Construct
/// with [`Transaction::with_isolation`] at [`IsolationLevel::Serializable`] and
/// record observed entity versions via [`Transaction::record_read`] to also get
/// first-committer-wins validation over the **key-addressed read set**, which
/// prevents write skew expressed over specific entities read by id. The
/// effective level is inspectable via [`Transaction::isolation_level`].
pub struct Transaction {
    /// Unique identifier for this transaction.
    pub id: String,
    ops: Vec<StagedOp>,
    /// Entity versions observed during the transaction (FEAT-008 TXN-05).
    /// Only populated and validated under [`IsolationLevel::Serializable`].
    reads: Vec<ReadRef>,
    /// Per-collection structural versions observed via scan/predicate reads
    /// (FEAT-008 TXN-05, ADR-026). Only populated and validated under
    /// [`IsolationLevel::Serializable`].
    scan_reads: Vec<ScanReadRef>,
    /// Effective isolation level for this transaction (FEAT-008 TXN-05).
    isolation: IsolationLevel,
    created_at: Instant,
    timeout: Duration,
}

impl Transaction {
    /// Create a new transaction with an auto-generated ID at the default
    /// isolation level ([`IsolationLevel::Snapshot`]).
    pub fn new() -> Self {
        Self {
            id: next_tx_id(),
            ops: Vec::new(),
            reads: Vec::new(),
            scan_reads: Vec::new(),
            isolation: IsolationLevel::default(),
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

    /// Create a transaction at the given isolation level (FEAT-008 TXN-05).
    pub fn with_isolation(isolation: IsolationLevel) -> Self {
        Self {
            isolation,
            ..Self::new()
        }
    }

    /// The effective isolation level of this transaction (FEAT-008 TXN-05).
    ///
    /// The level is inspectable per transaction so callers can confirm whether
    /// the stronger Serializable read-set validation is in effect for the
    /// invariant they care about.
    pub fn isolation_level(&self) -> IsolationLevel {
        self.isolation
    }

    /// Record an entity version that was observed (read) during the transaction.
    ///
    /// Pass `observed_version = 0` to record that the entity was observed
    /// **absent**. Under [`IsolationLevel::Serializable`] every recorded read is
    /// validated to still hold at commit; under [`IsolationLevel::Snapshot`]
    /// this is a no-op (the read-set is not consulted), so snapshot transactions
    /// pay no capture cost.
    ///
    /// Reads recorded here are the **key-addressed** read set: serializable
    /// validation covers exactly these entities. Reads performed via queries,
    /// index scans, traversals, or aggregations are not captured and are not
    /// covered (see [`IsolationLevel::Serializable`]).
    ///
    /// Returns `Err(InvalidArgument)` if the read-set already has [`MAX_READS`]
    /// entries.
    pub fn record_read(
        &mut self,
        collection: CollectionId,
        id: EntityId,
        observed_version: u64,
    ) -> Result<(), AxonError> {
        if !self.isolation.is_serializable() {
            return Ok(());
        }
        if self.reads.len() >= MAX_READS {
            return Err(AxonError::InvalidArgument(format!(
                "transaction exceeds maximum of {MAX_READS} tracked reads"
            )));
        }
        self.reads.push(ReadRef {
            collection,
            id,
            observed_version,
        });
        Ok(())
    }

    /// Record the structural version of a collection observed via a
    /// scan/predicate read (FEAT-008 TXN-05, ADR-026).
    ///
    /// Callers that run a query, index scan, traversal, or aggregation over a
    /// collection under [`IsolationLevel::Serializable`] should record the
    /// collection's [`StorageAdapter::structural_version`] at read time. At
    /// commit, every recorded scan read is re-validated; if the collection's
    /// structural version changed (a concurrent create/delete — a possible
    /// phantom), the transaction aborts first-committer-wins, surfaced as
    /// [`AxonError::ConflictingVersion`] (409, retryable).
    ///
    /// This is the **predicate/phantom** read guard, complementing
    /// [`Self::record_read`]'s key-addressed read set. It is conservative
    /// (collection-granular): it aborts on any concurrent insert/delete to a
    /// scanned collection, even one that does not match the predicate. This is
    /// **sound** — every phantom is a membership change — at the cost of a higher
    /// abort rate (ADR-026).
    ///
    /// Under [`IsolationLevel::Snapshot`] this is a no-op (the scan-read set is
    /// not consulted), so snapshot transactions pay no capture cost.
    ///
    /// Returns `Err(InvalidArgument)` if the scan-read set already has
    /// [`MAX_READS`] entries.
    pub fn record_scan_read(
        &mut self,
        collection: CollectionId,
        observed_structural_version: u64,
    ) -> Result<(), AxonError> {
        if !self.isolation.is_serializable() {
            return Ok(());
        }
        if self.scan_reads.len() >= MAX_READS {
            return Err(AxonError::InvalidArgument(format!(
                "transaction exceeds maximum of {MAX_READS} tracked scan reads"
            )));
        }
        // Holds the level-appropriate signature: the membership
        // (`structural_version`) under Serializable, or the content
        // (`content_version`) signature under SerializableStrict. The caller
        // captures the matching one; commit re-checks with the same.
        self.scan_reads.push(ScanReadRef {
            collection,
            observed_structural_version,
        });
        Ok(())
    }

    fn check_op_limit(&self) -> Result<(), AxonError> {
        if self.ops.len() >= MAX_OPS {
            return Err(AxonError::InvalidArgument(format!(
                "transaction exceeds maximum of {MAX_OPS} operations"
            )));
        }
        Ok(())
    }

    pub(crate) fn staged_ops(&self) -> &[StagedOp] {
        &self.ops
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
        self.ops.push(StagedOp::Entity(WriteOp {
            entity,
            expected_version: 0,
            data_before: None,
            mutation: MutationType::EntityCreate,
        }));
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
        self.ops.push(StagedOp::Entity(WriteOp {
            entity,
            expected_version,
            data_before,
            mutation: MutationType::EntityUpdate,
        }));
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
        self.check_op_limit()?;
        // We store a sentinel entity with empty data for the delete op.
        let sentinel = Entity {
            collection,
            id,
            version: expected_version,
            data: serde_json::Value::Null,
            created_at_ns: None,
            updated_at_ns: None,
            created_by: None,
            updated_by: None,
            schema_version: None,
            gate_results: Default::default(),
        };
        self.ops.push(StagedOp::Entity(WriteOp {
            entity: sentinel,
            expected_version,
            data_before,
            mutation: MutationType::EntityDelete,
        }));
        Ok(())
    }

    /// Stage a typed link creation.
    pub fn create_link(&mut self, link: Link) -> Result<(), AxonError> {
        self.check_op_limit()?;
        self.ops.push(StagedOp::LinkCreate(link));
        Ok(())
    }

    /// Stage a typed link deletion.
    pub fn delete_link(&mut self, link: Link) -> Result<(), AxonError> {
        self.check_op_limit()?;
        self.ops.push(StagedOp::LinkDelete(link));
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
        attribution: Option<AuditAttribution>,
    ) -> Result<Vec<Entity>, AxonError> {
        let (written, ()) =
            self.commit_with_storage_hook(storage, audit, actor, attribution, None, |_| Ok(()))?;
        Ok(written)
    }

    pub(crate) fn commit_with_storage_hook<S, L, F, T>(
        self,
        storage: &mut S,
        audit: &mut L,
        actor: Option<String>,
        attribution: Option<AuditAttribution>,
        intent_lineage: Option<MutationIntentAuditMetadata>,
        before_commit: F,
    ) -> Result<(Vec<Entity>, T), AxonError>
    where
        S: StorageAdapter,
        L: AuditLog,
        F: FnOnce(&mut S) -> Result<T, AxonError>,
    {
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

        match self.execute(storage, actor, attribution, intent_lineage) {
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

                let hook_result = match before_commit(storage) {
                    Ok(result) => result,
                    Err(e) => {
                        let _ = storage.abort_tx();
                        return Err(e);
                    }
                };

                match storage.commit_tx() {
                    Ok(()) => {
                        // Storage committed (entities + co-located audit entries
                        // are now durable). Also flush to the standalone audit log
                        // so callers that query via `AuditLog` see the entries.
                        for entry in pending_entries {
                            audit.append(entry)?;
                        }
                        Ok((written, hook_result))
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
        attribution: Option<AuditAttribution>,
        intent_lineage: Option<MutationIntentAuditMetadata>,
    ) -> Result<(Vec<Entity>, Vec<AuditEntry>), AxonError> {
        let tx_id = self.id.clone();
        let actor_str = actor.as_deref().unwrap_or("anonymous");

        // ── Phase 1: Version check ───────────────────────────────────────────
        for op in &self.ops {
            match op {
                StagedOp::Entity(op) => {
                    let current = storage.get(&op.entity.collection, &op.entity.id)?;
                    let current_version = current.as_ref().map(|e| e.version).unwrap_or(0);

                    let ok = match op.mutation {
                        MutationType::EntityCreate => current_version == 0, // must not exist
                        MutationType::EntityUpdate | MutationType::EntityDelete => {
                            current_version == op.expected_version
                        }
                        MutationType::CollectionCreate
                        | MutationType::CollectionDrop
                        | MutationType::TemplateCreate
                        | MutationType::TemplateUpdate
                        | MutationType::TemplateDelete
                        | MutationType::SchemaUpdate
                        | MutationType::EntityRevert
                        | MutationType::LinkCreate
                        | MutationType::LinkDelete
                        | MutationType::GuardrailRejection
                        | MutationType::IntentPreview
                        | MutationType::IntentApprove
                        | MutationType::IntentReject
                        | MutationType::IntentExpire
                        | MutationType::IntentCommit => true,
                    };

                    if !ok {
                        return Err(AxonError::ConflictingVersion {
                            expected: op.expected_version,
                            actual: current_version,
                            current_entity: current.map(Box::new),
                        });
                    }

                    if matches!(
                        op.mutation,
                        MutationType::EntityCreate | MutationType::EntityUpdate
                    ) {
                        if let Some(schema) = storage.get_schema(&op.entity.collection)? {
                            validate(&schema, &op.entity.data)?;
                        }
                    }
                }
                StagedOp::LinkCreate(link) => {
                    if storage
                        .get(&link.source_collection, &link.source_id)?
                        .is_none()
                    {
                        return Err(AxonError::NotFound(format!(
                            "source entity {}/{}",
                            link.source_collection, link.source_id
                        )));
                    }
                    if storage
                        .get(&link.target_collection, &link.target_id)?
                        .is_none()
                    {
                        return Err(AxonError::NotFound(format!(
                            "target entity {}/{}",
                            link.target_collection, link.target_id
                        )));
                    }
                    let link_id = Link::storage_id(
                        &link.source_collection,
                        &link.source_id,
                        &link.link_type,
                        &link.target_collection,
                        &link.target_id,
                    );
                    if storage.get(&Link::links_collection(), &link_id)?.is_some() {
                        return Err(AxonError::AlreadyExists(format!(
                            "link {}/{}/{}/{}/{}",
                            link.source_collection,
                            link.source_id,
                            link.link_type,
                            link.target_collection,
                            link.target_id
                        )));
                    }
                }
                StagedOp::LinkDelete(link) => {
                    let link_id = Link::storage_id(
                        &link.source_collection,
                        &link.source_id,
                        &link.link_type,
                        &link.target_collection,
                        &link.target_id,
                    );
                    if storage.get(&Link::links_collection(), &link_id)?.is_none() {
                        return Err(AxonError::NotFound(format!(
                            "link {}/{} --[{}]--> {}/{}",
                            link.source_collection,
                            link.source_id,
                            link.link_type,
                            link.target_collection,
                            link.target_id
                        )));
                    }
                }
            }
        }

        // ── Phase 1b: Serializable read-set validation (FEAT-008 TXN-05) ──────
        // Under Serializable isolation, every entity observed via `record_read`
        // must still be at its observed version. A changed (or newly created /
        // deleted) read entity means a concurrent transaction wrote into this
        // transaction's read set — a serialization anomaly (write skew) — so we
        // abort first-committer-wins. Surfaced as `ConflictingVersion` (409,
        // retryable) like any other OCC conflict. This covers the key-addressed
        // read set only; predicate/phantom anomalies are out of scope (ADR-004).
        if self.isolation.is_serializable() {
            for r in &self.reads {
                let current = storage.get(&r.collection, &r.id)?;
                let current_version = current.as_ref().map(|e| e.version).unwrap_or(0);
                if current_version != r.observed_version {
                    return Err(AxonError::ConflictingVersion {
                        expected: r.observed_version,
                        actual: current_version,
                        current_entity: current.map(Box::new),
                    });
                }
            }

            // ── Phase 1b (cont.): predicate/phantom scan-read validation ─────
            // (FEAT-008 TXN-05, ADR-026). Every scanned collection's signature
            // must be unchanged. Under Serializable the signature is the
            // membership `structural_version` (a concurrent create/delete — a
            // phantom — aborts). Under SerializableStrict it is the
            // `content_version` (a concurrent create, delete, *or in-place
            // update* aborts), closing the update-driven predicate-skew gap. The
            // caller recorded the matching signature; we re-check with the same.
            // Both are fail-closed on adapters lacking support, so a scan read
            // recorded against such an adapter aborts loudly.
            let strict = matches!(self.isolation, IsolationLevel::SerializableStrict);
            for sr in &self.scan_reads {
                let current_signature = if strict {
                    storage.content_version(&sr.collection)?
                } else {
                    storage.structural_version(&sr.collection)?
                };
                if current_signature != sr.observed_structural_version {
                    return Err(AxonError::ConflictingVersion {
                        expected: sr.observed_structural_version,
                        actual: current_signature,
                        current_entity: None,
                    });
                }
            }
        }

        // ── Phase 2: Apply writes ────────────────────────────────────────────
        let mut written = Vec::new();
        let mut pending_entries = Vec::new();

        // Attribution is shared across all entries in a single transaction commit.
        let tx_attribution = attribution;
        let tx_intent_lineage = intent_lineage;

        for op in self.ops {
            match op {
                StagedOp::Entity(op) => match op.mutation {
                    MutationType::EntityCreate => {
                        // Secondary-index maintenance and unique-constraint
                        // enforcement (FEAT-013) are performed atomically by the
                        // `put` primitive itself, inside the surrounding
                        // `begin_tx`/`commit_tx`.
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
                        entry = attach_audit_context(
                            entry,
                            tx_attribution.as_ref(),
                            tx_intent_lineage.as_ref(),
                        );
                        pending_entries.push(entry);
                        written.push(op.entity);
                    }
                    MutationType::EntityUpdate => {
                        // Secondary-index maintenance and unique-constraint
                        // enforcement (FEAT-013) are performed atomically by the
                        // `compare_and_swap` primitive itself, inside the
                        // surrounding `begin_tx`/`commit_tx`.
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
                        entry = attach_audit_context(
                            entry,
                            tx_attribution.as_ref(),
                            tx_intent_lineage.as_ref(),
                        );
                        pending_entries.push(entry);
                        written.push(updated);
                    }
                    MutationType::EntityDelete => {
                        // Secondary-index removal (FEAT-013) is performed
                        // atomically by the `delete` primitive itself, inside the
                        // surrounding `begin_tx`/`commit_tx`.
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
                        entry = attach_audit_context(
                            entry,
                            tx_attribution.as_ref(),
                            tx_intent_lineage.as_ref(),
                        );
                        pending_entries.push(entry);
                        written.push(op.entity);
                    }
                    MutationType::CollectionCreate
                    | MutationType::CollectionDrop
                    | MutationType::TemplateCreate
                    | MutationType::TemplateUpdate
                    | MutationType::TemplateDelete
                    | MutationType::SchemaUpdate
                    | MutationType::EntityRevert
                    | MutationType::LinkCreate
                    | MutationType::LinkDelete
                    | MutationType::GuardrailRejection
                    | MutationType::IntentPreview
                    | MutationType::IntentApprove
                    | MutationType::IntentReject
                    | MutationType::IntentExpire
                    | MutationType::IntentCommit => {}
                },
                StagedOp::LinkCreate(link) => {
                    storage.put_link(&link)?;
                    let link_entity = link.to_entity();
                    let mut entry = AuditEntry::new(
                        link_entity.collection.clone(),
                        link_entity.id.clone(),
                        link_entity.version,
                        MutationType::LinkCreate,
                        None,
                        Some(link_entity.data.clone()),
                        Some(actor_str.into()),
                    );
                    entry.transaction_id = Some(tx_id.clone());
                    entry = attach_audit_context(
                        entry,
                        tx_attribution.as_ref(),
                        tx_intent_lineage.as_ref(),
                    );
                    pending_entries.push(entry);
                    written.push(link_entity);
                }
                StagedOp::LinkDelete(link) => {
                    let link_id = Link::storage_id(
                        &link.source_collection,
                        &link.source_id,
                        &link.link_type,
                        &link.target_collection,
                        &link.target_id,
                    );
                    let link_entity = storage
                        .get(&Link::links_collection(), &link_id)?
                        .ok_or_else(|| AxonError::NotFound(format!("link {}", link_id)))?;
                    storage.delete_link(
                        &link.source_collection,
                        &link.source_id,
                        &link.link_type,
                        &link.target_collection,
                        &link.target_id,
                    )?;
                    let mut entry = AuditEntry::new(
                        link_entity.collection.clone(),
                        link_entity.id.clone(),
                        link_entity.version,
                        MutationType::LinkDelete,
                        Some(link_entity.data.clone()),
                        None,
                        Some(actor_str.into()),
                    );
                    entry.transaction_id = Some(tx_id.clone());
                    entry = attach_audit_context(
                        entry,
                        tx_attribution.as_ref(),
                        tx_intent_lineage.as_ref(),
                    );
                    pending_entries.push(entry);
                    written.push(link_entity);
                }
            }
        }

        Ok((written, pending_entries))
    }
}

fn attach_audit_context(
    mut entry: AuditEntry,
    attribution: Option<&AuditAttribution>,
    intent_lineage: Option<&MutationIntentAuditMetadata>,
) -> AuditEntry {
    if let Some(attr) = attribution {
        entry = entry.with_attribution(attr.clone());
    }
    if let Some(lineage) = intent_lineage {
        entry = entry.with_intent_lineage(lineage.clone());
    }
    entry
}

impl Default for Transaction {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
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
        fn create_if_absent(
            &mut self,
            entity: Entity,
            expected_absent_version: u64,
        ) -> Result<Entity, AxonError> {
            self.inner.create_if_absent(entity, expected_absent_version)
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
            .commit(&mut storage, &mut audit, Some("system".into()), None)
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

        let err = tx.commit(&mut storage, &mut audit, None, None).unwrap_err();
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

        let err = tx.commit(&mut storage, &mut audit, None, None).unwrap_err();
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

        let err = tx.commit(&mut storage, &mut audit, None, None).unwrap_err();
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

        let _ = tx.commit(&mut storage, &mut audit, None, None);

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

        tx.commit(&mut storage, &mut audit, None, None).unwrap();

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

        let err = tx.commit(&mut storage, &mut audit, None, None).unwrap_err();
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

    #[test]
    fn op_limit_applies_to_delete() {
        let mut tx = Transaction::new();
        for i in 0..100 {
            tx.delete(accounts(), EntityId::new(format!("e-{i}")), 1, None)
                .unwrap();
        }
        // 101st delete should fail.
        let err = tx
            .delete(accounts(), EntityId::new("e-100"), 1, None)
            .unwrap_err();
        assert!(
            matches!(err, AxonError::InvalidArgument(_)),
            "expected InvalidArgument for delete op limit, got: {err}"
        );
    }

    #[test]
    fn isolation_level_defaults_to_snapshot_and_is_inspectable() {
        let tx = Transaction::new();
        assert_eq!(tx.isolation_level(), IsolationLevel::Snapshot);
        let tx = Transaction::with_isolation(IsolationLevel::Serializable);
        assert_eq!(tx.isolation_level(), IsolationLevel::Serializable);
        assert_eq!(IsolationLevel::Serializable.as_str(), "serializable");
    }

    // Classic write-skew: invariant "X.flag + Y.flag >= 1". Two transactions
    // each read the *other* entity (disjoint write sets, crossing read sets).
    // Under Snapshot isolation both commit and the invariant is violated;
    // under Serializable the second committer is aborted.

    #[test]
    fn write_skew_is_allowed_under_snapshot_isolation() {
        let mut storage = MemoryStorageAdapter::default();
        let mut audit = MemoryAuditLog::default();
        storage.put(account("X", 1)).unwrap(); // flag = balance, both start at 1
        storage.put(account("Y", 1)).unwrap();

        // T1 reads Y (sees 1), clears X. Snapshot: record_read is a no-op.
        let mut t1 = Transaction::new();
        t1.record_read(accounts(), EntityId::new("Y"), 1).unwrap();
        t1.update(account("X", 0), 1, None).unwrap();
        t1.commit(&mut storage, &mut audit, Some("t1".into()), None)
            .expect("T1 commits");

        // T2 read X *before* T1 committed (observed version 1), clears Y.
        let mut t2 = Transaction::new();
        t2.record_read(accounts(), EntityId::new("X"), 1).unwrap();
        t2.update(account("Y", 0), 1, None).unwrap();
        t2.commit(&mut storage, &mut audit, Some("t2".into()), None)
            .expect("under snapshot isolation T2 also commits (write skew allowed)");

        let x = storage
            .get(&accounts(), &EntityId::new("X"))
            .unwrap()
            .unwrap();
        let y = storage
            .get(&accounts(), &EntityId::new("Y"))
            .unwrap()
            .unwrap();
        // Invariant X+Y >= 1 is VIOLATED — both are 0. This documents the SI gap.
        assert_eq!(x.data["balance"], 0);
        assert_eq!(y.data["balance"], 0);
    }

    #[test]
    fn write_skew_is_prevented_under_serializable_isolation() {
        let mut storage = MemoryStorageAdapter::default();
        let mut audit = MemoryAuditLog::default();
        storage.put(account("X", 1)).unwrap();
        storage.put(account("Y", 1)).unwrap();

        // T1: read Y@1, clear X. Commits, bumping X to version 2.
        let mut t1 = Transaction::with_isolation(IsolationLevel::Serializable);
        t1.record_read(accounts(), EntityId::new("Y"), 1).unwrap();
        t1.update(account("X", 0), 1, None).unwrap();
        t1.commit(&mut storage, &mut audit, Some("t1".into()), None)
            .expect("T1 commits on fresh state");

        // T2 observed X@1 before T1 committed; clears Y. Its read of X is now
        // stale (X is at version 2), so serializable validation must abort it.
        let mut t2 = Transaction::with_isolation(IsolationLevel::Serializable);
        t2.record_read(accounts(), EntityId::new("X"), 1).unwrap();
        t2.update(account("Y", 0), 1, None).unwrap();
        let err = t2
            .commit(&mut storage, &mut audit, Some("t2".into()), None)
            .expect_err("serializable must reject T2: its read set changed");
        assert!(
            matches!(
                err,
                AxonError::ConflictingVersion {
                    expected: 1,
                    actual: 2,
                    ..
                }
            ),
            "expected read-set serialization conflict, got: {err}"
        );

        // Invariant holds: Y was never cleared (T2 aborted), so X+Y == 1.
        let x = storage
            .get(&accounts(), &EntityId::new("X"))
            .unwrap()
            .unwrap();
        let y = storage
            .get(&accounts(), &EntityId::new("Y"))
            .unwrap()
            .unwrap();
        assert_eq!(x.data["balance"], 0);
        assert_eq!(y.data["balance"], 1, "Y must be untouched after T2 aborts");
    }

    #[test]
    fn serializable_detects_concurrent_create_of_observed_absent_entity() {
        // A transaction whose invariant depends on an entity being ABSENT must
        // abort if that entity is concurrently created (key-addressed phantom).
        let mut storage = MemoryStorageAdapter::default();
        let mut audit = MemoryAuditLog::default();
        storage.put(account("anchor", 1)).unwrap();

        // Concurrently, "Z" gets created (version 1).
        storage.put(account("Z", 5)).unwrap();

        let mut tx = Transaction::with_isolation(IsolationLevel::Serializable);
        tx.record_read(accounts(), EntityId::new("Z"), 0).unwrap(); // observed absent
        tx.update(account("anchor", 2), 1, None).unwrap();
        let err = tx
            .commit(&mut storage, &mut audit, None, None)
            .expect_err("must abort: observed-absent Z now exists");
        assert!(
            matches!(err, AxonError::ConflictingVersion { expected: 0, .. }),
            "expected conflict on observed-absent read, got: {err}"
        );
    }

    // ── Predicate/phantom write skew (FEAT-008 TXN-05, ADR-026) ──────────────
    //
    // Invariant: "at most one on-call engineer". The anomaly is a *phantom* —
    // each transaction inserts a NEW row that the other never read by id, so the
    // key-addressed read set cannot see it. Only the per-collection
    // structural-version guard catches it.

    fn engineers() -> CollectionId {
        CollectionId::new("engineers")
    }

    fn on_call(id: &str) -> Entity {
        Entity::new(engineers(), EntityId::new(id), json!({"on_call": true}))
    }

    #[test]
    fn phantom_write_skew_allowed_under_snapshot() {
        // Two transactions each scan engineers, see zero on-call, and each
        // INSERTS a new on-call engineer. Under Snapshot both commit and the
        // "at most one on-call" invariant is violated — documents the SI gap.
        let mut storage = MemoryStorageAdapter::default();
        let mut audit = MemoryAuditLog::default();

        // T1: scan (snapshot → record_scan_read is a no-op), insert E1.
        let mut t1 = Transaction::new();
        let v1 = storage.structural_version(&engineers()).unwrap();
        t1.record_scan_read(engineers(), v1).unwrap();
        t1.create(on_call("E1")).unwrap();
        t1.commit(&mut storage, &mut audit, Some("t1".into()), None)
            .expect("T1 commits");

        // T2 scanned before T1 inserted, inserts E2. Snapshot allows it.
        let mut t2 = Transaction::new();
        t2.record_scan_read(engineers(), v1).unwrap();
        t2.create(on_call("E2")).unwrap();
        t2.commit(&mut storage, &mut audit, Some("t2".into()), None)
            .expect("under snapshot isolation T2 also commits (phantom skew allowed)");

        // Invariant "at most one on-call" is VIOLATED — both rows exist.
        assert_eq!(storage.count(&engineers()).unwrap(), 2);
    }

    #[test]
    fn phantom_write_skew_prevented_under_serializable() {
        // Same scenario under Serializable: T2's scanned collection changed
        // structurally (T1 inserted E1), so its scan read is stale and the
        // structural-version guard aborts it.
        let mut storage = MemoryStorageAdapter::default();
        let mut audit = MemoryAuditLog::default();

        let v0 = storage.structural_version(&engineers()).unwrap();

        // T1: scan@v0, insert E1. Commits, bumping the structural version.
        let mut t1 = Transaction::with_isolation(IsolationLevel::Serializable);
        t1.record_scan_read(engineers(), v0).unwrap();
        t1.create(on_call("E1")).unwrap();
        t1.commit(&mut storage, &mut audit, Some("t1".into()), None)
            .expect("T1 commits on fresh state");

        // T2 observed the collection at v0 before T1 inserted; inserts E2.
        // Its scan read is now stale (structural version advanced), so
        // serializable validation must abort it.
        let mut t2 = Transaction::with_isolation(IsolationLevel::Serializable);
        t2.record_scan_read(engineers(), v0).unwrap();
        t2.create(on_call("E2")).unwrap();
        let err = t2
            .commit(&mut storage, &mut audit, Some("t2".into()), None)
            .expect_err("serializable must reject T2: its scanned collection changed");
        assert!(
            matches!(err, AxonError::ConflictingVersion { expected, .. } if expected == v0),
            "expected phantom serialization conflict, got: {err}"
        );

        // Invariant holds: only E1 was inserted (T2 aborted).
        assert_eq!(storage.count(&engineers()).unwrap(), 1);
    }

    #[test]
    fn serializable_scan_read_tolerates_pure_updates() {
        // A pure in-place update does NOT change collection membership, so the
        // structural version must not advance and a scan read over a
        // concurrently-updated (but not inserted/deleted) collection must NOT
        // falsely abort. (Updates are covered by the key-addressed read set.)
        let mut storage = MemoryStorageAdapter::default();
        let mut audit = MemoryAuditLog::default();
        storage.put(on_call("E1")).unwrap();

        let v = storage.structural_version(&engineers()).unwrap();

        // Concurrent pure update of E1 (version 1 → 2): membership unchanged.
        storage
            .compare_and_swap(
                Entity::new(engineers(), EntityId::new("E1"), json!({"on_call": false})),
                1,
            )
            .unwrap();

        // A serializable txn that scanned the collection commits unaffected by
        // the update because the structural version is unchanged.
        let mut tx = Transaction::with_isolation(IsolationLevel::Serializable);
        tx.record_scan_read(engineers(), v).unwrap();
        tx.create(on_call("E2")).unwrap();
        tx.commit(&mut storage, &mut audit, None, None)
            .expect("pure update must not bump the structural version");
    }

    #[test]
    fn record_scan_read_is_noop_under_snapshot() {
        // Snapshot must not consult the scan-read set: a stale scan-read must
        // not cause an abort.
        let mut storage = MemoryStorageAdapter::default();
        let mut audit = MemoryAuditLog::default();
        let mut tx = Transaction::new();
        // Record a deliberately-wrong structural version; snapshot ignores it.
        tx.record_scan_read(engineers(), 999).unwrap();
        tx.create(on_call("E1")).unwrap();
        tx.commit(&mut storage, &mut audit, None, None)
            .expect("snapshot must ignore the scan-read set");
    }
}
