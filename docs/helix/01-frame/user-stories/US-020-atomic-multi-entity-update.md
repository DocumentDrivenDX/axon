---
ddx:
  id: US-020
  review:
    self_hash: 6c6870a9794e66ed3428fcd12a70e12501a591eb5ac99be831beff4af583c11a
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# US-020: Atomic Multi-Entity Update

**Feature**: FEAT-008 — ACID Transactions
**Feature Requirements**: TXN-01, TXN-02, TXN-03, TXN-07, TXN-10
**PRD Requirements**: FR-5
**Priority**: P0
**Status**: Draft

## Story

**As** Wei, a business workflow builder implementing a financial flow
**I want** to debit one account and credit another atomically
**So that** money is never lost or duplicated by partial failures

## Context

The canonical multi-record invariant: a transfer touches two accounts and a
ledger record, and a half-applied transfer is corruption. This story
exercises FEAT-008's atomic commit (TXN-01..TXN-03), structured conflict
detail (TXN-07), and audit threading (TXN-10). The transaction wire protocol
is normative in CONTRACT-001.

## Walkthrough

1. Wei submits one transaction: debit `accounts/acct-A`, credit `accounts/acct-B`, create a ledger link (surface per CONTRACT-001).
2. The system checks each operation's version expectation and schema validity.
3. All operations commit together; all audit entries share one transaction ID.
4. If any operation conflicts or fails validation, nothing commits and the response names the failing record.

## Acceptance Criteria

- [ ] **US-020-AC1** — Given a transaction debiting account A and crediting account B, when it commits, then both updates are applied; when it aborts, neither is.
- [ ] **US-020-AC2** — Given account A's version changed since it was read, when the transaction is submitted with the stale expectation, then the entire transaction aborts and account B is untouched.
- [ ] **US-020-AC3** — Given an aborted transaction, when Wei inspects the response, then it identifies which record caused the conflict and includes that record's current state (per CONTRACT-001).
- [ ] **US-020-AC4** — Given a committed transaction, when Wei queries audit history, then all the transaction's mutations carry the same transaction ID.

## Edge Cases

- **Cross-collection transaction**: operations spanning `accounts` and `ledger-entries` commit atomically (TXN-02).
- **Schema violation inside the transaction**: the whole transaction aborts with the specific validation error; no operations or audit events apply.
- **Empty transaction**: commits as a no-op with no audit entry.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Happy path | US-020-AC1 | A=100, B=50, both version 1 | Txn: A-=30, B+=30 | A=70, B=80, both committed |
| Stale abort | US-020-AC2 | A bumped to version 2 by another writer | Same txn expecting version 1 | Abort; B unchanged |
| Conflict detail | US-020-AC3 | Same as above | Inspect abort response | Names account A, includes current state |
| Audit threading | US-020-AC4 | Committed transfer txn | Query audit | Both entries share one transaction ID |

## Dependencies

- **Stories**: US-010 (single-entity OCC), US-018 (links)
- **Feature Spec**: FEAT-008
- **Feature Requirements**: TXN-01, TXN-02, TXN-03, TXN-07, TXN-10
- **PRD Requirements**: FR-5
- **External**: CONTRACT-001 (transaction endpoint, abort/conflict shapes), FEAT-003 / CONTRACT-005 (transaction-threaded audit)

## Out of Scope

- Retry-safe resubmission (US-081).
- Isolation-level guarantees between concurrent transactions (US-022).
- Approval-routed writes (FEAT-030).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
