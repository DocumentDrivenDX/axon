# Tenant Control Plane

Multi-tenant path routing, users, credentials, JWT grant bounds, revocation, deployment registration, and BYOC isolation.

- Release target: Axon 0.7.1
- Persona: Operators running Axon for multiple tenants or deployments.
- Demo reel: `tenant-control-reel`
- Website page: `website/content/docs/demo-reels/tenant-control-reel.md`
- Coverage entries: 26

## Files

- `schemas/`: JSON Schemas for every collection in the example.
- `seed/`: JSONL seed data by collection.
- `demo.sh`: CLI script that loads schemas, entities, links, and representative queries.

## Workflow

1. Register two deployments with same-named tenant data boundaries.
2. Issue user membership and credential-grant fixtures.
3. Exercise path-routed reads and writes so cross-tenant access fails closed.
4. Revoke a credential and confirm the audit record names the operator.

## Covered HELIX Entries

- feature: FEAT-012 - Authentication, Identity, and Authorization
- feature: FEAT-014 - Tenancy, Namespace Hierarchy, and Path-Based Addressing
- feature: FEAT-025 - BYOC Deployment Control Plane
- scenario: SCN-011 - Cross-Tenant Isolation via Path Routing (FEAT-014, ADR-018)
- scenario: SCN-012 - User in Two Tenants with Different Roles (FEAT-012, FEAT-014)
- scenario: SCN-013 - JWT Credential Grant Enforcement and Revocation (FEAT-012)
- scenario: SCN-014 - Authentication Rejection Matrix (ADR-018 §4)
- scenario: SCN-015 - Default-Tenant Bootstrap Under Concurrency (FEAT-014, ADR-018 §6)
- scenario: SCN-016 - BYOC Deployment Boundary (FEAT-025, ADR-017)
- story: US-035 - Create and Use a Database (within a tenant)
- story: US-036 - Organize Collections with Schemas
- story: US-037 - Zero-Config Default Tenant for Dev Mode
- story: US-038 - Scope Access to a Specific Database via Tenant Membership
- story: US-039 - Register Nodes and Track Placement
- story: US-043 - Authenticate via Tailscale
- story: US-044 - Role-Based Access Control
- story: US-087 - Create a Tenant with Multiple Databases
- story: US-088 - Users Are Members of Multiple Tenants
- story: US-089 - First-Class User with Tailscale Auto-Provisioning
- story: US-090 - JWT Credential for Integration Access
- story: US-091 - User in Multiple Tenants
- story: US-123 - Development Without Auth
- story: US-124 - Per-Principal Role Assignment
- story: US-144 - Provision and Register a BYOC Deployment
- story: US-145 - Observe a Fleet Without Reading Tenant Data
- story: US-146 - Deprovision with Retention Guarantees
