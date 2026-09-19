---
ddx:
  id: FEAT-011
  depends_on:
    - helix.prd
  review:
    self_hash: fbed5035703274fa3f9b134202782fa14426ccb8d77fbdd637b096a2f5bfb1a5
    deps:
      helix.prd: 6703170c71275bba7d108c4f9c329d32e4104f9c965278db888ad43cdc3ca367
    reviewed_at: "2026-07-11T04:03:36Z"
---
# Feature Specification: FEAT-011 — Admin Web UI

**Feature ID**: FEAT-011
**Status**: approved
**Priority**: P1
**Owner**: Core Team
**Covered PRD Subsystem(s)**: API and Deployment Surfaces
**Covered PRD Requirements**: FR-24 (admin-UI flows for schema management, data
inspection, audit, and repair; the policy-testing and approval flows of FR-24
are owned by FEAT-031)
**Cross-Subsystem Rationale**: None — single subsystem.
**Requirement Prefix**: UI

## Overview

The admin web UI is a browser-based console for managing and inspecting an
Axon server, implementing the operator-facing admin-UI portion of PRD FR-24.
It represents the ADR-018 control-plane hierarchy explicitly: global users,
tenants, tenant members, tenant credentials, tenant databases, and
database-scoped collections, entities, schemas, audit history, GraphQL, links,
lifecycle transitions, rollback, and markdown templates.

FEAT-031 extends this console with policy and mutation-intent operator
workflows (policy explanation, dry-run, approval inbox, intent detail,
stale-intent handling, MCP envelope visibility, and audit lineage). Those
workflows are specified separately; this feature owns the tenant/database
administration console they extend.

## Ideal Future State

An operator opens one console and can answer every routine question about an
Axon deployment without crafting a curl command: which tenants exist, who can
access them, what databases and collections they contain, what an entity looks
like right now, what it looked like before an agent touched it, and how to put
it back. Tenant and database scope is always explicit on screen, so a data
operation can never silently cross a tenant boundary. Developers iterate on
schemas, try GraphQL queries, and exercise links, lifecycles, and templates
from the same workspace they use to inspect data.

## Problem Statement

- **Current situation**: The CLI and raw HTTP/GraphQL APIs are the only ways
  to inspect or administer an Axon instance.
- **Pain points**: Developers and operators must craft curl commands or write
  code for exploratory work — scanning entity data, checking schema
  definitions, reviewing audit history, or administering users and
  credentials. This is slow, error-prone, and makes tenant scope easy to get
  wrong.
- **Desired outcome**: Every routine administration and inspection task is
  completable in the browser, with tenant/database scope explicit at every
  step.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Tenant, user, and credential administration | Who can access this deployment, and with what authority? | Tenant, database, user, member, and credential management screens |
| Collection management | What collections exist and how are they configured? | Collection list, create, describe, drop |
| Entity browsing and editing | What is in this collection right now? | Paginated entity tables, detail views, CRUD, query filters |
| Schema workspace | What shape must this data have? | Schema list, structured/raw views, preview-before-save editing |
| Audit and recovery | What changed, who changed it, and how do I undo it? | Audit browsing/filtering, entry detail, history, rollback dry-run and apply |
| Advanced database tools | How do I exercise higher-level Axon features in context? | GraphQL console, links, lifecycle transitions, markdown templates |
| Navigation and chrome | Where am I, and what scope am I operating in? | Tenant/database-scoped routing, sidebar context, not-found states |

## Requirements

### Functional Requirements by Area

#### Tenant, User, and Credential Administration

- **UI-01**. The UI must list all tenants and route into tenant-scoped
  databases, members, and credentials screens.
- **UI-02**. The UI must create tenants from the tenant list and delete
  tenants with a confirmation step.
- **UI-03**. The UI must create and delete named databases under a tenant.
- **UI-04**. The UI must provision and suspend user records and maintain
  deployment-wide user ACL rows.
- **UI-05**. The UI must add, update, and remove tenant member role
  assignments.
- **UI-06**. The UI must issue tenant credentials for a tenant member,
  showing the issued token exactly once in the issue flow; list credential
  metadata; and revoke active credentials. Credential format and issuance
  semantics are governed by ADR-018; the wire surface is defined in
  CONTRACT-001 (control-plane routes) and CONTRACT-002 (control-plane
  GraphQL).

#### Collection Management

- **UI-07**. The UI must list registered collections with entity count and
  schema version.
- **UI-08**. The UI must create a named collection with an optional entity
  schema, scoped to the current tenant/database.
- **UI-09**. The UI must show collection metadata (entity count, schema,
  timestamps).
- **UI-10**. The UI must drop a collection with a confirmation prompt.

#### Entity Browsing and Editing

- **UI-11**. The UI must present a paginated table of entities in a
  collection (50 per page) showing ID, version, and a data preview.
- **UI-12**. The UI must show entity detail with full JSON data, version,
  collection, schema version, and system metadata.
- **UI-13**. The UI must create entities through a form with ID and JSON data
  input, validating against the collection schema client-side before
  submission. Client-side validation is advisory; the server remains the
  enforcement point.
- **UI-14**. The UI must delete entities with confirmation and surface
  referential-integrity errors (inbound links) instead of silently failing.
- **UI-15**. The UI must filter entities by field=value expressions.

#### Schema Workspace

- **UI-16**. The UI must list all collections with their schema status
  (version or "no schema").
- **UI-17**. The UI must display the full collection schema in both a
  structured view and a raw JSON view.
- **UI-18**. The UI must support schema editing with a preview step before
  saving and inline display of validation errors.

#### Audit and Recovery

- **UI-19**. The UI must browse recent audit entries with operation type,
  collection, entity ID, version, and actor.
- **UI-20**. The UI must filter the audit log by collection, entity ID,
  actor, and operation type.
- **UI-21**. The UI must show audit entry detail including before/after data
  and a diff.
- **UI-22**. The UI must show per-entity history (operation, version, actor,
  timestamp, data preview) on the entity detail view.
- **UI-23**. The UI must support entity recovery: list prior versions from
  audit history, preview a rollback as a dry-run diff, and apply the rollback
  to restore a selected prior version. Reverting an audit update entry must
  restore the entity's prior data.

#### Advanced Database Tools

- **UI-24**. The UI must provide a GraphQL console with a query editor and
  response pane, supporting introspection and rendering errors for invalid
  queries (GraphQL surface per CONTRACT-002).
- **UI-25**. The UI must create and remove outbound links from the entity
  detail view.
- **UI-26**. The UI must show an entity's current lifecycle state and perform
  an allowed transition.
- **UI-27**. The UI must create, preview (through an entity), and delete
  markdown templates for a collection.

#### Navigation and Chrome

- **UI-28**. Navigation must be tenant-scoped: the UI root redirects to the
  tenant list; a tenant workspace exposes Databases, Members, and Credentials;
  a database workspace exposes Collections, Schemas, Audit Log, and GraphQL.
  Database-scoped tools are available only within an explicit tenant/database
  scope.
- **UI-29**. Unknown tenant or database routes must render a not-found state
  instead of a misleading empty console.
- **UI-30**. Two tenants containing identical database, collection, and
  entity IDs must remain fully isolated in the UI.
- **UI-31**. A contextual sidebar must show the active tenant/database
  context and route to tenant and database workspace tools. It must not
  replace workspace navigation with health telemetry.
- **UI-32**. The UI must provide extension points for the FEAT-031 Policies
  and Intents database tools using the same tenant/database-scoped navigation
  model.

### Non-Functional Requirements

- **API boundary**: The UI is the canonical Axon GraphQL consumer.
  Tenant-scoped data-plane workflows and control-plane workflows use the
  GraphQL surfaces by default; REST usage is limited to the documented
  exception classes in CONTRACT-001 (health/auth discovery, static assets,
  file/stream-oriented transports, no-scope compatibility fallbacks, and
  break-glass recovery operations).
- **Performance**: Production bundle under 200 KB gzipped; any route reaches
  interactive state in under 1 second against a local server (target,
  assumption to validate).
- **Usability**: Usable at 1024 px+ viewport width; dark color scheme
  suitable for developer tooling.
- **Accessibility**: Core workflows (navigation, forms, confirmation dialogs)
  must be keyboard-operable with visible focus states.
- **Deployment**: The UI ships as static assets served by the Axon server;
  no separate UI runtime process in production.

## User Stories

- [US-040 — Navigate the Tenant and Database Model](../user-stories/US-040-navigate-the-tenant-and-database-model.md)
- [US-041 — Administer Users, Members, and Credentials](../user-stories/US-041-administer-users-members-and-credentials.md)
- [US-042 — Manage Collections and Entities](../user-stories/US-042-manage-collections-and-entities.md)
- [US-121 — Manage Schemas Visually](../user-stories/US-121-manage-schemas-visually.md)
- [US-122 — Inspect Audit and Recover Entity State](../user-stories/US-122-inspect-audit-and-recover-entity-state.md)
- [US-045 — Use Advanced Database Tools](../user-stories/US-045-use-advanced-database-tools.md)

## Edge Cases and Error Handling

- **Schema validation errors**: When creating an entity that violates the
  schema, the UI displays the full structured error (all field violations,
  not just the first).
- **Concurrent modification**: If another client modifies an entity while the
  UI is viewing it, the next refresh shows the updated state; no real-time
  push is required.
- **Large collections**: The entity table paginates at 50 rows; no infinite
  scroll.
- **Large entity data**: The JSON viewer truncates at reasonable display
  limits with explicit "show more" expansion.
- **Referential integrity on delete**: Deleting an entity with inbound links
  shows the structured error rather than a generic failure.

## Success Metrics

- An operator can complete the full tenant → database → collection → entity
  creation flow entirely in the UI, with zero CLI or curl steps.
- Every control-plane administration task (users, members, credentials,
  tenants, databases) is achievable through the UI.
- An audit investigation — find a bad change, inspect the diff, dry-run a
  rollback, apply it — is completable without leaving the UI.

## Constraints and Assumptions

- The UI stack (framework, bundler, package manager) is fixed by ADR-006; the
  `typescript-bun` cross-cutting concern governs the runtime and quality
  gates. This spec does not restate those decisions.
- The UI is not a security boundary: authentication and authorization are
  enforced server-side (FEAT-012); client-side validation is advisory.
- Build tooling must stay fast enough for iterative development (install and
  production build each complete in seconds, not minutes).
- The console targets desktop browsers; mobile layouts are not assumed.

## Dependencies

- **Other features**: FEAT-001 (Collections), FEAT-002 (Schema Engine),
  FEAT-004 (Entity Operations), FEAT-005 (API Surface), FEAT-012
  (Authentication/Authorization — the UI inherits server-enforced auth),
  FEAT-015 (GraphQL Query Layer — the UI is its canonical consumer).
- **External services**: None at runtime. Wire surface defined in
  CONTRACT-001 (HTTP) and CONTRACT-002 (GraphQL); credential model per
  ADR-018; UI stack per ADR-006.
- **PRD requirements**: FR-24 (P1).

## Out of Scope

- Policy and mutation-intent operator workflows (policy explanation, dry-run,
  approval inbox, intent detail, MCP envelope visibility) — owned by FEAT-031.
- Graph visualization (force-directed layout of the entity-link graph).
- Bead lifecycle management UI.
- Real-time updates (WebSocket/SSE push); refresh-based consistency is
  acceptable.
- Mobile-responsive layout (the admin console is desktop-only).
- Theming / light mode (dark theme only).
