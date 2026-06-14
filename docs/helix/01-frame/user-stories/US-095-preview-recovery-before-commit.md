---
ddx:
  id: US-095
  review:
    self_hash: f8e5bf602d4b8b7da259c5e8cdd9046cc8d0d9ce013a30c8ebad98bd8d15f44e
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# US-095: Preview Recovery Before Commit

**Feature**: FEAT-023 — Rollback and Recovery
**Feature Requirements**: RBK-09, RBK-10, RBK-11, RBK-12, RBK-13
**PRD Requirements**: FR-19, FR-30
**Priority**: P1
**Status**: Draft

## Story

**As an** operator about to undo an agent mistake (Wei, business workflow
builder)
**I want** rollback dry-run output before any mutation occurs
**So that** I can verify exactly what will change and whether conflicts exist

## Context

Committing a recovery blind is how one incident becomes two. Dry-run is the
trust-building step: it shows the compensating operations, the provenance of
the damage, and any conflicts — without locks or side effects. This story
exercises dry-run mode (RBK-09), conflict detection (RBK-10), repair-plan
metadata (RBK-11), and diff/log/blame-style presentation (RBK-12) across the
governed surfaces (RBK-13).

## Walkthrough

1. Wei identifies a corrupted entity and selects a target version from its
   audit history.
2. Wei runs an entity rollback dry-run; Axon returns a field-level diff
   between current and target state without mutating anything.
3. The dry-run also reports the original audit IDs, the actor/tool that made
   the damaging writes, the policy and approval decisions, and the
   compensating operations that a commit would submit.
4. Wei extends the dry-run to the whole bad time window; Axon lists every
   entity and link that would change and flags one entity modified since the
   window as a conflict.
5. Wei resolves the conflict plan and proceeds to commit (US-096/US-097).

## Acceptance Criteria

- [ ] **US-095-AC1** — Given an entity with audit history, when an entity
  rollback dry-run runs against a target version, then it returns a diff and
  the entity is not mutated.
- [ ] **US-095-AC2** — Given a selected target version, when the dry-run
  returns, then the preview identifies that target version from audit
  history.
- [ ] **US-095-AC3** — Given the current entity version changed since the
  rollback target, when a dry-run runs, then the response includes conflict
  information identifying the would-be OCC conflict.
- [ ] **US-095-AC4** — Given any rollback dry-run, when the response is
  returned, then it includes repair-plan metadata: original audit IDs,
  actor/delegated authority, tool/API origin, policy decision, approval
  decision, and the compensating operations that would be submitted.
- [ ] **US-095-AC5** — Given a point-in-time dry-run, when it runs, then it
  lists every entity and link that would change without committing anything.
- [ ] **US-095-AC6** — Given a transaction rollback dry-run, when it runs,
  then it identifies all original transaction mutations and their
  compensating operations.

## Edge Cases

- **Empty window**: a point-in-time dry-run with no mutations after the
  cutoff returns an empty plan and a commit would be a no-op.
- **Redacted fields**: before/after values in the repair plan respect the
  caller's redaction policy.
- **Concurrent writes during dry-run**: dry-run takes no locks; the plan is
  advisory and commit-time OCC remains authoritative.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Side-effect-free diff | US-095-AC1 | Invoice at v7, target v5 | Entity rollback dry-run to v5 | Field diff v7→v5; invoice still at v7 |
| Conflict detection | US-095-AC3 | Target v5; entity changed to v8 after preview prepared | Dry-run | Conflict info naming current version |
| Repair plan | US-095-AC4 | Damage from agent `bot-3` via MCP at v6 | Dry-run | Plan includes original audit IDs, actor `bot-3`, tool origin, policy/approval decisions, compensating ops |
| Window listing | US-095-AC5 | 14 mutations across 9 entities after T | Point-in-time dry-run after T | All 9 entities/links listed; no commit |
| Transaction plan | US-095-AC6 | Transaction `tx-42` touched 3 entities | Transaction dry-run for `tx-42` | 3 original mutations + 3 compensating ops |

## Dependencies

- **Stories**: None (entry point for US-096, US-097).
- **Feature Spec**: FEAT-023
- **Feature Requirements**: RBK-09, RBK-10, RBK-11, RBK-12, RBK-13
- **PRD Requirements**: FR-19, FR-30
- **External**: CONTRACT-001 (rollback/audit endpoints), CONTRACT-002
  (GraphQL fields), CONTRACT-008 (CLI rollback dry-run, audit diff/blame),
  CONTRACT-009 (SDK helpers), CONTRACT-005 (audit references)

## Out of Scope

- Committing the rollback (US-096, US-097).
- Approval routing of the eventual commit (FEAT-030).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
