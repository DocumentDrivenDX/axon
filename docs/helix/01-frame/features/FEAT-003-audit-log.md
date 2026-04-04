---
dun:
  id: FEAT-003
  depends_on:
    - helix.prd
---
# Feature Specification: FEAT-003 - Audit Log

**Feature ID**: FEAT-003
**Status**: Draft
**Priority**: P0
**Owner**: Core Team
**Created**: 2026-04-04
**Updated**: 2026-04-04

## Overview

The audit log is Axon's immutable record of everything that happened. Every mutation — document creates, updates, deletes, collection lifecycle events — produces an audit entry with actor, timestamp, operation type, and before/after state. The audit log is not a feature bolted onto Axon; it is the architecture. Writes go to the audit log first; the current state is a projection of the audit log.

## Problem Statement

When agents modify state, there is no trace of what happened, who/what did it, or why. Debugging agent behavior requires reconstructing state changes from scattered logs. Reverting a bad agent action means manual data surgery. Compliance requirements demand full audit trails that most ad-hoc storage solutions can't provide.

- Current situation: Agent state changes are fire-and-forget. No history, no provenance
- Pain points: Can't debug agent behavior, can't revert mistakes, can't prove compliance
- Desired outcome: Complete, queryable, immutable record of every state change with full provenance

## Requirements

### Functional Requirements

- **Audit on every mutation**: Every create, update, and delete produces an audit entry. No bypass path exists
- **Audit entry structure**: Each entry contains:
  - `id`: Unique, monotonically increasing audit entry ID
  - `timestamp`: Server-assigned UTC timestamp (nanosecond precision)
  - `actor`: Who/what performed the operation (user ID, agent ID, API key ID, or "system")
  - `operation`: One of `document.create`, `document.update`, `document.delete`, `collection.create`, `collection.drop`, `schema.update`
  - `collection`: Collection name
  - `document_id`: Document ID (for document operations)
  - `before`: Full document state before the operation (null for creates)
  - `after`: Full document state after the operation (null for deletes)
  - `diff`: Structured diff of changed fields (for updates)
  - `metadata`: Optional key-value metadata (e.g., reason, correlation ID, agent session)
- **Immutability**: Audit entries cannot be modified or deleted through normal API operations. The audit log is append-only
- **Query audit log**: Query audit entries by collection, document ID, actor, operation type, time range. Support pagination
- **Revert from audit**: Given an audit entry, restore a document to its `before` state. The revert itself produces a new audit entry
- **Audit log for collections**: Collection creation and drop events are also audited

### Non-Functional Requirements

- **Performance**: Audit writes must not add more than 2ms to mutation latency. Append-only writes are inherently fast
- **Storage**: Audit entries are stored durably. V1 stores in the same backend as documents. Tiered storage is P2
- **Retention**: V1 retains all audit entries. Configurable retention policies are P2
- **Ordering**: Audit entries within a database are totally ordered by ID. Cross-database ordering is not guaranteed

## User Stories

### Story US-007: Query the Audit Trail [FEAT-003]

**As a** developer debugging agent behavior
**I want** to query the audit log for a specific document or time range
**So that** I can understand what happened and reconstruct the sequence of events

**Acceptance Criteria:**
- [ ] `axon audit list --collection <name> --document <id>` shows all mutations for a document in chronological order
- [ ] `axon audit list --collection <name> --since <time> --until <time>` filters by time range
- [ ] `axon audit list --actor <id>` filters by who/what made the change
- [ ] Each entry shows operation, actor, timestamp, and diff
- [ ] API returns paginated results with cursor-based pagination

### Story US-008: Revert a Document to Previous State [FEAT-003]

**As a** developer who discovered an agent made a bad change
**I want** to revert a document to a previous state using the audit log
**So that** I can undo agent mistakes without manual data editing

**Acceptance Criteria:**
- [ ] `axon audit revert <entry-id>` restores the document to the `before` state of that audit entry
- [ ] The revert itself produces a new audit entry (the audit log never loses information)
- [ ] Revert validates the restored state against the current schema
- [ ] If the schema has evolved since the audit entry, revert warns or fails with a clear message

### Story US-009: Attach Metadata to Mutations [FEAT-003]

**As an** agent performing operations
**I want** to attach context (reason, session ID, correlation ID) to my writes
**So that** the audit trail carries meaningful provenance beyond just "what changed"

**Acceptance Criteria:**
- [ ] API accepts optional `audit_metadata` on write operations
- [ ] Metadata is stored with the audit entry and returned in queries
- [ ] Metadata keys and values are strings (simple key-value)
- [ ] Metadata does not affect the operation itself — it's purely informational

## Edge Cases and Error Handling

- **Revert to incompatible schema**: Document state from an old audit entry may not validate against the current schema. Revert fails with a clear error. Force-revert option bypasses schema validation (with warning)
- **High-volume writes**: Under high write throughput, audit log must not become a bottleneck. Batch audit writes internally if needed
- **Actor identification**: If no actor is provided (e.g., embedded mode with no auth), actor defaults to "anonymous" — but the entry is still created
- **Large documents**: Before/after state for large documents may consume significant storage. V1 stores full state; compression and diff-only storage are P2 optimizations
- **Clock skew**: In embedded mode, timestamps are local system time. Server mode uses server time. Cross-instance time ordering requires distributed timestamps (P2)

## Success Metrics

- 100% of mutations have corresponding audit entries (zero gaps)
- Audit queries return results in < 100ms for typical queries (single document, recent time range)
- Developers can trace any state change back to its cause within one CLI command

## Constraints and Assumptions

### Constraints
- Audit log is append-only and immutable through normal operations
- Audit entries are stored in the same database as the collections they audit
- Full before/after state stored in V1 (diff-only storage is a P2 optimization)

### Assumptions
- Most audit queries are for a specific document or narrow time range
- Audit log size will be manageable for V1 use cases (single-digit GB)
- Developers value completeness over storage efficiency for audit trails

## Dependencies

- None (audit log is foundational, alongside schema engine)

## Out of Scope

- Configurable retention policies (P2)
- Tiered/compressed audit storage (P2)
- Cross-database audit correlation (P2)
- Audit log export (P2)
- Audit tamper detection / cryptographic chaining (P2)

## Traceability

### Related Artifacts
- **Parent PRD Section**: Requirements Overview > P0 #3 (Audit Log)
- **User Stories**: US-007, US-008, US-009
- **Prior Art**: niflheim WAL, event sourcing patterns, DoltDB commit log
- **Test Suites**: `tests/FEAT-003/`
- **Implementation**: `src/audit/` or equivalent

### Feature Dependencies
- **Depends On**: None
- **Depended By**: FEAT-001 (Collections — lifecycle audit), FEAT-004 (Document Operations — mutation audit)
