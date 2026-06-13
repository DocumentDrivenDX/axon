# Follow-up Beads — CONTRACT-006 Unimplemented Areas

The following CONTRACT-006 requirements are **not** implemented in this bead.
Each has a precise blocker and a corresponding child bead.

## axon-34c4dd4b — CDC cursor persistence (`_cdc_cursors` table)

**Requirement**: CONTRACT-006 §Cursor semantics — `_cdc_cursors` table persists
the last emitted `audit_id` per sink; producers resume from the stored cursor on
restart; consumers deduplicate by `source.audit_id`.

**Blocker**: `axon-audit` has no persistent storage adapter. The `replay()` trait
method is a default impl on `AuditLog`; cursor state would need to be stored via
`axon-storage` or a dedicated table in the server layer. No integration point exists
yet in this bead's scope.

**Scope of remaining work**: `CdcCursorStore` trait, `MemoryCursorStore` impl,
wiring into the server-level CDC pipeline.

---

## axon-3fbdffab — Confluent-compatible schema registry endpoints

**Requirement**: CONTRACT-006 §Schema registry REST endpoints — `GET /subjects`,
`POST /subjects/{subject}/versions`, `GET /schemas/ids/{id}`, compatibility
endpoints, etc., served on a configurable port (default 8081).

**Blocker**: `axon-registry` crate does not exist. `axon-schema` holds
`schema_versions` but has no HTTP facade. The registry API is a separate server
surface.

---

## axon-88caddb4 — Tombstone emission on delete (Kafka log compaction)

**Requirement**: CONTRACT-006 normative — "Deletes MUST be followed by a tombstone
event (same key, null value) for Kafka log compaction."

**Blocker**: `CdcSink::emit` takes a single `&CdcEnvelope`; a tombstone requires
emitting a second message with the same key and a null value. This needs a sink API
change (e.g., `emit_tombstone` or returning `Vec<Message>`) and `KafkaCdcSink`
updates.
