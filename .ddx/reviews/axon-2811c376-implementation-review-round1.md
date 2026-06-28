## Target

Adversarial implementation review for `axon-2811c376`.

## Context

The plan converged in `.ddx/reviews/axon-2811c376-plan-round8.md`.
The current working tree implements that plan:

- new `crates/axon-esf` leaf crate;
- root `jsonschema` workspace dependency changed to `default-features = false`;
- `axon-schema` re-exports moved ESF index types and uses
  `axon_esf::CompiledSchema` for entity validation;
- `axon-schema::EsfDocument` now parses `description`, unified `indexes`, and
  legacy `compound_indexes`;
- CLI reqwest dependency now declares `json` explicitly because removing
  `jsonschema` default features removed the accidental transitive feature.

## Verified So Far

- `cargo test -p axon-esf` passed.
- `cargo test -p axon-schema` passed.
- `cargo tree -p axon-esf` negative check passed for:
  `axon-core|axon-schema|axon-cypher-ast|axon-api|axon-storage|axon-server|axon-graphql|axon-mcp|reqwest|hyper|tower|tower-http`.
- `cargo run -p axon-esf --release --example compiled_schema_perf` passed
  with average `215 ns`.
- `cargo check` passed.
- `cargo clippy -- -D warnings` passed.
- `cargo fmt --check` passed.
- Full `cargo test` failed only in `axon-storage` Postgres tests due
  `pool timed out while waiting for an open connection`; non-Postgres tests
  visible in the run passed. This appears environment-related.

## Review Question

Review the current working tree adversarially against the bead, the round 8
plan, and the repository code. Focus on correctness and regressions, not style.
Look for blockers that should be fixed before commit.

Pay special attention to:

- public API shape of `axon-esf`;
- whether moved index types preserve `String/Integer/Float/Datetime/Boolean`;
- serde behavior for `IndexDeclaration` inside carriers with flattened `extra`;
- whether enhanced `axon-schema` errors still have the required data;
- dependency leakage into `axon-esf`;
- workspace behavior caused by changing `jsonschema` default features;
- whether the CLI reqwest feature change is justified and sufficient.

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
