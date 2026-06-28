## Target

Implementation plan for bead `axon-2811c376`: carve a new `axon-esf` leaf
crate for external consumers such as pqueue/Cayce.

### Bead Requirements

- Add a new Rust crate `crates/axon-esf`.
- Move reusable ESF core out of `axon-schema`: `IndexType`, `IndexDef`,
  `CompoundIndexField`, `CompoundIndexDef`, and a thin holder for the
  JSON-Schema-2020-12 `entity_schema` document plus Layer-4 index declarations.
- `axon-esf` must depend on `serde`, `serde_json`, and `jsonschema` only.
- `axon-esf` must not depend on `axon-core`, `axon-cypher-ast`, `reqwest`,
  `hyper`, `tower`, or HTTP clients.
- Use `jsonschema` with default features disabled to avoid remote HTTP
  resolution dependencies.
- Provide a compile-once `CompiledSchema` API for repeated validation.
- Enable format assertions for `email`, `uuid`, and `date-time`.
- Repoint `axon-schema` to consume and re-export the moved ESF types.
- Preserve existing Axon behavior and structured validation errors.

### Current Code Evidence

- `crates/axon-schema/src/schema.rs` currently defines `IndexType`, `IndexDef`,
  `CompoundIndexField`, `CompoundIndexDef`, `CollectionSchema`, `LinkTypeDef`,
  `LifecycleDef`, `GateDef`, `NamedQueryDef`, and `EsfDocument`.
- `CollectionSchema` currently depends on `axon_core::id::CollectionId`.
- `EsfDocument::parse` currently depends on `serde_yaml` and returns
  `axon_core::error::AxonError`.
- `crates/axon-schema/src/validation.rs` currently recompiles a `jsonschema`
  validator inside `validate_entity`.
- `jsonschema 0.28.3` default features include `resolve-http`, which pulls
  `reqwest`, then `hyper` and `tower-http`.
- `CONTRACT-010` defines ESF layers. Layer 1 is `entity_schema`; Layer 4 is
  `indexes`; `access_control` and `queries` are adjacent contracts, not the
  thin external consumer surface.

### Proposed Implementation Boundary

1. Create `axon-esf` with:
   - `types.rs`: `IndexType`, `IndexDef`, `CompoundIndexField`,
     `CompoundIndexDef`, `EntitySchemaDocument`, and `EsfCoreDocument`.
   - `validation.rs`: raw validation errors and `CompiledSchema`.
   - `lib.rs`: public re-exports.
2. Keep these in `axon-schema`:
   - `CollectionSchema` using `CollectionId`;
   - link types;
   - access control;
   - gates;
   - validation rules;
   - lifecycles;
   - named queries;
   - evolution;
   - enhanced Axon validation errors and `AxonError` conversion.
3. Add `axon-esf = { path = "../axon-esf" }` to `axon-schema`.
4. Re-export the moved ESF types from `axon-schema` so existing Axon code and
   downstream imports continue to compile.
5. Change `axon-schema` validation internals to use `axon-esf::CompiledSchema`
   for compilation and validation, then map raw errors into the existing
   enhanced `SchemaValidationError` shape.
6. Use `jsonschema = { version = "0.28", default-features = false }` in
   `axon-esf`; keep workspace `jsonschema` as-is if broader Axon crates still
   rely on it.

### Proposed Tests And Checks

- `cargo test -p axon-esf`
- `cargo test -p axon-schema`
- `cargo tree -p axon-esf`
- `cargo tree -p axon-esf | rg 'reqwest|hyper|tower|axon-core|axon-cypher-ast'`
  returns no matches.
- Round-trip serde tests for the moved index types and ESF core holder.
- Format assertion tests reject invalid `email`, `uuid`, and `date-time`.
- Compile-once validation test validates multiple instances through one
  `CompiledSchema`.
- Existing `axon-schema` validation tests still pass.
- Final workspace gates: `cargo check`, `cargo test`, `cargo clippy -- -D warnings`,
  and `cargo fmt --check`.

## Governing Artifacts

- `docs/helix/02-design/contracts/CONTRACT-010-esf-schema-format.md`
- `docs/helix/02-design/adr/ADR-002-schema-format.md`
- `docs/helix/02-design/adr/ADR-010-physical-storage-and-secondary-indexes.md`
- `AGENTS.md` / repository instructions: no `unwrap()` in library code; clippy
  clean with `-D warnings`; tests are truth.

## Review Question

You are a critic, not a validator. Find implementation rework risks,
contradictions, missing constraints, ambiguous interfaces, hidden assumptions,
and places where two competent implementers would make different choices.
Do not implement the plan. Do not balance criticism with praise.

Focus on whether this plan will satisfy `axon-2811c376` without breaking
existing Axon behavior or creating an unsuitable public `axon-esf` interface.

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
