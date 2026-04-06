# Project Concerns

## Active Concerns
- rust-cargo (tech-stack)
- svelte-bun (tech-stack, admin UI)

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

### svelte-bun
- **Framework**: SvelteKit (Svelte 5, runes) with adapter-static
- **Runtime**: Bun (package manager + script runner)
- **Bundler**: Vite (SvelteKit default)
- **Language**: TypeScript throughout
- **Location**: `ui/` at project root
- **Build**: `cd ui && bun install && bun run build`
- **Dev**: `cd ui && bun run dev` (Vite on :5173, proxies API to :3000)
- **Serving**: axum serves static files from `--ui-dir` path; Phase 2 embeds in binary
- **Design reference**: ADR-006
