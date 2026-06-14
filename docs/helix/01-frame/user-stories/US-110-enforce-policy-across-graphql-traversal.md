---
ddx:
  id: US-110
  review:
    self_hash: f2a2ee8a8785b46b3f3ac946bf6b63ce4056fb536d691c3fcd0306b9355ff681
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# US-110: Enforce Policy Across GraphQL Traversal

**Feature**: FEAT-015 — GraphQL Query Layer
**Feature Requirements**: GQL-06, GQL-09, GQL-16
**PRD Requirements**: FR-13, FR-20
**Priority**: P0
**Status**: Draft

## Story

**As** Ava, an application developer exposing GraphQL to her users and agents
**I want** GraphQL queries to enforce row and field policies across nested
relationships and pagination
**So that** direct GraphQL access cannot leak hidden business records

## Context

Query and redaction behavior leaking hidden data through counts, traversal,
nullability, or pagination is a named P0 product risk (PRD FR-13). This story
exercises FEAT-015's policy-safe resolution requirements (GQL-06, GQL-09,
GQL-16) on top of FEAT-029's compiled policies.

## Walkthrough

1. A caller whose policy hides certain rows and redacts certain fields runs
   a point read, a paginated list, and a nested relationship query.
2. The denied point read resolves to null exactly as a missing entity would.
3. List edges, cursors, and totals are computed only over visible rows.
4. Redacted fields resolve to null; hidden relationship targets are omitted
   without distinguishable errors.
5. The caller asks for a policy explanation and gets the reasoning without
   gaining any additional data access.

## Acceptance Criteria

- [ ] **US-110-AC1** — Given a row hidden from the caller, when the caller
  performs a point read on it, then the result is null and indistinguishable
  from a nonexistent entity.
- [ ] **US-110-AC2** — Given a paginated query over a collection with hidden
  rows, when edges and total count are computed, then they reflect only rows
  visible after FEAT-029 row filters.
- [ ] **US-110-AC3** — Given a field redactable by policy, when the caller
  selects it, then the generated type permits null and the field resolves to
  null when denied.
- [ ] **US-110-AC4** — Given nested relationship fields with hidden targets,
  when the caller traverses them, then hidden targets are omitted and
  relationship counts do not reveal them.
- [ ] **US-110-AC5** — Given a policy-explanation query, when the caller
  requests an explanation for a denied operation, then the explanation is
  returned without weakening enforcement on the real operation.

## Edge Cases

- **Caller with full visibility**: Results are identical to a
  policy-disabled baseline (no over-filtering).
- **Policy change mid-query**: The in-flight query uses the policy snapshot
  active at execution start; no mixed-policy page.
- **Cursor reuse across policy change**: A cursor obtained under an older
  policy cannot expose rows hidden under the current policy.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Hidden point read | US-110-AC1 | Row hidden for caller | Point read by ID | `null`; same shape as missing ID |
| Safe totals | US-110-AC2 | 10 rows, 4 hidden | Paginated list with total count | 6 edges total, total count 6 |
| Redacted field | US-110-AC3 | `salary` redacted | Select `salary` | `null`, no error |
| Hidden traversal targets | US-110-AC4 | 3 links, 1 target hidden | Nested relationship query with count | 2 targets, count 2 |
| Explanation safety | US-110-AC5 | Denied write | Policy explanation query | Rule names + denied paths; operation still denied |

## Dependencies

- **Stories**: US-048
- **Feature Spec**: FEAT-015
- **Feature Requirements**: GQL-06, GQL-09, GQL-16
- **PRD Requirements**: FR-13, FR-20
- **External**: CONTRACT-002 (GraphQL surface), CONTRACT-004 (policy
  grammar), FEAT-029 (policy compilation and enforcement)

## Out of Scope

- Policy authoring, fixtures, and dry-runs (FEAT-029 stories).
- Aggregate leak safety (FEAT-018 AGG-09 and its stories).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
