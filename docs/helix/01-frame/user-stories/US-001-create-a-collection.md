---
ddx:
  id: US-001
  review:
    self_hash: 52edbcef8447515773bb3f17f0a2253392903eef8ace6d2427b6469b383fd520
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# US-001: Create a Collection

**Feature**: FEAT-001 — Collections
**Feature Requirements**: COL-01, COL-03, COL-04, COL-07
**PRD Requirements**: FR-1
**Priority**: P0
**Status**: Draft

## Story

**As a** developer setting up an agentic application
**I want** to create a named collection bound to a schema
**So that** my agents have a structured, governed place to store entities

## Context

Before this works, a developer has no governed container to point agents at — data lands in ad-hoc storage with no schema binding or audit boundary. This story exercises COL-04 (schema-bound creation with audit), COL-01 (uniqueness within the schema namespace), COL-03 (structured naming errors), and COL-07 (mandatory schema binding). Collection management surfaces are defined by CONTRACT-008 (CLI) and CONTRACT-001 (HTTP API); the schema document format by CONTRACT-010.

## Walkthrough

1. Developer prepares a schema document in the Entity Schema Format (CONTRACT-010).
2. Developer issues a create-collection request via the CLI (CONTRACT-008) or HTTP API (CONTRACT-001), naming the collection and supplying the schema.
3. System validates the schema, checks the name against the naming rules, and checks uniqueness within the target schema namespace.
4. System creates the collection, records the creation in the audit log, and returns the collection metadata including its schema version.

## Acceptance Criteria

- [ ] **US-001-AC1** — Given a valid schema document and an unused collection name, when the developer creates the collection via the CLI or HTTP API (CONTRACT-008/CONTRACT-001), then the collection is created and its metadata (name, schema version) is returned.
- [ ] **US-001-AC2** — Given a collection with the same name already exists in the target schema namespace, when the developer attempts to create another with that name, then the request is rejected with a structured conflict error and no collection is created.
- [ ] **US-001-AC3** — Given a collection name that violates the naming rules, when the developer attempts to create it, then the request is rejected with a structured error naming the violated rule.
- [ ] **US-001-AC4** — Given a schema document that fails Entity Schema Format validation (CONTRACT-010), when the developer attempts to create a collection with it, then the request is rejected with the schema validation errors and no collection is created.
- [ ] **US-001-AC5** — Given a collection was just created, when the audit log is queried for that collection, then a creation record exists identifying the actor and the bound schema version.

## Edge Cases

- **Concurrent creation with the same name**: Exactly one create succeeds; the other receives a structured conflict error.
- **Same name in a different schema namespace or database**: Creation succeeds — uniqueness is scoped to the schema namespace; the fully qualified name is `database.schema.collection`.
- **Missing schema**: A create request without a schema is rejected (COL-07) — schemaless collections are not supported.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Happy path | US-001-AC1 | Valid `invoices` schema; no `invoices` collection in namespace | Create `invoices` with the schema | Collection created; metadata returned with schema version 1 |
| Duplicate name | US-001-AC2 | `invoices` already exists in the same schema namespace | Create `invoices` again | Structured conflict error; existing collection unchanged |
| Invalid name | US-001-AC3 | Name `9Invoices!` | Create collection | Structured validation error naming the violated naming rule |
| Invalid schema | US-001-AC4 | Schema with contradictory constraints | Create `invoices` with that schema | Rejected with schema validation errors; no collection created |
| Audit record | US-001-AC5 | Collection just created | Query audit log for the collection | Creation record present with actor and schema version |

## Dependencies

- **Stories**: None
- **Feature Spec**: FEAT-001
- **Feature Requirements**: COL-01, COL-03, COL-04, COL-07
- **PRD Requirements**: FR-1
- **External**: CONTRACT-001, CONTRACT-008, CONTRACT-010

## Out of Scope

- Schema content semantics and validation behavior (FEAT-002, US-004).
- Access control on who may create collections (FEAT-012/FEAT-029).
- Renaming and dropping collections (US-003 covers drop; rename is COL-05).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
