## Target

Convergence plan for bead `axon-2811c376`: implement a new public
`axon-esf` leaf crate and repoint `axon-schema` to consume it.

## Exact Public `axon-esf` API

### Module Exports

`crates/axon-esf/src/lib.rs` exports:

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

No parse helpers are exposed. `axon-esf` owns serde data types and compiled
JSON-Schema validation only.

### Dependency Boundary

`axon-esf` normal dependencies are exactly:

```toml
serde.workspace = true
serde_json.workspace = true
jsonschema.workspace = true
```

The workspace `jsonschema` dependency is changed to:

```toml
jsonschema = { version = "0.28", default-features = false }
```

Forbidden normal dependency families for `axon-esf`:

- Axon crates: `axon-core`, `axon-schema`, `axon-cypher-ast`, `axon-api`,
  `axon-storage`, `axon-server`, `axon-graphql`, `axon-mcp`.
- HTTP/runtime stacks introduced by jsonschema defaults: `reqwest`, `hyper`,
  `tower`, `tower-http`.

Checks:

```bash
cargo tree -p axon-esf
cargo tree -p axon-esf | rg 'axon-core|axon-schema|axon-cypher-ast|axon-api|axon-storage|axon-server|axon-graphql|axon-mcp|reqwest|hyper|tower|tower-http'
```

The second command must produce no output.

### Types

Moved from `axon-schema` to `axon-esf`, preserving serde field names:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IndexType {
    String,
    Integer,
    Float,
    Datetime,
    Boolean,
}

impl std::fmt::Display for IndexType { /* existing impl moved unchanged */ }

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

New mixed Layer-4 declaration:

```rust
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum IndexDeclaration {
    Single(IndexDef),
    Compound(CompoundIndexDef),
}
```

`IndexDeclaration` has a custom `Deserialize`:

- `field` and not `fields` -> `Single`.
- `fields` and not `field` -> `Compound`.
- both -> error.
- neither -> error.

Derived `Serialize` emits the same object shape as the wrapped declaration.

Entity schema holder:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntitySchemaDocument {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<IndexDeclaration>,
}
```

Core ESF document for external consumers:

```rust
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
}
```

This is a subset/companion of `axon-schema::EsfDocument`: it carries only
external ESF core fields that pqueue needs. `axon-schema::EsfDocument` remains
the YAML parser and Axon-specific converter.

### Validation API

Avoid naming collisions with existing `axon-schema::SchemaValidationError`.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaCompileError {
    pub message: String, // raw jsonschema build error string, no prefix
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawValidationError {
    pub instance_path: String,
    pub message: String, // exactly jsonschema::ValidationError::to_string()
    pub instance: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawValidationErrors(pub Vec<RawValidationError>);

pub struct CompiledSchema {
    validator: jsonschema::Validator,
}

impl CompiledSchema {
    pub fn compile(schema: &Value) -> Result<Self, SchemaCompileError>;
    pub fn validate(&self, data: &Value) -> Result<(), RawValidationErrors>;
}
```

`compile` uses Draft 2020-12 and `should_validate_formats(true)`.
`validate` owns all returned errors by cloning the failing instance value from
the jsonschema error. No public API exposes `jsonschema::ValidationError`.

Internal `#/$defs` `$ref` support remains part of jsonschema and should be
covered by a test. External/file/HTTP `$ref` resolution is deliberately out of
scope because default features are disabled.

## `axon-schema` Integration

1. Add `axon-esf = { path = "../axon-esf" }`.
2. Preserve `axon_schema::schema::*` by adding this to
   `crates/axon-schema/src/schema.rs`:

   ```rust
   pub use axon_esf::{
       CompoundIndexDef, CompoundIndexField, IndexDeclaration, IndexDef, IndexType,
   };
   ```

3. Preserve crate-root re-exports from `crates/axon-schema/src/lib.rs`.
4. Refactor entity validation only:
   - `validate_entity` calls `CompiledSchema::compile` and `validate`.
   - `compile_entity_schema` calls `CompiledSchema::compile`.
   - `validate_link_metadata` remains behavior-compatible in this bead and
     keeps its existing string-joined error shape.
5. Refactor the enhancer boundary explicitly:
   - replace `enhance_json_schema_error(&jsonschema::ValidationError, &Value)`
     with `enhance_raw_schema_error(&axon_esf::RawValidationError, &Value)`.
   - replace classifier calls so they consume:
     - `raw.message.as_str()`;
     - `raw.instance_path.as_str()`;
     - `&raw.instance`.
   - preserve existing prefixes:
     - `validate_entity` compile failure -> `invalid schema definition: {raw}`;
     - `compile_entity_schema` failure -> `invalid schema: {raw}`.

## `axon-schema::EsfDocument`

Keep YAML parsing and Axon conversion in `axon-schema`.

Extend `EsfDocument` with:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub description: Option<String>,
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub indexes: Vec<axon_esf::IndexDeclaration>,
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub compound_indexes: Vec<axon_esf::CompoundIndexDef>,
```

Conversion:

- `description` flows into `CollectionSchema.description`.
- `indexes` is split into `CollectionSchema.indexes` and
  `CollectionSchema.compound_indexes`.
- legacy `compound_indexes` input is appended to `CollectionSchema.compound_indexes`
  for compatibility with current split Axon schema JSON shapes.
- `EsfCoreDocument` serializes only the unified `indexes:` list; it does not
  emit `compound_indexes`.

## Tests And Checks

Focused tests:

- `cargo test -p axon-esf`
- `cargo test -p axon-schema`

`axon-esf` tests:

- serde round-trip for `IndexType`, `IndexDef`, `CompoundIndexField`,
  `CompoundIndexDef`.
- `IndexDeclaration` serializes single and compound declarations to the
  expected object shapes.
- `IndexDeclaration` rejects both `field`+`fields` and neither.
- `EsfCoreDocument` deserializes/serializes `esf_version`, `collection`,
  `description`, `entity_schema`, and mixed `indexes`.
- `EntitySchemaDocument` deserializes/serializes `entity_schema` and mixed
  `indexes`.
- `CompiledSchema` validates multiple records after one compile.
- invalid `email`, `uuid`, and `date-time` fail.
- internal `#/$defs` `$ref` works.

`axon-schema` tests:

- existing enhanced required/enum/type tests pass unchanged.
- ESF fixture still parses.
- ESF document with `description` and mixed `indexes:` converts into
  `CollectionSchema.description`, `indexes`, and `compound_indexes`.

Performance evidence:

- Add `crates/axon-esf/examples/compiled_schema_perf.rs`.
- Command:

  ```bash
  cargo run -p axon-esf --release --example compiled_schema_perf
  ```

- It compiles once, validates many small valid records, prints average
  nanoseconds per validation, and exits non-zero if release-mode average is
  >= 1 microsecond. This command is hardware-sensitive but is the explicit
  acceptance demonstration for the bead.

Final gates:

```bash
cargo tree -p axon-esf
cargo tree -p axon-esf | rg 'axon-core|axon-schema|axon-cypher-ast|axon-api|axon-storage|axon-server|axon-graphql|axon-mcp|reqwest|hyper|tower|tower-http'
cargo check
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

## Review Question

You are a critic, not a validator. Find implementation rework risks,
contradictions, missing constraints, ambiguous interfaces, hidden assumptions,
and places where two competent implementers would make different choices.
Do not implement the plan. Do not balance criticism with praise.

Focus on whether this fourth-round plan has converged enough to implement.

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
