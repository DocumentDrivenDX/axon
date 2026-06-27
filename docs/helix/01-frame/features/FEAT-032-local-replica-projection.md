---
ddx:
  id: FEAT-032
  depends_on:
    - helix.prd
  links:
    depends_on:
      - FEAT-021
      - CONTRACT-006
      - CONTRACT-009
  # TODO: refresh review stamp (new spec authored 2026-06-27; deps unstamped)
---
# Feature Specification: FEAT-032 — Local Replica Projection

**Feature ID**: FEAT-032
**Status**: draft
**Priority**: P2
**Owner**: Core Team
**Covered PRD Subsystem(s)**: Audit, Change Capture, and Repair; API and Deployment Surfaces
**Covered PRD Requirements**: FR-32; supporting FR-23 (embedded mode), FR-26 (storage portability for embedded), FR-31 (resumable scoped change streams)
**Cross-Subsystem Rationale**: The cross-subsystem workflow IS the feature: a
governed local read replica is one capability that spans the change-capture
subsystem (it consumes the FR-31/FR-18 change stream) and the API/SDK surface
subsystem (it is delivered through the SDK as a client-side store). Splitting
"the projection" from "the client surface that consumes it" would leave neither
half shippable — the replica is the pairing of a durable server-side cursor and
a client-side materialized store.
**FR Prefix**: LRP

## Overview

A governed client-side **local read replica**: clients maintain a local store
fed by Axon's existing resumable, scoped change stream (FR-31, FEAT-021) and
query it locally — search, sort, filter, traverse — for responsive UIs (PRD
FR-32). This is the CQRS read half of "local-first": the server remains the
single committing authority and the local replica is a read-only projection of
the same audit-derived change stream that powers CDC and GraphQL subscriptions.
Offline writes and reconciliation are explicitly **not** part of this feature
(deferred as FR-33; see `docs/helix/parking-lot.md`).

This feature **builds on the existing CQRS substrate** — the audit log is the
source of truth and CDC/subscriptions are already projections of it (ADR-014).
It does not introduce a new event model; it extends the same projection and
cursor vocabulary to a client-resident consumer.

## Ideal Future State

A developer using the SDK calls into a local store that is always populated and
queryable. The first connection bootstraps a snapshot scoped to what the
subject may see, then tails the live change stream; on disconnect and reconnect
the client resumes exactly where it left off using an opaque cursor token,
without re-downloading the world and without missing events. Deletes are
applied as tombstones so the local store never shows phantom rows. Search,
sort, filter, and traversal run against the local store with no round-trip, and
every record in the local store has already passed policy redaction — the
replica can hold nothing the subject is not allowed to see. UI latency is a
local-query latency, not a network latency.

## Problem Statement

- **Current situation**: Axon has a working CQRS substrate on the server. The
  audit log is the source of truth; CDC emits Debezium envelopes (FEAT-021,
  ADR-014, CONTRACT-006) and GraphQL subscriptions push live changes
  (FEAT-015). FR-32 was re-scoped on 2026-06-27 from offline read+write to a
  governed local read replica, but there is **no feature, no client replica,
  and several server gaps** (this is the AR-2026-06-27-full-repo.md §2 H1
  disposition). Specifically:
  - **No durable cursor backend.** Only `MemoryCursorStore` exists
    (`crates/axon-audit/src/cursor.rs`); it is explicitly not durable across
    process restarts. CONTRACT-006 §Cursor semantics requires a durable
    `_cdc_cursors`-style store, but no durable backend is implemented.
  - **Resume tokens are not opaque or stable.** The GraphQL subscription resume
    point is a raw `audit_id` string (`crates/axon-graphql/src/subscriptions.rs`
    — the `audit_id` field used as `since_audit_id` on reconnect). CONTRACT-006
    §Cursor semantics ("Cursor token") requires an **opaque** token that
    remains valid across producer restarts and schema changes. A raw `audit_id`
    leaks internal sequence numbering and is not schema-change-stable by
    contract.
  - **No client-side replica at all.** There is no local materialized store and
    no local query engine in the SDK; CONTRACT-009 (SDK surface) does not yet
    cover a replica consumer.
- **Pain points**: UIs that need responsive search/sort/filter must round-trip
  to the server for every interaction, or hand-roll a bespoke local cache with
  no resume/tombstone/redaction guarantees. Without this feature, FR-32 is an
  orphaned requirement with no owning capability.
- **Desired outcome**: A governed, resumable, policy-redacted local read
  replica exposed through the SDK, backed by a durable cursor store and opaque
  cursor tokens on the server. Measured by: bootstrap-then-tail correctness,
  resume-after-disconnect with no gaps or duplicates beyond at-least-once,
  tombstone correctness, and zero redacted-field leakage into the local store.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Server: durable cursor store | "Will my resume point survive a server restart?" | Provide a durable `CdcCursorStore` backend (today only `MemoryCursorStore` exists) so cursors persist across restarts per CONTRACT-006 |
| Server: opaque cursor tokens | "Can I hand my resume token back after a restart or schema change?" | Issue opaque, restart-stable, schema-change-stable cursor tokens per CONTRACT-006 §Cursor token (today the resume token is a raw `audit_id`) |
| Client SDK: materialized store + query engine | "Can I search/sort/filter locally without a round-trip?" | Maintain a local materialized store and a local query engine (search/sort/filter, and link traversal where the change stream carries link events) |
| Client SDK: bootstrap, tombstones, resume | "How does the replica become and stay correct?" | Snapshot + tail bootstrap, tombstone application on delete, resume-after-disconnect using the opaque cursor |
| Governance: redaction at projection | "Can the local store ever hold data I may not see?" | Apply policy redaction at projection time so the replica only ever materializes subject-visible, field-redacted data |

## Requirements

### Functional Requirements by Area

#### Server: Durable Cursor Store

LRP-01. The system must provide a durable `CdcCursorStore` backend that
persists cursor progress across server process restarts. Today only the
non-durable `MemoryCursorStore` (`crates/axon-audit/src/cursor.rs`) exists; a
durable backend (e.g. backed by the storage adapter / `_cdc_cursors` table per
CONTRACT-006 §Cursor semantics) must exist before a client can rely on
restart-stable resume. **In-scope gap.**

LRP-02. After a server restart, a previously issued cursor must still identify
the correct resume position with no replay before the persisted offset beyond
the documented at-least-once window.

#### Server: Opaque Cursor Tokens

LRP-03. The system must expose cursor tokens that are **opaque** to clients
(not a raw `audit_id`). Today the GraphQL subscription resume point is a raw
`audit_id` string (`crates/axon-graphql/src/subscriptions.rs`); this must be
replaced by an opaque token per CONTRACT-006 §Cursor token. **In-scope gap.**

LRP-04. A cursor token must remain valid across server restarts and across
schema changes to the scoped collections, per CONTRACT-006 §Cursor token.
Resuming with an old token after a schema change must not fail or silently skip
events.

LRP-05. The cursor token must carry (opaquely) the scope it was issued for
(database, schema, collection, entity/link, transaction per FR-31) so resume
re-establishes the same scoped stream.

#### Client SDK: Materialized Store and Query Engine

LRP-06. The SDK must maintain a local materialized store of the projected
records for the subscribed scope.

LRP-07. The SDK must provide a local query engine that can search, sort, and
filter the local store without a server round-trip, and traverse links where
the change stream carries link events. **In-scope gap** — no local query engine
exists today.

#### Client SDK: Bootstrap, Tombstones, Resume

LRP-08. On first subscription, the SDK must bootstrap by applying a snapshot
(`op: "r"` per CONTRACT-006 §Snapshot) for the scope and then tailing live
events from the snapshot boundary, with no gap between snapshot and tail.

LRP-09. The SDK must apply tombstones on delete so the local store never
returns entities/links that have been deleted on the server.

LRP-10. On disconnect and reconnect, the SDK must resume from its last opaque
cursor token without re-downloading the full snapshot and without losing events
(at-least-once; deduplicate by the cursor/source identity).

#### Governance: Redaction at Projection

LRP-11. Policy redaction must be applied **at projection time** so the local
store only ever materializes data the subject is permitted to see, with
field-level redactions already applied (architecture.md §Reads — "field
redaction at projection"; ADR-019). The replica must never hold a value that
policy would redact on a direct read. **In-scope gap / acceptance criterion** —
this is the primary data-exfiltration control (see threat-model.md: the replica
ships tenant data to client devices).

LRP-12. The projected stream must be scoped to the subject's authorization at
projection time, not filtered client-side after delivery; denied rows and
redacted fields must never leave the server in the stream.

### Non-Functional Requirements

- **NFR-performance**: Local search/sort/filter over a materialized store of up
  to 100k records returns in < 50 ms p95 on a typical client device, with no
  network round-trip.
- **NFR-security**: Zero redacted-field leakage — a property/fixture test must
  show that no field a subject's policy redacts ever appears in the local store
  or on the wire (LRP-11/LRP-12). The change stream must be transport-protected
  (TLS) end to end.
- **NFR-reliability**: After an induced disconnect at an arbitrary point,
  resume must converge the local store to the server state with no missing
  events and no phantom (un-tombstoned) rows; duplicates bounded by the
  documented at-least-once semantics.
- **NFR-scalability**: Cursor token validity and durable cursor storage must
  hold across server restarts and across at least one schema-compatible
  migration of every scoped collection.

## User Stories

<!-- TODO: decompose into user stories under docs/helix/01-frame/user-stories/
     when this feature is scheduled (acceptance criteria live in stories per
     ADR-009). -->

## Edge Cases and Error Handling

- **Schema change mid-tail**: A scoped collection's schema changes while a
  client is tailing — the opaque cursor must remain valid (LRP-04) and the
  client must continue without a forced full re-bootstrap.
- **Cursor older than retention**: A client presents a cursor whose offset has
  been compacted/aged out — the server must signal that a fresh snapshot is
  required rather than silently skipping events.
- **Tombstone before snapshot completes**: A delete arrives for an entity not
  yet seen in the snapshot — application must remain consistent (the entity must
  not reappear).
- **Policy change narrowing visibility**: A subject loses access to records
  already in their local store — projection-time redaction must stop streaming
  them; behavior for already-materialized records (purge vs. stop-updating)
  must be defined when stories are written.
- **Reconnect storm / duplicate delivery**: At-least-once redelivery must be
  deduplicated by the client without corrupting the store.

## Success Metrics

- 100% of redaction fixtures show no redacted field reaching the local store or
  the wire (the data-exfiltration guarantee).
- Resume-after-disconnect converges to server state in 100% of fault-injection
  runs (no missing events, no phantom rows).
- Local search/sort/filter p95 < 50 ms over a 100k-record store with no
  round-trip.
- A durable cursor backend and opaque cursor tokens pass a server-restart and a
  schema-change conformance test (closing the two server gaps).

## Constraints and Assumptions

- **Read-only.** The local replica never accepts writes; offline writes +
  reconciliation are FR-33 (deferred, parking lot). The server is the single
  committing authority.
- Builds on the existing CQRS substrate: the audit log is the source of truth
  and CDC/subscriptions are already projections of it (ADR-014). This feature
  does not introduce a new event model.
- One cursor vocabulary across GraphQL subscriptions, MCP resource
  notifications, SDK change readers, and CDC sinks (CONTRACT-006 §Cursor
  vocabulary parity).
- Assumes FR-31 (resumable, scoped change streams) and FEAT-021 (CDC) are the
  stream source; assumes FEAT-029 policy enforcement is available at projection
  time.

## Dependencies

- **Other features**: FEAT-021 (Change Feeds / CDC — stream source and cursor
  semantics); FEAT-029 (Data-Layer Access Control — projection-time redaction);
  FEAT-015 (GraphQL subscriptions — live-tail transport); FEAT-028 (unified
  binary / embedded mode, FR-23).
- **Contracts**: CONTRACT-006 (CDC envelope and cursor semantics — durable
  cursor, opaque token, snapshot, vocabulary parity); CONTRACT-009 (SDK surface
  — the SDK replica/query consumer to be added here).
- **Design**: ADR-014 (audit-as-source-of-truth, CDC-as-projection — amended to
  name the client replica as a first-class consumer); ADR-025 (client-projection
  cursor API — opaque restart/schema-stable resume tokens, snapshot+tail).
- **PRD requirements**: FR-32 (P1, governed local read replica); supporting
  FR-23, FR-26, FR-31. Explicitly **not** FR-33 (deferred offline writes).

## Out of Scope

- **Offline local writes and reconciliation** — FR-33, deferred to the parking
  lot. The replica is read-only.
- **A new event/envelope format** — reuses CONTRACT-006; no bespoke client
  format.
- **Server-side analytics/materialized views** — this is a client-resident
  read model, not a server query accelerator (those are FEAT-013/018).
- **Conflict resolution / CRDTs / merge semantics** — only relevant to offline
  writes (FR-33).

## Review Checklist

Use this checklist when reviewing a feature specification:

- [x] Covered PRD Subsystem(s) and Requirements (`FR-n`) are listed; a feature spanning >1 subsystem carries an explicit cross-subsystem rationale (else split per the Decomposition test)
- [x] Functional areas (if any) are subordinate parts of this one capability, not separate capabilities (each fails the ship/cut/metric test on its own)
- [x] Overview connects this feature to a specific PRD requirement
- [x] Ideal future state describes the desired user-visible outcome, not only current problems
- [x] Problem statement describes what exists now and what is broken — not just what is wanted
- [x] Functional areas are mapped when the feature spans multiple surfaces, workflows, or domain objects
- [x] Requirements are grouped by functional area when a flat list would mix unrelated scopes
- [x] Domain objects that sound similar are explicitly separated (read replica vs. offline-write reconciliation)
- [x] Every functional requirement is testable — you can write an assertion for it
- [ ] Acceptance criteria are defined in the user stories that decompose this feature, not here (ADR-009) — stories TODO when scheduled
- [x] Non-functional requirements have specific numeric targets, not "must be fast"
- [x] Edge cases cover realistic failure scenarios, not just happy paths
- [x] Success metrics are specific to this feature, not product-level metrics
- [x] Dependencies reference real artifact IDs (FEAT-XXX, CONTRACT-XXX, ADR-XXX)
- [x] Out of scope excludes things someone might reasonably assume are in scope
- [x] No implementation details beyond naming the existing gaps as evidence; requirements specify WHAT not HOW
- [x] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to CONTRACT-006/009
- [x] Feature is consistent with governing PRD requirements
- [x] No `[NEEDS CLARIFICATION]` markers remain (P2 feature; story-level detail deferred)
