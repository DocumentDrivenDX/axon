---
dun:
  id: helix.feature-story-e2e-traceability
  depends_on:
    - helix.prd
    - helix.technical-requirements
    - helix.test-plan
---
# Feature Story and E2E Traceability Review

**Date**: 2026-04-19
**Status**: Active
**Scope**: FEAT-001 through FEAT-028

## Review Standard

This review treats "E2E" as the executable path that proves a user's story
arc through the appropriate product surface:

- Browser workflows use Playwright tests under `ui/tests/e2e/`, executed via
  `scripts/test-ui-e2e-docker.sh` so the browser/runtime environment is
  container-controlled even when the target Axon instance is external.
- HTTP/gRPC/GraphQL/MCP workflows use server contract tests under
  `crates/axon-server/tests/`.
- Embedded API and business workflows use integration tests under
  `crates/axon-api/tests/`.
- Storage isolation workflows use adapter tests under
  `crates/axon-storage/tests/`.
- CLI workflows use subprocess-style tests under `crates/axon-cli/tests/`.
- SDK workflows use TypeScript tests under `sdk/typescript/test/`.

A feature is not considered implemented unless each checked acceptance
criterion names at least one executable test. Future criteria may remain
unchecked, but they must name a planned test target so the missing proof is
visible.

## Fresh-Eyes Findings

The most important product-level finding is that tenant and database scope
must be present in every story that touches data. Axon's governing model is
`tenant -> database -> schema -> collection`; tests that only exercise a
bare collection name do not prove the user can operate Axon safely.

The second finding is that many feature specs describe backend capability
but omit the user recovery arc: what error the user sees, how they inspect
state after a failed write, and how they retry or recover. Acceptance
criteria should cover success, rejection, audit evidence, and cross-scope
isolation where applicable.

The third finding is that FEAT-011 is the best current model: each checked
criterion names a concrete Playwright spec, and the reverse route matrix
covers CRUD by route/tab. Other features should move toward the same shape
before their criteria are checked off.

## Feature Matrix

| Feature | Story and AC review | Executable coverage now | Remaining E2E coverage debt |
| --- | --- | --- | --- |
| FEAT-001 Collections | Stories cover create, list/inspect, drop. Missing explicit tenant/database/schema scope in CLI examples and rename behavior from ADR-010. | `crates/axon-api/tests/backend_parity.rs`; `crates/axon-server/tests/api_contract.rs`; `ui/tests/e2e/schema-editing.spec.ts`; `ui/tests/e2e/tenant-isolation.spec.ts` | CLI collection create/list/describe/drop with tenant/database flags; collection lifecycle audit assertions; same collection name in different schemas/databases. |
| FEAT-002 Schema Engine | Stories cover schema definition, validation errors, inspection. Missing schema evolution boundary and client/server validation parity. | `crates/axon-api/tests/backend_parity.rs`; `crates/axon-server/tests/api_contract.rs`; `crates/axon-server/tests/graphql_contract.rs`; `ui/tests/e2e/schema-editing.spec.ts` | Multiple validation errors in one response; actionable fix suggestions; CLI schema show; nested/default field coverage. |
| FEAT-003 Audit Log | Stories cover query, revert, metadata. Missing tenant/database isolation, collection lifecycle audit, and metadata propagation through all transports. | `crates/axon-api/tests/backend_parity.rs`; `crates/axon-server/tests/api_contract.rs`; `ui/tests/e2e/audit-route.spec.ts`; `ui/tests/e2e/wave1-capabilities.spec.ts` | Actor/time filters; audit metadata on create/update/delete; audit immutability attempts; cross-database audit isolation. |
| FEAT-004 Entity Operations | Stories cover CRUD, query, patch. Missing projection, delete semantics, unconditional write policy, and tenant/database duplicate-id isolation. | `crates/axon-api/tests/backend_parity.rs`; `crates/axon-server/tests/api_contract.rs`; `sdk/typescript/test/http-client.test.ts`; `ui/tests/e2e/entity-crud.spec.ts` | Cursor stability under concurrent writes; projection; no-op patch; delete with inbound link error; count-only query. |
| FEAT-005 API Surface | Stories cover SDK and CLI. Missing explicit HTTP path-based contract, SDK tenant/database ergonomics, and CLI/server parity. | `crates/axon-server/tests/api_contract.rs`; `crates/axon-server/tests/path_router_test.rs`; `sdk/typescript/test/http-client.test.ts` | CLI entity/audit/query commands; Go SDK; embedded/server parity from the same SDK calls; structured error fixtures across SDKs. |
| FEAT-006 Bead Storage Adapter | Stories cover bead CRUD, dependencies, ready queue. Missing integration with actual DDx bead semantics and audit evidence. | `crates/axon-api/tests/business_scenarios.rs` (`scn_006`, `scn_007`) | CLI `axon bead` commands; cycle rejection via API; dependency removal audit; ready queue through tenant/database routes. |
| FEAT-007 Entity-Graph Model | Stories cover nested entities, links, cross-graph query. Missing link metadata schema, link cardinality, and referential delete behavior. | `crates/axon-api/tests/backend_parity.rs`; `crates/axon-api/tests/business_scenarios.rs`; `crates/axon-server/tests/api_contract.rs`; `ui/tests/e2e/wave1-capabilities.spec.ts` | Duplicate link conflict; missing target error body; link metadata validation; inbound-link delete restriction. |
| FEAT-008 ACID Transactions | Stories cover atomicity, conflicts, snapshot isolation, idempotency. Missing read-set/write-skew clarity and isolation selection. | `crates/axon-server/tests/api_contract.rs`; `crates/axon-api/tests/business_scenarios.rs` | Snapshot isolation transaction read behavior; in-flight duplicate idempotency key; same key in different tenants; shared transaction id in audit for multi-op commits. |
| FEAT-009 Graph Traversal Queries | Stories cover dependency traversal, BOM, reachability. Some duplicate ACs should be collapsed. Missing reverse traversal and deleted-target semantics in API tests. | `crates/axon-api/tests/business_scenarios.rs`; `crates/axon-server/tests/api_contract.rs` | Cycle path reporting; multi-link-type reachability; link metadata in traversal response; shared component path list. |
| FEAT-010 Workflow State Machines | Stories cover transitions, dependency guards, transition discovery. Missing tenant/database-scoped lifecycle endpoints and audit metadata assertions. | `crates/axon-server/tests/lifecycle_test.rs`; `crates/axon-server/tests/api_contract.rs`; `crates/axon-server/tests/graphql_mutations.rs`; `ui/tests/e2e/wave1-capabilities.spec.ts` | Guard traversal over links; valid-transition discovery endpoint; all failing guards in one response; lifecycle audit metadata. |
| FEAT-011 Admin Web UI | Stories and route matrix now cover tenant/database navigation, users/members/credentials, collections/entities, schemas, audit/rollback, GraphQL, links, lifecycle, markdown. | `ui/tests/e2e/*.spec.ts`; `crates/axon-server/tests/graphql_consumer_parity.rs` mirrors the native UI's GraphQL canary path at the API layer. | Actor/date audit filters and transaction-level rollback remain explicit V2 hardening items. |
| FEAT-012 Authorization | Stories cover Tailscale, RBAC, no-auth, ABAC, first-class users, JWT, multi-tenant users. Missing checked test references in the feature doc. | `crates/axon-server/tests/auth_pipeline_test.rs`; `crates/axon-server/tests/auth_pipeline_integration_test.rs`; `crates/axon-server/tests/control_credentials_test.rs`; `crates/axon-server/tests/control_users_provision_test.rs`; `crates/axon-storage/tests/tenant_users_test.rs`; `sdk/typescript/test/auth-error-manifest.test.ts`; `ui/tests/e2e/tenant-admin.spec.ts` | Field masking; immutable field policy; per-principal role assignment CLI; credential grant rejection across databases in another tenant. |
| FEAT-013 Secondary Indexes | Stories cover declaration, uniqueness, compound indexes, background build. Missing user-visible planner evidence and index state transitions. | Query behavior is covered by `crates/axon-api/tests/backend_parity.rs`, but index-specific proof is not yet present. | Dedicated storage/planner E2E for indexed equality/range/sort; uniqueness conflict; background build state; writes during build. |
| FEAT-014 Multi-Tenancy | Stories cover tenants, multiple databases, schemas, bootstrap, membership, nodes. Missing checked test refs in the feature doc. | `crates/axon-server/tests/path_router_test.rs`; `crates/axon-server/tests/database_router_test.rs`; `crates/axon-server/tests/bootstrap_test.rs`; `crates/axon-server/tests/control_tenants_test.rs`; `crates/axon-server/tests/control_databases_test.rs`; `crates/axon-server/tests/scn_011_cross_tenant_isolation_test.rs`; `crates/axon-storage/tests/postgres_tenant_isolation.rs`; `ui/tests/e2e/tenant-isolation.spec.ts` | Schema namespace CRUD; tenant drop cascade; data-plane database list; node placement/migration P2 tests. |
| FEAT-015 GraphQL Query Layer | Stories cover relationships, introspection, subscriptions, mutations, admin UI. The consumer parity matrix covers GraphQL CRUD, generic and typed queries, error extensions, subscriptions, tenant/database isolation, RBAC denial, and UI canary workflow parity. | `crates/axon-server/tests/graphql_contract.rs`; `crates/axon-server/tests/graphql_mutations.rs`; `crates/axon-server/tests/graphql_subscriptions.rs`; `crates/axon-server/tests/graphql_consumer_parity.rs`; `ui/tests/e2e/wave1-capabilities.spec.ts`; `ui/tests/e2e/entity-crud.spec.ts`; `ui/tests/e2e/audit-route.spec.ts`; `ui/tests/e2e/tenant-admin.spec.ts` | Field masking and policy-aware GraphQL row filtering remain FEAT-029 coverage. |
| FEAT-016 MCP Server | Stories cover discovery, CRUD, GraphQL bridge, subscriptions, stdio. Missing stdio and GraphQL bridge depth. | `crates/axon-server/tests/mcp_contract.rs` | Stdio transport; `axon.query` GraphQL execution errors; subscription event delivery after entity mutation; dynamic tool refresh after schema changes. |
| FEAT-017 Schema Evolution | Stories cover compatibility, force, revalidate, diff. Missing full E2E except schema dry-run. | `crates/axon-server/tests/api_contract.rs` (`grpc_put_schema_dry_run`); `ui/tests/e2e/schema-editing.spec.ts` for preview-before-save | Compatibility classifier; force apply audit diff; revalidation report; schema diff between versions. |
| FEAT-018 Aggregation Queries | Stories cover count, numeric aggregates, GraphQL, MCP. GraphQL aggregate count and numeric sum are asserted in the consumer parity canary, with broader grouped aggregate coverage in the contract suite. | `crates/axon-server/tests/graphql_contract.rs`; `crates/axon-server/tests/graphql_consumer_parity.rs`; `crates/axon-server/tests/mcp_contract.rs` | HTTP aggregate route parity and additional null/missing group behavior remain V2 hardening. |
| FEAT-019 Validation Rules | Stories cover cross-field rules, gates, gate queries, actionable errors, schema-save validation. Missing most executable coverage. | Basic schema validation is covered by `crates/axon-api/tests/backend_parity.rs` and `crates/axon-server/tests/graphql_contract.rs`. | Cross-field rule engine; gate materialization; gate filters across REST/GraphQL/MCP; actionable "did you mean" errors. |
| FEAT-020 Link Discovery and Graph Queries | Stories cover candidates, neighbors, GraphQL relationships, MCP discovery. GraphQL link create/delete, relationship fields, `linkCandidates`, and `neighbors` are asserted in the consumer parity matrix. | `crates/axon-server/tests/graphql_contract.rs`; `crates/axon-server/tests/graphql_consumer_parity.rs`; `crates/axon-server/tests/api_contract.rs`; `crates/axon-server/tests/mcp_contract.rs`; `ui/tests/e2e/wave1-capabilities.spec.ts` | MCP auto-generated link discovery tools and deeper DataLoader/performance coverage remain V2 hardening. |
| FEAT-021 Change Feeds CDC | Stories cover Kafka CDC, replay, schema registry, file/SSE, link events. Missing CDC sink E2E. | `crates/axon-server/tests/api_contract.rs` covers in-process change publication with audit id. | Kafka Debezium envelope; snapshot/replay; schema registry; file and SSE sinks; link events. |
| FEAT-022 Agent Guardrails | User stories added for scoped writes, mutation throttling, and per-agent policy CRUD. Semantic hooks remain deferred. | None yet. | `crates/axon-server/tests/agent_guardrails_test.rs` for scope, rate limit, policy CRUD, and audit rejection evidence. |
| FEAT-023 Rollback and Recovery | User stories added for dry-run preview, entity rollback, transaction/time-window rollback. Entity UI coverage is checked; GraphQL audit revert and rollback API coverage now protects the UI migration path. | `crates/axon-server/tests/graphql_mutations.rs`; `ui/tests/e2e/wave2-rollback.spec.ts`; `ui/tests/e2e/audit-route.spec.ts` | Transaction rollback and point-in-time rollback remain documented REST-only break-glass exceptions until a GraphQL interface is intentionally designed. |
| FEAT-024 Application Substrate | User stories added for generated TypeScript client, generated admin app, and deployable template. | None yet. | Codegen compile tests; generated client/server validation parity; generated UI Playwright workflow; deployment template smoke. |
| FEAT-025 BYOC Control Plane | User stories added for provision/register, fleet observation without data access, deprovision/retention. Current E2E covers the shipped lifecycle slice. | `crates/axon-control-plane/tests/byoc_flow.rs`; `crates/axon-control-plane/src/service.rs` unit tests | Registration credential; aggregate tenant counts without names; 100+ deployment dashboard; retention policy enforcement; lifecycle audit. |
| FEAT-026 Markdown Templates | Stories cover define, render, schema evolution. Missing HTTP route and UI route linkage in feature doc. | `crates/axon-cli/tests/markdown.rs`; `ui/tests/e2e/wave1-capabilities.spec.ts` | HTTP template CRUD; schema warning for removed fields; audit assertions for template changes. |
| FEAT-027 Git Mirror | Stories cover enabling mirror, commits, shard strategy, resume after failure. Not implemented. | None yet. | Mirror config API/CLI; initial snapshot; create/update/delete commits; transaction commit coalescing; retry/resume. |
| FEAT-028 Unified Binary | Stories cover `axon serve`, diagnose, install, CLI mode selection, install script, config precedence. Linux installer and user-service management now have Docker-backed CI coverage. | CI and E2E launch server binaries indirectly; `scripts/test-linux-installer-service.sh` exercises the Linux install script plus `axon server install/start/status/stop/restart/uninstall` with stubbed systemd control. | `axon serve` healthz; config creation and precedence; diagnose output; real systemd/launchd daemon smoke outside stubbed CI; CLI auto-detect/server/embedded mode. |
| FEAT-029 Data-Layer Access Control Policies | Stories cover hidden rows, field redaction, denied writes, idempotent forbidden replays, and policy explanation. | None yet. | REST/GraphQL hidden-row omission; row filters before pagination; nullable redactable GraphQL fields; audit read redaction; denied write errors; nexiq reference policy set. |

## Reverse UI Route Coverage

FEAT-011 owns browser route coverage. Its reverse matrix confirms each UI
route/tab has at least one Playwright workflow for the expected CRUD-style
operation:

- Tenant and database routes: list, create, open, delete, not-found behavior,
  and verification that a newly-created non-default database can immediately
  create/read collections and entities.
- User/member/credential routes: create/provision, read, update/revoke,
  suspend/remove.
- Collection routes: create from the database Collections route, list, open
  detail, drop.
- Schema route: create for schema-first workflows, inspect structured/raw
  schema, preview and apply schema evolution.
- Entity data tab: create, read, update, delete.
- Entity history/audit/rollback tabs: read audit history, preview recovery,
  apply recovery.
- Link/lifecycle/markdown tabs: create/delete links, perform transitions,
  create/preview/delete templates.
- GraphQL route: execute introspection and render errors.

Any new UI route must add a row to FEAT-011's reverse matrix and at least
one Playwright test before the route is considered implemented.

## GraphQL Consumer Parity Matrix

`crates/axon-server/tests/graphql_consumer_parity.rs` is the durable consumer
parity suite for FEAT-015 and the GraphQL-backed native UI canary. It runs
against live in-process gateway routes and covers:

- Full consumer canary: create, query/filter/sort/count, update with OCC,
  audit/version verification, link create/delete, relationship field
  traversal, `linkCandidates`, `neighbors`, subscription delivery, delete, and
  final-state assertions.
- Bulk transaction replay: mixed create/update/link operations across
  collections, persisted postconditions, shared transaction id audit rows, and
  same-key idempotent replay semantics.
- Error and policy shape: read-role RBAC denial, schema validation field
  details, VERSION_CONFLICT current entity, INVALID_TRANSITION valid
  transitions, unsupported REST-only audit filter extensions, and GraphQL CORS
  preflight headers.
- Tenant/database isolation: same collection and entity id in separate tenants
  resolve to independent GraphQL data and audit rows.
