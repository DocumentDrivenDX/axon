---
ddx:
  id: US-034
  review:
    self_hash: 2502297d611670b9038757cf73d7a030842792e7ab15d4bdaef1d81502f2c9c2
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---
# US-034: Background Index Build

**Feature**: FEAT-013 — Secondary Indexes and Query Acceleration
**Feature Requirements**: IDX-15, IDX-16
**PRD Requirements**: FR-4
**Priority**: P1
**Status**: Draft

## Story

**As a** Wei, a business workflow builder operating a live collection
**I want** a newly declared index to build in the background
**So that** existing entities get indexed without blocking normal operations

## Context

Extracted from FEAT-013. Exercises the index lifecycle `building` → `ready`
states (IDX-15, IDX-16), including write visibility during the build.

## Walkthrough

1. Wei adds an index to a collection that already holds entities.
2. The index enters `building`; queries keep using their previous plans.
3. Writes arriving during the build are reflected in the new index.
4. The build completes; the index becomes `ready` and the planner adopts it.

## Acceptance Criteria

- [ ] **US-034-AC1** — Given a collection with existing entities, when a new index is declared, then a background build starts and the index reports the `building` state.
- [ ] **US-034-AC2** — Given an index in `building`, when queries filter on its field, then the planner does not use the building index and results remain correct via the previous path.
- [ ] **US-034-AC3** — Given an index in `building`, when entities are written during the build, then those writes are reflected in the new index when it becomes `ready`.
- [ ] **US-034-AC4** — Given a build that scans all existing entities, when it completes, then the index transitions to `ready` and subsequent queries use it.

## Edge Cases

- **Empty collection**: the index is immediately `ready` with no background phase.
- **Rebuild**: an admin-triggered rebuild returns a `ready` index to `building`, discards entries, and rescans (IDX-18).

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Build starts | US-034-AC1 | 50K-entity collection | Declare new index | State `building`; build running |
| Not used while building | US-034-AC2 | Index `building` | Query on indexed field | Previous plan used; correct results |
| Writes during build | US-034-AC3 | Index `building` | Create/update entities | New index reflects them at `ready` |
| Adoption | US-034-AC4 | Build completes | Query on indexed field | Index path used |
| Empty collection | edge | 0 entities | Declare index | Immediately `ready` |

## Dependencies

- **Stories**: US-031 (declaration).
- **Feature Spec**: [FEAT-013 — Secondary Indexes and Query Acceleration](../features/FEAT-013-secondary-indexes.md)
- **Feature Requirements**: IDX-15, IDX-16 (IDX-18 for the rebuild edge case)
- **PRD Requirements**: FR-4
- **External**: ADR-010 (index lifecycle design)

## Out of Scope

- Index drop cleanup mechanics (feature-level edge case, IDX-17).

## Review Checklist

- [ ] Persona comes from the PRD
- [ ] Every AC is independently testable with one Given/When/Then
- [ ] No exact API/CLI/event/schema surface inline; Contract IDs referenced
- [ ] Test scenarios cover the happy path and at least one edge case
