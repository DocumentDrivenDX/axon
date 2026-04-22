---
ddx:
  id: ADR-006
  depends_on:
    - helix.prd
    - FEAT-005
    - ADR-005
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

### Directory Structure

```
ui/
  package.json
  bun.lock
  vite.config.ts
  svelte.config.js
  tsconfig.json
  src/
    app.html
    app.css                        # global styles (dark theme, typography)
    routes/
      +layout.svelte               # sidebar nav, health indicator
      +page.svelte                 # collections list (home)
      collections/
        [name]/+page.svelte        # collection detail: entities table + schema tab
      schemas/
        +page.svelte               # schema list with version badges
        [name]/+page.svelte        # schema editor (JSON textarea + save)
      audit/
        +page.svelte               # audit log table with filters
    lib/
      api.ts                       # typed fetch wrapper for HTTP gateway
      types.ts                     # Entity, CollectionMetadata, CollectionSchema, AuditEntry
```

### Development Workflow

```bash
cd ui
bun install                         # install dependencies (~1s)
bun run dev                         # vite dev server on :5173
                                    # proxies /api/* to localhost:3000

# In another terminal:
cargo run -p axon-server            # HTTP gateway on :3000, gRPC on :50051
```

Vite proxies API requests to the running axon-server. No CORS configuration
needed in development.

### Production Build

```bash
cd ui
bun run build                       # outputs static files to ui/build/
```

### Serving Strategy

**Phase 1 (current)**: axum serves the built static files from a configurable
`--ui-dir` path. The axon-server binary and UI assets are deployed separately.

```rust
// In gateway.rs router construction:
.nest_service("/ui", ServeDir::new(ui_dir))
```

**Phase 2 (follow-on)**: Embed built assets into the binary via
`include_dir!` macro in `build.rs` for single-binary distribution.

### API Integration

The UI calls the same HTTP gateway endpoints that already exist:

| UI Feature | HTTP Endpoint |
|-----------|---------------|
| List collections | `GET /collections` |
| Create collection | `POST /collections/{name}` |
| Describe collection | `GET /collections/{name}` |
| Drop collection | `DELETE /collections/{name}` |
| Query entities | `POST /collections/{name}/query` |
| Get entity | `GET /entities/{collection}/{id}` |
| Create entity | `POST /entities/{collection}/{id}` |
| Delete entity | `DELETE /entities/{collection}/{id}` |
| Get schema | `GET /collections/{name}/schema` |
| Put schema | `PUT /collections/{name}/schema` |
| Query audit | `GET /audit/query` |
| Health check | `GET /health` |

No new backend routes are required.

### Authentication

Per ADR-005, auth is deferred. The UI runs as admin with full access to all
collections and operations. When Tailscale auth is implemented, the UI will
inherit the same identity — the browser's Tailscale session provides the
identity via the LocalAPI whois mechanism.

### Scope — V1

**Included**:
- Browse collections (list, describe, drop)
- Create collections with schema
- Browse entities (table view, detail view)
- Create and delete entities
- View and edit schemas (JSON editor)
- Browse audit log with filtering

**Not included (follow-on)**:
- Entity editing (update with OCC)
- Graph/link visualization
- Real-time updates (WebSocket/SSE)
- Schema diff / migration preview
- Bead lifecycle management UI

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
- No new backend work: UI consumes existing HTTP API
- Clean separation: `ui/` directory is independently buildable

**Negative**:
- Adds Bun as a build dependency (not just Rust toolchain)
- Two dev servers during development (Vite + axon-server)
- SvelteKit is newer than React — smaller ecosystem for admin UI components
- Static adapter means no SSR — initial load is blank until JS executes

**Build integration notes**:
- CI needs `bun install && bun run build` before or alongside `cargo build`
- `.gitignore` should exclude `ui/node_modules/` and `ui/build/` (but not
  `ui/.svelte-kit/` which is needed for type generation)
- The `ui/` directory is not a Cargo workspace member — it's a separate
  build artifact
