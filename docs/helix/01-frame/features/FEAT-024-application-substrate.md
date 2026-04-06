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

## Acceptance Criteria

- [ ] TypeScript client generated from ESF schema compiles and provides
      typed entity operations
- [ ] Client-side validation matches server-side validation for all
      supported constraint types
- [ ] Admin UI generated from schema provides entity browser, editor,
      and audit viewer
- [ ] Deployment template produces a running Axon instance with one
      command
- [ ] Schema changes trigger client and UI regeneration
