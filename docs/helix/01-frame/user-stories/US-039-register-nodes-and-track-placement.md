---
ddx:
  id: US-039
  review:
    self_hash: cf83d92d343448afd64947f63a44ccc6ac5293cc31a2e8e78978cd8885ac7b22
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---
# US-039: Register Nodes and Track Placement

**Feature**: FEAT-014 — Tenancy, Namespace Hierarchy, and Path-Based Addressing
**Feature Requirements**: None active — capability deferred (see Context)
**PRD Requirements**: FR-27 (deferred aspect), PRD P2 #3
**Priority**: P2
**Status**: Draft

## Story

**As a** Wei, a business workflow builder operating Axon across regions
**I want** to register nodes and assign databases to specific nodes
**So that** data lives in the region closest to its users and satisfies residency requirements

## Context

Extracted from FEAT-014. This capability — node registry, database
placement, request proxying, and database migration — is **deferred**: see
the "Distributed Placement and Migration" entry in
`docs/helix/parking-lot.md` (PRD Non-Goals and FR-27 deferral). The story is
retained so the parked scope stays concrete; it carries no active feature
requirements. FEAT-014's TEN-20 (placement-independent addressing)
deliberately keeps this future work client-transparent.

## Walkthrough

1. Wei registers a node with a region and endpoint.
2. Wei creates a database placed on a specific node.
3. Requests arriving at any node for that database are proxied or redirected to its primary node.
4. Wei migrates the database to another node without changing its address.

## Acceptance Criteria

- [ ] **US-039-AC1** — Given a running deployment, when a node is registered with a name, region, and endpoint, then it appears in the node registry.
- [ ] **US-039-AC2** — Given registered nodes, when a database is created with a placement target, then the database is placed on that node.
- [ ] **US-039-AC3** — Given a database placed on a remote node, when a request for it arrives at another node, then the request is proxied or redirected to the primary node.
- [ ] **US-039-AC4** — Given a placed database, when it is migrated to another node, then its canonical URLs continue to resolve unchanged throughout and after the migration.

## Edge Cases

- **Node offline**: requests to databases on an offline node fail with a service-unavailable error; no automatic failover.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Register node | US-039-AC1 | Deployment with one node | Register `eu-west` node | Node listed in registry |
| Placed creation | US-039-AC2 | Nodes `us-east`, `eu-west` | Create database placed on `eu-west` | Placement recorded on `eu-west` |
| Remote routing | US-039-AC3 | Database on `eu-west` | Request via `us-east` | Proxied/redirected; correct response |
| Transparent migration | US-039-AC4 | Database on `eu-west` | Migrate to `us-east` | Same URLs resolve before, during, after |

## Dependencies

- **Stories**: US-035 (database model).
- **Feature Spec**: [FEAT-014 — Tenancy, Namespace Hierarchy, and Path-Based Addressing](../features/FEAT-014-multi-tenancy.md) (Out of Scope — parked)
- **Feature Requirements**: None active (parked scope)
- **PRD Requirements**: FR-27 (deferred), P2 #3
- **External**: `docs/helix/parking-lot.md` ("Distributed Placement and Migration"); ADR-011 (node topology and migration protocol design record)

## Out of Scope

- Automatic failover and consensus.
- Multi-region replication.

## Review Checklist

- [ ] Persona comes from the PRD
- [ ] Every AC is independently testable with one Given/When/Then
- [ ] No exact API/CLI/event/schema surface inline; Contract IDs referenced
- [ ] Test scenarios cover the happy path and at least one edge case
