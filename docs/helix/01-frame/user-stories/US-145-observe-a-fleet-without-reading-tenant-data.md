---
ddx:
  id: US-145
  review:
    self_hash: 835d27de5c0f2ceca1de5aaf42d7d45d87828dad8cfc5222d77b261d719082d1
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# US-145: Observe a Fleet Without Reading Tenant Data

**Feature**: FEAT-025 — BYOC Deployment Control Plane
**Feature Requirements**: BYO-04, BYO-08, BYO-09, BYO-10, BYO-11, BYO-12
**PRD Requirements**: FR-27
**Priority**: P1
**Status**: Draft

## Story

**As an** operator responsible for fleet health
**I want** health, capacity, version, and error-rate visibility across
managed deployments
**So that** I can operate the fleet without reading customer entity data
or internal tenant/user/credential records

## Context

Renumbered from US-102 (collision with FEAT-029). The BYOC value
proposition stands or falls on observation without data access. This
story exercises fleet observation (BYO-04, BYO-08..10) under the
data-sovereignty constraints (BYO-11, BYO-12): everything the operator
sees derives from metrics and health endpoints, never data inspection.

## Walkthrough

1. The operator opens the aggregate fleet dashboard.
2. Every managed deployment appears with status, version, and latest
   health: storage bytes, open connections, p99 latency, error rate.
3. Internal tenants of each deployment appear only as aggregate counts.
4. The operator configures an alert threshold on error rate; a deployment
   crossing it raises an alert.
5. At no point does any control-plane view or route expose entity, link,
   collection, audit, tenant, user, or credential data from a managed
   deployment.

## Acceptance Criteria

- [ ] **US-145-AC1** — Given registered deployments, when the operator
      views the dashboard, then every deployment's health is visible in
      one aggregate view.
- [ ] **US-145-AC2** — Given a healthy deployment, when its health report
      arrives, then it includes version, storage bytes, open connections,
      p99 latency, and error rate.
- [ ] **US-145-AC3** — Given a deployment with internal tenants, when the
      dashboard renders it, then tenants appear only as an aggregate count
      — never tenant names, users, credentials, or entity data.
- [ ] **US-145-AC4** — Given the control-plane API, when its routes are
      enumerated, then none expose Axon entity, link, collection, or audit
      data of a managed deployment.
- [ ] **US-145-AC5** — Given a configured alert threshold, when a
      deployment's metric crosses it, then an alert is raised for that
      deployment.
- [ ] **US-145-AC6** — Given a fleet of 100+ managed deployments, when the
      dashboard loads, then inventory and health render within the
      feature's latency target (FEAT-025 NFRs).

## Edge Cases

- **Unreachable deployment**: dashboard shows last-seen health with
  staleness; prolonged unreachability raises an alert instead of dropping
  the row.
- **Health report from a terminated deployment**: rejected and audited
  (see US-146).
- **Compromised observation credential**: short-lived credentials limit
  exposure; revocation takes effect for subsequent reports.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Aggregate view | US-145-AC1 | 3 active deployments | Open dashboard | 3 rows with status/version/health |
| Health payload | US-145-AC2 | Deployment reporting | Inspect health row | Version, storage, connections, p99, error rate present |
| Sovereignty | US-145-AC3 | Deployment with 12 tenants | Render row | Shows "12 tenants"; no tenant names anywhere |
| No data routes | US-145-AC4 | Control-plane route table | Enumerate routes | Zero entity/link/collection/audit data endpoints |
| Alerting | US-145-AC5 | Error-rate threshold 1% | Deployment reports 5% | Alert raised for that deployment |

## Dependencies

- **Stories**: US-144 (deployments must be registered)
- **Feature Spec**: FEAT-025
- **Feature Requirements**: BYO-04, BYO-08, BYO-09, BYO-10, BYO-11, BYO-12
- **PRD Requirements**: FR-27
- **External**: managed deployments' exposed metrics/health endpoints;
  control-plane API Contract (future, when scheduled)

## Out of Scope

- Lifecycle operations (US-144, US-146); per-deployment data inspection
  (deliberately impossible); billing/usage metering.

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
