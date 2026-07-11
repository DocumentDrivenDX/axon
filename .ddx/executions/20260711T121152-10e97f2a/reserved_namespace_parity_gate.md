# Reserved Namespace Surface Parity Gate

Bead: axon-gap-closure-15d3a1e6
Date: 2026-07-11

## Result

All acceptance gates passed after restoring TypeScript dependencies with `npm --prefix sdk/typescript ci`.

## Evidence

- `cargo test --workspace reserved_namespace_surface_parity` passed.
- `npm --prefix sdk/typescript test -- reserved_namespace_surface_parity` passed.
- `rg -n "reserved_namespace_surface_parity" crates sdk/typescript` found shared vectors plus embedded Rust, HTTP, gRPC, GraphQL, MCP, CLI, and TypeScript coverage.
- `cargo test --workspace` passed.
- `npm --prefix sdk/typescript run build` passed.
- `cargo clippy -- -D warnings` passed.
- `cargo fmt --check` passed.
