---
title: Links & Graph
weight: 4
---

**Links** are typed, directed edges between entities. They connect entities across collections and form the graph layer of Axon.

## Creating a link

```bash
axon links set tasks task-001 projects proj-alpha --type belongs-to
axon links set tasks task-002 tasks   task-001   --type depends-on
```

A link has five fields:

| Field | Example |
|-------|---------|
| `source_collection` | `tasks` |
| `source_id` | `task-002` |
| `target_collection` | `tasks` |
| `target_id` | `task-001` |
| `link_type` | `depends-on` |

## Listing outbound links

```bash
axon links list tasks task-002
```

`links list` does a depth-1 traversal and returns the target entities.

## Graph traversal

```bash
axon graph tasks task-002 --depth 2
```

`graph` traverses from the given entity to any depth, returning all reachable entities. Useful for dependency resolution, ownership hierarchies, and impact analysis.

Filter by link type:

```bash
axon graph tasks task-002 --link-type depends-on --depth 3
```

## Use cases

| Pattern | Example link types |
|---------|--------------------|
| Ownership / hierarchy | `belongs-to`, `owned-by`, `parent-of` |
| Dependencies | `depends-on`, `blocks`, `requires` |
| References | `references`, `derived-from` |
| Workflow | `triggers`, `assigned-to`, `reviewed-by` |

Links are directional but traversal can follow outbound, inbound, or both directions. Axon stores metadata on links for weighted or annotated edges.
