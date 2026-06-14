---
ddx:
  id: US-049
  review:
    self_hash: 75f72265bc0c3c987f0bf4f2d2fa86d915f76e3fdf632551945934a97cb2b306
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# US-049: Discover the API via Introspection

**Feature**: FEAT-015 — GraphQL Query Layer
**Feature Requirements**: GQL-01, GQL-02, GQL-04
**PRD Requirements**: FR-20
**Priority**: P0
**Status**: Draft

## Story

**As** Ava, an agent application developer integrating with Axon
**I want** the GraphQL schema to reflect the current collection schemas
**So that** I can use standard GraphQL tooling to explore and query the API

## Context

Discoverability is a product principle: developers should learn Axon's
surface from the API itself. This story exercises FEAT-015's schema
generation and metadata requirements (GQL-01, GQL-02, GQL-04): generated
types track registered collections live, and collection metadata exposes the
policy and audit affordances clients need before writing.

## Walkthrough

1. Ava points a GraphQL IDE at Axon and runs introspection.
2. The IDE shows typed queries, mutations, and inputs for every registered
   collection.
3. Ava registers a new collection; introspection immediately shows its types.
4. Ava modifies a schema; the corresponding GraphQL types update.
5. Ava reads collection metadata to learn policy envelopes, redactable
   fields, approval-routed operations, schema/policy versions, and audit
   cursor support before writing client code.

## Acceptance Criteria

- [ ] **US-049-AC1** — Given registered collections, when Ava runs GraphQL
  introspection, then types exist for all registered collections (naming per
  CONTRACT-002).
- [ ] **US-049-AC2** — Given a newly created collection, when introspection
  runs again, then the new collection's types are available without restart.
- [ ] **US-049-AC3** — Given a schema modification, when introspection runs
  again, then the GraphQL type definitions reflect the change.
- [ ] **US-049-AC4** — Given collection metadata queries, when Ava inspects a
  collection, then policy envelopes, redactable fields, approval-routed
  operations, schema/policy versions, and audit cursor support are exposed.
- [ ] **US-049-AC5** — Given development mode, when Ava opens the GraphQL
  endpoint in a browser, then an interactive playground is available.
- [ ] **US-049-AC6** — Given 20 registered collections, when the full
  introspection query runs, then it completes in under 100ms.

## Edge Cases

- **Collection without a schema**: It is reachable through the generic
  entity queries and is absent from typed introspection rather than
  producing a broken type.
- **Schema swap mid-introspection**: Introspection reflects one consistent
  schema version, never a mix.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Full discovery | US-049-AC1 | 3 registered collections | Run introspection | Types for all 3 collections present |
| Live registration | US-049-AC2 | Add `invoices` collection | Re-run introspection | `invoices` types present |
| Schema update | US-049-AC3 | Add field `dueDate` to schema | Re-run introspection | Field appears on the generated type |
| Metadata envelope | US-049-AC4 | Collection with redaction + approval policy | Query collection metadata | Envelope fields all present |
| Introspection latency | US-049-AC6 | 20 collections, 100 fields | Time the introspection query | < 100ms |

## Dependencies

- **Stories**: None
- **Feature Spec**: FEAT-015
- **Feature Requirements**: GQL-01, GQL-02, GQL-04
- **PRD Requirements**: FR-20
- **External**: CONTRACT-002 (GraphQL surface), CONTRACT-010 (ESF schema
  format)

## Out of Scope

- MCP discovery parity (US-052).
- Schema authoring and validation workflows (FEAT-002).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
