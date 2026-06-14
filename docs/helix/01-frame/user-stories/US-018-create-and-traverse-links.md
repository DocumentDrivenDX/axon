---
ddx:
  id: US-018
  review:
    self_hash: 05863b223efd66ed283862f73e5783f30128c242a18e0060a31964f14fbe8554
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# US-018: Create and Traverse Links

**Feature**: FEAT-007 — Entity-Graph Data Model
**Feature Requirements**: GRF-05, GRF-08, GRF-09, GRF-10
**PRD Requirements**: FR-2
**Priority**: P0
**Status**: Draft

## Story

**As** Ava, an agent application developer whose agents manage a dependency graph
**I want** my agents to create typed links between entities and follow them
**So that** relationships like "bead A depends on bead B" are first-class, validated records

## Context

This story exercises FEAT-007's link model and lifecycle: declared link types
(GRF-08), referential checks at create (GRF-10), triple uniqueness (GRF-09),
and the first-class link record (GRF-05). The traversal step at the end is
read-path behavior governed by FEAT-009 and CONTRACT-007; it appears here to
demonstrate the model end-to-end.

## Walkthrough

1. Ava's agent creates a `depends-on` link from `beads/bead-A` to `beads/bead-B` (surface per CONTRACT-001/CONTRACT-008).
2. The system verifies both entities exist, the link type is declared, and the triple is new, then commits and audits the link.
3. The agent lists bead-A's outgoing `depends-on` links and sees bead-B.
4. The agent traverses `depends-on` to depth 3 (read model per FEAT-009) and sees the transitive dependency tree.

## Acceptance Criteria

- [ ] **US-018-AC1** — Given two existing entities and a declared link type, when the agent creates a link between them, then the link is committed as a first-class record with its own identity and audit entry.
- [ ] **US-018-AC2** — Given a link-create request whose target entity does not exist, when submitted, then it fails with a not-found error identifying the missing entity and no link is created.
- [ ] **US-018-AC3** — Given an existing (source, target, type) link, when the agent creates the same triple again, then it is rejected with a conflict error.
- [ ] **US-018-AC4** — Given created links, when the agent lists outgoing links of one type from an entity, then exactly the linked targets are returned.
- [ ] **US-018-AC5** — Given a chain of `depends-on` links, when the agent traverses from the root to depth 3 (per FEAT-009 / CONTRACT-007), then the transitive dependency tree is returned.

## Edge Cases

- **Undeclared link type**: creation fails with a validation error naming the type (GRF-10).
- **Link metadata violating the link type's metadata schema**: rejected with field-level errors (GRF-08).
- **Deleting an entity with inbound links**: rejected unless force-cascade is requested (GRF-12).

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Happy path | US-018-AC1 | `bead-A`, `bead-B` exist; `depends-on` declared | Create link A→B | Link committed, audited |
| Missing target | US-018-AC2 | `bead-Z` does not exist | Create link A→Z | Not-found naming `bead-Z`; no link |
| Duplicate triple | US-018-AC3 | Link A→B `depends-on` exists | Create A→B `depends-on` again | Conflict error |
| List | US-018-AC4 | A→B, A→C `depends-on` | List outgoing from A | B and C |
| Traverse | US-018-AC5 | A→B→C→D chain | Traverse depth 3 from A | B, C, D in tree shape |

## Dependencies

- **Stories**: US-010 (entities exist), US-017 (schema declared)
- **Feature Spec**: FEAT-007
- **Feature Requirements**: GRF-05, GRF-08, GRF-09, GRF-10
- **PRD Requirements**: FR-2
- **External**: CONTRACT-001 (link operations, envelope), CONTRACT-010 (`link_types` declaration), CONTRACT-007 / FEAT-009 (traversal semantics), CONTRACT-008 (CLI)

## Out of Scope

- Pattern matching, filtered traversal, and reachability queries (FEAT-009 stories US-023..US-025).
- Bidirectional link sugar — model two links or query both directions.

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
