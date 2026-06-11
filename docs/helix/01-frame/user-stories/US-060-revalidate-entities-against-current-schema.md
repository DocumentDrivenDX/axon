---
ddx:
  id: US-060
---

# US-060: Revalidate Entities Against Current Schema

**Feature**: FEAT-017 — Schema Evolution and Migration
**Feature Requirements**: EVO-09, EVO-11
**PRD Requirements**: FR-1; PRD Should-Have P1-1 (schema evolution and migration)
**Priority**: P1
**Status**: Draft

## Story

**As an** operator after a schema change
**I want** to find all entities that don't conform to the current schema
**So that** I can fix or migrate them

## Context

After a forced breaking change, some stored entities no longer conform, and an operator needs a complete, accurate list to plan repair. This story exercises EVO-09 (on-demand revalidation with per-entity error reports) and EVO-11 (background execution with progress for large collections). The revalidation operation surface is defined by CONTRACT-008 (CLI) and CONTRACT-001 (HTTP API).

## Walkthrough

1. Operator triggers revalidation of a collection via the CLI or HTTP API (CONTRACT-008/CONTRACT-001).
2. System scans every entity in the collection, validating each against the current active schema; for a large collection the scan runs in the background and reports progress.
3. System returns a report listing each invalid entity with its identifier, version, and specific validation errors; valid entities are untouched and unflagged.
4. Operator uses the report to fix or migrate the non-conforming entities.

## Acceptance Criteria

- [ ] **US-060-AC1** — Given a collection containing non-conforming entities, when the operator runs revalidation (CONTRACT-008/CONTRACT-001), then every invalid entity is reported and every valid entity is not.
- [ ] **US-060-AC2** — Given a revalidation report, when the operator reads an entry, then it contains the entity identifier, the entity version, and the specific validation errors for that entity.
- [ ] **US-060-AC3** — Given a revalidation run, when it completes, then no entity has been modified or flagged in storage — the operation is read-only.
- [ ] **US-060-AC4** — Given a collection with more than 1,000 entities, when revalidation is triggered, then it runs as a background operation.
- [ ] **US-060-AC5** — Given a background revalidation in progress, when the operator checks its status, then progress is reported as entities scanned out of total.

## Edge Cases

- **Empty collection**: Revalidation returns success with zero invalid entities.
- **Writes during revalidation**: New writes validate against the current schema at write time; the report may include an entity that is subsequently updated to conform.
- **Revalidation re-run**: Running revalidation twice on unchanged data yields the same report.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Mixed conformance | US-060-AC1 | 10 entities; 3 violate current schema | Run revalidation | Exactly those 3 reported invalid |
| Report detail | US-060-AC2 | Entity `inv-7` v4 missing required `region` | Read its report entry | Entry has `inv-7`, version 4, missing-required-field error |
| Read-only | US-060-AC3 | Same 10 entities | Run revalidation, then re-read entities | All entities byte-identical and at the same versions |
| Background mode | US-060-AC4 | Collection with 5,000 entities | Trigger revalidation | Operation runs in background |
| Progress | US-060-AC5 | Background run halfway through | Check status | Progress reports 2,500 / 5,000 |

## Dependencies

- **Stories**: US-059 (forced breaking changes create the non-conformance this story finds)
- **Feature Spec**: FEAT-017
- **Feature Requirements**: EVO-09, EVO-11
- **PRD Requirements**: FR-1; Should-Have P1-1
- **External**: CONTRACT-001, CONTRACT-008

## Out of Scope

- Fixing or transforming the invalid entities (migration rules are deferred to V2).
- Automatic revalidation triggered by a breaking apply (EVO-10, exercised in US-059).
- Lazy-read behavior for old-version entities (US-125).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
