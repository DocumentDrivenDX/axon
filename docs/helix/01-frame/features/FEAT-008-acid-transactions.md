---
ddx:
  id: FEAT-008
  depends_on:
    - helix.prd
  review:
    self_hash: de4e47fda5c2045ef2c4765371cac1caf29353ec4b5c78dbffb651d02b6eab82
    deps:
      helix.prd: dff98156a6cc934f406611b78b513892d85cee1bd7b4c011f045146fcdfd23e1
    reviewed_at: "2026-06-15T00:35:16Z"
---
# Feature Specification: FEAT-008 — ACID Transactions

**Feature ID**: FEAT-008
**Status**: draft
**Priority**: P0
**Owner**: Core Team
**Covered PRD Subsystem(s)**: Guardrailed Transactions and Mutation Intents
**Covered PRD Requirements**: FR-5; FR-6 (multi-entity scope)
**Cross-Subsystem Rationale**: None — single subsystem. Single-entity OCC (FR-6 single-entity scope) is owned by FEAT-004; this feature builds multi-entity semantics on it.
**Requirement Prefix**: TXN

## Overview

ACID transactions are Axon's multi-record correctness guarantee. When an agent debits account A and credits account B, both changes commit or neither does; when two concurrent transactions touch the same records, exactly one commits and the other is told why. This feature implements PRD FR-5 (atomic multi-entity/link commits) and the multi-entity scope of FR-6 (stale-write rejection with retry context).

## Ideal Future State

A developer expresses a multi-record business change — move an invoice to approved, update the vendor balance, create the ledger link — as one transaction and never reasons about partial failure. Concurrent agents work against shared state without locks, without lost updates, and with conflict responses rich enough to merge and retry programmatically. A lost network response is safe to retry: the same transaction is never applied twice.

## Problem Statement

- **Current situation**: Agent storage options either lack transactions entirely (object stores, JSON files, many BaaS products) or push locking and rollback logic onto application code.
- **Pain points**: Concurrent agents corrupt shared state through partial writes and silent overwrites; retry-after-timeout produces duplicate writes; developers hand-build compensation logic that drifts.
- **Desired outcome**: Atomic, isolated, audited multi-entity/link transactions with deterministic conflict behavior and safe retries.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Atomic multi-record commit | "Apply these changes together or not at all" | All-or-nothing transactions over entities and links across collections |
| Isolation and conflict resolution | "What do concurrent transactions see, and who wins?" | Snapshot isolation, first-writer-wins, structured conflict detail |
| Idempotent submission | "Is it safe to retry after a lost response?" | At-most-once application of a retried transaction |
| Audit integration | "Which changes happened together?" | Transaction identity threaded through atomic audit entries |

## Requirements

### Functional Requirements by Area

#### Atomic Multi-Record Commit

- **TXN-01**. A transaction containing entity and link operations must either fully commit or apply none of its staged operations.
- **TXN-02**. Transactions must span multiple collections within one database (for example, debit one account, credit another, and create a ledger link).
- **TXN-03**. Each operation in a transaction must carry its own version expectation; any single version conflict, schema violation, or invalid operation aborts the entire transaction with the specific cause.
- **TXN-04**. Transactions must be bounded: at most 100 operations per transaction, and open transactions are aborted after a configurable timeout (default 30 seconds).

#### Isolation and Conflict Resolution

- **TXN-05**. Transactions must read from a consistent snapshot: no dirty reads, non-repeatable reads, or phantoms within a transaction. Snapshot isolation is the V1 default. A **Serializable** level is available as an opt-in that, in addition to the write-set checks, validates the transaction's recorded **key-addressed read set** (entities read by id) is unchanged at commit — preventing write skew expressed over specific read entities. Serializable additionally supports a **collection-granular predicate/phantom guard** (ADR-026): a transaction records the structural version of each scanned collection via `record_scan_read` and aborts at commit if that collection's membership changed (any concurrent create/delete) — sound but conservative (over-aborts on non-matching inserts/deletes). **Delivered status (honesty):** the phantom guard is implemented and sound on **all** storage backends — a backend-agnostic membership-signature default plus native overrides (memory counter, SQLite ordered-id hash, PostgreSQL `md5(string_agg)` push-down); no backend fails closed. It is **membership-only**: it catches insert/delete phantoms but **not update-driven** predicate changes (e.g. a concurrent `status: open → closed` flipping a `WHERE status = open` count). Reads through the transaction-aware handler methods — `tx_get_entity` (key-addressed), `tx_query_entities`, `tx_aggregate`, `tx_traverse` (scan / phantom), and Cypher read footprints (`tx_record_cypher_scan`, which records the collections a Cypher query references) — **auto-capture** into the transaction's read sets, so callers cannot forget. (Wiring GraphQL named-query reads to run *inside* a commit-transaction is a remaining integration, not a guard gap.) **Update-driven predicate serializability and precise SSI** (minimal aborts) remain out of scope — see Constraints, ADR-004, and ADR-026. The effective isolation level must be inspectable per transaction.
- **TXN-06**. Conflict resolution must be first-writer-wins: the first transaction to commit wins, and later transactions whose write set overlaps are aborted with a conflict.
- **TXN-07**. An abort response must identify which entity or link caused the conflict, include its current committed state, and indicate whether the failure is retryable (version conflicts are; schema violations are not). The wire semantics — HTTP/gRPC status mapping, error codes, conflict detail, and the retryable flag — are defined in [CONTRACT-001 — HTTP API Surface](../../02-design/contracts/CONTRACT-001-http-api-surface.md).
- **TXN-08**. Single-entity reads and writes must behave per FEAT-004's optimistic-concurrency requirements (ENT-10..ENT-12); this feature must not introduce divergent single-entity semantics.

#### Idempotent Submission

- **TXN-09**. A transaction submitted with an idempotency key must be applied at most once: a retry within the key's lifetime returns the original response without re-execution, and concurrent duplicates are rejected as retryable. The full idempotency protocol (key format, scope, TTL, caching rules, in-flight behavior) is defined in CONTRACT-001 §Transaction and idempotency protocol.

#### Audit Integration

- **TXN-10**. Every transaction must be assigned a unique transaction ID shared by all audit entries it produces, and those audit entries must be written atomically with the data changes.
- **TXN-11**. Transaction boundaries must be visible in audit queries: an operator can see which mutations committed together.

### Non-Functional Requirements

- **Performance**: Transaction commit latency < 20 ms p99 for 2–5 operation transactions on reference hardware.
- **Reliability**: Zero lost updates — no committed write is ever silently overwritten by a stale writer; verified by jepsen-style concurrency tests.
- **Scalability**: Deadlock-free by construction — OCC holds no locks during transaction execution; throughput degrades gracefully under contention via aborts, not stalls.

## User Stories

| ID | Title | Link |
|----|-------|------|
| US-020 | Atomic Multi-Entity Update | [US-020](../user-stories/US-020-atomic-multi-entity-update.md) |
| US-021 | Concurrent Agent Safety | [US-021](../user-stories/US-021-concurrent-agent-safety.md) |
| US-022 | Snapshot Isolation | [US-022](../user-stories/US-022-snapshot-isolation.md) |
| US-081 | Idempotent Transaction Submission | [US-081](../user-stories/US-081-idempotent-transaction-submission.md) |

## Edge Cases and Error Handling

- **Transaction timeout**: A transaction open longer than the configured timeout (default 30 seconds) is automatically aborted, preventing resource leaks from abandoned transactions.
- **Partial read in transaction**: Reading a non-existent entity within a transaction returns not-found but does not abort the transaction (allows conditional logic).
- **Empty transaction**: A transaction with no operations commits as a no-op and produces no audit entry.
- **Schema violation within transaction**: Any schema-invalid operation aborts the entire transaction with the specific validation error; no operations apply.
- **Size limit exceeded**: A transaction with more than 100 operations is rejected; the caller must split it.
- **Lost response then retry without idempotency key**: The retry is a new transaction and may legitimately conflict with the original; clients needing safe retry must use idempotency keys (TXN-09).

## Success Metrics

- Zero lost updates under jepsen-style concurrency test suites.
- Transaction commit latency < 20 ms p99 for 2–5 operation transactions.
- Snapshot-isolation guarantees (no dirty/non-repeatable/phantom reads) hold under the concurrency suite; retried submissions with idempotency keys never double-apply.

## Constraints and Assumptions

### Constraints

- **Isolation honesty — default is Snapshot Isolation; Serializable is opt-in.** The default level is Snapshot Isolation: write-set OCC does not detect write skew, so two transactions that each read disjoint records and write into each other's read set can both commit. An opt-in **Serializable** level adds (a) key-addressed read-set validation (delivered by B-104): preventing write skew over **specific entities read by id**; and (b) a collection-granular predicate/phantom guard (ADR-026): preventing phantom write skew over query results / index scans / traversals / aggregations by aborting when a scanned collection's membership changes. The predicate guard is conservative (over-aborts on any concurrent insert/delete to a scanned collection) and is **delivered on all storage backends** (backend-agnostic default + native memory/SQLite/PostgreSQL overrides; none fail closed). It is **membership-only**: **update-driven** predicate changes (an in-place update that flips a predicate) are not caught. Reads via `tx_get_entity` / `tx_query_entities` / `tx_aggregate` / `tx_traverse` / `tx_record_cypher_scan` **auto-capture** into the transaction; **update-driven predicate serializability and precise SSI** (minimal aborts) remain out of scope. No artifact or API may claim unqualified "serializable"; the honest claim is "Serializable for key-addressed read sets, plus a conservative collection-granular phantom guard on all backends." Invariants over **mutable** predicates must still be enforced at the application level.
- Single-instance transactions only in V1 (no distributed transactions).
- OCC-based: no pessimistic locks, no SELECT FOR UPDATE.
- Transaction size limit of 100 operations; configurable timeout, default 30 seconds.
- Read-uncommitted is never offered; Axon does not expose uncommitted state.

### Assumptions

- Agentic workloads have low contention (agents typically work on different records).
- Most transactions involve 1–5 records.
- Version conflicts are rare but must be handled correctly when they occur.

## Dependencies

- **Other features**: FEAT-004 (Entity Operations — single-entity OCC semantics this feature extends), FEAT-007 (Entity-Graph Data Model — the records transactions operate on), FEAT-003 (Audit Log — atomic transaction-threaded audit entries).
- **External services**: None. Normative surface: [CONTRACT-001 — HTTP API Surface](../../02-design/contracts/CONTRACT-001-http-api-surface.md) (transaction endpoint, idempotency protocol, status-code and conflict-detail semantics).
- **PRD requirements**: FR-5 (P0); FR-6 multi-entity scope (P0).

## Out of Scope

- **Precise / update-driven** predicate serializability (SSI / predicate locking) with minimal aborts. A **conservative, collection-granular phantom guard** is delivered on **all** adapters (opt-in Serializable + `record_scan_read`, ADR-026), alongside key-addressed write-skew prevention on all adapters (B-104). Auto-capture is delivered for entity reads, queries, aggregates, traversals, and Cypher read footprints (`tx_get_entity`/`tx_query_entities`/`tx_aggregate`/`tx_traverse`/`tx_record_cypher_scan`). Still future: catching **update-driven** predicate changes (a version-inclusive signature or full Cahill SSI), and wiring GraphQL named-query reads to run inside a commit-transaction (see Constraints, ADR-004, ADR-026).
- Distributed / cross-instance transactions.
- Pessimistic locking / SELECT FOR UPDATE.
- Savepoints within transactions; two-phase commit.
- Saga / compensation patterns (application-level concern).
- Mutation preview, approval routing, and intent binding (FEAT-030).

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
