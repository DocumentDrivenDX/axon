---
ddx:
  id: ADR-021
  depends_on:
    - helix.prd
    - ADR-002
    - ADR-010
    - ADR-012
    - ADR-019
    - ADR-020
    - FEAT-002
    - FEAT-009
    - FEAT-013
    - FEAT-015
    - FEAT-016
    - FEAT-029
---
# ADR-021: Graph Query Language — openCypher Subset

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-05-02 | Accepted | Erik LaBianca | ADR-020, ADR-010, FEAT-009, FEAT-013, FEAT-015, FEAT-029 | High |

## Context

ADR-020 fixed Axon's data model as document-shaped with first-class entities
and links. This ADR records the read-side query language that runs on top of
that data model.

The PRD describes filter, sort, aggregate, traverse, and pattern as separate
query primitives (PRD §4 Query Model). In practice they share an underlying
planner and an underlying index strategy (FEAT-013), and exposing them
through different surfaces — GraphQL connection arguments for filter/sort,
traversal directives, neighbor queries, and ad-hoc patterns — multiplies
spec, test, and policy surface without any corresponding gain. They should
be expressed as one language with one planner.

The DDx consumer use case (axon-05c1019d) sharpened this: the ready-queue
query *"open beads with no open `depends_on` targets"* requires a single
round-trip predicate over outgoing-link target state. None of FEAT-009's
existing depth-bounded traversal, FEAT-020's link discovery, or FEAT-015's
GraphQL connection filters express this directly. A graph query language
does.

Four candidate languages were considered:

- **Cypher** (openCypher subset, with ISO/IEC 39075:2024 GQL on the horizon)
- **SPARQL 1.1** (W3C standard, RDF-shaped)
- **GraphQL with custom directives and computed predicates** (stay in current
  surface, declare every pattern as a schema-baked field)
- **Custom DSL** (named patterns only, small expression language, no general
  graph query language)

ADR-020 rejected RDF as Axon's primitive shape, which removes SPARQL from
contention as a *primary* language. The remaining choice is between Cypher,
GraphQL extensions, and a custom DSL.

## Decision

**Axon adopts a read-only subset of openCypher as the unified read-side query
language.** All read operations — filter, sort, aggregate, traversal, pattern
matching, neighbor discovery, and link endpoint discovery — compile to the
same query plan and run through the same planner. FEAT-013 secondary indexes
and the dedicated links table accelerate the planner.

Two surfaces expose the language:

1. **Schema-declared named queries.** A collection schema (FEAT-002 / ESF)
   may declare a `queries` block. Each named query is a parameterized
   openCypher pattern. The schema compiler typechecks the query against the
   collection schema, validates index usage and policy compatibility, and
   generates a typed GraphQL field exposing the result. Named queries are
   the safe default — they are pre-compiled, policy-bound, and indexable
   at schema-write time.

2. **Ad-hoc queries.** A generic GraphQL field `axonQuery(cypher: String!)`
   accepts a query string at request time. Ad-hoc queries are subject to
   policy enforcement, depth limits, cost limits, and reject any reference
   to a label/property/relationship not in the active schema. Ad-hoc queries
   never bypass policy.

Writes remain in the GraphQL/MCP mutation-intent flow (FEAT-030). Cypher's
write clauses (`CREATE`, `MERGE`, `SET`, `DELETE`, `REMOVE`) are not part
of the V1 subset.

## Supported subset (V1)

The V1 implementation supports the following openCypher constructs. Anything
not listed is rejected at parse time.

### Reading and matching

- `MATCH (n)`, `MATCH (n:Label)`, `MATCH (n:Label {prop: value})`
- `MATCH (a)-[:REL]->(b)`, `MATCH (a)-[:REL|REL2]->(b)` (relationship-type alternation)
- `MATCH (a)-[r:REL]->(b)` (binding a relationship to a variable for property access)
- `MATCH (a)-[:REL*1..3]->(b)` (variable-length paths with explicit lower and upper bounds)
- `OPTIONAL MATCH ...` (left-join semantics)
- `WHERE` predicates: comparisons, `AND`/`OR`/`NOT`, `IN`, `CONTAINS`,
  `STARTS WITH`, `ENDS WITH`, `IS NULL`, `IS NOT NULL`, `EXISTS { ... }`,
  `NOT EXISTS { ... }`

### Projection and aggregation

- `RETURN n`, `RETURN n.field`, `RETURN n.field AS alias`
- `RETURN DISTINCT ...`
- Aggregation functions: `count(*)`, `count(n)`, `sum`, `avg`, `min`, `max`,
  `collect`
- `WITH ... AS ...` for query pipelining and intermediate aggregation
- `ORDER BY ... ASC|DESC`, `SKIP`, `LIMIT`

### Parameters and types

- Parameters: `$paramName` references bound by the GraphQL/MCP caller
- Literal types: string, integer, float, boolean, null, list, map
- Property types follow the ESF schema; type mismatch at parse time

### Excluded from V1

- All write clauses: `CREATE`, `MERGE`, `SET`, `DELETE`, `REMOVE`,
  `DETACH DELETE`
- Unbounded variable-length paths (`*`, `*1..`) — must specify both lower
  and upper bounds
- `shortestPath()`, `allShortestPaths()`
- `CALL { subquery }` and procedure calls
- User-defined functions
- `LOAD CSV`, `USING INDEX`, `USING SCAN`, planner hints
- Map projections `n {.foo, .bar}` — use explicit `RETURN n.foo, n.bar` instead
- `UNION` / `UNION ALL` (revisit if a real use case appears)

The excluded set is reviewed when V1 ships and again for V2.

## Compilation strategy

### Multi-backend execution

Axon's storage trait (ADR-003, ADR-010) supports multiple backends. The
Cypher executor is **interpreted-streaming** by default: the parser produces
an AST, the planner produces a streaming pipeline of operators (scan, index
lookup, filter, expand, project, aggregate, sort, limit), and the executor
runs the pipeline against the storage trait. This works on every backend —
in-memory, SQLite, PostgreSQL, and any future KV store — because the
storage trait is the substrate.

A **PostgreSQL-native path via Apache AGE** is left as a future optimization,
not a V1 commitment. AGE compiles Cypher to recursive SQL on Postgres and
would offer better performance for deep traversals and large aggregations on
the Postgres backend specifically. Since the interpreted executor must
exist anyway for SQLite and KV backends, AGE is purely additive — easy to
add later when a Postgres deployment hits a perf ceiling.

### Index usage

The planner is rules-based, not cost-based, in V1. It applies these rules
in order:

1. **Label + property predicate** → use the FEAT-013 secondary index for
   the property. Example: `MATCH (b:Bead {status: 'open'})` uses the
   `string` index on `Bead.status`.
2. **Relationship traversal** → use the links table PK
   (`source_collection_id, source_id, link_type`) for outgoing, and the
   links target index (`target_collection_id, target_id, link_type`) for
   incoming.
3. **`EXISTS { (a)-[:R]->(b) }`** → predicate compiles to an index probe
   on the links table; presence/absence determined without materializing the
   target row.
4. **`ORDER BY n.field`** → if a single-field or compound index covers the
   sort field with a matching prefix, the index scan produces sorted output
   with no application-layer sort.
5. **No index match** → full collection scan with application-layer filter.
   Allowed for named queries with a sufficiently small collection. Rejected
   for ad-hoc queries unless explicitly opted in via a request flag and
   subject to a strict cost budget.

A query plan that requires unindexed access on a collection above a
configurable threshold (default: 1,000 entities) returns
`unsupported_query_plan` with diagnostics suggesting an index declaration.
This mirrors ADR-019's policy-compiler "unindexed plans rejected unless
bounded" rule.

### Cost and depth limits

- **Variable-length paths**: explicit lower/upper bounds required; default
  hard cap at depth 10 (configurable per database, mirroring FEAT-009's
  existing traversal depth budget).
- **Cardinality estimate**: the planner estimates cardinality from index
  statistics. Plans whose worst-case cardinality exceeds a configurable
  budget (default: 1M intermediate rows) are rejected for ad-hoc queries.
  Named queries can override the budget at schema-declaration time after
  operator review.
- **Wall-clock timeout**: every query carries a 30-second hard timeout
  (matches FEAT-008's transaction-timeout default; configurable).
- **Memory budget**: bounded streaming — operators yield rows incrementally;
  full materialization happens only at the boundaries that require it
  (`ORDER BY` without a covering index, `collect()`, `DISTINCT` on large
  cardinalities). Spilling to disk is V2.

## Policy integration

The Cypher executor must enforce FEAT-029 / ADR-019 policy at every point
where data flows into a result row, otherwise the language becomes a policy
bypass.

1. **Row policy at each label match.** `MATCH (n:Label)` must apply the
   row-policy predicate for `Label` before yielding `n`. Row-policy filters
   are pushed down into the index scan when indexable.
2. **Field redaction at projection.** `RETURN n.field` returns the redacted
   value (typically `null`) if policy redacts `field` for the current
   subject. Redaction is applied uniformly across `RETURN`, `WITH`,
   `WHERE` (a redacted field cannot be used as a predicate), and aggregations
   (a redacted field cannot leak via `count`, `sum`, etc.).
3. **`EXISTS` is policy-aware.** `EXISTS { (a)-[:R]->(b:Label) }` returns
   true only if there is a `b` reachable through `R` *that the current
   subject is allowed to see*. Hidden targets must not leak via existence.
   This is the highest-risk policy-bypass vector and gets dedicated
   contract tests.
4. **`count(*)` and aggregate counts are policy-filtered.**
   `count(*)` over a label returns the count of rows the current subject
   is allowed to see — hidden rows do not contribute. This matches
   FEAT-015's connection `totalCount` policy semantics.
5. **Relationship metadata is field-redacted.** Properties on relationship
   variables (`r.weight`, `r.created_at`) are subject to the same field-
   redaction rules as entity properties.

The schema compiler validates each named query against the active policy:
queries that require policy bypass to be useful are rejected at schema-write
time with a typed diagnostic (`policy_required_bypass`). Ad-hoc queries
get the same validation at parse time.

## GraphQL surfacing

### Named queries

A schema with:

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

generates a typed GraphQL field:

```graphql
type Query {
  ready_beads(first: Int, after: String): DdxBeadConnection!
}
```

The connection contract follows FEAT-015 — `edges`, `pageInfo`, `totalCount`,
all policy-filtered. The result type is the named query's `RETURN`
projection.

Parameterized queries surface their parameters as field arguments:

```yaml
queries:
  beads_blocked_by:
    cypher: |
      MATCH (b:DdxBead {status: 'open'})-[:DEPENDS_ON]->(d:DdxBead {id: $blocker_id})
      RETURN b
    parameters:
      - name: blocker_id
        type: ID
        required: true
```

→

```graphql
type Query {
  beads_blocked_by(blocker_id: ID!, first: Int, after: String): DdxBeadConnection!
}
```

### Ad-hoc queries

A single generic field:

```graphql
type Query {
  axonQuery(cypher: String!, parameters: JSON): AxonQueryResult!
}

type AxonQueryResult {
  rows: [JSON!]!
  schema: AxonQuerySchema!
  metadata: AxonQueryMetadata!
}
```

Returns rows as JSON because the result shape isn't statically typed at the
GraphQL level. The `schema` field describes the result column types and
nullability for clients that want to project safely. `metadata` reports
plan info, index usage, and policy decisions for debugging.

### Subscriptions

Named queries support live subscriptions:

```graphql
type Subscription {
  ready_beads: DdxBeadConnection!
}
```

Implemented via the existing FEAT-015 subscription path; the change-feed
pipeline re-evaluates the named query when the underlying collections or
links change. Ad-hoc queries do *not* support subscriptions in V1 — the
re-evaluation overhead and policy-snapshotting model are too expensive
for runtime-defined queries.

## MCP surfacing

FEAT-016 / ADR-013. MCP exposes:

- `axon.query(cypher, parameters)` — generic ad-hoc query tool, mirrors
  GraphQL `axonQuery`.
- One tool per named query — generated per collection, named after the
  query (e.g. `ddx_beads.ready_beads`). Same parameter shape as the GraphQL
  field.

Both tools enforce the same policy and limits as the GraphQL surface. Tool
descriptions include the named query's documentation string so agents can
discover them.

## Alternatives

| Option | Pros | Cons | Evaluation |
|---|---|---|---|
| **A. openCypher subset (read-only)** | Best entity+link fit; mature ecosystem (Neo4j, Memgraph, AGE); ISO 39075:2024 GQL emerging; clean policy compilation; LLMs generate Cypher reliably | Cypher is not yet ISO-standard (GQL is); we are betting on the openCypher / GQL convergence | **Selected** |
| B. SPARQL 1.1 | W3C standard with formal algebra; built-in federation; powerful property paths | Ruled out by ADR-020; triple-pattern verbosity; agent persona mismatch | Rejected |
| C. GraphQL extensions (directives, computed predicates) | Stays in current surface; no new language | Anaemic — every graph pattern requires a schema declaration; complex traversals (`*1..N`) inexpressible; ad-hoc graph queries impossible | Rejected |
| D. Custom DSL (named patterns only) | Smallest implementation; tightest control | Not a query language — pre-canned views only; closes the door on ad-hoc exploration; users will reach for SQL or build their own DSL on top | Rejected |
| E. Cypher subset with writes | Single language for read and write | Massive policy/intent integration cost; conflicts with FEAT-030 mutation-intent flow; V2+ at earliest | Deferred |

## Consequences

| Type | Impact |
|------|--------|
| Positive | One read-side language. One planner. One policy enforcement point. FEAT-013 indexes accelerate every read path. DDx ready/blocked queue solvable in a single round-trip. Cypher is well-known to LLMs and developers. ISO GQL alignment provides a forward path. |
| Negative | We are betting on openCypher / GQL convergence; if GQL diverges from openCypher significantly, our subset may need adjustment. Ad-hoc queries add a real cost-and-policy-bypass attack surface that requires careful contract testing. Cypher's `EXISTS` semantics with policy enforcement is the highest-risk subsystem. |
| Neutral | FEAT-009 absorbs FEAT-020 and rewrites as the unified-graph-query feature. PRD §4 Query Model is updated to show Cypher examples. New crate `crates/axon-cypher`. Existing GraphQL/MCP surfaces gain a new field/tool but do not lose any current ones. |

## Implementation impact

Implementation lives in:

- **`crates/axon-cypher/`** (new) — parser, AST, plan, planner, executor.
- **`crates/axon-schema/`** — adds support for the `queries:` block; named-
  query type-checking against the collection schema.
- **`crates/axon-graphql/`** — `axonQuery` resolver; per-named-query field
  generation; subscription wiring for named queries.
- **`crates/axon-mcp/`** — `axon.query` tool; per-named-query tool generation.
- **`crates/axon-storage/`** — no changes required; the executor uses the
  existing storage trait for reads and the existing FEAT-013 index access
  paths.

Test surfaces:

- **Parser unit tests** — every supported clause; every rejection path.
- **Planner tests** — index selection rules; cost/depth budget rejection;
  schema-typecheck rejection.
- **Executor tests** — correctness on memory + SQLite backends; streaming
  bounded memory; timeout enforcement.
- **Policy contract tests** (extend FEAT-029 suite) — row policy at each
  label, field redaction at projection, `EXISTS` hidden-existence safety,
  policy-filtered `count(*)`, redacted properties in aggregations.
- **DDx benchmark** (closes axon-05c1019d) — ready/blocked queue at 1k
  and 10k beads with the latency targets in PRD §4 / FEAT-009.

## Open questions

- **GQL convergence risk.** ISO 39075:2024 (GQL) is published; openCypher
  is its primary input. We track GQL ratification and adjust the V1 subset
  if GQL diverges meaningfully. Low risk but worth watching.
- **Apache AGE adoption.** Pure optimization. Defer the decision to when
  the Postgres backend has real production load and we can measure benefit
  vs. dependency cost.
- **Spilling to disk for large `ORDER BY` / `collect()`.** V1 returns
  `query_too_large` if the in-memory budget is exceeded. V2 introduces
  spilling.
- **`UNION` / `UNION ALL`.** Excluded from V1. Add when a real use case
  appears.

## References

- openCypher: https://opencypher.org/
- Apache AGE (Cypher on Postgres): https://age.apache.org/
- Memgraph (Cypher implementation): https://memgraph.com/
- ISO/IEC 39075:2024 (GQL — published April 2024)
- ADR-020: Data Model — Document-Shaped Entities, Not Native RDF
- ADR-010: Physical Storage and Secondary Indexes
- ADR-019: Policy Authoring and Mutation Intents
- FEAT-009: Unified Graph Query (rewrite, this round)
- FEAT-013: Secondary Indexes and Query Acceleration
- FEAT-015: GraphQL Query Layer
- FEAT-016: MCP Server
- FEAT-029: Access Control (Policy)
