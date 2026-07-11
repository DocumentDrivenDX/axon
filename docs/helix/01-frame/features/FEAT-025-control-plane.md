---
ddx:
  id: FEAT-025
  depends_on:
    - helix.prd
  review:
    self_hash: 5ff1ca8b03318957e25d5a3752ebf8999a45378a7b83aa6c2978739263ac3603
    deps:
      helix.prd: 6703170c71275bba7d108c4f9c329d32e4104f9c965278db888ad43cdc3ca367
    reviewed_at: "2026-07-11T04:03:37Z"
---
# Feature Specification: FEAT-025 — BYOC Deployment Control Plane

**Feature ID**: FEAT-025
**Status**: draft
**Priority**: P1
**Owner**: Core Team
**Covered PRD Subsystem(s)**: Identity, Tenancy, and Storage Portability
**Covered PRD Requirements**: FR-27
**Cross-Subsystem Rationale**: None — single subsystem.
**FR Prefix**: BYO

> Ratified as committed P1 product scope by the product owner on
> 2026-06-10 (PRD Open Questions; FR-27).

## Scope and Terminology

FEAT-025 is about managing **Axon deployments** — running Axon server
processes, each of which is its own self-contained unit with its own
backing store. A "managed deployment" in this spec is an Axon instance
running in a customer's cloud (BYOC) that the control plane observes and
provisions.

**Distinction from FEAT-014 and FEAT-012**: within a single managed
deployment, that deployment's own control surface manages its internal
**tenants** (ADR-018 global account boundaries), **users**, and
**credentials**. Those are defined in FEAT-014 and FEAT-012, not here.
FEAT-025 is strictly the external-to-each-deployment control layer that
commercial BYOC customers use to inventory, health-check, and provision
their fleet of Axon deployments.

In short:

- FEAT-025 = "I'm an operator running 12 Axon deployments in 12 customer
  clouds, and I need to see them all on one dashboard"
- FEAT-014 + FEAT-012 = "inside any one Axon deployment, I have tenants,
  users, memberships, and credentials"

The two layers do not talk to each other. The BYOC control plane never
reads customer data inside a managed deployment, per the data-sovereignty
requirements below. Each managed deployment's embedded control plane is
the sole authority for its own tenants and users.

## Overview

A lightweight management plane for multi-deployment Axon fleets,
implementing PRD FR-27: "a BYOC fleet control plane that manages
customer-hosted Axon deployments without reading tenant data." It provides
centralized deployment lifecycle, monitoring, and operational visibility
for the BYOC (Bring Your Own Cloud) commercial model where customers run
Axon in their own infrastructure.

## Ideal Future State

A fleet operator provisions a deployment slot, hands the customer a
registration credential, and watches the customer-hosted instance appear
in the fleet inventory — active, versioned, and health-checked — without
ever holding access to the customer's data. The single dashboard answers
"which deployments are healthy, near capacity, or behind on versions"
across the whole fleet. Offboarding is explicit and auditable: retention
policy is enforced before termination, terminated deployments cannot
silently rejoin, and every lifecycle action carries an audit trail. The
control plane's own state lives in a dedicated store with no path —
technical or operational — to customer entity data.

## Problem Statement

- **Current situation**: Axon targets single-deployment development use;
  operating multiple production deployments across customer clouds means
  ad-hoc inventory, monitoring, and provisioning per deployment.
- **Pain points**: Operators need provisioning, monitoring, and lifecycle
  control without direct access to each deployment's data plane; customers
  need contractual certainty that the vendor's control layer cannot read
  their data.
- **Desired outcome**: One control plane inventories, provisions,
  observes, and deprovisions customer-hosted deployments, with verifiable
  data sovereignty.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Managed-deployment lifecycle | "Onboard, configure, and offboard a customer deployment" | Provision, register, configure, deprovision, and terminate deployments with audited transitions and retention enforcement |
| Fleet observation | "Is my fleet healthy, and where is capacity going?" | Health, capacity, version, and error-rate visibility with alerting and an aggregate dashboard |
| Data sovereignty | "Prove the control plane cannot read customer data" | Strict separation of control-plane state from tenant data; observation limited to metrics and health endpoints |

## Requirements

### Functional Requirements by Area

#### Managed-Deployment Lifecycle

- **BYO-01**. Operators MUST be able to provision a new managed deployment
  with a configured backing store through the control-plane API.
- **BYO-02**. A customer-hosted Axon instance MUST be able to register its
  endpoint against a provisioned deployment and move it to active status;
  registration against a hosted, terminated, or unknown deployment MUST be
  rejected with a structured error.
- **BYO-03**. Control-plane communication with managed deployments MUST be
  authenticated using registration tokens and short-lived credentials for
  the observation path; these credentials MUST be separate from any
  per-deployment user credential (FEAT-012).
- **BYO-04**. The control plane MUST maintain a deployment inventory
  listing every managed deployment with status, version, and health.
- **BYO-05**. Operators MUST be able to manage per-deployment
  configuration: schema, rate limits, and guardrails.
- **BYO-06**. Deprovisioning MUST enforce the deployment's retention
  policy (retain, delete-after, legal-hold) before termination; terminated
  deployments MUST reject later health reports; a terminated deployment ID
  MUST NOT be re-provisionable — operators create a new deployment.
- **BYO-07**. Every lifecycle operation (provision, register, deprovision,
  terminate) MUST be recorded in the control plane's audit trail with
  actor, timestamp, previous status, new status, and applicable retention
  policy.

#### Fleet Observation

- **BYO-08**. The control plane MUST health-check all managed deployments;
  health reports MUST include version, storage bytes, open connections,
  p99 latency, and error rate.
- **BYO-09**. The control plane MUST monitor capacity (storage
  utilization, connection counts, latency) and MUST support configurable
  alert thresholds for latency, error rate, and storage.
- **BYO-10**. The control plane MUST provide an aggregate dashboard — a
  single pane of glass across all managed deployments — in which a
  deployment's internal tenants (FEAT-014) appear only as aggregate counts
  (e.g., "deployment acme has 12 tenants"), never as tenant names, users,
  credentials, or entity data.

#### Data Sovereignty

- **BYO-11**. The control plane MUST NOT read customer entity data and
  MUST NOT read any managed deployment's internal tenant, user, or
  credential records; those belong to the deployment's embedded control
  plane (FEAT-014 + FEAT-012) and are private to that deployment. The
  control plane MUST NOT expose any entity, link, collection, or audit
  data endpoint for managed deployments.
- **BYO-12**. All monitoring MUST be based on metrics and health endpoints
  exposed by the managed deployment, never on data inspection.
- **BYO-13**. Control-plane metadata MUST live in a dedicated
  control-plane store, separate from tenant data (per ADR-017); each
  managed deployment's data stays in the deployment's chosen backing store
  and region.
- **BYO-14**. The control plane MUST support an air-gapped posture: a
  locally run control plane managing deployments with no connectivity to
  any vendor-hosted service.

### Non-Functional Requirements

- **Reliability**: the control plane itself is highly available; loss of
  the control plane MUST NOT affect data-plane availability of any managed
  deployment.
- **Isolation**: control-plane operations (provision, register, observe,
  deprovision) MUST NOT degrade any managed deployment's data-plane p99
  latency by more than 5% relative to the same workload with observation
  disabled. *(Numeric target proposed as an assumption — pending
  ratification against the benchmark suite.)*
- **Scalability**: the fleet dashboard lists 100+ managed deployments with
  inventory and health rendered in under 2 seconds. *(Numeric target
  proposed as an assumption.)*
- **Security**: the control-plane API is authenticated and authorized,
  integrating with the FEAT-012 identity model; observation credentials
  are short-lived and revocable.

## User Stories

- [US-144 — Provision and Register a BYOC Deployment](../user-stories/US-144-provision-and-register-a-byoc-deployment.md)
- [US-145 — Observe a Fleet Without Reading Tenant Data](../user-stories/US-145-observe-a-fleet-without-reading-tenant-data.md)
- [US-146 — Deprovision with Retention Guarantees](../user-stories/US-146-deprovision-with-retention-guarantees.md)

## Edge Cases and Error Handling

- **Registration replay**: a second registration attempt against an
  already-active deployment is rejected with a structured error; the
  original registration remains intact and the attempt is audited.
- **Health reports from terminated deployments**: rejected (BYO-06) and
  audited, so a decommissioned instance cannot silently reappear.
- **Unreachable deployment**: the inventory shows last-seen health and
  staleness; unreachability beyond the configured threshold raises an
  alert rather than dropping the deployment from inventory.
- **Clock skew between control plane and deployments**: short-lived
  observation credentials tolerate bounded skew; expiry decisions use
  control-plane time.
- **Air-gapped fleets**: registration and observation work entirely
  against the local control plane; no functionality silently depends on a
  vendor-hosted endpoint.

## Success Metrics

- A BYOC deployment goes from provisioned slot to active, health-reporting
  fleet member in under 30 minutes of operator + customer effort.
- 100% of lifecycle transitions (provision, register, deprovision,
  terminate) carry complete audit records in the control-plane audit
  trail.
- Zero control-plane code paths or API routes can return tenant entity,
  link, collection, audit, user, or credential data — verified by
  sovereignty contract tests.
- Fleet-wide health visibility: an unhealthy managed deployment is visible
  on the dashboard within one health-check interval.

## Constraints and Assumptions

- The control plane is at a different layer than the per-deployment
  tenant model; it never mutates a deployment's internal tenant list
  (that is done by the deployment's own admin through its own control
  surface, FEAT-014).
- Control-plane state lives in a dedicated store separate from tenant
  data; ADR-017 governs the store choice. This spec does not prescribe a
  specific database product.
- **Assumption**: the 5% isolation bound and the 100-deployment / 2 s
  dashboard target (see NFRs) are proposed numbers awaiting product-owner
  ratification.
- Distributed node placement, database migration, and routing remain
  deferred (PRD FR-27; `docs/helix/parking-lot.md`).

## Dependencies

- **Other features**:
  - FEAT-014 (Tenancy) — defines the in-deployment tenant model; FEAT-025
    observes deployments through their exposed health/metrics surface and
    never mutates internal tenancy.
  - FEAT-012 (Authentication, Identity, Authorization) — defines users,
    credentials, and membership; FEAT-025 issues its own BYOC registration
    credentials, separate from per-deployment user credentials.
- **External services**: customer cloud infrastructure hosting managed
  deployments; exact control-plane API surface lives in a Contract
  artifact when scheduled for design.
- **PRD requirements**: FR-27 (P1).

## Out of Scope

- **Per-deployment tenant/user/credential administration**: owned by each
  deployment's embedded control plane (FEAT-014, FEAT-012).
- **Reading, exporting, or migrating customer entity data**: the control
  plane has no data-plane access by design.
- **Distributed placement, database migration, and routing** across nodes
  or regions: deferred (parking lot).
- **Billing and usage metering**: commercial metering may later consume
  control-plane inventory but is not part of this feature.
- **Managing non-Axon software** in the customer's cloud (databases, load
  balancers, networks): the control plane manages Axon deployments only.
