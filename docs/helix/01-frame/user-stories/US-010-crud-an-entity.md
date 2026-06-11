---
ddx:
  id: US-010
---

# US-010: CRUD an Entity

**Feature**: FEAT-004 — Entity Operations
**Feature Requirements**: ENT-01, ENT-02, ENT-03, ENT-05, ENT-10, ENT-11
**PRD Requirements**: FR-1, FR-6
**Priority**: P0
**Status**: Draft

## Story

**As** Ava, an agent application developer giving her agents durable state
**I want** my agents to create, read, update, and delete entities in a collection
**So that** they manage structured business records safely without bespoke storage code

## Context

Entity CRUD is the smallest complete loop through Axon's governed write path:
schema validation, the system-metadata envelope, optimistic concurrency, and
audit. This story exercises FEAT-004's CRUD lifecycle (ENT-01..ENT-05) and
single-entity OCC (ENT-10, ENT-11); every failure mode must be structured
enough for an agent to self-correct.

## Walkthrough

1. Ava's agent creates an entity in a collection (surface per CONTRACT-001).
2. The system validates the body against the active schema and returns the entity with its system-metadata envelope.
3. The agent reads the entity by ID, then updates it supplying the expected version.
4. The system commits the update, increments the version, and audits the change.
5. The agent deletes the entity; subsequent reads return not-found, and the deleted state remains in audit history.

## Acceptance Criteria

- [ ] **US-010-AC1** — Given a schema-valid body, when the agent creates an entity, then the response contains the full entity including the system-metadata envelope (identity, version 1, creation timestamp) per CONTRACT-001.
- [ ] **US-010-AC2** — Given an existing entity, when read by ID, then the full entity with envelope is returned; given a missing ID, the read fails with a structured not-found error naming the requested ID.
- [ ] **US-010-AC3** — Given the correct expected version, when the agent updates the entity, then the update commits and the version increments by exactly 1.
- [ ] **US-010-AC4** — Given a stale expected version, when the agent updates the entity, then the update fails with a version-conflict error that includes the current committed state (per CONTRACT-001), and the stored entity is unchanged.
- [ ] **US-010-AC5** — Given an existing entity, when the agent deletes it, then subsequent reads return not-found and an audit entry capturing the deleted state exists (FEAT-003).

## Edge Cases

- **Schema-invalid create or update**: rejected with field-level validation errors; nothing is stored or changed.
- **Delete with stale expected version**: fails with a version conflict like any OCC write.
- **Client-provided ID that already exists**: create is rejected as a conflict per ADR-022.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Happy path | US-010-AC1 | Valid `tasks` body | Create | Entity returned, version 1, envelope present |
| Missing read | US-010-AC2 | No entity `t-404` | Read `t-404` | Structured not-found naming `t-404` |
| OCC success | US-010-AC3 | Entity at version 2 | Update with expected version 2 | Commit; version 3 |
| OCC conflict | US-010-AC4 | Entity at version 3 | Update with expected version 2 | Version conflict + current state; entity unchanged |
| Delete then read | US-010-AC5 | Entity exists | Delete, then read | Not-found; audit entry holds deleted state |

## Dependencies

- **Stories**: None
- **Feature Spec**: FEAT-004
- **Feature Requirements**: ENT-01, ENT-02, ENT-03, ENT-05, ENT-10, ENT-11
- **PRD Requirements**: FR-1, FR-6
- **External**: CONTRACT-001 (entity routes, envelope, error/conflict shapes), ADR-022 (create semantics), FEAT-002 (schema validation), FEAT-003 (audit)

## Out of Scope

- Partial patch semantics (US-012).
- Predicate queries over collections (US-011 / FEAT-009).
- Multi-entity atomicity (FEAT-008).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
