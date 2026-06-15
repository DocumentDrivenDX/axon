---
ddx:
  id: ADR-007
  depends_on:
    - FEAT-002
    - ADR-002
  review:
    self_hash: 5a96b23ec82c256af094753065c60c6862a9a7c2fd8e7db3bb681d896627f727
    deps:
      ADR-002: 914b8c8b1a9829504c826ae36b8d8b48a6118b0268c6c8c562fc446ee01b9a77
      FEAT-002: 0e2c69a223cadb6a5d1421cf36a9f91ce49880b66edb0680fd0c229cf1445533
    reviewed_at: "2026-06-15T00:35:16Z"
---
# ADR-007: Schema Versioning and Link Type Validation

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-04-05 | Accepted | Erik LaBianca | FEAT-002, FEAT-017, ADR-002 | High |

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
| Decision Drivers | Schema-as-data principle (audit/history applies to schemas too); caller-supplied versions invite regressions; broken link targets must fail at save time, not query time |

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

The admin UI (FEAT-011) will offer a "make bidirectional" convenience
when editing link types. This checkbox:

1. Shows a picker for the reverse link type name
2. Edits the target collection's schema to add the reverse declaration
3. Saves both schemas in sequence

This is purely a UI workflow — the schema model itself has no concept
of bidirectionality.

## Alternatives

*Alternatives reconstructed retrospectively (2026-06-10) for record completeness.*

| Option | Pros | Cons | Evaluation |
|--------|------|------|------------|
| Keep caller-supplied versions (status quo) | No storage change; caller controls semantics | No monotonicity guarantee; version confusion and silent regressions; no history | Rejected: defeats the purpose of versioning |
| Single-row schema + history derived from audit log | No new table; audit already records schema writes | Historical reads require audit replay/reconstruction; couples schema reads to audit retention | Rejected: point-in-time schema reads must be a direct lookup |
| Bidirectional link declarations in the schema | One declaration covers both directions | Two schemas silently coupled; ambiguous ownership of cardinality/metadata | Rejected: bidirectionality is a UI convenience, not a schema concept |
| **Auto-incremented `schema_versions` table + save-time link validation** | Full history, system-assigned monotonic versions, broken targets caught at save | Unbounded (slow) table growth; three new trait methods; breaking `put_schema` return type | **Selected: history and integrity with minimal model complexity** |

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

## Risks

| Risk | Prob | Impact | Mitigation |
|------|------|--------|------------|
| `schema_versions` growth becomes a problem for high-churn schemas | Low | Low | Schemas change rarely by nature; a retention/compaction policy can be added without changing the model |
| Drop-collection blocking (pointed-to check) frustrates legitimate teardown | Medium | Low | Error message lists referencing schemas so the caller can remove link types first |
| Backends diverge on the three new trait methods | Low | Medium | L4 backend conformance suite covers the new methods identically across backends |

## Validation

| Success Metric | Review Trigger |
|----------------|----------------|
| Every `put_schema` produces exactly `current_max + 1`; no version regressions in audit history | Any observed non-monotonic version |
| Save-time rejection of link types targeting nonexistent collections, with clear errors | Any orphaned link-type declaration found in a stored schema |
| Historical schema reads used in practice for debugging | If history goes unused after several quarters and growth matters, reconsider retention; if rollback is requested repeatedly, promote schema rollback from deferred to planned |

## Supersession

- **Supersedes**: None
- **Superseded by**: None (ADR-010 §4 carries this model into the physical storage schema with `collection_id` keys)

## Concern Impact

- **rust-cargo**: Adds three `StorageAdapter` trait methods that all backend adapters must implement.
- **security-owasp**: Save-time validation of link targets and metadata schemas closes a schema-integrity gap; versioned history supports audit review of schema changes.

## References

- [ADR-002: Schema Format](ADR-002-schema-format.md)
- [ADR-010: Physical Storage and Secondary Indexes](ADR-010-physical-storage-and-secondary-indexes.md) — physical materialization of `schema_versions`
- [FEAT-002: Schema Engine](../../01-frame/features/FEAT-002-schema-engine.md)
