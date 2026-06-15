---
ddx:
  id: US-045
  review:
    self_hash: a169cfbfedd4bfede88d35b2d3b8fb34d8cc4cbc834285f71b016a4cbdca45fa
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# US-045: Use Advanced Database Tools

**Feature**: FEAT-011 — Admin Web UI
**Feature Requirements**: UI-24, UI-25, UI-26, UI-27
**PRD Requirements**: FR-24
**Priority**: P1
**Status**: Approved

## Story

**As a** developer or operator (Ava, Agent Application Developer persona)
**I want** GraphQL, links, lifecycle transitions, and markdown templates exposed in context
**So that** I can exercise higher-level Axon features from the same database workspace

## Context

Beyond CRUD, Axon's higher-level capabilities — GraphQL queries, typed links,
lifecycle state machines, and markdown templates — need an in-context surface
so developers can explore and verify them without external tooling. Exercises
FEAT-011 requirements UI-24 through UI-27.

## Walkthrough

1. Developer opens the database GraphQL console.
2. System loads a query editor and response pane; introspection returns the
   generated schema.
3. Developer opens an entity's Links tab and creates an outbound link, then
   removes it.
4. Developer opens the entity's Lifecycle tab, sees the current state, and
   performs an allowed transition.
5. Developer creates a markdown template on the collection, previews it
   through an entity, and deletes it.

## Acceptance Criteria

- [ ] **US-045-AC1** — Given a database workspace, when the developer opens
  the GraphQL console, then a query editor and response pane load.
- [ ] **US-045-AC2** — Given the GraphQL console, when the developer runs an
  introspection query, then a schema is returned, and when the developer runs
  an invalid query, then errors are rendered.
- [ ] **US-045-AC3** — Given an entity detail view, when the developer
  creates and then removes an outbound link from the Links tab, then both
  operations succeed and are reflected in the link list.
- [ ] **US-045-AC4** — Given an entity with a lifecycle, when the developer
  opens the Lifecycle tab, then the current state is shown and an allowed
  transition can be performed.
- [ ] **US-045-AC5** — Given a collection, when the developer creates a
  markdown template, previews it through an entity tab, and deletes it, then
  each step succeeds.

## Edge Cases

- **Disallowed lifecycle transition**: the UI surfaces the structured guard
  error and the state remains unchanged.
- **Template referencing missing fields**: the preview shows the rendering
  error rather than failing silently.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Console loads | US-045-AC1 | Database workspace | Open GraphQL console | Editor and response pane render |
| Introspection and errors | US-045-AC2 | Console open | Run introspection; run invalid query | Schema returned; errors rendered |
| Link create/remove | US-045-AC3 | Entities `e1`, `e2` | Link `e1`→`e2`, then remove | Link appears, then disappears |
| Lifecycle transition | US-045-AC4 | `e1` in state `draft` | Transition to allowed state | New state shown |
| Template lifecycle | US-045-AC5 | Collection `invoices` | Create, preview via entity, delete template | All steps succeed |

## Dependencies

- **Stories**: US-042 (entity browsing)
- **Feature Spec**: FEAT-011
- **Feature Requirements**: UI-24 through UI-27
- **PRD Requirements**: FR-24
- **External**: CONTRACT-002 (GraphQL surface), CONTRACT-001 (lifecycle and
  markdown template endpoints)

## Out of Scope

- Policy-aware GraphQL console workflows (FEAT-031).
- Graph visualization of the link graph.

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
