---
dun:
  id: FEAT-011
  depends_on:
    - helix.prd
    - FEAT-001
    - FEAT-002
    - FEAT-004
    - FEAT-005
---
# Feature Specification: FEAT-011 - Admin Web UI

**Feature ID**: FEAT-011
**Status**: Implemented
**Priority**: P1
**Owner**: Core Team
**Created**: 2026-04-05
**Updated**: 2026-04-11

## Overview

The admin web UI is a browser-based console for managing and inspecting an
Axon server. It provides a visual interface for the operations currently
available via CLI and API: browsing collections, inspecting entities,
viewing and editing schemas, and reading the audit log.

The UI is a SvelteKit application built with Bun, served as static files
by the axon-server HTTP gateway (see ADR-006). The cross-cutting concern
for the UI surface is `typescript-bun`, with ADR-006 providing the
SvelteKit and Vite-specific overrides.

## Problem Statement

Developers and operators need a quick way to inspect what's in their Axon
instance without crafting curl commands or writing code. The CLI works for
scripting and automation, but a visual interface is faster for exploratory
work: scanning entity data, checking schema definitions, reviewing audit
history.

## Requirements

### Functional Requirements

#### Collections

- **List collections**: Display all registered collections with entity count
  and schema version
- **Create collection**: Form to create a named collection with an optional
  JSON Schema for entity validation
- **Describe collection**: Show collection metadata (entity count, schema,
  timestamps)
- **Drop collection**: Delete a collection with confirmation prompt

#### Entities

- **Browse entities**: Table view of entities in a collection with ID,
  version, and data preview. Paginated (50 per page)
- **View entity detail**: Full JSON view of entity data, version, and
  system metadata
- **Create entity**: Form with ID and JSON data input. Validates against
  schema client-side before submission
- **Delete entity**: Delete with confirmation. Respects referential
  integrity (shows error if inbound links exist)
- **Query entities**: Filter entities by field=value expressions

#### Schemas

- **List schemas**: Show all collections with their schema status (version
  or "no schema")
- **View schema**: Display the full CollectionSchema as formatted JSON
- **Edit schema**: JSON editor for the schema. Save via PUT to the schema
  endpoint. Show validation errors inline

#### Audit Log

- **Browse audit log**: Table of recent audit entries with operation type,
  collection, entity ID, version, and actor
- **Filter audit log**: Filter by collection, entity ID, actor, or
  operation type
- **View audit entry detail**: Full entry including before/after data and
  diff

#### Navigation and Chrome

- **Sidebar navigation**: Collections, Schemas, Audit Log sections
- **Health indicator**: Live server health status (version, uptime) via
  `/health` endpoint, polled every 15 seconds
- **Dark theme**: Default dark color scheme suitable for developer tooling
- **Responsive**: Usable at 1024px+ viewport width (not mobile-optimized)

### Non-Functional Requirements

- **No separate server**: UI is served as static files by axum. No
  additional Node.js/Bun runtime in production
- **Fast builds**: `bun install` < 3s, `bun run build` < 10s
- **Small bundle**: Production build < 200KB gzipped
- **No auth in V1**: Runs as admin, full access to all collections and
  operations. Auth deferred per ADR-005
- **API reuse**: UI calls existing HTTP gateway endpoints only. No new
  backend routes required for UI functionality

## User Stories

### Story US-040: Browse Axon Data Visually [FEAT-011]

**As a** developer using Axon
**I want** a web UI to browse collections and entities
**So that** I can quickly inspect data without writing queries

**Acceptance Criteria:**
- [x] Opening `http://localhost:4170/ui` shows the collections list
- [x] Clicking a collection shows its entities in a table
- [x] Clicking an entity shows its full JSON data
- [x] Empty collections show an empty state with a "Create Entity" action
- [x] Entity table paginates at 50 rows per page with next/previous navigation
- [x] "Create Entity" opens a form with entity ID and JSON data inputs
- [x] Editing an existing entity opens the JSON editor inline; Cancel restores read-only view
- [x] Dropping a collection shows a confirmation prompt; confirming removes it from the list
- [x] Deleting an entity shows a confirmation prompt; confirming removes it from the table

### Story US-041: Manage Schemas Visually [FEAT-011]

**As a** developer defining Axon schemas
**I want** a web UI to view and edit collection schemas
**So that** I can iterate on schema definitions without CLI round-trips

**Acceptance Criteria:**
- [x] Schemas page lists all collections with schema status
- [x] Clicking a collection shows its schema as formatted JSON
- [x] Edit mode opens a textarea pre-filled with the current schema JSON
- [x] Saving a schema change shows a preview/diff before commit
- [x] Saving invalid schema JSON shows an inline error with details
- [x] Cancelling an edit restores the read-only schema view
- [x] Creating a collection via the schema workspace includes a schema textarea input

### Story US-042: Inspect Audit Log Visually [FEAT-011]

**As an** operator debugging agent behavior
**I want** a web UI to browse and filter the audit log
**So that** I can trace what happened to specific entities

**Acceptance Criteria:**
- [x] Audit page shows recent entries in a table
- [x] Entries show operation type, collection, entity ID, version, and actor
- [x] Filtering by collection narrows the results
- [x] Filtering by actor narrows the results
- [x] Clearing filters restores all entries
- [x] Clicking an entry shows the full entry detail
- [ ] Audit log supports date range filtering (since/until) — deferred to V2

### Story US-043: Manage Multi-Tenant Databases [FEAT-011]

**As an** operator managing multiple Axon tenants
**I want** a Databases admin page
**So that** I can create tenants and assign databases without using the API directly

**Acceptance Criteria:**
- [x] Databases page is accessible from the sidebar
- [x] Page shows a form to create a new tenant
- [x] Created tenant appears in the tenant list with its own panel
- [x] Each tenant panel has a form to assign a named database
- [x] Assigned database appears in the tenant's database table
- [x] Each database row has a Remove button with a confirmation step

## Technical Design

### Architecture

```
┌──────────────┐     HTTP      ┌──────────────────────────────┐
│   Browser     │──────────────▶│   axon-server (axum)         │
│               │               │                              │
│  SvelteKit    │  GET /ui/*    │  ┌─────────────────────┐     │
│  (static JS)  │──────────────▶│  │  Static file server  │    │
│               │               │  │  (tower-http ServeDir)│    │
│               │  GET/POST     │  └─────────────────────┘     │
│  fetch()      │──────────────▶│  ┌─────────────────────┐     │
│               │  /collections │  │  HTTP Gateway (axum)  │    │
│               │  /entities    │  │  (existing routes)    │    │
│               │  /audit       │  └─────────────────────┘     │
└──────────────┘               └──────────────────────────────┘
```

### Stack (per ADR-006)

- **SvelteKit** with `adapter-static` — file-based routing, compiled Svelte
  components, TypeScript
- **Bun** — package manager, script runner, Vite runtime
- **Vite** — bundler, dev server with API proxy
- **Concern package**: `typescript-bun` governs the runtime, package manager,
  and quality gates; ADR-006 fixes the framework/bundler choice to
  SvelteKit + Vite for `ui/`

### Key Implementation Details

- **API client** (`lib/api.ts`): Typed fetch wrapper with error handling.
  All API calls go to the same origin (no CORS issues in production). In
  dev, Vite proxies to the axon-server port.
- **Error handling**: API errors (4xx/5xx) are parsed as `{code, detail}`
  JSON and displayed as toast notifications.
- **Routing**: SvelteKit file-based routes. `/ui` prefix in production via
  axum's `nest_service`.
- **State**: Svelte 5 runes (`$state`, `$derived`) for reactive state. No
  external state management library.

### Server Integration

The axon-server binary gains a `--ui-dir` flag:

```
axon-server --http-port 4170 --no-auth --storage memory --ui-dir ./ui/build
```

When `--ui-dir` is provided, axum serves static files from that directory
under the `/ui` path prefix. When omitted, the UI routes are not registered
(headless mode).

## Edge Cases

- **Schema validation errors**: When creating an entity that violates the
  schema, the UI displays the full structured error (all field violations,
  not just the first)
- **Concurrent modification**: If another client modifies an entity while
  the UI is viewing it, the next refresh shows the updated state. No
  real-time push in V1
- **Large collections**: Entity table paginates at 50 rows. No infinite
  scroll in V1
- **Large entity data**: JSON viewer truncates at reasonable display limits
  with "show more" expansion

## Dependencies

- **FEAT-001** (Collections): UI browses and manages collections
- **FEAT-002** (Schema Engine): UI views and edits schemas
- **FEAT-004** (Entity Operations): UI CRUDs entities
- **FEAT-005** (API Surface): UI calls HTTP gateway endpoints
- **ADR-006**: Technology choices (SvelteKit, Bun, Vite)
- **Cross-cutting concern**: `typescript-bun` with ADR-006 overrides for
  the admin UI surface
- **Bun runtime**: Required at build time, not at runtime

## Out of Scope

- Link management (create/delete/traverse links — V2)
- Graph visualization (force-directed layout of entity-link graph — V2)
- Bead lifecycle management (bead-specific UI — V2)
- Real-time updates (WebSocket/SSE push — V2)
- Schema diff / migration preview (V2, depends on schema evolution)
- Mobile-responsive layout (admin console is desktop-only)
- Theming / light mode (dark theme only in V1)

## Traceability

### Related Artifacts
- **Parent PRD Section**: Requirements Overview > P1 #8 (Admin web UI)
- **User Stories**: US-040, US-041, US-042
- **Architecture**: ADR-006 (SvelteKit + Bun + Vite)
- **Cross-cutting concern**: `typescript-bun` in `concerns.md`, scoped to
  `area:ui` beads with ADR-006 overrides
- **Implementation**: `ui/` at project root

### Feature Dependencies
- **Depends On**: FEAT-001, FEAT-002, FEAT-004, FEAT-005
- **Depended By**: None (leaf feature)

### Playwright E2E Test Coverage

All 78 tests pass against a live `axon-server --no-auth --storage memory --ui-dir ui/build --http-port 4170`.

| Test file | Description | Stories covered |
|-----------|-------------|-----------------|
| `tests/e2e/health.spec.ts` | Root redirect, health panel, sidebar nav links | US-040 |
| `tests/e2e/collections.spec.ts` | Create collection via UI, verify in schemas list and collections table | US-040 |
| `tests/e2e/entities.spec.ts` | Full entity CRUD: create, browse, detail, edit/cancel, pagination, empty state | US-040 |
| `tests/e2e/schemas.spec.ts` | Schema detail, edit, invalid JSON error, preview before save, create collection form | US-041 |
| `tests/e2e/audit.spec.ts` | Audit table, filter by collection/actor, clear filters, entry detail | US-042 |
| `tests/e2e/drop-delete.spec.ts` | Drop collection confirm/cancel, delete entity confirm/cancel | US-040 |
| `tests/e2e/databases.spec.ts` | Databases page, create tenant, assign/remove database | US-043 |
| `tests/collections.spec.ts` | Collections browser with mocked API: table, headers, pagination | US-040 |
| `tests/schemas.spec.ts` | Schemas page with mocked API: list, detail, edit textarea | US-041 |
| `tests/navigation.spec.ts` | Sidebar nav, routing, health panel visibility | US-040 |

**Key implementation notes captured in tests:**
- `fullyParallel: true` is set globally; inter-dependent tests use `test.describe.configure({ mode: 'serial' })`.
- In-memory server state persists within a process run; `beforeAll` API calls use `[201, 409]` to be idempotent.
- Entity `startEdit()` uses JSON round-trip clone (`JSON.parse(JSON.stringify(...))`) instead of `structuredClone` to avoid `DataCloneError` on Svelte 5 deep-reactive proxies.
