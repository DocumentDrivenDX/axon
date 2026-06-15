---
ddx:
  id: FEAT-016
  depends_on:
    - helix.prd
  review:
    self_hash: 9a2522adbeae59163b67207dc28717d0abc0f7ff65bdb155bd6b23d490d1ba5e
    deps:
      helix.prd: dff98156a6cc934f406611b78b513892d85cee1bd7b4c011f045146fcdfd23e1
    reviewed_at: "2026-06-15T00:35:16Z"
---
# Feature Specification: FEAT-016 — MCP Server

**Feature ID**: FEAT-016
**Status**: draft
**Priority**: P0
**Owner**: Core Team
**Requirement Prefix**: MCP
**Covered PRD Subsystem(s)**: API and Deployment Surfaces
**Covered PRD Requirements**: FR-21; contributes the MCP leg of FR-12 (decision parity), FR-28 (governed writes), and FR-31 (resume cursors)
**Cross-Subsystem Rationale**: None — single subsystem.

## Overview

Axon implements a Model Context Protocol (MCP) server that lets AI agents
discover and interact with Axon collections, entities, links, and schemas
using the standard MCP primitives: tools, resources, and prompts. Tool
definitions and resource templates are auto-generated from ESF schemas; when
a collection schema changes, the MCP surface updates automatically and
connected agents are notified. This feature implements PRD FR-21.

MCP is not a separate policy surface. It is a protocol adapter over the same
handler used by GraphQL (FEAT-015, FEAT-029, FEAT-030), so agents observe the
same row filters, field redaction, policy explanations, mutation intents, and
approval outcomes that human and UI clients see through GraphQL — and it
defines no second authorization model.

See [ADR-013](../../02-design/adr/ADR-013-mcp-server.md) for the design and
[CONTRACT-003](../../02-design/contracts/CONTRACT-003-mcp-surface.md) for the
normative tool, resource, transport, and error surface.

## Ideal Future State

Any MCP-capable agent connects to Axon — over stdio locally or HTTP remotely
— and immediately discovers what data exists, what operations are available,
and what policy allows, entirely from generated tool metadata. The safe path
is the discoverable path: tool descriptions carry policy envelopes and
approval requirements, write tools default to preview/intent/commit
semantics, and structured outcomes tell the agent exactly why a write was
allowed, routed for approval, denied, or stale. A low-effort MCP client needs
no Axon-specific guardrail code to behave safely.

## Problem Statement

- **Current situation**: AI agents (Claude, GPT, local agent frameworks) need
  a standard way to interact with structured data stores; each framework must
  write custom integration code to use Axon's GraphQL API directly.
- **Pain points**: Custom integrations duplicate discovery, validation,
  policy, and error handling per framework, and simple MCP wrappers around
  databases reimplement guardrails inconsistently or not at all.
- **Desired outcome**: Any MCP-capable agent connects and immediately
  discovers what data exists and what operations are available, with policy,
  approval, and audit semantics identical to GraphQL. Axon's value
  proposition is being agent-native; MCP is the agent-native protocol.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Tools | "What operations can I invoke, and how do I write safely?" | Core and collection-generated tools with governed-write outcomes |
| Resources | "How do I read entities, links, schemas, and audit history?" | URI-addressable read surface with discoverable templates |
| Subscriptions | "How do I react to changes without polling?" | Resource-update notifications with resumable audit cursors |
| Prompts | "How do I get guided workflows over my data?" | Generated exploration/analysis/review prompts |
| Transports | "How do I connect locally and remotely?" | Stdio and HTTP transports over the same protocol |
| Parity and metadata | "Do agents see the same rules as humans?" | Mirroring GraphQL policy, intent, error, and metadata semantics |

## Requirements

### Functional Requirements by Area

#### Tools

- **MCP-01**: A core tool set must always be present, covering GraphQL query
  execution, mutation preview and intent commit, collection management, and
  schema operations. Tool names and parameter schemas are normative in
  CONTRACT-003.
- **MCP-02**: For each registered collection, a tool set must be
  auto-generated from ESF covering create, get, update, patch, delete,
  query, aggregate, link/unlink, traverse, audit, and — when the schema
  declares lifecycles — transition. Generation rules, parameter derivation
  from ESF layers, and signatures are normative in CONTRACT-003.
- **MCP-03**: The GraphQL bridge tool must accept a GraphQL document and
  return the response with full
  [CONTRACT-002](../../02-design/contracts/CONTRACT-002-graphql-surface.md)
  semantics, giving agents relationship traversal and field selection
  through a single tool.
- **MCP-04**: Schema and policy changes must trigger tool-definition
  regeneration, and connected agents must receive a list-changed
  notification.
- **MCP-05**: Tool descriptions must include the caller-visible policy
  envelopes generated from the compiled policy plan (ADR-019), such as
  autonomous thresholds and approval requirements, and must be
  self-contained enough for an agent to use them without external
  documentation.
- **MCP-06**: Write tools must expose preview/intent/commit semantics.
  Approval-routed writes must return a needs-approval outcome with intent
  metadata instead of committing through a direct tool call (FEAT-030).
- **MCP-07**: Write tools must return structured allowed, needs-approval,
  denied, and conflict outcomes, and tool results must preserve schema
  version, policy version, stale dimension, current version,
  denied/redacted field paths, intent ID, transaction ID, and audit
  references where applicable (outcome payloads per CONTRACT-003).

#### Resources

- **MCP-08**: Entities, collection listings, outbound and inbound links,
  audit history (with cursor pagination), ESF schemas, and collection
  metadata must be readable as MCP resources. The tenant-aware four-level
  URI grammar in CONTRACT-003 is normative; ADR-013's legacy two-level
  grammar predates the tenant/database hierarchy and is superseded
  (two-level URIs remain a stdio-only default-expansion convenience).
- **MCP-09**: Resource templates must be published so agents can discover
  the URI grammar without prior configuration.

#### Resource Subscriptions

- **MCP-10**: Agents must be able to subscribe to an entity resource
  (notified on that entity's changes) and to a collection resource
  (notified on any mutation in the collection); notifications must include
  the audit cursor needed to resume through the audit resource after
  reconnect, and the agent re-reads the resource to get new state.

#### Prompts

- **MCP-11**: Guided prompts must be available for collection exploration,
  dependency analysis, audit review, and schema review (names and arguments
  per CONTRACT-003). Prompts are optional conveniences; tools and resources
  are sufficient on their own.

#### Transports

- **MCP-12**: The MCP server must be reachable over stdio for local agents
  and over HTTP (request plus server-sent notifications) for remote agents,
  speaking the same protocol with the same tool surface. Invocation,
  tenant-prefixed endpoints, and authentication alignment with the HTTP
  surface are normative in CONTRACT-003 (with
  [CONTRACT-001](../../02-design/contracts/CONTRACT-001-http-api-surface.md)
  for the auth envelope).

#### Parity and Error Model

- **MCP-13**: Tool errors must carry the same stable machine-readable codes
  as the shared error model so agents can switch programmatically, and
  version conflicts must include the current entity state (code set and
  payloads per CONTRACT-003, mirroring CONTRACT-001/CONTRACT-002).

### Non-Functional Requirements

- **Tool count**: Up to 15 tools per collection; with 20 collections,
  roughly 300 tools total must remain discoverable and usable by agent
  frameworks.
- **Discovery latency**: Tool listing responds in < 50ms for 20
  collections.
- **Tool execution latency**: MCP protocol overhead < 5ms above the
  underlying Axon operation.
- **Subscription latency**: < 500ms from entity write to agent
  notification.
- **Policy parity**: MCP tool behavior must match GraphQL policy decisions
  for the same subject, operation, and policy version.
- **Metadata parity**: MCP tool metadata must match GraphQL collection
  metadata for schema shape, policy envelopes, redaction, approval
  requirements, conflict/stale fields, and audit references.

## User Stories

- [US-052 — Agent Discovers Axon via MCP](../user-stories/US-052-agent-discovers-axon-via-mcp.md)
- [US-053 — Agent CRUDs Entities via MCP](../user-stories/US-053-agent-cruds-entities-via-mcp.md)
- [US-054 — Agent Queries via GraphQL through MCP](../user-stories/US-054-agent-queries-via-graphql-through-mcp.md)
- [US-055 — Agent Subscribes to Changes via MCP](../user-stories/US-055-agent-subscribes-to-changes-via-mcp.md)
- [US-056 — Local Agent Connects via Stdio](../user-stories/US-056-local-agent-connects-via-stdio.md)
- [US-112 — Agent Discovers Policy Envelopes](../user-stories/US-112-agent-discovers-policy-envelopes.md)

## Edge Cases and Error Handling

- **Schema change during agent session**: The agent receives a list-changed
  notification; stale tool parameters may produce validation errors until
  the agent re-fetches the tool list.
- **Large tool list**: Some agent frameworks struggle with hundreds of
  tools; tool grouping or pagination is a deferred mitigation (see Out of
  Scope).
- **Wrong tool parameters**: The server returns a structured error with
  parameter validation details, including the expected schema.
- **Concurrent tool execution**: Concurrent tool calls are independent;
  optimistic concurrency handles write conflicts.
- **Agent disconnects during subscription**: The subscription is cleaned up;
  no dangling audit-log pollers.
- **Collection dropped while agent holds tools**: The next call to a dropped
  collection's tool returns a not-found error; a list-changed notification
  is sent.
- **Version conflict on write**: The tool error includes the current entity
  state so the agent can retry with the correct version.
- **Policy change during session**: The agent receives a list-changed
  notification or a policy-version mismatch result and must re-fetch tool
  metadata.

## Success Metrics

- An MCP-capable agent completes discovery plus a governed write (preview →
  needs-approval → approved commit) using only generated tool metadata — no
  Axon-specific client code (tutorial-validated).
- 100% of MCP policy decisions and outcome payloads match GraphQL on the
  shared parity fixture suite.
- 100% of tool definitions regenerate within one notification cycle after a
  schema or policy change in fixture tests.
- Zero authorization decisions made in the MCP layer itself (all decisions
  observable as shared-handler decisions) in conformance tests.

## Constraints and Assumptions

- MCP mirrors GraphQL; it never exposes capabilities, decisions, or metadata
  that GraphQL does not.
- The stdio transport shares the local trust boundary of unauthenticated
  development mode; remote transports use the same authentication middleware
  as the HTTP surface (CONTRACT-001/CONTRACT-003).
- Agent frameworks are assumed to tolerate tool lists in the low hundreds
  for V1 deployments.

## Dependencies

- **Other features**:
  - FEAT-004 (Entity Operations) — tools delegate to entity CRUD
  - FEAT-005 (API Surface) — MCP is served by the shared handler foundation
  - [FEAT-009 (Unified Graph Query (Cypher))](FEAT-009-unified-graph-query.md)
    — traverse/query tools and named-query tool generation use the unified
    planner
  - FEAT-013 (Secondary Indexes) — query tools route through the index-aware
    planner
  - FEAT-015 (GraphQL) — the GraphQL bridge tool and the semantics MCP
    mirrors
  - FEAT-029 (Access Control) — row filters, field redaction, and policy
    explanation
  - FEAT-030 (Mutation Intents and Approval) — preview, approval, and intent
    commit outcomes
- **External services**: MCP protocol specification. Normative surface lives
  in CONTRACT-003; ADR-013 records the design.
- **PRD requirements**: FR-21 (P0); contributes to FR-12, FR-28, FR-31

## Out of Scope

- **MCP sampling**: Axon requesting LLM completions (not needed).
- **Per-agent tool hiding as a security boundary**: Tool metadata may hide
  or annotate unavailable operations for ergonomics, but enforcement always
  happens in shared-handler execution via FEAT-029.
- **MCP tool annotations**: Read-only vs destructive hints (protocol still
  evolving).
- **Tool grouping / namespacing**: For large deployments with many
  collections (deferred).
- **Streaming tool responses**: Incremental streaming of large result sets
  (deferred pending protocol support).

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
