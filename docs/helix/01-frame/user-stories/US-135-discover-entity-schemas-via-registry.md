---
ddx:
  id: US-135
  review:
    self_hash: f56f2e07361a97afa4f3d6079a29b3910ec1f80cd071622148cc272780ecaba2
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# US-135: Discover Entity Schemas via Registry

**Feature**: FEAT-021 — Change Feeds (CDC)
**Feature Requirements**: CDC-11, CDC-12, CDC-13, CDC-18
**PRD Requirements**: FR-18
**Priority**: P1
**Status**: Draft

## Story

**As a** Kafka consumer developer
**I want** a Confluent-compatible schema registry serving Axon entity schemas
**So that** I can generate typed consumer code and validate message formats

## Context

Renumbered from US-076 (collision with FEAT-009). Debezium-ecosystem
consumers (Kafka Connect sinks, codegen tooling) expect a Confluent-style
registry. FEAT-021 provides one as a facade over Axon's stored schema
versions (CDC-11..13) — no separate schema store. The registry endpoint
set, wire format, and port configuration are normative in CONTRACT-006.

## Walkthrough

1. The consumer developer points standard registry tooling at Axon's
   registry endpoint (CONTRACT-006).
2. The tooling lists subjects and sees the collection names.
3. The developer fetches the latest schema version for the `invoices`
   subject and receives the collection's entity schema as JSON Schema.
4. Code generation produces typed consumer classes from the schema.
5. A Kafka Connect sink registers a schema through the registry API; the
   registration maps onto Axon's schema store, and a compatibility check
   validates it against existing versions per Axon's evolution
   classification (FEAT-017).

## Acceptance Criteria

- [ ] **US-135-AC1** — Given collections exist, when a client requests the
      registry subject list (CONTRACT-006), then all collection names are
      returned as subjects.
- [ ] **US-135-AC2** — Given a collection with a schema, when a client
      requests the latest version for its subject, then the current entity
      schema is returned as JSON Schema.
- [ ] **US-135-AC3** — Given a registry restart, when a client re-fetches
      a previously issued schema ID, then the same ID resolves to the same
      schema (IDs are stable across restarts).
- [ ] **US-135-AC4** — Given a registry schema-registration request, when
      it is accepted, then it is applied to Axon's schema store — the
      registry maintains no separate store.
- [ ] **US-135-AC5** — Given an existing schema version, when a
      compatibility check is requested for a candidate schema, then the
      verdict matches Axon's schema-evolution classification (FEAT-017)
      for the corresponding compatibility mode.
- [ ] **US-135-AC6** — Given registry configuration (CONTRACT-006), when
      the operator sets a non-default registry port, then the registry
      serves on the configured port.

## Edge Cases

- **Schema change during consumption**: the registry serves both old and
  new versions; consumers pinned to the old version keep working for
  backward-compatible changes.
- **Subject for a dropped collection**: historical schema versions remain
  retrievable for consumers replaying old events.
- **Incompatible registration**: a registration that Axon classifies as
  breaking under the active compatibility mode is rejected with a
  structured error.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Subject listing | US-135-AC1 | Collections `invoices`, `vendors` | List subjects | Both names returned |
| Latest schema | US-135-AC2 | `invoices` schema v3 active | Fetch latest for `invoices` | v3 entity schema as JSON Schema |
| Stable IDs | US-135-AC3 | Schema ID 7 issued, registry restarted | Fetch schema ID 7 | Same schema returned |
| Compatibility check | US-135-AC5 | v3 schema; candidate removes a required field | Check compatibility (backward mode) | Rejected, consistent with FEAT-017 breaking classification |

## Dependencies

- **Stories**: none
- **Feature Spec**: FEAT-021
- **Feature Requirements**: CDC-11, CDC-12, CDC-13, CDC-18
- **PRD Requirements**: FR-18
- **External**: CONTRACT-006 (registry endpoints, wire format, port
  config); FEAT-017 (evolution classification)

## Out of Scope

- Pushing schemas to external registries (FEAT-021 Out of Scope); event
  emission and replay (US-130, US-132).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
