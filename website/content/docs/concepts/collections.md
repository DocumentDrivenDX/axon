---
title: Collections
weight: 1
---

A **collection** is a named container for entities of the same type — analogous to a database table, but designed for JSON documents with schema enforcement.

## Creating a collection

```bash
axon collections create tasks
axon collections create projects
axon collections list
```

Collections are lightweight namespaces. They don't require a schema upfront, but you should define one before writing data in production.

## Namespaces

Collections can be organized into namespaces for multi-tenant isolation:

```
prod.billing.invoices    # namespace: prod.billing, collection: invoices
dev.billing.invoices     # same collection name, different namespace
```

Qualified collection names use dot-separated path syntax.

## Schema version

Every collection tracks a `schema_version` — an integer that increments each time the schema is updated. Entities store the schema version they were validated against, enabling schema migration tracking.

## Listing and inspecting

```bash
axon collections list
axon collections describe tasks   # (coming soon)
```

## Dropping a collection

```bash
axon collections drop tasks --confirm
```

Dropping removes all entities, links, audit entries, and schema history for the collection. This is irreversible — `--confirm` is required to prevent accidents.

## Namespaced collections

When using namespaces, qualify the collection name:

```bash
axon collections create prod.billing.invoices
axon entities create prod.billing.invoices --id inv-001 --data '{"amount": 100}'
```
