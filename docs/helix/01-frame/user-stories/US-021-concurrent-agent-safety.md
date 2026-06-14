---
ddx:
  id: US-021
  review:
    self_hash: f141e5e4ea32f0d2848b6254621ba8f3df6e0027f64b1fbf1f45f4b13aa4f236
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# US-021: Concurrent Agent Safety

**Feature**: FEAT-008 — ACID Transactions
**Feature Requirements**: TXN-06, TXN-07, TXN-08
**PRD Requirements**: FR-6
**Priority**: P0
**Status**: Draft

## Story

**As** Ava, an agent application developer running multiple agents on shared state
**I want** an agent's write to fail loudly when another agent changed the record first
**So that** no agent ever silently overwrites another agent's work

## Context

Concurrent agents are the norm, not the exception. This story exercises the
no-lost-updates guarantee: first-writer-wins (TXN-06), conflict responses
with current state (TXN-07), and consistency with FEAT-004's single-entity
OCC (TXN-08). The merge-and-retry loop must be fully programmatic.

## Walkthrough

1. Agent A reads an entity at version 5; agent B reads the same entity.
2. Agent B commits an update; the entity moves to version 6.
3. Agent A submits its update expecting version 5; the system rejects it with a version conflict carrying the version-6 state.
4. Agent A merges the current state with its intent and retries with version 6; the update commits.

## Acceptance Criteria

- [ ] **US-021-AC1** — Given agent B updated the entity to version 6 after agent A read version 5, when agent A submits an update expecting version 5, then the write fails with a version conflict and no change is applied.
- [ ] **US-021-AC2** — Given that conflict, when agent A inspects the response, then it contains the current committed state written by agent B (per CONTRACT-001).
- [ ] **US-021-AC3** — Given agent A re-reads or uses the returned state, when it retries with the current version, then the update commits.
- [ ] **US-021-AC4** — Given no concurrent writer touched the entity, when an agent updates with its read version, then the write succeeds on the first attempt.

## Edge Cases

- **Both agents conflict-and-retry simultaneously**: exactly one retry commits per round; the loop converges (first-writer-wins each round).
- **Conflict on delete**: deletes carry version expectations and conflict identically to updates.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Lost-update prevention | US-021-AC1 | Entity v5 read by A and B; B commits v6 | A updates expecting v5 | Version conflict; entity still B's state |
| Current state in conflict | US-021-AC2 | Same | Inspect conflict detail | Contains v6 state |
| Merge and retry | US-021-AC3 | A holds conflict response | Retry expecting v6 | Commit; entity v7 |
| Uncontended | US-021-AC4 | Entity v5, single writer | Update expecting v5 | Commit; v6 |

## Dependencies

- **Stories**: US-010 (single-entity OCC semantics, FEAT-004)
- **Feature Spec**: FEAT-008
- **Feature Requirements**: TXN-06, TXN-07, TXN-08
- **PRD Requirements**: FR-6
- **External**: CONTRACT-001 (conflict status/code/detail shapes)

## Out of Scope

- Automatic server-side merge strategies — merging is the caller's decision.
- Cross-record invariant protection under disjoint read/write sets (write skew; see FEAT-008 Constraints, P1).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
