---
ddx:
  id: FEAT-023
  depends_on:
    - helix.prd
  review:
    self_hash: 24416c13b9a48e864ae43e3967c63d2711763c745905850dbb4f03768ffc7949
    deps:
      helix.prd: dff98156a6cc934f406611b78b513892d85cee1bd7b4c011f045146fcdfd23e1
    reviewed_at: "2026-06-14T04:39:42Z"
---
# Feature Specification: FEAT-023 — Rollback and Recovery

**Feature ID**: FEAT-023
**Status**: draft
**Priority**: P1
**Owner**: Core Team
**Created**: 2026-04-06
**Updated**: 2026-06-10
**Requirement Prefix**: RBK
**Covered PRD Subsystem(s)**: Audit, Change Capture, and Repair; API and Deployment Surfaces
**Covered PRD Requirements**: FR-19; FR-30 (repair-plan and rollback dry-run views; audit views are shared with FEAT-003 and operator-surface features)
**Cross-Subsystem Rationale**: The repair workflow IS the feature: rollback turns audit history (repair subsystem) into operator-facing dry-run/commit flows across CLI, GraphQL, SDK, and UI surfaces (surface subsystem). Splitting the compensating-write engine from its repair views would leave neither shippable on its own.

## Overview

Structured rollback capabilities powered by the audit log. Supports
point-in-time, entity-level, and transaction-level rollback with dry-run
preview, implementing PRD FR-19 and the repair-plan portion of FR-30.
Rollback transforms the audit log from a read-only history into an
actionable recovery mechanism.

Rollback is also an operator interface: dry-runs must use diff/log/blame-style
output so humans can understand what changed, who or what changed it, why it
was allowed or denied, and what compensating write Axon would apply.

## Ideal Future State

When an agent or human damages business state — a bad batch update, a rogue
automation, a misconfigured workflow — an operator recovers in minutes
without hand-crafting corrective writes. They identify the bad entity,
transaction, or time window; run a dry-run that shows exactly what would
change, what conflicts exist, and the full provenance of the damage; and
then commit a compensating transaction that is itself governed, approved
where policy requires, and audited with references back to the mutations it
repairs. History is never rewritten: recovery is always a new, explainable
write.

## Problem Statement

- **Current situation**: The audit log (FEAT-003) captures every mutation
  with repair-grade context, but there is no structured way to use that
  history to undo changes.
- **Pain points**: Recovery from a mistake requires manual intervention —
  reading the audit log, reconstructing previous state by hand, and
  applying corrective writes — which is slow, error-prone, and itself
  ungoverned.
- **Desired outcome**: Point-in-time, entity-level, and transaction-level
  rollback are first-class operations with dry-run preview, conflict
  detection, governed commits, and full audit linkage.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Entity-level rollback | Restore one corrupted entity to a known-good version | Revert by version or audit entry ID as a new OCC write |
| Transaction-level rollback | Undo everything one bad transaction did | Atomic compensating transaction for all touched entities and links |
| Point-in-time rollback | Undo everything after a timestamp in a collection/database | Inverse transaction over all mutations after the cutoff |
| Dry-run and repair plans | What would this rollback change, and is it safe? | Side-effect-free preview with diffs, conflicts, and provenance metadata |
| Governed rollback surfaces | How do operators and agents invoke recovery? | Rollback exposed as ordinary governed writes across CLI, GraphQL, SDK, and MCP |

## Requirements

### Functional Requirements by Area

#### Entity-Level Rollback

- **RBK-01**. Axon must revert a specific entity to a previous version,
  identified by version number or audit entry ID.
- **RBK-02**. Entity-level rollback must be applied as a new write at the
  current version under standard optimistic concurrency control — never a
  version rewrite. If the entity has been modified since the target
  version, the rollback may conflict and require resolution.
- **RBK-03**. Entity-level rollback must be audited using the entity
  revert operation defined in the audit operation taxonomy of
  [CONTRACT-005 — audit record](../../02-design/contracts/CONTRACT-005-audit-record.md).

#### Transaction-Level Rollback

- **RBK-04**. Axon must undo a specific transaction by ID, reverting all
  entities and links it touched to their pre-transaction state via a new
  compensating transaction.
- **RBK-05**. Transaction-level rollback must maintain cross-entity
  consistency: the compensating transaction commits atomically or fails
  as a unit if any compensating operation conflicts.
- **RBK-06**. Transaction-level rollback must be audited using the
  transaction rollback operation reserved in CONTRACT-005.

#### Point-in-Time Rollback

- **RBK-07**. Axon must undo all changes to a collection (or database)
  after a specified timestamp by producing a new transaction that applies
  the inverse of all mutations after the cutoff.
- **RBK-08**. Point-in-time rollback must be audited using the collection
  rollback operation reserved in CONTRACT-005, including references to
  the original mutations it compensates for.

#### Dry-Run and Repair Plans

- **RBK-09**. All rollback operations must support a dry-run mode that
  returns the set of changes that would be applied without committing
  them, acquiring write locks, or modifying any state.
- **RBK-10**. Dry-run must detect conflicts: it identifies entities
  modified since the rollback target that would produce OCC conflicts on
  commit.
- **RBK-11**. Dry-run must return repair-plan metadata: original audit
  IDs, original transaction IDs, actor and delegated authority, tool/API
  origin, policy decision, approval decision, before/after values subject
  to caller redaction, and the compensating operations that would be
  submitted.
- **RBK-12**. Dry-run and audit-inspection output must use
  diff/log/blame-style presentation so operators can see what changed,
  who or what changed it, why it was allowed or denied, and what the
  repair would do (PRD FR-30).

#### Governed Rollback Surfaces

- **RBK-13**. Rollback dry-run and commit must be exposed for entity,
  transaction, and point-in-time recovery across the public surfaces:
  HTTP rollback/audit endpoints per
  [CONTRACT-001](../../02-design/contracts/CONTRACT-001-http-api-surface.md),
  GraphQL fields per
  [CONTRACT-002](../../02-design/contracts/CONTRACT-002-graphql-surface.md),
  CLI rollback and audit diff/blame commands per
  [CONTRACT-008](../../02-design/contracts/CONTRACT-008-cli-and-config.md),
  and SDK helpers per
  [CONTRACT-009](../../02-design/contracts/CONTRACT-009-sdk-surface.md).
  All surfaces submit the same handler operations.
- **RBK-14**. MCP may expose read-only rollback dry-run tools for agent
  diagnosis; committing rollback remains subject to the same policy and
  approval rules as any other write.
- **RBK-15**. Rollback writes are ordinary governed mutations: FEAT-029
  policy applies, FEAT-030 preview/approval applies when policy returns
  `needs_approval`, and the rollback mutation itself is audited with
  references to the original audit IDs it compensates.
- **RBK-16**. Rollback dry-run and commit responses must expose stable
  audit references that SDK, CLI, GraphQL, and operator UI clients can
  preserve in machine-readable output.

### Non-Functional Requirements

- **Performance**: rollback of a single entity must meet the same latency
  target as a normal write (< 10 ms p99).
- **Scalability**: point-in-time rollback cost must scale with the number
  of mutations being reversed, not the total audit log size.
- **Safety**: dry-run must not acquire write locks or modify any state.

## User Stories

- [US-095 — Preview Recovery Before Commit](../user-stories/US-095-preview-recovery-before-commit.md)
- [US-096 — Revert One Entity Safely](../user-stories/US-096-revert-one-entity-safely.md)
- [US-097 — Undo a Bad Transaction or Time Window](../user-stories/US-097-undo-a-bad-transaction-or-time-window.md)

## Edge Cases and Error Handling

- **Target entity modified since rollback target**: commit produces an
  OCC conflict; current state is left unchanged and the conflict is
  reported with current-version context.
- **Rollback of a rollback**: re-applies the later state as another new
  write with its own audit entry; history accumulates, never rewrites.
- **Reverting past a delete**: compensating a deletion recreates the
  record as a new governed write; compensating a creation deletes it.
  Both directions carry the original audit references.
- **Empty rollback window**: a point-in-time rollback with no mutations
  after the cutoff is a no-op dry-run result and commits nothing.
- **Partially redacted history**: callers whose policy redacts fields see
  redacted before/after values in repair plans; redaction never silently
  alters what a commit would apply.
- **Schema changed since target version**: a revert whose restored
  payload no longer validates against the active schema is rejected by
  normal write validation; dry-run surfaces this before commit.
- **Approval-routed rollback**: when policy classifies the rollback as
  `needs_approval`, commit follows the FEAT-030 intent flow; a stale
  intent (state moved again) requires a fresh dry-run.

## Success Metrics

- An operator can take a known bad transaction from discovery to
  committed, audited recovery using only rollback dry-run and commit
  flows — no hand-written corrective mutations — in the reference repair
  walkthrough.
- 100% of rollback commits carry audit references to the original
  mutations they compensate.
- 0 state mutations or write locks observable from dry-run operations
  under concurrent load.
- 100% of rollback commits pass through policy evaluation, and
  approval-routed rollbacks cannot commit without an approved intent.

## Constraints and Assumptions

- Rollback is reconstruction from audit history: it assumes FEAT-003
  repair-grade audit coverage (before/after state, versions, transaction
  IDs) for every mutation it may need to reverse.
- Recovery never rewrites history; every rollback is a forward-moving
  governed write under standard OCC.
- Rollback operations and their audit literals are extend-only and
  governed by CONTRACT-005; surfaces are governed by CONTRACT-001/002/
  008/009.
- Redaction applies to repair-plan visibility, not to the correctness of
  the compensating write itself.

## Dependencies

- **Other features**: FEAT-003 (Audit Log — rollback reads audit history
  to reconstruct previous state), FEAT-008 (ACID Transactions — rollback
  operations are themselves atomic transactions), FEAT-005 (API Surface)
  and FEAT-015 (GraphQL) — rollback dry-run and commit ride the same
  public interface contracts as other governed writes, FEAT-029
  (Access Control — rollback is authorized as an ordinary write against
  current policy), FEAT-030 (Mutation Intents — approval-routed rollback
  commits use the same preview, intent, approval, and stale-binding
  rules as other risky mutations).
- **External services**: None. Normative surfaces live in CONTRACT-001
  (rollback/audit endpoints), CONTRACT-002 (GraphQL), CONTRACT-005
  (audit operation taxonomy), CONTRACT-008 (CLI commands), and
  CONTRACT-009 (SDK helpers).
- **PRD requirements**: FR-19 (P1) — dry-run and commit flows for entity,
  transaction, and point-in-time rollback; FR-30 (P1) — diff/log/blame
  style repair views.

## Out of Scope

- **Graph-wide arbitrary point-in-time rollback**: rolling the entire
  entity graph back to an arbitrary instant across all collections and
  databases — also explicitly excluded by FEAT-030's intent scope.
  Point-in-time rollback here is scoped to a selected collection or
  database window.
- **Backup/restore and disaster recovery**: rollback compensates logical
  mutations from audit history; it is not a physical backup, snapshot, or
  PITR-restore mechanism.
- **Audit-log rewriting or pruning**: rollback never deletes or edits
  audit entries; retention and erasure are separate compliance design.
- **Schema rollback**: reverting schema versions is FEAT-017 schema
  evolution territory; FEAT-023 reverts entity/link data.
- **Automatic rollback triggers**: anomaly-detection-driven automatic
  recovery is future work; V1 rollback is operator-initiated.
