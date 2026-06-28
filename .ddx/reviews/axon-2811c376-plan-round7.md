## Target

Converged implementation plan for `axon-2811c376`.

## Decisions

### Workspace And Dependencies

- Add `"crates/axon-esf"` to root `[workspace].members`.
- Change root `[workspace.dependencies]` to:

  ```toml
  jsonschema = { version = "0.28", default-features = false }
  ```

- Create `crates/axon-esf/Cargo.toml` with normal dependencies exactly:

  ```toml
  serde.workspace = true
  serde_json.workspace = true
  jsonschema.workspace = true
  ```

- Add the required integration edge in `crates/axon-schema/Cargo.toml`:

  ```toml
  axon-esf = { path = "../axon-esf" }
  jsonschema.workspace = true
  ```

- `axon-esf` must not depend on `axon-core`, `axon-schema`,
  `axon-cypher-ast`, any Axon storage/API/server crate, `reqwest`, `hyper`,
  `tower`, or `tower-http`.
- Disabling `jsonschema` default features intentionally disables
  `resolve-http` and `resolve-file` for the workspace, including
  `axon-schema` entity and link-metadata validation. This is accepted to meet
  the leaf-crate dependency floor. Internal in-document references such as
  `#/$defs/...` remain supported and must be tested.

### Draft, Format, And Error Semantics

`axon-esf::CompiledSchema::compile` must compile with exactly this semantic
contract:

```rust
jsonschema::options()
    .with_draft(jsonschema::Draft::Draft202012)
    .should_validate_formats(true)
    .build(schema)
```

Consequences:

- Entity validation now rejects malformed declared formats, including
  `email`, `uuid`, and `date-time`. This is an intentional behavior tightening
  required by the bead, not an incidental refactor.
- The built-in `jsonschema` format assertion switch is global; the
  implementation must not attempt to enable only those three formats.
- `CompiledSchema::validate` must call `validator.iter_errors(data)` and
  collect all errors. It returns `Ok(())` only when the iterator is empty.
- `SchemaCompileError.message` is exactly the underlying
  `jsonschema` build error's `.to_string()`, with no prefix.
- `RawValidationError.message` is exactly
  `jsonschema::ValidationError::to_string()`, with no path prefix and no
  classification rewrite. `axon-schema` relies on these strings for its
  existing enhanced error classifier.

### Public `axon-esf` API

`crates/axon-esf/src/lib.rs`:

```rust
pub mod types;
pub mod validation;

pub use types::{
    CompoundIndexDef, CompoundIndexField, EntitySchemaDocument, EsfCoreDocument,
    IndexDeclaration, IndexDef, IndexType,
};
pub use validation::{
    CompiledSchema, RawValidationError, RawValidationErrors, SchemaCompileError,
};
```

Moved index types preserve their existing serde shape and public derives:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IndexType {
    Hash,
    BTree,
    FullText,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexDef {
    pub field: String,
    #[serde(rename = "type")]
    pub index_type: IndexType,
    #[serde(default)]
    pub unique: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompoundIndexField {
    pub field: String,
    #[serde(rename = "type")]
    pub index_type: IndexType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompoundIndexDef {
    pub fields: Vec<CompoundIndexField>,
    #[serde(default)]
    pub unique: bool,
}
```

`IndexType` keeps its lowercase `Display` implementation in `axon-esf`.

The external ESF `indexes:` carrier is:

```rust
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum IndexDeclaration {
    Single(IndexDef),
    Compound(CompoundIndexDef),
}
```

`IndexDeclaration` has custom `Deserialize` that accepts exactly one
discriminator:

- `field` => `Single(IndexDef)`.
- `fields` => `Compound(CompoundIndexDef)`.
- Reject unknown keys, empty objects, missing discriminator, and objects that
  contain both `field` and `fields`.

Document carriers are serde-only data structures for external consumers. They
do not parse YAML text and they do not replace `axon-schema::EsfDocument`.
They preserve full-ESF fields that are outside this bead's typed surface:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntitySchemaDocument {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<IndexDeclaration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compound_indexes: Vec<CompoundIndexDef>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EsfCoreDocument {
    pub esf_version: String,
    pub collection: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<IndexDeclaration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compound_indexes: Vec<CompoundIndexDef>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, Value>,
}
```

Validation types must derive `Debug` so they can implement
`std::error::Error`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaCompileError {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawValidationError {
    pub instance_path: String,
    pub message: String,
    pub instance: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawValidationErrors(pub Vec<RawValidationError>);
```

Required impls:

- `Display` and `Error` for `SchemaCompileError`.
- `Display` for `RawValidationError`.
- `Display` and `Error` for `RawValidationErrors`.
- `RawValidationErrors::len` and `RawValidationErrors::is_empty`.

Display shapes:

- `SchemaCompileError`: `self.message`.
- `RawValidationError`: `message` when `instance_path` is empty, otherwise
  `"{instance_path}: {message}"`.
- `RawValidationErrors`: semicolon-joined `RawValidationError` displays.

### `axon-schema` Integration

- `crates/axon-schema/src/schema.rs` must delete its local definitions of
  `IndexType`, `IndexDef`, `CompoundIndexField`, and `CompoundIndexDef`.
- It must re-export:

  ```rust
  pub use axon_esf::{
      CompoundIndexDef, CompoundIndexField, IndexDeclaration, IndexDef,
      IndexType,
  };
  ```

- `crates/axon-schema/src/lib.rs` must continue exposing those names from the
  crate root, now including `IndexDeclaration`.
- `validate_entity` and `compile_entity_schema` signatures remain unchanged.
- `validate_entity` maps `SchemaCompileError.message` back to the existing
  field-path `/` error with the existing prefix:
  `invalid schema definition: {message}`.
- `compile_entity_schema` maps `SchemaCompileError.message` back to
  `AxonError::SchemaValidation("invalid schema: {message}")`.
- The enhanced error path in `axon-schema` must consume
  `RawValidationError { instance_path, message, instance }` and preserve the
  current `field_path`, `severity`, `fix`, and `context` behavior.
- `validate_link_metadata` remains on the direct `jsonschema` path in this
  bead and keeps its current error string shape, aside from the accepted
  external/file `$ref` feature loss caused by the workspace dependency change.

### `axon-schema::EsfDocument`

Add these optional/typed fields:

- `description: Option<String>`
- `indexes: Vec<IndexDeclaration>`
- `compound_indexes: Vec<CompoundIndexDef>` for legacy split-form input

`EsfDocument::into_collection_schema` must:

- transfer `description`;
- split unified `indexes` into `CollectionSchema.indexes` and
  `CollectionSchema.compound_indexes`;
- append legacy `compound_indexes`;
- preserve order and duplicates;
- not merge with API body supplied indexes in this bead.

### Tests And Checks

Add focused tests for:

- moved type serde round trips and `IndexType` display;
- all serde attributes listed above, including `type` rename and `unique`
  defaulting;
- `IndexDeclaration` serialize/deserialize and rejection cases;
- `EsfCoreDocument` and `EntitySchemaDocument` preserving `extra` and legacy
  `compound_indexes`;
- `CompiledSchema` validating many records after one compile;
- malformed `email`, `uuid`, and `date-time` rejected by default;
- internal `#/$defs` references still work;
- `CompiledSchema::validate` returns all errors from `iter_errors`;
- existing enhanced `axon-schema` required/enum/type/did-you-mean errors still
  pass;
- malformed `date-time` through `axon-schema::validate_entity` is now rejected;
- ESF `description`, unified indexes, and legacy compound indexes convert into
  `CollectionSchema`.

Performance evidence:

- Add `crates/axon-esf/examples/compiled_schema_perf.rs`.
- Use a fixed schema, 10,000 warmup validations, and 1,000,000 timed
  validations.
- Use `std::hint::black_box`.
- In debug builds, print the average and exit success.
- In release builds, exit non-zero when average is `>= 1,000 ns`.
- Because this check is hardware-sensitive, failure blocks bead closure rather
  than being hand-waved; record the output as evidence.

Final verification commands:

```bash
cargo test -p axon-esf
cargo test -p axon-schema
cargo run -p axon-esf --release --example compiled_schema_perf
cargo tree -p axon-esf > .ddx/reviews/axon-2811c376-cargo-tree.txt
if rg 'axon-core|axon-schema|axon-cypher-ast|axon-api|axon-storage|axon-server|axon-graphql|axon-mcp|reqwest|hyper|tower|tower-http' .ddx/reviews/axon-2811c376-cargo-tree.txt; then
  exit 1
fi
cargo check
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

## Review Question

You are a critic, not a validator. Find only remaining implementation rework
risks that would block execution. Do not implement the plan. Do not balance
criticism with praise.

Focus on whether this seventh-round plan has converged enough to implement.

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
