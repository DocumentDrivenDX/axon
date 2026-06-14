---
ddx:
  id: US-015
  review:
    self_hash: 377deb4cb0b5c7964e2d6ebfad51c846e2c09da659be599b013027f44754dad4
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---
# US-015: Store and Query Beads

**Feature**: FEAT-006 — Bead Storage Adapter
**Feature Requirements**: BED-01, BED-03, BED-04, BED-05, BED-09, BED-11, BED-13
**PRD Requirements**: None directly (dogfooding extension; builds on FR-1)
**Priority**: P1
**Status**: Draft

## Story

**As a** Ava, an agent application developer wiring an agent framework to a work queue
**I want** a purpose-built bead collection in Axon with a standard schema and validated lifecycle
**So that** I do not have to reinvent bead storage and lifecycle management, and lifecycle violations are caught at the data layer

## Context

Extracted from FEAT-006. Exercises the pre-defined bead schema (BED-01), the
DDx-superset lifecycle vocabulary and transition validation (BED-03..BED-05),
dependency reference validation (BED-09), and bead-specific queries (BED-11).
The exact CLI commands and HTTP routes are owned by CONTRACT-008 and
CONTRACT-001; this story specifies the behavior behind them.

## Walkthrough

1. Ava initializes the bead module on a fresh deployment; a bead collection with the standard schema is created and discoverable.
2. Ava creates a bead with an issue type and title; it persists with the lifecycle's initial state and a full audit record.
3. Ava lists beads filtered by status and sees only matching beads.
4. An agent attempts an invalid status transition; Axon rejects it and lists the valid next states.
5. Ava closes a finished bead; later attempts to update its status are rejected as transitions from a terminal state.

## Acceptance Criteria

- [ ] **US-015-AC1** — Given a fresh deployment, when the bead module is initialized, then a bead collection exists whose schema accepts every field the DDx tracker writes.
- [ ] **US-015-AC2** — Given the bead collection, when a bead is created with issue type `task` and a title, then it persists with the declared initial lifecycle state and is readable by id.
- [ ] **US-015-AC3** — Given beads in several states, when beads are listed filtered by a status, then only beads in that status are returned.
- [ ] **US-015-AC4** — Given a bead in a non-terminal state, when a status update requests a transition not allowed by the lifecycle, then the update is rejected with a structured error listing the valid next states from the current state.
- [ ] **US-015-AC5** — Given a bead in the terminal state `closed`, when an ordinary status update is attempted, then it is rejected with a transition-from-terminal error; only the explicit reopen operation moves it back to `open`.
- [ ] **US-015-AC6** — Given a create request whose dependency list references a bead id that does not exist, when the bead is created, then the operation fails with a validation error naming the missing id.

## Edge Cases

- **Status outside the vocabulary**: a create or update naming a status not in the DDx-superset vocabulary is rejected with an error listing the accepted statuses.
- **Reopen of a `cancelled` bead**: reopen applies to `closed`; `cancelled` stays terminal unless the lifecycle declaration says otherwise.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Init creates collection | US-015-AC1 | Fresh deployment, no bead collection | Initialize bead module | Bead collection exists; schema lists DDx-compatible fields |
| Create and read back | US-015-AC2 | Initialized bead collection | Create bead `{type: task, title: "Review PR"}` | Bead persisted with initial state; readable by id |
| Filter by status | US-015-AC3 | Beads in `open`, `in_progress`, `closed` | List with status=`open` | Only `open` beads returned |
| Invalid transition | US-015-AC4 | Bead in `proposed` | Update status to `in_progress` (if not a declared transition) | Rejected; error lists valid next states from `proposed` |
| Terminal state guard | US-015-AC5 | Bead in `closed` | Update status to `in_progress` | Rejected with transition-from-terminal error; explicit reopen → `open` succeeds |
| Dangling dependency | US-015-AC6 | No bead `bead-999` | Create bead with dependency on `bead-999` | Validation error naming `bead-999` |

## Dependencies

- **Stories**: US-016 (dependency tracking and ready queue).
- **Feature Spec**: [FEAT-006 — Bead Storage Adapter](../features/FEAT-006-bead-storage-adapter.md)
- **Feature Requirements**: BED-01, BED-03, BED-04, BED-05, BED-09, BED-11, BED-13
- **PRD Requirements**: None directly (dogfooding extension)
- **External**: CONTRACT-001 (HTTP routes), CONTRACT-008 (CLI command tree), CONTRACT-010 (schema and lifecycle declaration grammar)

## Out of Scope

- Dependency graph queries and ready-queue semantics (US-016).
- Import/export round-trip fidelity (feature-level conformance test, FEAT-006 NFR).

## Review Checklist

- [ ] Persona comes from the PRD
- [ ] Every AC is independently testable with one Given/When/Then
- [ ] No exact API/CLI/event/schema surface inline; Contract IDs referenced
- [ ] Test scenarios cover the happy path and at least one edge case
