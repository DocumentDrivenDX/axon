---
ddx:
  id: FEAT-016
  depends_on:
    - helix.prd
    - FEAT-004
    - FEAT-005
    - FEAT-009
    - FEAT-013
    - FEAT-015
    - ADR-013
---
# Feature Specification: FEAT-016 - MCP Server

**Feature ID**: FEAT-016
**Status**: Draft
**Priority**: P0
**Owner**: Core Team
**Created**: 2026-04-05
**Updated**: 2026-04-22

## Overview

Axon implements a Model Context Protocol (MCP) server that allows AI
agents to discover and interact with Axon collections, entities, links,
and schemas using the standard MCP primitives: tools, resources, and
prompts.

Tool definitions and resource templates are auto-generated from ESF
schemas. When a collection schema changes, the MCP tool surface updates
automatically and connected agents are notified.

MCP is not a separate policy surface. It mirrors the GraphQL semantics from
FEAT-015, FEAT-029, and FEAT-030 so agents observe the same row filters, field
redaction, policy explanations, mutation intents, and approval outcomes that
human and UI clients see through GraphQL.

See [ADR-013](../../02-design/adr/ADR-013-mcp-server.md) for the full
design.

## Problem Statement

AI agents (Claude, GPT, local agent frameworks) need a standard way to
interact with structured data stores. Today, each agent framework must write
custom integration code to use Axon's GraphQL API directly. MCP eliminates
this: any MCP-capable agent connects and immediately discovers what data exists
and what operations are available.

Axon's value proposition is being agent-native. MCP is the agent-native
protocol.

## Requirements

### Functional Requirements

#### Tools (Write + Query Operations)

- **Core tools**: Always-present tools for collection management, schema
  operations, and GraphQL queries (`axon.query`, `axon.collection.create`,
  `axon.collection.list`, `axon.collection.drop`, `axon.schema.get`,
  `axon.schema.put`)
- **Collection-specific tools**: Auto-generated CRUD, link, traverse,
  and audit tools per registered collection (e.g., `beads.create`,
  `beads.patch`, `beads.query`, `beads.link`, `beads.traverse`,
  `beads.audit`)
- **Lifecycle tools**: When a collection has lifecycle declarations,
  a transition tool is generated (e.g., `beads.transition`) with valid
  target states derived from the lifecycle definition
- **GraphQL bridge**: `axon.query` tool accepts a GraphQL query string
  and returns the response, giving agents full relationship traversal
  and field selection through a single tool
- **Tool regeneration**: Schema changes trigger tool definition
  regeneration. Connected agents receive `tools/list_changed`
  notification
- **Policy envelopes**: Tool descriptions include the caller-visible policy
  envelopes generated from ADR-019, such as autonomous thresholds and approval
  requirements
- **Intent outcomes**: Write tools return structured `allowed`,
  `needs_approval`, `denied`, and `conflict` results; approval-routed writes
  use FEAT-030 mutation intents

#### Resources (Read Operations)

- **Entity resources**: `axon://{collection}/{id}` returns entity data
- **Collection resources**: `axon://{collection}` returns paginated
  entity listing
- **Link resources**: `axon://{collection}/{id}/links` returns outbound
  links
- **Audit resources**: `axon://{collection}/{id}/audit` returns audit
  history
- **Schema resources**: `axon://_schemas/{collection}` returns ESF schema
- **Resource templates**: Agents discover URI patterns via MCP resource
  template listing

#### Resource Subscriptions

- **Entity subscriptions**: Agent subscribes to `axon://{collection}/{id}`
  and receives `resource_updated` notifications when the entity changes
- **Collection subscriptions**: Agent subscribes to `axon://{collection}`
  and receives notifications on any mutation in the collection
- **Backed by audit log**: Same polling mechanism as GraphQL
  subscriptions (ADR-012)

#### Prompts

- **axon.explore_collection**: Guided exploration of a collection's
  schema, sample data, and purpose
- **axon.dependency_analysis**: Traverse and analyze an entity's
  dependency graph
- **axon.audit_review**: Summarize recent changes to an entity or
  collection
- **axon.schema_review**: Evaluate schema quality and suggest
  improvements

#### Transport

- **Stdio**: `axon-server --mcp-stdio` for local agents (Claude Code,
  local frameworks). MCP over stdin/stdout
- **HTTP+SSE**: `/mcp` (POST) and `/mcp/sse` (GET) for remote agents.
  Served by axum alongside GraphQL and compatibility routes

### Non-Functional Requirements

- **Tool count**: Up to 15 tools per collection. With 20 collections,
  ~300 tools total. Agent frameworks must handle this gracefully
- **Discovery latency**: `tools/list` response < 50ms for 20 collections
- **Tool execution latency**: MCP protocol overhead < 5ms above the
  underlying Axon operation
- **Subscription latency**: < 500ms from entity write to agent
  notification
- **Policy parity**: MCP tool behavior must match GraphQL policy decisions for
  the same subject, operation, and policy version

## User Stories

### Story US-052: Agent Discovers Axon via MCP [FEAT-016]

**As an** AI agent connecting to Axon for the first time
**I want** to discover what collections and operations are available
**So that** I can interact with Axon without prior configuration

**Acceptance Criteria:**
- [ ] Agent calls `tools/list` and receives typed tool definitions for
  all collections
- [ ] Tool parameters include JSON Schema from the collection's ESF
- [ ] Tool descriptions are detailed enough for the agent to use them
  without documentation
- [ ] Adding a new collection makes new tools available to connected
  agents
- [ ] Each tool description includes parameter constraints and expected response shape
- [ ] Tool descriptions are self-contained: an agent can determine correct usage without external documentation

### Story US-053: Agent CRUDs Entities via MCP [FEAT-016]

**As an** AI agent managing beads
**I want** to create, read, update, and delete beads using MCP tools
**So that** I can manage work items through the standard agent protocol

**Acceptance Criteria:**
- [ ] `beads.create` creates a bead and returns it with ID and version
- [ ] `beads.get` retrieves a bead by ID
- [ ] `beads.patch` partially updates a bead with OCC
- [ ] `beads.delete` deletes a bead
- [ ] `beads.transition` changes bead status with lifecycle validation
- [ ] Errors include structured information the agent can act on
- [ ] Tool errors include structured `code` (e.g., `NOT_FOUND`, `CONFLICT`, `VALIDATION_ERROR`) and `detail` fields
- [ ] Version conflict errors include the current entity state in the error response

### Story US-054: Agent Queries via GraphQL through MCP [FEAT-016]

**As an** AI agent needing complex data
**I want** to execute GraphQL queries through an MCP tool
**So that** I can fetch entities with relationships in one request

**Acceptance Criteria:**
- [ ] `axon.query` tool accepts a GraphQL query string
- [ ] Response includes entity data with nested relationships
- [ ] GraphQL errors are surfaced as MCP tool errors
- [ ] Agent can compose queries dynamically based on tool schema
- [ ] Invalid GraphQL syntax returns an MCP error before reaching the execution engine
- [ ] GraphQL execution errors are surfaced as structured MCP tool errors

### Story US-055: Agent Subscribes to Changes via MCP [FEAT-016]

**As an** AI agent monitoring a collection
**I want** to be notified when entities change
**So that** I can react to state changes without polling

**Acceptance Criteria:**
- [ ] Agent subscribes to `axon://beads` resource
- [ ] When a bead is created/updated/deleted, agent receives
  `resource_updated` notification
- [ ] Agent re-reads the resource to get new state
- [ ] Multiple subscriptions work independently
- [ ] If a subscribed entity is deleted, agent receives a `resource_updated` notification
- [ ] Subscriptions survive schema changes (tool definitions update but subscription continues)

### Story US-056: Local Agent Connects via Stdio [FEAT-016]

**As a** developer using Claude Code with Axon
**I want** to add Axon as an MCP server via stdio
**So that** Claude can directly query and modify my Axon data

**Acceptance Criteria:**
- [ ] `axon-server --mcp-stdio` starts MCP server on stdin/stdout
- [ ] Claude Code can be configured to use this as an MCP server
- [ ] All tools, resources, and prompts are available
- [ ] No authentication required for stdio transport
- [ ] MCP server can be configured as a Claude Code MCP server via `axon-server --mcp-stdio`

### Story US-112: Agent Discovers Policy Envelopes [FEAT-016]

**As an** agent
**I want** MCP tool metadata to describe allowed, approval-routed, and denied
write envelopes
**So that** I can choose safe actions before attempting a mutation

**Acceptance Criteria:**
- [ ] Tool descriptions include policy envelope summaries for the current
  subject where available
- [ ] A write inside an autonomous envelope returns `allowed` and commits only
  when the caller chooses commit mode
- [ ] A write outside the autonomous envelope returns `needs_approval` with a
  mutation intent token
- [ ] A denied write returns the same policy explanation as GraphQL
- [ ] Tool metadata refreshes after policy/schema changes

## Edge Cases

- **Schema change during agent session**: Agent receives
  `tools/list_changed` notification. Old tool definitions may produce
  validation errors if the agent uses stale parameters. Agent should
  re-fetch tool list
- **Large tool list**: 20 collections × 15 tools = 300 tools. Some
  agent frameworks may struggle with large tool lists. Consider tool
  grouping or pagination in future
- **Agent calls tool with wrong parameters**: Return structured MCP
  error with parameter validation details. Include the expected schema
- **Concurrent tool execution**: MCP allows concurrent tool calls. Each
  call is independent — OCC handles write conflicts
- **Agent disconnects during subscription**: Subscription is cleaned up.
  No dangling audit log pollers
- **Collection dropped while agent has tools**: Agent's next call to
  a tool for the dropped collection returns a not-found error.
  `tools/list_changed` notification sent
- **Version conflict on write**: MCP tool error includes current entity
  state so the agent can retry with the correct version
- **Policy change during session**: Agent receives `tools/list_changed` or a
  policy version mismatch result and must re-fetch tool metadata

## Dependencies

- **FEAT-004** (Entity Operations): MCP tools delegate to entity CRUD
- **FEAT-005** (API Surface): MCP endpoint served by the shared server
- **FEAT-009** (Graph Traversal): Traverse tools use link traversal
- **FEAT-013** (Secondary Indexes): Query tools route through index-aware
  planner
- **FEAT-015** (GraphQL): `axon.query` tool bridges MCP to GraphQL
- **FEAT-029** (Data-Layer Access Control Policies): MCP mirrors row filters,
  field redaction, and policy explanation
- **FEAT-030** (Mutation Intents and Approval): MCP mirrors preview,
  approval, and intent commit outcomes
- **ADR-013**: Full design for tool generation, resources, prompts,
  transports

### Crate Dependencies

- MCP protocol implementation (Rust MCP SDK or custom JSON-RPC)
- `axon-core`, `axon-schema`, `axon-api`, `axon-graphql`

## Out of Scope

- **MCP Sampling**: Axon requesting LLM completions (not needed)
- **Per-agent tool hiding as a security boundary**: Tool metadata may hide or
  annotate unavailable operations for ergonomics, but enforcement always
  happens in GraphQL/MCP execution via FEAT-029
- **MCP tool annotations**: Read-only vs destructive hints (spec still
  evolving)
- **Tool grouping / namespacing**: For large deployments with many
  collections (deferred)
- **Streaming tool responses**: Large result sets streamed incrementally
  (MCP spec may support this in future)

## Traceability

### Related Artifacts
- **Parent PRD Section**: Requirements Overview > P1 #13 (MCP server)
- **User Stories**: US-052, US-053, US-054, US-055, US-056, US-112
- **Architecture**: ADR-013 (MCP Server)
- **Implementation**: `crates/axon-mcp/`

### Feature Dependencies
- **Depends On**: FEAT-004, FEAT-005, FEAT-009, FEAT-013, FEAT-015
- **Depended By**: Agent-facing workflows and policy-authoring validation
