---
dun:
  id: ADR-007
  depends_on:
    - FEAT-002
    - ADR-002
---
# ADR-007: Schema Versioning and Link Type Validation

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-04-05 | Accepted | Erik LaBianca | FEAT-002, ADR-002 | High |

## Context

Schemas are currently stored as a single row per collection with no history.
The version number is caller-supplied with no monotonicity enforcement.
Link type definitions live inside the schema but are not validated against
the target collection's existence or schema.

| Aspect | Description |
|--------|-------------|
| Problem | No schema history, no auto-versioning, no link type validation on save |
| Current State | `put_schema` does INSERT OR REPLACE; version is whatever the caller says |
| Requirements | Version history for debugging/auditing; validation that link targets exist |

## Decision

### Schema Versioning

1. **Auto-increment**: Version is assigned by the system, not the caller.
   Each `put_schema` stores a new version = `current_max + 1`. First
   schema is v1.

2. **Version history**: All versions are retained in a `schema_versions`
   table. The latest version is the active one used for validation.

3. **Read paths**:
   - `get_schema(collection)` → latest version (unchanged API)
   - `get_schema_version(collection, version)` → specific historical version
   - `list_schema_versions(collection)` → list of `{ version, created_at_ns }`

4. **No rollback in V1**: Old versions are viewable but not restorable.
   Schema migration and rollback are deferred.

5. **`CollectionSchema.version` field**: Stays in the struct — all schemas
   are versioned, so the version is a first-class field. On write, the
   handler overwrites whatever the caller passed with `current_max + 1`.
   On read, it reflects the stored version.

### Storage Schema

```sql
CREATE TABLE schema_versions (
    collection   TEXT NOT NULL,
    version      INTEGER NOT NULL,
    schema_json  TEXT NOT NULL,     -- JSONB in PostgreSQL
    created_at   BIGINT NOT NULL,  -- epoch nanoseconds
    PRIMARY KEY (collection, version)
);
```

The existing `schemas` table is replaced by this table. `get_schema`
queries with `ORDER BY version DESC LIMIT 1`.

### Link Type Validation

Link types are **always unidirectional** declarations. There is no
`bidirectional` flag in the schema. If a link should be navigable in
both directions, both schemas must independently declare their
respective link types. This is a UI/editor concern, not a schema concern.

**Validation on `put_schema`**:

1. **Target collection must exist**: Every `link_type.target_collection`
   must be a registered collection. Reject with a clear error if not.

2. **Self-referential links are valid**: A collection can declare link
   types targeting itself (e.g., `depends-on` in a beads collection).

3. **Cardinality is syntactically valid**: Must be one of the four
   defined values.

4. **Metadata schema is valid JSON Schema**: If `metadata_schema` is
   provided, it must compile as a valid JSON Schema 2020-12 document.

5. **Cannot delete a pointed-to collection**: If any other collection's
   schema has a link type targeting collection X, then dropping
   collection X is blocked. The error message lists which schemas
   reference it. This prevents orphaned link type declarations.

**Not validated** (deferred):
- Whether the reverse link type exists in the target schema (this is
  a UI convenience, not a schema constraint)
- Whether existing entities conform to a new/tightened schema
- Whether existing links conform to changed cardinality constraints

### Bidirectional Links — UI Concern

The admin UI (FEAT-009) will offer a "make bidirectional" convenience
when editing link types. This checkbox:

1. Shows a picker for the reverse link type name
2. Edits the target collection's schema to add the reverse declaration
3. Saves both schemas in sequence

This is purely a UI workflow — the schema model itself has no concept
of bidirectionality.

## Consequences

**Positive**:
- Full schema history for debugging ("what did the schema look like last week?")
- Auto-versioning prevents version confusion and regression
- Link type validation catches broken references at save time
- Clean separation: schema layer is simple, UI handles ergonomics

**Negative**:
- `schema_versions` table grows unboundedly (one row per schema save).
  Not a practical concern — schemas change rarely
- Three new trait methods on `StorageAdapter` (must be implemented for
  all backends)
- Breaking change: `put_schema` return type changes from `()` to `u32`
  (the assigned version number)
