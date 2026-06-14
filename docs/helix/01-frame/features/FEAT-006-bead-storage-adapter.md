---
ddx:
  id: FEAT-006
  depends_on:
    - helix.prd
  review:
    self_hash: decac5fea0f1d97b7d7f502aada0c9733e10f1771a07560c413ae358e2be4c4f
    deps:
      helix.prd: d87a9cbc61d7abb53d32d8c675cc74c63fd9502e953c0ebee44285efde51df1f
    reviewed_at: "2026-06-14T03:52:45Z"
---
# Feature Specification: FEAT-006 — Bead Storage Adapter

**Feature ID**: FEAT-006
**Status**: draft
**Priority**: P1
**Owner**: Core Team
**Requirement Prefix**: BED
**Covered PRD Subsystem(s)**: None directly — dogfooding extension layered on the Entity-Graph Data Model subsystem
**Covered PRD Requirements**: Dogfooding extension — no direct FR; sanctioned via the PRD's bead-workflow exemplar (PRD Risks: "Center docs and examples on invoice/procurement and bead workflows"). Built entirely on capabilities owned by FEAT-001/FEAT-004 (FR-1), FEAT-007 (FR-2), and FEAT-010 lifecycle enforcement.
**Cross-Subsystem Rationale**: None — single capability; an opinionated module over existing subsystem primitives, owning no PRD subsystem of its own.

## Overview

The bead storage adapter is a purpose-built collection schema and query layer for storing beads — the portable work items used by agentic frameworks such as steveyegge/beads and DDx. It provides a pre-defined schema, lifecycle state management, dependency tracking, and ready-queue queries. It is Axon's first "opinionated module": a domain-specific layer built entirely on the generic collection, entity, link, and lifecycle primitives, and the primary dogfooding vehicle — Axon's own issue tracker (DDx beads) must be able to live in Axon.

## Ideal Future State

An agent framework points its bead tracker at Axon and gets durable, audited, queryable work-item storage without writing any schema or storage code. A DDx deployment imports its existing `beads.jsonl`, operates against Axon as its tracker backend, and exports at any time with zero data loss — every field DDx ever wrote comes back unchanged. Agents ask one question — "what is ready to work on?" — and get a correct, dependency-aware answer in milliseconds, with every lifecycle change validated and recorded in the audit trail.

## Problem Statement

- **Current situation**: Every agentic framework that uses beads (or bead-like work items) reinvents storage: file-based JSONL, SQLite, custom CRUD APIs. Axon's own development runs on the DDx bead tracker backed by a flat `beads.jsonl` file.
- **Pain points**: The bead data model — lifecycle states, dependency DAGs, ready-queue semantics — is rebuilt per framework, with no validation, no audit trail, and no concurrent-safe mutation. Flat-file trackers cannot answer dependency-aware queries efficiently or safely under concurrent agents.
- **Desired outcome**: A bead collection that works out of the box, mirrors the DDx lifecycle model exactly (as a superset), and round-trips DDx data with 100% field fidelity.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Schema and compatibility | "Can my existing bead data live here unchanged?" | Pre-defined bead schema that accepts every field DDx writes, plus open extension metadata |
| Lifecycle | "Which status changes are legal for this bead?" | DDx-superset status vocabulary with validated transitions, enforced via FEAT-010 |
| Dependencies and ready queue | "What can I work on right now?" | Typed bead dependencies, cycle rejection, and a derived ready-queue predicate |
| Queries | "Show me beads by status, type, owner, label" | Bead-specific query patterns over standard entity queries |
| Import/export | "Can I get my data in and out losslessly?" | JSONL import/export compatible with DDx and steveyegge/beads formats |

## Requirements

### Functional Requirements by Area

#### Schema and Compatibility

- **BED-01**: Axon must provide a built-in bead collection schema that accepts every field the DDx bead tracker writes (source: DDx `beads.jsonl`, `schema_version` 1), including at minimum: id, title, description, acceptance, status, issue type, labels, priority, owner, assignee, parent, dependencies, notes, timestamps, and arbitrary custom fields. Fields the schema does not model explicitly must be preserved as open extension metadata, not dropped.
- **BED-02**: The bead schema must be declared in the standard Entity Schema Format; the normative schema and lifecycle declaration grammar is owned by CONTRACT-010 (ESF schema format).

#### Lifecycle

- **BED-03**: The bead status vocabulary must be a superset of the DDx bead lifecycle as implemented by the DDx tracker (source: `ddx bead` lifecycle validation and the `bead-lifecycle` v1 schema marker): stored states `proposed`, `open`, `in_progress`, `blocked`, `closed`, `cancelled`, with `closed` and `cancelled` terminal.
- **BED-04**: Bead status transitions must be validated against the declared lifecycle (enforced by the FEAT-010 entity state machine mechanism). Invalid transitions are rejected with a structured error listing the valid next states from the current state.
- **BED-05**: Ordinary status updates must not move a bead out of a terminal state; an explicit reopen operation is the only path out of `closed` (mirroring DDx `reopen` semantics).
- **BED-06**: "Ready" must be a derived queue predicate, not a stored status: a bead is ready when its status is `open` and every blocking dependency has reached `closed` (mirroring DDx ready-queue semantics).

#### Dependencies and Ready Queue

- **BED-07**: Beads must be able to declare typed dependencies on other beads (DDx dependency type `blocks` at minimum). The dependency graph is queryable, including the dependency tree of a single bead.
- **BED-08**: Creating or adding a dependency that would form a cycle must be rejected with a cycle-detection error identifying the cycle.
- **BED-09**: Creating a bead with a dependency referencing a bead that does not exist must fail with a validation error (import is the exception — see Edge Cases).
- **BED-10**: Axon must answer the ready-queue query: all beads satisfying the BED-06 predicate.

#### Queries

- **BED-11**: Axon must support bead queries by status, issue type, owner/assignee, label, parent, and dependency state, composed from the standard entity query model.

#### Import/Export

- **BED-12**: Axon must import beads from JSONL files produced by the DDx tracker and the steveyegge/beads format, and export the collection back to the same format.
- **BED-13**: Bead CLI commands and API endpoints are exposed through the standard surfaces; the normative command tree is owned by CONTRACT-008 (CLI and config) and the normative HTTP routes by CONTRACT-001 (HTTP API surface).

### Non-Functional Requirements

- **Compatibility (superset, not subset)**: Importing a DDx-produced `beads.jsonl` and exporting it again must round-trip every field DDx writes unchanged — 100% field-level fidelity, including fields the schema does not model explicitly (claim metadata, execution evidence, custom `--set` fields). Verified by a round-trip conformance test against a real DDx tracker file.
- **Performance**: Ready-queue query completes in < 50ms for collections with < 10,000 beads.

## User Stories

| ID | Title | Link |
|----|-------|------|
| US-015 | Store and Query Beads | [US-015](../user-stories/US-015-store-and-query-beads.md) |
| US-016 | Track Bead Dependencies | [US-016](../user-stories/US-016-track-bead-dependencies.md) |

## Edge Cases and Error Handling

- **Import with unmodeled fields**: Fields the schema does not model explicitly are preserved as extension metadata and survive export unchanged — never silently dropped.
- **Import with unresolved dependencies**: A DDx file may reference beads that were archived out of the file. Import tolerates dangling dependency references and flags them as unresolved rather than rejecting the import.
- **Import with a status outside the vocabulary**: Rejected with a validation error listing the accepted status vocabulary.
- **Transition from a terminal state**: A status update on a `closed` or `cancelled` bead is rejected with a transition-from-terminal error; only the explicit reopen operation succeeds for `closed`.
- **Concurrent claim**: Two agents claim the same `open` bead concurrently; optimistic concurrency ensures exactly one transition to `in_progress` succeeds and the other receives a version conflict.
- **Cycle via dependency edit**: Adding a dependency that closes a cycle (A→B→C→A) is rejected; the dependency graph is unchanged.

## Success Metrics

- A real DDx `beads.jsonl` (1,000+ beads) round-trips through import/export with 100% field-level fidelity in the conformance test.
- The DDx tracker workflow (create, claim, block, close, reopen, ready-queue) runs against an Axon-backed bead collection with no behavioral divergence from the file-backed tracker.
- An agent framework adopts the bead module without writing any schema definition code.

## Constraints and Assumptions

### Constraints

- The DDx bead lifecycle model is the compatibility baseline; Axon may extend the vocabulary but must never narrow it or change DDx transition semantics.
- The bead collection is a regular collection: all standard entity, audit, and policy machinery applies — no bespoke storage path.

### Assumptions

- DDx remains the primary consumer and reference implementation for compatibility testing.
- Bead collections stay modest in size (tens of thousands of beads, not millions).

## Dependencies

- **Other features**: FEAT-001 (Collections — the bead collection is a regular collection with a pre-defined schema), FEAT-004 (Entity Operations — bead CRUD uses standard entity operations), FEAT-005 (API Surface — bead commands and endpoints ride the standard surfaces), FEAT-010 (Entity State Machines — lifecycle transition enforcement).
- **External services**: None. Normative interface surface: CONTRACT-001 (HTTP API), CONTRACT-008 (CLI and config), CONTRACT-010 (ESF schema format, lifecycle declarations).
- **PRD requirements**: No direct FR — dogfooding extension; builds on FR-1, FR-2, FR-3 capabilities owned by other features.

## Out of Scope

- Bead execution / agent dispatch — Axon stores work-item state; it does not run agents.
- Cross-Axon-instance bead sync.
- Bead visualization / UI.
- Replacing the DDx tracker's queue, lease, and cooldown orchestration — Axon stores the beads; DDx owns its execution machinery.

## Review Checklist

Use this checklist when reviewing this feature specification:

- [ ] Covered PRD Subsystem(s) and Requirements (`FR-n`) are listed; a feature spanning >1 subsystem carries an explicit cross-subsystem rationale (else split per the Decomposition test)
- [ ] Functional areas (if any) are subordinate parts of this one capability, not separate capabilities
- [ ] Overview connects this feature to a specific PRD requirement
- [ ] Ideal future state describes the desired user-visible outcome, not only current problems
- [ ] Problem statement describes what exists now and what is broken — not just what is wanted
- [ ] Every functional requirement is testable — you can write an assertion for it
- [ ] Acceptance criteria are defined in the user stories that decompose this feature, not here (ADR-009)
- [ ] Non-functional requirements have specific numeric targets
- [ ] Edge cases cover realistic failure scenarios, not just happy paths
- [ ] Success metrics are specific to this feature, not product-level metrics
- [ ] Dependencies reference real artifact IDs
- [ ] Out of scope excludes things someone might reasonably assume are in scope
- [ ] No implementation details — WHAT not HOW
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
- [ ] Feature is consistent with governing PRD requirements
