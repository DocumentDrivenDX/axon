## Target

Second adversarial implementation review for `axon-2811c376`.

## Context

Round 1 implementation review found one blocker:

- `IndexDeclaration` accepted malformed compound declarations with a top-level
  `type` key because the allowed-key check was global.

The current working tree fixes that by:

- checking allowed keys after discriminating single vs compound;
- allowing `["field", "type", "unique"]` for single indexes;
- allowing only `["fields", "unique"]` for compound indexes;
- adding regression tests for direct `IndexDeclaration` parsing and for
  `EntitySchemaDocument` / `EsfCoreDocument` flattened carriers.

## Verified After Fix

- `cargo test -p axon-esf` passed.
- `cargo test -p axon-schema` passed.
- `cargo check` passed.
- `cargo clippy -- -D warnings` passed.
- `cargo fmt --check` passed.
- `cargo tree -p axon-esf` negative check passed.

Prior full `cargo test` failed only in `axon-storage` Postgres tests due
`pool timed out while waiting for an open connection`.

## Review Question

Review the current working tree adversarially. Focus on whether the round 1
blocker is fixed and whether any new blocker remains before commit. Do not
repeat accepted warnings unless they now imply a concrete pre-commit defect.

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
