---
dun:
  id: FEAT-025
  depends_on:
    - helix.prd
    - FEAT-012
    - FEAT-014
    - ADR-018
---
# Feature Specification: FEAT-025 - BYOC Deployment Control Plane

**Feature ID**: FEAT-025
**Status**: Draft
**Priority**: P1
**Owner**: Core Team
**Created**: 2026-04-06
**Updated**: 2026-04-14

## Scope and Terminology

FEAT-025 is about managing **Axon deployments** — running `axon-server`
processes, each of which is its own self-contained unit with its own
backing store. A "managed deployment" in this spec is an `axon-server`
instance running in a customer's cloud (BYOC) that the control plane
observes and provisions.

**Distinction from FEAT-014 and FEAT-012**: within a single managed
deployment, the `/control/tenants`, `/control/users`, and
`/control/tenants/{id}/credentials` routes manage the deployment's
internal **tenants** (ADR-018 global account boundaries), **users**, and
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
reads customer data inside a managed deployment, per the data sovereignty
requirement below. Each managed deployment's embedded control plane is
the sole authority for its own tenants and users.

## Overview

A lightweight management plane for multi-deployment Axon fleets.
Provides centralized deployment lifecycle, monitoring, and operational
visibility. Designed for the BYOC (Bring Your Own Cloud) commercial
model where customers run Axon in their own infrastructure.

## Problem Statement

As Axon moves from single-deployment development use to fleets of
production deployments across customer clouds, operators need a
centralized way to manage deployments without touching the data inside
any of them. Provisioning, monitoring, and operational tasks should not
require direct access to each deployment's data plane.

## Requirements

### Functional Requirements

#### Managed Deployment Lifecycle

- Provision new Axon deployments with configured backing store.
- Deprovision deployments with data retention policy enforcement.
- Configuration management: schema, rate limits, guardrails per
  deployment.
- Deployment inventory: list all managed deployments with status,
  version, health.
- Note on internal tenants: the per-deployment tenants managed inside
  each Axon instance (FEAT-014) are visible to this control plane only
  as aggregate counts (e.g., "deployment acme has 12 tenants"). The BYOC
  control plane never inspects per-tenant data and never mutates the
  internal tenant list directly — that's done via each deployment's
  `/control/tenants` routes by the deployment's own admin.

#### Centralized Monitoring

- Health checks across all managed Axon instances.
- Capacity monitoring: storage utilization, connection counts, latency.
- Alerting: configurable thresholds for latency, error rate, storage.
- Aggregate dashboards: single pane of glass across all tenants.

#### Data Sovereignty

- Control plane metadata lives in its own PostgreSQL database (the
  control plane's store), separate from any managed deployment.
- Control plane never reads customer entity data, and it never reads
  the per-deployment `tenants` / `users` / `credentials` tables either.
  These belong to the managed deployment's embedded control plane
  (FEAT-014 + FEAT-012) and are private to that deployment.
- All monitoring is based on metrics and health endpoints exposed by
  the managed deployment, not data inspection.
- Each managed deployment's data stays in the deployment's chosen
  backing store and region.

#### BYOC Support

- Customer-managed infrastructure: Axon runs in customer's cloud account.
- Control plane communicates with managed deployments via authenticated
  API (registration token + short-lived JWTs for the observation path).
- Supports air-gapped deployments with local control plane option.
- Managed deployment registration and deregistration.

### Non-Functional Requirements

- Control plane itself must be highly available (standard PostgreSQL HA).
- Adding/removing tenants must not affect other tenants' performance.
- Control plane API authenticated and authorized (integrates with
  FEAT-012).

### Dependencies

- **FEAT-014** (Tenancy) — defines the in-deployment tenant model and
  path-based wire protocol. FEAT-025 observes managed deployments
  *through* their FEAT-014 `/control/tenants` surface but never
  mutates it directly.
- **FEAT-012** (Authentication, Identity, Authorization) — defines
  users, JWT credentials, and membership. FEAT-025 issues its own
  BYOC-registration credentials that are separate from per-deployment
  user credentials.
- **ADR-018** — governing ADR for the tenant/user/credential model.
  Clarifies that the BYOC control plane in FEAT-025 is at a different
  layer than the per-deployment tenant model in FEAT-014.

## Acceptance Criteria

- [ ] New Axon deployment can be provisioned via BYOC control plane API
- [ ] Deployment health is visible in aggregate dashboard (cross-fleet
      view)
- [ ] BYOC control plane never accesses entity data or per-deployment
      tenant data
- [ ] BYOC deployment: customer-hosted Axon instance registers with
      control plane and receives a registration credential
- [ ] Deployment deprovisioning respects data retention policies
- [ ] Control plane scales to 100+ managed deployments
