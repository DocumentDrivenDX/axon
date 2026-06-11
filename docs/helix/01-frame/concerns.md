---
ddx:
  id: helix.concerns
  depends_on: [helix.prd]
---

# Project Concerns

Project Concerns declare active cross-cutting context for downstream work. They
are not principles, requirements, ADRs, test plans, or implementation tasks.

## Active Concerns

| Concern | Source | Areas | Why Active | Key Practices |
|---------|--------|-------|------------|---------------|
| rust-cargo | library (`.ddx/plugins/helix/workflows/concerns/rust-cargo`) | `area:all` (server/core; everything except `area:ui` and `area:website`) | Axon is implemented in Rust as a Cargo workspace; every crate, test, and CI gate flows through the Rust toolchain. | Pinned toolchain (`rust-toolchain.toml`); `#![forbid(unsafe_code)]` in library crates; `cargo clippy -- -D warnings`; `cargo fmt`; `cargo deny` checks; workspace-level dependency declarations. |
| typescript-bun | library (`.ddx/plugins/helix/workflows/concerns/typescript-bun`) | `area:ui` only | The admin web UI (`ui/`) is a SvelteKit + TypeScript app run with Bun; it is the single non-Rust implementation surface. | `cd ui && bun run typecheck && bun run lint && bun test && bun run build` quality gate; Bun as package manager and script runner; Vite bundling via SvelteKit; TypeScript throughout. |
| security-owasp | library (`.ddx/plugins/helix/workflows/concerns/security-owasp`) | `area:all` (cross-cutting, both stacks) | Axon is an auditable transactional store for agentic apps; authn/authz, input validation, and audit integrity are core product properties, not add-ons. | Tailscale-delegated identity (ADR-005); deny-by-default RBAC on all axum/tonic handlers; JSON Schema validation on entity write paths; parameterized SQL only; per-actor rate limiting; structured non-leaking errors; immutable audit log (axon-audit); no secrets in source control; `cargo deny check advisories` + secrets scanning in CI. |
| hugo-hextra | library (`.ddx/plugins/helix/workflows/concerns/hugo-hextra`) | `area:website` | The product microsite lives in `website/` and is built with Hugo + the Hextra theme. | `cd website && hugo --gc --minify` must build without errors; Hugo 0.159.2 extended pinned in CI; Hextra v0.12.1 via Hugo Modules; base URL `https://DocumentDrivenDX.github.io/axon/`. |
| demo-asciinema | library (`.ddx/plugins/helix/workflows/concerns/demo-asciinema`) | `area:website`, `area:cli` | CLI demo reels are recorded with asciinema and embedded in the microsite. | Demo sources in `docs/demos/quickstart/` (script, Dockerfile, recordings); cast output at `website/static/demos/quickstart.cast`; record via `docker build -t axon:recording docs/demos/quickstart/ && docker run --rm -v $(pwd)/website/static/demos:/recordings axon:recording`; terminal dimensions 100 cols × 35 rows. |

> **Setup gap**: the concern package directories referenced above exist in the
> plugin (`.ddx/plugins/helix/workflows/concerns/`) but are currently **empty**
> — `rust-cargo`, `typescript-bun`, `security-owasp`, `hugo-hextra`,
> `demo-asciinema`, and `e2e-playwright` contain no `concern.md` or
> `practices.md` (compare `auth-local-sessions`, which ships both). Until the
> plugin sync is repaired, the practices recorded in this file are the
> authoritative copy.

## Project Overrides

| Concern | Practice | Override | Authority |
|---------|----------|----------|-----------|
| rust-cargo | Rust edition | Currently 2021; upgrade to 2024 is tracked work | Tracked in issue tracker |
| rust-cargo | MSRV | 1.75 — lower than niflheim's; raise as dependencies allow | Cargo.toml (`rust-version`) |
| rust-cargo | Toolchain pinning via wrapper script | `rust-toolchain.toml` at project root — standard rustup mechanism, equivalent reproducibility | rust-toolchain.toml |
| rust-cargo | Unsafe code policy | `#![forbid(unsafe_code)]` in all library crates (axon-core, axon-schema, axon-audit, axon-storage, axon-api, axon-graphql, axon-mcp, axon-render). Binary crates (axon-server, axon-cli, axon-sim) unrestricted because upstream deps may require it, but project code must not introduce `unsafe` blocks | Needs ADR |
| rust-cargo | Workspace lints | `[workspace.lints]` configured with clippy pedantic+nursery (resolved) | Cargo.toml |
| rust-cargo | cargo-deny | `deny.toml` configured for license, advisory, ban, source checks (resolved) | deny.toml |
| typescript-bun | Default scope (whole project) | Scoped to `area:ui` beads only; Rust crates remain governed by rust-cargo | ADR-001, ADR-006 |
| typescript-bun | Frontend framework (shipped default react-nextjs) | SvelteKit (Svelte 5, runes) with adapter-static; Vite bundler (SvelteKit default); located at `ui/` | ADR-006 |
| typescript-bun | Serving model | Dev: `cd ui && bun run dev` (Vite on :5173, proxies API to :3000). Build: `bun install && bun run build`. axum serves static files from `--ui-dir`; Phase 2 embeds assets in the binary | ADR-006 |
| security-owasp | Authentication | Tailscale whois via tsnet/LocalAPI; guest mode with synthetic actor identity when Tailscale is unavailable; no password storage — identity fully delegated to the Tailscale control plane | ADR-005 |
| security-owasp | TLS termination | Delegated to Tailscale (MagicDNS automatic certs) or a reverse proxy; axon listens on localhost by default | ADR-005 |
| security-owasp | CSP headers | Not applicable — API-first service; no user-supplied HTML rendered in a browser context | ADR-005 (rationale recorded here) |
| security-owasp | CSRF protection | Not applicable — bearer-style Tailscale identity, not cookies | ADR-005 (rationale recorded here) |
| security-owasp | CORS | Not expected in the primary Tailscale-mesh deployment; if the admin UI is served cross-origin, CORS is scoped to that origin only | ADR-005, ADR-006 |
| security-owasp | Password storage | Not applicable — authentication fully delegated to Tailscale | ADR-005 |
| hugo-hextra | Scope | `area:website` — `website/` directory only | Project convention |
| demo-asciinema | Scope | CLI demo reels for the microsite only | Project convention |

## Area Labels

This project uses the following area labels for concern scoping:

| Label | Applies to |
|-------|-----------|
| `area:all` | Every bead |
| `area:api` | HTTP/gRPC server layer (axon-api, axon-server) |
| `area:cli` | axon-cli |
| `area:data` | Storage adapters, schema validation (axon-storage, axon-schema) |
| `area:ui` | Admin web UI (SvelteKit, `ui/`) |
| `area:website` | Product microsite (Hugo + Hextra, `website/`) |

## Concern Conflicts

| Conflict | Resolution |
|----------|------------|
| rust-cargo vs. typescript-bun (both are language-runtime candidates) | Rust owns the server and core: all `crates/*` work, the API/gRPC layer, storage, CLI, and tooling are governed by rust-cargo. TypeScript/Bun is a scoped secondary, limited to `area:ui` — the SvelteKit admin UI under `ui/` and its SDK surface. A bead labeled `area:ui` follows typescript-bun practices; every other bead follows rust-cargo. Neither concern's practices leak across the boundary. |

## Slot Records

Exclusive-slot resolution per `.ddx/plugins/helix/workflows/concerns/slots.yml`
(resolution order: operator override → shipped default → recorded assumption).
Operator overrides are recorded in `docs/helix/01-frame/concerns.local.yml`.

| Slot | Filler | Source | Shipped Default | Notes |
|------|--------|--------|-----------------|-------|
| language-runtime | rust-cargo | operator-override | typescript-bun | Deviation from shipped default. typescript-bun remains active as a **scoped secondary** for `area:ui` only (SvelteKit admin UI + SDK); it does not fill this slot. See Concern Conflicts. |
| frontend-framework | sveltekit | operator-override | react-nextjs | Recorded deviation from shipped default, governed by ADR-006 (SvelteKit + Bun for the admin UI). No `sveltekit` package exists in the concern library; practices are carried by the typescript-bun overrides above. |
| e2e-framework | e2e-playwright | shipped-default | e2e-playwright | Matches shipped default. Playwright suite exists under `ui/tests/e2e/` (*.spec.ts). Concern package directory in the plugin is empty — see setup gap above. |
| auth-provider | tailscale-localapi | operator-override | auth-local-sessions | Deviation from shipped default, governed by ADR-005 (Tailscale LocalAPI whois via tsnet; guest-mode fallback). No `tailscale-localapi` package exists in the concern library; practices recorded under security-owasp overrides. |
| datastore | sqlite-libsql + postgresql | assumption | — (slot defined, no default) | Recorded from ADR-003 (backing store architecture) and ADR-010 (physical storage and secondary indexes): embedded SQLite/libSQL plus PostgreSQL adapter. **Flagged for operator confirmation.** |
| deploy-target | self-hosted single binary + BYOC control plane | assumption | — (no default) | Recorded from FEAT-028 (unified single self-hosted binary) and FEAT-025 (BYOC control plane, ratified P1 by product owner 2026-06-10). **Flagged for operator confirmation.** |
| architecture-style | modular monolith (single unified binary) | assumption | — (no default; select on signal) | Recorded from FEAT-028 and ADR-017 (control plane): one binary composing the workspace crates as modules. **Flagged for operator confirmation.** |
