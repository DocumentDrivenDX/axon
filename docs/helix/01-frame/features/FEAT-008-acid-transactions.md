---
dun:
  id: FEAT-008
  depends_on:
    - helix.prd
    - FEAT-007
---
# Feature Specification: FEAT-008 - ACID Transactions

**Feature ID**: FEAT-008
**Status**: Draft
**Priority**: P0
**Owner**: Core Team
**Created**: 2026-04-04
**Updated**: 2026-04-04

## Overview

ACID transactions are Axon's correctness guarantee. When an agent debits account A and credits account B, both changes commit or neither does. When two agents concurrently update the same entity, exactly one succeeds and the other is informed of the conflict. Transactions span entities and links across collections within a single Axon instance, with serializable isolation as the default.

## Problem Statement

Agents operating concurrently on shared state produce corrupt or inconsistent data without transactional guarantees. A bead tracker that marks a bead "done" and updates a counter needs atomicity. A workflow that moves an invoice from "pending" to "approved" and creates an audit entry needs isolation. Current agent storage solutions either lack transactions entirely (Firebase, JSON files) or require developers to manually implement locking and rollback logic.

## Requirements

### Isolation Levels

| Level | Guarantee | Anomalies Prevented | Axon Support |
|-------|-----------|---------------------|-------------|
| **Serializable** | Transactions execute as if in serial order | All: dirty reads, non-repeatable reads, phantom reads, write skew | **Default** in V1. Implemented via OCC with conflict detection at commit |
| **Snapshot Isolation** | Transaction reads from a consistent point-in-time snapshot | Dirty reads, non-repeatable reads, phantoms. Vulnerable to write skew | **P1**. Opt-in for long-running reads |
| **Read Committed** | Each statement sees only committed data | Dirty reads | **Available** as opt-in for reporting |
| **Read Uncommitted** | Not supported | — | **Never**. Axon does not expose uncommitted state |

### Single-Entity Operations

- **Linearizable reads**: After a write is acknowledged, all subsequent reads from any client on the same instance return the updated value
- **Read-your-writes**: Within a session/connection, a client always sees its own writes immediately
- **Optimistic concurrency control**: Writes include expected `_version`. Conflict if version has changed since read
- **Conflict response**: On version conflict, return HTTP 409 / gRPC ABORTED with the current entity state so the client can merge and retry

### Multi-Entity Transactions

- **Atomic commit**: A transaction updating entities A, B and creating link L either fully commits or fully rolls back
- **Cross-collection**: Transactions can span multiple collections (debit `accounts/acct-A`, credit `accounts/acct-B`, create `ledger-entries/txn-123`)
- **Version checks per entity**: Each entity mutation within a transaction carries its own version expectation. Any single version conflict aborts the entire transaction
- **Transaction size limit**: V1 limits transactions to 100 operations (entities + links). Sufficient for all expected use cases; prevents runaway transactions

### Conflict Resolution

- **First-writer-wins**: Under serializable isolation, the first transaction to commit wins. Subsequent transactions that touch the same entities are aborted with conflict
- **Conflict detail**: The abort response includes which entity/link caused the conflict and its current state
- **Retry guidance**: API response includes a `retryable: true` flag for version conflicts (vs. `retryable: false` for schema violations)

### Audit Integration

- **Transaction ID**: Each transaction is assigned a unique ID. All audit entries within the transaction share this ID
- **Atomic audit**: The audit entries for all operations in a transaction are written atomically with the data changes
- **Transaction boundaries visible in audit**: `axon audit list` shows which mutations were part of the same transaction

## User Stories

### Story US-020: Atomic Multi-Entity Update [FEAT-008]

**As a** developer building a financial workflow
**I want** to debit one account and credit another atomically
**So that** money is never lost or duplicated due to partial failures

**Acceptance Criteria:**
- [ ] Transaction debiting account A and crediting account B either both succeed or both fail
- [ ] If account A's version has changed (someone else updated it), the entire transaction aborts
- [ ] Abort response identifies which entity caused the conflict and includes its current state
- [ ] Audit log shows both updates under the same transaction ID

### Story US-021: Concurrent Agent Safety [FEAT-008]

**As an** agent operating on shared state
**I want** to know if another agent modified the entity I'm about to update
**So that** I never silently overwrite another agent's work

**Acceptance Criteria:**
- [ ] Agent A reads entity at version 5. Agent B updates entity to version 6. Agent A's update (expecting version 5) fails with conflict
- [ ] Conflict response includes the current state (version 6) written by Agent B
- [ ] Agent A can read the new state, merge, and retry with version 6
- [ ] If no other agent has touched the entity, the update succeeds on the first try

### Story US-022: Serializable Isolation [FEAT-008]

**As a** developer
**I want** serializable isolation for my transactions
**So that** concurrent transactions produce the same result as if they ran one at a time

**Acceptance Criteria:**
- [ ] Two concurrent transactions that both read entity X and conditionally write to entity Y produce the same result as serial execution
- [ ] Write skew is prevented: if T1 reads A and writes B while T2 reads B and writes A, at most one commits
- [ ] Isolation level can be checked / set per transaction

## Edge Cases and Error Handling

- **Transaction timeout**: Transactions that remain open for > 30 seconds (configurable) are automatically aborted. Prevents resource leaks from abandoned transactions
- **Partial read in transaction**: Reading an entity that doesn't exist within a transaction returns 404 but does NOT abort the transaction (allows conditional logic)
- **Empty transaction**: A transaction with no writes commits as a no-op (no audit entry)
- **Schema violation within transaction**: If any operation violates schema, the entire transaction aborts with the specific validation error
- **Transaction exceeds size limit**: Attempting to add the 101st operation returns an error; transaction must be committed or split
- **Deadlock**: OCC is deadlock-free by design (no locks are held during transaction execution; conflicts are detected at commit)

## Success Metrics

- Zero lost updates: no write is silently overwritten by a stale client
- Transaction commit latency < 20ms p99 for 2-5 entity transactions
- Serializable isolation verified by jepsen-style concurrency tests

## Constraints and Assumptions

### Constraints
- Single-instance transactions only in V1 (no distributed transactions)
- OCC-based: no pessimistic locks, no SELECT FOR UPDATE
- Transaction size limit of 100 operations
- 30-second transaction timeout (configurable)

### Assumptions
- Agentic workloads have low contention (agents typically work on different entities)
- Most transactions involve 1-5 entities
- Version conflicts are rare but must be handled correctly when they occur

## Dependencies

- **FEAT-007** (Entity-Graph Model): Transactions operate on entities and links
- **FEAT-003** (Audit Log): Transactions produce atomic audit entries

## Out of Scope

- Distributed / cross-instance transactions (P2)
- Pessimistic locking / SELECT FOR UPDATE
- Savepoints within transactions
- Two-phase commit protocol
- Saga / compensation patterns (application-level concern)

## Traceability

### Related Artifacts
- **Parent PRD Section**: Section 3 (Transaction Model), Requirements Overview > P0 #8-9
- **User Stories**: US-020, US-021, US-022
- **Test Suites**: `tests/FEAT-008/`
- **Implementation**: `src/transactions/` or equivalent

### Feature Dependencies
- **Depends On**: FEAT-007 (Entity-Graph Model), FEAT-003 (Audit Log)
- **Depended By**: FEAT-004 (Entity Operations — uses transaction layer), FEAT-006 (Bead Adapter — atomic bead state transitions)
