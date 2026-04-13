---
dun:
  id: helix.principles
  depends_on: [helix.prd]
---
# Axon Design Principles

*Project-specific principles extending the HELIX workflow principles.*

---

## HELIX Core Principles Applied

| HELIX Principle | Axon Application |
|-----------------|-----------------|
| **Specification Completeness** | All transaction semantics, consistency guarantees, audit contracts, and API behaviors fully specified before implementation |
| **Test-First Development** | FoundationDB-style: correctness test suite written before implementation. Tests define the system; code exists to pass them |
| **Simplicity First** | Entity-graph-relational model serves the common case well. Escape hatches exist but the well-lit path is simple |
| **Observable Interfaces** | Audit log, collection metadata, and schema inspection are queryable. The system is transparent by default |
| **Continuous Validation** | Every commit runs the full simulation and correctness suite. HELIX ratchets ensure metrics only improve |
| **Feedback Integration** | DDx+HELIX metrics and research loops track capabilities, performance, and correctness over time |

---

## Project-Specific Principles

### P1: Test Suite First, Implementation Second

Following FoundationDB's approach to correctness: the test suite that specifies correct behavior is written before the implementation. Deterministic simulation testing, fault injection, and property-based tests establish correctness guarantees before code ships.

| Criterion | Test |
|-----------|------|
| Test suite exists before implementation | Feature test files committed before implementation files |
| Deterministic replay | Any test failure can be reproduced with a seed |
| Fault injection coverage | All storage, network, and concurrency failure modes exercised |
| Ratcheted quality | Test coverage and correctness properties can only increase across commits |

### P2: Audit is Not Optional

Every write is an audit event. The audit log is not a feature; it's the architecture.

| Criterion | Test |
|-----------|------|
| 100% mutation coverage | No code path exists that mutates state without producing an audit entry |
| Audit log is append-only | No API or internal path can modify or delete audit entries |
| Queryable provenance | Any state can be traced to its causal chain of mutations |

### P3: Entities and Links are the Model

The world is things and relationships. Axon models both as first-class, typed, audited objects.

| Criterion | Test |
|-----------|------|
| Entities are schema-validated | Every write is checked against the collection schema |
| Links are typed and directional | Link-types are declared in schema; untyped links are rejected |
| Both are audited | Entity and link mutations produce identical audit trail quality |

### P4: Transactions Mean Transactions

ACID semantics for multi-entity operations. If it can be partially applied, it's not a transaction.

| Criterion | Test |
|-----------|------|
| Atomicity | Multi-entity transaction: all operations commit or none do |
| Snapshot Isolation | Concurrent transactions are isolated from each other's writes; write skew prevention (full serializability) is P1. |
| No lost updates | Version-based OCC prevents stale-state overwrites |
| Linearizable single-entity reads | Read-after-write returns the written value from any client on the same instance |

### P5: Schema Earns Its Keep

Schemas must provide enough value — validation, migration, documentation, query optimization — that defining them is obviously worthwhile.

| Criterion | Test |
|-----------|------|
| Validation on every write | Invalid entities are rejected with structured, actionable errors |
| Schema inspection | Agents can retrieve and reason about schemas programmatically |
| Evolution support | Additive changes are automatic; breaking changes are detected and require migration |

### P6: Cloud-Native Means Location-Transparent

Same API whether embedded, self-hosted, or managed. Storage is an implementation detail.

| Criterion | Test |
|-----------|------|
| API parity | Embedded and server modes pass the identical test suite |
| Storage abstraction | Switching storage backends does not change API behavior |
| Zero-config embedded | Embedded mode works with no external dependencies |

### P7: Agents are First-Class Citizens

API design optimizes for programmatic consumption, not human UI patterns.

| Criterion | Test |
|-----------|------|
| Structured errors | Every error is machine-parseable with field path, expected/actual, and suggested fix |
| Self-describing schemas | API exposes collection schemas for agent introspection |
| Transactional batches | Multi-operation transactions are a first-class API concept |

### P8: Local-First is a Requirement

Offline operation and sync are core, not bolt-on.

| Criterion | Test |
|-----------|------|
| Offline writes | Embedded mode accepts writes with no network connectivity |
| Conflict resolution | Concurrent offline edits are resolved deterministically on sync |

### P9: Simplicity Over Flexibility

A well-lit path for common patterns beats maximum configurability. Convention over configuration where possible.

| Criterion | Test |
|-----------|------|
| Default works | A new collection with a schema and basic CRUD requires no configuration beyond the schema |
| Escape hatches exist | Flexible zones, custom validators, and raw queries are available but not required |

---

## Exceptions Log

| Date | Principle | Exception | Justification | Resolution Timeline |
|------|-----------|-----------|---------------|-------------------|
| | | | | |
