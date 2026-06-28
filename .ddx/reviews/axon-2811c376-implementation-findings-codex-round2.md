### Findings

| Severity | Area | Finding |
|---|---|---|
| NOTE | Round 1 blocker | Fixed. `IndexDeclaration` now discriminates on `field` vs `fields` before checking allowed keys; the compound branch only permits `["fields", "unique"]` at `crates/axon-esf/src/types.rs:82-93`. Regression coverage includes direct rejection of `{"fields": ..., "type": ...}` at `crates/axon-esf/src/types.rs:232-245` and flattened carrier rejection at `crates/axon-esf/src/types.rs:280-297`. I reran both targeted tests and they passed. |
| NOTE | Dependency floor | `cargo tree -p axon-esf --edges normal` shows `axon-esf` depends on `serde`, `serde_json`, and `jsonschema`; no `reqwest`, `hyper`, `tower`, `axon-core`, or `axon-cypher-ast` appeared in the tree. |
| NOTE | Commit readiness | `git status --short` shows `crates/axon-esf/` and `.ddx/reviews/` are untracked. Not a code blocker, but the commit must intentionally include the new `crates/axon-esf` crate or the workspace member added in `Cargo.toml` will point at missing files. |

### Verdict: APPROVE

### Summary

The round 1 blocker is resolved: malformed unified compound index declarations with a top-level `type` key are now rejected through `IndexDeclaration`, including inside the flattened carrier structs. I found no new blocking defect in the current working tree. The only pre-commit caution is staging scope: the new `crates/axon-esf` files are still untracked.