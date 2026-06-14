---
ddx:
  id: US-107
  review:
    self_hash: 86e4772b09d24e57249e46a20c66c5ffaaa947a17476d13750caaf6c3d8920c1
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# US-107: Prevent Stale Approval Execution

**Feature**: FEAT-030 — Mutation Intents and Approval
**Feature Requirements**: INT-05, INT-06, INT-07, INT-12, INT-13, INT-14, INT-16
**PRD Requirements**: FR-8
**Priority**: P0
**Status**: Draft

## Story

**As a** compliance reviewer (Wei, business workflow builder, acting as
approver of record)
**I want** approved mutations to execute only against the reviewed state
**So that** an approval cannot be reused for a different write

## Context

The TOCTOU hazard is the core safety claim of mutation intents: a human
approves a diff for version 5 and must never accidentally commit a different
diff against version 7. This story exercises version/hash binding (INT-05),
expiry and single-use (INT-06), re-authorization (INT-07), commit-time
revalidation with stale/mismatch outcomes (INT-12..INT-14), and surface
parity for those outcomes (INT-16). This is the PRD approval-safety metric
(100% stale-intent rejection).

## Walkthrough

1. An agent previews an invoice update at version 5; Wei approves the
   intent.
2. Before commit, another transaction advances the invoice to version 6.
3. The agent commits the approved intent; Axon revalidates the bindings,
   detects the changed pre-image, and fails with a stale outcome naming the
   stale dimension.
4. Nothing commits; the agent previews again against version 6 and the
   review cycle repeats against the true current state.

## Acceptance Criteria

- [ ] **US-107-AC1** — Given an approved intent bound to a pre-image
  version, when the target entity version changes before commit, then
  commit fails with a stale outcome naming the pre-image dimension.
- [ ] **US-107-AC2** — Given an approved intent, when the policy version
  changes before commit, then commit fails with a stale outcome naming the
  policy dimension.
- [ ] **US-107-AC3** — Given an intent, when commit is attempted with an
  operation differing from the bound operation hash, then commit fails as a
  mismatch.
- [ ] **US-107-AC4** — Given a stale or mismatched intent over a
  multi-entity transaction, when commit fails, then no operation partially
  commits — one stale entity invalidates the whole intent.
- [ ] **US-107-AC5** — Given a committed intent, when the same token is
  used again, then it is rejected (single-use); given an expired intent,
  when commit or approval is attempted, then it is rejected.
- [ ] **US-107-AC6** — Given the SDK commit helper and GraphQL commit
  field, when exercised for success, stale, mismatch, and authorization
  outcomes, then their behavior and decision vocabulary match
  (CONTRACT-009 vs CONTRACT-002).

## Edge Cases

- **Grant or subject change**: revoking the caller's grant or changing
  subject binding between preview and commit fails commit-time
  re-authorization (INT-07), independent of staleness.
- **Schema version change**: a schema migration between preview and commit
  is a stale dimension like policy and pre-image.
- **Approver loses role after approving**: the approval stands as a
  historical record, but commit still re-checks the committer's current
  authorization envelope.
- **Race between two commits of one intent**: exactly one wins; the other
  observes the token as already committed.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Entity moved | US-107-AC1 | Intent bound to invoice v5; invoice now v6 | Commit intent | Stale failure naming pre-image dimension; no mutation |
| Policy moved | US-107-AC2 | Policy version bumped after approval | Commit intent | Stale failure naming policy dimension |
| Hash mismatch | US-107-AC3 | Commit payload differs from previewed operation | Commit | Mismatch failure; no mutation |
| All-or-nothing | US-107-AC4 | 3-entity transaction; 1 pre-image stale | Commit | Whole commit fails; 0 of 3 applied |
| Replay | US-107-AC5 | Intent already committed | Commit same token again | Rejected as already committed |

## Dependencies

- **Stories**: US-105 (binding established at preview), US-106 (approval
  precedes commit for routed writes).
- **Feature Spec**: FEAT-030
- **Feature Requirements**: INT-05, INT-06, INT-07, INT-12, INT-13, INT-14, INT-16
- **PRD Requirements**: FR-8
- **External**: ADR-019 (commit revalidation rules), CONTRACT-002 (GraphQL
  commit semantics), CONTRACT-009 (SDK parity), FEAT-017 (schema versions),
  FEAT-029 (policy versions)

## Out of Scope

- Conflict resolution guidance beyond "preview again".
- Optimistic-concurrency semantics of ordinary direct writes (FEAT-008).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
