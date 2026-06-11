---
ddx:
  id: FEAT-005
  depends_on:
    - helix.prd
    - FEAT-001
    - FEAT-002
    - FEAT-003
    - FEAT-004
    - FEAT-021
    - FEAT-023
    - FEAT-029
    - FEAT-030
---
# Feature Specification: FEAT-005 — API Surface

**Feature ID**: FEAT-005
**Status**: draft
**Priority**: P0
**Owner**: Core Team
**Requirement Prefix**: API
**Covered PRD Subsystem(s)**: API and Deployment Surfaces
**Covered PRD Requirements**: FR-22, FR-28, FR-29; FR-24 (CLI flows — admin web UI flows are owned by FEAT-011)
**Cross-Subsystem Rationale**: None — single subsystem. GraphQL (FR-20) is owned by FEAT-015 and MCP (FR-21) by FEAT-016; FEAT-005 owns the shared foundation those surfaces project.

## Overview

The API surface is how agents, applications, and humans interact with Axon. It
provides the shared operation foundation — one handler contract — under
collection management, entity operations, schema inspection, policy and
mutation-intent workflows, and audit queries. This feature implements PRD
FR-22 (shared semantics below every public surface), FR-28 (governed default
write path), FR-29 (typed SDK verbs for the governed workflow), and the CLI
portion of FR-24.

FEAT-005 defines the internal and compatibility API foundation. FEAT-015
defines GraphQL as the primary public application API. FEAT-016 defines MCP as
the agent-native surface. REST/JSON and gRPC remain compatibility, SDK, and
operational surfaces, not the product-defining interface.

## Ideal Future State

A developer or agent reaches every Axon capability through whichever surface
fits the moment — SDK in application code, CLI at the terminal, GraphQL or MCP
for generated clients — and observes identical behavior everywhere, because
every surface is a projection of one operation foundation. The governed path
is the easy path: schema discovery, policy envelopes, redactions,
preview/intent/approval/commit, stale/conflict causes, audit references, and
rollback dry-runs flow through one handler contract instead of being rebuilt
per surface. Errors are structured and stable enough for a program to match
on, and the CLI transparently uses a running server when one exists or falls
back to embedded mode when it does not.

## Problem Statement

- **Current situation**: Agents and applications that mutate durable business
  records must call database APIs that are either too low-level (SQL), too
  unstructured (ad-hoc JSON), too endpoint-centric for graph-shaped data, or
  too vendor-specific (proprietary BaaS SDKs).
- **Pain points**: Each surface a team adds (API, CLI, SDK, tool server)
  re-implements validation, authorization, error shapes, and audit hooks,
  which drifts. Developers cannot rely on identical semantics between local
  development and server deployments.
- **Desired outcome**: One structured operation foundation that GraphQL, MCP,
  SDKs, CLI tools, and compatibility routes share, so policy decisions, error
  semantics, and governed-write behavior are identical across every surface.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Operation foundation | "Is behavior identical no matter how I call Axon?" | Canonical handler boundary and typed operations below all surfaces |
| Governed-write and discoverability contract | "How do I write safely, and how do I know what is allowed?" | Preview/intent/approval/commit semantics and public metadata exposed by every surface |
| SDK surface | "How does my application code call Axon?" | First-party typed SDK verbs for the governed workflow |
| CLI surface | "How do I inspect and manage Axon from a terminal?" | CLI equivalents for every operation, output formats, client/embedded mode selection |
| Compatibility protocols and error model | "Can plain HTTP clients and integrations talk to Axon?" | HTTP/JSON (and optional gRPC) compatibility routes and the structured error model |

## Requirements

### Functional Requirements by Area

#### Operation Foundation

- **API-01**: Native handler traits and typed request/response structures
  must be the canonical implementation boundary below GraphQL, MCP, CLI,
  SDKs, and compatibility routes. No surface duplicates authorization,
  validation, or audit logic.
- **API-02**: The operation foundation must cover all collection, entity,
  schema, and audit operations defined by FEAT-001 through FEAT-004.
- **API-03**: In embedded mode, the same operations must be available as
  native library calls with the same types and behavior as server mode.
- **API-04**: Server-streaming must be supported for change feeds and large
  query results; stream cursor semantics align with FEAT-021.

#### Governed-Write and Discoverability Contract

- **API-05**: Approval-routed writes must expose preview, intent, approval,
  and commit semantics. Direct writes must still route through shared schema,
  policy, transaction, optimistic-concurrency, and audit checks and must not
  bypass approval-required policy envelopes.
- **API-06**: Public metadata must expose schema shape, relationship shape,
  policy envelopes, redacted fields, approval requirements, stale/conflict
  causes, policy/schema versions, and audit references.
- **API-07**: GraphQL schema, MCP tool schemas, and any protobuf definitions
  must be generated views of the same Axon operation and ESF metadata — never
  independently authored surface descriptions.

#### SDK Surface

- **API-08**: First-party SDKs must expose typed verbs for the governed
  workflow: previewing mutations; committing, approving, and rejecting
  intents; explaining policy decisions; querying audit history; and
  dry-running rollbacks. The normative SDK surface (exports, method names,
  signatures, error types) is
  [CONTRACT-009](../../02-design/contracts/CONTRACT-009-sdk-surface.md).
- **API-09**: SDKs must behave identically against embedded and server modes
  and must surface structured errors that programs can match on, preserving
  policy, intent, conflict, stale-dimension, and audit-reference fields from
  the shared handler contract.

#### CLI Surface

- **API-10**: Every API operation must have a CLI equivalent, covering
  collection management, entity operations, schema operations, policy
  explanation and testing, mutation-intent workflows, audit inspection
  (including diff/blame-style views), recovery operations, and configuration.
  The normative command tree, flags, and configuration schema are
  [CONTRACT-008](../../02-design/contracts/CONTRACT-008-cli-and-config.md).
- **API-11**: The CLI must support human-readable table output by default
  plus machine-parseable JSON and YAML output, identical in embedded and
  client modes (formats per CONTRACT-008).
- **API-12**: The CLI must operate in client mode against a reachable server
  and fall back to embedded mode when no server is reachable. Client mode
  issues requests against the existing HTTP surface
  ([CONTRACT-001](../../02-design/contracts/CONTRACT-001-http-api-surface.md))
  — no new protocol. Mode selection (default server URL, connection timeout,
  fallback, and the explicit embedded/server override flags) follows the
  client-mode connection rules in CONTRACT-008.
- **API-13**: When a server is running, client mode must be the expected
  path; embedded mode serves offline and development use without a server.

#### Compatibility Protocols and Error Model

- **API-14**: HTTP/JSON compatibility routes (and optional gRPC) may be
  exposed for SDK compatibility, operational integrations, and cases where
  GraphQL is intractable. The normative route grammar, request/response
  shapes, error envelope, and status-code semantics are
  [CONTRACT-001](../../02-design/contracts/CONTRACT-001-http-api-surface.md).
- **API-15**: Errors must be structured with a stable code, message,
  field-level details, policy/intent detail where applicable, and suggested
  action. GraphQL error extensions, MCP tool errors, SDK error types, and CLI
  output must preserve these fields (envelopes per CONTRACT-001/002/003/009).
- **API-16**: The API must be versioned (v1); breaking changes require a
  version bump.

### Non-Functional Requirements

- **Latency**: Compatibility-surface network overhead < 1ms against a local
  server. GraphQL and MCP overhead targets are owned by FEAT-015 and
  FEAT-016.
- **Compatibility**: The HTTP gateway must work with any HTTP client; no SDK
  is required for basic operations.
- **Documentation**: Generated GraphQL schema, MCP tool schemas, and
  compatibility protocol definitions must include descriptions; OpenAPI may
  be generated for fallback HTTP routes.
- **Reliability**: Mode auto-detection (client vs embedded) must select the
  correct mode in 100% of the fixture matrix (server up/down × override
  flags).

## User Stories

- [US-013 — Use Axon from an Agent](../user-stories/US-013-use-axon-from-an-agent.md)
- [US-014 — Use Axon from the Command Line](../user-stories/US-014-use-axon-from-the-command-line.md)

## Edge Cases and Error Handling

- **Server unavailable**: Client SDKs return a connection error with retry
  guidance; the CLI falls back to embedded mode per the CONTRACT-008
  connection rules.
- **Invalid request**: Malformed requests return field-level structured
  errors (envelope and status semantics per CONTRACT-001).
- **Auth failure**: Missing or invalid credentials return structured
  authentication/authorization errors with a clear message (code table per
  CONTRACT-001).
- **Rate limiting**: Write-path rate limiting returns a structured
  rate-limit response with retry guidance (semantics per CONTRACT-001).
- **Large responses**: Paginated by default; streaming for very large result
  sets.

## Success Metrics

- 100% of handler operations are reachable through the CLI and at least one
  programmatic surface (SDK, GraphQL, or MCP).
- 100% identical policy decisions across handler, GraphQL, MCP, CLI, and SDK
  paths on the shared policy fixture suite (this feature's slice of the PRD
  parity metric).
- A developer completes a governed write end to end (preview → approve →
  commit) through both the SDK and the CLI using only generated metadata and
  documentation — no server source reading required.
- Zero error-shape divergences across surfaces in the shared error-model
  fixture suite.

## Constraints and Assumptions

- The CLI subcommand vocabulary uses the canonical data-model term `entity`
  (no `doc` alias): the spec stack standardizes on "entity" and a permanent
  terminology split between data model and CLI is not acceptable
  (naming recorded in CONTRACT-008).
- GraphQL is the primary application protocol and MCP the agent protocol;
  REST/gRPC compatibility surfaces never gate policy, preview, or approval
  capabilities.
- V1 is a single deployment fronting one backing store; embedded and server
  modes must stay API-compatible.

## Dependencies

- **Other features**:
  - FEAT-001 through FEAT-004 — the API exposes all their operations
  - FEAT-021 — change-feed cursor semantics inform streaming behavior
  - FEAT-023 — rollback dry-run and commit flows surface through CLI and SDK
  - FEAT-029 — policy metadata, redactions, and explanations derive from the
    compiled policy plan
  - FEAT-030 — mutation preview, approval, commit, and stale-intent behavior
    define the governed write contract
- **External services**: None. Exact surface definitions live in
  CONTRACT-001 (HTTP), CONTRACT-008 (CLI/config), and CONTRACT-009 (SDK).
- **PRD requirements**: FR-22, FR-28, FR-29 (P0/P1); FR-24 (CLI portion, P1)

## Out of Scope

- Redefining FEAT-015 GraphQL semantics or FEAT-016 MCP semantics — this
  feature owns the foundation they project, not their protocol surfaces.
- Treating REST/gRPC parity as a blocker for policy, preview, or approval
  capabilities.
- Admin dashboard / web UI (owned by FEAT-011).
- Defining exact routes, commands, flags, payloads, status codes, or SDK
  signatures — normative surface lives in CONTRACT-001, CONTRACT-008, and
  CONTRACT-009.

## Review Checklist

Use this checklist when reviewing a feature specification:

- [ ] Covered PRD Subsystem(s) and Requirements (`FR-n`) are listed; a feature spanning >1 subsystem carries an explicit cross-subsystem rationale (else split per the Decomposition test)
- [ ] Functional areas (if any) are subordinate parts of this one capability, not separate capabilities (each fails the ship/cut/metric test on its own)
- [ ] Overview connects this feature to a specific PRD requirement
- [ ] Ideal future state describes the desired user-visible outcome, not only current problems
- [ ] Problem statement describes what exists now and what is broken — not just what is wanted
- [ ] Functional areas are mapped when the feature spans multiple surfaces, workflows, or domain objects
- [ ] Requirements are grouped by functional area when a flat list would mix unrelated scopes
- [ ] Domain objects that sound similar are explicitly separated (for example, artifact instances vs artifact types)
- [ ] Every functional requirement is testable — you can write an assertion for it
- [ ] Acceptance criteria are defined in the user stories that decompose this feature, not here (ADR-009)
- [ ] Non-functional requirements have specific numeric targets, not "must be fast"
- [ ] Edge cases cover realistic failure scenarios, not just happy paths
- [ ] Success metrics are specific to this feature, not product-level metrics
- [ ] Dependencies reference real artifact IDs (FEAT-XXX, external APIs)
- [ ] Out of scope excludes things someone might reasonably assume are in scope
- [ ] No implementation details ("use X library", "create Y table") — specify WHAT not HOW
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
- [ ] Feature is consistent with governing PRD requirements
- [ ] No `[NEEDS CLARIFICATION]` markers remain unresolved for P0 features
