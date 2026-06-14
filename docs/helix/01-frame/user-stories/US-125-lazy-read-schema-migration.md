---
ddx:
  id: US-125
  review:
    self_hash: ba0b8b1ab4d7cdf92d316d57b93559049e3cbcf2dcdf5d239a280c4b3c086c12
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---

# US-125: Lazy-Read Schema Migration

**Feature**: FEAT-017 — Schema Evolution and Migration
**Feature Requirements**: EVO-15, EVO-16, EVO-17, EVO-18, EVO-19
**PRD Requirements**: FR-1; PRD Should-Have P1-1 (schema evolution and migration)
**Priority**: P1
**Status**: Draft

## Story

**As a** maintainer evolving a schema with live consumers
**I want** entities written at schema version N to remain readable after the schema bumps to N+1
**So that** workers don't see validation failures during a rolling schema change

## Context

Renumbered from US-062 (collision with FEAT-018). Entities track the schema version they were validated against (ADR-007), and compatible changes apply with zero downtime — but a write at version N+1 may add fields that older entities lack. Without lazy-read semantics, every read of an old entity either returns an unexpected shape or fails, breaking the read path for live consumers (e.g. a work-tracker backend whose workers read and write concurrently during a rolling schema change). This story exercises EVO-15 through EVO-19; read-time default declarations are part of the Entity Schema Format (CONTRACT-010).

## Walkthrough

1. Maintainer applies a schema update from version N to N+1 that adds fields, declaring read-time defaults for them in the schema (CONTRACT-010).
2. A worker reads an entity stored at version N against the now-active N+1 schema.
3. System returns the entity successfully: declared read-time defaults are populated, undeclared new fields come back null or omitted per their schema declaration, and the entity reports its actual stored version N. Storage is not modified.
4. Another worker writes that entity; the write validates against the active N+1 schema and the entity moves to N+1, while old and new readers continue without coordination.

## Acceptance Criteria

- [ ] **US-125-AC1** — Given an entity stored at schema version N and an active schema at N+1, when the entity is read, then the read succeeds and the entity reports its actual stored version N.
- [ ] **US-125-AC2** — Given a schema declaring read-time defaults for fields added in N+1 (CONTRACT-010), when a version-N entity is read, then those fields are populated from the declared defaults in the returned entity.
- [ ] **US-125-AC3** — Given fields added in N+1 with no declared read-time default, when a version-N entity is read, then those fields are returned as null or omitted, per the field's schema declaration.
- [ ] **US-125-AC4** — Given a force-applied N+1 schema that adds a required field with no default, when a version-N entity is read, then the read succeeds with the field absent or null plus a structured warning.
- [ ] **US-125-AC5** — Given a force-applied N+1 schema that adds a required field with no default, when a write omitting that field is submitted, then the write is rejected — the required-with-no-default constraint applies to writes only.
- [ ] **US-125-AC6** — Given lazy reads of a version-N entity, when storage is inspected afterward, then the entity is unmodified and still at version N.
- [ ] **US-125-AC7** — Given a version-N entity, when it is next written, then the write validates against the active schema and the stored entity moves to the active version.
- [ ] **US-125-AC8** — Given a rolling change with readers and writers at both N and N+1, when they operate concurrently on the same collection, then no reader fails and no coordination between old and new consumers is required.

## Edge Cases

- **Entity several versions behind**: An entity at N read against N+3 receives read-time defaults from all intervening versions that declare them.
- **Operator wants eager migration**: Lazy read is the runtime default; an operator can opt into eager revalidation via the US-060 revalidation operation.
- **Read-time default for a field the entity already has**: The stored value wins; defaults apply only to absent fields.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Old entity readable | US-125-AC1 | Entity `bead-1` at v1; active schema v2 | Read `bead-1` | Success; reported schema version is 1 |
| Defaults applied | US-125-AC2 | v2 adds `priority` with read-time default `3` | Read v1 entity | Returned entity has `priority = 3` |
| No default | US-125-AC3 | v2 adds optional `tags` with no read-time default | Read v1 entity | `tags` null or omitted per declaration |
| Required, no default | US-125-AC4/AC5 | v2 force-applied adding required `owner`, no default | Read v1 entity; then write omitting `owner` | Read succeeds with warning; write rejected |
| Storage untouched | US-125-AC6 | v1 entity read 100 times under v2 | Inspect storage | Entity unchanged, still v1 |
| Upgrade on write | US-125-AC7 | v1 entity; active v2 | Update the entity with a conforming payload | Stored entity now at v2 |
| Mixed consumers | US-125-AC8 | Workers reading/writing at v1 and v2 concurrently | Run both during rolling change | Zero read failures; no coordination needed |

## Dependencies

- **Stories**: US-058 (classification of the schema bump), US-060 (eager revalidation as the opt-in alternative)
- **Feature Spec**: FEAT-017
- **Feature Requirements**: EVO-15, EVO-16, EVO-17, EVO-18, EVO-19
- **PRD Requirements**: FR-1; Should-Have P1-1
- **External**: CONTRACT-010 (read-time default declarations in ESF)

## Out of Scope

- Schema-version-aware transformation beyond simple defaults — field renames, nested-shape changes, type conversions are deferred V2 transform-rule territory (FEAT-017 Out of Scope).
- Eagerly rewriting stored entities to the new version (migration backfill, deferred to V2).
- Change-feed schema-registry semantics for external consumers (FEAT-021).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
