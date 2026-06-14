---
ddx:
  id: US-037
  review:
    self_hash: 37168cff635ce65377dc12b1def9770fcf80bf15d5844b42c16daf77d6ceb2d8
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---
# US-037: Zero-Config Default Tenant for Dev Mode

**Feature**: FEAT-014 — Tenancy, Namespace Hierarchy, and Path-Based Addressing
**Feature Requirements**: TEN-05, TEN-06, TEN-11
**PRD Requirements**: FR-25
**Priority**: P1
**Status**: Draft

## Story

**As a** Ava, an agent application developer running Axon locally
**I want** a working default tenant and database without explicit provisioning
**So that** I can start building immediately after starting the server

## Context

Extracted from FEAT-014. Exercises the idempotent, concurrency-safe default
tenant bootstrap (TEN-05), the no-auth dev mode semantics (TEN-06), and the
default database convenience (TEN-11). The normative bootstrap concurrency
pattern is ADR-018 Section 6; the dev-mode flag is owned by CONTRACT-008.

## Walkthrough

1. Ava starts a fresh server and makes her first authenticated request.
2. Axon auto-creates a `default` tenant with Ava as sole admin, plus a `default` database and `default` schema.
3. Subsequent requests against the default tenant/database work with no provisioning.
4. The CLI defaults to the default tenant and database when no flags are given.

## Acceptance Criteria

- [ ] **US-037-AC1** — Given a fresh deployment with zero tenants, when the first successful authenticated request arrives, then a `default` tenant is auto-created with the authenticating user as sole admin, along with a `default` database and `default` schema.
- [ ] **US-037-AC2** — Given the bootstrap has run, when subsequent requests address the default tenant and database, then they succeed with no explicit provisioning step.
- [ ] **US-037-AC3** — Given the CLI with no tenant/database flags, when an entity operation runs, then it targets the `default` tenant and `default` database.
- [ ] **US-037-AC4** — Given any tenant already exists, when further requests arrive, then the bootstrap does not re-run (idempotent).
- [ ] **US-037-AC5** — Given two simultaneous first-requests on a fresh deployment, when both complete, then exactly one default tenant exists and neither request fails because of the race.
- [ ] **US-037-AC6** — Given the server runs in no-auth dev mode, when a request names any tenant/database path, then it is honored with a synthesized in-memory context and no persistent tenant or user records are written.

## Edge Cases

- **Explicitly created tenants**: get no auto-database; databases must be created through the control plane (TEN-11).
- **No-auth persistence**: nothing from no-auth mode survives process restart unless the configured storage adapter persists it.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Bootstrap | US-037-AC1 | Fresh deployment | First authenticated request | Default tenant + database + schema created; caller is sole admin |
| Zero-config use | US-037-AC2 | Bootstrap done | CRUD against default scope | Succeeds |
| CLI defaults | US-037-AC3 | No tenant/database flags | CLI entity create | Targets default/default |
| Idempotence | US-037-AC4 | A tenant exists | New requests | No re-bootstrap |
| Concurrent first-requests | US-037-AC5 | Fresh deployment | Two simultaneous requests | Exactly one default tenant; no race failure |
| No-auth mode | US-037-AC6 | Server in no-auth mode | Request to arbitrary tenant/database path | Works in-memory; no persistent rows |

## Dependencies

- **Stories**: US-035 (database semantics).
- **Feature Spec**: [FEAT-014 — Tenancy, Namespace Hierarchy, and Path-Based Addressing](../features/FEAT-014-multi-tenancy.md)
- **Feature Requirements**: TEN-05, TEN-06, TEN-11
- **PRD Requirements**: FR-25
- **External**: CONTRACT-008 (dev-mode flag and CLI defaults); ADR-018 §6 (bootstrap concurrency pattern)

## Out of Scope

- Authentication mechanics and identity resolution (FEAT-012).

## Review Checklist

- [ ] Persona comes from the PRD
- [ ] Every AC is independently testable with one Given/When/Then
- [ ] No exact API/CLI/event/schema surface inline; Contract IDs referenced
- [ ] Test scenarios cover the happy path and at least one edge case
