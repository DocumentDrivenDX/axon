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
- The rollback itself is audited (creates audit entries with
  `operation: rollback` and a reference to the original mutations).

#### Entity-Level Rollback

- Revert a specific entity to a previous version (identified by version
  number or audit entry ID).
- Standard OCC applies — the rollback is a write at the current version,
  not a version rewrite.
- If the entity has been modified since the target version by other
  transactions, the rollback may conflict and require resolution.

#### Transaction-Level Rollback

- Undo a specific transaction by ID — reverting all entities and links
  it touched to their pre-transaction state.
- Produces a new compensating transaction.
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

## Acceptance Criteria

- [ ] Point-in-time rollback reverts all mutations after a given
      timestamp
- [ ] Entity-level rollback restores a specific entity to a prior version
- [ ] Transaction-level rollback reverses all changes from a specific
      transaction
- [ ] Dry-run mode shows what would change without committing
- [ ] All rollback operations are themselves audited
- [ ] OCC conflicts during rollback are reported clearly
- [ ] Rollback of a rollback (re-apply) works correctly
