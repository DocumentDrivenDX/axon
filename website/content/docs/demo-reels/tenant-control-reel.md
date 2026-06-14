---
title: Tenant Control Plane Reel
weight: 14
prev: ../
---

# Tenant Control Plane Reel

Release target: Axon 0.7.1

Multi-tenant path routing, users, credentials, JWT grant bounds, revocation, deployment registration, and BYOC isolation.

- Sample project: [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane)
- Script source: [`docs/demos/reels/tenant-control-reel.md`](https://github.com/DocumentDrivenDX/axon/blob/master/docs/demos/reels/tenant-control-reel.md)
- Coverage entries: 26

## Storyboard

1. Register two deployments with same-named tenant data boundaries.
2. Issue user membership and credential-grant fixtures.
3. Exercise path-routed reads and writes so cross-tenant access fails closed.
4. Revoke a credential and confirm the audit record names the operator.

## Covered HELIX Entries

| Type | ID | Title | Source | Sample | Demo reel |
|---|---|---|---|---|---|
| feature | FEAT-012 | Authentication, Identity, and Authorization | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/features/FEAT-012-authorization.md) | [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane) | [tenant-control-reel](../tenant-control-reel/) |
| feature | FEAT-014 | Tenancy, Namespace Hierarchy, and Path-Based Addressing | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/features/FEAT-014-multi-tenancy.md) | [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane) | [tenant-control-reel](../tenant-control-reel/) |
| feature | FEAT-025 | BYOC Deployment Control Plane | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/features/FEAT-025-control-plane.md) | [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane) | [tenant-control-reel](../tenant-control-reel/) |
| scenario | SCN-011 | Cross-Tenant Isolation via Path Routing (FEAT-014, ADR-018) | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/03-test/test-plan.md) | [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane) | [tenant-control-reel](../tenant-control-reel/) |
| scenario | SCN-012 | User in Two Tenants with Different Roles (FEAT-012, FEAT-014) | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/03-test/test-plan.md) | [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane) | [tenant-control-reel](../tenant-control-reel/) |
| scenario | SCN-013 | JWT Credential Grant Enforcement and Revocation (FEAT-012) | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/03-test/test-plan.md) | [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane) | [tenant-control-reel](../tenant-control-reel/) |
| scenario | SCN-014 | Authentication Rejection Matrix (ADR-018 §4) | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/03-test/test-plan.md) | [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane) | [tenant-control-reel](../tenant-control-reel/) |
| scenario | SCN-015 | Default-Tenant Bootstrap Under Concurrency (FEAT-014, ADR-018 §6) | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/03-test/test-plan.md) | [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane) | [tenant-control-reel](../tenant-control-reel/) |
| scenario | SCN-016 | BYOC Deployment Boundary (FEAT-025, ADR-017) | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/03-test/test-plan.md) | [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane) | [tenant-control-reel](../tenant-control-reel/) |
| story | US-035 | Create and Use a Database (within a tenant) | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-035-create-and-use-a-database.md) | [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane) | [tenant-control-reel](../tenant-control-reel/) |
| story | US-036 | Organize Collections with Schemas | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-036-organize-collections-with-schemas.md) | [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane) | [tenant-control-reel](../tenant-control-reel/) |
| story | US-037 | Zero-Config Default Tenant for Dev Mode | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-037-zero-config-default-tenant-for-dev-mode.md) | [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane) | [tenant-control-reel](../tenant-control-reel/) |
| story | US-038 | Scope Access to a Specific Database via Tenant Membership | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-038-scope-access-via-tenant-membership.md) | [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane) | [tenant-control-reel](../tenant-control-reel/) |
| story | US-039 | Register Nodes and Track Placement | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-039-register-nodes-and-track-placement.md) | [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane) | [tenant-control-reel](../tenant-control-reel/) |
| story | US-043 | Authenticate via Tailscale | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-043-authenticate-via-tailscale.md) | [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane) | [tenant-control-reel](../tenant-control-reel/) |
| story | US-044 | Role-Based Access Control | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-044-role-based-access-control.md) | [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane) | [tenant-control-reel](../tenant-control-reel/) |
| story | US-087 | Create a Tenant with Multiple Databases | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-087-create-a-tenant-with-multiple-databases.md) | [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane) | [tenant-control-reel](../tenant-control-reel/) |
| story | US-088 | Users Are Members of Multiple Tenants | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-088-users-are-members-of-multiple-tenants.md) | [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane) | [tenant-control-reel](../tenant-control-reel/) |
| story | US-089 | First-Class User with Tailscale Auto-Provisioning | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-089-first-class-user-with-tailscale-auto-provisioning.md) | [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane) | [tenant-control-reel](../tenant-control-reel/) |
| story | US-090 | JWT Credential for Integration Access | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-090-jwt-credential-for-integration-access.md) | [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane) | [tenant-control-reel](../tenant-control-reel/) |
| story | US-091 | User in Multiple Tenants | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-091-user-in-multiple-tenants.md) | [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane) | [tenant-control-reel](../tenant-control-reel/) |
| story | US-123 | Development Without Auth | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-123-development-without-auth.md) | [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane) | [tenant-control-reel](../tenant-control-reel/) |
| story | US-124 | Per-Principal Role Assignment | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-124-per-principal-role-assignment.md) | [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane) | [tenant-control-reel](../tenant-control-reel/) |
| story | US-144 | Provision and Register a BYOC Deployment | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-144-provision-and-register-a-byoc-deployment.md) | [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane) | [tenant-control-reel](../tenant-control-reel/) |
| story | US-145 | Observe a Fleet Without Reading Tenant Data | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-145-observe-a-fleet-without-reading-tenant-data.md) | [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane) | [tenant-control-reel](../tenant-control-reel/) |
| story | US-146 | Deprovision with Retention Guarantees | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-146-deprovision-with-retention-guarantees.md) | [tenant-control-plane](https://github.com/DocumentDrivenDX/axon/tree/master/examples/tenant-control-plane) | [tenant-control-reel](../tenant-control-reel/) |
