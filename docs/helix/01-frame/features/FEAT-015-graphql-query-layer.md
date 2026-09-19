---
ddx:
  id: FEAT-015
  depends_on:
    - helix.prd
  review:
    self_hash: c75ebd606ba19b7ac509eefcd0bb47c229433b5a14b1110fcae70d6c3898bd6f
    deps:
      helix.prd: 6703170c71275bba7d108c4f9c329d32e4104f9c965278db888ad43cdc3ca367
    reviewed_at: "2026-07-11T04:03:36Z"
---
# Feature Specification: FEAT-015 — GraphQL Query Layer

**Feature ID**: FEAT-015
**Status**: draft
**Priority**: P0
**Owner**: Core Team
**Requirement Prefix**: GQL
**Covered PRD Subsystem(s)**: API and Deployment Surfaces
**Covered PRD Requirements**: FR-20; contributes the GraphQL leg of FR-12/FR-13 (parity, leak safety), FR-28 (governed writes), and FR-31 (subscription cursors)
**Cross-Subsystem Rationale**: None — single subsystem.

## Overview

A full read/write GraphQL API auto-generated from Entity Schema Format (ESF)
declarations. Entity types, relationship fields, filter/sort inputs,
mutations, policy metadata, mutation-intent workflows, and Relay-style
pagination are derived from the active collection schemas at runtime.
WebSocket subscriptions provide real-time change feeds backed by the audit
log. This feature implements PRD FR-20.

GraphQL is Axon's primary application API surface. MCP (FEAT-016) mirrors the
same semantics for agents. REST/JSON endpoints remain compatibility and
operational fallbacks for cases where GraphQL is genuinely intractable.

GraphQL also carries the first-class discoverability contract for application
developers: generated types and metadata expose schema shape, policy
envelopes, redactions, approval requirements, stale/conflict causes, and
audit references that MCP, SDK, CLI, and operator UI surfaces must preserve.
GraphQL additionally hosts the surface of the unified read query language:
the ad-hoc Cypher entry point and the fields generated from schema-declared
named queries, planned by
[FEAT-009 — Unified Graph Query (Cypher)](FEAT-009-unified-graph-query.md).

See [ADR-012](../../02-design/adr/ADR-012-graphql-query-layer.md) for the
design and
[CONTRACT-002](../../02-design/contracts/CONTRACT-002-graphql-surface.md) for
the normative surface.

## Ideal Future State

A developer points standard GraphQL tooling at Axon and sees a typed,
introspectable API for every registered collection — types, relationships,
filters, mutations, subscriptions, policy metadata — without writing a line
of SDL. Reads, including ad-hoc Cypher and named queries, return only what
policy allows, with no existence leaks through traversal, counts, or
pagination. Writes default to the governed path: a risky mutation comes back
as an approval-required preview rather than a silent commit. Clients resume
change subscriptions losslessly after a disconnect using audit cursors, and
linked-data consumers can negotiate JSON-LD without affecting plain-JSON
clients.

## Problem Statement

- **Current situation**: Agents and the admin UI need entities with their
  relationships in a single request; an endpoint-per-operation API requires
  multiple calls to traverse links and assemble related data, and clients
  must poll the audit log for changes.
- **Pain points**: Multi-round-trip assembly is slow and error-prone;
  hand-built read layers reimplement policy filtering and leak hidden data
  through counts, nulls, and pagination; there is no push-based change
  notification.
- **Desired outcome**: One generated, policy-enforced GraphQL surface for
  declarative reads with nested relationship resolution, governed mutations,
  and subscriptions — with resolver behavior under redaction, row filtering,
  traversal, and pagination proven as a V1 quality gate.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Schema generation | "Does the API reflect my current schemas?" | Generate and atomically swap GraphQL types from ESF |
| Queries and pagination | "How do I fetch and page through entities?" | Typed/generic queries, filters, sorts, Relay connections, audit queries |
| Relationship resolution | "Can I fetch related entities in one request, safely?" | Forward/reverse relationship fields with policy-safe traversal |
| Unified query hosting | "How do I run Cypher reads and named queries?" | Host the ad-hoc query field and named-query/subscription field generation for the FEAT-009 planner |
| Mutations and governed writes | "How do I change data through GraphQL?" | Generated CRUD/link/lifecycle/transaction mutations with OCC and safe-write defaults |
| Policy and mutation intents | "What am I allowed to do, and how do I preview it?" | Effective-policy, explanation, preview, approval, and intent-commit fields |
| Subscriptions | "How do I react to changes without polling?" | Change-feed subscriptions with resumable audit cursors |
| Content negotiation | "Can linked-data clients consume Axon directly?" | JSON-LD rendering on request without changing the default JSON surface |

## Requirements

### Functional Requirements by Area

#### Schema Generation

- **GQL-01**: Each registered collection must produce GraphQL types
  automatically from its ESF declaration; no hand-written `.graphql` files.
  The JSON-Schema-to-GraphQL type mapping and naming determinism rules are
  normative in CONTRACT-002.
- **GQL-02**: When a schema is written, the GraphQL schema must be
  regenerated and swapped atomically; in-flight operations complete against
  the schema version active when they started.
- **GQL-03**: Every generated entity type must include the system fields for
  identity, version, timestamps, and actor attribution (exact field set per
  CONTRACT-002).
- **GQL-04**: Generated schema metadata must expose policy version, schema
  version, redactable fields, approval-routed operations, autonomous write
  envelopes, and supported audit/change cursor fields for each collection.

#### Queries and Pagination

- **GQL-05**: GraphQL must expose per-collection typed queries, generic
  entity queries for schemaless access, collection introspection
  (metadata, schema, indexes, lifecycles), and audit-log queries with
  cursor-based pagination and resume (field names and shapes per
  CONTRACT-002).
- **GQL-06**: All list fields must return Relay-style connections with
  edges, page info, and total count; row policies are applied before edges,
  cursors, and totals are constructed.
- **GQL-07**: Filter and sort inputs must be generated per entity type,
  covering indexed fields (FEAT-013) and non-indexed fields (scan
  fallback); the operator set and compound filter forms are normative in
  CONTRACT-002.

#### Relationship Resolution

- **GQL-08**: Each schema-declared link type must produce a forward
  relationship field and an auto-generated reverse field, both accepting
  filter arguments (naming per CONTRACT-002).
- **GQL-09**: Relationship fields must omit hidden target entities rather
  than returning policy errors; relationship predicates may reuse the target
  collection's read policy without duplicating membership rules; totals
  never include hidden rows; and policy denials for hidden rows are
  indistinguishable from not-found/null results wherever existence would
  otherwise leak.

#### Unified Query Hosting

- **GQL-10**: GraphQL must host the ad-hoc `axonQuery` entry point for the
  unified read-only Cypher language planned by FEAT-009. Parsing, planning,
  limits, error codes, and policy enforcement are owned by the unified
  planner; the language and field shape are normative in
  [CONTRACT-007](../../02-design/contracts/CONTRACT-007-cypher-query-surface.md)
  and the GraphQL field surface in CONTRACT-002.
- **GQL-11**: GraphQL must generate one typed query field for each
  schema-declared named query (FEAT-009), with connection-shaped,
  policy-filtered results, and must make named queries subscribable through
  the subscription path with an initial snapshot on subscribe. Ad-hoc
  `axonQuery` is not subscribable. Generated field shape per
  CONTRACT-007/CONTRACT-002.

#### Mutations and Governed Writes

- **GQL-12**: GraphQL must generate per-collection entity CRUD mutations,
  link mutations, lifecycle-transition mutations, an atomic multi-operation
  transaction mutation, and collection/schema management mutations (names,
  inputs, and payloads per CONTRACT-002).
- **GQL-13**: Update, patch, and delete mutations must require an expected
  version; version conflicts return a structured error carrying the current
  entity state (error extension codes per CONTRACT-002). Invalid lifecycle
  transitions return the valid target states.
- **GQL-14**: A direct mutation that policy classifies as needing approval
  must return an approval-required result and must not commit; generated
  write documentation and SDK generation prefer the preview-plus-intent flow
  for approval-routed operations.

#### Policy and Mutation Intents

- **GQL-15**: GraphQL must expose effective-policy and policy-explanation
  queries, mutation preview, and the approve/reject/commit intent workflow
  (FEAT-029/FEAT-030 semantics; field shapes per CONTRACT-002), with enough
  envelope metadata for SDKs and UIs to distinguish autonomous,
  approval-routed, and denied operations before attempting a commit.
- **GQL-16**: Any field that can be redacted by policy must be nullable in
  the generated GraphQL type, even if required in ESF, and must resolve to
  null when denied.
- **GQL-17**: Preview, stale, conflict, approval-required, and committed
  responses must expose stable machine-readable fields that SDKs and MCP
  tools preserve (extension codes per CONTRACT-002).

#### Subscriptions (Change Feeds)

- **GQL-18**: GraphQL must expose per-collection and generic change
  subscriptions whose events carry the mutation type, entity data, previous
  version, actor, timestamp, audit cursor, and transaction ID; the WebSocket
  transport, subprotocol, and event shapes are normative in CONTRACT-002.
- **GQL-19**: Subscription events must carry the audit cursor needed to
  resume through the audit-log query after disconnect.

#### Content Negotiation (JSON-LD)

- **GQL-20**: When a client requests JSON-LD via content negotiation, entity
  payloads must render as JSON-LD with a generated context, the canonical
  dereferenceable entity URL as the node identifier, and a type derived from
  the collection schema; linked entities render as identifier-bearing nested
  nodes; output validates against a JSON-LD 1.1 processor. The default JSON
  response shape is unchanged when JSON-LD is not requested. Field names
  colliding with JSON-LD reserved keywords are remapped via context aliases,
  with a schema-write-time warning (FEAT-002).

### Non-Functional Requirements

- **Schema generation**: < 1ms for 20 collections with 100 total fields.
- **Query latency**: GraphQL overhead < 2ms above the underlying Axon
  operation latency.
- **Relationship batching**: Resolving a relationship field for a page of N
  parent entities must issue a bounded number of batched lookups, not N
  per-parent lookups.
- **Request limits**: Query depth and complexity limits must be enforced
  before resolver execution, with operator-configurable bounds (defaults per
  CONTRACT-002).
- **Policy correctness**: Policy filtering, redaction, relationship
  traversal, and pagination must be tested against realistic business
  schemas before V1.
- **Interface parity**: Generated GraphQL metadata must match MCP tool
  metadata for policy envelopes, approval requirements, redactions,
  stale/conflict fields, and audit references.
- **Subscription latency**: < 500ms from entity write to subscriber
  notification.
- **JSON-LD**: No measurable performance regression on the plain-JSON path.

## User Stories

- [US-048 — Query Entities with Relationships](../user-stories/US-048-query-entities-with-relationships.md)
- [US-049 — Discover the API via Introspection](../user-stories/US-049-discover-the-api-via-introspection.md)
- [US-050 — Subscribe to Entity Changes](../user-stories/US-050-subscribe-to-entity-changes.md)
- [US-051 — Use GraphQL from the Admin UI](../user-stories/US-051-use-graphql-from-the-admin-ui.md)
- [US-057 — Mutate Entities via GraphQL](../user-stories/US-057-mutate-entities-via-graphql.md)
- [US-078 — JSON-LD Content Negotiation](../user-stories/US-078-json-ld-content-negotiation.md)
- [US-110 — Enforce Policy Across GraphQL Traversal](../user-stories/US-110-enforce-policy-across-graphql-traversal.md)
- [US-111 — Preview And Commit Mutation Intents](../user-stories/US-111-preview-and-commit-mutation-intents.md)

## Edge Cases and Error Handling

- **Empty collection**: The GraphQL type is generated; queries return empty
  connections.
- **Schema with no link types**: The entity type has only scalar fields, no
  relationship fields.
- **Collection with no schema**: Served by the generic entity queries
  returning JSON; no typed query is generated.
- **Deeply nested query**: The depth limit rejects queries exceeding the
  maximum with a clear error before execution.
- **Subscription to dropped collection**: The subscription ends with an
  error event; the client must resubscribe.
- **Concurrent schema change during query**: In-flight queries use the
  schema version active when the query started; no mid-query schema change.
- **Large result sets**: Pagination is mandatory for list fields; a default
  limit applies when none is specified.
- **Policy changes during query**: In-flight queries use the policy snapshot
  active when execution starts.
- **Policy changes during intent approval**: The intent is marked stale and
  requires a new preview (FEAT-030).

## Success Metrics

- 100% of registered collections are discoverable through introspection with
  types, relationships, filters, mutations, and policy metadata.
- Zero data-leak findings (hidden rows, counts, existence, redacted fields)
  in the traversal/pagination/aggregation policy fixture suite.
- 100% metadata and decision parity with MCP on the shared parity fixture
  suite.
- Subscription consumers resume losslessly via audit cursors in 100% of
  reconnect fixture scenarios.

## Constraints and Assumptions

- GraphQL is the primary documented application surface; MCP mirrors it and
  must never receive richer semantics than GraphQL exposes.
- All generated fields derive from ESF and the compiled policy plan; no
  user-defined resolvers in V1.
- The read-side query language is read-only (PRD non-goal: no writable
  Cypher/SQL); writes flow only through generated mutations and the intent
  workflow.
- JSON-LD adoption is additive: the canonical surface remains plain JSON.

## Dependencies

- **Other features**:
  - FEAT-002 (Schema Engine) — ESF is the source for schema generation
  - FEAT-004 (Entity Operations) — resolvers delegate to entity operations
  - FEAT-005 (API Surface) — GraphQL is served by the shared handler
    foundation
  - [FEAT-009 (Unified Graph Query (Cypher))](FEAT-009-unified-graph-query.md)
    — the unified read planner behind relationship traversal, `axonQuery`,
    and named-query fields
  - FEAT-013 (Secondary Indexes) — filter arguments route through the
    index-aware planner
  - FEAT-029 (Access Control) — row filters, field redaction, policy
    explanation, and safe pagination
  - FEAT-030 (Mutation Intents and Approval) — preview, approval, and intent
    commit workflows
- **External services**: None. Normative surface lives in CONTRACT-002
  (GraphQL) and CONTRACT-007 (Cypher query surface); ADR-012 records the
  design.
- **PRD requirements**: FR-20 (P0); contributes to FR-12, FR-13, FR-28,
  FR-31

## Out of Scope

- **Schema stitching / federation**: Single Axon instance only.
- **Persisted queries**: Client-sent query strings only; no server-side
  query storage.
- **Custom resolvers / computed fields**: All fields derive from ESF; no
  user-defined resolvers.
- **Cypher language and planner semantics**: Owned by FEAT-009 and
  CONTRACT-007 — FEAT-015 hosts the GraphQL surface only.
- **SQL integration**: SQL query frontend (not scheduled).
- **Vector similarity filter**: Semantic-search filtering (not scheduled).
- **Full-text filter**: Document-search filtering (not scheduled).

## Review Checklist

Use this checklist when reviewing a feature specification:

- [ ] Covered PRD Subsystem(s) and Requirements (`FR-n`) are listed; a feature spanning >1 subsystem carries an explicit cross-subsystem rationale (else split per the Decomposition test)
- [ ] Functional areas (if any) are subordinate parts of this one capability, not separate capabilities (each fails the ship/cut/metric test on its own)
- [ ] Overview connects this feature to a specific PRD requirement
- [ ] Ideal future state describes the desired user-visible outcome, not only current problems
- [ ] Problem statement describes what exists now and what is broken — not just what is wanted
- [ ] Functional areas are mapped when the feature spans multiple surfaces, workflows, or domain objects
- [ ] Requirements are grouped by functional area when a flat list would mix unrelated scopes
- [ ] Domain objects that sound similar are explicitly separated (for example, artifact instances vs artifact types)
- [ ] Every functional requirement is testable — you can write an assertion for it
- [ ] Acceptance criteria are defined in the user stories that decompose this feature, not here (ADR-009)
- [ ] Non-functional requirements have specific numeric targets, not "must be fast"
- [ ] Edge cases cover realistic failure scenarios, not just happy paths
- [ ] Success metrics are specific to this feature, not product-level metrics
- [ ] Dependencies reference real artifact IDs (FEAT-XXX, external APIs)
- [ ] Out of scope excludes things someone might reasonably assume are in scope
- [ ] No implementation details ("use X library", "create Y table") — specify WHAT not HOW
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
- [ ] Feature is consistent with governing PRD requirements
- [ ] No `[NEEDS CLARIFICATION]` markers remain unresolved for P0 features
