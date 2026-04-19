---
dun:
  id: FEAT-023
  depends_on:
    - helix.prd
    - FEAT-003
    - FEAT-008
---
# Feature Specification: FEAT-023 - Rollback and Recovery

**Feature ID**: FEAT-023
**Status**: Draft
**Priority**: P1
**Owner**: Core Team
**Created**: 2026-04-06
**Updated**: 2026-04-06

## Overview

Structured rollback capabilities powered by the audit log. Supports
point-in-time, entity-level, and transaction-level rollback with dry-run
preview. Transforms the audit log from a read-only history into an
actionable recovery mechanism.

## Problem Statement

The audit log (FEAT-003) captures every mutation, but there is no
structured way to use that history to undo changes. When an agent or
human makes a mistake — a bad batch update, a rogue automation, a
misconfigured workflow — recovery requires manual intervention: reading
the audit log, reconstructing the previous state, and applying corrective
writes. This should be a first-class operation.

## Requirements

### Functional Requirements

#### Point-in-Time Rollback

- Undo all changes to a collection (or database) after a specified
  timestamp.
- Produces a new transaction that applies the inverse of all mutations
  after the cutoff.
- The rollback itself is audited using the dot-namespaced audit
  taxonomy from FEAT-003. Point-in-time rollback reserves
  `operation: collection.rollback` and includes references to the
  original mutations it compensates for.

#### Entity-Level Rollback

- Revert a specific entity to a previous version (identified by version
  number or audit entry ID).
- Entity-level rollback is audited as `operation: entity.revert`, which
  is the shipped V1 rollback operation defined by FEAT-003 and emitted
  by the current implementation.
- Standard OCC applies — the rollback is a write at the current version,
  not a version rewrite.
- If the entity has been modified since the target version by other
  transactions, the rollback may conflict and require resolution.

#### Transaction-Level Rollback

- Undo a specific transaction by ID — reverting all entities and links
  it touched to their pre-transaction state.
- Produces a new compensating transaction.
- Transaction-level rollback reserves `operation: transaction.rollback`
  when that workflow is implemented.
- Cross-entity consistency is maintained: all entities in the original
  transaction are rolled back atomically.

#### Dry-Run Rollback

- All rollback operations support a `dry_run` flag.
- Dry run returns the set of changes that would be applied, without
  committing them.
- Includes conflict detection: identifies entities that have been
  modified since the rollback target, which would produce OCC conflicts.

### Non-Functional Requirements

- Rollback of a single entity must meet the same latency targets as a
  normal write (<10ms p99).
- Point-in-time rollback performance scales with the number of mutations
  being reversed, not the total audit log size.
- Dry-run must not acquire write locks or modify any state.

### Dependencies

- FEAT-003 (Audit Log) — rollback reads the audit log to reconstruct
  previous state.
- FEAT-008 (ACID Transactions) — rollback operations are themselves
  atomic transactions.

## User Stories

### Story US-095: Preview Recovery Before Commit [FEAT-023]

**As an** operator about to undo an agent mistake
**I want** rollback dry-run output before any mutation occurs
**So that** I can verify exactly what will change and whether conflicts
exist

**Acceptance Criteria:**
- [x] Entity rollback preview shows a diff without mutating the entity.
  E2E: `ui/tests/e2e/wave2-rollback.spec.ts`
- [x] The preview identifies the selected target version from audit
  history. E2E: `ui/tests/e2e/wave2-rollback.spec.ts`
- [ ] Dry-run API responses include conflict information when the current
  entity version has changed since the rollback target. Planned E2E:
  `crates/axon-server/tests/rollback_recovery_test.rs`
- [ ] Point-in-time dry-run lists every entity and link that would change
  without committing. Planned E2E:
  `crates/axon-server/tests/rollback_recovery_test.rs`
- [ ] Transaction rollback dry-run identifies all original transaction
  mutations and their compensating operations. Planned E2E:
  `crates/axon-server/tests/rollback_recovery_test.rs`

### Story US-096: Revert One Entity Safely [FEAT-023]

**As a** developer repairing one corrupted entity
**I want** to restore that entity to a previous version from the audit log
**So that** recovery is precise, audited, and does not rewrite history

**Acceptance Criteria:**
- [x] Entity-level rollback restores a specific entity to a prior version.
  E2E: `ui/tests/e2e/wave2-rollback.spec.ts`,
  `ui/tests/e2e/audit-route.spec.ts`
- [x] The rollback is applied as a new mutation rather than by rewriting
  old versions. E2E: `ui/tests/e2e/wave2-rollback.spec.ts`
- [ ] Entity-level rollback audit entries use `operation: entity.revert`.
  Planned E2E: `crates/axon-server/tests/rollback_recovery_test.rs`
- [ ] OCC conflicts during entity rollback are reported clearly and leave
  current state unchanged. Planned E2E:
  `crates/axon-server/tests/rollback_recovery_test.rs`
- [ ] Rollback of a rollback re-applies the later state and creates its
  own audit entry. Planned E2E:
  `crates/axon-server/tests/rollback_recovery_test.rs`

### Story US-097: Undo a Bad Transaction or Time Window [FEAT-023]

**As an** operator recovering from a bad automation run
**I want** transaction-level and point-in-time rollback
**So that** I can recover a coherent set of related mutations atomically

**Acceptance Criteria:**
- [ ] Transaction-level rollback reverses all changes from a specific
  transaction atomically. Planned E2E:
  `crates/axon-server/tests/rollback_recovery_test.rs`
- [ ] Transaction rollback fails as a unit if any compensating operation
  conflicts. Planned E2E: `crates/axon-server/tests/rollback_recovery_test.rs`
- [ ] Point-in-time rollback reverts all mutations after a given timestamp
  for the selected collection or database. Planned E2E:
  `crates/axon-server/tests/rollback_recovery_test.rs`
- [ ] Point-in-time rollback reserves `operation: collection.rollback` and
  transaction-level rollback reserves `operation: transaction.rollback`.
  Planned E2E: `crates/axon-server/tests/rollback_recovery_test.rs`
- [ ] All rollback operations are themselves audited with references to
  the original audit ids they compensate. Planned E2E:
  `crates/axon-server/tests/rollback_recovery_test.rs`
