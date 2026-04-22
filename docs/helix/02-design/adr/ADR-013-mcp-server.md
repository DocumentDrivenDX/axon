---
ddx:
  id: ADR-013
  depends_on:
    - ADR-002
    - ADR-008
    - ADR-010
    - ADR-012
    - FEAT-004
    - FEAT-005
    - FEAT-013
    - FEAT-015
---
# ADR-013: MCP Server (Model Context Protocol)

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-04-05 | Accepted | Erik LaBianca | ADR-002, ADR-012, ADR-019, FEAT-004, FEAT-005, FEAT-015, FEAT-029, FEAT-030 | High |

## Context

Axon's primary persona is agent developers. The Model Context Protocol
(MCP) is the emerging standard for how AI agents (Claude, GPT, etc.)
interact with external tools and data sources. If Axon is "the central
nervous system for agentic applications," agents should be able to talk
to it natively via MCP without writing custom integration code.

Axon currently has GraphQL as the primary application API plus compatibility
and internal interfaces. These are developer APIs - an agent framework has to
write an adapter to use them. MCP eliminates this: any MCP-capable agent can
connect to Axon and immediately discover what collections exist, what
operations are available, what data shapes to expect, and which writes are
autonomous, approval-routed, or denied by policy.

| Aspect | Description |
|--------|-------------|
| Problem | Agents need custom integration code to use Axon. No standard agent-to-data-store protocol |
| Current State | GraphQL plus compatibility/internal APIs - developer-facing, not agent-native |
| Requirements | MCP server that exposes Axon's full read/write surface to agents via the standard MCP primitives (tools, resources, prompts) |

## Decision

Axon implements an **MCP server** that auto-generates its tool and
resource surface from ESF schemas, just as ADR-012 auto-generates
GraphQL types. The MCP server is a protocol adapter over the same
`AxonHandler` used by all other interfaces.

MCP mirrors GraphQL semantics for policy and mutation intents. It does not
define a second authorization model.

### 1. MCP Primitives Mapping

| MCP Primitive | Axon Mapping | Description |
|---|---|---|
| **Tools** | Write operations + GraphQL queries | Actions the agent can perform |
| **Resources** | Entity and collection URIs | Data the agent can read |
| **Resource subscriptions** | Audit-log-backed change notifications | Push updates when data changes |
| **Prompts** | Pre-built query/action templates | Guided interactions for common workflows |

### 2. Tools

#### Core Tools (Always Present)

These tools exist regardless of which collections are registered:

```
axon.query
  description: "Execute a GraphQL query or mutation against Axon"
  parameters:
    query: string (required) — GraphQL query or mutation text
    variables: object (optional) — GraphQL variables
  returns: GraphQL response JSON

axon.mutation.preview
  description: "Preview a GraphQL mutation or transaction and return diff, policy decision, and intent token"
  parameters:
    operation: object (required)
  returns: mutation preview result

axon.mutation.commit_intent
  description: "Commit a previously previewed and allowed/approved mutation intent"
  parameters:
    intent_token: string (required)
  returns: commit result or stale/mismatch error

axon.collection.create
  description: "Create a new collection with a schema"
  parameters:
    name: string (required)
    schema: object (required) — ESF schema document
  returns: collection metadata

axon.collection.list
  description: "List all collections with metadata"
  parameters: (none)
  returns: array of collection metadata

axon.collection.drop
  description: "Drop a collection and all its entities"
  parameters:
    name: string (required)
    confirm: boolean (required, must be true)
  returns: confirmation

axon.schema.get
  description: "Get the schema for a collection"
  parameters:
    collection: string (required)
  returns: ESF schema document

axon.schema.put
  description: "Update the schema for a collection"
  parameters:
    collection: string (required)
    schema: object (required)
  returns: schema version number
```

#### Collection-Specific Tools (Auto-Generated from ESF)

When a collection is registered with a schema, Axon generates
collection-specific MCP tools. These provide typed, discoverable
operations that agents can use without knowing the generic API.

For a `beads` collection with the beads ESF schema:

```
beads.create
  description: "Create a new bead"
  parameters:
    id: string (optional — server generates UUIDv7 if omitted)
    data:
      bead_type: string (required, enum: [task, bug, epic, ...])
      status: string (optional, defaults to "draft")
      title: string (required, minLength: 1)
      description: string (optional)
      priority: integer (optional, 0-4)
      labels: array of string (optional)
      owner: string (optional)
      assignee: string (optional)
      ... (all fields from entity_schema)
    actor: string (optional)
  returns: created entity with id, version, timestamps

beads.get
  description: "Get a bead by ID"
  parameters:
    id: string (required)
  returns: entity or not-found error

beads.update
  description: "Replace a bead (full replacement, OCC)"
  parameters:
    id: string (required)
    data: object (required — full entity data)
    expected_version: integer (required)
    actor: string (optional)
  returns: updated entity

beads.patch
  description: "Partially update a bead (JSON Merge Patch)"
  parameters:
    id: string (required)
    patch: object (required — fields to change, null to remove)
    expected_version: integer (required)
    actor: string (optional)
  returns: updated entity (or unchanged entity if no-op)

beads.delete
  description: "Delete a bead"
  parameters:
    id: string (required)
    expected_version: integer (optional)
    force: boolean (optional, default false — force deletes links)
    actor: string (optional)
  returns: confirmation

beads.query
  description: "Query beads with filters and sorting"
  parameters:
    filter: object (optional)
      status: string or array
      bead_type: string or array
      priority: object { gt, gte, lt, lte, eq }
      assignee: string
      ... (fields from indexed fields)
    sort: object { field: string, direction: "asc"|"desc" }
    limit: integer (optional, default 50)
    after: string (optional, cursor)
  returns: array of entities with pagination cursor

beads.link
  description: "Create a link from a bead to another entity"
  parameters:
    source_id: string (required)
    link_type: string (required, enum from schema link_types)
    target_collection: string (required)
    target_id: string (required)
    metadata: object (optional)
  returns: link confirmation

beads.unlink
  description: "Remove a link from a bead"
  parameters:
    source_id: string (required)
    link_type: string (required)
    target_collection: string (required)
    target_id: string (required)
  returns: confirmation

beads.traverse
  description: "Traverse links from a bead"
  parameters:
    start_id: string (required)
    link_type: string (required)
    direction: string (optional, "forward"|"reverse", default "forward")
    max_depth: integer (optional, default 3)
    filter: object (optional — hop filter)
  returns: array of entities with paths

beads.audit
  description: "View audit history for a bead"
  parameters:
    id: string (required)
    limit: integer (optional, default 20)
  returns: array of audit entries
```

**Lifecycle-aware tools**: When the schema declares lifecycles, the
collection gets a transition tool:

```
beads.transition
  description: "Transition a bead's status"
  parameters:
    id: string (required)
    to: string (required, enum: valid targets from current state)
    expected_version: integer (required)
    actor: string (optional)
  returns: updated entity

  Note: The 'to' enum is dynamic — it depends on the entity's current
  status. The tool description includes the full transition map so the
  agent can reason about valid transitions without fetching the entity
  first.
```

#### Tool Generation Rules

1. One set of CRUD tools per registered collection
2. Tool parameter schemas derived from ESF Layer 1 (JSON Schema)
3. Filter parameters derived from ESF Layer 4 (indexed fields are
   highlighted in descriptions as "fast lookup")
4. Link tool parameters derived from ESF Layer 2 (link types constrain
   the `link_type` enum)
5. Transition tool derived from ESF Layer 3 (lifecycle transitions
   constrain the `to` enum)
6. Tool descriptions include enough context for an agent to use them
   without reading documentation

#### Tool Regeneration

When a collection schema changes (via `put_schema`), the MCP tool
definitions are regenerated. Connected agents receive a `tools/list_changed`
notification per the MCP protocol, prompting them to re-fetch the tool
list.

### 3. Resources

MCP resources provide read access to Axon data via URIs. Resources are
complementary to tools — an agent can read a resource to understand
current state, then call a tool to modify it.

#### Resource URI Scheme

```
axon://{collection}/{id}                — single entity
axon://{collection}                     — collection listing (paginated)
axon://{collection}/{id}/links          — entity's outbound links
axon://{collection}/{id}/links/inbound  — entity's inbound links
axon://{collection}/{id}/audit          — entity's audit history
axon://_schemas/{collection}            — collection schema
axon://_schemas                         — all schemas
axon://_collections                     — collection metadata list
```

With multi-tenancy (FEAT-014), the URI gains database and schema
prefixes:

```
axon://{database}/{schema}/{collection}/{id}
```

Defaulting to `axon://default/default/{collection}/{id}` when omitted.

#### Resource Content

Resources return JSON. Entity resources return the entity data with
system metadata. Collection listing resources return paginated entity
summaries.

#### Resource Templates

MCP resource templates allow agents to discover the URI pattern:

```json
{
  "uriTemplate": "axon://{collection}/{id}",
  "name": "Entity by ID",
  "description": "Retrieve a single entity from a collection",
  "mimeType": "application/json"
}
```

### 4. Resource Subscriptions

Agents can subscribe to resource changes. The MCP server translates
subscriptions into audit log polling (same mechanism as GraphQL
subscriptions from ADR-012):

```
Agent subscribes to: axon://beads/bead-42
  → MCP server polls audit log for (collection=beads, entity_id=bead-42)
  → On change: push resource_updated notification to agent

Agent subscribes to: axon://beads
  → MCP server polls audit log for (collection=beads)
  → On change: push resource_updated notification for any bead mutation
```

The notification tells the agent that the resource has changed. The agent
then re-reads the resource to get the new state (per MCP protocol — the
notification doesn't include the new data).

### 5. Prompts

MCP prompts are pre-built interaction templates. Axon generates prompts
that help agents perform common workflows:

```
axon.explore_collection
  description: "Understand the structure and contents of a collection"
  arguments:
    collection: string (required)
  generates: prompt asking the LLM to inspect the schema, sample
    entities, and summarize the collection's purpose and data shape

axon.dependency_analysis
  description: "Analyze the dependency graph of an entity"
  arguments:
    collection: string (required)
    id: string (required)
    link_type: string (optional, default: all link types)
  generates: prompt asking the LLM to traverse dependencies, identify
    blocked items, and suggest next actions

axon.audit_review
  description: "Review recent changes to an entity or collection"
  arguments:
    collection: string (required)
    id: string (optional — omit for collection-wide review)
    limit: integer (optional, default 20)
  generates: prompt with audit data asking the LLM to summarize what
    changed, who made changes, and flag anything unusual

axon.schema_review
  description: "Review a collection schema and suggest improvements"
  arguments:
    collection: string (required)
  generates: prompt with the ESF schema asking the LLM to evaluate
    field types, required fields, index coverage, and lifecycle
    completeness
```

Prompts are optional — agents work fine with just tools and resources.
Prompts add guided workflows for common patterns.

### 6. Transport

#### Stdio (Local Agents)

For agents running on the same machine (Claude Code, local agent
frameworks):

```bash
axon-server --mcp-stdio
```

The server speaks MCP over stdin/stdout. This is the standard MCP
transport for local tools.

#### HTTP+SSE (Remote Agents)

For agents connecting over the network:

```
POST /mcp          — MCP JSON-RPC requests
GET  /mcp/sse      — Server-Sent Events for notifications
```

These are served by axum alongside GraphQL and compatibility endpoints.

Both transports speak the same MCP JSON-RPC protocol. The server
implementation is transport-agnostic.

### 7. Authentication

MCP requests carry the same identity as other Axon protocols:

- **Stdio**: No auth (same process, same trust boundary as `--no-auth`)
- **HTTP+SSE**: Same auth middleware as the GraphQL/API gateway (FEAT-012).
  The MCP request includes the auth header; the server resolves identity
  and applies RBAC/ABAC before executing the tool

The `actor` field in tool parameters is optional — if omitted, the
authenticated identity is used. If provided, it's validated against the
authenticated identity's permissions.

### 8. axon.query Tool and GraphQL

The `axon.query` tool is the bridge between MCP and GraphQL. An agent
can use it for both complex reads and writes:

**Complex read:**
```json
{
  "tool": "axon.query",
  "arguments": {
    "query": "{ beads(filter: {status: {eq: \"in_progress\"}}, limit: 5) { edges { node { id title dependsOn { edges { node { id status } } } } } } }"
  }
}
```

**Mutation via GraphQL:**
```json
{
  "tool": "axon.query",
  "arguments": {
    "query": "mutation { transitionBeadStatus(input: {id: \"bead-42\", to: IN_PROGRESS, expectedVersion: 3, actor: \"agent-1\"}) { bead { id version status } } }"
  }
}
```

**Atomic transaction:**
```json
{
  "tool": "axon.query",
  "arguments": {
    "query": "mutation($ops: [TransactionOp!]!) { commitTransaction(input: {operations: $ops}) { transactionId results { index success } } }",
    "variables": { "ops": [{"createEntity": {"collection": "beads", "data": {"title": "new bead", "bead_type": "task", "status": "draft"}}}] }
  }
}
```

This gives agents the full power of GraphQL — queries with relationship
traversal, mutations with OCC, and transactions — through a single MCP
tool. Simple agents use the collection-specific MCP tools (`beads.create`,
`beads.transition`). Sophisticated agents compose GraphQL through
`axon.query`.

The `axon.query` tool also serves as the path for future query
capabilities: when Cypher or vector search are added, they could surface
as extensions to the GraphQL schema, automatically available through
`axon.query` without new MCP tools.

### 9. Crate and Dependencies

```
crates/
  axon-mcp/           # MCP server implementation
    src/
      server.rs       # MCP protocol handler (JSON-RPC)
      tools.rs        # Tool definitions and dispatch
      resources.rs    # Resource URI resolution
      prompts.rs      # Prompt templates
      generate.rs     # ESF → MCP tool/resource generation
      transport/
        stdio.rs      # Stdin/stdout transport
        http.rs       # HTTP+SSE transport (axum)
```

- **`rmcp`** or similar MCP SDK crate for Rust (if mature enough),
  otherwise implement the JSON-RPC protocol directly (it's simple)
- Depends on `axon-core`, `axon-schema`, `axon-api`, `axon-graphql`

### 10. Relationship to Other Interfaces

```
                    ┌──────────────┐
                    │    Agent     │
                    │  (Claude,    │
                    │   GPT, etc.) │
                    └──────┬───────┘
                           │ MCP (stdio or HTTP+SSE)
                           ▼
                    ┌──────────────┐
                    │  axon-mcp    │
                    │              │
                    │ Tools ───────┤
                    │ Resources ───┤
                    │ Prompts ─────┤
                    └──────┬───────┘
                           │
          ┌────────────────┼────────────────┐
          │                │                │
          ▼                ▼                ▼
   ┌──────────��─┐  ┌────────────┐  ┌────────────┐
   │  Structured │  │  GraphQL   │  │  Direct    │
   │  API calls  │  │  (ADR-012) │  │  Handler   │
   │  (CRUD,     │  │  via       │  │  calls     │
   │   links,    │  │  axon.query│  │  (resources)│
   │   traverse) │  │            │  │            │
   └──────┬──────┘  └─────┬──────┘  └─────┬──────┘
          └───────────────┼────────────────┘
                          ▼
                   ┌──────────────┐
                   │ AxonHandler  │
                   └──────────────┘
```

All interfaces share the same handler, same storage, same indexes, same
audit log. MCP is a protocol adapter, not a separate system.

## Example: Agent Workflow

An agent connecting to Axon via MCP:

1. **Discovery**: Agent calls `tools/list` → receives all available tools
   including `beads.create`, `beads.query`, `beads.transition`, etc.
   Tool schemas tell the agent exactly what parameters each tool accepts.

2. **Read**: Agent calls `beads.query` with `filter: {status: "ready"}` →
   receives ready beads with their data.

3. **Complex read**: Agent calls `axon.query` with a GraphQL query →
   gets beads with dependencies and audit history in one response.

4. **Write**: Agent calls `beads.transition` with `{id: "bead-42",
   to: "in_progress", expected_version: 3}` → bead status updated.

5. **Subscribe**: Agent subscribes to `axon://beads` resource →
   receives notifications when any bead changes.

6. **Link**: Agent calls `beads.link` with `{source_id: "bead-42",
   link_type: "depends-on", target_collection: "beads",
   target_id: "bead-99"}` → dependency created.

The agent never reads Axon documentation. The MCP tool definitions
*are* the documentation.

## Consequences

**Positive**:
- Any MCP-capable agent can use Axon with zero integration code
- Tool definitions are auto-generated from ESF — zero maintenance
- Schema changes automatically update the agent's available tools
- `axon.query` gives agents full GraphQL power through a single MCP tool
- Resource URIs provide a standard way to reference Axon data
- Subscriptions give agents push-based change notification
- Prompts provide guided workflows for common agent tasks
- Same handler, same auth, same audit — consistent across all protocols

**Negative**:
- New crate and protocol to implement and maintain
- MCP protocol is still evolving — may require adaptation as the spec
  matures
- Auto-generated tool lists can be large for many collections (agent
  context window pressure). May need tool grouping or lazy loading
- Stdio transport requires the MCP server to run as a subprocess,
  which is a different deployment model than the HTTP server

**Deferred**:
- MCP Sampling (allowing Axon to request LLM completions) — not needed
  for V1
- MCP tool annotations (read-only vs destructive hints) — add when the
  spec stabilizes
- Per-agent tool filtering (show agent X only the tools it has
  permission to use) — requires FEAT-012 integration

## References

- [ADR-012: GraphQL Query Layer](ADR-012-graphql-query-layer.md)
- [FEAT-004: Entity Operations](../../01-frame/features/FEAT-004-entity-operations.md)
- [FEAT-015: GraphQL Query Layer](../../01-frame/features/FEAT-015-graphql-query-layer.md)
- [Model Context Protocol Specification](https://modelcontextprotocol.io)
- [MCP Rust SDK](https://github.com/modelcontextprotocol/rust-sdk)
