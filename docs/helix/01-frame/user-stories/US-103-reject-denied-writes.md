---
ddx:
  id: US-103
  review:
    self_hash: 32a010db847111e36e932a3828524465da66f64b82d09488bf88ac1db1e81c13
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# US-103: Reject Denied Writes

**Feature**: FEAT-029 — Data-Layer Access Control Policies
**Feature Requirements**: ACL-13, ACL-15
**PRD Requirements**: FR-11, FR-12
**Priority**: P0
**Status**: Approved

## Story

**As an** application developer (Ava, Agent Application Developer persona)
**I want** denied writes to fail with stable policy errors
**So that** my SDK and UI can distinguish policy failures from validation and missing-record failures

## Context

Stable, machine-readable denial semantics are what make policy failures
programmable: row-write denial, field-write denial, and approval routing each
carry distinct reason codes (CONTRACT-004). Denials must also compose
correctly with transactions and idempotent replay.

## Walkthrough

1. Developer's app submits an update to a row the subject cannot mutate.
2. System rejects it with the stable forbidden envelope and row-denial
   reason.
3. The app submits a write touching a denied field.
4. System rejects it naming the denied field path.
5. The app retries the same denied transaction with its idempotency key and
   receives the same terminal response.

## Acceptance Criteria

- [ ] **US-103-AC1** — Given a row the caller cannot mutate, when an update
  is submitted, then it fails with the stable forbidden envelope and
  row-denial reason (CONTRACT-004).
- [ ] **US-103-AC2** — Given a write including a denied field, when it is
  submitted, then it fails with the forbidden envelope naming the denied
  field path.
- [ ] **US-103-AC3** — Given a transaction containing one denied operation,
  when it is committed, then the entire transaction aborts with no partial
  writes and no audit mutation entry.
- [ ] **US-103-AC4** — Given a denied idempotent transaction, when the same
  request is replayed within the idempotency TTL, then the same forbidden
  response is returned even if policy or data changed in between.

## Edge Cases

- **Lifecycle and rollback writes**: transitions and rollbacks that would
  mutate denied fields fail with the same envelope.
- **Denial vs validation**: a write that is both schema-invalid and
  policy-denied surfaces the policy decision per the evaluation order in
  CONTRACT-004.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Row denial | US-103-AC1 | Subject without row write | Update row | Forbidden, row-denial reason |
| Field denial | US-103-AC2 | Denied field `status` | Patch sets `status` | Forbidden naming `status` path |
| Transaction abort | US-103-AC3 | 3-op transaction, op 2 denied | Commit | Whole transaction aborts; no audit mutation |
| Idempotent replay | US-103-AC4 | Denied transaction + key | Replay within TTL | Identical forbidden response |

## Dependencies

- **Stories**: US-101, US-102
- **Feature Spec**: FEAT-029
- **Feature Requirements**: ACL-13, ACL-15
- **PRD Requirements**: FR-11, FR-12
- **External**: CONTRACT-004 (denial envelopes, reason codes), CONTRACT-001
  (transaction and idempotency protocol), CONTRACT-002 (GraphQL error
  extensions)

## Out of Scope

- Approval-routed (`needs_approval`) write flows (FEAT-030).
- UI presentation of denial codes (FEAT-031).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
