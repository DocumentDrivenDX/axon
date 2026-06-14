---
ddx:
  id: US-024
  review:
    self_hash: d3fd60732ae266c7f896a7ccf6ccdca7c9791f18c7c7d2972a5f111f80b32940
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# US-024: Explode a Bill of Materials

**Feature**: FEAT-009 — Unified Graph Query (Cypher)
**Feature Requirements**: QRY-01, QRY-13
**PRD Requirements**: FR-3
**Priority**: P0
**Status**: Draft

## Story

**As** Wei, a business workflow builder on an ERP application
**I want** to recursively expand a product into its component parts with link metadata
**So that** I can compute total cost and check inventory across all sub-assemblies

## Context

BOM explosion stresses two model+query features at once: bounded recursive
traversal and link metadata projection (quantities live on the `CONTAINS`
relationship, not the part). This story exercises QRY-01 (variable-length
paths, relationship property access, collection projection) over FEAT-007's
metadata-bearing links.

## Walkthrough

1. Wei runs a bounded traversal from a product along `CONTAINS` links, projecting components and relationship metadata (language per CONTRACT-007).
2. The result carries each component with its relationship's `quantity` property.
3. Shared components reached via multiple paths are returned once, with all paths recoverable via collection projection.
4. Wei identifies leaf parts (no outgoing `CONTAINS` links) for inventory checks.

## Acceptance Criteria

- [ ] **US-024-AC1** — Given a product with a multi-level component tree, when the bounded traversal runs, then the full BOM tree is returned including relationship metadata on each hop.
- [ ] **US-024-AC2** — Given relationship properties such as `quantity`, when projected, then their values are accessible in the result rows.
- [ ] **US-024-AC3** — Given a shared component reachable via multiple paths (diamond), when the query runs, then the component appears once with all paths recoverable via collection projection.
- [ ] **US-024-AC4** — Given components whose link targets were deleted (dangling), when the traversal runs, then dangling links are skipped without error.

## Edge Cases

- **Leaf identification**: parts with no outgoing `CONTAINS` links are identifiable from the result shape (no children).
- **Deeper-than-bound assemblies**: nodes beyond the explicit bound are absent; the query must use an adequate bound (CONTRACT-007 limits apply).

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Full explosion | US-024-AC1 | widget-X → frame(2) → bolt(8) | `MATCH (p:Product {id:'widget-X'})-[c:CONTAINS*1..8]->(comp) RETURN comp, c` | Frame and bolt rows with relationship data |
| Quantity projection | US-024-AC2 | `CONTAINS {quantity: 2}` | Project `c.quantity` | 2 |
| Diamond | US-024-AC3 | Two assemblies share `bolt` | Explode with `collect()` of paths | Bolt once, both paths listed |
| Dangling | US-024-AC4 | One component force-deleted | Explode | Dangling link skipped, no error |

## Dependencies

- **Stories**: US-018 (links with metadata)
- **Feature Spec**: FEAT-009
- **Feature Requirements**: QRY-01, QRY-13
- **PRD Requirements**: FR-3
- **External**: CONTRACT-007 (language, projection, limits), CONTRACT-010 (link metadata schema), FEAT-007 (link model)

## Out of Scope

- Cost roll-up arithmetic beyond aggregation projections (application logic).
- Weighted path computation (V2).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
