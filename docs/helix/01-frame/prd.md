---
ddx:
  id: helix.prd
  depends_on:
    - helix.product-vision
kind: product
---

# Product Requirements Document: Axon

**Version**: 0.3.0
**Date**: 2026-04-04
**Revised**: 2026-06-06
**Status**: Draft
**Author**: Erik LaBianca

> **Variant guidance.** This is a product PRD. Data-product variant sections
> from the shared HELIX PRD template do not apply.

## Summary

Axon is a governed transactional entity store for developers building agents
and internal workflow systems that mutate durable business records. It fills
the gap between a general-purpose database, backend-as-a-service, policy
engine, and workflow engine: Axon owns structured business state and the
guardrails around reading, mutating, approving, auditing, and repairing that
state.

The product exists because agent and human workflows now share the same
business records, but the current DIY stack requires teams to assemble schema
validation, graph/tabular querying, RBAC/ABAC, field redaction, stale-write
protection, approval routing, audit lineage, and rollback support from
separate systems. That creates policy drift, unsafe direct writes, and change
history that is often too thin to repair damage when preventive controls fail.

V1 proves the governed-agent-write value proposition: a developer defines
entity and link schemas, declares data-layer policies, exposes GraphQL and MCP
surfaces, lets agents preview and submit mutation intents, routes risky writes
for approval, rejects stale commits, and records repair-grade audit history.
The highest-signal launch metrics are time to first trusted agent write
(<1 day), 100% policy decision parity across API surfaces, and 100%
repair-grade audit coverage for entity/link mutations.

### Vision Alignment

The Product Vision defines Axon as the reusable governed state layer for
agentic business applications. The PRD translates that vision into four product
principles and keeps every P0 requirement anchored to them:

| Vision Principle | PRD Interpretation | Requirement Anchors |
|------------------|--------------------|---------------------|
| Guardrailed human-agent collaboration | Humans and agents can mutate shared records without silent data loss, stale approvals, or unclear authority boundaries | P0-3, P0-5; FR-5 through FR-8 |
| Simple structured business data | Developers model business objects and relationships once, then query them in graph-shaped or tabular form for application workflows | P0-1, P0-2; FR-1 through FR-4 |
| Reusable access and visibility control | RBAC, ABAC, relationship-aware, field-level, and transition policies are declared once and enforced below all application, agent, CLI, and SDK surfaces | P0-4, P0-7; FR-10 through FR-14, FR-20 through FR-22, FR-28 through FR-29 |
| Audit, change capture, rollback, and repair | Preventive guardrails are backed by durable lineage so operators can investigate, export, manually repair, and later automate rollback | P0-6; FR-15 through FR-19, FR-30 through FR-31 |

### Product Space

Axon is the governed business-state layer. It owns the operational entity graph
and the reusable controls around that graph: schema validation, access and
visibility policy, transaction safety, mutation preview, approval binding,
audit lineage, and change capture. It interoperates with adjacent systems
instead of replacing them.

| Adjacent Space | Axon's Boundary |
|----------------|-----------------|
| Application database | Axon stores durable operational entities and links, but remains opinionated around schema, policy, audit, and agent-safe writes rather than serving every database workload |
| Backend-as-a-service | Axon can power low-effort apps, but the core product is the governed data layer, not a complete app platform or hosted UI suite |
| Policy engine | Axon embeds reusable data-layer policy for its records, but does not try to be a universal authorization service for unrelated systems |
| Workflow engine | Axon enforces record lifecycle states, approvals, and transitions, but does not orchestrate long-running jobs or agent plans |
| Analytics/change consumers | Axon emits audit and change data, but analytical processing belongs in downstream systems |
| MCP/tool server | Axon generates policy-safe tools over governed data, so simple MCP servers do not need to reimplement schema, policy, approval, and audit logic |

### Interface Principles

Competitive products teach that developers adopt the interface before they
adopt the architecture. Axon's exposed interface therefore has to make the
governed path the easiest path for humans, applications, and agents:

| Learning | Axon Requirement |
|----------|------------------|
| The safe path must be the easy path | Generated write surfaces default to preview, intent, approval, and commit flows rather than requiring each app or MCP server to wrap direct writes safely |
| Everything should be discoverable | Schemas, relationships, policy envelopes, redactions, approval requirements, stale/conflict causes, and audit references are visible through generated GraphQL, MCP, SDK, CLI, and operator surfaces |
| Do not invent a novel write language | Developers use conventional typed SDK calls, GraphQL mutations, MCP tools, and CLI commands while Axon owns validation, policy, approval binding, and audit below them |
| Diffs should feel familiar | Mutation previews, audit inspection, and repair plans use diff/log/blame-style ergonomics that explain what changed, who or what changed it, why it was allowed or denied, and what can be repaired |
| Change streams need replay semantics | Audit and change capture expose ordered cursors so consumers can resume, replay, and scope changes by database, collection, entity/link, or transaction |
| Policy must be testable before activation | Policy authoring includes compile reports, fixture tests, dry-runs, and explanation output so RBAC, ABAC, relationship, field, and transition controls are reusable with confidence |
| GraphQL and MCP must be semantically identical | Agent tools and application APIs expose the same governed capabilities, error shapes, policy decisions, approval flows, and audit references through the shared handler path |

## Problem and Goals

### Problem

Teams building agentic business applications face four recurring failure
modes:

- **Unsafe collaboration on shared records**: humans approve, agents act, and
  applications mutate the same entities through different paths. Without one
  governed write path, stale approvals, silent overwrites, and unclear
  authority boundaries cause data loss.
- **Fragmented structured data**: teams choose between document, graph, and
  relational stores, then hand-build the missing pieces for business objects,
  relationships, lifecycle states, and graph/tabular query patterns.
- **Policy drift across low-effort surfaces**: every app, resolver, CLI, SDK,
  and MCP server reimplements role checks, attribute checks, relationship
  visibility, field redaction, and transition authorization. Drift creates
  bypasses.
- **Thin audit and poor repairability**: many systems can say a write
  happened, but cannot reconstruct actor authority, tool origin, policy
  decision, approval context, versions, and before/after state well enough to
  repair affected records.

### Goals

1. Humans and agents can collaborate on shared business records without silent
   data loss, stale approvals, or unclear authority boundaries.
2. Developers can model structured business objects and relationships once,
   then query them in graph-shaped or tabular form without assembling separate
   document, graph, relational, and indexing systems.
3. Low-effort apps, generated GraphQL resolvers, MCP tools, CLI commands, and
   SDKs inherit one reusable data-layer policy model.
4. Every mutation produces enough audit and change-capture data to investigate,
   repair, export, and later roll back affected state.
5. Axon behaves the same in embedded and server deployments, with storage
   backend differences hidden behind the product API.

### Success Metrics

| Metric | Target | Measurement Method |
|--------|--------|--------------------|
| Time to first trusted agent write | <1 day for a competent developer | Invoice/procurement tutorial from schema to audited GraphQL and MCP write |
| Policy decision parity | 100% identical allow, deny, redaction, and approval decisions across handler API, GraphQL, MCP, CLI, and SDK paths | Shared policy fixture suite |
| Repair-grade audit coverage | 100% of entity/link mutations include actor, authority, tool/API origin, policy decision, approval decision, versions, and before/after state | Audit schema contract tests and rollback-fixture review |
| Approval safety | 100% stale intent rejection for changed pre-image, schema, policy, grant, or operation hash | Mutation intent contract tests |
| Schema enforcement | 100% of production collection writes validated against active schema | Schema validation pass/fail telemetry |
| Single-entity latency | p99 <10 ms for read/write with validation and audit on reference hardware | Benchmark suite |
| Early adoption | 10+ serious internal or external Axon-backed projects | Integration count and GitHub adoption review |

### Non-Goals

- **Analytics engine**: Axon is OLTP. Analytical workloads consume Axon change
  data in systems such as niflheim, DuckDB, or a warehouse.
- **General-purpose database replacement**: Axon is an opinionated governed
  entity store, not a replacement for every relational, document, graph, or KV
  workload.
- **Durable workflow engine**: Temporal, Restate, Inngest, DBOS, and LangGraph
  orchestrate long-running execution. Axon governs the durable records those
  workflows read and mutate.
- **REST-first BaaS**: REST and JSON compatibility can exist, but GraphQL is
  the primary application surface and MCP is the primary agent surface.
- **Arbitrary SQL or Cypher writes**: V1 query language is read-only. Writes
  stay in schema-validated GraphQL/MCP/handler mutation flows.
- **Multi-region distributed database in V1**: V1 is a single deployment
  fronting one backing store. Distributed placement and migration are deferred.

Deferred items are tracked in `docs/helix/parking-lot.md`.

## Users and Scope

### Primary Persona: Ava, Agent Application Developer

**Role**: Software engineer building agents for procurement, invoicing,
compliance, customer operations, and internal automation.

**Goals**:
- Give agents a governed place to read, preview, and write business state.
- Avoid building schema, policy, approval, audit, and rollback plumbing for
  every application.
- Run the same data model locally during development and in production.

**Pain Points**:
- Agents can corrupt state through malformed writes or stale assumptions.
- Policy and redaction behavior differs between app code, APIs, and tools.
- Approval flows are custom, stale-prone, and hard to bind to a reviewed
  pre-image.
- Audit logs often cannot explain what an agent saw, why a write was allowed,
  or how to repair a bad write.

### Secondary Persona: Wei, Business Workflow Builder

**Role**: Developer or technical lead building internal business tools such as
approval workflows, document review, time tracking, and ERP-like systems.

**Goals**:
- Model business records as entities with relationships and lifecycle states.
- Build low-effort UIs and tools without weakening access controls.
- Preserve compliance-grade history for review and repair.

**Pain Points**:
- Business state lives in spreadsheets, email threads, SaaS tools, and custom
  CRUD apps with inconsistent policy and audit behavior.
- Existing BaaS platforms help ship UI quickly but do not provide reusable
  guardrails for agentic writes.

## Requirements

Each requirement traces to the product vision and is broad enough to govern
feature specs without embedding implementation design.

### Must Have (P0)

1. **Entity/link/collection model**: Axon stores schema-validated entities and
   typed links as first-class, versioned, audited business objects.
2. **Structured graph/tabular query model**: Axon exposes one policy-aware
   read model for filtering, sorting, aggregation, and graph traversal.
3. **Transactional write safety**: Axon provides ACID multi-entity/link writes,
   optimistic concurrency, no lost updates, and stale-write rejection.
4. **Reusable data-layer policy**: Axon enforces role-based, attribute-based,
   relationship-aware, field-level, and transition policies below every public
   surface.
5. **Mutation preview and approval**: Axon previews diffs, explains policy,
   routes high-risk writes for approval, and binds commit to the reviewed
   pre-image and active authority context.
6. **Repair-grade audit and change capture**: Axon records every entity/link
   mutation with enough lineage to investigate, export, manually repair, and
   later automate rollback.
7. **Safe, discoverable interface parity**: Human, UI, SDK, operator, and
   agent surfaces expose the safe path first: schema discovery, policy
   envelopes, mutation preview and approval, audit references, and identical
   GraphQL/MCP semantics.

### Should Have (P1)

1. **Schema evolution and migration**: classify breaking changes, validate
   existing data, and support safe additive evolution.
2. **Secondary indexes and query acceleration**: make common filters, sorts,
   aggregates, and traversals fast without leaking backend-specific features.
3. **Authentication, tenancy, and grants**: model tenant/database scope,
   stable users, delegated agents, credentials, and grants in every policy and
   audit decision.
4. **Rollback and recovery tooling**: provide point-in-time, entity-level, and
   transaction-level dry-run and commit flows powered by audit history.
5. **Change feeds**: publish ordered at-least-once change records for
   integrations, analytics, sync, and repair workflows.
6. **Operator UI and CLI**: let humans inspect collections, policies, intents,
   approvals, diff/log/blame-style audit, and rollback dry-run workflows
   without custom app code.

### Nice to Have (P2)

1. **Local-first sync**: support offline-capable clients with deterministic
   conflict handling.
2. **Application substrate**: generate low-effort Axon-backed apps, SDKs, and
   admin surfaces from schema.
3. **BYOC fleet control plane**: manage customer-hosted Axon deployments
   without reading tenant data.
4. **Advanced indexes and search**: add vector, full-text, and specialized
   search once the governed core is proven.
5. **Distributed placement and migration**: route databases across nodes and
   migrate placement after single-deployment semantics are stable.

## Functional Requirements

### Subsystem: Entity-Graph Data Model

- **FR-1 (P0)**: Axon MUST create, read, update, delete, list, and query
  entities with stable identity, version, metadata, and active schema
  validation.
- **FR-2 (P0)**: Axon MUST model typed directional links as first-class objects
  with schema-declared link types, metadata, versioning, and audit records.
- **FR-3 (P0)**: Axon MUST expose collection queries that cover filtering,
  sorting, pagination, aggregation, and graph traversal through one policy-aware
  read model.
- **FR-4 (P1)**: Axon SHOULD maintain portable secondary indexes for declared
  fields and compound query patterns without exposing backend-specific query
  operators.

### Subsystem: Guardrailed Transactions and Mutation Intents

- **FR-5 (P0)**: Axon MUST commit multi-entity and multi-link transactions
  atomically, or apply none of the staged operations.
- **FR-6 (P0)**: Axon MUST reject stale writes using expected entity/link
  versions and return enough current-state context for the caller to retry.
- **FR-7 (P0)**: Axon MUST preview mutation intents with a diff, affected
  records, policy decision, policy explanation, approval route, and pre-image
  bindings before risky writes commit.
- **FR-8 (P0)**: Axon MUST reject intent commits when the pre-image, schema
  version, policy version, grant version, subject binding, or operation hash no
  longer matches the preview.
- **FR-9 (P1)**: Axon SHOULD support policy-bounded rate limits, delegated
  authority, scope constraints, and semantic validation hooks after mutation
  intents are proven.

### Subsystem: Reusable Policy Enforcement

- **FR-10 (P0)**: Axon MUST let schemas declare role-based, attribute-based,
  row/entity, field, relationship, and transition policies using a closed,
  declarative grammar.
- **FR-11 (P0)**: Axon MUST enforce data-layer policies in the shared handler
  path below GraphQL, MCP, CLI, SDK, and compatibility APIs.
- **FR-12 (P0)**: Axon MUST produce the same allow, deny, redaction, and
  approval decision for the same subject, resource, and operation across every
  public surface.
- **FR-13 (P0)**: Axon MUST prevent visibility leaks through relationship
  traversal, pagination, aggregates, nullability, and count behavior.
- **FR-14 (P1)**: Axon SHOULD provide policy fixture tests, dry-runs, compile
  reports, and explanation output before policy activation.

### Subsystem: Audit, Change Capture, and Repair

- **FR-15 (P0)**: Axon MUST produce an immutable audit record for every
  entity/link mutation, including actor, delegated authority, tenant/database
  scope, tool/API origin, policy decision, approval decision, transaction ID,
  versions, operation, timestamp, and before/after state.
- **FR-16 (P0)**: Axon MUST let operators query mutation history by entity,
  link, transaction, actor, tool origin, policy decision, and approval context.
- **FR-17 (P0)**: Axon MUST retain enough audit data to reconstruct the causal
  chain of a business record and manually repair affected state.
- **FR-18 (P1)**: Axon SHOULD emit ordered change-feed records with audit
  cursors for external consumers.
- **FR-19 (P1)**: Axon SHOULD provide dry-run and commit flows for entity,
  transaction, and point-in-time rollback.

### Subsystem: API and Deployment Surfaces

- **FR-20 (P0)**: Axon MUST expose generated GraphQL reads, writes, mutation
  intents, approvals, policy explanation, audit queries, schema discovery,
  redaction metadata, approval requirements, stale/conflict causes, and audit
  references for application and operator clients.
- **FR-21 (P0)**: Axon MUST expose generated MCP tools and resources that
  mirror GraphQL schema, policy, error, mutation-intent, approval, redaction,
  stale/conflict, and audit-reference semantics for agents.
- **FR-22 (P0)**: Axon MUST route GraphQL, MCP, CLI, SDK, and internal handler
  operations through shared semantics rather than duplicating authorization or
  validation logic per surface.
- **FR-23 (P1)**: Axon SHOULD run in embedded and server modes with identical
  behavior for schema, policy, transaction, audit, and query semantics.
- **FR-24 (P1)**: Axon SHOULD provide operator-facing CLI and admin UI flows
  for schema management, policy testing, data inspection, approvals, audit, and
  repair.
- **FR-28 (P0)**: Axon MUST make the governed path the default public write
  path: generated application and agent surfaces expose preview, intent,
  approval, and commit semantics for approval-routed writes, and direct writes
  still pass the shared schema, policy, transaction, and audit handler path.
- **FR-29 (P1)**: Axon SHOULD provide boring, typed SDK calls for the core
  workflow, including `previewMutation`, `commitIntent`, `approveIntent`,
  `rejectIntent`, `explainPolicy`, `queryAudit`, and `rollbackDryRun`.
- **FR-30 (P1)**: Axon SHOULD expose diff/log/blame-style audit and repair
  views across CLI, GraphQL, SDK, and operator UI surfaces, including what
  changed, who or what changed it, why it was allowed or denied, the reviewed
  pre-image, and the available repair plan.
- **FR-31 (P1)**: Axon SHOULD expose ordered audit and change streams with
  stable cursors, replay, and resume semantics scoped by database, collection,
  entity/link, and transaction.

### Subsystem: Identity, Tenancy, and Storage Portability

- **FR-25 (P1)**: Axon SHOULD include tenant, database, user, agent,
  delegated-by, credential, grant version, and attributes in policy decisions
  and audit records.
- **FR-26 (P1)**: Axon SHOULD support SQLite/libSQL for embedded mode and
  PostgreSQL for server mode through a storage adapter that does not leak
  backend behavior into the API.
- **FR-27 (P2)**: Axon MAY add BYOC fleet management, node placement, database
  migration, and distributed routing after the single-deployment data-plane
  contract is stable.

## Acceptance Test Sketches

| Requirement | Scenario | Input | Expected Output |
|-------------|----------|-------|-----------------|
| P0-1 Entity/link/collection model | Create a valid invoice linked to a vendor, then create an invalid invoice missing `amount` | `Invoice{id, amount, status}` schema; `Vendor` schema; `INVOICED_BY` link type; one valid entity and one invalid entity | Valid entity and link commit with version and audit records; invalid write is rejected with structured schema error and no audit mutation |
| P0-2 Structured graph/tabular query model | Query pending invoices over a vendor relationship and aggregate visible totals | Read-only query matching `Vendor -> Invoice` with tenant/user subject and policy-bound fields | Only visible invoices contribute to rows and aggregates; denied fields are redacted or omitted consistently |
| P0-3 Transactional write safety | Two clients read invoice version 5 and both attempt conflicting updates | Client A updates status; Client B updates amount with expected version 5 | One write commits; the other receives a version conflict with current committed state; no lost update occurs |
| P0-4 Reusable data-layer policy | Compare the same high-value invoice update through handler API, GraphQL, MCP, and CLI | Subject `agent_id=agent-1`, delegated finance user, invoice amount changing from 9000 to 12000 | Every surface returns the same `needs_approval` decision, explanation, redactions, and approval route |
| P0-5 Mutation preview and approval | Approve a previewed intent, mutate the invoice through another transaction, then commit the old token | Preview token bound to invoice version 5; approval recorded; invoice advances to version 6 before commit | Commit rejects as stale and requires a new preview; no mutation applies from the stale intent |
| P0-6 Repair-grade audit and change capture | Update an invoice through an MCP tool and inspect audit history | MCP patch request with actor, delegated user, policy context, and idempotency scope | Audit record includes actor, delegated authority, tool/API origin, policy decision, approval state, transaction ID, versions, diff, before state, and after state |
| P0-7 Safe, discoverable interface parity | Discover and execute the same allowed low-risk invoice update through GraphQL and MCP | Same collection schema, policy catalog, caller subject, mutation payload, and generated GraphQL/MCP metadata | Both surfaces expose equivalent capabilities, policy envelopes, preview or approval requirements, stale/conflict error shapes, and audit references; both commit equivalent mutations through the shared handler path |

## Technical Context

- **Language/Runtime**: Rust Cargo workspace; current project concern records
  Rust edition 2021 and MSRV 1.75.
- **Application UI**: SvelteKit/Svelte 5 with TypeScript and Bun for the admin
  UI under `ui/`.
- **Storage**: SQLite/libSQL for embedded development and testing;
  PostgreSQL for server deployments; storage behavior mediated by adapter
  traits.
- **APIs**: Generated GraphQL, MCP tools/resources, CLI, SDK-facing handler
  APIs, and JSON/REST compatibility where needed. Writes remain schema- and
  policy-validated through preview, intent, approval, and commit flows where
  risk requires them; read-side graph/tabular queries use the read-only query
  model governed by ADR-020 and ADR-021. SDK and operator surfaces expose
  explicit `previewMutation`, `commitIntent`, `explainPolicy`, `queryAudit`,
  and `rollbackDryRun` workflows.
- **Security Model**: Tenant/database scoped identity, credential grants,
  RBAC/ABAC data policies, field redaction, denial explanations, and audit
  attribution. Current transport authentication is governed by ADR-005 and
  ADR-018.
- **Platform Targets**: Linux and macOS development; embedded and server
  deployments; BYOC production posture after the V1 data-plane contract is
  stable.

## Constraints, Assumptions, Dependencies

### Constraints

- **Technical**: No `unwrap()` in library code; clippy must pass with
  `-D warnings`; embedded and server behavior must remain API-compatible.
- **Performance**: Single-entity write latency target is p99 <10 ms with
  validation and audit enabled on reference hardware.
- **Product**: V1 must prove governed agent writes before expanding into
  advanced application substrate, fleet control plane, or distributed database
  scope.
- **Compliance**: Audit history must remain useful for investigation and
  repair while supporting redaction, crypto-shredding, and tenant-sensitive
  retention policies in later design.

### Assumptions

- Agentic applications will increasingly need to mutate real business records,
  not just draft suggestions.
- Developers will accept schema declaration when it also yields validation,
  queryability, policy reuse, GraphQL/MCP generation, and audit lineage.
- A read-only graph/tabular query language plus mutation-intent writes is
  simpler and safer for V1 than general writeable SQL or Cypher.
- SQLite/libSQL and PostgreSQL cover enough embedded and server use cases to
  prove the product before adding distributed storage.

### Dependencies

- `docs/helix/01-frame/principles.md` governs project-level quality lenses.
- `docs/helix/01-frame/concerns.md` selects active implementation concerns:
  rust-cargo, typescript-bun, security-owasp, hugo-hextra, and demo-asciinema.
- ADR-005, ADR-018, ADR-019, ADR-020, and ADR-021 govern identity, tenancy,
  policy grammar, data model, and query language.
- FEAT-001 through FEAT-031 decompose the PRD subsystems into feature-level
  behavior and acceptance criteria.

## Risks

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| Policy authoring becomes too complex for low-effort apps | Medium | High | Keep the policy grammar closed and declarative; require compile reports, fixture tests, dry-runs, and explanation output before activation |
| GraphQL, MCP, CLI, and SDK surfaces drift in policy behavior | Medium | High | Enforce policy below all surfaces in the shared handler path and gate releases on shared parity fixtures |
| Interfaces make unsafe direct writes easier than governed preview flows | Medium | High | Make preview, intent, approval, and commit the default generated write flow; require direct writes to pass shared policy and deny approval-routed writes outside the intent flow |
| Audit records are complete but too expensive | Medium | Medium | Benchmark audit overhead early, keep writes append-only, and design tiered retention/compaction separately from the V1 mutation contract |
| Entity-graph-relational modeling feels unfamiliar | Medium | Medium | Center docs and examples on invoice/procurement and bead workflows; keep schema declaration, generated APIs, and query examples concrete |
| Query and redaction behavior leaks hidden data through counts or traversal | Medium | High | Treat relationship traversal, pagination, aggregation, redaction, and count safety as P0 contract tests |
| Scope creep from workflow engine, app substrate, sync, and BYOC needs | High | High | Keep those capabilities P1/P2 until the V1 governed-agent-write proof slice is green |
| Backend abstraction leaks PostgreSQL-specific behavior | Medium | High | Run shared storage adapter conformance tests across SQLite/libSQL and PostgreSQL before claiming parity |

## Open Questions

- [ ] Should local-first sync remain P2, or move into the first post-V1 roadmap?
      This blocks sync-specific feature sequencing; ask the product owner.
- [ ] Is the BYOC fleet control plane required for first external launch, or
      only for commercialization after early adopters? This blocks control-plane
      milestone planning; ask the product owner.
- [ ] Which audit retention and erasure guarantees are required for the first
      regulated customer? This blocks compliance and storage-retention design;
      ask the first target customer and product owner.

## Success Criteria

- [ ] All P0 requirements are implemented and covered by contract, integration,
      and end-to-end tests.
- [ ] A developer completes the invoice/procurement reference workflow through
      GraphQL and MCP in less than one day.
- [ ] Handler API, GraphQL, MCP, CLI, and SDK paths enforce identical policy
      decisions for the same subject, resource, and operation.
- [ ] Generated GraphQL, MCP, CLI, and SDK surfaces make schema, policy
      envelopes, preview/approval requirements, stale/conflict causes, and
      audit references discoverable.
- [ ] Mutation preview and approval reject stale pre-image, schema, policy,
      grant, subject binding, and operation-hash changes.
- [ ] Audit records cover 100% of entity/link mutations with repair-grade
      actor, authority, tool/API origin, policy, approval, version, and
      before/after context.
- [ ] Embedded and server modes pass the same behavior suite for schema,
      policy, transaction, audit, and query semantics.
- [ ] Single-entity operations meet the p99 <10 ms target on reference
      hardware.

## Review Checklist

Use this checklist when reviewing a PRD artifact:

- [x] Summary works as a standalone 1-pager - someone can decide whether to read the rest
- [x] Problem statement describes a specific failure mode with concrete cost
- [x] Goals are outcomes, not activities
- [x] Success metrics have numeric targets and named measurement methods
- [x] Non-goals exclude things a reasonable person might assume are in scope
- [x] Personas have specific pain points, not generic descriptions
- [x] P0 requirements are necessary for launch - removing any one makes the product unusable
- [x] P1/P2 requirements are correctly prioritized relative to each other
- [x] Every P0 requirement has an acceptance test sketch
- [x] Requirements can trace upward to the Product Vision and downward to downstream artifacts
- [x] Functional requirements are testable - each can be verified with specific inputs and expected outputs
- [x] Each functional requirement carries a stable `FR-n` ID so user stories can trace to it by name
- [x] Functional requirements are grouped under canonical `### Subsystem: <name>` headings, each `FR-n` under exactly one subsystem; each subsystem is a capability that maps to roughly one feature spec
- [x] Technical context names specific versions and interfaces, not vague technology areas
- [x] Risks have concrete mitigations, not vague strategies
- [x] Open questions name who can answer and what is blocked
- [x] No contradictions between requirements sections
- [x] PRD is consistent with the governing product vision
