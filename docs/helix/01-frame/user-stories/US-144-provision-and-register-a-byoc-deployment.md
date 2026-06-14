---
ddx:
  id: US-144
  review:
    self_hash: 9f317186e86c85c2248015e88eb122267f777e317bce5d4f9d4273639470d174
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---

# US-144: Provision and Register a BYOC Deployment

**Feature**: FEAT-025 — BYOC Deployment Control Plane
**Feature Requirements**: BYO-01, BYO-02, BYO-03, BYO-07
**PRD Requirements**: FR-27
**Priority**: P1
**Status**: Draft

## Story

**As an** Axon operator onboarding a customer
**I want** to provision a managed deployment slot and let the customer's
Axon instance register itself
**So that** BYOC deployments enter the fleet inventory without exposing
customer data

## Context

Renumbered from US-101 (collision with FEAT-029). BYOC onboarding must
work without the vendor ever holding data-plane access. This story
exercises the lifecycle area of FEAT-025: provisioning (BYO-01),
customer-side registration with structured rejection of invalid states
(BYO-02), authenticated registration/observation credentials (BYO-03),
and audited transitions (BYO-07).

## Walkthrough

1. The operator provisions a managed deployment slot through the
   control-plane API, specifying the backing-store configuration.
2. The control plane issues a registration token for the slot.
3. The customer starts Axon in their own cloud and registers the
   instance's endpoint using the token.
4. The control plane validates the registration, moves the deployment to
   active status, and returns a short-lived credential (or token-exchange
   reference) for the observation path.
5. The deployment appears in the fleet inventory; the registration event
   is recorded in the control-plane audit trail.

## Acceptance Criteria

- [ ] **US-144-AC1** — Given an authenticated operator, when they
      provision a new managed deployment via the control-plane API, then a
      deployment slot exists with the configured backing store and
      provisioned status.
- [ ] **US-144-AC2** — Given a provisioned slot and a valid registration
      token, when the customer-hosted instance registers its endpoint,
      then the deployment moves to active status in the inventory.
- [ ] **US-144-AC3** — Given a successful registration, when the response
      is returned, then it contains a short-lived credential or
      token-exchange reference for the observation path, separate from any
      per-deployment user credential.
- [ ] **US-144-AC4** — Given a hosted, terminated, or unknown deployment
      ID, when registration is attempted against it, then it is rejected
      with a structured error.
- [ ] **US-144-AC5** — Given any provision or registration event, when it
      completes, then it is stored in the control-plane audit trail with
      actor, timestamp, and status transition.

## Edge Cases

- **Registration replay**: a second registration against an
  already-active deployment is rejected with a structured error and
  audited; the original registration is unaffected.
- **Expired registration token**: rejected with a structured error; the
  operator can issue a new token without re-provisioning.
- **Air-gapped fleet**: the same flow works against a locally run control
  plane with no vendor-hosted dependency.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Provision | US-144-AC1 | Authenticated operator | Provision deployment `acme-prod` | Slot exists, status provisioned, audited |
| Register | US-144-AC2 | `acme-prod` provisioned; valid token | Customer instance registers endpoint | Status active; inventory row present |
| Invalid target | US-144-AC4 | Deployment `old-1` terminated | Register against `old-1` | Structured rejection; no status change |
| Audit trail | US-144-AC5 | Completed provision + register | Query control-plane audit trail | Both events with actor, timestamp, transitions |

## Dependencies

- **Stories**: none
- **Feature Spec**: FEAT-025
- **Feature Requirements**: BYO-01, BYO-02, BYO-03, BYO-07
- **PRD Requirements**: FR-27
- **External**: FEAT-012 (identity model the control-plane auth integrates
  with); control-plane API Contract (future, when scheduled)

## Out of Scope

- Fleet observation/dashboards (US-145); deprovisioning (US-146);
  per-deployment internal tenant/user administration (FEAT-014/FEAT-012).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
