# Axon — Claude Code Instructions

## Project Overview

Axon is a cloud-native, auditable, schema-first transactional data store for agentic applications. It is implemented in **Rust** as a Cargo workspace.

- Product Vision: `docs/helix/00-discover/product-vision.md`
- PRD: `docs/helix/01-frame/prd.md`
- Technical Requirements: `docs/helix/01-frame/technical-requirements.md`
- Features: `docs/helix/01-frame/features/`

## Workspace Layout

```
crates/
  axon-core/      # Core types, traits, error hierarchy
  axon-schema/    # Schema definitions and validation
  axon-audit/     # Immutable audit log
  axon-storage/   # Storage adapter trait + in-memory impl
  axon-api/       # Request/response types and handler
  axon-cli/       # axon binary entry point
```

## Development Commands

```bash
cargo check          # Type-check all crates
cargo test           # Run all tests
cargo clippy -- -D warnings   # Lint (warnings are errors)
cargo fmt            # Format all code
```

## Key Conventions

- **Test-first**: write tests before or alongside implementation.
- **No unwrap() in library code**: use `?` and `AxonError`.
- **Clippy clean**: CI enforces `-D warnings`.
- **Workspace dependencies**: declare shared deps in the root `Cargo.toml` `[workspace.dependencies]` table.
- **Authority order**: Vision > PRD > Technical Requirements > Features > Tests > Code.

## Issue Tracker

Issues are managed with `ddx bead` subcommands against `.ddx/beads.jsonl`.

```bash
ddx bead ready --json          # List ready issues
ddx bead show <id>             # Show issue details
ddx bead update <id> --claim   # Claim an issue (sets in_progress)
ddx bead close <id>            # Close a completed issue
```
