## Target

Implementation-ready convergence plan for bead `axon-2811c376`.

## Non-Negotiable Decisions

1. `crates/axon-esf` is added to `[workspace].members`.
2. `axon-esf` normal dependencies are exactly `serde`, `serde_json`, and
   `jsonschema`.
3. Workspace `jsonschema` changes to:

   ```toml
   jsonschema = { version = "0.28", default-features = false }
   ```

4. Entity validation format assertions are intentionally turned ON:
   `CompiledSchema::compile` always uses `should_validate_formats(true)`.
   This is a deliberate behavior tightening required by the bead. It means
   malformed `email`, `uuid`, `date-time`, and other built-in formats in
   entity schemas can now be rejected where they previously passed. Closure
   evidence must name this explicitly.
5. Link metadata validation is not tightened in this bead.

## Exact `axon-esf` Public API

Exports from `lib.rs`:

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

### Types

`IndexType`, `IndexDef`, `CompoundIndexField`, and `CompoundIndexDef` move from
`axon-schema` to `axon-esf` with the same serde field names. `IndexType`'s
`Display` impl moves with the type.

`IndexDeclaration`:

```rust
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum IndexDeclaration {
    Single(IndexDef),
    Compound(CompoundIndexDef),
}
```

Custom deserialization rules:

- Single allowed keys: `field`, `type`, `unique`.
- Compound allowed keys: `fields`, `unique`.
- `field` without `fields` -> `Single`.
- `fields` without `field` -> `Compound`.
- both `field` and `fields` -> error.
- neither -> error.
- unknown keys -> error.
- empty object -> error.

`EntitySchemaDocument`:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntitySchemaDocument {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<IndexDeclaration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compound_indexes: Vec<CompoundIndexDef>,
}
```

The `compound_indexes` field is accepted for legacy split-shape compatibility
so external consumers do not silently drop it. Contract-shaped serialization
uses `indexes`; legacy split data remains visible if present.

`EsfCoreDocument`:

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compound_indexes: Vec<CompoundIndexDef>,
}
```

No parse helpers. These are serde data types only.

### Validation

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaCompileError {
    pub message: String,
}

impl std::fmt::Display for SchemaCompileError { ... }
impl std::error::Error for SchemaCompileError {}

#[derive(Debug, Clone, PartialEq)]
pub struct RawValidationError {
    pub instance_path: String,
    pub message: String,
    pub instance: Value,
}

impl std::fmt::Display for RawValidationError { ... }

#[derive(Debug, Clone, PartialEq)]
pub struct RawValidationErrors(pub Vec<RawValidationError>);

impl RawValidationErrors {
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
}

impl std::fmt::Display for RawValidationErrors { ... }
impl std::error::Error for RawValidationErrors {}

pub struct CompiledSchema {
    validator: jsonschema::Validator,
}

impl CompiledSchema {
    pub fn compile(schema: &Value) -> Result<Self, SchemaCompileError>;
    pub fn validate(&self, data: &Value) -> Result<(), RawValidationErrors>;
}
```

`RawValidationError.message` is exactly `jsonschema::ValidationError::to_string()`.
`RawValidationError.instance_path` carries the path separately. No public API
returns `jsonschema` internals.

## `axon-schema` Integration

- Add dependency on `axon-esf`.
- Preserve all existing paths:
  - `axon_schema::schema::{IndexType, IndexDef, CompoundIndexField,
    CompoundIndexDef, IndexDeclaration}`
  - `axon_schema::{IndexType, IndexDef, CompoundIndexField, CompoundIndexDef,
    IndexDeclaration}`
- `validate_entity` keeps signature:

  ```rust
  pub fn validate_entity(
      schema: &CollectionSchema,
      data: &Value,
  ) -> Result<(), SchemaValidationErrors>
  ```

- `compile_entity_schema` keeps signature:

  ```rust
  pub fn compile_entity_schema(json_schema: &Value) -> Result<(), AxonError>
  ```

- Compile-failure message prefixes remain:
  - validate path: `invalid schema definition: {message}` with field path `/`;
  - compile path: `invalid schema: {message}`.
- Refactor enhancer:
  - `enhance_raw_schema_error(&axon_esf::RawValidationError, &Value)`;
  - classifier consumes `raw.message`, `raw.instance_path`, and `raw.instance`.
- `validate_link_metadata` remains on the previous direct path and previous
  string-joined error shape.

## `axon-schema::EsfDocument`

Add:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub description: Option<String>,
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub indexes: Vec<axon_esf::IndexDeclaration>,
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub compound_indexes: Vec<axon_esf::CompoundIndexDef>,
```

Conversion:

- `description` flows to `CollectionSchema.description`.
- unified `indexes` splits into existing internal `indexes` and
  `compound_indexes`.
- legacy split `compound_indexes` is appended after unified compound entries.
- duplicates are preserved, not deduplicated. Current `CollectionSchema`
  preserves vector contents; validation/deduplication is not introduced here.

## Tests

`axon-esf`:

- serde round-trip for moved index types.
- `IndexDeclaration` serializes single and compound shapes.
- `IndexDeclaration` rejects ambiguous, empty, missing discriminator, and
  unknown-key objects.
- `EsfCoreDocument` and `EntitySchemaDocument` preserve `compound_indexes`
  legacy split data and unified `indexes`.
- `CompiledSchema` validates multiple records after one compile.
- invalid `email`, `uuid`, and `date-time` fail.
- at least one test proves formats still work after `default-features = false`.
- internal `#/$defs` `$ref` works.

`axon-schema`:

- existing enhanced required/enum/type tests pass unchanged.
- compile failure still reports field path `/` and existing message prefix.
- ESF fixture still parses.
- ESF document with `description`, mixed `indexes`, and legacy
  `compound_indexes` converts into expected `CollectionSchema` fields.
- add an entity-validation regression test showing malformed `date-time` is now
  rejected intentionally.

## Performance Example

Add `crates/axon-esf/examples/compiled_schema_perf.rs` with fixed parameters:

- schema: object with required `id`, `email`, `created_at`, `count`;
  formats `uuid`, `email`, `date-time`;
  `additionalProperties: false`.
- record: one valid object with RFC3339 `created_at`.
- warmup: 10,000 validations.
- timed loop: 1,000,000 validations.
- measure with `std::time::Instant`.
- print total duration and average nanoseconds.
- in release builds, exit non-zero if average >= 1,000 ns.
- in debug builds, print a message and do not enforce the threshold.

Acceptance command:

```bash
cargo run -p axon-esf --release --example compiled_schema_perf
```

This is deliberately hardware-sensitive because the bead asks for a
sub-microsecond demonstration; failures should be treated as evidence to inspect
the machine or threshold before closing.

## Final Checks

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

You are a critic, not a validator. Find implementation rework risks,
contradictions, missing constraints, ambiguous interfaces, hidden assumptions,
and places where two competent implementers would make different choices.
Do not implement the plan. Do not balance criticism with praise.

Focus on whether this fifth-round plan has converged enough to implement.

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
