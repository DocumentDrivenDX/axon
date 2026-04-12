---
title: Audit Log
weight: 5
---

Every mutation in Axon produces an **immutable audit entry**. The audit log is a first-class component of the architecture — not an optional add-on.

## What is recorded

Every audit entry captures:

| Field | Description |
|-------|-------------|
| `id` | Sequential entry ID |
| `collection` | Collection affected |
| `entity_id` | Entity affected (empty for collection-level operations) |
| `mutation` | Operation type (see below) |
| `actor` | Identity of the caller |
| `version` | Entity version after the mutation |
| `data_before` | Entity data before the mutation (`null` for creates) |
| `data_after` | Entity data after the mutation (`null` for deletes) |
| `timestamp_ns` | Nanosecond-precision timestamp |

## Mutation types

| Mutation | Triggered by |
|----------|-------------|
| `collection.create` | `collections create` |
| `collection.drop` | `collections drop` |
| `schema.update` | `schema set` |
| `entity.create` | `entities create` |
| `entity.update` | `entities update` |
| `entity.delete` | `entities delete` |

## Querying the audit log

```bash
# All recent entries for a collection
axon audit list --collection tasks --limit 20

# Entries for a specific entity
axon audit list --entity-id task-001

# By actor
axon audit list --actor alice@example.com

# Combined
axon audit list --collection tasks --entity-id task-001 --limit 5
```

## Immutability guarantee

Audit entries are append-only — there is no API for deleting or modifying audit records. The storage layer enforces this at the schema level.

## Rollback

Because every mutation captures `data_before`, you can reconstruct any previous entity state by replaying the audit log in reverse. Programmatic rollback is available via the REST API.
