---
ddx:
  id: ADR-006
  depends_on:
    - helix.prd
    - FEAT-005
    - ADR-005
  review:
    self_hash: 4a36f82ee091e5895f8a131244be5bb2dff01c46c4a37e1e83e2bb8e237d0cad
    deps:
      ADR-005: 86046c9a1474abf0f42a1962eedade8582212487f7face55f5256fefa800ff98
      FEAT-005: 1fab4e58214106451af84deee1a1bfb5c2b520333e6be2a7cd723153730c829c
      helix.prd: dff98156a6cc934f406611b78b513892d85cee1bd7b4c011f045146fcdfd23e1
    reviewed_at: "2026-06-15T00:35:16Z"
---
# ADR-006: Admin UI — SvelteKit + Bun + Vite

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-04-05 | Accepted | Erik LaBianca | FEAT-005, ADR-005 | High |

## Context

Axon's PRD lists a GUI/dashboard as a post-V1 item, but the need for a simple
admin console has become clear: browsing collections, inspecting entities,
viewing/editing schemas, and reading the audit log. The console should be a
lightweight web application that calls the existing HTTP gateway API.

| Aspect | Description |
|--------|-------------|
| Problem | No visual interface for inspecting or managing Axon data |
| Current State | CLI and API only; all inspection requires terminal commands |
| Requirements | Browse collections, CRUD entities, view/edit schemas, browse audit log |
| Decision Drivers | Must reuse the existing HTTP gateway (no new backend surface); fast iteration for a small internal tool; minimal bundle and toolchain footprint outside the Rust workspace |

## Decision

Build the admin UI as a **SvelteKit** application using **Bun** as the
package manager and runtime, with **Vite** as the bundler.

### Stack

| Layer | Choice | Rationale |
|-------|--------|-----------|
| Framework | SvelteKit (Svelte 5, runes) | Compiled to vanilla JS — no virtual DOM, tiny bundles, scoped CSS built-in. File-based routing gives structure without boilerplate |
| Language | TypeScript | Type safety, matches the existing SDK |
| Bundler | Vite | SvelteKit's default; fast HMR, optimized production builds |
| Package manager / runtime | Bun | Single binary, ~1s installs, runs Vite natively, replaces npm+node |
| Styling | Scoped CSS (Svelte) | No CSS framework — admin console doesn't need a component library |
| API layer | fetch() to HTTP gateway | No gRPC from browser; reuse existing REST endpoints |
| Deployment | adapter-static → static files served by axum | No server-side rendering needed; purely client-side SPA |

The UI lives in a standalone `ui/` directory (not a Cargo workspace member)
and consumes only the existing HTTP gateway. **No new backend routes are
required** — the normative HTTP endpoint surface is owned by
[CONTRACT-001](../contracts/CONTRACT-001-http-api-surface.md), not this ADR.

**Serving**: Phase 1 — axum serves the built static files from a configurable
`--ui-dir` path. Phase 2 (follow-on) — embed built assets into the binary for
single-binary distribution.

### Authentication

The admin UI inherits Tailscale identity per ADR-005 — the browser's
connection arrives over the tailnet, and the server resolves the user via
the LocalAPI whois mechanism. The UI itself carries no credentials.

### Scope — V1

**Included**: browse/create/drop collections, browse/create/delete entities,
view/edit schemas (JSON editor), browse audit log with filtering.

**Not included (follow-on)**: entity editing with OCC, graph/link
visualization, real-time updates, schema diff/migration preview, bead
lifecycle management UI.

## Alternatives Considered

### A. React + Vite + Bun

Standard React SPA with TypeScript.

**Not chosen because**: Larger bundle, more boilerplate (hooks, state
management), virtual DOM overhead. For an admin console, Svelte's compiled
approach produces a smaller, faster result with less code.

### B. Embedded single-file HTML (no build step)

Vanilla HTML + JS + CSS embedded in the Rust binary via `include_str!`.

**Not chosen because**: No component model, no type safety, unmaintainable
past ~500 lines. Schema editor and entity browser need real components.

### C. htmx + server-rendered templates (askama/maud)

HTML rendered server-side in Rust with htmx for interactivity.

**Not chosen because**: Requires adding template rendering to the Rust server,
mixes UI concerns into backend code, harder to build interactive features
(schema editor, entity detail modals).

### D. Leptos / Dioxus (Rust WASM)

Write the UI in Rust, compile to WASM.

**Not chosen because**: Immature ecosystems, larger WASM bundles, steep
learning curve for UI work, poor developer experience for rapid iteration.

## Consequences

**Positive**:
- Fast iteration: Bun installs in ~1s, Vite HMR updates in <100ms
- Tiny production bundles: Svelte compiles away the framework
- Type-safe: TypeScript throughout, shared types with SDK
- No new backend work: UI consumes existing HTTP API (CONTRACT-001)
- Clean separation: `ui/` directory is independently buildable

**Negative**:
- Adds Bun as a build dependency (not just Rust toolchain); CI needs
  `bun install && bun run build` alongside `cargo build`
- Two dev servers during development (Vite + axon-server)
- SvelteKit is newer than React — smaller ecosystem for admin UI components
- Static adapter means no SSR — initial load is blank until JS executes

## Risks

| Risk | Prob | Impact | Mitigation |
|------|------|--------|------------|
| Svelte 5 / SvelteKit churn breaks the build | Medium | Low | UI is isolated in `ui/`; pinned lockfile (`bun.lock`); small app surface makes upgrades cheap |
| Bun incompatibility with a needed package | Low | Low | Bun is npm-registry compatible; fallback to node for the build is possible without changing the framework choice |
| UI drifts from the HTTP API | Medium | Medium | CONTRACT-001 owns the endpoint surface; typed `api.ts` wrapper centralizes all calls |

## Validation

| Success Metric | Review Trigger |
|----------------|----------------|
| `cd ui && bun run typecheck && bun run lint && bun test && bun run build` passes in CI | Persistent CI breakage attributable to the Bun/SvelteKit toolchain |
| Admin tasks (browse, schema edit, audit review) doable without CLI | Users routinely fall back to the CLI for covered tasks |
| Production bundle stays small (no framework runtime bloat) | Bundle growth that negates the Svelte selection rationale — re-evaluate stack |

## Supersession

- **Supersedes**: None
- **Superseded by**: None

## Concern Impact

- **typescript-bun (ui)**: This ADR is the source of the typescript-bun concern — it selects SvelteKit/TypeScript/Bun for `area:ui` and records the framework and serving-model overrides in `docs/helix/01-frame/concerns.md`.
- **security-owasp**: UI carries no credentials (identity inherited per ADR-005); CORS scoping for a cross-origin admin UI is recorded as a project override.

## References

- [CONTRACT-001: HTTP API Surface](../contracts/CONTRACT-001-http-api-surface.md) — owns the endpoint surface the UI consumes
- [ADR-005: Authentication via Tailscale LocalAPI](ADR-005-authentication-tailscale-localapi.md)
- [FEAT-005: API Surface](../../01-frame/features/FEAT-005-api-surface.md)
