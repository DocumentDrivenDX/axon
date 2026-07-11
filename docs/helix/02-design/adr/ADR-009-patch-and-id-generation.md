---
ddx:
  id: ADR-009
  depends_on:
    - FEAT-004
    - ADR-004
  review:
    self_hash: 7c5d1ebee3b962eae5cfaec389f6c7fb5b107928607f9d8040722afd4e17dc80
    deps:
      ADR-004: a58eda0c55e1ac9c4e8cd6fc69d213455354b62286d62be2579de9add3ad01d2
      FEAT-004: 1ba0ba90778c2e6b4a38b11632d8ca73d3b328ac19ad326e151534c26ecd0b46
    reviewed_at: "2026-07-11T04:22:33Z"
---
# ADR-009: JSON Merge Patch and Optional ID Generation

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-04-05 | Accepted | Erik LaBianca | FEAT-004 (US-012), ADR-004 | High |

## Context

Axon's `update_entity` is full-replacement only — the caller must send the
entire entity data body, even to change one field. FEAT-004 specifies partial
update as US-012. Additionally, entity IDs are currently always client-supplied,
but the PRD and FEAT-004 specify server-generated UUIDv7 as the default with
optional client override.

| Aspect | Description |
|--------|-------------|
| Problem | No partial update; no server-generated IDs |
| Current State | PUT replaces entire data; ID is required on create |
| Requirements | Patch preserves unmentioned fields; IDs optionally auto-generated |
| Decision Drivers | Agents should mutate one field without round-tripping whole entities; standard semantics (RFC 7396) over bespoke patch formats; time-ordered IDs improve scan locality |

## Decision

> **Scope note**: This record bundles 2 related decisions made together;
> future changes to any one of them require a superseding ADR for that part.
>
> - **Decision A**: Partial updates via JSON Merge Patch (RFC 7396) on a new `PATCH` operation
> - **Decision B**: Server-generated UUIDv7 entity IDs when the caller omits the ID

### 1. JSON Merge Patch (RFC 7396)

Add a `PATCH` operation that performs a **JSON Merge Patch** (RFC 7396) on
the entity data. The existing `PUT` (full replacement) remains unchanged.

#### Semantics

Given a stored entity with data:
```json
{"title": "Invoice #42", "status": "draft", "amount": 100, "notes": "rush"}
```

A merge patch of:
```json
{"status": "submitted", "notes": null}
```

Produces:
```json
{"title": "Invoice #42", "status": "submitted", "amount": 100}
```

**Rules** (per RFC 7396):
- Fields present in the patch overwrite the stored value
- Fields set to `null` in the patch are **removed** from the entity
- Fields absent from the patch are **preserved** unchanged
- Arrays are replaced wholesale, not merged element-by-element
- The merge is shallow for non-object values, recursive for nested objects

#### API

**HTTP**:
```
PATCH /entities/{collection}/{id}
Content-Type: application/merge-patch+json

{
  "data": {"status": "submitted", "notes": null},
  "expected_version": 3,
  "actor": "agent-1"
}
```

**gRPC**:
```protobuf
rpc PatchEntity(PatchEntityRequest) returns (PatchEntityResponse);

message PatchEntityRequest {
  string collection = 1;
  string id = 2;
  string patch_json = 3;        // RFC 7396 merge patch document
  uint64 expected_version = 4;
  string actor = 5;
}
```

**Rust handler**:
```rust
pub fn patch_entity(
    &mut self,
    req: PatchEntityRequest,
) -> Result<PatchEntityResponse, AxonError>
```

#### Handler Logic

1. Read the current entity (need current data + version for OCC)
2. Apply the merge patch to produce the new data
3. Validate the **merged result** against the schema (not the patch alone)
4. If a lifecycle field changed, validate the transition (ADR-008)
5. Write via `compare_and_swap` with `expected_version`
6. Audit entry records `data_before`, `data_after`, and field-level diff

#### Empty Patch

If the merge patch produces no changes (patch data is identical to stored
data, or patch is `{}`), the operation is a **no-op**:
- Version is NOT incremented
- No audit entry is produced
- Response returns the current entity unchanged

This matches FEAT-004: "empty patch is a no-op (no version increment, no
audit entry)."

#### Merge Patch Implementation

RFC 7396 is small enough to implement inline (no crate needed):

```rust
fn json_merge_patch(target: &mut Value, patch: &Value) {
    if let Some(patch_obj) = patch.as_object() {
        if !target.is_object() {
            *target = Value::Object(serde_json::Map::new());
        }
        let target_obj = target.as_object_mut().unwrap();
        for (key, value) in patch_obj {
            if value.is_null() {
                target_obj.remove(key);
            } else {
                let entry = target_obj
                    .entry(key.clone())
                    .or_insert(Value::Null);
                json_merge_patch(entry, value);
            }
        }
    } else {
        *target = patch.clone();
    }
}
```

### 2. Optional ID Generation

Entity IDs become **optional on create**. When the caller omits the ID,
the server generates a UUIDv7.

#### Semantics

- `id` field in `CreateEntityRequest` changes from required to optional
- When `id` is `None` (or empty string), server generates a UUIDv7
- When `id` is provided, it's used as-is (current behavior)
- The response always includes the assigned ID
- UUIDv7 provides time-ordering: entities created later have
  lexicographically greater IDs, which is useful for range scans and
  cursor pagination

#### API

**HTTP** — ID becomes optional in the path:
```
POST /entities/{collection}
Body: { "data": {"title": "hello"}, "actor": "alice" }
Response: { "entity": { "id": "019537a1-7c4d-7000-8000-abcdef123456", ... } }
```

The existing `POST /entities/{collection}/{id}` continues to work for
client-supplied IDs.

**gRPC**:
```protobuf
message CreateEntityRequest {
  string collection = 1;
  string id = 2;          // optional — empty string triggers UUIDv7
  string data_json = 3;
  string actor = 4;
}
```

**Rust handler**:
```rust
// In create_entity:
let id = if req.id.as_str().is_empty() {
    EntityId::new(uuid7())
} else {
    req.id
};
```

#### Crate

Add `uuid` crate (v1.x) with `v7` feature to the workspace dependencies.
UUIDv7 uses the system clock + random bits, producing monotonically
increasing IDs within a single process.

## Alternatives

*Alternatives reconstructed retrospectively (2026-06-10) for record completeness.*

| Option | Pros | Cons | Evaluation |
|--------|------|------|------------|
| JSON Patch (RFC 6902) | Distinguishes remove vs set-null; array element ops; test operations | Verbose operation lists; harder for agents to author; overkill for field-level updates | Rejected for V1: can be added later if RFC 7396's null limitation bites |
| Protobuf-style field masks | Explicit about which fields change | Non-standard for JSON HTTP APIs; awkward for nested objects; second vocabulary to learn | Rejected: merge patch covers the need with a standard |
| PUT-only (status quo, no partial update) | One update path, less API surface | Agents must fetch and resend whole entities; violates FEAT-004 US-012 | Rejected: requirement explicitly demands partial update |
| Client-required IDs (status quo) / server UUIDv4 | UUIDv4 is ubiquitous; client IDs allow natural keys | Required client IDs add agent friction; UUIDv4 is random — poor range-scan locality and cursor pagination | Rejected: UUIDv7 keeps both options and adds time-ordering |
| **Merge patch (RFC 7396) + optional server UUIDv7** | Standard semantics; minimal agent friction; time-ordered IDs; client IDs still allowed | Two update paths; null-vs-remove ambiguity | **Selected: standard, small, fits agent workloads** |

## Consequences

**Positive**:
- Agents can update one field without fetching/sending the full entity
- Empty patch is a clean no-op (no version churn, no audit noise)
- Auto-generated IDs remove a friction point for agents that don't care
  about ID format
- UUIDv7 IDs are time-ordered, improving range scan locality
- Both PUT (full replace) and PATCH (merge) coexist — use whichever
  fits the use case

**Negative**:
- Two update paths (PUT and PATCH) — more API surface, more tests
- JSON Merge Patch can't distinguish "set field to null" from "remove
  field" — this is the known RFC 7396 limitation. Acceptable for V1;
  if precision is needed, JSON Patch (RFC 6902) can be added later
- Auto-generated IDs mean the client doesn't know the ID until the
  response — requires reading the response, not fire-and-forget
- `uuid` crate adds a dependency (but it's widely used and small)

**Migration**:
- Existing `CreateEntityRequest.id` changes from `EntityId` to
  `Option<EntityId>` (or empty-string semantics for proto compat)
- Existing callers that always supply IDs are unaffected
- `UpdateEntityRequest` (PUT) is unchanged
- New `PatchEntityRequest` is additive — no breaking changes

## Risks

| Risk | Prob | Impact | Mitigation |
|------|------|--------|------------|
| RFC 7396 null-vs-remove ambiguity surprises callers needing explicit nulls | Medium | Low | Documented limitation; JSON Patch (RFC 6902) can be added as a separate operation later |
| Inline merge-patch implementation diverges from RFC 7396 | Low | Medium | RFC test vectors covered in unit tests; implementation is ~20 lines |
| UUIDv7 monotonicity assumptions break across processes/clock skew | Low | Low | Ordering is a locality optimization, not a correctness guarantee — nothing depends on strict ordering |

## Validation

| Success Metric | Review Trigger |
|----------------|----------------|
| Patch preserves unmentioned fields; merged result always schema-validated; empty patch is a true no-op (no version bump, no audit entry) | Any field loss, validation bypass, or no-op that increments version |
| Agents adopt PATCH for single-field updates (reduced full-entity round trips) | If PATCH usage stays near zero, revisit whether the second update path earns its API surface |
| Server-generated IDs are valid, unique, time-ordered UUIDv7 | Repeated requests for explicit null semantics — trigger to add RFC 6902 |

## Supersession

- **Supersedes**: None
- **Superseded by**: None

## Concern Impact

- **rust-cargo**: Adds the `uuid` crate (v1.x, `v7` feature) to workspace dependencies; merge patch implemented inline with no new crate.
- **security-owasp**: Merged result (not the raw patch) is validated against the schema, preserving input validation on the partial-update path; patches produce full before/after audit diffs.

## References

- [FEAT-004: Entity Operations](../../01-frame/features/FEAT-004-entity-operations.md) (US-012)
- [ADR-004: Transaction Model](ADR-004-transaction-model.md) — OCC interaction
- [ADR-008: Schema Lifecycles](ADR-008-schema-lifecycles.md) — lifecycle checks on patched fields
- RFC 7396 (JSON Merge Patch); RFC 9562 (UUIDv7)
