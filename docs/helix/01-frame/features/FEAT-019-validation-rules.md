---
ddx:
  id: FEAT-019
  depends_on:
    - helix.prd
  review:
    self_hash: ddf48d3192c435e1b9a40b2dc77ec60f363bfd91230e99fab336ebf4232785c4
    deps:
      helix.prd: d87a9cbc61d7abb53d32d8c675cc74c63fd9502e953c0ebee44285efde51df1f
    reviewed_at: "2026-06-14T03:52:45Z"
---
# Feature Specification: FEAT-019 — Validation Rules and Actionable Errors

**Feature ID**: FEAT-019
**Status**: draft
**Priority**: P1
**Owner**: Core Team
**Created**: 2026-04-05
**Updated**: 2026-06-10
**Requirement Prefix**: VAL
**Covered PRD Subsystem(s)**: Entity-Graph Data Model
**Covered PRD Requirements**: FR-1 (cross-field validation and gate readiness within active schema validation)
**Cross-Subsystem Rationale**: None — single subsystem.

## Overview

JSON Schema (ESF Layer 1) validates field types and basic constraints.
Validation rules (ESF Layer 5) go further: cross-field conditions,
conditional requirements, **validation gates**, and actionable error
messages that tell agents exactly what is wrong and how to fix it. This
implements the schema-validation dimension of PRD FR-1.

Validation gates are named checkpoints that group rules by purpose. The
`save` gate blocks persistence. Other gates (`complete`, `review`,
`processing`) allow saves but track readiness — an entity can be saved in
an incomplete state and progressively improved until it passes the
required gate for a lifecycle transition or workflow step. Each rule
carries exactly one of `gate` or `advisory: true`; advisory rules never
block and always report. There is no severity field.

This is Axon's key differentiator over general-purpose databases. A SQL
constraint violation says "CHECK constraint failed." An Axon validation
error says "Field 'approver_id' is required when status is 'approved'.
Current status: 'approved'. Set approver_id to a valid user ID." — and it
tells you which workflow gates the entity passes and which it doesn't.

## Ideal Future State

A developer declares business rules once, in the collection schema, and
every write surface enforces them identically. An agent that submits an
incomplete draft is never blocked from saving early; instead the write
response tells it exactly which gates the entity passes, which rules still
fail, and what concrete fix would resolve each failure. An operator asks
"show me orders ready for processing" and gets an indexed, fast answer
derived from materialized gate status rather than re-evaluating rules per
query. When a write is rejected, the error is complete (every violation in
one response) and self-correcting agents can repair their payload without
human help.

## Problem Statement

- **Current situation**: JSON Schema validates structure but not
  semantics. Real-world entities have cross-field dependencies — approved
  invoices need an approver, bugs need a priority, completed tasks should
  have a resolution — that cannot be expressed in JSON Schema alone.
- **Pain points**: Validation errors from JSON Schema libraries are
  technical and generic ("instance failed to match pattern"). Agents need
  errors that explain the business rule, identify the fix, and distinguish
  hard constraints from soft readiness checks. Hard validation that blocks
  saves on incomplete data is hostile to agentic workflows where entities
  are created early and progressively filled in.
- **Desired outcome**: Cross-field rules are declared in the schema,
  enforced on every write, reported with actionable fixes, and gate
  readiness is both returned on every write and queryable as a fast
  indexed filter.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Rule declaration | How do I express "approved items need an approver" in the schema? | Layer 5 rule grammar: condition, requirement, message, fix; exactly one of `gate` or `advisory: true` |
| Gate semantics and lifecycle integration | Can I save a draft now and finish it later? When may it transition? | Save-blocking vs readiness-tracking gates; gate inclusion; `requires_gate` on lifecycle transitions |
| Materialized gate status and querying | Which entities are ready for processing? | Gate pass/fail materialized on every write, returned in responses, queryable as an indexed filter on every read surface |
| Actionable errors | What exactly is wrong and how do I fix it? | Structured, complete error reporting with fix suggestions and near-match hints, including translated JSON Schema violations |
| Rule validation on schema save | How do I know my rules are well-formed? | Schema-save validation of rule names, fields, gate/advisory structure, operators, and regexes |

## Requirements

### Functional Requirements by Area

#### Rule Declaration (ESF Layer 5)

- **VAL-01**. Collection schemas must declare validation rules as ESF
  Layer 5, alongside the entity schema (L1), link types (L2), lifecycles
  (L3), and indexes (L4). Each rule must carry a unique name, exactly one
  of `gate` or `advisory: true`, an optional activation condition
  (`when`), a required constraint (`require`), a required human-readable
  `message`, and an optional actionable `fix` that may interpolate current
  field values. There is no severity field. The normative YAML grammar,
  rule structure, and field tables are defined in
  [CONTRACT-010 — ESF schema format](../../02-design/contracts/CONTRACT-010-esf-schema-format.md).
- **VAL-02**. Rules must support the condition and requirement operator
  set defined in CONTRACT-010, including equality, membership, null
  checks, numeric/date comparison, regex match, composite `all`/`any`
  conditions, and cross-field comparisons (`gt_field` and relatives).
- **VAL-03**. A rule without a `when` condition must always apply. A
  `when` condition referencing an absent field must evaluate to false
  (the rule does not fire), which is distinct from the field being null.

#### Gate Semantics and Lifecycle Integration

- **VAL-04**. A failing `save`-gate rule must block persistence: the
  entity is not written, no index is updated, and no mutation audit entry
  is produced for the attempted write.
- **VAL-05**. Failing custom-gate rules (`complete`, `review`, and other
  declared gates) must allow the save; the write response must report gate
  pass/fail and the failure details (rule, field, message, fix) for each
  failing gate.
- **VAL-06**. Advisory rules must never block a write; advisories must be
  reported in write responses and remain queryable like gate status.
- **VAL-07**. Gates must support explicit inclusion (`includes`), with
  the effective rule set computed as the transitive union per the gate
  declaration grammar in CONTRACT-010 (failing a `complete` rule also
  fails a `review` gate that includes `complete`).
- **VAL-08**. A lifecycle transition declaring `requires_gate` must be
  blocked while the entity fails that gate; the blocked-transition error
  must include the gate name, failing rules, and fix suggestions.
- **VAL-09**. Validation must run on every write path that persists
  entity payload data (create, update, patch), in the evaluation order
  defined by CONTRACT-010: JSON Schema (L1) first; Layer 5 only when L1
  passes; lifecycle validation (L3) independently; within Layer 5 all
  rules evaluated with no short-circuit and all findings collected. Rules
  on patch must evaluate against the merged result, not the patch
  document.
- **VAL-10**. Save gates are payload-only: rules must validate only
  fields in the submitted entity and must not branch on caller identity,
  tenant role, grant, or other request context. Subject-aware
  authorization belongs to FEAT-029 policies. Until FEAT-029 enforces an
  application-specific role field, browser-writable bootstrap schemas must
  be expressible with conservative payload gates that allow only safe
  default values (for example, a `users` save gate that accepts only
  `role: member`, so self-created rows cannot self-promote to admin on
  the browser-writable path).

#### Materialized Gate Status and Querying

- **VAL-11**. Gate definitions must be registered when a schema with
  validation rules is saved, and on every entity write Axon must evaluate
  all non-save gates and advisories and materialize per-entity, per-gate
  pass/fail status with failure details. Physical storage, table layout,
  and index design for gate status are governed by ADR-010 (gate tables).
- **VAL-12**. Gate status must be queryable through every read surface —
  structured API, GraphQL, Cypher read queries, and MCP — and gate
  filters must compose with ordinary field filters in one query. The
  exact filter surfaces are defined in
  [CONTRACT-002 — GraphQL surface](../../02-design/contracts/CONTRACT-002-graphql-surface.md)
  and
  [CONTRACT-007 — Cypher query surface](../../02-design/contracts/CONTRACT-007-cypher-query-surface.md).
- **VAL-13**. Entities with no gate evaluations (for example, collections
  without validation rules) must not match gate filters.
- **VAL-14**. Every entity response must include current gate status and
  advisories in the shape defined by CONTRACT-010, so an agent always
  knows what is still needed before the entity can proceed.
- **VAL-15**. When validation rules change on schema save, gate status
  for existing entities must be recomputed in the background; queries must
  not silently mix pre- and post-change semantics without the
  recomputation eventually converging.

#### Actionable Errors

- **VAL-16**. Every validation failure must produce a structured error
  identifying the rule, gate, field, message, fix, and the activating
  condition context; all failures and advisories must be reported in one
  response, not just the first. The normative `VALIDATION_FAILED`
  envelope is defined in CONTRACT-010.
- **VAL-17**. JSON Schema (L1) violations must be translated into the
  same actionable shape with generated fix suggestions, expected/actual
  type and value context, and near-match "did you mean?" hints for enum
  values and field names.

#### Rule Validation on Schema Save

- **VAL-18**. Saving a schema with validation rules must validate the
  rules themselves: unique rule names per collection, referenced fields
  exist in the entity schema, exactly one of `gate` or `advisory: true`
  per rule, gate values name `save` or a declared gate, `includes`
  references declared gates without cycles, non-empty messages, valid
  condition/requirement operators, valid regexes, and existing
  cross-field comparison targets — per the schema-save validation rules in
  CONTRACT-010. Invalid rules must be rejected at schema save, not at
  entity write time.

### Non-Functional Requirements

- **Rule evaluation latency**: < 1 ms for 20 rules on a typical entity
  (50 fields); rule evaluation must not dominate write latency.
- **Error generation**: < 0.5 ms to produce the enhanced error response,
  including fix suggestions and near-match detection.
- **Gate-filter query latency**: gate-status filters must use the
  materialized gate index; < 50 ms on 100K entities.
- **Memory**: rule definitions are cached per collection; rule evaluation
  must not require per-request rule parsing.

## User Stories

- [US-066 — Cross-Field Validation Rules](../user-stories/US-066-cross-field-validation-rules.md)
- [US-067 — Validation Gates](../user-stories/US-067-validation-gates.md)
- [US-068 — Actionable Error Messages](../user-stories/US-068-actionable-error-messages.md)
- [US-069 — Validate Rules on Schema Save](../user-stories/US-069-validate-rules-on-schema-save.md)
- [US-074b — Query by Gate Status](../user-stories/US-074b-query-by-gate-status.md)
  (legacy suffixed ID retained per the user-story ID registry)

## Edge Cases and Error Handling

- **Rule references optional field**: if the `when` condition references
  an absent field, the condition evaluates to false and the rule does not
  fire. This is distinct from the field being `null`.
- **Rule on nested field**: field paths support dot notation
  (`address.city`); if the parent object is absent, the rule does not
  fire.
- **Rule on array field**: `not_null` on an array checks that the array
  exists and is non-empty; comparison operators on arrays are not
  supported (use JSON Schema for array constraints).
- **Rule evaluation on patch**: rules evaluate against the merged result,
  not the patch document, so the agent sees findings for the full entity
  including fields it did not change.
- **Circular rule dependencies**: rules evaluate independently against
  entity data; rules cannot depend on each other, so no rule-level cycle
  is possible. Gate `includes` cycles are rejected at schema save
  (VAL-18).
- **Many rules**: 100+ rules on a single collection are unusual but
  supported; evaluation cost is linear in rule count.
- **Near-match suggestions**: "did you mean?" suggestions are computed
  only for enums with ≤ 20 options; larger enums skip suggestions.
- **Schema change while entities exist**: gate status becomes stale until
  background recomputation converges (VAL-15); gate filters reflect the
  most recent evaluation.

## Success Metrics

- 100% of writes that fail a save-gate rule leave no persisted entity and
  no mutation audit entry.
- An agent receiving a validation failure can repair the payload from the
  error response alone (rule, field, message, fix) without consulting the
  schema source — measured by the reference agent tutorial completing
  self-correction without human intervention.
- Gate-status filter queries return in < 50 ms on collections of 100K
  entities.
- 100% of malformed rule definitions are rejected at schema save rather
  than surfacing as entity-write failures.

## Constraints and Assumptions

- Validation rules are payload-only in V1: no caller-context branching,
  no cross-entity lookups, no external calls. Subject-aware enforcement
  is FEAT-029's job.
- Rules and gates are part of the ESF schema document and version with
  it; the grammar is closed and declarative (CONTRACT-010).
- Gate status is materialized state derived from rules; it is never
  authored directly by clients.
- Advisory findings are informational; no workflow may treat an advisory
  as blocking.

## Dependencies

- **Other features**: FEAT-002 (Schema Engine — rules extend the schema
  validation pipeline), FEAT-004 (Entity Operations — rules execute on
  every write), FEAT-013 (Secondary Indexes — gate-status queries use the
  same indexed query planning).
- **External services**: None. Normative surfaces live in CONTRACT-010
  (rule grammar, gate grammar, error envelope), CONTRACT-002 (GraphQL
  filters), and CONTRACT-007 (Cypher read surface).
- **PRD requirements**: FR-1 (P0) — active schema validation on entity
  writes; gates also support lifecycle transition enforcement governed by
  ADR-008.

## Out of Scope

- **Cross-entity validation**: rules that reference other entities (for
  example, "assignee must exist in users collection") — requires
  cross-collection lookups on every write.
- **Computed fields**: rules that derive a field value from other fields.
- **Async or external validation**: rules that call external services;
  semantic validation hooks are tracked under FEAT-022's parking-lot
  entry in `docs/helix/parking-lot.md`.
- **Rule inheritance**: rules inherited from a parent schema.
- **Subject-aware rules**: any rule that branches on caller identity,
  role, or grant — that is FEAT-029 policy territory.
