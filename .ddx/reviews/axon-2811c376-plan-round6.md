## Target

Final convergence plan for implementing `axon-2811c376`.

## Final Decisions

### Workspace And Dependencies

- Add `crates/axon-esf` to `[workspace].members`.
- `axon-esf` normal dependencies are exactly:
  - `serde`
  - `serde_json`
  - `jsonschema`
- `axon-schema` keeps `jsonschema.workspace = true` because
  `validate_link_metadata` remains on the direct jsonschema path in this bead.
- Change workspace jsonschema to:

  ```toml
  jsonschema = { version = "0.28", default-features = false }
  ```

This intentionally disables jsonschema's `resolve-http` and `resolve-file`
features for the workspace, including `axon-schema` entity validation and link
metadata validation. That is accepted to satisfy the leaf crate dependency
floor and must be named in closure evidence. Internal in-document `$ref` via
`#/$defs` remains supported and tested. External HTTP/file `$ref` resolution is
not part of the `axon-esf` contract and no current repository fixture/test
schema uses it.

### Exact Draft And Format Behavior

`CompiledSchema::compile` must use exactly:

```rust
jsonschema::options()
    .with_draft(jsonschema::Draft::Draft202012)
    .should_validate_formats(true)
    .build(schema)
```

This intentionally tightens entity validation: malformed values for built-in
formats now fail when a schema declares `format`. This is required by the bead.
`validate_link_metadata` is not changed to assert formats, but it does inherit
the loss of external/file `$ref` resolution from `default-features = false`.

### Exact Public `axon-esf` API

`lib.rs`:

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

Moved derives:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IndexType { ... }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexDef { ... }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompoundIndexField { ... }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompoundIndexDef { ... }
```

`IndexType`'s `Display` impl moves into `axon-esf`.

`IndexDeclaration`:

```rust
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum IndexDeclaration {
    Single(IndexDef),
    Compound(CompoundIndexDef),
}
```

Custom deserialization rejects unknown keys, empty objects, no discriminator,
and both `field` and `fields`.

Document types preserve unknown full-ESF fields instead of silently dropping
them:

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

`EsfCoreDocument` is a serde carrier for external consumers: it strongly types
the ESF fields pqueue needs, preserves other ESF fields in `extra`, and is not
used by `axon-schema::EsfDocument`.

Validation API:

```rust
pub struct SchemaCompileError { pub message: String }
impl Display for SchemaCompileError;
impl Error for SchemaCompileError;

pub struct RawValidationError {
    pub instance_path: String,
    pub message: String,
    pub instance: Value,
}
impl Display for RawValidationError;

pub struct RawValidationErrors(pub Vec<RawValidationError>);
impl RawValidationErrors { pub fn len(&self) -> usize; pub fn is_empty(&self) -> bool; }
impl Display for RawValidationErrors;
impl Error for RawValidationErrors;

pub struct CompiledSchema { validator: jsonschema::Validator }
impl CompiledSchema {
    pub fn compile(schema: &Value) -> Result<Self, SchemaCompileError>;
    pub fn validate(&self, data: &Value) -> Result<(), RawValidationErrors>;
}
```

Display shapes:

- `SchemaCompileError`: `self.message`.
- `RawValidationError`: if `instance_path` is empty, `message`; otherwise
  `"{instance_path}: {message}"`.
- `RawValidationErrors`: semicolon-joined `RawValidationError` displays.

### `axon-schema` Integration

- Preserve both:
  - `axon_schema::schema::{IndexType, IndexDef, CompoundIndexField,
    CompoundIndexDef, IndexDeclaration}`
  - `axon_schema::{IndexType, IndexDef, CompoundIndexField, CompoundIndexDef,
    IndexDeclaration}`
- `validate_entity` and `compile_entity_schema` signatures remain unchanged.
- Compile failure prefixes remain unchanged, including validate-entity field
  path `/`.
- Refactor enhancer to consume `RawValidationError` via
  `raw.message`, `raw.instance_path`, and `raw.instance`.
- `validate_link_metadata` keeps existing direct implementation and error
  shape, with only the accepted external/file `$ref` feature loss.

### `axon-schema::EsfDocument`

Add `description`, unified `indexes`, and legacy `compound_indexes`.
`into_collection_schema`:

- transfers `description`;
- splits unified `indexes`;
- appends legacy `compound_indexes`;
- preserves duplicate entries; no deduplication is introduced.

This path is independent from API body structs like gateway
`CreateCollectionSchemaBody`/`PutSchemaBody`; no merging with body-supplied
indexes occurs in this bead.

## Tests And Checks

Add tests for:

- moved type serde round-trips and `IndexType` display;
- `IndexDeclaration` serialize/deserialize and rejection cases;
- `EsfCoreDocument`/`EntitySchemaDocument` preserve `extra` and legacy
  `compound_indexes`;
- `CompiledSchema` validates repeatedly after one compile;
- invalid `email`, `uuid`, and `date-time` fail;
- internal `#/$defs` `$ref` works;
- existing enhanced `axon-schema` errors still pass;
- malformed date-time through `axon-schema::validate_entity` is now rejected
  intentionally;
- ESF `description`, unified indexes, and legacy compound indexes convert into
  `CollectionSchema`.

Performance example:

- `crates/axon-esf/examples/compiled_schema_perf.rs`.
- fixed schema, warmup 10,000, timed loop 1,000,000.
- uses `std::hint::black_box` for schema, validator, and record use.
- release build exits non-zero if average >= 1,000 ns; debug build prints only.

Final commands:

```bash
cargo tree -p axon-esf
cargo tree -p axon-esf | rg 'axon-core|axon-schema|axon-cypher-ast|axon-api|axon-storage|axon-server|axon-graphql|axon-mcp|reqwest|hyper|tower|tower-http'
cargo test -p axon-esf
cargo test -p axon-schema
cargo run -p axon-esf --release --example compiled_schema_perf
cargo check
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

## Review Question

You are a critic, not a validator. Find remaining implementation rework risks
that would block execution. Do not implement the plan. Do not balance criticism
with praise.

Focus on whether this sixth-round plan has converged enough to implement.

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
