---
ddx:
  id: US-022
---

# US-022: Snapshot Isolation

**Feature**: FEAT-008 — ACID Transactions
**Feature Requirements**: TXN-05, TXN-06
**PRD Requirements**: FR-5, FR-6
**Priority**: P0
**Status**: Draft

## Story

**As** Ava, an agent application developer
**I want** my transactions to read from a consistent snapshot
**So that** concurrent transactions never observe uncommitted or partially-applied state

## Context

Atomicity without isolation still corrupts reasoning: a transaction that
sees half of another transaction's writes makes bad decisions. This story
exercises TXN-05 (snapshot reads, inspectable isolation level) and TXN-06
(first-writer-wins on overlapping write sets). The known V1 limit — write
skew is not prevented under snapshot isolation — is a feature-level
constraint (FEAT-008 Constraints), not an acceptance criterion here.

## Walkthrough

1. Transaction T1 opens and reads entities X and Y.
2. Transaction T2 commits updates to X and Y while T1 is open.
3. T1's subsequent reads still see its original snapshot — never a mix of old Y and new X.
4. T1 attempts to write X; the overlap with T2's committed write set aborts T1 with a conflict.

## Acceptance Criteria

- [ ] **US-022-AC1** — Given an open transaction, when another transaction commits mid-flight, then the open transaction's reads continue to reflect its snapshot (no dirty or non-repeatable reads, no torn multi-record view).
- [ ] **US-022-AC2** — Given two concurrent transactions writing to the same entity, when both attempt to commit, then exactly one commits and the other aborts with a version conflict.
- [ ] **US-022-AC3** — Given a transaction, when its isolation level is inspected, then the effective level is reported (snapshot isolation by default; read-committed where explicitly opted in).

## Edge Cases

- **Read of a non-existent entity inside a transaction**: returns not-found without aborting the transaction (conditional logic allowed).
- **Write skew shape** (T1 reads A writes B; T2 reads B writes A): both may commit in V1 — documented constraint, deferred to P1 serializable isolation; applications must guard such invariants.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Snapshot stability | US-022-AC1 | T1 open; T2 commits X', Y' | T1 re-reads X, Y | Original snapshot values |
| Write-write conflict | US-022-AC2 | T1 and T2 both write X | Commit both | One commits; one version-conflict abort |
| Level inspection | US-022-AC3 | Default transaction | Inspect isolation level | Snapshot isolation reported |
| Write skew (documented gap) | — | T1 reads A writes B; T2 reads B writes A | Commit both | Both commit in V1; recorded as constraint, not failure |

## Dependencies

- **Stories**: US-020 (transaction basis), US-021 (conflict behavior)
- **Feature Spec**: FEAT-008
- **Feature Requirements**: TXN-05, TXN-06
- **PRD Requirements**: FR-5, FR-6
- **External**: CONTRACT-001 (transaction protocol, conflict shapes)

## Out of Scope

- Serializable isolation and write-skew prevention (P1; FEAT-008 Constraints).
- Read-uncommitted in any form — never offered.

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
