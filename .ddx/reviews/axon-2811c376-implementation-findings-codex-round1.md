### Findings

| Severity | Area | Finding |
|---|---|---|
| BLOCKING | `axon-esf` serde | `IndexDeclaration` does not reject all malformed compound declarations. Round 8 requires unknown-key rejection and exactly one discriminator ([plan](/Users/erik/Projects/axon/.ddx/reviews/axon-2811c376-plan-round8.md:130)), but the implementation globally allows `type` before discriminating ([types.rs](/Users/erik/Projects/axon/crates/axon-esf/src/types.rs:82)). A throwaway probe confirmed `{"fields":[{"field":"status","type":"string"}],"type":"string"}` is accepted as `Compound(...)`, and the same malformed declaration is accepted through both flattened-extra carriers ([types.rs](/Users/erik/Projects/axon/crates/axon-esf/src/types.rs:126)). |
| NOTE | Dependencies | `axon-esf` dependency shape matches the plan: its manifest only lists `serde`, `serde_json`, and `jsonschema` ([Cargo.toml](/Users/erik/Projects/axon/crates/axon-esf/Cargo.toml:13)), and `cargo tree -p axon-esf` had no matches for Axon crates, reqwest, hyper, tower, or tower-http. |
| NOTE | Validation | `CompiledSchema` uses Draft 2020-12, enables format validation, collects `iter_errors`, and preserves raw message/path/instance fields as planned ([validation.rs](/Users/erik/Projects/axon/crates/axon-esf/src/validation.rs:72)). |

### Verdict: REQUEST_CHANGES

### Summary

The implementation is close, but the malformed compound-index acceptance violates an explicit round 8 contract and affects the new external carrier API. Fix the discriminator-specific unknown-key check and add a regression test for compound declarations with top-level `type` inside `IndexDeclaration`, `EntitySchemaDocument`, and/or `EsfCoreDocument`. I did not find dependency leakage or a problem with the CLI reqwest `json` feature change.