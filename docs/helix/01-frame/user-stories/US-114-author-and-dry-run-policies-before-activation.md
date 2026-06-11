---
ddx:
  id: US-114
---

# US-114: Author And Dry-Run Policies Before Activation

**Feature**: FEAT-031 — Policy and Intents Admin UI
**Feature Requirements**: PUI-01, PUI-02
**PRD Requirements**: FR-24
**Priority**: P0
**Status**: Approved

## Story

**As a** developer defining collection schemas (Ava, Agent Application Developer persona)
**I want** to edit and test policy blocks in the web UI before activation
**So that** policy mistakes are caught before agents or users rely on them

## Context

The schema workspace is where policy lives, so policy authoring belongs
beside the schema editor: compile, read the report, dry-run fixtures, then
activate. This is the UI realization of FEAT-029's authoring workflow
(US-109).

## Walkthrough

1. Developer opens a collection's schema workspace and edits the
   access-control block beside the raw schema editor.
2. Developer previews the change; the system runs the FEAT-029 compiler and
   renders a compile report.
3. If the compile fails, activation is blocked and the active version is
   unchanged.
4. On a successful dry-run, the developer evaluates fixture subjects and rows
   before applying the new schema/policy version.

## Acceptance Criteria

- [ ] **US-114-AC1** — Given a collection schema workspace, when the
  developer opens it, then the access-control policy block is exposed beside
  the raw schema editor.
- [ ] **US-114-AC2** — Given a previewed schema/policy change, when the
  compiler runs, then the UI renders a compile report with errors, warnings,
  affected GraphQL nullability, and MCP envelope changes.
- [ ] **US-114-AC3** — Given a failed policy compile, when the developer
  attempts activation, then activation is blocked and the active policy
  version is unchanged.
- [ ] **US-114-AC4** — Given a successful dry-run, when the developer
  evaluates fixture subjects and rows, then decisions are returned before the
  new schema/policy version is applied.

## Edge Cases

- **Compile warnings without errors**: activation is allowed but warnings
  stay visible in the report.
- **Concurrent edit**: if the active schema version changes while editing,
  the preview surfaces the conflict instead of silently compiling against a
  stale base.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Editor exposure | US-114-AC1 | Collection with policy | Open schema workspace | Policy block beside raw schema |
| Compile report | US-114-AC2 | Valid candidate change | Preview | Report with nullability and envelope changes |
| Failed compile blocks | US-114-AC3 | Rule with invalid field path | Preview, then activate | Activation blocked; version unchanged |
| Fixture dry-run | US-114-AC4 | Compiled candidate | Evaluate fixture subject | Decision rendered pre-activation |

## Dependencies

- **Stories**: US-109 (compiler and dry-run backend), US-121 (schema
  workspace)
- **Feature Spec**: FEAT-031
- **Feature Requirements**: PUI-01, PUI-02
- **PRD Requirements**: FR-24
- **External**: CONTRACT-004 (grammar, compile reasons), CONTRACT-002
  (GraphQL operations)

## Out of Scope

- A visual policy-builder DSL — raw policy editing only.
- Backend compile semantics (FEAT-029).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
