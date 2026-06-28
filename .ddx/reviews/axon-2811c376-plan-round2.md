## Target

Revised implementation plan for bead `axon-2811c376`: carve a new
`axon-esf` leaf crate for external consumers such as pqueue/Cayce.

This revision incorporates round-1 adversarial findings.

## Resolved Decisions

### 1. Cargo dependency isolation

Only `axon-schema` currently depends on `jsonschema`. Change the workspace
dependency to:

```toml
jsonschema = { version = "0.28", default-features = false }
```

Both `axon-esf` and `axon-schema` inherit this dependency. This prevents Cargo
feature unification from re-enabling `resolve-http` through `axon-schema`.
Acceptance is checked with:

```bash
cargo tree -p axon-esf | rg 'reqwest|hyper|tower|axon-core|axon-cypher-ast'
```

which must return no matches.

### 2. Public Layer-4 ESF shape

Move these existing current-code types byte-for-byte/serde-shape-compatible
into `axon-esf`:

- `IndexType`
- `IndexDef`
- `CompoundIndexField`
- `CompoundIndexDef`

Also add a public mixed declaration enum for the normative CONTRACT-010 /
ADR-010 `indexes:` list:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IndexDeclaration {
    Single(IndexDef),
    Compound(CompoundIndexDef),
}
```

Add an entity-schema holder:

```rust
pub struct EntitySchemaDocument {
    pub entity_schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<IndexDeclaration>,
}
```

Add a core ESF document for external consumers:

```rust
pub struct EsfCoreDocument {
    pub esf_version: String,
    pub collection: String,
    pub description: Option<String>,
    pub entity_schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<IndexDeclaration>,
}
```

`axon-esf` owns serde data types only. It does not parse YAML text because its
normal dependency floor excludes `serde_yaml`. YAML parsing remains in
`axon-schema`; tests may use `serde_yaml` as a dev-dependency only if needed.

### 3. Relationship with existing `axon-schema::EsfDocument`

Keep `EsfDocument::parse` and `into_collection_schema` in `axon-schema`
because they return `AxonError`, parse YAML, and construct
`CollectionSchema` with `CollectionId`.

Extend `axon-schema::EsfDocument` to include:

```rust
#[serde(default)]
pub indexes: Vec<axon_esf::IndexDeclaration>
```

Then split `IndexDeclaration::Single` into `CollectionSchema.indexes` and
`IndexDeclaration::Compound` into `CollectionSchema.compound_indexes` inside
`into_collection_schema`. This preserves existing Axon internals while giving
external consumers the contract shape.

### 4. Validation and raw error contract

`axon-esf` exposes owned errors, never borrowed `jsonschema::ValidationError`:

```rust
pub struct SchemaValidationError {
    pub instance_path: String,
    pub message: String,
    pub instance: Value,
}

pub struct SchemaValidationErrors(pub Vec<SchemaValidationError>);
```

`CompiledSchema::validate(&self, data)` collects `jsonschema` errors into this
owned shape. `instance` is cloned from the error so `axon-schema` can preserve
its existing enhanced messages for actual values, enum mismatches, and type
mismatches.

### 5. Format assertion behavior

`jsonschema 0.28` does not assert formats by default for Draft 2020-12:
`validates_formats_by_default` returns false unless explicitly enabled.

Use:

```rust
jsonschema::options()
    .with_draft(jsonschema::Draft::Draft202012)
    .should_validate_formats(true)
```

This enables all built-in format assertions, not only three selected formats.
The bead explicitly requires `email`, `uuid`, and `date-time` assertion; the
implementation will test those three. Any additional built-in format assertion
is accepted as a consequence of the upstream API. This is an intentional
validation tightening required by the bead, while preserving the public API and
the structured error shape.

### 6. `axon-schema` validation integration

Remove direct `jsonschema` usage from `axon-schema` validation paths where
possible:

- `validate_entity` compiles through `axon_esf::CompiledSchema`, maps raw
  errors into the existing enhanced `SchemaValidationError`, and keeps the
  existing return type.
- `compile_entity_schema` uses `CompiledSchema::compile`.
- `validate_link_metadata` also uses `CompiledSchema::compile` so metadata
  schema validation has the same Draft 2020-12 and format behavior.

The old `validate_entity(&CollectionSchema, &Value)` API remains per-call and
therefore still recompiles. Add a new `axon-esf::CompiledSchema` API for
external hot paths; do not try to cache inside `CollectionSchema`.

## Work Breakdown

1. Add `crates/axon-esf` and workspace membership.
2. Move/re-export index types and add `IndexDeclaration`,
   `EntitySchemaDocument`, and `EsfCoreDocument`.
3. Add `CompiledSchema` and owned raw validation errors.
4. Repoint `axon-schema` imports/re-exports to `axon-esf`.
5. Extend `axon-schema::EsfDocument` to split mixed `indexes:` declarations
   into existing single and compound internal vectors.
6. Add tests:
   - serde compatibility for `IndexType`, `IndexDef`, `CompoundIndexField`,
     `CompoundIndexDef`;
   - mixed `indexes:` single+compound document deserialization;
   - existing ESF fixture still converts to `CollectionSchema`;
   - `CompiledSchema` validates multiple instances after one compile;
   - invalid `email`, `uuid`, and `date-time` fail;
   - `axon-schema` enhanced enum/type/required error tests still pass.
7. Add a micro-benchmark or a deterministic performance test demonstrating
   compile-once validation is sub-microsecond on a small schema after compile.
   Prefer a criterion-free unit test using `std::time::Instant` with a generous
   threshold and enough iterations to avoid noise; if too flaky, add a bench
   target and document the command.
8. Run focused checks, then full gates.

## Review Question

You are a critic, not a validator. Find implementation rework risks,
contradictions, missing constraints, ambiguous interfaces, hidden assumptions,
and places where two competent implementers would make different choices.
Do not implement the plan. Do not balance criticism with praise.

Focus on whether this revised plan now satisfies `axon-2811c376` without
breaking existing Axon behavior beyond the explicitly required format
assertion tightening.

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
