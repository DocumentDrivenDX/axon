---
ddx:
  id: FEAT-003
  depends_on:
    - helix.prd
  review:
    self_hash: 15881e4941cec74cf6e0be6d023da0a34cb4f1f4efb5efbb6a9b8246e037010f
    deps:
      helix.prd: dff98156a6cc934f406611b78b513892d85cee1bd7b4c011f045146fcdfd23e1
    reviewed_at: "2026-06-14T04:39:42Z"
---
# Feature Specification: FEAT-003 — Audit Log

**Feature ID**: FEAT-003
**Status**: draft
**Priority**: P0
**Owner**: Core Team
**Covered PRD Subsystem(s)**: Audit, Change Capture, and Repair
**Covered PRD Requirements**: FR-15, FR-16, FR-17
**Cross-Subsystem Rationale**: None — single subsystem.
**Requirement Prefix**: AUD

## Overview

The audit log is Axon's immutable record of everything that happened. Every mutation — entity creates, updates, deletes, link lifecycle, collection lifecycle, schema and template changes — produces an audit entry with repair-grade provenance. This feature implements PRD FR-15 (immutable audit record per mutation), FR-16 (operator history queries), and FR-17 (causal-chain reconstruction and manual repair).

## Ideal Future State

An operator or developer can answer "what happened to this record, who or what did it, under what authority, and what did it look like before?" for any business record, at any time, with one query. Any single bad mutation can be reverted from its audit entry without manual data surgery, and the revert itself becomes part of the permanent history. Provenance-aware external systems can consume Axon lineage through a standard vocabulary without bespoke translation.

## Problem Statement

- **Current situation**: Agent state changes in the DIY stack are fire-and-forget. History, when present, is scattered across application logs that cannot reconstruct actor authority, tool origin, or before/after state.
- **Pain points**: Developers cannot debug agent behavior, cannot revert mistakes without manual data surgery, and cannot prove compliance. When preventive controls fail, the change history is too thin to repair damage.
- **Desired outcome**: A complete, queryable, immutable record of every state change with full provenance, strong enough to power investigation, manual repair, and (in FEAT-023) automated rollback.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Audit capture | "Is every change recorded, no matter the path?" | Produce one immutable entry per mutation through the shared handler path |
| Audit query | "What happened to this entity / who did this / what changed last night?" | History queries by entity, collection, actor, operation, and time, plus a multi-collection ordered tail |
| Entity revert | "Undo this one bad change" | Restore an entity to a recorded prior state, itself audited |
| Provenance interchange | "Can my lineage tooling consume this?" | Additive PROV-O / JSON-LD serialization of the same entries |

## Requirements

### Functional Requirements by Area

#### Audit Capture

- **AUD-01**. Every mutation — entity, link, collection lifecycle, schema, and template — must produce exactly one immutable audit entry through the shared handler path. No public surface may bypass audit capture.
- **AUD-02**. Each audit entry must record what happened (an operation drawn from the audit operation taxonomy), when (a server-assigned timestamp), who or what acted (actor and delegated authority), where (tenant, database, collection, entity/link identity), and the full before state, after state, and structured diff of the affected record. The normative field set and operation taxonomy are defined in [CONTRACT-005 — Audit Record](../../02-design/contracts/CONTRACT-005-audit-record.md).
- **AUD-03**. The operation taxonomy must be extensible by owning feature specs (for example, lifecycle and template operations) without changing the audit entry shape; extensions are registered in CONTRACT-005.
- **AUD-04**. Audit entries must be append-only: no public API operation may modify or delete an existing entry.
- **AUD-05**. Audit entries within a database must be totally ordered by entry ID; entry IDs are unique and monotonically increasing. Cross-database ordering is not guaranteed.
- **AUD-06**. Callers must be able to attach optional key-value audit metadata (reason, correlation ID, agent session) to any mutation. Metadata is stored with the entry and returned on query, and never affects the operation's outcome.
- **AUD-07**. Collection creation and drop, and schema changes, must be audited like any other mutation.

#### Audit Query

- **AUD-08**. Operators must be able to query audit history filtered by collection, entity ID, actor, operation type, and time range, with cursor-based pagination. The filter and pagination surface is defined in CONTRACT-005 and [CONTRACT-001 — HTTP API Surface](../../02-design/contracts/CONTRACT-001-http-api-surface.md) (audit query and tail endpoints).
- **AUD-09**. Operators must be able to follow an ordered audit tail spanning multiple collections in one stream, so cross-collection workflows can be observed without merging per-collection queries client-side.
- **AUD-10**. Unsupported audit filters must be rejected with a structured error naming the supported filters (per CONTRACT-001).

#### Entity Revert

- **AUD-11**. Given an audit entry, the affected entity must be restorable to that entry's before state. The revert is an ordinary governed mutation: it is validated against the active schema and produces a new audit entry. The audit log never loses information.
- **AUD-12**. When the restored state does not validate against the current schema, the revert must fail with a clear, structured error; an explicit force option may bypass schema validation with a warning.

#### Provenance Interchange

- **AUD-13**. Audit queries must offer an additive PROV-O / JSON-LD serialization of the same entries, selected by content negotiation or query parameter, with the native JSON shape unchanged. The serialization mapping, IRI rules, and negotiation surface are defined in CONTRACT-005 §PROV-O / JSON-LD serialization.
- **AUD-14**. PROV-O output must round-trip: native audit JSON serialized to PROV-O and re-imported preserves all auditable facts.

### Non-Functional Requirements

- **Performance**: Audit capture must add no more than 2 ms to mutation latency; typical audit queries (single entity, recent time range) return in under 100 ms.
- **Reliability**: Audit entries are written atomically with the mutation they record — a mutation without its audit entry (or vice versa) must be impossible.
- **Storage**: Audit entries are stored durably in the same backend as the entities they audit; V1 retains all entries.
- **Scalability**: Sustained write throughput at the PRD single-entity latency target must not be bottlenecked by audit capture.

## User Stories

| ID | Title | Link |
|----|-------|------|
| US-007 | Query the Audit Trail | [US-007](../user-stories/US-007-query-the-audit-trail.md) |
| US-008 | Revert an Entity to Previous State | [US-008](../user-stories/US-008-revert-an-entity-to-previous-state.md) |
| US-009 | Attach Metadata to Mutations | [US-009](../user-stories/US-009-attach-metadata-to-mutations.md) |
| US-079 | Multi-Collection Audit Tail | [US-079](../user-stories/US-079-multi-collection-audit-tail.md) |
| US-120 | PROV-O Audit Shape | [US-120](../user-stories/US-120-prov-o-audit-shape.md) |

US-120 was renumbered from this spec's former US-010 (ID retained by FEAT-004
"CRUD an Entity") per the user-story ID registry.

## Edge Cases and Error Handling

- **Revert to incompatible schema**: Entity state from an old audit entry may not validate against the current schema. Revert fails with a clear structured error; an explicit force option bypasses schema validation with a warning.
- **High-volume writes**: Under high write throughput, audit capture must not become a bottleneck; the 2 ms overhead budget holds at sustained load.
- **Missing actor identity**: If no actor identity is available (for example, embedded mode with no auth), the entry is still created with the documented anonymous actor value (CONTRACT-005).
- **Large entities**: Before/after state for large entities is stored in full in V1; the entry is never truncated silently.
- **Clock skew**: Timestamps are assigned by the serving instance (local time in embedded mode, server time in server mode). Ordering guarantees come from entry IDs, not timestamps.

## Success Metrics

- 100% of mutations have corresponding audit entries (zero gaps) under the audit-coverage contract test suite.
- Typical audit queries (single entity, recent time range) return in under 100 ms.
- A developer can trace any state change back to its cause with a single query or CLI invocation.

## Constraints and Assumptions

### Constraints

- The audit log is append-only and immutable through all public operations.
- Audit entries are stored in the same database as the collections they audit.
- Full before/after state is stored in V1; diff-only storage is a deferred optimization.
- PROV-O is an additive serialization in V1; the native JSON shape remains canonical. A future amendment may promote PROV-O to canonical if integrations justify it.

### Assumptions

- Most audit queries target a specific entity or a narrow time range.
- Audit log size is manageable for V1 use cases (single-digit GB).
- Developers value completeness over storage efficiency for audit trails.

## Dependencies

- **Other features**: None — the audit log is foundational. FEAT-023 (Rollback and Recovery) extends single-entry revert into transaction and point-in-time repair; FEAT-021 (Change Feeds) consumes audit entries for CDC.
- **External services**: None. Normative surfaces: [CONTRACT-005 — Audit Record](../../02-design/contracts/CONTRACT-005-audit-record.md) (entry fields, operation taxonomy, PROV-O serialization), [CONTRACT-001 — HTTP API Surface](../../02-design/contracts/CONTRACT-001-http-api-surface.md) (audit query/tail endpoints), [CONTRACT-008 — CLI and Config](../../02-design/contracts/CONTRACT-008-cli-and-config.md) (audit CLI commands).
- **PRD requirements**: FR-15, FR-16, FR-17 (P0).

## Out of Scope

- Configurable retention policies, tiered or compressed audit storage.
- Cross-database audit correlation.
- Bulk audit export pipelines (change feeds are FEAT-021).
- Audit tamper detection / cryptographic chaining.
- Transaction-level and point-in-time rollback workflows (FEAT-023; this feature provides the single-entry revert primitive).

## Review Checklist

Use this checklist when reviewing a feature specification:

- [ ] Covered PRD Subsystem(s) and Requirements (`FR-n`) are listed; a feature spanning >1 subsystem carries an explicit cross-subsystem rationale (else split per the Decomposition test)
- [ ] Functional areas (if any) are subordinate parts of this one capability, not separate capabilities
- [ ] Overview connects this feature to a specific PRD requirement
- [ ] Ideal future state describes the desired user-visible outcome, not only current problems
- [ ] Every functional requirement is testable — you can write an assertion for it
- [ ] Acceptance criteria are defined in the user stories that decompose this feature, not here (ADR-009)
- [ ] Non-functional requirements have specific numeric targets
- [ ] Edge cases cover realistic failure scenarios, not just happy paths
- [ ] Success metrics are specific to this feature, not product-level metrics
- [ ] Dependencies reference real artifact IDs
- [ ] Out of scope excludes things someone might reasonably assume are in scope
- [ ] No exact API/CLI/event/schema/config surface is defined inline; normative surface links to Contract artifacts
