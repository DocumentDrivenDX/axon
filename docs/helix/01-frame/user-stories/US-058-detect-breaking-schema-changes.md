---
ddx:
  id: US-058
  review:
    self_hash: b1a287fb78ba88650d75b9fce846ebdc3365739e65a6e1e897ccc0f16062f70e
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# US-058: Detect Breaking Schema Changes

**Feature**: FEAT-017 — Schema Evolution and Migration
**Feature Requirements**: EVO-01, EVO-02, EVO-07
**PRD Requirements**: FR-1; PRD Should-Have P1-1 (schema evolution and migration)
**Priority**: P1
**Status**: Draft

## Story

**As a** developer evolving a collection schema
**I want** the system to tell me if my change is breaking before it applies
**So that** I don't accidentally invalidate existing data

## Context

Without classification, a developer cannot tell whether tightening a constraint or adding a field will strand existing entities. This story exercises EVO-01 (compatible/breaking/metadata-only classification on every schema update), EVO-02 (impact report for breaking changes), and EVO-07 (dry-run mode). Schema update and dry-run surfaces are defined by CONTRACT-008 (CLI) and CONTRACT-001 (HTTP API).

## Walkthrough

1. Developer submits a proposed schema update in dry-run mode via the CLI or HTTP API (CONTRACT-008/CONTRACT-001).
2. System compares the proposed schema to the active version and classifies the change as compatible, breaking, or metadata-only.
3. For a breaking change, the system reports which fields changed, how many entities are potentially affected, and what the validation failures would be — without applying anything.
4. Developer reads the report and decides whether to apply, revise, or plan a migration.

## Acceptance Criteria

- [ ] **US-058-AC1** — Given an active schema, when the developer submits an update adding an optional field, then the change is classified as compatible.
- [ ] **US-058-AC2** — Given an active schema, when the developer submits an update adding a required field, then the change is classified as breaking.
- [ ] **US-058-AC3** — Given stored entities containing a field, when the developer submits an update removing that field, then the change is classified as breaking.
- [ ] **US-058-AC4** — Given an enum field, when the developer submits an update adding enum values, then the change is classified as compatible.
- [ ] **US-058-AC5** — Given an enum field, when the developer submits an update removing enum values, then the change is classified as breaking.
- [ ] **US-058-AC6** — Given a constrained field, when the developer submits an update tightening the constraint (e.g. raising a minimum length), then the change is classified as breaking.
- [ ] **US-058-AC7** — Given a breaking change, when classification completes, then the response includes the count of potentially affected entities and the validation failures they would incur.
- [ ] **US-058-AC8** — Given any proposed schema update, when it is submitted in dry-run mode (CONTRACT-008/CONTRACT-001), then the classification and impact report are returned and no change is applied.

## Edge Cases

- **Metadata-only change**: Changing a field description or adding an index is classified metadata-only and applies with no entity impact.
- **Breaking change on an empty collection**: Still classified as breaking by schema analysis; affected-entity count is zero.
- **Identical schema resubmitted**: Classified as metadata-only or rejected as a no-op per CONTRACT-001/CONTRACT-008 semantics; never increments the version with a phantom change.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Optional add | US-058-AC1 | Active schema v1 | Dry-run update adding optional `notes` | Classified compatible |
| Required add | US-058-AC2 | Active schema v1 | Dry-run update adding required `region` with no default | Classified breaking |
| Enum narrow | US-058-AC5 | `status` enum `[pending, active, done]`; entities with `done` | Dry-run removing `done` | Classified breaking |
| Impact report | US-058-AC7 | 42 entities would fail under new schema | Dry-run breaking update | Report shows affected count 42 with would-be validation failures |
| Dry-run is inert | US-058-AC8 | Active schema v1 | Dry-run any update | Schema remains v1; classification returned |

## Dependencies

- **Stories**: US-004 (an active schema must exist)
- **Feature Spec**: FEAT-017
- **Feature Requirements**: EVO-01, EVO-02, EVO-07
- **PRD Requirements**: FR-1; Should-Have P1-1
- **External**: CONTRACT-001, CONTRACT-008

## Out of Scope

- Applying a breaking change (US-059).
- Scanning stored entities for current conformance (US-060).
- Viewing diffs between historical versions (US-061).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
