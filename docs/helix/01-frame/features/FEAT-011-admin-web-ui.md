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
**Updated**: 2026-04-19

## Overview

The admin web UI is a browser-based console for managing and inspecting an
Axon server. It represents the ADR-018 control-plane hierarchy explicitly:
global users, tenants, tenant members, tenant credentials, tenant databases,
and database-scoped collections, entities, schemas, audit history, GraphQL,
links, lifecycle transitions, rollback, and markdown templates.

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

#### Tenants, Users, and Databases

- **List tenants**: Display all tenants and route into tenant-specific
  databases, members, and credentials
- **Create/delete tenants**: Create tenants from the tenant list and remove
  tenants with confirmation
- **Manage databases**: Create and delete named databases under a tenant
- **Manage users**: Provision users and maintain deployment-wide user ACL rows
- **Manage tenant members**: Add, update, and remove tenant role assignments
- **Manage tenant credentials**: Issue one-time JWT credentials, list
  credential metadata, and revoke active credentials

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

- **Tenant-scoped navigation**: `/ui/tenants/:tenant` is the root for tenant
  management; `/ui/tenants/:tenant/databases/:database` is the root for
  database-scoped tools
- **Database sub-navigation**: Collections, Schemas, Audit Log, and GraphQL
  sections are available only within an explicit tenant/database scope
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

### Story US-040: Navigate the Tenant and Database Model [FEAT-011]

**As an** operator managing Axon
**I want** the UI to make tenant and database scope explicit
**So that** data operations cannot accidentally cross tenant boundaries

**Acceptance Criteria:**
- [x] `/ui/` redirects to `/ui/tenants`, and top navigation exposes only tenant and user control-plane roots. E2E: `smoke-restructure.spec.ts`
- [x] Creating a tenant routes to `/ui/tenants/:tenant`, and tenant sub-navigation exposes Databases, Members, and Credentials. E2E: `smoke-restructure.spec.ts`
- [x] Creating a database routes to `/ui/tenants/:tenant/databases/:database`, and database sub-navigation exposes Collections, Schemas, Audit Log, and GraphQL. E2E: `smoke-restructure.spec.ts`
- [x] Unknown tenant routes render the not-found state instead of a misleading empty console. E2E: `smoke-restructure.spec.ts`
- [x] Two tenants can contain the same database, collection, and entity IDs while remaining isolated in the UI. E2E: `tenant-isolation.spec.ts`

### Story US-041: Administer Users, Members, and Credentials [FEAT-011]

**As an** operator
**I want** user and tenant access controls in the UI
**So that** I can administer access without direct API calls

**Acceptance Criteria:**
- [x] The Users route supports adding, changing, and removing deployment-wide ACL rows. E2E: `tenant-admin.spec.ts`
- [x] The Users route supports provisioning and suspending user records. E2E: `tenant-admin.spec.ts`
- [x] The tenant Members route supports adding a provisioned user, changing the tenant role, and removing the member. E2E: `tenant-admin.spec.ts`
- [x] The tenant Credentials route issues a one-time JWT for a tenant member and shows the JWT exactly in the issue flow. E2E: `tenant-admin.spec.ts`
- [x] Issued credential metadata appears in the credentials table and can be revoked. E2E: `tenant-admin.spec.ts`

### Story US-042: Manage Collections and Entities [FEAT-011]

**As a** developer using Axon
**I want** tenant-scoped collection and entity CRUD
**So that** I can inspect and repair data without writing curl commands

**Acceptance Criteria:**
- [x] The database Collections route lists registered collections and links to each collection detail route. E2E: `tenant-isolation.spec.ts`
- [x] A collection can be created through the schema workspace, appears in the collections route, and can be dropped with confirmation. E2E: `schema-editing.spec.ts`
- [x] The collection detail route supports entity create, read, update, and delete from the UI. E2E: `entity-crud.spec.ts`
- [x] The entity detail route shows version, collection, schema version, and JSON data. E2E: `entity-crud.spec.ts`, `wave1-capabilities.spec.ts`
- [x] The entity History tab shows audit versions for the selected entity. E2E: `wave1-capabilities.spec.ts`

### Story US-043: Manage Schemas Visually [FEAT-011]

**As a** developer defining Axon schemas
**I want** a database-scoped schema workspace
**So that** I can iterate on collection definitions without CLI round-trips

**Acceptance Criteria:**
- [x] The Schemas route lists registered collections and opens a structured schema view. E2E: `schema-editing.spec.ts`
- [x] The raw JSON view displays the full collection schema payload. E2E: `schema-editing.spec.ts`
- [x] Creating a collection accepts entity schema JSON and registers the collection in the current tenant/database scope. E2E: `schema-editing.spec.ts`
- [x] Editing a schema requires previewing the change before saving. E2E: `schema-editing.spec.ts`

### Story US-044: Inspect Audit and Recover Entity State [FEAT-011]

**As an** operator debugging agent behavior
**I want** audit history and rollback tools in the entity UI
**So that** I can trace and recover unintended changes

**Acceptance Criteria:**
- [x] The database Audit Log route is reachable from the tenant/database sub-navigation. E2E: `smoke-restructure.spec.ts`
- [x] The database Audit Log route filters by collection and opens entry detail. E2E: `audit-route.spec.ts`
- [x] Reverting an audit update entry restores the entity's prior data. E2E: `audit-route.spec.ts`
- [x] The selected entity History tab shows operation, version, actor, timestamp, and data preview. E2E: `wave1-capabilities.spec.ts`
- [x] The entity Rollback tab lists prior versions from audit history. E2E: `wave2-rollback.spec.ts`
- [x] Rollback preview performs a dry-run diff before mutation. E2E: `wave2-rollback.spec.ts`
- [x] Applying rollback mutates the entity to the selected prior version. E2E: `wave2-rollback.spec.ts`

### Story US-045: Use Advanced Database Tools [FEAT-011]

**As a** developer or operator
**I want** GraphQL, links, lifecycle transitions, and markdown templates exposed in context
**So that** I can exercise higher-level Axon features from the same database workspace

**Acceptance Criteria:**
- [x] The GraphQL route loads a query editor and response pane. E2E: `wave1-capabilities.spec.ts`
- [x] GraphQL introspection returns a schema, and invalid queries render errors. E2E: `wave1-capabilities.spec.ts`
- [x] The entity Links tab creates and removes outbound links. E2E: `wave1-capabilities.spec.ts`
- [x] The entity Lifecycle tab shows current state and performs an allowed transition. E2E: `wave1-capabilities.spec.ts`
- [x] The collection Markdown Template section creates, previews through an entity tab, and deletes a template. E2E: `wave1-capabilities.spec.ts`

### Reverse Route Coverage

| Route or tab | Expected workflows | E2E coverage |
| --- | --- | --- |
| `/ui/` | Redirect to tenant root | `smoke-restructure.spec.ts` |
| `/ui/tenants` | List, create, open tenant; top nav shape | `smoke-restructure.spec.ts`, `tenant-isolation.spec.ts` |
| `/ui/tenants/:tenant` | Read tenant, create/delete database | `smoke-restructure.spec.ts`, `tenant-admin.spec.ts` |
| `/ui/tenants/:tenant/members` | Create, read, update, delete tenant member | `tenant-admin.spec.ts` |
| `/ui/tenants/:tenant/credentials` | Issue, read, revoke credential | `tenant-admin.spec.ts` |
| `/ui/users` | Provision/suspend user; create, read, update, delete ACL user row | `tenant-admin.spec.ts` |
| `/ui/tenants/:tenant/databases/:database` | Read database overview and section links | `tenant-isolation.spec.ts` |
| `/collections` | Read collections, route to detail, drop collection | `schema-editing.spec.ts`, `tenant-isolation.spec.ts` |
| `/collections/:name` Data tab | Create, read, update, delete entity | `entity-crud.spec.ts` |
| `/collections/:name` History tab | Read entity audit history | `wave1-capabilities.spec.ts` |
| `/collections/:name` Links tab | Create and delete outbound links | `wave1-capabilities.spec.ts` |
| `/collections/:name` Lifecycle tab | Read state and update by transition | `wave1-capabilities.spec.ts` |
| `/collections/:name` Markdown tab | Render saved collection template for entity data | `wave1-capabilities.spec.ts` |
| `/collections/:name` Rollback tab | Read history, preview rollback, apply rollback | `wave2-rollback.spec.ts` |
| `/schemas` | Create collection, read structured/raw schema, preview and update schema | `schema-editing.spec.ts` |
| `/audit` | Route reachability, collection filter, entry detail, revert update entry | `smoke-restructure.spec.ts`, `audit-route.spec.ts` |
| `/graphql` | Read console, execute introspection, handle invalid query | `wave1-capabilities.spec.ts` |

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

- Graph visualization (force-directed layout of entity-link graph — V2)
- Bead lifecycle management (bead-specific UI — V2)
- Real-time updates (WebSocket/SSE push — V2)
- Audit actor/date filter E2E and transaction-level rollback UI coverage (V2 hardening)
- Mobile-responsive layout (admin console is desktop-only)
- Theming / light mode (dark theme only in V1)

## Traceability

### Related Artifacts
- **Parent PRD Section**: Requirements Overview > P1 #8 (Admin web UI)
- **User Stories**: US-040, US-041, US-042, US-043, US-044, US-045
- **Architecture**: ADR-006 (SvelteKit + Bun + Vite)
- **Cross-cutting concern**: `typescript-bun` in `concerns.md`, scoped to
  `area:ui` beads with ADR-006 overrides
- **Implementation**: `ui/` at project root
- **E2E coverage**: `ui/tests/e2e/*.spec.ts`, run with
  `AXON_E2E_PORT=4171 bun run test:e2e:real`

### Feature Dependencies
- **Depends On**: FEAT-001, FEAT-002, FEAT-004, FEAT-005
- **Depended By**: None (leaf feature)

### Playwright E2E Test Coverage

The UI E2E suite runs against a live `axon-server --no-auth --storage
memory --ui-dir ui/build` using
`AXON_E2E_PORT=4171 bun run test:e2e:real`.

| Test file | Description | Stories covered |
|-----------|-------------|-----------------|
| `ui/tests/e2e/smoke-restructure.spec.ts` | Tenant root redirect, tenant/database navigation, unknown tenant state, database tool routes | US-040, US-044 |
| `ui/tests/e2e/tenant-isolation.spec.ts` | Cross-tenant UI isolation for same database, collection, and entity ids | US-040, US-042 |
| `ui/tests/e2e/tenant-admin.spec.ts` | Users, tenant members, credentials, credential revocation | US-041 |
| `ui/tests/e2e/schema-editing.spec.ts` | Schema route, create/drop collection, raw schema, preview-before-save | US-042, US-043 |
| `ui/tests/e2e/entity-crud.spec.ts` | Entity create, read, update, delete through collection detail | US-042 |
| `ui/tests/e2e/audit-route.spec.ts` | Database audit filtering, audit entry detail, revert update entry | US-044 |
| `ui/tests/e2e/wave1-capabilities.spec.ts` | GraphQL console, links tab, lifecycle tab, history tab, markdown template tab | US-042, US-044, US-045 |
| `ui/tests/e2e/wave2-rollback.spec.ts` | Entity rollback history, dry-run preview, apply rollback | US-044 |

**Key implementation notes captured in tests:**
- `fullyParallel: true` is set globally; inter-dependent tests use `test.describe.configure({ mode: 'serial' })`.
- In-memory server state persists within a process run; `beforeAll` API calls use `[201, 409]` to be idempotent.
- Entity `startEdit()` uses JSON round-trip clone (`JSON.parse(JSON.stringify(...))`) instead of `structuredClone` to avoid `DataCloneError` on Svelte 5 deep-reactive proxies.
