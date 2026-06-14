---
ddx:
  id: US-026
  review:
    self_hash: ab55532f63794c97bcc2800c46eb52c042efcdf7a85b6b5e390ff72439498d08
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---
# US-026: Enforce Invoice Approval Workflow

**Feature**: FEAT-010 — Entity State Machines and Transition Guards
**Feature Requirements**: LCM-03, LCM-04, LCM-05, LCM-07
**PRD Requirements**: FR-10
**Priority**: P1
**Status**: Draft

## Story

**As a** Wei, a business workflow builder running an AP system on Axon
**I want** Axon to enforce that invoices follow the declared approval workflow
**So that** no invoice reaches `paid` status without going through `approved`

## Context

Extracted from FEAT-010. Exercises transition validation (LCM-03), atomic
transition + audit capture (LCM-04, LCM-07), and field-based guard conditions
(LCM-05) using the AP/AR invoice lifecycle (draft → submitted → approved →
paid → reconciled).

## Walkthrough

1. Wei declares the invoice lifecycle in the collection schema, with a guard that `approved` requires `approver_id`.
2. An agent attempts to move a `draft` invoice straight to `paid`; Axon rejects it and lists the valid transitions.
3. A reviewer sets `approver_id` and moves a `submitted` invoice to `approved`, supplying a `reason`.
4. Wei inspects the audit trail and sees the transition with its metadata.

## Acceptance Criteria

- [ ] **US-026-AC1** — Given an invoice in `draft`, when its status is updated directly to `paid`, then the update fails with a structured error listing the valid transitions from `draft`.
- [ ] **US-026-AC2** — Given any rejected transition, when the error is returned, then it includes the current state, the attempted state, and the valid transitions from the current state.
- [ ] **US-026-AC3** — Given an invoice in `submitted` with no `approver_id`, when a transition to `approved` is attempted, then it fails identifying the unmet guard; with `approver_id` set, the same transition succeeds.
- [ ] **US-026-AC4** — Given a transition declaring metadata, when the transition commits with `reason` provided, then the audit entry for the transition includes that metadata.

## Edge Cases

- **Guard met but version stale**: a concurrent update bumps the invoice version between read and transition; the transition fails with a version conflict, not a guard error.
- **Rejected transition leaves no trace on the entity**: a failed transition produces no entity mutation and no mutation audit record.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Skip approval | US-026-AC1 | Invoice in `draft` | Update status to `paid` | Rejected; valid transitions from `draft` listed |
| Error shape | US-026-AC2 | Invoice in `draft`, attempted `paid` | Inspect rejection | Error carries current=`draft`, attempted=`paid`, valid transitions |
| Guard enforcement | US-026-AC3 | Invoice in `submitted`, `approver_id` null | Transition to `approved` | Rejected naming the `approver_id` guard; succeeds after `approver_id` set |
| Audited metadata | US-026-AC4 | Invoice in `in_review` | Transition to `rejected` with `reason: "budget exceeded"` | Audit entry includes the reason metadata |

## Dependencies

- **Stories**: US-028 (introspection of valid transitions).
- **Feature Spec**: [FEAT-010 — Entity State Machines and Transition Guards](../features/FEAT-010-entity-state-machines.md)
- **Feature Requirements**: LCM-03, LCM-04, LCM-05, LCM-07
- **PRD Requirements**: FR-10
- **External**: CONTRACT-001 (HTTP lifecycle endpoints), CONTRACT-010 (lifecycle declaration grammar)

## Out of Scope

- Approval routing and mutation intents (FEAT-030).
- Link-based guards (US-027).

## Review Checklist

- [ ] Persona comes from the PRD
- [ ] Every AC is independently testable with one Given/When/Then
- [ ] No exact API/CLI/event/schema surface inline; Contract IDs referenced
- [ ] Test scenarios cover the happy path and at least one edge case
