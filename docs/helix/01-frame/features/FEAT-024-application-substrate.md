---
dun:
  id: FEAT-024
  depends_on:
    - helix.prd
    - FEAT-002
    - FEAT-005
    - FEAT-011
    - FEAT-015
---
# Feature Specification: FEAT-024 - Application Substrate

**Feature ID**: FEAT-024
**Status**: Draft
**Priority**: P2
**Owner**: Core Team
**Created**: 2026-04-06
**Updated**: 2026-04-06

## Overview

Make Axon trivially deployable as the backend for lightweight
domain-specific applications. A project template or cross-cutting concern
in Helix that produces an Axon-backed app with auto-generated TypeScript
client, admin UI, and deployment configuration for Cloud Run, Cloudflare
Workers, or similar platforms.

## Problem Statement

Building a domain application (ERP, project tracker, asset manager)
currently requires assembling a database, API layer, validation, admin UI,
and deployment pipeline from scratch. Axon already provides entity storage,
schema validation, audit, and APIs — but there is no streamlined path from
"I have a schema" to "I have a running application."

## Requirements

### Functional Requirements

#### Auto-Generated TypeScript Client

- Generate a typed TypeScript client from ESF schema definitions.
- Client includes: entity types, create/update/query functions, validation
  functions matching server-side rules.
- Published as an npm package or importable module.
- Stays in sync with schema changes via code generation.

#### Auto-Generated Admin UI

- Generate a functional admin interface from ESF schema definitions.
- Entity browser: view, search, filter entities by type and properties.
- Entity editor: create and edit entities with form fields derived from
  schema.
- Relationship visualizer: see entity graph connections.
- Audit log viewer: trace changes per entity.
- Extends FEAT-011 (Admin Web UI) with schema-driven generation.

#### Deployment Templates

- Cloud Run deployment template (Docker container + service config).
- Single command to go from schema to running instance.
- Includes: Axon server, generated admin UI, health checks, observability.

#### Client-Side Validation

- TypeScript validator generated from ESF schema, usable in browser UIs.
- Same validation rules enforced server-side and client-side.
- Eliminates the pattern where validation is maintained in three places
  (database, API, UI).

### Non-Functional Requirements

- Code generation must complete in <10s for schemas with up to 50 entity
  types.
- Generated client must be tree-shakeable for minimal bundle size.
- Generated admin UI must work without additional backend beyond Axon.

### Dependencies

- FEAT-002 (Schema Engine) — schema definitions drive all generation.
- FEAT-005 (API Surface) — generated client wraps the API.
- FEAT-011 (Admin Web UI) — admin UI generation extends the base UI.
- FEAT-015 (GraphQL) — generated client can use GraphQL as transport.

## User Stories

### Story US-098: Generate a Typed Client from Schema [FEAT-024]

**As a** developer starting an Axon-backed application
**I want** a TypeScript client generated from my ESF schemas
**So that** application code gets typed entity operations and local
validation without hand-written API wrappers

**Acceptance Criteria:**
- [ ] TypeScript client generation accepts one or more ESF schemas and
  emits compilable TypeScript. Planned E2E:
  `crates/axon-codegen/tests/application_substrate_test.rs`
- [ ] Generated types include entity create, read, update, delete, query,
  and link operations for each collection. Planned E2E:
  `crates/axon-codegen/tests/application_substrate_test.rs`
- [ ] Client-side validation matches server-side validation for all
  supported constraint types. Planned E2E:
  `crates/axon-codegen/tests/application_substrate_test.rs`,
  `sdk/typescript/test/generated-client.test.ts`
- [ ] Regenerating after a schema change updates affected client types and
  leaves unaffected collections stable. Planned E2E:
  `crates/axon-codegen/tests/application_substrate_test.rs`
- [ ] The generated package can be imported by a minimal Vite/TypeScript
  app without bundler-specific hacks. Planned E2E:
  `sdk/typescript/test/generated-client.test.ts`

### Story US-099: Generate a Schema-Driven Admin App [FEAT-024]

**As a** workflow builder
**I want** a usable admin application generated from schema metadata
**So that** I can browse, edit, validate, and audit domain data without
building forms by hand

**Acceptance Criteria:**
- [ ] Generated admin UI includes collection navigation, entity browser,
  entity editor, link viewer, and audit viewer. Planned E2E:
  `ui/tests/e2e/generated-app.spec.ts`
- [ ] Required fields, optional fields, enums, arrays, and nested objects
  render as editable controls derived from schema. Planned E2E:
  `ui/tests/e2e/generated-app.spec.ts`
- [ ] Invalid form submissions are blocked client-side and rejected
  server-side with matching validation messages. Planned E2E:
  `ui/tests/e2e/generated-app.spec.ts`
- [ ] The generated UI preserves tenant/database scope in every route and
  API call. Planned E2E: `ui/tests/e2e/generated-app.spec.ts`
- [ ] Schema changes trigger UI regeneration and update visible form
  fields. Planned E2E: `ui/tests/e2e/generated-app.spec.ts`

### Story US-100: Deploy a Schema-Backed App with One Command [FEAT-024]

**As an** application developer
**I want** a template that packages Axon, generated client code, generated
UI, and deployment config
**So that** I can move from schema to running app without bespoke
infrastructure work

**Acceptance Criteria:**
- [ ] The deployment template builds a container containing `axon-server`,
  generated UI assets, and generated client artifacts. Planned E2E:
  `crates/axon-codegen/tests/application_substrate_test.rs`
- [ ] One command produces a running local instance with health check,
  default tenant/database, and generated UI. Planned E2E:
  `crates/axon-codegen/tests/application_substrate_test.rs`
- [ ] Cloud Run configuration includes health checks and persistent
  storage configuration. Planned E2E:
  `crates/axon-codegen/tests/application_substrate_test.rs`
- [ ] Re-running generation after a schema change updates client and UI
  artifacts without deleting operator-owned config. Planned E2E:
  `crates/axon-codegen/tests/application_substrate_test.rs`
- [ ] The template documents which files are generated and which are safe
  for application-specific edits. Planned E2E:
  `crates/axon-codegen/tests/application_substrate_test.rs`
