---
ddx:
  id: US-059
---

# US-059: Force-Apply a Breaking Change

**Feature**: FEAT-017 — Schema Evolution and Migration
**Feature Requirements**: EVO-06, EVO-08, EVO-10
**PRD Requirements**: FR-1; PRD Should-Have P1-1 (schema evolution and migration)
**Priority**: P1
**Status**: Draft

## Story

**As a** developer who understands the impact of a breaking change
**I want** to apply it with explicit confirmation
**So that** I can evolve the schema even when existing data doesn't conform

## Context

Some breaking changes are intentional — a constraint must tighten even though old data violates it. The system's job is to make that an informed, explicit, audited decision rather than an accident. This story exercises EVO-06 (reject without force, apply with force), EVO-08 (audited classification and force usage), and EVO-10 (background revalidation after a breaking apply). Force confirmation surfaces are defined by CONTRACT-008 (CLI) and CONTRACT-001 (HTTP API).

## Walkthrough

1. Developer submits a breaking schema update without force confirmation; the system rejects it and returns the compatibility report.
2. Developer reviews the report and resubmits the update with explicit force confirmation (CONTRACT-008/CONTRACT-001).
3. System applies the change, increments the schema version, and writes an audit entry containing the compatibility classification, the field-level diff, and the fact that force was used.
4. Background revalidation runs, and the developer can retrieve the list of entities that are now invalid under the new schema.

## Acceptance Criteria

- [ ] **US-059-AC1** — Given a breaking schema change, when it is submitted without force confirmation, then it is rejected and the response carries the compatibility report.
- [ ] **US-059-AC2** — Given a breaking schema change, when it is submitted with explicit force confirmation, then it applies and the schema version increments.
- [ ] **US-059-AC3** — Given a force-applied breaking change, when the audit log is queried, then the entry includes the compatibility classification, the field-level diff, and that force was used.
- [ ] **US-059-AC4** — Given a force-applied breaking change, when revalidation results are retrieved, then the entities that are now invalid under the new schema are reported.

## Edge Cases

- **Force on a compatible change**: Allowed but unnecessary; the change applies exactly as it would without force.
- **Concurrent breaking updates**: Two forced updates race — one wins via the version increment (ADR-007); the other receives a conflict error.
- **Force with zero affected entities**: Applies; revalidation reports zero invalid entities.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Rejected without force | US-059-AC1 | Breaking update narrowing `status` enum | Submit without force | Rejected; compatibility report returned; schema version unchanged |
| Applied with force | US-059-AC2 | Same breaking update | Submit with force confirmation | Applied; version v1 → v2 |
| Audit completeness | US-059-AC3 | Force-applied update from previous scenario | Query audit log | Entry has classification `breaking`, field diff, force flag |
| Post-apply revalidation | US-059-AC4 | 3 entities carry removed enum value | Retrieve revalidation results | Those 3 entities reported invalid with their errors |

## Dependencies

- **Stories**: US-058 (classification produces the report force overrides), US-060 (revalidation reporting)
- **Feature Spec**: FEAT-017
- **Feature Requirements**: EVO-06, EVO-08, EVO-10
- **PRD Requirements**: FR-1; Should-Have P1-1
- **External**: CONTRACT-001, CONTRACT-008, CONTRACT-005 (audit record shape)

## Out of Scope

- Transforming or fixing the now-invalid entities (migration rules are deferred — FEAT-017 Out of Scope).
- Rolling back to the previous schema version (deferred to V2).
- Who is authorized to force changes (FEAT-012/FEAT-029).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
