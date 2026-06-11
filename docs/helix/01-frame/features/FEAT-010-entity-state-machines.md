---
ddx:
  id: FEAT-010
  depends_on:
    - helix.prd
    - FEAT-007
    - FEAT-008
    - FEAT-009
    - FEAT-019
---
# Feature Specification: FEAT-010 — Entity State Machines and Transition Guards

**Feature ID**: FEAT-010
**Status**: draft
**Priority**: P1
**Owner**: Core Team
**Requirement Prefix**: LCM
**Covered PRD Subsystem(s)**: Reusable Policy Enforcement
**Covered PRD Requirements**: FR-10 (the lifecycle/transition declaration and enforcement aspect; role/attribute/field/relationship policy aspects are owned by FEAT-012/FEAT-029)
**Cross-Subsystem Rationale**: None — single subsystem.

## Overview

Entity state machines provide first-class lifecycle management for entities,
implementing the transition-policy aspect of PRD FR-10. A state machine
definition specifies valid states, allowed transitions, guard conditions, and
audit metadata. When an entity's status field changes, Axon validates the
transition against the state machine and rejects invalid transitions.

This feature is deliberately limited to entity transition guards. Axon does not
become a durable long-running workflow engine. External orchestrators such as
Temporal, Restate, Inngest, DBOS, or LangGraph may coordinate long-running work;
Axon enforces whether the resulting entity transition is valid and auditable.

## Ideal Future State

A schema author declares an entity lifecycle once — states, transitions,
guards, transition metadata — and every surface (GraphQL, MCP, CLI, SDK,
handler API) enforces it identically. An agent or UI can ask, for any entity,
"which transitions are valid right now, and why are the others blocked?" and
get a structured answer good enough to drive its next action. Invalid
transitions never reach storage, every transition is atomic with its audit
entry, and lifecycle behavior is impossible to bypass from any client.

## Problem Statement

- **Current situation**: Every application that manages entities with lifecycles reinvents state machine validation in application code: "can this invoice move from `pending` to `approved`?"
- **Pain points**: Without data-layer enforcement, invalid transitions slip through — an agent sets an issue to `closed` without going through `review`, a workflow skips the approval step. Application-level validation is inconsistent across clients and bypassable.
- **Desired outcome**: Schema-declared lifecycles enforced below every surface, with structured, actionable rejection errors and full audit capture of transitions and their metadata.

Use case research shows this pattern in 6 of 10 domains: approval chains
(AP/AR, time tracking), deal pipelines (CRM), issue lifecycles (issue
tracking), document review (document management), and bead lifecycles (agentic
apps).

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Lifecycle declaration | "How do I declare states, transitions, and guards?" | Schema-level state machine definition (grammar owned by CONTRACT-010 Layer 3) |
| Transition enforcement | "Was this status change legal?" | Validate every status change against the declared machine; reject with structured errors |
| Guards and metadata | "Can this entity transition given its data and links?" | Guard condition evaluation over entity fields and links; transition metadata captured in audit |
| Introspection | "What can this entity do next?" | Return the machine definition and the valid transitions (with guard status) for an entity |

## Requirements

### Functional Requirements by Area

#### Lifecycle Declaration

- **LCM-01**: A state machine must be definable in the collection schema as part of a designated status field, specifying states (nodes) and transitions (edges) with optional guard conditions. The normative lifecycle declaration grammar is owned by CONTRACT-010 (ESF schema format, Layer 3).
- **LCM-02**: A state machine may declare an initial state and terminal states. New entities must start in the initial state when one is declared; terminal states accept no outgoing transitions unless explicitly declared.

#### Transition Enforcement

- **LCM-03**: On entity update, if the status field changes, Axon must validate the transition against the state machine. Invalid transitions are rejected with a structured error listing the valid transitions from the current state.
- **LCM-04**: Transition validation, the entity update, and the audit entry must be one atomic operation.

#### Guards and Metadata

- **LCM-05**: Transitions must support guard conditions over the entity's fields and its links — e.g., "can only move to `approved` if `approver_id` is set"; "can only move to `done` if all `depends-on` targets are `done`". Link-based guard evaluation uses the FEAT-009 traversal primitive.
- **LCM-06**: When a guard fails, the error must identify the specific guard condition and the failing entity/field; when multiple guards fail, all failures are reported.
- **LCM-07**: Transitions must support declared metadata fields captured in the audit entry — e.g., moved to `rejected` with `reason: "budget exceeded"`.
- **LCM-08** (P2+): Transitions may declare tightly bounded field updates (e.g., `completed_at = now()` on transition to `done`). Long-running orchestration, retries, timers, notifications, and external task execution remain outside Axon.

#### Introspection

- **LCM-09**: Axon must expose the state machine definition for a collection and the valid transitions from any given state. The normative endpoints are owned by CONTRACT-001 (HTTP API surface, lifecycle endpoints).
- **LCM-10**: For a specific entity, Axon must report each candidate transition with whether its guards currently pass or fail, including the reason for each failing guard, in a structured response agents can act on.

### State Machine Schema (Conceptual)

The following YAML is a conceptual illustration only; CONTRACT-010 owns the
normative lifecycle declaration grammar.

```yaml
status_field: status
states:
  - draft
  - pending
  - in_review
  - approved
  - rejected
  - done
  - cancelled
initial: draft
terminal: [done, cancelled]
transitions:
  - from: draft
    to: pending
  - from: pending
    to: in_review
    guard: "entity.reviewer_id != null"
  - from: in_review
    to: approved
    guard: "entity.approver_id != null"
    metadata: [reason]
  - from: in_review
    to: rejected
    metadata: [reason]
  - from: approved
    to: done
  - from: [draft, pending, in_review]
    to: cancelled
    metadata: [reason]
```

### Domain Applications (from use case research)

| Domain | Entity | States | Key Transitions |
|--------|--------|--------|-----------------|
| AP/AR | Invoice | draft → submitted → approved → paid → reconciled | Guard: `approved` requires `approver_id`; `paid` requires linked payment |
| CRM | Deal | prospecting → qualification → proposal → negotiation → closed_won / closed_lost | Guard: `proposal` requires `amount > 0` |
| Issue Tracking | Issue | open → in_progress → in_review → done / wont_fix | Guard: `in_review` requires `assignee_id` |
| Time Tracking | TimeEntry | draft → submitted → approved → billed | Guard: `approved` requires linked approval |
| Document Mgmt | Document | draft → review → approved → published / archived | Guard: `approved` requires all `reviews` links resolved |
| Agentic Apps | Bead | proposed → open → in_progress → blocked → closed / cancelled (DDx lifecycle, see FEAT-006) | Guard: leaving `open` for `in_progress` may require all `blocks` dependencies `closed` |

### Non-Functional Requirements

- **Performance**: Transition validation < 2ms (guard evaluation may add latency for link-based conditions).
- **Atomicity**: Transition validation, entity update, and audit entry are one atomic operation (LCM-04).

## Non-Goals

- Durable workflow orchestration.
- Timers, retries, sleeps, queues, or scheduled task execution.
- Replacing LangGraph, Temporal, Restate, Inngest, or DBOS.
- Arbitrary side-effect hooks that mutate external systems.

## User Stories

| ID | Title | Link |
|----|-------|------|
| US-026 | Enforce Invoice Approval Workflow | [US-026](../user-stories/US-026-enforce-invoice-approval-workflow.md) |
| US-027 | Bead Lifecycle with Dependency Guards | [US-027](../user-stories/US-027-bead-lifecycle-with-dependency-guards.md) |
| US-028 | Query Valid Transitions | [US-028](../user-stories/US-028-query-valid-transitions.md) |

## Edge Cases and Error Handling

- **No state machine defined**: Collections without a state machine allow any value in the status field (standard schema validation only).
- **Bulk update with mixed transitions**: In a batch, each entity's transition is validated independently. One invalid transition fails the whole transaction.
- **Guard references deleted link target**: Guard fails (link target not found). Entity cannot transition until the dependency is resolved.
- **Concurrent state change**: Optimistic concurrency handles this — version conflict if two clients try to transition the same entity simultaneously.

## Success Metrics

- Zero invalid lifecycle transitions reach storage on collections with a declared state machine, across all public surfaces.
- Agents and UIs can choose their next action from the introspection response alone — no trial-and-error invalid transition attempts needed.
- Transition validation stays within the < 2ms NFR target on reference hardware.

## Constraints and Assumptions

### Constraints

- Guard conditions are declarative and evaluated by Axon; they cannot call out to external systems.
- Lifecycle enforcement lives in the shared handler path so no surface can bypass it (FR-11 alignment).

### Assumptions

- Most collections declare a single status field with a modest state machine (≤ 20 states).
- Link-based guards traverse a bounded neighborhood (direct link targets), not arbitrary graph depth.

## Dependencies

- **Other features**: FEAT-007 (Entity-Graph Model — guard conditions may reference links), FEAT-008 (ACID Transactions — transition + update + audit must be atomic), FEAT-009 (Unified Graph Query — link-based guards use the traversal primitive), FEAT-019 (Validation Rules — guard conditions share the rule/condition grammar and gate machinery).
- **External services**: None. Normative interface surface: CONTRACT-001 (HTTP lifecycle endpoints), CONTRACT-010 (lifecycle declaration grammar).
- **PRD requirements**: FR-10 (P0, transition-policy aspect).

## Out of Scope

- Timer-based automatic transitions.
- Parallel states / state hierarchy.
- External notification/webhook side effects.
- Visual state machine editor.

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
