<bead-review>
  <bead id="axon-1202db7c" iter=1>
    <title>refactor(axon-cypher): extract axon-cypher-ast crate (parser/AST/validator)</title>
    <description>
Split axon-cypher into two crates so that the executor can later depend on axon-storage without forming a cycle through axon-schema. Extract the AST/parser/validator surface into a new leaf crate `axon-cypher-ast`, leaving the executor + planner + QueryStore in `axon-cypher`.

Sub-bead 1/3 of the FEAT-009 dep inversion enabling axon-5956e527's AC1 (StorageAdapter-backed QueryStore physically inside axon-cypher).

Layout after split:

axon-cypher-ast (new leaf crate, depends on axon-core only):
  - src/ast.rs (move from axon-cypher)
  - src/lexer.rs (move)
  - src/parser.rs (move)
  - src/validator.rs (move)
  - src/schema.rs (move; this is the cypher schema namespace, not axon-schema)
  - src/error.rs (move CypherError; executor variants stay accessible via re-export)
  - src/lib.rs (new)

axon-cypher (existing crate, slimmed):
  - Adds axon-cypher-ast = { path = "../axon-cypher-ast" }
  - lib.rs re-exports parse, plan, validate, ast::*, schema::*, CypherError so existing callers (axon-schema, tests) compile unchanged
  - executor.rs, planner.rs, memory_store.rs (QueryStore trait + MemoryQueryStore) stay

In-scope files:
  - crates/axon-cypher-ast/Cargo.toml (new)
  - crates/axon-cypher-ast/src/{lib.rs,ast.rs,lexer.rs,parser.rs,validator.rs,schema.rs,error.rs}
  - crates/axon-cypher/Cargo.toml (add axon-cypher-ast dep)
  - crates/axon-cypher/src/lib.rs (re-export from axon-cypher-ast; drop moved modules)
  - Cargo.toml (workspace members)

Per CLAUDE.md, declare any shared deps in root Cargo.toml [workspace.dependencies] and use { workspace = true } in crate manifests.

Out-of-scope (separate beads):
  - axon-schema imports — sub-bead B1.2.
  - axon-storage move of storage_adapter_store.rs — sub-bead B1.3.

Rollback: if the split is partial, revert all moved files; do not leave a half-extracted crate in the workspace. Either the new crate compiles or the change is reverted entirely.
    </description>
    <acceptance>
AC1. crates/axon-cypher-ast/ exists with the parser/AST/validator code; cargo build -p axon-cypher-ast succeeds.
AC2. axon-cypher re-exports parse, validate, plan, ast::*, schema::*, CypherError so existing callers compile without changes.
AC3. cargo check --workspace passes.
AC4. cargo test --workspace passes.
AC5. cargo clippy --workspace -- -D warnings passes.
AC6. cargo tree -p axon-cypher-ast -e normal shows no axon-* dep other than axon-core (axon-cypher-ast is a leaf w.r.t. workspace crates).
    </acceptance>
    <labels>helix, feat-009, area:cypher, kind:refactor</labels>
  </bead>

  <changed-files>
    <file>.ddx/executions/20260508T012422-7a41d5c9/manifest.json</file>
    <file>.ddx/executions/20260508T012422-7a41d5c9/result.json</file>
  </changed-files>

  <governing>
    <ref id="FEAT-009" path="docs/helix/01-frame/features/FEAT-009-graph-traversal-queries.md" title="Feature Specification: FEAT-009 — Unified Graph Query (Cypher)">
      <content>
<untrusted-data>
---
ddx:
  id: FEAT-009
  depends_on:
    - helix.prd
    - FEAT-002
    - FEAT-007
    - FEAT-013
    - FEAT-015
    - FEAT-016
    - FEAT-029
    - ADR-010
    - ADR-019
    - ADR-020
    - ADR-021
---
# Feature Specification: FEAT-009 — Unified Graph Query (Cypher)

**Feature ID**: FEAT-009
**Status**: Specified
**Priority**: P0
**Owner**: Core Team
**Created**: 2026-04-04
**Updated**: 2026-05-02

## Overview

A single read-side query language unifies filter, sort, aggregate,
traversal, neighbor discovery, and pattern matching. The language is a
read-only subset of openCypher (selected in ADR-021, sitting on the
document-shaped data model fixed in ADR-020).

Two surfaces expose the language:

1. **Schema-declared named queries** — declared in the collection schema,
   compiled and policy-validated at schema-write time, exposed as typed
   GraphQL fields and MCP tools.
2. **Ad-hoc queries** — `axonQuery(cypher: String!)` accepts a query
   string at request time, subject to policy, depth, and cost limits.

This feature absorbs the previous FEAT-020 (link discovery and graph
queries). FEAT-020 is retired; its user stories (US-070 link candidates,
US-071 neighbors, US-072 graph exploration, US-073 MCP discovery) live
here.

## Problem Statement

Entities in isolation are documents. Entities connected by typed links are
a graph. Without a unified query language, every read pattern requires its
own surface — connection arguments for filter/sort, traversal directives,
neighbor queries, link-discovery queries, ad-hoc patterns. That multiplies
spec, test, and policy surface and pushes graph-shaped reasoning into
client code with N+1 round trips.

The DDx consumer use case (axon-05c1019d) sharpened this: the ready-queue
query *"open beads with no open `depends_on` targets"* requires a single
round-trip predicate over outgoing-link target state. None of the existing
FEAT-009 traversal, FEAT-020 link discovery, or FEAT-015 connection
filters express it directly. A real graph query language does, and the
same language naturally subsumes filter/sort/aggregate/traversal/neighbor
discovery as well — there is no reason to keep separate paths.

## Requirements

### Functional Requirements

#### Cypher subset (per ADR-021)

The supported clauses, parameters, types, and exclusions are specified
in ADR-021. In summary, V1 supports `MATCH`, `OPTIONAL MATCH`, `WHERE`,
`WITH`, `RETURN`, `ORDER BY`, `SKIP`, `LIMIT`, `EXISTS`/`NOT EXISTS`,
variable-length paths with explicit bounds (`*1..N`), and standard
aggregations (`count`, `sum`, `avg`, `min`, `max`, `collect`). Write
clauses, `shortestPath()`, `CALL`, `LOAD CSV`, and unbounded path
patterns are excluded.

#### Schema-declared named queries

A collection schema may declare a `queries:` block (FEAT-002 / ESF
extension):

```yaml
collection: ddx_beads
queries:
  ready_beads:
    description: "Open beads with no open dependencies"
    cypher: |
      MATCH (b:DdxBead {status: 'open'})
      WHERE NOT EXISTS {
        (b)-[:DEPENDS_ON]->(d:DdxBead)
        WHERE d.status <> 'closed'
      }
      RETURN b
      ORDER BY b.priority DESC, b.updated_at DESC
    parameters: []
```

- Each named query is type-checked at `put_schema` time against the active
  collection schemas.
- The schema compiler validates index usage; queries requiring unindexed
  scans on collections above the configured threshold are rejected with a
  diagnostic suggesting an index declaration.
- The schema compiler validates policy compatibility; queries that would
  require policy bypass to be useful are rejected (`policy_required_bypass`).
- Each named query generates one typed GraphQL field on `Query` and one
  MCP tool on the collection's tool group.

#### Ad-hoc queries

```graphql
type Query {
  axonQuery(cypher: String!, parameters: JSON): AxonQueryResult!
}
```

- Same parser, planner, and policy-enforcement path as named queries.
- Parsing rejects any reference to a label, property, or relationship type
  not present in the active schema.
- Cost-budget rejection: ad-hoc queries are stricter than named queries on
  unindexed scans and intermediate cardinality. Named queries can opt into
  larger budgets at schema-declaration time.
- Result type is JSON because the result shape isn't statically typed.
  Metadata field reports plan info, index usage, and policy decisions.

#### Subscriptions on named queries

Named queries are subscribable via FEAT-015's GraphQL subscription path.
The change-feed pipeline re-evaluates the named query when underlying
collections or links change. Ad-hoc queries are *not* subscribable in V1.

#### Index usage and acceleration

Query planning rules (rule-based, not cost-based; ADR-021 §Compilation):

1. Label + property predicate → use the FEAT-013 secondary index.
2. Relationship traversal → use links table PK + target index.
3. `EXISTS { (a)-[:R]->(b) }` → index probe on links table.
4. `ORDER BY n.field` covered by index → index scan order, no sort.
5. No match → full collection scan with application-layer filter; subject
   to budget.

#### Policy integration

(See FEAT-029 / ADR-019 / ADR-021 §Policy integration.)

- Row policy at each label match.
- Field redaction at projection — `RETURN n.field` returns redacted value
  when policy redacts; redacted fields cannot be used in `WHERE` predicates
  or aggregations.
- `EXISTS` is policy-aware; hidden targets do not leak via existence.
- `count(*)` and aggregates only count rows the subject is allowed to see.

### Non-Functional Requirements

| Operation | Target (p99) | Notes |
|---|---|---|
| Single-entity match by label + property | < 5 ms | Index lookup |
| 3-hop traversal over 10K entities | < 50 ms | Same as prior FEAT-009 budget |
| Ready/blocked pattern @ 1K beads | < 100 ms | DDx Use Case A latency budget |
| Ready/blocked pattern @ 10K beads | < 500 ms | DDx Use Case A latency budget |
| Ad-hoc query parse + plan | < 10 ms | Excludes execution |
| Schema-time named query compile | < 50 ms | Per query |

Bounded streaming memory; spilling to disk deferred to V2; 30-second wall-
clock timeout; 10-hop default depth cap.

## User Stories

### Story US-023: Traverse a Dependency Graph [FEAT-009]

**As an** agent managing a work queue
**I want** to find all transitive dependencies of a bead
**So that** I can determine if the bead is ready to execute

**Acceptance Criteria:**
- [ ] `MATCH (a:Bead {id: 'bead-A'})-[:DEPENDS_ON*1..10]->(d:Bead) RETURN d` returns all transitive dependencies.
- [ ] Result includes the path when `RETURN p` is used with `MATCH p = (a)-[:DEPENDS_ON*1..10]->(d)`.
- [ ] Circular dependencies are detected and reported (no infinite loop).
- [ ] Filtering at each hop works: `WHERE d.status <> 'done'` returns only incomplete dependencies.
- [ ] Cycle-bearing traversal terminates safely (no infinite loop or timeout).

### Story US-024: Explode a Bill of Materials [FEAT-009]

**As an** ERP application
**I want** to recursively expand a product into its component parts
**So that** I can calculate total cost and check inventory for all sub-assemblies

**Acceptance Criteria:**
- [ ] `MATCH (p:Product {id: 'widget-X'})-[c:CONTAINS*1..8]->(comp) RETURN comp, c` returns the full BOM tree with relationship metadata.
- [ ] Relationship properties (`c.quantity`) accessible in `RETURN`.
- [ ] Leaf nodes (no outgoing `:CONTAINS` links) are identified.
- [ ] Shared components reached via multiple paths appear once with all paths listed via `collect(p)`.
- [ ] Deleted link targets are skipped.

### Story US-025: Check Reachability [FEAT-009]

**As a** project management tool
**I want** to check if issue A is transitively blocked by issue B
**So that** I can warn users about hidden dependencies

**Acceptance Criteria:**
- [ ] `RETURN EXISTS { MATCH (a:Issue {id: $a})-[:BLOCKS|DEPENDS_ON*1..10]->(b:Issue {id: $b}) }` returns true/false without materializing the path.
- [ ] Short-circuits on first path found.
- [ ] Multi-link-type alternation (`:BLOCKS|DEPENDS_ON`) works.

### Story US-070: Find Link Targets [FEAT-009] (formerly FEAT-020)

**As an** agent creating a dependency link
**I want** to discover which entities I can link to
**So that** I can pick the right target without fetching the entire collection

**Acceptance Criteria:**
- [ ] A named query `link_candidates(source_id, link_type, search, limit)` returns entities from the link type's target collection.
- [ ] Search and filter clauses combine in a single `MATCH ... WHERE` with index-backed predicates.
- [ ] An `OPTIONAL MATCH` against the source's existing links produces an `already_linked` boolean projection.
- [ ] Cardinality from the schema is exposed as schema metadata, not a query result.
- [ ] Result returns in < 50ms for a 10K-entity target collection with indexed predicate.

### Story US-071: List Entity Neighbors [FEAT-009] (formerly FEAT-020)

**As an** agent understanding an entity's relationships
**I want** to see all entities linked to and from an entity
**So that** I can understand the entity's context in the graph

**Acceptance Criteria:**
- [ ] `MATCH (a {id: $id})-[r]-(b) RETURN type(r), b` returns both inbound and outbound neighbors.
- [ ] Direction can be filtered (`-[r]->` or `<-[r]-`) for outbound-only or inbound-only.
- [ ] Link-type filter via `[r:DEPENDS_ON]`.
- [ ] Returns in < 20ms p99 for an entity with < 100 links.

### Story US-072: Explore Graph via GraphQL [FEAT-009] (formerly FEAT-020)

**As a** UI developer building a relationship explorer
**I want** to traverse the entity graph via GraphQL
**So that** I can build interactive graph views with drill-down

**Acceptance Criteria:**
- [ ] Named query results expose typed GraphQL connections (`edges`, `pageInfo`, `totalCount`).
- [ ] Multi-hop named queries work without N+1 fanout.
- [ ] Depth limit prevents infinite nesting (default 10).
- [ ] Connection arguments (`first`, `after`, `last`, `before`) work on named query connections.

### Story US-073: Discover Links via MCP [FEAT-009] (formerly FEAT-020)

**As an** AI agent building entity relationships
**I want** MCP tools for link discovery and neighbor queries
**So that** I can explore the graph through the standard agent protocol

**Acceptance Criteria:**
- [ ] Each named query generates a corresponding MCP tool with parameters drawn from the query's `parameters:` block.
- [ ] Tool descriptions include the named query's `description:` field.
- [ ] `axon.query(cypher, parameters)` exposes ad-hoc queries.
- [ ] Both tools enforce the same policy and limits as the GraphQL surface.

### Story US-074: Pattern query for ready/blocked queue [FEAT-009]

**As a** DDx worker (downstream consumer per axon-05c1019d)
**I want** to retrieve all "ready" beads (status open, no open dependencies) in a single round-trip
**So that** worker pickup decisions don't dominate latency

**Acceptance Criteria:**
- [ ] A schema-declared named query `ready_beads` with the pattern `MATCH (b:DdxBead {status:'open'}) WHERE NOT EXISTS { (b)-[:DEPENDS_ON]->(d:DdxBead) WHERE d.status <> 'closed' } RETURN b` returns all ready beads.
- [ ] A complementary `blocked_beads` query returns the inverse.
- [ ] At 1K beads (500 open, varied dep states): < 100ms p99 single round-trip.
- [ ] At 10K beads: < 500ms p99 single round-trip.
- [ ] Subscription on `ready_beads` emits updates when underlying beads or links change.
- [ ] DDx can drop its two-phase fallback after this lands.

### Story US-075: Schema-declared named query [FEAT-009]

**As a** developer defining a collection schema
**I want** to declare reusable graph queries in the schema
**So that** they are policy-validated, index-validated, and surfaced as typed GraphQL fields

**Acceptance Criteria:**
- [ ] `put_schema` accepts a `queries:` block per ADR-021's shape.
- [ ] Each named query is type-checked against the collection's schema (label, properties, relationships exist).
- [ ] Each named query is policy-validated; queries requiring policy bypass are rejected.
- [ ] Each named query is index-validated; queries requiring unindexed scans on large collections are rejected with a diagnostic.
- [ ] On successful schema activation, the named query appears as a typed GraphQL field and a corresponding MCP tool.
- [ ] `put_schema --dry-run` returns a compile report including named-query diagnostics without activating.

### Story US-076: Ad-hoc Cypher query [FEAT-009]

**As a** developer or operator exploring the entity graph
**I want** to run an ad-hoc Cypher query at runtime
**So that** I can inspect data, debug, and answer one-off questions without re-shipping a schema

**Acceptance Criteria:**
- [ ] `query { axonQuery(cypher: "...") { rows schema metadata } }` parses, plans, executes, and returns rows as JSON with column type metadata.
- [ ] Parsing rejects references to labels, properties, or relationship types not in the active schema.
- [ ] Policy is enforced identically to named queries.
- [ ] Ad-hoc queries are rejected when their planned cardinality exceeds the configured budget.
- [ ] `axonQuery` errors carry stable error codes (`unsupported_clause`, `unknown_label`, `unsupported_query_plan`, `policy_required_bypass`, `query_too_large`, `query_timeout`).

### Story US-077: Subscribe to a named query [FEAT-009]

**As a** UI or downstream consumer (DDx server, admin UI live view)
**I want** to subscribe to the result of a named query
**So that** I see updates without polling

**Acceptance Criteria:**
- [ ] `subscription { ready_beads { ... } }` delivers updates when a relevant change lands (entity created/updated/deleted, link created/deleted) that affects the result set.
- [ ] Updates are policy-filtered for the subscriber's identity.
- [ ] Initial snapshot is delivered on subscribe.
- [ ] Subscription tears down cleanly on disconnect; no leaked watchers.

## Edge Cases

- **Disconnected entity**: traversal from an entity with no outgoing links of the matched type returns empty.
- **Cross-collection traversal**: links span collections naturally.
- **Deleted link target**: traversal skips dangling links.
- **Large fan-out**: pagination + streaming prevent memory exhaustion.
- **Diamond pattern**: same entity reachable via multiple paths returned once with all paths listed via `collect()`.
- **Empty target collection**: candidate queries return empty rows, not error.
- **Self-referential pattern**: `MATCH (a)-[:R]->(a)` works; the query must be careful about cycles when chained.

## Dependencies

- **FEAT-002** (Schema Engine): label/property typing, named-query block.
- **FEAT-007** (Entity-Graph Model): entities and links exist.
- **FEAT-013** (Secondary Indexes): query acceleration.
- **FEAT-015** (GraphQL): named-query field generation, subscriptions, ad-hoc resolver.
- **FEAT-016** (MCP): per-named-query tools, `axon.query` tool.
- **FEAT-029** (Access Control): row policy, field redaction, `EXISTS` policy-awareness.
- **ADR-010** (Physical Storage and Secondary Indexes).
- **ADR-019** (Policy Authoring): policy compilation rules apply to named queries.
- **ADR-020** (Data Model): document-shaped, not RDF.
- **ADR-021** (Graph Query Language): the language itself, supported subset, planner.

## Out of Scope

- Cypher write clauses (`CREATE`, `MERGE`, `SET`, `DELETE`, `REMOVE`) — V2+, would conflict with FEAT-030 mutation-intent flow.
- `shortestPath()` / `allShortestPaths()` — V2.
- Weighted path computation — V2.
- `CALL { subquery }` and procedure calls — V2.
- User-defined functions — V2.
- `UNION` / `UNION ALL` — revisit when a real use case appears.
- Spilling to disk for large `ORDER BY` / `collect()` — V2.
- Subscriptions on ad-hoc queries — V2.
- SPARQL or alternative query grammars — rejected per ADR-020.
- Graph visualization — UI concern (FEAT-011 V2).
- Graph analytics (PageRank, centrality, community detection) — analytical workloads belong in CDC → DuckDB / niflheim.

## Traceability

### Related Artifacts
- **Parent PRD Sections**: §4 (Data Model — Query Model), §8 P0 #16 (Unified graph query).
- **Use Case Research**: All 10 domains use traversal; ERP BOM, agentic dependency DAGs, CDP identity lineage, DDx ready/blocked queue (axon-05c1019d).
- **User Stories**: US-023, US-024, US-025, US-070, US-071, US-072, US-073, US-074, US-075, US-076, US-077.
- **Architecture**: ADR-020 (data model), ADR-021 (language), ADR-010 (storage + indexes), ADR-019 (policy).
- **Implementation**: `crates/axon-cypher/` (parser, planner, executor), `crates/axon-schema/` (named-query block), `crates/axon-graphql/` (field generation, subscriptions, ad-hoc resolver), `crates/axon-mcp/` (tools).

### Feature Dependencies
- **Depends On**: FEAT-002, FEAT-007, FEAT-013, FEAT-015, FEAT-016, FEAT-029.
- **Depended By**: FEAT-006 (Bead Adapter — ready queue uses named query), FEAT-011 (Admin UI graph exploration), and the DDx adoption epic (axon-82b6f7b2).
- **Supersedes**: FEAT-020 (Link Discovery and Graph Queries) — retired as of 2026-05-02.
</untrusted-data>
      </content>
    </ref>
  </governing>

  <diff rev="27b26d0fe0b8ab38ac4ff0505333072bf11b6fd9">
<untrusted-data>
diff --git a/.ddx/executions/20260508T012422-7a41d5c9/manifest.json b/.ddx/executions/20260508T012422-7a41d5c9/manifest.json
new file mode 100644
index 0000000..fb46bb3
--- /dev/null
+++ b/.ddx/executions/20260508T012422-7a41d5c9/manifest.json
@@ -0,0 +1,55 @@
+{
+  "attempt_id": "20260508T012422-7a41d5c9",
+  "bead_id": "axon-1202db7c",
+  "base_rev": "b63b86754707d9e2d9e9f3f1b5e1ca915993d2cd",
+  "created_at": "2026-05-08T01:24:23.252858796Z",
+  "requested": {
+    "prompt": "synthesized"
+  },
+  "bead": {
+    "id": "axon-1202db7c",
+    "title": "refactor(axon-cypher): extract axon-cypher-ast crate (parser/AST/validator)",
+    "description": "Split axon-cypher into two crates so that the executor can later depend on axon-storage without forming a cycle through axon-schema. Extract the AST/parser/validator surface into a new leaf crate `axon-cypher-ast`, leaving the executor + planner + QueryStore in `axon-cypher`.\n\nSub-bead 1/3 of the FEAT-009 dep inversion enabling axon-5956e527's AC1 (StorageAdapter-backed QueryStore physically inside axon-cypher).\n\nLayout after split:\n\naxon-cypher-ast (new leaf crate, depends on axon-core only):\n  - src/ast.rs (move from axon-cypher)\n  - src/lexer.rs (move)\n  - src/parser.rs (move)\n  - src/validator.rs (move)\n  - src/schema.rs (move; this is the cypher schema namespace, not axon-schema)\n  - src/error.rs (move CypherError; executor variants stay accessible via re-export)\n  - src/lib.rs (new)\n\naxon-cypher (existing crate, slimmed):\n  - Adds axon-cypher-ast = { path = \"../axon-cypher-ast\" }\n  - lib.rs re-exports parse, plan, validate, ast::*, schema::*, CypherError so existing callers (axon-schema, tests) compile unchanged\n  - executor.rs, planner.rs, memory_store.rs (QueryStore trait + MemoryQueryStore) stay\n\nIn-scope files:\n  - crates/axon-cypher-ast/Cargo.toml (new)\n  - crates/axon-cypher-ast/src/{lib.rs,ast.rs,lexer.rs,parser.rs,validator.rs,schema.rs,error.rs}\n  - crates/axon-cypher/Cargo.toml (add axon-cypher-ast dep)\n  - crates/axon-cypher/src/lib.rs (re-export from axon-cypher-ast; drop moved modules)\n  - Cargo.toml (workspace members)\n\nPer CLAUDE.md, declare any shared deps in root Cargo.toml [workspace.dependencies] and use { workspace = true } in crate manifests.\n\nOut-of-scope (separate beads):\n  - axon-schema imports — sub-bead B1.2.\n  - axon-storage move of storage_adapter_store.rs — sub-bead B1.3.\n\nRollback: if the split is partial, revert all moved files; do not leave a half-extracted crate in the workspace. Either the new crate compiles or the change is reverted entirely.",
+    "acceptance": "AC1. crates/axon-cypher-ast/ exists with the parser/AST/validator code; cargo build -p axon-cypher-ast succeeds.\nAC2. axon-cypher re-exports parse, validate, plan, ast::*, schema::*, CypherError so existing callers compile without changes.\nAC3. cargo check --workspace passes.\nAC4. cargo test --workspace passes.\nAC5. cargo clippy --workspace -- -D warnings passes.\nAC6. cargo tree -p axon-cypher-ast -e normal shows no axon-* dep other than axon-core (axon-cypher-ast is a leaf w.r.t. workspace crates).",
+    "labels": [
+      "helix",
+      "feat-009",
+      "area:cypher",
+      "kind:refactor"
+    ],
+    "metadata": {
+      "claimed-at": "2026-05-08T01:22:59Z",
+      "claimed-machine": "sindri",
+      "claimed-pid": "2610707",
+      "events": [
+        {
+          "actor": "erik",
+          "body": "{\"rationale\":\"Well-formed task bead with clear title, typed labels, detailed description, and six verifiable acceptance criteria. Minor deductions: (1) no explicit 'priority' or 'effort' field to help queue ordering; (2) AC6 could be tightened — 'shows no axon-* dep other than axon-core' is slightly ambiguous about transitive deps vs direct deps, but acceptable for a lint pass; (3) the rollback note is good prose but not reflected as a formal AC, leaving partial-extraction detection untestable in CI.\",\"score\":97,\"suggested_fixes\":[\"Add a priority field (e.g., 'priority':'medium') so the queue can be sorted without manual inference.\",\"Add an effort/story-points field to support sprint planning.\",\"Consider making rollback a testable AC: 'AC7. No axon-cypher-ast directory exists in the workspace if AC1 fails (partial extraction is reverted).' — or accept that this is a process note and leave it in description only.\",\"Clarify AC6 to distinguish direct vs transitive: 'cargo tree -p axon-cypher-ast -e normal shows axon-core as the only workspace-internal dependency (direct or transitive).'\"],\"waivers_applied\":[]}",
+          "created_at": "2026-05-08T01:24:22.110107323Z",
+          "kind": "bead-quality.lint",
+          "source": "ddx agent execute-loop",
+          "summary": "score=97"
+        }
+      ],
+      "execute-loop-heartbeat-at": "2026-05-08T01:22:59.168971717Z",
+      "spec-id": "FEAT-009"
+    }
+  },
+  "governing": [
+    {
+      "id": "FEAT-009",
+      "path": "docs/helix/01-frame/features/FEAT-009-graph-traversal-queries.md",
+      "title": "Feature Specification: FEAT-009 — Unified Graph Query (Cypher)"
+    }
+  ],
+  "paths": {
+    "dir": ".ddx/executions/20260508T012422-7a41d5c9",
+    "prompt": ".ddx/executions/20260508T012422-7a41d5c9/prompt.md",
+    "manifest": ".ddx/executions/20260508T012422-7a41d5c9/manifest.json",
+    "result": ".ddx/executions/20260508T012422-7a41d5c9/result.json",
+    "checks": ".ddx/executions/20260508T012422-7a41d5c9/checks.json",
+    "usage": ".ddx/executions/20260508T012422-7a41d5c9/usage.json",
+    "worktree": "tmp/ddx-exec-wt/.execute-bead-wt-axon-1202db7c-20260508T012422-7a41d5c9"
+  },
+  "prompt_sha": "e3fab3d06cc51dd0a542cb2985246f3bc813efe8bd02b52ba3e1c11dd1ee3a31"
+}
\ No newline at end of file
diff --git a/.ddx/executions/20260508T012422-7a41d5c9/result.json b/.ddx/executions/20260508T012422-7a41d5c9/result.json
new file mode 100644
index 0000000..5b853dd
--- /dev/null
+++ b/.ddx/executions/20260508T012422-7a41d5c9/result.json
@@ -0,0 +1,23 @@
+{
+  "bead_id": "axon-1202db7c",
+  "attempt_id": "20260508T012422-7a41d5c9",
+  "base_rev": "b63b86754707d9e2d9e9f3f1b5e1ca915993d2cd",
+  "result_rev": "82500f295d5cd6f296ffaefba23fa84565b7f351",
+  "outcome": "task_succeeded",
+  "status": "success",
+  "detail": "success",
+  "harness": "claude",
+  "model": "sonnet",
+  "session_id": "eb-fc4fa021",
+  "duration_ms": 1306669,
+  "tokens": 40,
+  "cost_usd": 2.9677235999999994,
+  "exit_code": 0,
+  "execution_dir": ".ddx/executions/20260508T012422-7a41d5c9",
+  "prompt_file": ".ddx/executions/20260508T012422-7a41d5c9/prompt.md",
+  "manifest_file": ".ddx/executions/20260508T012422-7a41d5c9/manifest.json",
+  "result_file": ".ddx/executions/20260508T012422-7a41d5c9/result.json",
+  "usage_file": ".ddx/executions/20260508T012422-7a41d5c9/usage.json",
+  "started_at": "2026-05-08T01:24:23.254013681Z",
+  "finished_at": "2026-05-08T01:46:09.92317848Z"
+}
\ No newline at end of file
</untrusted-data>
  </diff>

  <instructions>
You are reviewing a bead implementation against its acceptance criteria.

For each acceptance-criteria (AC) item, decide whether it is implemented correctly, then assign one overall verdict:

- APPROVE — every AC item is fully and correctly implemented.
- REQUEST_CHANGES — some AC items are partial or have fixable minor issues.
- BLOCK — at least one AC item is not implemented or incorrectly implemented; or the diff is insufficient to evaluate.

## Required output format (schema_version: 1)

Respond with EXACTLY one JSON object as your final response, fenced as a single ```json … ``` code block. Do not include any prose outside the fenced block. The JSON must match this schema:

```json
{
  "schema_version": 1,
  "verdict": "APPROVE",
  "summary": "≤300 char human-readable verdict justification",
  "findings": [
    { "severity": "info", "summary": "what is wrong or notable", "location": "path/to/file.go:42" }
  ]
}
```

Rules:
- "verdict" must be exactly one of "APPROVE", "REQUEST_CHANGES", "BLOCK".
- "severity" must be exactly one of "info", "warn", "block".
- Output the JSON object inside ONE fenced ```json … ``` block. No additional prose, no extra fences, no markdown headings.
- Do not echo this template back. Do not write the words APPROVE, REQUEST_CHANGES, or BLOCK anywhere except as the JSON value of the verdict field.
  </instructions>
</bead-review>
