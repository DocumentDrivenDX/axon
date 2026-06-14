---
ddx:
  id: FEAT-007
  depends_on:
    - helix.prd
  review:
    self_hash: 730a71d71ea4d398f55a2a62b9bf812fc10290809796f4fab4e8ba1b50d53849
    deps:
      helix.prd: dff98156a6cc934f406611b78b513892d85cee1bd7b4c011f045146fcdfd23e1
    reviewed_at: "2026-06-14T04:39:42Z"
---
# Feature Specification: FEAT-007 — Entity-Graph Data Model

**Feature ID**: FEAT-007
**Status**: draft
**Priority**: P0
**Owner**: Core Team
**Covered PRD Subsystem(s)**: Entity-Graph Data Model
**Covered PRD Requirements**: FR-2; FR-1 (entity model shape — the operation surface is owned by FEAT-004)
**Cross-Subsystem Rationale**: None — single subsystem.
**Requirement Prefix**: GRF

## Overview

The entity-graph data model is Axon's foundational abstraction. Entities are deeply nested, schema-validated structures representing real-world objects. Links are typed, directional, first-class relationships between entities, with their own metadata, versioning, and audit records. This feature implements PRD FR-2 (typed links as first-class objects) and defines the entity model shape behind FR-1.

This model avoids the false choice between documents (rich but isolated), graphs (connected but schema-loose), and tables (structured but flat).

**Boundary note**: this feature owns the *data model* — what entities and links are. All read paths over that model (traversal, neighbor discovery, filtering, sorting, aggregation, pattern matching) are owned by FEAT-009's unified graph query. Single-entity operations are owned by FEAT-004.

## Ideal Future State

A developer models business objects and relationships once — invoices, vendors, beads, approval chains — with deep nesting where the domain has depth and typed links where the domain has relationships. Both sides of the model carry schema guarantees and audit lineage, so the graph is trustworthy: a link cannot point at a record that never existed, link metadata conforms to its declared shape, and every relationship change is attributable.

## Problem Statement

- **Current situation**: Developers model entities as rows or documents and relationships as foreign keys, join tables, or ad-hoc reference fields.
- **Pain points**: Flat schemas can't express deep nesting. Foreign keys can't express typed, directional relationships with metadata. Graph databases lack schema enforcement and audit lineage.
- **Desired outcome**: A data model where entities have depth, links have types and metadata, and both are schema-validated, versioned, and audited.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Entity model | "What shape can a record take?" | Nested structure, identity, versioning, schema binding |
| Link model | "How do records relate?" | Typed, directional, metadata-bearing first-class links |
| Link lifecycle | "Create and remove relationships safely" | Referentially-checked, schema-validated, audited link mutations |

## Requirements

### Functional Requirements by Area

#### Entity Model

- **GRF-01**. Entities must support deeply nested JSON-like structures: objects, arrays, and primitive values at arbitrary depth, with nested fields individually addressable.
- **GRF-02**. Every entity must have an immutable identity unique within its collection and a monotonically increasing version. Identity, version, and the rest of the server-managed envelope are specified by FEAT-004 and [CONTRACT-001 §Entity system-metadata envelope](../../02-design/contracts/CONTRACT-001-http-api-surface.md); this feature does not redefine them.
- **GRF-03**. Every entity must belong to a collection and validate against that collection's active schema on every write, including required fields inside nested objects.
- **GRF-04**. Entity schemas must support recursive types (for example, a tree node containing child nodes of the same type).

#### Link Model

- **GRF-05**. A link must be a first-class record connecting a source entity reference to a target entity reference with a named link type, optional metadata properties, and its own system metadata, version, and audit records. The normative link record shape is defined in [CONTRACT-001 — HTTP API Surface](../../02-design/contracts/CONTRACT-001-http-api-surface.md) (link operations and envelope) and the link-type declaration grammar in [CONTRACT-010 — ESF Schema Format](../../02-design/contracts/CONTRACT-010-esf-schema-format.md) (`link_types`).
- **GRF-06**. Links must be directional (source → target); bidirectional relationships are modeled as two links or queried in both directions by the read model.
- **GRF-07**. Links must be able to connect entities in different collections.
- **GRF-08**. Link types must be declared in the schema, with per-link-type target collection, cardinality, and metadata schema (per CONTRACT-010); link metadata must validate against that schema on write.
- **GRF-09**. A given (source, target, link type) triple must be unique; creating a duplicate is rejected as a conflict. Different link types between the same pair are allowed.

#### Link Lifecycle

- **GRF-10**. Creating a link must verify that both source and target entities exist and that the link type is declared; failures identify the missing entity or undeclared type.
- **GRF-11**. Deleting a link must produce an audit record capturing the deleted link.
- **GRF-12**. Entity deletion must honor the dangling-link policy: deletion is rejected while inbound links exist, with an explicit force option that cascade-deletes the links (all audited).

### Non-Functional Requirements

- **Performance**: Entity writes with 5+ levels of nesting validate and commit within the FEAT-004 p99 < 10 ms single-entity budget; link create/delete meets the same p99 < 10 ms budget.
- **Scalability**: The model supports collections of thousands to low millions of entities and links per database; it is not designed for warehouse-scale analytics.
- **Reliability**: Link referential checks and uniqueness are enforced atomically with the link write — no window in which a duplicate or dangling-at-create link is observable.

## User Stories

| ID | Title | Link |
|----|-------|------|
| US-017 | Model Entities with Nested Structure | [US-017](../user-stories/US-017-model-entities-with-nested-structure.md) |
| US-018 | Create and Traverse Links | [US-018](../user-stories/US-018-create-and-traverse-links.md) |
| US-019 | Query Across Entity-Link Graph | [US-019](../user-stories/US-019-query-across-entity-link-graph.md) |

US-018 and US-019 exercise this feature's link model through the read paths
owned by FEAT-009; their traversal/query semantics are governed by FEAT-009
and [CONTRACT-007 — Cypher Query Surface](../../02-design/contracts/CONTRACT-007-cypher-query-surface.md).

## Edge Cases and Error Handling

- **Dangling links**: Deleting an entity with inbound links is rejected; an explicit force option cascade-deletes the links, and every cascaded deletion is audited.
- **Circular links**: `depends-on` from A→B and B→A is allowed at the link level; circularity detection is application-level, except for link types that opt into acyclicity in their declaration.
- **Cross-collection link integrity**: Link creation validates that both source and target exist at creation time; later deletions are handled by the dangling-link policy.
- **Link type not declared**: Creating a link with an undeclared link type fails with a validation error naming the type.
- **Metadata schema violation**: Link metadata that does not validate against the link type's metadata schema is rejected with field-level errors.

## Success Metrics

- Entity writes with deep nesting and link create/delete meet the p99 < 10 ms single-record latency budget.
- Zero referential-integrity violations (dangling-at-create or duplicate-triple links) under concurrency contract tests.
- The model expresses the reference domains — bead dependency graphs, document authorship, account hierarchies, invoice approval chains — without ad-hoc reference fields.

## Constraints and Assumptions

### Constraints

- Entities and links are always schema-bound; there are no schemaless records.
- Links are directional; bidirectional relationships require explicit modeling.
- The model targets operational graphs (thousands to low millions of records), not massive analytical graphs.

### Assumptions

- Most entity graphs in agentic applications have fewer than 100,000 nodes.
- The entity-graph model maps naturally to bead DAGs, org hierarchies, document relationships, and workflow chains.

## Dependencies

- **Other features**: FEAT-002 (Schema Engine — entity and link-type schema declarations), FEAT-003 (Audit Log — entity and link mutations audited), FEAT-004 (Entity Operations — operation surface and system-metadata envelope), FEAT-009 (Unified Graph Query — all read paths over this model).
- **External services**: None. Normative surfaces: [CONTRACT-001 — HTTP API Surface](../../02-design/contracts/CONTRACT-001-http-api-surface.md) (link operations, envelope), [CONTRACT-010 — ESF Schema Format](../../02-design/contracts/CONTRACT-010-esf-schema-format.md) (`link_types` declaration grammar).
- **PRD requirements**: FR-2 (P0); FR-1 model shape (P0).

## Out of Scope

- All traversal, neighbor, filter, sort, aggregation, and pattern-match read paths — owned by FEAT-009 (Unified Graph Query).
- Single-entity CRUD semantics and the system-metadata envelope — owned by FEAT-004.
- Graph visualization.
- Link inference / reasoning (OWL-style).
- Weighted shortest-path and other graph algorithms.

## Review Checklist

Use this checklist when reviewing a feature specification:

- [ ] Covered PRD Subsystem(s) and Requirements (`FR-n`) are listed; a feature spanning >1 subsystem carries an explicit cross-subsystem rationale (else split per the Decomposition test)
- [ ] Functional areas (if any) are subordinate parts of this one capability, not separate capabilities
- [ ] Overview connects this feature to a specific PRD requirement
- [ ] Ideal future state describes the desired user-visible outcome, not only current problems
- [ ] Every functional requirement is testable — you can write an assertion for it
- [ ] Acceptance criteria are defined in the user stories that decompose this feature, not here (ADR-009)
- [ ] Non-functional requirements have specific numeric targets
- [ ] Edge cases cover realistic failure scenarios, not just happy paths
- [ ] Success metrics are specific to this feature, not product-level metrics
- [ ] Dependencies reference real artifact IDs
- [ ] Out of scope excludes things someone might reasonably assume are in scope
- [ ] No exact API/CLI/event/schema/config surface is defined inline; normative surface links to Contract artifacts
