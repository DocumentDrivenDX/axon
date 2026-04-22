---
ddx:
  id: ADR-015
  depends_on:
    - ADR-004
    - FEAT-003
    - FEAT-008
    - FEAT-023
---
# ADR-015: Rollback and Recovery — Compensating Transaction Semantics

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-04-13 | Accepted | Erik LaBianca | FEAT-023, ADR-004, FEAT-003, FEAT-008 | High |

## Context

FEAT-023 promises structured rollback powered by the audit log: point-in-time
rollback, entity-level rollback, transaction-level rollback, and dry-run
preview. The audit log (FEAT-003) captures every mutation with full
before/after state; ADR-004 makes entity-level OCC the backbone of
concurrency control. The open design question is *how* rollback interacts
with both: does rollback rewrite history, mutate version pointers, or write
new data?

Getting this wrong would compromise the two invariants the audit log and
transaction model exist to guarantee — append-only auditability (INV-003)
and deterministic OCC (ADR-004). A version-pointer rewrite approach would
silently violate both: observers downstream of the audit log (FEAT-021 CDC
feeds, GraphQL subscriptions, sync replicas) would miss the state change,
and OCC invariants about monotonic versions would need backend-specific
exemption paths.

This ADR resolves the semantics so FEAT-023 implementation work can proceed
without relitigating the core model for every rollback variant.

| Aspect | Description |
|--------|-------------|
| Problem | Define rollback semantics that preserve append-only audit, OCC invariants, and cross-entity atomicity |
| Current State | Single-entity `EntityRevert` exists (US-008); no transaction-level or point-in-time rollback; no dry-run; no shared rollback grouping |
| Requirements | FEAT-023 acceptance criteria: entity/transaction/point-in-time rollback, dry-run, conflict reporting, rollback-of-rollback |
| Prior context | ADR-004 (OCC + version vectors), FEAT-003 (append-only audit log with full before/after), FEAT-008 (ACID transactions with shared `transaction_id`) |

## Decision

### 1. Rollback is a Compensating Write, Not a History Rewrite

**A rollback produces new writes at `version = current_version + 1` whose
payload is the target prior state.** It does not mutate existing audit
entries, does not reassign `version` pointers, and does not physically
remove any data.

For a single-entity rollback targeting audit entry `E` for entity `e`:

```text
let target_state      = E.data_before       // the state we want to restore
let current_entity    = storage.get(e)
let compensating_op = WriteOp {
    entity_id:        e,
    expected_version: current_entity.version,
    data_after:       target_state,
    mutation_type:    EntityRollback,  // new variant; see §4
    rollback_source:  Some(E.id),      // audit reference
}
```

The compensating write then flows through the ordinary OCC commit path
(ADR-004), the ordinary audit flush, and the ordinary change-feed
projection (ADR-014). Rollback has zero special cases in the storage or
audit layers — it is a normal mutation whose *payload* happens to be a
historical snapshot.

**Why not rewrite version pointers?** A version-pointer rewrite would:
- Violate FEAT-003's append-only invariant (entries would need mutation).
- Break the audit log as a source of truth for change feeds (FEAT-021 CDC
  consumers would receive *no event* for the rollback, silently diverging
  from Axon state).
- Require backend-specific rollback paths (PostgreSQL, SQLite, and memory
  each encode versions differently).
- Defeat the "rollback of a rollback" acceptance criterion — a second
  rewrite would have nothing to rewrite back *to*.

**Why not physical delete?** Even a "safe" delete of post-target audit
entries destroys observable history that compliance, debugging, and replay
(FEAT-026) all depend on. The audit log must remain the complete story of
what happened, including the mistake and its correction.

**Consequence**: after a rollback, the affected entity has *more* audit
entries than before, not fewer. The history of the rollback is itself
recoverable, which is exactly what lets "rollback of a rollback" work —
the compensating write for undoing a rollback is just another
compensating write targeting the pre-rollback state.

### 2. OCC Applies Unchanged to Compensating Writes

Rollback compensating writes go through the same `expected_version`
check that ADR-004 describes for every other mutation. For entity `e`,
the compensating write's `expected_version` is `e`'s *current* version
at the moment the compensating transaction commits — not the target
version being restored.

```text
Given target audit entry E for entity e at historical version V_target:
  let V_current = storage.get(e).version
  let compensating_write.expected_version = V_current
  let compensating_write.data_after       = E.data_before
  // On commit, e.version becomes V_current + 1
```

#### Conflict Semantics

If another transaction modifies `e` between the caller reading the
rollback intent and the compensating transaction committing, the OCC
check fails with `AxonError::ConflictingVersion` — identical to any
other concurrent-writer race. The caller sees:

```rust
ConflictingVersion {
    expected: V_current,   // what we saw at rollback plan time
    actual:   V_now,       // what's in storage
    current_entity: <state at V_now>,
}
```

The caller's resolution options are the same as for any OCC conflict:
re-plan the rollback against the newer version, merge intelligently,
or abort. **Axon does not attempt automatic conflict resolution** for
rollback — the semantics of "roll back over a concurrent edit" are
application-specific (is the concurrent edit a bug fix that should be
preserved, or another piece of the bad state being corrected?).

#### The "Moving Target" Case

For point-in-time rollback, the set of entities to revert is discovered
by scanning the audit log after the cutoff. Between that scan and the
commit, new mutations to those entities can land. Two policies were
considered:

| Policy | Behavior |
|--------|----------|
| **Fail-fast (selected)** | Use the version observed during planning as the `expected_version`. Any interleaving write produces a conflict. Caller sees a deterministic list of conflicts and decides. |
| Last-writer-wins | Ignore versions, unconditionally apply the compensating write. Silent data loss; violates ADR-004. Rejected. |

Fail-fast is the only policy consistent with ADR-004 and FEAT-008
conflict guarantees. The dry-run path (§5) exists precisely to let
callers preview and resolve conflicts before committing.

### 3. Cross-Entity Atomicity: Compensating Transactions

Transaction-level and point-in-time rollbacks touch multiple entities.
FEAT-023 requires cross-entity consistency: "all entities in the original
transaction are rolled back atomically." This is satisfied by grouping
all compensating writes into a **single new transaction**.

```text
fn rollback_transaction(source_tx_id: TxId) -> Result<RollbackSummary> {
    let affected = audit.entries_for_transaction(source_tx_id);
    let compensations: Vec<WriteOp> = affected
        .iter()
        .map(|e| compensating_write_for(e))
        .collect();

    // Single new transaction, shared rollback_transaction_id.
    let tx = Transaction::new_rollback(source_tx_id);
    for op in compensations {
        tx.stage(op)?;
    }
    tx.commit()  // OCC check runs once, for all entities, atomically.
}
```

The compensating transaction inherits the ADR-004 commit protocol:
- All version checks run inside a single `begin_tx()` / `commit_tx()`
  storage transaction.
- **Any single OCC conflict aborts the entire compensating transaction.**
  Partial rollback is rejected by construction.
- On abort, the conflict list (entity, expected, actual, current state
  for every conflicting entity) is returned to the caller so they see
  *all* conflicts at once, not just the first.
- On success, all compensating writes commit atomically and a single
  `transaction_id` (the new compensating transaction's) is stamped on
  every new audit entry.

#### The `rollback_transaction_id` Link

The new compensating transaction carries a `rollback_source_transaction_id`
field pointing at the original transaction being compensated. This is
distinct from the ordinary `transaction_id` (which identifies the
compensating transaction itself in the audit log).

```rust
pub struct Transaction {
    pub id: TransactionId,
    /// Set when this transaction is a compensating (rollback) transaction.
    /// Points at the original transaction being undone.
    pub rollback_source_transaction_id: Option<TransactionId>,
    // ... existing fields
}
```

Point-in-time rollback is *not* tied to a single source transaction — it
reverts mutations from many source transactions. For that case the
`rollback_source_transaction_id` is `None` and the per-entry audit
`rollback_source_audit_id` (§4) is the provenance link.

### 4. Audit Trail for Compensating Writes

A new `MutationType::EntityRollback` variant is added alongside the
existing `EntityRevert`. **`EntityRevert` is preserved** as the
single-entity, single-audit-entry revert that already exists in
`axon-api/src/handler.rs` (US-008); `EntityRollback` is the new
transaction-aware form used by FEAT-023's transaction-level and
point-in-time rollback flows.

| Variant | Origin | Grouping |
|---------|--------|----------|
| `EntityRevert` | US-008 `revert_entity_to_audit_entry` | Single entity, standalone |
| `EntityRollback` | FEAT-023 transaction / point-in-time rollback | Grouped under a compensating `transaction_id` |

Both emit dot-notation `entity.revert` / `entity.rollback` respectively.

#### New Audit Entry Fields

Two optional provenance fields are added to `AuditEntry`:

```rust
pub struct AuditEntry {
    // ... existing fields ...

    /// If this entry records a compensating write, this is the audit
    /// entry ID whose `data_before` was restored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rollback_source_audit_id: Option<u64>,

    /// If this entry is part of a compensating transaction, this is the
    /// original transaction ID being compensated. None for point-in-time
    /// rollback (which spans transactions) and for normal mutations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rollback_source_transaction_id: Option<TransactionId>,
}
```

Both fields are append-only and immutable like every other audit field.
They are populated by the rollback implementation at audit-flush time
and visible to all downstream consumers (FEAT-021 CDC, GraphQL
subscriptions, `axon audit list`).

#### Observability

`axon audit list --entity <id>` on a rolled-back entity shows, in
chronological order: the mutations that are being undone, followed by
the `entity.rollback` entries that undid them, each pointing back at
its `rollback_source_audit_id`. The rollback operation is
self-describing from audit alone — no external provenance store is
needed.

### 5. Dry-Run Semantics

Dry-run is a pure function over the audit log and current entity state:

```text
fn dry_run_rollback(scope: RollbackScope) -> RollbackPreview {
    // 1. Walk the audit log to find entries in scope.
    let affected_entries = audit.query(scope);

    // 2. For each affected entity, compute the target state.
    let targets = group_by_entity(affected_entries)
        .map(|(e, entries)| (e, earliest(entries).data_before));

    // 3. Read current state of each entity.
    let current_states = storage.get_many(targets.keys());

    // 4. Build the proposed compensating write set.
    let writes = targets.zip(current_states)
        .map(|(t, c)| WriteOp {
            entity_id: t.entity,
            expected_version: c.version,
            data_after: t.target_state,
            ...
        });

    // 5. Detect OCC conflicts without committing: an entry is a conflict
    //    if the current version differs from the version at scope cutoff.
    let conflicts = detect_conflicts(&writes, &scope);

    RollbackPreview { writes, conflicts }
}
```

Dry-run **must**:
- Acquire no write locks.
- Produce no audit entries.
- Return a structurally identical `RollbackPreview` to the summary that
  a real rollback would report, so callers can compare dry-run and live
  results.
- Surface conflicts the same way the commit path would — the conflict
  list format is shared between dry-run and OCC-abort responses.

Dry-run **does not guarantee** that a subsequent real rollback will see
the same set of conflicts; concurrent writes between dry-run and commit
can introduce new conflicts. Dry-run is a best-effort preview, not a
reservation. Callers that need stronger guarantees must serialize
rollback operations externally or wrap dry-run and commit in a single
retry loop.

### 6. Rollback of a Rollback

By construction, a compensating write is a normal write with an
`entity.rollback` mutation type. Rolling it back works exactly the same
way as rolling back any other mutation: the caller selects the audit
entry (or transaction) for the unwanted rollback and issues a new
compensating write whose payload is *that* entry's `data_before` —
which is the state that existed before the rollback, i.e., the
post-bad-state that the first rollback undid.

This satisfies FEAT-023 acceptance criterion: "Rollback of a rollback
(re-apply) works correctly" with no special-case code.

### 7. Known Constraints

Two implementation constraints in the initial FEAT-023 delivery are
not conceptual limitations of the compensating-write model but are
scoped out of V1 pending dedicated follow-on work.

#### 7a. Transaction Rollback Rejects Create/Delete Source Mutations

`Handler::rollback_transaction` (the transaction-level rollback entry
point) **rejects source transactions that include entity creates or
deletes**. A rollback request whose source transaction contains an
`EntityCreate` or `EntityDelete` mutation returns
`AxonError::InvalidOperation` with a message naming the offending
entity. Only `EntityUpdate`, `EntityRevert`, and `EntityRollback`
source mutations are supported by the V1 transaction rollback path.

**Rationale.** The cross-entity atomicity contract (§3) requires the
compensating transaction to stage every compensating write through a
single `begin_tx`/`commit_tx` via the OCC `compare_and_swap` path. That
path handles *updates only*: reversing an `EntityCreate` requires
issuing a `storage.delete`, and reversing an `EntityDelete` requires
reconstructing a prior-version entity whose `created_at_ns`,
`created_by`, and lineage metadata cannot be trivially reconstructed
from audit state alone (the creation may itself have been a revert of
an earlier delete, etc.). Mixing delete and create compensating
operations into the shared transaction also breaks the single-path OCC
check — `compare_and_swap` has no equivalent for "expect absent, then
create" or "expect present at version V, then delete".

Rather than quietly skip these mutations (which would violate the
all-or-nothing contract and produce silent partial rollbacks), V1
fails fast with a clear error so callers understand the limitation.
A separate bead tracks full create/delete rollback support (see
Follow-on Work below).

**What this means for callers.**

| Source transaction shape | V1 transaction rollback behavior |
|---|---|
| Updates only | Full rollback, atomic |
| Any create | Rejected with `InvalidOperation` — caller must fall back to per-entity compensation |
| Any delete | Rejected with `InvalidOperation` — same fallback |
| Mixed updates + create/delete | Rejected — the update portion is **not** partially applied |

Point-in-time rollback (§7b) handles creates and deletes via its
per-entity best-effort path, so callers whose source transaction is
purely a create/delete workload can use PIT rollback with an
appropriate cutoff as a workaround.

#### 7b. Point-in-Time Rollback is Best-Effort, Not All-or-Nothing

`Handler::rollback_collection` (the point-in-time collection rollback
entry point) is **best-effort at the entity level**, not atomic across
the collection. This is a deliberate divergence from the transaction
rollback contract (§3), driven by three characteristics unique to PIT
rollback:

1. **PIT rollback is not bounded by a single source transaction.** It
   reverts mutations from potentially thousands of source transactions
   scanned out of the audit log by timestamp cutoff, and easily exceeds
   the 100-op transaction limit (FEAT-008). Grouping every
   compensating write into one transaction is structurally impossible
   for large rollbacks.
2. **The compensating operation set is heterogeneous.** A single PIT
   rollback can mix restores, recreates (entities deleted after the
   cutoff), and deletes (entities created after the cutoff). Unlike
   transaction rollback, there is no uniform `compare_and_swap` path
   the whole set flows through, so the shared-transaction OCC abort
   guarantee is unavailable.
3. **PIT rollback is a recovery tool, not a business operation.** When
   an operator is unwinding bad state after an incident, applying 99
   of 100 compensating writes and reporting the one failure is
   strictly more useful than refusing to apply anything.

**Contract.** `rollback_collection` applies every compensating write
it can and reports per-entity outcomes:

- Successful compensating writes are listed in `compensating_writes`.
- Entities whose compensation could not be applied (OCC drift between
  plan time and commit, storage drift from the audit view, or
  concurrent re-appearance of a deleted entity) are listed in
  `partial_failures`.
- The response field is named `partial_failures` — not `conflicts` —
  to make the best-effort contract unambiguous at the type level.
- A full-success rollback has `partial_failures == []`.
- A fully-blocked rollback has `compensating_writes == []` and
  `partial_failures` populated with the blocked entities.
- A partially-applied rollback has both lists non-empty; the
  compensating transaction ID (`compensating_transaction_id`) groups
  whatever writes did land so callers can inspect them in the audit
  log.

**Retry semantics.** Callers observing a non-empty `partial_failures`
list can retry the PIT rollback — the compensating writes already
applied become new audit entries that subsequent runs skip past, and
the still-failing entities get a fresh plan/commit attempt. Retrying
until `partial_failures` is empty is safe and idempotent (modulo
concurrent writers continuing to produce new drift).

**Why not mirror transaction rollback's all-or-nothing contract?**
Two alternatives were considered and rejected:

| Alternative | Rejection reason |
|---|---|
| Run PIT rollback inside a single `begin_tx`/`commit_tx` | Capped at 100 ops (FEAT-008); excludes the primary use case (large incident recovery). |
| Run PIT rollback as a chain of 100-op transactions, abort all on any failure | Requires a distributed compensating-of-compensating cleanup path that does not exist in V1 and would itself be best-effort; adds complexity for marginal benefit over per-entity reporting. |

The per-entity best-effort contract is the honest minimum viable
semantic. A "strict mode" that refuses to commit any writes when any
entity fails is a P1 follow-on (Follow-on Work), as is chunked
rollback with chunk-level atomicity.

#### 7c. Follow-on Work

The following beads track removing these constraints:

- **Full create/delete transaction rollback.** Extend
  `rollback_transaction` to stage compensating `put`/`delete`
  operations alongside updates inside a single compensating
  transaction, with reconstruction of creation metadata from the
  earliest audit entry for each entity. Tracked as a separate P1
  bead filed alongside this ADR amendment.
- **PIT rollback strict mode.** Add an opt-in `strict: bool` flag to
  `RollbackCollectionRequest` that causes any entity-level failure to
  abort the whole rollback by issuing inverse compensating writes for
  any entries already applied in the current run. P2 follow-on.
- **Chunked PIT rollback with chunk-level atomicity.** Break a PIT
  rollback into N-op chunks, each of which is atomic via shared
  transaction; the overall rollback remains best-effort across
  chunks. P2 follow-on.

Until these follow-ons land, callers of the V1 API should treat
`rollback_transaction` as "updates-only, atomic" and
`rollback_collection` as "any mutation, best-effort per entity".

### 8. Explicit V1 Out of Scope

The following are explicitly **not** V1 concerns and should not gate
FEAT-023 shipping:

| Feature | Status | Rationale |
|---------|--------|-----------|
| **CRDT merging** of rollback conflicts | Deferred | Requires per-type merge semantics; application-specific. Callers resolve conflicts manually. |
| **Automatic conflict resolution** during rollback | Deferred | "Last-writer-wins" silently loses data; anything smarter is application-specific. V1 returns conflict list to caller. |
| **Saga compensation** / stepwise compensating actions | Deferred | Axon rollback compensates at the *data* layer. External side effects (emails sent, payments processed) are not reversed — that is the caller's concern, and belongs in an application-level saga library. |
| **Rolling back schema changes** | Deferred | FEAT-017 schema evolution governs schema changes. A rollback whose `data_before` does not match the current schema fails schema validation (same guard as US-008 `revert_entity_to_audit_entry`). `--force` escape hatch is a P1 follow-on. |
| **Rolling back audit entries themselves** | Out of scope forever | The audit log is append-only. There is no "undo" for audit entries — that would defeat their purpose. |
| **Cross-database rollback** | Deferred | V1 rollback operates within a single Axon database. Multi-database coordination requires distributed transactions (P2). |
| **Partial rollback** of a failing *transaction-level* compensating transaction | Rejected | FEAT-023 and §3 require all-or-nothing for transaction-level rollback. Callers who want partial rollback of updates should issue multiple single-entity rollbacks. Point-in-time rollback is a separate contract — see §7b, it **is** best-effort by design. |
| **Time-bounded undo window** with GC after N days | Deferred | V1 retains all audit entries (FEAT-003 Constraints). Retention policies are a P2 FEAT-003 deliverable. |

## Alternatives Considered

### A1. Version Pointer Rewrite

Treat rollback as repointing `entity.current_version` at a prior audit
entry without writing new data.

| Pros | Cons |
|------|------|
| O(1) storage cost | Violates FEAT-003 append-only invariant |
| No OCC interaction required | CDC/change-feed consumers see *no event* — silent divergence |
| | "Rollback of rollback" has nothing to re-point to |
| | Backend-specific: SQLite, PostgreSQL, and memory adapters each encode versions differently |
| | Breaks ADR-004 monotonic version guarantee |

**Rejected.** The audit log stops being the source of truth the moment
a rollback can silently disappear from it.

### A2. Audit-Entry Physical Delete

Physically remove post-cutoff audit entries and reset entity state to
the pre-cutoff snapshot.

| Pros | Cons |
|------|------|
| Leaves entity state identical to "it never happened" | Destroys compliance evidence (GDPR aside — compliance teams need the mistake *and* the fix) |
| | Breaks any external replica or CDC consumer that already emitted the deleted events |
| | No way to distinguish "never happened" from "happened and was undone" |

**Rejected.** The mistake is part of the history. Compliance and
debugging require seeing both the wrong state and the correction.

### A3. Compensating Writes with Independent Transactions per Entity

Compensate each entity with its own transaction, not a shared one.

| Pros | Cons |
|------|------|
| Simpler to implement | Violates FEAT-023 cross-entity atomicity |
| Conflicts on entity N don't block entities 1..N-1 | Partial rollback leaves the database in an inconsistent middle state |
| | No single `rollback_transaction_id` to group by in the audit log |

**Rejected.** FEAT-023's acceptance criterion for transaction-level
rollback explicitly requires atomic cross-entity reversal. Partial
rollback is a worse state than no rollback.

### A4. Compensating Writes via a Shared Transaction (Selected)

Group all compensating writes for a rollback into one new transaction
with a shared `rollback_source_transaction_id`.

| Pros | Cons |
|------|------|
| Reuses ADR-004 commit protocol unchanged | Large point-in-time rollbacks can exceed the 100-op transaction limit (FEAT-008) — must be chunked by the caller |
| Atomic all-or-nothing by construction | |
| Single OCC abort path; single audit group; single CDC event burst | |
| "Rollback of a rollback" is just another rollback | |
| Dry-run uses the same planner as commit | |

**Selected.** Reuses every piece of machinery the database already has,
introduces no new concurrency primitives, and preserves every invariant
that ADR-004 and FEAT-003 depend on.

The 100-op limit is a known constraint: a point-in-time rollback
affecting more than 100 entities must be split into multiple
compensating transactions by the caller. Each sub-transaction is
atomic; the overall point-in-time rollback is *not* atomic across
sub-transactions. A larger-limit rollback-specific transaction budget
is a P1 follow-on.

## Consequences

| Type | Impact |
|------|--------|
| Positive | Rollback inherits OCC, audit, and CDC behavior for free. No new storage-layer code paths. Audit log remains strictly append-only. Rollback of rollback works without special cases. Dry-run and commit share planning logic, so preview accuracy is high. |
| Positive | FEAT-021 CDC consumers see compensating writes as ordinary mutations with `op: u` (update) — downstream replicas converge correctly with no special handling. |
| Negative | Rollback cost scales with the number of compensated entities — reverting 10 000 entities requires 10 000 compensating writes and 10 000 new audit entries. Point-in-time rollback of a busy database may be expensive. |
| Negative | 100-op transaction limit forces caller-side chunking for large point-in-time rollbacks; chunked rollbacks lose cross-chunk atomicity. |
| Negative | OCC conflicts must be resolved by the caller. There is no "force" rollback mode in V1 — if the caller truly wants to overwrite a concurrent edit, they must read current state, merge, and retry (same as any OCC flow). |
| Negative | V1 transaction-level rollback rejects source transactions that include entity creates or deletes (§7a). Callers must use per-entity compensation or point-in-time rollback as a workaround until the follow-on bead lands. |
| Neutral | Point-in-time rollback is best-effort, not atomic (§7b). The `RollbackCollectionResponse.partial_failures` field carries per-entity failures; callers choose whether to retry, escalate, or accept the partial result. |
| Neutral | `AuditEntry` gains two optional fields (`rollback_source_audit_id`, `rollback_source_transaction_id`) and `MutationType` gains `EntityRollback`. Both are additive and do not affect existing entries. |

## Implementation Notes

- New variant: `axon_audit::entry::MutationType::EntityRollback` with
  dot-notation `entity.rollback`.
- New fields on `AuditEntry`: `rollback_source_audit_id: Option<u64>`,
  `rollback_source_transaction_id: Option<TransactionId>`. Both
  `#[serde(skip_serializing_if = "Option::is_none")]` to keep existing
  entry JSON backwards-compatible.
- New field on `Transaction` (in `axon-api/src/transaction.rs`):
  `rollback_source_transaction_id: Option<TransactionId>`.
- New handler entry points in `axon-api/src/handler.rs`:
  `rollback_transaction(req: RollbackTransactionRequest) -> RollbackSummary`,
  `rollback_point_in_time(req: PointInTimeRollbackRequest) -> RollbackSummary`,
  each with a sibling `..._dry_run` variant sharing the same planner.
- Dry-run and commit share a `plan_rollback` function that returns a
  `RollbackPlan { writes, conflicts }`. Commit feeds `plan.writes` into
  a `Transaction` and commits it; dry-run returns the plan unchanged.
- The existing `revert_entity_to_audit_entry` (US-008) is retained as
  the single-entity special case and emits `EntityRevert`. The new
  transaction-level entry points emit `EntityRollback`. A P1 follow-on
  can unify them if the distinction proves unhelpful in practice.
- `Handler::rollback_transaction` returns `InvalidOperation` for source
  transactions containing entity creates or deletes (§7a). The rejection
  is implemented before any storage transaction is opened, so no partial
  state is written. A follow-on bead tracks full create/delete support.
- `Handler::rollback_collection` records per-entity failures in the
  response's `partial_failures` field (§7b). Plan-time drift, commit-time
  OCC conflicts, and concurrent re-appearance of deleted entities all
  route into this field rather than aborting the whole operation. The
  compensating `transaction_id` groups whichever writes did land so the
  partial result is inspectable via `axon audit list`.
- Point-in-time rollback uses an audit log scan by timestamp cutoff
  (FEAT-003 query API). The scan cost is proportional to mutations
  *after* the cutoff, satisfying FEAT-023 NFR ("scales with mutations
  reversed, not total audit log size"). Storing an audit timestamp
  index is assumed; if absent, it's a blocker tracked under FEAT-003.
- Transactions exceeding the 100-op limit return `InvalidArgument` at
  the planner stage — dry-run will surface this before commit is
  attempted.

## Validation

| Success Metric | Review Trigger |
|----------------|----------------|
| All FEAT-023 acceptance criteria pass in `tests/FEAT-023/` integration tests | Any acceptance test failure |
| INV-003 (audit completeness) holds across arbitrary sequences of rollback and rollback-of-rollback | Any audit gap or silent audit rewrite |
| Rollback OCC conflicts are reported as `ConflictingVersion` with the same structure as normal mutation conflicts | Any divergence between normal and rollback conflict shapes |
| CDC consumers (FEAT-021) receive exactly one `op: u` event per compensating write | Any missing or extra CDC event |
| Dry-run produces no audit entries and no storage writes under load | Any audit/storage write observed during dry-run |
| Single-entity rollback p99 < 10ms (FEAT-023 NFR) | Benchmark regression |

## References

- [FEAT-023: Rollback and Recovery](../../01-frame/features/FEAT-023-rollback-recovery.md)
- [FEAT-003: Audit Log](../../01-frame/features/FEAT-003-audit-log.md)
- [FEAT-008: ACID Transactions](../../01-frame/features/FEAT-008-acid-transactions.md)
- [ADR-004: Transaction Model — Optimistic Concurrency Control](./ADR-004-transaction-model.md)
- [ADR-014: Change Feeds — Debezium CDC](./ADR-014-change-feeds-debezium-cdc.md)
- [Audit log implementation](../../../crates/axon-audit/src/entry.rs)
- [Transaction implementation](../../../crates/axon-api/src/transaction.rs)
- [Existing single-entity revert](../../../crates/axon-api/src/handler.rs)
