# Project Concerns

## Active Concerns
- rust-cargo (tech-stack)
- typescript-bun (tech-stack, admin UI)
- security-owasp (security, all)

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
- **Toolchain pinning**: `rust-toolchain.toml` at project root (not a wrapper script). This is the standard rustup mechanism for reproducible builds and is equivalent to the concern library's wrapper-script recommendation.
- **Unsafe code policy**: `#![forbid(unsafe_code)]` in all library crates except axon-storage which uses `#![deny(unsafe_code)]` (storage adapters implement `unsafe Send` for connection wrappers with `#[allow(unsafe_code)]` on those specific impls). Binary crates are not restricted. Project code must not introduce new `unsafe` blocks.
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

### security-owasp
- **Scope**: cross-cutting; applies to all areas and both tech stacks
- **Authentication**: Tailscale whois via tsnet (ADR-005). When Tailscale is unavailable, the server falls back to guest mode with a synthetic actor identity. No passwords are stored — identity is delegated entirely to the Tailscale control plane.
- **Authorization**: RBAC role enforcement on all HTTP (axum) and gRPC (tonic) handlers. Policy is deny-by-default; each endpoint declares required roles and rejects requests that lack them.
- **Input validation**: JSON Schema validation on all entity write paths. Schemas are registered per entity type and enforced before any data reaches storage.
- **SQL injection prevention**: all database queries use parameterized statements via `rusqlite` (SQLite) and `tokio-postgres` (PostgreSQL). No string interpolation of user input into SQL.
- **Rate limiting**: per-actor sliding-window rate limiter applied at the HTTP middleware layer.
- **Error handling**: error responses return structured error codes without leaking internal details (no stack traces, SQL errors, or file paths).
- **Audit logging**: security-relevant actions (auth, entity mutations, schema changes) are recorded in the immutable audit log (axon-audit).
- **Secret management**: no secrets in source control; configuration loaded from environment variables.
- **TLS**: delegated to Tailscale (MagicDNS with automatic certs) or a reverse proxy in production; axon itself listens on localhost by default.
- **Dependency audit**: `cargo deny check advisories` configured in `deny.toml` and intended for CI gate.
- **Secrets scanning**: `gitleaks` or equivalent to run in CI on PRs.
- **Not applicable / deferred**:
  - **CSP headers**: Axon is an API-first service; it does not render user-supplied HTML in a browser context, so Content-Security-Policy is not required.
  - **CSRF protection**: the API uses bearer-style Tailscale identity, not cookies, so CSRF is not a vector.
  - **CORS**: no browser-origin cross-site requests expected in the primary deployment model (Tailscale mesh). If the admin UI is served from a different origin, CORS will be scoped to that origin only.
  - **Password storage**: not applicable; authentication is fully delegated to Tailscale.
- **Concern package**: `/home/erik/Projects/helix/workflows/concerns/security-owasp`
