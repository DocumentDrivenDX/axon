---
title: Schema
weight: 3
---

A **schema** defines the shape and constraints for entities in a collection. Axon uses [JSON Schema](https://json-schema.org/) and enforces it on every write — there is no way to store a malformed entity.

## Setting a schema

```bash
axon schema set tasks --schema '{
  "type": "object",
  "properties": {
    "title":    {"type": "string"},
    "status":   {"type": "string", "enum": ["open", "in-progress", "done"]},
    "priority": {"type": "integer"},
    "assignee": {"type": "string"}
  },
  "required": ["title", "status"]
}'
```

## Viewing the current schema

```bash
axon schema show tasks
```

## Schema evolution

Axon classifies every schema change as **compatible** or **breaking**:

| Change | Classification |
|--------|---------------|
| Add optional field | Compatible |
| Add required field | Breaking |
| Remove field | Breaking |
| Narrow an enum | Breaking |
| Widen an enum | Compatible |
| Change field type | Breaking |

**Compatible changes** are applied automatically:

```bash
axon schema set tasks --schema '{ ...add optional "assignee" field... }'
# → {"compatibility": "compatible", ...}
```

**Breaking changes** require `--force`:

```bash
axon schema set tasks --schema '{ ...remove required "status"... }'
# → Error: schema change is breaking. Use --force to apply.

axon schema set tasks --schema '{ ...remove required "status"... }' --force
# → {"compatibility": "breaking", ...}
```

`--dry-run` previews what would change without applying it:

```bash
axon schema set tasks --schema '...' --dry-run
```

## Schema versioning

Each `schema set` increments the schema version. Entities store the version they were validated against, so you always know whether an entity conforms to the current schema or an older one.

## Link types

Schemas can declare which link types are valid for a collection:

```json
{
  "type": "object",
  "properties": { ... },
  "link_types": {
    "belongs-to": {"target_collection": "projects"},
    "depends-on":  {"target_collection": "tasks"}
  }
}
```

Declared link types appear in schema documentation and enable validation at link-creation time.
