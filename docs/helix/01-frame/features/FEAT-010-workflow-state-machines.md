---
dun:
  id: FEAT-010
  depends_on:
    - helix.prd
    - FEAT-007
    - FEAT-008
    - FEAT-009
    - FEAT-019
---
# Feature Specification: FEAT-010 - Workflow State Machines

**Feature ID**: FEAT-010
**Status**: Draft
**Priority**: P2
**Owner**: Core Team
**Created**: 2026-04-04
**Updated**: 2026-04-04

## Overview

Workflow state machines provide first-class lifecycle management for entities. A state machine definition specifies valid states, allowed transitions, guard conditions, and side effects. When an entity's status field changes, Axon validates the transition against the state machine, rejects invalid transitions, and optionally triggers side effects (link creation, audit metadata, field updates). Use case research shows this pattern in 6 of 10 domains: approval chains (AP/AR, time tracking), deal pipelines (CRM), issue lifecycles (issue tracking), document review (document management), and bead lifecycles (agentic apps).

## Problem Statement

Every application that manages entities with lifecycles reinvents state machine validation: "can this invoice move from `pending` to `approved`?" "can this bead go from `draft` to `done` directly?" Without database-level enforcement, invalid transitions slip through — an agent sets an issue to `closed` without going through `review`, a workflow skips the approval step. Application-level validation is inconsistent across clients and bypassable.

## Requirements

### Functional Requirements

- **State machine definition**: Defined in the collection schema as part of a designated status field. Specifies states (nodes) and transitions (edges) with optional guard conditions
- **Transition validation**: On entity update, if the status field changes, Axon validates the transition against the state machine. Invalid transitions are rejected with a structured error listing valid transitions from the current state
- **Guard conditions**: Transitions can require conditions on the entity or its links. "Can only move to `approved` if `approver_id` is set." "Can only move to `done` if all `depends-on` targets are `done`"
- **Transition metadata**: Each transition can carry metadata captured in the audit entry. "Moved to `rejected` with `reason: 'budget exceeded'`"
- **Side effects** (P2+): Transitions can trigger: link creation (e.g., `approved-by` link on approval), field updates (e.g., `completed_at = now()` on transition to `done`), notification hooks
- **State introspection**: API returns the state machine definition for a collection and the valid transitions from any given state

### State Machine Schema (Conceptual)

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
| Agentic Apps | Bead | draft → pending → ready → in_progress → review → done | Guard: `ready` requires all `depends-on` targets in `done` state |

### Non-Functional Requirements

- **Performance**: Transition validation < 2ms (guard evaluation may add latency for link-based conditions)
- **Atomicity**: Transition validation, entity update, and audit entry are one atomic operation

## User Stories

### Story US-026: Enforce Invoice Approval Workflow [FEAT-010]

**As an** AP system
**I want** Axon to enforce that invoices follow the approval workflow
**So that** no invoice reaches `paid` status without going through `approved`

**Acceptance Criteria:**
- [ ] Updating invoice status from `draft` to `paid` directly fails with error listing valid transitions
- [ ] Error response includes: current state, attempted state, valid transitions from current state
- [ ] Transition from `submitted` to `approved` succeeds only if `approver_id` is set
- [ ] The audit entry for a transition includes the transition metadata (`reason`)
- [ ] Audit entry for a lifecycle transition includes transition metadata (reason, actor) when provided on the update

### Story US-027: Bead Lifecycle with Dependency Guards [FEAT-010]

**As an** agentic framework
**I want** beads to enforce lifecycle transitions with dependency-aware guards
**So that** no bead starts execution until all its dependencies are complete

**Acceptance Criteria:**
- [ ] Bead cannot transition from `pending` to `ready` if any `depends-on` target has status != `done`
- [ ] Guard evaluation traverses links (uses FEAT-009 traversal primitive)
- [ ] Transition to `ready` succeeds once all dependencies are `done`
- [ ] API endpoint `GET /collections/beads/state-machine` returns the full state machine definition
- [ ] Guard evaluation for link-based conditions uses the same traversal primitive as FEAT-009
- [ ] If a guard fails, the error message identifies the specific guard condition and the failing entity/field

### Story US-028: Query Valid Transitions [FEAT-010]

**As an** agent or UI
**I want** to know which state transitions are valid for an entity in its current state
**So that** I can present valid options or avoid invalid API calls

**Acceptance Criteria:**
- [ ] `GET /collections/{coll}/entities/{id}/transitions` returns valid next states with guard status
- [ ] Each transition includes whether the guard currently passes or fails (with reason)
- [ ] Response is structured JSON that agents can use to make decisions
- [ ] Response includes all failing guards with specific reasons when multiple guards fail simultaneously

## Edge Cases

- **No state machine defined**: Collections without a state machine allow any value in the status field (standard schema validation only)
- **Bulk update with mixed transitions**: In a batch, each entity's transition is validated independently. One invalid transition fails the whole transaction
- **Guard references deleted link target**: Guard fails (link target not found). Entity cannot transition until the dependency is resolved
- **Concurrent state change**: OCC handles this — version conflict if two clients try to transition the same entity simultaneously

## Dependencies

- **FEAT-007** (Entity-Graph Model): Guard conditions may reference links
- **FEAT-008** (ACID Transactions): Transition + update + audit must be atomic
- **FEAT-009** (Graph Traversal): Link-based guards use traversal

## Out of Scope

- Timer-based automatic transitions (P3)
- Parallel states / state hierarchy (P3)
- External notification/webhook side effects (P2+)
- Visual state machine editor

## Traceability

### Related Artifacts
- **Parent PRD Section**: Section 8 (P2 #2 workflow primitives)
- **Use Case Research**: AP/AR, CRM, Issue Tracking, Time Tracking, Document Management, Agentic Applications
- **User Stories**: US-026, US-027, US-028
- **Test Suites**: `tests/FEAT-010/`

### Feature Dependencies
- **Depends On**: FEAT-007, FEAT-008, FEAT-009
- **Depended By**: FEAT-006 (Bead Adapter — bead lifecycle)
