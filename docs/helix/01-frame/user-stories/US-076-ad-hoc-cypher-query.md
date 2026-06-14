---
ddx:
  id: US-076
  review:
    self_hash: 39310b6b094ec060810ad8bb486a8024a0dfc21daa7efdc84ce94719c324680f
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# US-076: Ad-hoc Cypher Query

**Feature**: FEAT-009 — Unified Graph Query (Cypher)
**Feature Requirements**: QRY-02, QRY-10, QRY-11, QRY-14, QRY-15, QRY-16
**PRD Requirements**: FR-3
**Priority**: P0
**Status**: Draft

## Story

**As** Ava, an agent application developer exploring and debugging her entity graph
**I want** to run an ad-hoc Cypher query at runtime
**So that** I can inspect data and answer one-off questions without re-shipping a schema

## Context

Named queries cover production reads; development and operations need a
runtime escape hatch with the same guarantees. This story exercises the
ad-hoc surface (QRY-10): identical parser/planner/policy path, stricter cost
budgets (QRY-11), plan metadata (QRY-14), and stable error codes. The field
shape, result shape, and error codes are normative in CONTRACT-007 and
CONTRACT-002.

## Walkthrough

1. Ava submits an ad-hoc query string with parameters through the ad-hoc surface (per CONTRACT-007 §Ad-hoc query field).
2. The system parses, plans, and executes under policy, returning rows with column type metadata and plan/policy metadata.
3. A typo'd label is rejected at parse time with a stable error code.
4. An expensive unindexed pattern is rejected by the cost budget before execution.

## Acceptance Criteria

- [ ] **US-076-AC1** — Given a valid ad-hoc query, when executed, then rows are returned with column type metadata and plan/index/policy metadata (per CONTRACT-007).
- [ ] **US-076-AC2** — Given a query referencing a label, property, or relationship type not in the active schema, when submitted, then parsing rejects it with the documented stable error code.
- [ ] **US-076-AC3** — Given the same subject and data, when an ad-hoc query and an equivalent named query run, then policy enforcement (row visibility, redaction, counts) is identical.
- [ ] **US-076-AC4** — Given a query whose planned cardinality exceeds the configured ad-hoc budget, when submitted, then it is rejected before execution with the documented error code.
- [ ] **US-076-AC5** — Given any ad-hoc failure (unsupported clause, unknown label, unsupported plan, policy bypass, budget, timeout), when returned, then the error carries the corresponding stable error code from CONTRACT-007 §Stable error codes.

## Edge Cases

- **Write clause submitted** (`CREATE`, `SET`, ...): rejected as unsupported — the language is read-only.
- **Long-running query**: aborted at the wall-clock timeout with the timeout error code.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Happy path | US-076-AC1 | Beads + links seeded | Ad-hoc neighbor query | Rows + column metadata + plan info |
| Unknown label | US-076-AC2 | Query uses `:Bread` | Submit | Stable unknown-label error |
| Policy parity | US-076-AC3 | Subject with row policy | Ad-hoc vs named equivalent | Identical visible rows and redactions |
| Budget | US-076-AC4 | Unindexed scan over large collection | Submit | Budget rejection before execution |
| Write clause | US-076-AC5 | `CREATE (n:Bead)` | Submit | Unsupported-clause error code |

## Dependencies

- **Stories**: US-075 (shared compile/plan machinery)
- **Feature Spec**: FEAT-009
- **Feature Requirements**: QRY-02, QRY-10, QRY-11, QRY-14, QRY-15, QRY-16
- **PRD Requirements**: FR-3
- **External**: CONTRACT-007 (field, result, error codes, budgets, limits), CONTRACT-002 (GraphQL integration), FEAT-029 (policy)

## Out of Scope

- Subscriptions on ad-hoc queries (V2).
- Larger opt-in budgets — those are a named-query declaration feature (QRY-11).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
