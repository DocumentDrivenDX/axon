---
ddx:
  id: US-006
  review:
    self_hash: d9ff23c155d97d4c94e9150e11744d4273635122a878745d6935baa3b99b7af6
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# US-006: Inspect a Schema

**Feature**: FEAT-002 — Schema Engine
**Feature Requirements**: SCH-12
**PRD Requirements**: FR-1
**Priority**: P0
**Status**: Draft

## Story

**As a** developer or agent
**I want** to retrieve the schema for a collection
**So that** I know what fields and types are expected before writing

## Context

Writers that cannot see the governing schema are reduced to trial-and-error against validation failures. This story exercises SCH-12: the schema for any collection is retrievable via the API and CLI in its portable format, including field descriptions when the definition provides them. Retrieval surfaces are CONTRACT-001/CONTRACT-008; the document format is CONTRACT-010.

## Walkthrough

1. Developer or agent requests the schema for a collection via the CLI (CONTRACT-008) or HTTP API (CONTRACT-001).
2. System returns the full active schema document in the portable Entity Schema Format (CONTRACT-010), including its version.
3. The caller reads the field names, types, constraints, and descriptions, then constructs a conforming write.

## Acceptance Criteria

- [ ] **US-006-AC1** — Given a collection with a bound schema, when the caller requests the schema via the CLI (CONTRACT-008), then the full schema document is displayed.
- [ ] **US-006-AC2** — Given a collection with a bound schema, when the caller requests the schema via the HTTP API (CONTRACT-001), then the schema is returned in its portable format (CONTRACT-010), consumable by standard JSON Schema tooling for the non-extended subset.
- [ ] **US-006-AC3** — Given a schema whose definition includes field descriptions, when the schema is retrieved, then the descriptions are present in the returned document.

## Edge Cases

- **Non-existent collection**: Schema retrieval returns a structured not-found error.
- **Schema with Axon extensions**: The returned document includes the extensions; the non-extended subset remains valid JSON Schema.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| CLI retrieval | US-006-AC1 | `invoices` bound to schema v2 | Request schema via CLI | Full schema v2 document displayed |
| API retrieval | US-006-AC2 | `invoices` bound to schema v2 | Request schema via API | Portable schema document returned; standard tooling parses the non-extended subset |
| Descriptions | US-006-AC3 | Schema defines `amount` with description "Invoice total in cents" | Retrieve schema | Description present on `amount` |
| Missing collection | US-006-AC1 | No collection `ghosts` | Request schema for `ghosts` | Structured not-found error |

## Dependencies

- **Stories**: US-004 (a schema must be defined before it can be inspected)
- **Feature Spec**: FEAT-002
- **Feature Requirements**: SCH-12
- **PRD Requirements**: FR-1
- **External**: CONTRACT-001, CONTRACT-008, CONTRACT-010

## Out of Scope

- Listing schema version history or diffs between versions (FEAT-017, US-061).
- Schema discovery via GraphQL introspection or MCP resources (FEAT-015, FEAT-016).
- Collection metadata beyond the schema (FEAT-001, US-002).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
