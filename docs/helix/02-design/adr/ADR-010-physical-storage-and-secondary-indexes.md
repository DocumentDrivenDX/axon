---
ddx:
  id: ADR-010
  depends_on:
    - ADR-003
    - ADR-007
    - ADR-009
    - FEAT-002
    - FEAT-004
---
# ADR-010: Physical Storage Schema and Secondary Indexes

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-04-05 | Accepted | Erik LaBianca | ADR-003, ADR-007, ADR-009, FEAT-002, FEAT-004 | High |

## Context

ADR-003 established the logical data layout shared by all backends.
The current implementation stores links as entities in pseudo-collections
(`__axon_links__`, `__axon_links_rev__`), uses text primary keys for
collections, and stores entity IDs as strings. Query execution does full
range scans with application-layer filtering — there are no secondary indexes.

| Aspect | Description |
|--------|-------------|
| Problem | No enforced referential integrity for links, string PKs prevent collection renames, no secondary indexes for query acceleration, UUID IDs stored as text waste space and index locality |
| Current State | Links stored as entities in pseudo-collections; all IDs are text; queries do full scans |
| Requirements | Enforced link integrity, collection renames, typed secondary indexes (single, compound, unique), design portable across SQL and KV backends |

## Design Principle

This ADR defines a **backend-agnostic logical storage model**. The design
must be implementable on:

- **SQL databases** (PostgreSQL, SQLite) — as relational tables with
  foreign keys and B-tree indexes
- **KV stores** (FoundationDB, Fjall) — as key-space partitions with
  encoded composite keys

Where this document shows SQL DDL, it is the **PostgreSQL materialization**
of the logical model. SQLite and KV backends implement the same logical
model with their native primitives. The section "Backend Materializations"
at the end describes how each backend maps the logical model.

## Decision

### 1. Numeric Collection IDs

Collections get a surrogate integer primary key. All other tables reference
collections by this integer, not by name.

```sql
CREATE TABLE collections (
    id    SERIAL  PRIMARY KEY,
    name  TEXT    NOT NULL UNIQUE
);
```

**Collection renames** become `UPDATE collections SET name = 'new_name' WHERE id = 3`
— no cascading updates, no rewriting of entity rows, link rows, or index rows.

The `StorageAdapter` trait continues to use `CollectionId(String)`. Each
backend adapter maintains an internal name-to-id cache, refreshed on miss.

On KV stores, the collection ID is encoded as the first bytes of every
key prefix, giving each collection its own key-space partition.

### 2. UUID Entity IDs

Entity IDs are stored as 16-byte UUIDs instead of variable-length text.
This improves index density, comparison speed, and storage efficiency.

**Logical model:**
```
entities:
    PK: (collection_id: int, id: uuid)
    version: int
    data: bytes          -- opaque to storage layer, JSON at app layer
    created_at: timestamp
    updated_at: timestamp
    created_by: text
    updated_by: text
```

**PostgreSQL materialization:**
```sql
CREATE TABLE entities (
    collection_id  INT     NOT NULL REFERENCES collections(id),
    id             UUID    NOT NULL,
    version        INT     NOT NULL DEFAULT 1,
    data           JSONB   NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by     TEXT,
    updated_by     TEXT,
    PRIMARY KEY (collection_id, id)
);
```

Entity data is **opaque to the storage layer** — all structured access
goes through declared secondary indexes. The data column type varies by
backend (JSONB in Postgres for developer ergonomics, TEXT in SQLite,
raw bytes in KV stores) but is not load-bearing for query execution.

Per ADR-009, when a client omits the entity ID, the application layer
generates a UUIDv7 and passes it to the adapter.

For entities with non-UUID string IDs (legacy or client-supplied), the
adapter uses UUID v5 (SHA-1 namespace hash) to produce a deterministic
UUID from the string, storing the original string ID in the data under
a `_original_id` key. The `StorageAdapter` trait boundary remains
string-based; each adapter handles the mapping.

### 3. Dedicated Links Table with Referential Integrity

Links move from pseudo-collections to a dedicated logical table with
enforced referential integrity.

**Logical model:**
```
links:
    PK: (source_collection_id, source_id, link_type,
         target_collection_id, target_id)
    metadata: bytes      -- optional, opaque
    created_at: timestamp
    created_by: text

    FK: (source_collection_id, source_id) → entities  ON DELETE RESTRICT
    FK: (target_collection_id, target_id) → entities  ON DELETE RESTRICT

    INDEX: (target_collection_id, target_id, link_type)  -- reverse lookups
```

**PostgreSQL materialization:**
```sql
CREATE TABLE links (
    source_collection_id  INT   NOT NULL,
    source_id             UUID  NOT NULL,
    target_collection_id  INT   NOT NULL,
    target_id             UUID  NOT NULL,
    link_type             TEXT  NOT NULL,
    metadata              JSONB,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by            TEXT,
    PRIMARY KEY (source_collection_id, source_id, link_type,
                 target_collection_id, target_id),
    FOREIGN KEY (source_collection_id, source_id)
        REFERENCES entities(collection_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (target_collection_id, target_id)
        REFERENCES entities(collection_id, id) ON DELETE RESTRICT
);

CREATE INDEX idx_links_target
    ON links (target_collection_id, target_id, link_type);
```

**Referential integrity** means the storage layer blocks deleting an
entity that has inbound or outbound links. The application-layer
force-delete flag (from `DeleteEntityRequest.force`) must delete links
first, then the entity. On SQL backends, foreign keys enforce this at
the database level. On KV backends, the adapter checks link existence
before entity deletion (application-enforced).

The `__axon_links__` and `__axon_links_rev__` pseudo-collections are
eliminated across all backends. Each adapter implements `create_link` /
`delete_link` against the links table (SQL) or links key-space (KV).
The reverse-index pseudo-collection is replaced by the target index.

### 4. Schema Version History

Per ADR-007, schemas get version history:

**Logical model:**
```
schema_versions:
    PK: (collection_id: int, version: int)
    schema_json: bytes
    created_at: timestamp

    FK: collection_id → collections
```

**PostgreSQL materialization:**
```sql
CREATE TABLE schema_versions (
    collection_id  INT     NOT NULL REFERENCES collections(id),
    version        INT     NOT NULL,
    schema_json    JSONB   NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (collection_id, version)
);
```

`get_schema(collection)` retrieves the latest version (highest version
number for the collection).

### 5. Secondary Indexes — EAV Pattern

No GIN index on the `data` column. All query acceleration goes through
explicitly declared secondary indexes. This keeps the storage layer at a
level that maps directly to a KV store (FoundationDB, Fjall) — the query
planner never depends on backend-specific JSON operators.

#### Index Declarations (ESF Layer 4)

Index declarations are added to the Entity Schema Format alongside entity
schema (L1), link types (L2), and lifecycles (L3):

```yaml
esf_version: "1.0"
collection: beads

entity_schema:
  # ... Layer 1 (JSON Schema) ...

link_types:
  # ... Layer 2 ...

lifecycles:
  # ... Layer 3 ...

indexes:                                    # Layer 4 — NEW
  # Single-field indexes
  - field: status
    type: string

  - field: priority
    type: integer

  - field: claimed-at
    type: datetime

  # Unique single-field index
  - field: spec-id
    type: string
    unique: true

  # Compound index
  - fields:
      - { field: status, type: string }
      - { field: priority, type: integer }

  # Compound unique index
  - fields:
      - { field: owner, type: string }
      - { field: title, type: string }
    unique: true
```

**Syntax rules:**
- `field` (singular) for single-field indexes
- `fields` (plural, array) for compound indexes
- `type` is the **index storage type**: `string`, `integer`, `float`,
  `datetime`, `boolean`
- `unique` defaults to `false`
- A compound index with N fields produces a single sort key preserving
  the declared field order

#### Single-Field Index Tables

One table per value type, shared across all collections and fields.

**Logical model** (all types follow the same pattern):
```
index_{type}:
    PK: (collection_id, field_path, value, entity_id)
    FK: (collection_id, entity_id) → entities  ON DELETE CASCADE
```

**PostgreSQL materialization:**
```sql
CREATE TABLE index_string (
    collection_id  INT   NOT NULL,
    field_path     TEXT  NOT NULL,
    value          TEXT  NOT NULL,
    entity_id      UUID  NOT NULL,
    PRIMARY KEY (collection_id, field_path, value, entity_id),
    FOREIGN KEY (collection_id, entity_id)
        REFERENCES entities(collection_id, id) ON DELETE CASCADE
);

CREATE TABLE index_integer (
    collection_id  INT     NOT NULL,
    field_path     TEXT    NOT NULL,
    value          BIGINT  NOT NULL,
    entity_id      UUID    NOT NULL,
    PRIMARY KEY (collection_id, field_path, value, entity_id),
    FOREIGN KEY (collection_id, entity_id)
        REFERENCES entities(collection_id, id) ON DELETE CASCADE
);

CREATE TABLE index_float (
    collection_id  INT              NOT NULL,
    field_path     TEXT             NOT NULL,
    value          DOUBLE PRECISION NOT NULL,
    entity_id      UUID             NOT NULL,
    PRIMARY KEY (collection_id, field_path, value, entity_id),
    FOREIGN KEY (collection_id, entity_id)
        REFERENCES entities(collection_id, id) ON DELETE CASCADE
);

CREATE TABLE index_datetime (
    collection_id  INT          NOT NULL,
    field_path     TEXT         NOT NULL,
    value          TIMESTAMPTZ  NOT NULL,
    entity_id      UUID         NOT NULL,
    PRIMARY KEY (collection_id, field_path, value, entity_id),
    FOREIGN KEY (collection_id, entity_id)
        REFERENCES entities(collection_id, id) ON DELETE CASCADE
);

CREATE TABLE index_boolean (
    collection_id  INT      NOT NULL,
    field_path     TEXT     NOT NULL,
    value          BOOLEAN  NOT NULL,
    entity_id      UUID     NOT NULL,
    PRIMARY KEY (collection_id, field_path, value, entity_id),
    FOREIGN KEY (collection_id, entity_id)
        REFERENCES entities(collection_id, id) ON DELETE CASCADE
);
```

**PK ordering** `(collection_id, field_path, value, entity_id)` gives
B-tree locality for:
- **Equality**: `WHERE collection_id = 3 AND field_path = 'status' AND value = 'draft'`
- **Range**: `... AND value BETWEEN 'a' AND 'm'`
- **Sort**: `ORDER BY value` is a direct index scan
- **Collection scoping**: all entries for one collection are physically
  adjacent

**`ON DELETE CASCADE`** from entities — when an entity is deleted, its
index entries are automatically cleaned up by the database. This contrasts
with links (`ON DELETE RESTRICT`) which block deletion.

**Unique indexes**: For single-field unique constraints, the adapter adds
a partial unique index:
```sql
CREATE UNIQUE INDEX uidx_{collection_id}_{field}
    ON index_string (collection_id, field_path, value)
    WHERE field_path = '{field}' AND collection_id = {id};
```

This enforces that no two entities in the same collection share the same
value for the indexed field, without constraining unrelated fields that
share the same index table.

#### Compound Index Table

Compound indexes use a single generalized table with a binary-encoded
sort key that preserves the declared field order:

```sql
CREATE TABLE index_compound (
    collection_id  INT    NOT NULL,
    index_name     TEXT   NOT NULL,
    sort_key       BYTEA  NOT NULL,
    entity_id      UUID   NOT NULL,
    PRIMARY KEY (collection_id, index_name, sort_key, entity_id),
    FOREIGN KEY (collection_id, entity_id)
        REFERENCES entities(collection_id, id) ON DELETE CASCADE
);
```

**Sort key encoding** — the `sort_key` is a binary-encoded composite of
the field values that preserves sort order under `BYTEA` comparison:

| Type | Encoding | Sort behavior |
|------|----------|---------------|
| `integer` | 8-byte big-endian with sign bit flipped | Numeric order |
| `float` | IEEE 754 with sign bit manipulation | Numeric order |
| `string` | UTF-8 bytes + `\x00\x01` separator | Lexicographic |
| `datetime` | 8-byte big-endian epoch nanos | Chronological |
| `boolean` | `\x00` (false) / `\x01` (true) | false < true |

Each field's encoded bytes are concatenated in declaration order with a
type-tagged separator to produce a single `BYTEA` value. B-tree index
scans on `sort_key` produce results in the correct multi-field sort order.

This encoding is the same one that would be used for KV store key
construction (FoundationDB tuple encoding, etc.), making the design
portable across backends.

**Compound unique indexes**: Same approach as single-field — a partial
unique index on `(collection_id, index_name, sort_key)`:
```sql
CREATE UNIQUE INDEX uidx_{collection_id}_{index_name}
    ON index_compound (collection_id, index_name, sort_key)
    WHERE index_name = '{name}' AND collection_id = {id};
```

#### Index Data Model

```rust
/// An index declaration from the schema (Layer 4 of ESF).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexDef {
    /// For single-field: the field path (e.g., "status", "address.city").
    /// None for compound indexes.
    pub field: Option<String>,

    /// For compound indexes: ordered list of field+type pairs.
    /// None for single-field indexes.
    pub fields: Option<Vec<IndexFieldDef>>,

    /// Index storage type (single-field only).
    /// Inferred from fields list for compound indexes.
    #[serde(rename = "type")]
    pub index_type: Option<IndexType>,

    /// Whether this index enforces uniqueness. Default: false.
    #[serde(default)]
    pub unique: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexFieldDef {
    pub field: String,
    #[serde(rename = "type")]
    pub index_type: IndexType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IndexType {
    String,
    Integer,
    Float,
    Datetime,
    Boolean,
}
```

`CollectionSchema` gains:
```rust
pub struct CollectionSchema {
    pub collection: CollectionId,
    pub description: Option<String>,
    pub version: u32,
    pub entity_schema: Option<Value>,                       // Layer 1
    pub link_types: HashMap<String, LinkTypeDef>,            // Layer 2
    pub lifecycles: HashMap<String, LifecycleDef>,           // Layer 3
    pub indexes: Vec<IndexDef>,                              // Layer 4 (NEW)
}
```

### 6. Index Write Path

On every entity write (create, update, patch):

1. Look up the collection's index declarations from the active schema
2. Delete existing index rows for `(collection_id, entity_id)` across
   all index tables (single-field and compound)
3. For each declared index, extract the field value(s) from the entity
   data
4. If the value is `null` or missing, skip — null values are not indexed
   (consistent with the RFC 7396 "null means absent" model)
5. Insert into the appropriate typed table (single-field) or encode the
   sort key and insert into `index_compound` (compound)

This is a delete-then-insert pattern, not an upsert. It's simple, correct,
and the CASCADE on entity delete means cleanup is automatic.

For unique indexes, the insert will fail with a constraint violation if
the value already exists for a different entity in the same collection.
The adapter surfaces this as `AxonError::Conflict` with a message
identifying the conflicting field and value.

### 7. Index Query Path

The handler's query planner checks incoming `QueryEntitiesRequest` filters
against the active schema's index declarations:

1. **Indexed field match**: If the filter references an indexed field with
   a supported operation (equality, range, IN), query the index table
   first to retrieve a set of `entity_id` values, then fetch entities
   by PK in batch.

2. **Compound index prefix match**: If the filter matches a prefix of a
   compound index's field list (leftmost fields), use the compound index
   table with a range scan on the encoded prefix.

3. **Sort optimization**: If the query's sort field matches an index
   (single or the leading field of a compound), the index scan produces
   pre-sorted results — no application-layer sort needed.

4. **No index**: Fall back to full range scan with application-layer
   filter/sort (current behavior).

The query planner is a simple rules-based matcher, not a cost-based
optimizer. It picks the first applicable index and uses it. This is
sufficient for V1 and keeps the logic portable.

### 8. Index Lifecycle

#### Creating an Index

When a schema is saved with new index declarations:

1. For a new (empty) collection, create the index metadata immediately.
   The index is `ready` — there are no entities to index.

2. For an existing collection with entities, create the index in
   `building` state. The query planner does not use `building` indexes.

3. A background worker scans all entities in the collection in batches,
   populating index rows. Entity writes that land during the build also
   populate the new index (double-write is safe — the index entry is
   keyed by entity_id).

4. When the scan completes, the worker transitions the index to `ready`.
   The query planner begins using it.

#### Dropping an Index

When a schema is saved with a previously declared index removed:

1. Mark the index as `dropping`. The query planner stops using it
   immediately.
2. Background worker deletes index rows in batches.
3. When deletion completes, remove the index metadata.

For unique indexes, dropping removes the unique constraint immediately
at step 1 (the partial unique index is dropped synchronously).

#### Index State Machine

```
                ┌──────────┐
  schema save   │          │  scan complete
  (new index) ──► building ├──────────────► ready
                │          │                  │
                └──────────┘                  │
                                              │ schema save
                                              │ (index removed)
                                              ▼
                                          dropping ──► (deleted)
```

#### Index Rebuild

An explicit rebuild operation (admin API or CLI command) is supported for:

- Reindexing after a bug fix in the sort key encoding
- Rebuilding after a schema change that alters the index type
- Consistency repair if index rows diverge from entity data

Rebuild transitions a `ready` index back to `building`, truncates its
index rows, and rescans. The index is unavailable to the query planner
during the rebuild.

### 9. Audit Log

**Logical model:**
```
audit_log:
    PK: id (auto-increment)
    timestamp_ns: int
    collection_id: int → collections
    entity_id: uuid
    version: int
    mutation: text
    actor: text
    transaction_id: text
    entry_json: bytes

    INDEX: (collection_id, entity_id, id)
```

**PostgreSQL materialization:**
```sql
CREATE TABLE audit_log (
    id              BIGSERIAL   PRIMARY KEY,
    timestamp_ns    BIGINT      NOT NULL,
    collection_id   INT         NOT NULL REFERENCES collections(id),
    entity_id       UUID        NOT NULL,
    version         INT         NOT NULL,
    mutation        TEXT        NOT NULL,
    actor           TEXT,
    transaction_id  TEXT,
    entry_json      JSONB       NOT NULL
);

CREATE INDEX idx_audit_entity
    ON audit_log (collection_id, entity_id, id);
```

### 10. Trait Boundary

The `StorageAdapter` trait is unchanged. All mapping happens inside each
backend adapter:

| Trait type | Physical type | Adapter responsibility |
|---|---|---|
| `CollectionId(String)` | `INT` (surrogate) | Name-to-id cache, lookup on miss |
| `EntityId(String)` | `UUID` (16 bytes) | Parse as UUID; UUID v5 fallback for non-UUID strings |
| `create_link` / `delete_link` | Dedicated links table/key-space | Direct ops, no pseudo-collections |
| `put` / `delete` (entity) | Entity write + index maintenance | Delete old index rows, insert new ones |
| `query_entities` with filter | Index lookup if available, else scan | Check schema for applicable indexes |

## Backend Materializations

### PostgreSQL

The SQL DDL shown throughout this document is the PostgreSQL materialization.
Key specifics:
- `SERIAL` / `BIGSERIAL` for auto-increment IDs
- Native `UUID` type for entity IDs
- `JSONB` for data and entry_json (developer ergonomics, not query-critical)
- `TIMESTAMPTZ` for timestamps
- Foreign keys with `ON DELETE RESTRICT` (links) and `ON DELETE CASCADE`
  (index rows) enforced by the database
- Partial unique indexes for unique index declarations

### SQLite

SQLite implements the same logical model with its type affinity system:

| Logical type | SQLite type |
|---|---|
| `int` (collection_id) | `INTEGER` |
| `uuid` (entity_id) | `BLOB(16)` — stored as raw 16-byte UUID |
| `bytes` (data) | `TEXT` (JSON) or `BLOB` |
| `timestamp` | `INTEGER` (epoch nanos) |

- `AUTOINCREMENT` for audit_log IDs, plain `INTEGER PRIMARY KEY` for
  collection IDs
- Foreign keys enforced via `PRAGMA foreign_keys = ON`
- Same EAV index tables with the same key ordering
- Same compound index table with `BLOB` sort keys
- Unique constraints via `CREATE UNIQUE INDEX` (no partial index syntax
  in SQLite — use a separate table or application-level enforcement)

### KV Stores (FoundationDB, Fjall)

The logical model maps to key-space partitions:

```
Key layout:
  collections/        {collection_id}  → {name, metadata}
  collections/byname/ {name}           → {collection_id}
  entities/           {collection_id}/{entity_id}  → {version, data, timestamps}
  links/fwd/          {src_col}/{src_id}/{link_type}/{tgt_col}/{tgt_id}  → {metadata}
  links/rev/          {tgt_col}/{tgt_id}/{link_type}/{src_col}/{src_id}  → {}
  schemas/            {collection_id}/{version}  → {schema_json}
  audit/              {collection_id}/{entity_id}/{sequence}  → {entry}
  idx/s/              {collection_id}/{field_path}/{value}/{entity_id}  → {}
  idx/i/              {collection_id}/{field_path}/{value_be8}/{entity_id}  → {}
  idx/f/              {collection_id}/{field_path}/{value_be8}/{entity_id}  → {}
  idx/d/              {collection_id}/{field_path}/{nanos_be8}/{entity_id}  → {}
  idx/b/              {collection_id}/{field_path}/{0|1}/{entity_id}  → {}
  idx/c/              {collection_id}/{index_name}/{sort_key}/{entity_id}  → {}
```

- Referential integrity is application-enforced (check before write)
- Unique index enforcement is a point-read before write (if key exists
  and entity_id doesn't match, reject)
- The binary sort key encoding from Section 5 is the same encoding used
  for KV key construction — no translation needed
- Range scans on index prefixes use the KV store's native range read
- Collection rename: update `collections/byname/` mapping, no key rewrite

### Memory (Test/Dev)

The in-memory adapter adopts the same logical model using `HashMap` and
`BTreeMap` structures. Index tables become `BTreeMap<(collection_id,
field_path, encoded_value), BTreeSet<entity_id>>`. This is primarily
for test fidelity — ensuring the query planner and index write path are
exercised in unit tests without requiring a database.

## Consequences

**Positive**:
- Enforced referential integrity for links across all backends
  (DB-enforced on SQL, application-enforced on KV)
- Collection renames are O(1) — update one row / one key
- UUID storage saves ~55% space per entity ID and improves comparison
  speed
- Secondary indexes enable sub-millisecond queries on declared fields
  without backend-specific operators
- Compound indexes support multi-field sorts and range scans
- Unique indexes enforce business rules at the storage level
- No dependency on GIN, JSONB operators, or any backend-specific query
  features — design is portable from Postgres to FoundationDB
- Binary sort key encoding is shared between SQL compound indexes and
  KV key construction — one implementation, all backends
- Index lifecycle (building → ready → dropping) prevents queries from
  using incomplete indexes
- Index rows cascade-delete with entities; link rows restrict-delete

**Negative**:
- Index maintenance adds write amplification — every entity write touches
  N index tables (where N = number of declared indexes)
- Binary sort key encoding must be implemented and tested carefully
  (incorrect encoding = incorrect query results)
- Background index builder adds operational complexity (worker lifecycle,
  crash recovery, progress tracking)
- UUID v5 fallback for non-UUID string IDs is a compatibility hack that
  adds edge cases
- All backends must implement the full index maintenance protocol,
  increasing adapter complexity

**Migration**:
- Existing databases need a migration from text-PK schema to
  integer-PK + UUID schema
- Existing links in pseudo-collections must be migrated to the dedicated
  links table
- Existing entity data is unchanged

## 11. Validation Gate Tables

See FEAT-019 for the full gate model. The gate tables extend the
physical schema:

**Gate definitions** — registered when a schema is saved:

```sql
CREATE TABLE gate_definitions (
    collection_id  INT   NOT NULL REFERENCES collections(id),
    gate_name      TEXT  NOT NULL,
    description    TEXT,
    includes       TEXT[],
    rule_count     INT   NOT NULL DEFAULT 0,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (collection_id, gate_name)
);
```

**Entity gate status** — materialized on every entity write:

```sql
CREATE TABLE entity_gates (
    collection_id  INT       NOT NULL,
    entity_id      UUID      NOT NULL,
    gate_name      TEXT      NOT NULL,
    pass           BOOLEAN   NOT NULL,
    failure_count  INT       NOT NULL DEFAULT 0,
    evaluated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    failures_json  JSONB,
    PRIMARY KEY (collection_id, entity_id, gate_name),
    FOREIGN KEY (collection_id, entity_id)
        REFERENCES entities(collection_id, id) ON DELETE CASCADE,
    FOREIGN KEY (collection_id, gate_name)
        REFERENCES gate_definitions(collection_id, gate_name) ON DELETE CASCADE
);

CREATE INDEX idx_gates_by_status
    ON entity_gates (collection_id, gate_name, pass, entity_id);
```

On KV stores:
```
gates/def/     {collection_id}/{gate_name}                     → {description, includes, rule_count}
gates/entity/  {collection_id}/{entity_id}/{gate_name}         → {pass, failure_count, failures}
gates/bystate/ {collection_id}/{gate_name}/{0|1}/{entity_id}   → {}
```

## 12. Table Partitioning (PostgreSQL)

PostgreSQL declarative partitioning provides operational benefits for
large tables. Partitioning strategy varies by table access pattern:

### Audit Log — Time-Range Partitioning

The audit log is append-only, queried by time range, and subject to
retention policies. Time-based partitioning enables:
- **Cheap retention**: `DROP` an old partition instead of `DELETE` —
  instant, no vacuum, no dead tuples
- **Query acceleration**: Time-range queries scan only relevant partitions
- **Tiered storage**: Old partitions can be moved to cheaper tablespaces

```sql
CREATE TABLE audit_log (
    id              BIGSERIAL,
    timestamp_ns    BIGINT      NOT NULL,
    collection_id   INT         NOT NULL,
    entity_id       UUID        NOT NULL,
    version         INT         NOT NULL,
    mutation        TEXT        NOT NULL,
    actor           TEXT,
    transaction_id  TEXT,
    entry_json      JSONB       NOT NULL,
    PRIMARY KEY (id, timestamp_ns)
) PARTITION BY RANGE (timestamp_ns);

-- Monthly partitions (epoch nanos for 2026-04)
CREATE TABLE audit_log_2026_04 PARTITION OF audit_log
    FOR VALUES FROM (1743465600000000000) TO (1746057600000000000);

-- Retention: DROP TABLE audit_log_2025_01;
```

Partition creation is automated: a background job creates the next
month's partition before it's needed. Retention is configurable per
database (FEAT-014).

### Entities — List Partitioning by Collection

For deployments with many collections or very large collections,
entities can be list-partitioned by `collection_id`:

```sql
CREATE TABLE entities (
    collection_id  INT     NOT NULL,
    id             UUID    NOT NULL,
    version        INT     NOT NULL DEFAULT 1,
    data           JSONB   NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by     TEXT,
    updated_by     TEXT,
    PRIMARY KEY (collection_id, id)
) PARTITION BY LIST (collection_id);

-- One partition per collection
CREATE TABLE entities_coll_1 PARTITION OF entities FOR VALUES IN (1);
CREATE TABLE entities_coll_2 PARTITION OF entities FOR VALUES IN (2);

-- Default partition for new collections before explicit partition creation
CREATE TABLE entities_default PARTITION OF entities DEFAULT;
```

Benefits:
- **Collection isolation**: Queries scoped to one collection scan only
  that partition
- **Collection drop**: `DROP TABLE entities_coll_N` is instant (vs
  `DELETE FROM entities WHERE collection_id = N`)
- **Maintenance**: VACUUM, ANALYZE, and REINDEX operate per-partition
- **Tablespace assignment**: Hot collections on fast storage, cold on
  cheaper storage

Partition creation is automated when a collection is registered.

### EAV Index Tables, Links, Entity Gates

Same list-partitioning by `collection_id` applies to:
- `index_string`, `index_integer`, `index_float`, `index_datetime`,
  `index_boolean`, `index_compound` — each partitioned by `collection_id`
- `links` — partitioned by `source_collection_id`
- `entity_gates` — partitioned by `collection_id`

All follow the same pattern: one partition per collection, auto-created
on collection registration, instant drop on collection deletion.

### Partitioning on SQLite and KV Stores

- **SQLite**: No native partitioning. Collection-scoped queries use the
  `collection_id` prefix in indexes, which gives similar scan locality.
  Audit log retention is handled by `DELETE` + `VACUUM`
- **KV stores**: Key prefix partitioning is inherent in the key layout.
  FoundationDB directory layer provides namespace-level isolation. No
  additional partitioning needed

### Partitioning Is Optional

Partitioning is a deployment-time optimization, not a schema
requirement. The tables work identically with or without partitioning.
Small deployments (< 100K entities, < 10 collections) don't need it.
Partitioning is recommended when:
- Audit log exceeds 10M rows (retention via partition drop)
- Any single collection exceeds 1M entities
- Collection drop needs to be instant (not a long DELETE)

## Not In Scope

- **Array field indexing**: Indexing individual elements of array fields
  (e.g., each label in a `labels` array) would require one index row per
  element. Deferred — filter on array fields falls back to scan.
- **Full-text search**: Text search indexes (tsvector/GIN in Postgres,
  FTS5 in SQLite) are a separate concern. Deferred.
- **Cost-based query planning**: V1 uses simple rules-based index
  selection. A cost-based optimizer could choose between multiple
  applicable indexes, but this is unnecessary at V1 scale.
- **GIN index on JSONB**: Deliberately omitted to keep the query path
  portable across storage backends.

## References

- [ADR-003: Backing Store Architecture](ADR-003-backing-store-architecture.md)
- [ADR-007: Schema Versioning](ADR-007-schema-versioning.md)
- [ADR-009: Patch and ID Generation](ADR-009-patch-and-id-generation.md)
- [Salesforce EAV Index Pattern](https://developer.salesforce.com/wiki/multi-tenant-architecture)
- [FoundationDB Tuple Encoding](https://apple.github.io/foundationdb/data-modeling.html#tuples)
