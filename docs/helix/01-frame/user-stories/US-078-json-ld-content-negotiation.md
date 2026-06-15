---
ddx:
  id: US-078
  review:
    self_hash: bda2992bfa47c726763ca2f06eac19207c21d89f678f43d5ec9a206aa9277a1f
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# US-078: JSON-LD Content Negotiation

**Feature**: FEAT-015 — GraphQL Query Layer
**Feature Requirements**: GQL-20
**PRD Requirements**: FR-20
**Priority**: P2
**Status**: Draft

## Story

**As** Wei, a business workflow builder integrating a linked-data-aware
consumer (knowledge-graph tool or external integrator)
**I want** to request entity payloads as JSON-LD via content negotiation
**So that** I can consume Axon data with standard linked-data context,
identity, and typing without bespoke translation

## Context

ADR-020 selected document-shaped storage with selective RDF concept
adoption; entity URLs are dereferenceable IRIs. JSON-LD adoption is additive:
the canonical surface remains plain JSON, and JSON-LD is rendered only on
request. This story exercises GQL-20; the canonical entity URL form is
defined by CONTRACT-001's route grammar.

## Walkthrough

1. Wei's integration requests an entity query with the JSON-LD media type in
   content negotiation.
2. Axon returns the response as JSON-LD with a generated context, the
   canonical entity URL as the node identifier, and a type derived from the
   collection schema.
3. Linked entities render as identifier-bearing nested nodes; the consumer
   dereferences an identifier to fetch the full entity.
4. A plain-JSON client makes the same request without the JSON-LD media type
   and receives the existing response shape unchanged.

## Acceptance Criteria

- [ ] **US-078-AC1** — Given a request negotiating the JSON-LD media type,
  when an entity query executes, then the body is JSON-LD with a generated
  context, the node identifier set to the canonical entity URL (route form
  per CONTRACT-001), and the type derived from the collection schema.
- [ ] **US-078-AC2** — Given the default or unspecified media type, when the
  same query executes, then the existing JSON response shape returns
  unchanged.
- [ ] **US-078-AC3** — Given a schema whose field names collide with
  JSON-LD reserved keywords, when the context is generated, then colliding
  names are remapped via context aliases, and schema writes emit a collision
  warning (FEAT-002).
- [ ] **US-078-AC4** — Given relationship traversals in the response, when
  rendered as JSON-LD, then linked entities appear as identifier-bearing
  nested nodes whose identifiers dereference to the full entity.
- [ ] **US-078-AC5** — Given any JSON-LD response, when processed by a
  conformant JSON-LD 1.1 processor, then it validates without errors.
- [ ] **US-078-AC6** — Given the plain-JSON benchmark, when JSON-LD support
  is enabled, then plain-JSON latency shows no measurable regression.

## Edge Cases

- **Unsupported media type**: Standard content-negotiation failure
  semantics; no partial or mixed rendering.
- **Schemaless collection**: JSON-LD typing falls back gracefully (generic
  typing) rather than failing the request.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| JSON-LD render | US-078-AC1 | Invoice entity | Query with JSON-LD media type | Context + canonical URL identifier + schema-derived type |
| Default unchanged | US-078-AC2 | Same entity | Query with default media type | Existing JSON shape, byte-stable fields |
| Keyword collision | US-078-AC3 | Schema with a reserved-keyword field name | Write schema, query as JSON-LD | Warning at schema write; alias in generated context |
| Nested nodes | US-078-AC4 | Entity with 2 links | JSON-LD query with traversal | 2 nested identifier-bearing nodes; identifiers dereference |
| Processor validation | US-078-AC5 | Any JSON-LD response | Run a JSON-LD 1.1 processor | Valid, no errors |

## Dependencies

- **Stories**: US-048
- **Feature Spec**: FEAT-015
- **Feature Requirements**: GQL-20
- **PRD Requirements**: FR-20
- **External**: CONTRACT-001 (canonical entity URLs), CONTRACT-002 (GraphQL
  surface), JSON-LD 1.1 specification

## Out of Scope

- RDF storage, SPARQL, or triple-store semantics (ADR-020 non-goals).
- JSON-LD framing or custom client-supplied contexts.

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
