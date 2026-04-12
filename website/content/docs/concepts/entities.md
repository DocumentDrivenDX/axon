---
title: Entities
weight: 2
---

An **entity** is a versioned JSON document identified by a stable ID within a collection. Every entity has:

| Field | Type | Description |
|-------|------|-------------|
| `collection` | string | The collection this entity belongs to |
| `id` | string | Stable identifier — human-readable or UUIDv7 |
| `data` | object | The JSON payload (validated against the collection schema) |
| `version` | integer | Monotonically increasing — increments on every update |
| `schema_version` | integer | Schema version at time of last write |

## Creating an entity

```bash
axon entities create tasks \
  --id task-001 \
  --data '{"title":"Ship it","status":"open","priority":1}'
```

IDs are optional — Axon generates a UUIDv7 if omitted:

```bash
axon entities create tasks \
  --data '{"title":"Auto-ID entity","status":"open"}'
```

## Reading an entity

```bash
axon entities get tasks task-001
```

## Updating an entity

Updates use **optimistic concurrency** — you supply the version you expect to update:

```bash
axon entities update tasks task-001 \
  --expected-version 1 \
  --data '{"title":"Ship it","status":"done","priority":1}'
```

If `--expected-version` is omitted, the CLI fetches the current version automatically. In concurrent environments, always supply the expected version to prevent silent overwrites.

## Deleting an entity

```bash
axon entities delete tasks task-001
```

Deletion is hard by default (entity removed from storage). Soft delete can be modeled by adding a `deleted_at` field to the schema.

## Querying entities

```bash
# List all entities in a collection (up to default limit)
axon entities list tasks

# Filter by field value
axon entities query tasks --filter status=open
axon entities query tasks --filter priority=1

# Limit results
axon entities list tasks --limit 10
```

## Versioning and concurrency

Every write increments `version` atomically. If two clients attempt to update the same entity simultaneously with the same expected version, one will win and the other will receive a conflict error — no silent overwrites, no lost data.
