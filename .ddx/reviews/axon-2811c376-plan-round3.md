## Target

Final revised implementation plan for bead `axon-2811c376`: carve a new
`axon-esf` leaf crate for external consumers such as pqueue/Cayce.

This revision incorporates round-1 and round-2 adversarial findings.

## Fixed Decisions Since Round 2

### Public derives and impl placement

- Move `IndexType` and its `Display` impl into `axon-esf`; the impl cannot stay
  in `axon-schema` after the type move.
- Preserve existing derives on moved types unless an additional derive is
  required and harmless. Do not derive `Eq` for `IndexDeclaration`; use
  `PartialEq` only to avoid a compile-time contradiction with existing structs.
- Preserve `IndexType` derives: `Debug`, `Clone`, `PartialEq`, `Eq`, `Hash`,
  `Serialize`, `Deserialize`.

### Backward-compatible paths

Preserve both import paths:

- `axon_schema::{IndexType, IndexDef, CompoundIndexField, CompoundIndexDef}`
- `axon_schema::schema::{IndexType, IndexDef, CompoundIndexField, CompoundIndexDef}`

Implementation: inside `crates/axon-schema/src/schema.rs`, `pub use` the moved
types from `axon_esf`, and keep the existing crate-root re-export in
`crates/axon-schema/src/lib.rs`.

### Deterministic mixed `indexes:` deserialization

`IndexDeclaration` will use a custom `Deserialize` implementation, not plain
`#[serde(untagged)]`.

Rules:

- Object with `field` and no `fields` => `Single(IndexDef)`.
- Object with `fields` and no `field` => `Compound(CompoundIndexDef)`.
- Object with both `field` and `fields` => error: ambiguous index declaration.
- Object with neither => error: index declaration must contain `field` or
  `fields`.

This keeps current `IndexDef` and `CompoundIndexDef` serde shapes unchanged
while making the new public `indexes:` contract deterministic.

### Compile error API

`axon-esf` exposes:

```rust
pub struct SchemaCompileError {
    pub message: String,
}
```

No public API returns `jsonschema` internals.

### Raw validation error API

`axon-esf` exposes owned validation errors:

```rust
pub struct SchemaValidationError {
    pub instance_path: String,
    pub message: String,
    pub instance: Value,
}

pub struct SchemaValidationErrors(pub Vec<SchemaValidationError>);
```

`message` is exactly `jsonschema::ValidationError::to_string()` without
manually prefixing the instance path. The path is carried only in
`instance_path`. This preserves `axon-schema` string classifiers for required,
enum, and type errors.

### ESF description handling

Add `description: Option<String>` to `axon-schema::EsfDocument` and carry it
through to `CollectionSchema.description`. This aligns with CONTRACT-010 and is
additive for existing documents.

### Format assertion scope

Scope format assertion tightening to entity-schema validation only:

- `axon-esf::CompiledSchema::compile` enables Draft 2020-12 and
  `should_validate_formats(true)`.
- `axon-schema::validate_entity` and `compile_entity_schema` use
  `axon_esf::CompiledSchema`.
- `validate_link_metadata` is left behavior-compatible in this bead. It may
  still inherit `jsonschema` with default features disabled, but it does not
  turn on format assertions unless separately requested.

Existing date-time schema sites were audited:

- `crates/axon-schema/src/named_queries.rs:568` defines `updated_at` but does
  not validate sample data there.
- `crates/axon-api/src/handler.rs:16704` defines `updated_at`; nearby test data
  uses RFC3339-compatible values where present, e.g. `2026-04-03T10:00:00Z`.
- `crates/axon-schema/fixtures/beads.esf.yaml:53` declares `claimed-at` only.

Closure evidence must explicitly name format assertion as an intentional
behavior tightening required by `axon-2811c376`, not accidental parity.

### `$ref` resolution

Changing the workspace `jsonschema` dependency to `default-features = false`
removes HTTP and file `$ref` resolution for the workspace. This is intentional
for the leaf dependency goal. Add a test documenting that remote `$ref`
resolution is not part of the leaf crate contract, and rely on full existing
schema tests to catch local regressions. No current fixture or test schema uses
external/file `$ref`.

### Performance evidence

Do not assert sub-microsecond timing in debug `cargo test`.

Add `crates/axon-esf/examples/compiled_schema_perf.rs`, runnable with:

```bash
cargo run -p axon-esf --release --example compiled_schema_perf
```

The example compiles once, validates many small records, prints average
nanoseconds per validation, and exits non-zero if release-mode average is
>= 1 microsecond. This is the explicit performance demonstration for the bead.

## Implementation Steps

1. Add `crates/axon-esf` and workspace membership.
2. Change workspace `jsonschema` dependency to `default-features = false`.
3. Add `axon-esf` public modules:
   - `types.rs`
   - `validation.rs`
   - `lib.rs`
4. Move index types and `IndexType::Display` into `axon-esf`.
5. Add deterministic `IndexDeclaration`, `EntitySchemaDocument`, and
   `EsfCoreDocument`.
6. Add `CompiledSchema`, `SchemaCompileError`, and owned
   `SchemaValidationError(s)`.
7. Repoint `axon-schema`:
   - dependency on `axon-esf`;
   - `pub use` moved types from `schema.rs`;
   - crate-root re-export unchanged for callers;
   - `validate_entity` and `compile_entity_schema` use `CompiledSchema`;
   - existing enhanced error return types remain unchanged.
8. Extend `axon-schema::EsfDocument`:
   - `description`;
   - mixed `indexes`;
   - split mixed declarations into existing `indexes` and `compound_indexes`.
9. Tests:
   - `cargo test -p axon-esf`;
   - `cargo test -p axon-schema`;
   - serde compatibility for moved types;
   - deterministic mixed-index deserialization errors;
   - ESF `description` and mixed `indexes:` conversion;
   - invalid `email`, `uuid`, `date-time` fail;
   - enhanced `axon-schema` required/enum/type tests still pass.
10. Checks:
   - `cargo tree -p axon-esf`;
   - no forbidden deps in tree;
   - release perf example;
   - full workspace gates.

## Review Question

You are a critic, not a validator. Find implementation rework risks,
contradictions, missing constraints, ambiguous interfaces, hidden assumptions,
and places where two competent implementers would make different choices.
Do not implement the plan. Do not balance criticism with praise.

Focus on whether this third-round plan has converged enough to implement.

## Output Contract

Produce findings as:

### Findings

| Severity | Area | Finding |
|---|---|---|
| BLOCKING | <area> | <specific issue with cited evidence> |
| WARNING  | <area> | <specific issue with cited evidence> |
| NOTE     | <area> | <observation with cited evidence> |

### Verdict: APPROVE | REQUEST_CHANGES | BLOCK

### Summary

2-4 sentences.
