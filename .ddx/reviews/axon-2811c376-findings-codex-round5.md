### Findings

| Severity | Area | Finding |
|---|---|---|
| BLOCKING | `jsonschema` feature policy | The plan mandates workspace `jsonschema = { version = "0.28", default-features = false }`, but does not say whether `axon-schema` should intentionally lose `resolve-http` / `resolve-file` behavior or re-enable those features locally. Current `axon-schema` uses `jsonschema.workspace = true` and directly compiles entity/link schemas. This conflicts with the bead’s “No behavior change for axon” requirement unless remote/file `$ref` loss is explicitly accepted or locally preserved. |
| BLOCKING | Draft selection | `CompiledSchema::compile` is required to call `should_validate_formats(true)`, but the exact validation API does not explicitly require `.with_draft(jsonschema::Draft::Draft202012)`. Current `axon-schema` does force Draft 2020-12, and the bead requires JSON Schema 2020-12. Two implementers could choose explicit Draft202012 vs default/autodetect behavior. |
| WARNING | Core document lossiness | `EsfCoreDocument` omits non-core ESF fields such as `link_types`, `access_control`, `queries`, `validation_rules`, and `lifecycles`, while deriving normal serde `Deserialize`. Unknown top-level fields will be silently dropped if a consumer round-trips a fuller ESF document through this type. The plan explicitly protects `compound_indexes` from silent loss, but does not define the unknown-field policy for the rest of ESF. |
| WARNING | Performance evidence | The performance example fixes warmup/timed counts and threshold, but does not require `std::hint::black_box` or otherwise specify how to prevent release-mode optimization from weakening the measurement. Since closure depends on a sub-microsecond demonstration, this leaves room for non-comparable implementations. |
| WARNING | Public `Display` contracts | `SchemaCompileError`, `RawValidationError`, and `RawValidationErrors` require `Display`, but only `RawValidationError.message` is specified exactly. If downstream code or tests compare string output, implementers can produce incompatible `Display` shapes while satisfying the written plan. |
| NOTE | Link metadata scope | The plan correctly says link metadata validation is not tightened, but the workspace `default-features = false` change can still alter link metadata behavior for schemas using file/http `$ref`. That is separate from format assertions and should be named in closure evidence if intentional. |

### Verdict: REQUEST_CHANGES

### Summary

The plan is close, but not fully converged. The remaining blockers are not about the new `axon-esf` API shape; they are about workspace-wide `jsonschema` behavior and the exact Draft 2020-12 compilation contract. Without tightening those points, competent implementers can make different choices that pass many listed tests but still create behavior drift or rework.