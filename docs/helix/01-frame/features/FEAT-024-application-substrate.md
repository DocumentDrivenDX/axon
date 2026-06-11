---
ddx:
  id: FEAT-024
  depends_on:
    - helix.prd
    - FEAT-002
    - FEAT-005
    - FEAT-011
    - FEAT-015
    - FEAT-028
---
# Feature Specification: FEAT-024 — Application Substrate

**Feature ID**: FEAT-024
**Status**: draft
**Priority**: P2
**Owner**: Core Team
**Covered PRD Subsystem(s)**: API and Deployment Surfaces
**Covered PRD Requirements**: PRD "Nice to Have" P2 #1 (Application
substrate) — no dedicated FR-n is allocated yet; generation builds on
FR-20 (generated GraphQL) and FR-28 (governed path as default write path)
**Cross-Subsystem Rationale**: None — single subsystem.
**FR Prefix**: SUB

## Overview

Make Axon trivially deployable as the backend for lightweight
domain-specific applications, implementing PRD P2 #1 ("Application
substrate: generate low-effort Axon-backed apps, SDKs, and admin surfaces
from schema"). From a set of ESF schema definitions, Axon generates a typed
TypeScript client, a schema-driven admin application, and a deployment
template, so a developer goes from "I have a schema" to "I have a running
application" with one command.

## Ideal Future State

A developer who has declared entity and link schemas runs one generation
command and receives: a typed client whose entity operations and validation
mirror the server exactly; a usable admin application for browsing,
editing, and auditing domain data; and a deployment template that produces
a running, health-checked instance locally or on the reference cloud
target. When the schema evolves, regeneration updates the affected
artifacts and leaves operator-owned configuration and unaffected
collections untouched. Validation logic exists in exactly one place — the
schema — and every generated surface inherits it.

## Problem Statement

- **Current situation**: Building a domain application (ERP, project
  tracker, asset manager) on Axon requires hand-assembling an API client,
  client-side validation, an admin UI, and deployment configuration, even
  though Axon already provides entity storage, schema validation, audit,
  and generated APIs.
- **Pain points**: Validation rules end up maintained in three places
  (database, API wrapper, UI); admin surfaces are rebuilt per project;
  there is no streamlined path from schema to running application.
- **Desired outcome**: Schema-to-running-app in one command, with generated
  client, UI, and deployment artifacts that stay in sync with the schema
  through regeneration.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Typed client generation | "Give my application typed entity operations and local validation" | Generate a TypeScript client with types, CRUD/query/link operations, and schema-equivalent validation |
| Admin app generation | "Let me browse, edit, and audit my domain data without building forms" | Generate a schema-driven admin application extending the base admin UI (FEAT-011) |
| Deployment template | "Get from schema to a running instance with one command" | Package server, generated UI, and client artifacts with deployment configuration for local and reference cloud targets |

## Requirements

### Functional Requirements by Area

#### Typed Client Generation

- **SUB-01**. Generation MUST accept one or more ESF schema definitions
  (CONTRACT-010) and emit a compilable TypeScript client containing entity
  types and create, read, update, delete, query, and link operations for
  each collection.
- **SUB-02**. The generated client MUST include client-side validation that
  produces the same accept/reject outcome as server-side validation for
  every supported constraint type.
- **SUB-03**. Regenerating after a schema change MUST update the affected
  client types and leave artifacts for unaffected collections byte-stable.
- **SUB-04**. The generated package MUST be importable by a standard
  TypeScript bundler setup without bundler-specific workarounds, and MUST
  be publishable as an npm package or importable module.

#### Admin App Generation

- **SUB-05**. Generation MUST produce an admin application with collection
  navigation, an entity browser (view, search, filter), an entity editor,
  a relationship/link viewer, and a per-entity audit viewer, extending the
  base admin UI (FEAT-011).
- **SUB-06**. Editor form controls MUST be derived from the schema:
  required fields, optional fields, enums, arrays, and nested objects each
  render as appropriate editable controls.
- **SUB-07**. Invalid form submissions MUST be blocked client-side and
  rejected server-side with matching validation messages.
- **SUB-08**. Every generated route and API call MUST preserve
  tenant/database scope.
- **SUB-09**. Schema changes MUST trigger UI regeneration that updates the
  visible form fields.

#### Deployment Template

- **SUB-10**. The deployment template MUST build a deployable artifact
  containing the Axon server (FEAT-028 unified binary), generated UI
  assets, and generated client artifacts.
- **SUB-11**. One command MUST produce a running local instance with a
  passing health check, a default tenant/database, and the generated UI
  served.
- **SUB-12**. The template MUST include configuration for the V1 reference
  cloud deployment target (Cloud Run — see Constraints and Assumptions),
  including health checks and persistent storage configuration.
- **SUB-13**. Re-running generation MUST update generated client and UI
  artifacts without deleting or overwriting operator-owned configuration,
  and the template MUST document which files are generated and which are
  safe for application-specific edits.

### Non-Functional Requirements

- **Performance**: code generation completes in < 10 s for schemas with up
  to 50 entity types.
- **Bundle size**: the generated client is tree-shakeable; importing one
  collection's operations does not pull in all collections.
- **Self-sufficiency**: the generated admin application requires no backend
  beyond the Axon server it was generated for.

## User Stories

- [US-098 — Generate a Typed Client from Schema](../user-stories/US-098-generate-a-typed-client-from-schema.md)
- [US-099 — Generate a Schema-Driven Admin App](../user-stories/US-099-generate-a-schema-driven-admin-app.md)
- [US-100 — Deploy a Schema-Backed App with One Command](../user-stories/US-100-deploy-a-schema-backed-app-with-one-command.md)

## Edge Cases and Error Handling

- **Schema with unsupported constraint constructs**: generation fails with
  an error naming the construct and the collection; it never silently
  emits weaker client-side validation than the server enforces.
- **Regeneration over locally modified generated files**: generated files
  are overwritten by design; the template's generated/editable boundary
  documentation is the safeguard. Operator-owned files are never touched.
- **Schema referencing collections that no longer exist**: generation
  reports the dangling reference instead of emitting a client with broken
  operations.
- **Generated app started against a server with a newer schema version**:
  client-side validation may pass while the server rejects; the server
  decision is authoritative and surfaces as a matching validation error
  (SUB-07).

## Success Metrics

- A developer goes from ESF schema to a running local app (generated
  client + admin UI + health-checked server) in under 15 minutes.
- Zero hand-written validation code in the generated client path:
  client/server validation parity holds for 100% of supported constraint
  types in the parity fixture suite.
- Regeneration after an additive schema change requires zero manual edits
  to previously generated artifacts.

## Constraints and Assumptions

- **Assumption (recorded 2026-06-10)**: Cloud Run is the V1 reference
  deployment target for the generated deployment template. Other platforms
  ("Cloudflare Workers or similar") are explicitly out of scope for V1
  (see Out of Scope).
- The PRD sequences this feature after the V1 governed-agent-write proof
  slice (PRD P2 #1 and the scope-creep risk mitigation): the governed data
  layer the generated apps inherit must be proven first.
- Generation consumes ESF schemas as defined in CONTRACT-010; it does not
  define its own schema dialect.
- Generated clients transport over the existing public surfaces (FEAT-005
  HTTP, FEAT-015 GraphQL); the substrate adds no new wire protocol.

## Dependencies

- **Other features**:
  - FEAT-002 (Schema Engine) — schema definitions drive all generation.
  - FEAT-005 (API Surface) — the generated client wraps the public API.
  - FEAT-011 (Admin Web UI) — admin app generation extends the base UI.
  - FEAT-015 (GraphQL) — generated client can use GraphQL as transport.
  - FEAT-028 (Unified Binary) — the one-command deploy and template
    packaging assume the single `axon` binary and `axon serve`.
- **External services**: container build/runtime tooling and the reference
  cloud target (Cloud Run); exact surface lives in future Contract
  artifacts when this feature is scheduled.
- **PRD requirements**: PRD P2 #1 (Application substrate); builds on FR-20
  and FR-28.

## Out of Scope

- **General application framework**: Axon is not a general-purpose app
  framework or hosted UI suite (PRD Product Space boundary). The substrate
  generates thin, schema-driven artifacts over Axon's governed surfaces;
  it does not provide routing frameworks, state management, or arbitrary
  custom UI composition.
- **Cloudflare Workers and other edge/serverless platforms**: future
  candidates; V1 templates target local runs and Cloud Run only.
- **Hand-customizable generated UI theming/branding** beyond what the base
  admin UI provides.
- **Non-TypeScript client generation** (Python, Go, etc.): the governed
  SDK surface is CONTRACT-009; substrate codegen for other languages is
  future work.
- **Production infrastructure management** (DNS, TLS issuance, secrets
  management, autoscaling policy): the template emits deployable
  configuration; operating it is the adopter's responsibility.
