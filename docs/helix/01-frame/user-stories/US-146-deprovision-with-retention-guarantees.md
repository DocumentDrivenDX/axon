---
ddx:
  id: US-146
  review:
    self_hash: c1aebebc4d23efe83bfa8029d0d033058a2e679d0840e86ada76c62b63149682
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---

# US-146: Deprovision with Retention Guarantees

**Feature**: FEAT-025 — BYOC Deployment Control Plane
**Feature Requirements**: BYO-06, BYO-07
**PRD Requirements**: FR-27
**Priority**: P1
**Status**: Draft

## Story

**As an** operator ending a customer contract
**I want** deprovisioning and termination to respect retention policy
**So that** data handling is explicit, auditable, and irreversible only
when allowed

## Context

Renumbered from US-103 (collision with FEAT-029). Offboarding is where
data-handling promises are tested. This story exercises BYO-06 and BYO-07:
retention policies (retain, delete-after, legal-hold) enforced before
termination, terminated deployments locked out of the fleet, and every
transition audited.

## Walkthrough

1. The operator initiates deprovisioning for a managed deployment.
2. The control plane evaluates the deployment's retention policy; a
   legal-hold blocks termination, a delete-after policy schedules it, a
   retain policy keeps metadata.
3. Once policy allows, the deployment transitions out of active service
   and is terminated.
4. The deployment's later health reports are rejected, and attempts to
   re-provision its ID are rejected.
5. Every transition appears in the control-plane audit trail with actor,
   timestamp, previous status, new status, and the applied retention
   policy.

## Acceptance Criteria

- [ ] **US-146-AC1** — Given an active deployment, when the operator
      deprovisions it, then it transitions out of active service.
- [ ] **US-146-AC2** — Given a terminated deployment, when it submits a
      later health report, then the report is rejected.
- [ ] **US-146-AC3** — Given a retain, delete-after, or legal-hold
      retention policy, when termination is requested, then the policy is
      enforced before termination proceeds (legal-hold blocks it).
- [ ] **US-146-AC4** — Given a deprovision or terminate operation, when it
      completes, then an audit record captures actor, timestamp, previous
      status, new status, and retention policy.
- [ ] **US-146-AC5** — Given a terminated deployment ID, when an operator
      attempts to re-provision it, then the attempt is rejected and a new
      deployment must be created instead.

## Edge Cases

- **Deprovision while unreachable**: control-plane status transitions
  proceed; the customer-side instance is rejected on its next contact.
- **Legal-hold added mid-deprovision**: termination is blocked until the
  hold is lifted; the blocking is audited.
- **Delete-after expiry in an air-gapped fleet**: enforcement runs on the
  local control plane's clock; no vendor-hosted dependency.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Deprovision | US-146-AC1 | `acme-prod` active | Deprovision | Status leaves active; audited |
| Lockout | US-146-AC2 | `acme-prod` terminated | Instance posts health report | Rejected with structured error |
| Legal hold | US-146-AC3 | `acme-prod` under legal-hold | Request termination | Blocked; status unchanged; audited |
| No ID reuse | US-146-AC5 | `acme-prod` terminated | Provision with same ID | Rejected; operator must create new deployment |

## Dependencies

- **Stories**: US-144 (an active deployment to offboard)
- **Feature Spec**: FEAT-025
- **Feature Requirements**: BYO-06, BYO-07
- **PRD Requirements**: FR-27
- **External**: control-plane API Contract (future, when scheduled)

## Out of Scope

- Deleting or migrating customer data inside the deployment's backing
  store (customer-owned); observation (US-145); audit retention/erasure
  policy design for regulated customers (PRD open question).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
