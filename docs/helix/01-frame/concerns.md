# Project Concerns

## Active Concerns
- rust-cargo (tech-stack)
- typescript-bun (tech-stack, admin UI)

## Area Labels

| Label | Applies to |
|-------|-----------|
| `all` | Every bead |
| `api` | HTTP/gRPC server layer (axon-api, axon-server) |
| `cli` | axon-cli |
| `data` | Storage adapters, schema validation (axon-storage, axon-schema) |
| `ui` | Admin web UI (SvelteKit, ui/) |

## Project Overrides

### rust-cargo
- **Edition**: currently 2021; upgrade to 2024 is tracked work
- **MSRV**: 1.75 — lower than niflheim's; raise as dependencies allow
- **Workspace lints**: `[workspace.lints]` configured with clippy pedantic+nursery (resolved)
- **cargo-deny**: `deny.toml` configured for license, advisory, ban, source checks (resolved)

### typescript-bun
- **Scope**: `area:ui` beads only. Rust crates remain governed by `rust-cargo`.
- **Framework override**: SvelteKit (Svelte 5, runes) with adapter-static
- **Runtime**: Bun (package manager + script runner)
- **Bundler override**: Vite (SvelteKit default)
- **Language override**: TypeScript throughout
- **Location**: `ui/` at project root
- **Quality gates**: `cd ui && bun run typecheck && bun run lint && bun test && bun run build`
- **Dev**: `cd ui && bun run dev` (Vite on :5173, proxies API to :3000)
- **Build**: `cd ui && bun install && bun run build`
- **Serving**: axum serves static files from `--ui-dir` path; Phase 2 embeds in binary
- **Design reference**: ADR-006
- **Concern package**: `/home/erik/Projects/helix/workflows/concerns/typescript-bun`
