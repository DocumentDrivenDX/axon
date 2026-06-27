# Axon — Claude Code Instructions

## Project Overview

Axon is a cloud-native, auditable, schema-first transactional data store for agentic applications. It is implemented in **Rust** as a Cargo workspace.

- Product Vision: `docs/helix/00-discover/product-vision.md`
- PRD: `docs/helix/01-frame/prd.md`
- Features: `docs/helix/01-frame/features/`
- Architecture: `docs/helix/02-design/architecture.md`
- ADR index and contracts: `docs/helix/02-design/README.md`

## Workspace Layout

```
crates/
  axon-core/           # Core types, traits, error hierarchy
  axon-schema/         # Schema definitions, validation, and migration
  axon-audit/          # Immutable audit log with provenance
  axon-storage/        # Storage adapter trait + in-memory impl
  axon-api/            # Request/response types and handler
  axon-server/         # HTTP/gRPC server (axum + tonic)
  axon-graphql/        # GraphQL schema auto-generated from collection schemas
  axon-mcp/            # MCP (Model Context Protocol) server
  axon-registry/       # Confluent-compatible schema registry HTTP facade (FEAT-021)
  axon-cypher-ast/     # openCypher subset AST, parser, validator
  axon-cypher/         # openCypher subset planner and executor
  axon-render/         # Markdown template rendering and validation
  axon-control-plane/  # Multi-tenant control plane (FEAT-025)
  axon-config/         # XDG path resolution and TOML config loading
  axon-sim/            # Deterministic simulation testing (DST) framework
  axon-cli/            # axon binary entry point
sdk/typescript/        # TypeScript SDK (@axon/client)
ui/                    # SvelteKit admin UI
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
- **Authority order**: Vision > PRD > Features/Stories > Architecture/ADRs/Contracts > Tests > Code.

## Issue Tracker

Issues are managed with `ddx bead` subcommands against `.ddx/beads.jsonl`.

```bash
ddx bead ready --json          # List ready issues
ddx bead show <id>             # Show issue details
ddx bead update <id> --claim   # Claim an issue (sets in_progress)
ddx bead close <id>            # Close a completed issue
```
