---
ddx:
  id: CONTRACT-007
  depends_on:
    - ADR-021
    - FEAT-009
    - FEAT-018
    - FEAT-029
  review:
    self_hash: a8835666faac1d25c9af382cbf4f039ae51073521aa646210b12fcc99de92407
    deps:
      ADR-021: 7672758c3841fb3871bc2b8f90aeb7c63d5453c42dae5bedf5cf27d6394dda78
      FEAT-009: 08784dee672189395e039843c292e6513155f125f9c9ec50bb29f2cc593c7bca
      FEAT-018: 32736251fbe98379326a28a9517474ad1b69ba9cbfb29b710f2cfaab1d3b8d08
      FEAT-029: f548dd83b06d298a7e8c575870ae1a06e5e9c53e94d6ccb64b2b876daf7b3b0c
    reviewed_at: "2026-07-11T02:26:23Z"
---

# Contract

**Contract ID**: CONTRACT-007
**Type**: protocol + HTTP API (unified read-side query language surface)
**Version**: 0.1.0
**Status**: draft
**Related**: ADR-021, FEAT-009, FEAT-018, FEAT-013, FEAT-015, FEAT-016, FEAT-029, CONTRACT-002, CONTRACT-003, CONTRACT-004

## Purpose

Defines the normative V1 openCypher subset, the `axonQuery` GraphQL field,
the schema-declared named-query `queries:` block, and the stable query
error-code set. All read operations — filter, sort, aggregate, traversal,
pattern matching, neighbor and link-endpoint discovery — are expressed in
this one language and compile through one planner. Query clients, the
parser/planner, and surface generators implement against this document.

## Scope and Boundaries

- In scope: accepted/rejected Cypher constructs, named-query declaration
  shape and generated field shape, ad-hoc query field and result types,
  error codes, cost/depth limits, policy-enforcement obligations of the
  executor.
- Out of scope: GraphQL connection contract details (FEAT-015 / GraphQL
  surface contract CONTRACT-002), MCP tool envelope (CONTRACT-003), policy
  grammar itself (CONTRACT-004), planner internals and index storage
  (ADR-010/FEAT-013).
- Owning system: `axon-cypher`, with surface generation in `axon-graphql`
  and `axon-mcp`.

## Normative Surface

### V1 openCypher subset

The language is read-only. Anything not listed below MUST be rejected at
parse time with `unsupported_clause`.

Reading and matching:

- `MATCH (n)`, `MATCH (n:Label)`, `MATCH (n:Label {prop: value})`
- `MATCH (a)-[:REL]->(b)`, `MATCH (a)-[:REL|REL2]->(b)` (relationship-type
  alternation)
- `MATCH (a)-[r:REL]->(b)` (relationship variable binding for property
  access)
- `MATCH (a)-[:REL*1..3]->(b)` (variable-length paths; explicit lower AND
  upper bounds required)
- `OPTIONAL MATCH ...` (left-join semantics)
- `WHERE` predicates: comparisons, `AND`/`OR`/`NOT`, `IN`, `CONTAINS`,
  `STARTS WITH`, `ENDS WITH`, `IS NULL`, `IS NOT NULL`, `EXISTS { ... }`,
  `NOT EXISTS { ... }`

Projection and aggregation:

- `RETURN n`, `RETURN n.field`, `RETURN n.field AS alias`
- `RETURN DISTINCT ...`
- Aggregations: `count(*)`, `count(n)`, `sum`, `avg`, `min`, `max`, `collect`
- `WITH ... AS ...` (pipelining and intermediate aggregation)
- `ORDER BY ... ASC|DESC`, `SKIP`, `LIMIT`

Parameters and types:

- Parameters: `$paramName`, bound by the GraphQL/MCP caller
- Literals: string, integer, float, boolean, null, list, map
- Property types follow the ESF schema; type mismatch rejected at parse time

Excluded from V1 (MUST be rejected):

- All write clauses: `CREATE`, `MERGE`, `SET`, `DELETE`, `REMOVE`,
  `DETACH DELETE`
- Unbounded variable-length paths (`*`, `*1..`)
- `shortestPath()`, `allShortestPaths()`
- `CALL { subquery }` and procedure calls
- User-defined functions
- `LOAD CSV`, `USING INDEX`, `USING SCAN`, planner hints
- Map projections `n {.foo, .bar}`
- `UNION` / `UNION ALL`

References to any label, property, or relationship type not present in the
active schema MUST be rejected at parse time with `unknown_label`.

### Ad-hoc query field

```graphql
type Query {
  axonQuery(cypher: String!, parameters: JSON): AxonQueryResult!
}

type AxonQueryResult {
  rows: [JSON!]!
  schema: AxonQuerySchema!     # result column types and nullability
  metadata: AxonQueryMetadata! # plan info, index usage, policy decisions
}
```

MCP mirrors this as the `axon.query(cypher, parameters)` tool with identical
parsing, planning, limits, and policy enforcement.

### Named-query declaration (`queries:` block)

A collection schema MAY declare named queries (ESF extension, FEAT-002):

```yaml
collection: ddx_beads
queries:
  <query_name>:
    description: <string>          # surfaced in GraphQL docs and MCP tool descriptions
    cypher: |                      # required; V1 subset only
      MATCH ...
      RETURN ...
    parameters:                    # required; may be []
      - name: <identifier>
        type: <GraphQL scalar, e.g. ID, String, Int>
        required: <bool>
```

At `put_schema` time the compiler MUST:

- type-check the query against the active collection schemas (labels,
  properties, relationship types exist);
- validate index usage — queries requiring unindexed scans above the hard
  threshold are rejected with a diagnostic suggesting an index declaration;
  there is no schema-declaration opt-out or request flag;
- validate policy compatibility — queries requiring policy bypass are
  rejected with `policy_required_bypass`;
- on activation, generate one typed GraphQL field on `Query` and one MCP
  tool on the collection's tool group.

Generated field shape: parameters become field arguments; results follow the
FEAT-015 connection contract (`edges`, `pageInfo`, `totalCount`, all
policy-filtered):

```graphql
type Query {
  beads_blocked_by(blocker_id: ID!, first: Int, after: String): DdxBeadConnection!
}
```

Named queries are subscribable through the FEAT-015 subscription path with
an initial snapshot on subscribe; ad-hoc queries are NOT subscribable in V1.

### Stable error codes

`axonQuery` and named-query execution errors MUST carry one of:

| Code | Condition |
|---|---|
| `unsupported_clause` | Query uses a construct outside the V1 subset |
| `unknown_label` | Label, property, or relationship type not in the active schema |
| `unsupported_query_plan` | Plan requires unindexed access on a collection above the hard threshold (1,000 entities) |
| `policy_required_bypass` | Query would require policy bypass to be useful (named-query compile and ad-hoc parse) |
| `query_too_large` | Planned worst-case cardinality exceeds the hard budget (1M intermediate rows) or in-memory materialization budget exceeded |
| `query_timeout` | Wall-clock timeout exceeded (30 seconds) |

The set is extend-only; codes MUST NOT be renamed or reused.

### Limits

| Limit | Default | Scope |
|---|---|---|
| Variable-length path depth cap | 10 | Hard V1 limit for named and ad-hoc queries; not configurable per database |
| Unindexed-scan collection threshold | 1,000 entities | Hard V1 limit for named and ad-hoc queries; no opt-out and no request flag |
| Worst-case cardinality budget | 1M intermediate rows | Hard V1 limit for named and ad-hoc queries |
| Wall-clock timeout | 30 seconds | Hard V1 limit for named and ad-hoc queries |

### Policy enforcement obligations

The executor MUST enforce FEAT-029 / CONTRACT-004 policy at every point data
flows into a result row:

1. Row policy applies at each label match, pushed into the index scan when
   indexable.
2. Field redaction applies at projection: redacted fields return `null` in
   `RETURN`/`WITH`, cannot be used as predicates in `WHERE`, and cannot leak
   through aggregations.
3. `EXISTS { ... }` is policy-aware: hidden targets MUST NOT leak via
   existence.
4. `count(*)` and aggregate counts are policy-filtered (matches FEAT-015
   `totalCount` semantics).
5. Relationship-variable properties are field-redacted like entity
   properties.

### Aggregation projections

Per product-owner decision (2026-06-10), FEAT-018 aggregation queries are
projections of this planner: every FEAT-018 aggregation MUST be expressible
in the V1 subset (`count`/`sum`/`avg`/`min`/`max`/`collect` with `WITH`
grouping) and MUST compile through the same plan, limits, and policy
enforcement. The GraphQL and MCP aggregation projections are surfaced per
CONTRACT-002 (GraphQL surface) and CONTRACT-003 (MCP surface); no separate
aggregation engine or grammar exists.

## Precedence and Compatibility

- Versioning: the subset is extend-only within V1; removing an accepted
  construct is a breaking change requiring a new language version. The
  excluded list is reviewed at V1 ship and again for V2.
- Writes: write clauses remain excluded; mutations flow through the
  GraphQL/MCP mutation-intent path (FEAT-030).
- Precedence: parse-time rejection (`unsupported_clause`, `unknown_label`)
  precedes plan-time rejection (`unsupported_query_plan`,
  `policy_required_bypass`, `query_too_large`), which precedes execution
  failures (`query_timeout`).
- Named queries are the safe default: pre-compiled, policy-bound, and
  index-validated at schema-write time; ad-hoc queries get the same
  validation at request time and never bypass policy or hard limits.
- Policy/schema snapshot: a query evaluates against the schema/policy
  snapshot active at query start.

## Error Semantics

| Condition | Error / Outcome | Retry | Recovery Expectation |
|-----------|------------------|-------|----------------------|
| Construct outside subset | `unsupported_clause` at parse | no | Rewrite within the V1 subset |
| Unknown label/property/relationship | `unknown_label` at parse | no | Use schema-declared names |
| Unindexed plan above threshold | `unsupported_query_plan` + diagnostics naming the required index | yes (after schema change) | Declare a FEAT-013 index or rewrite the query; there is no opt-in or opt-out path |
| Query needs policy bypass | `policy_required_bypass` (typed diagnostic) | no | Restructure the query; policy is never bypassed |
| Cardinality or memory budget exceeded | `query_too_large` | yes (narrower query) | Add predicates/limits or rewrite the query |
| Timeout | `query_timeout` after 30 seconds default | yes | Narrow the query; there is no timeout opt-out |

## Examples

```yaml
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

```graphql
query {
  axonQuery(
    cypher: "MATCH (b:DdxBead {status: $status}) RETURN b.id, b.title ORDER BY b.priority DESC LIMIT 20"
    parameters: { status: "open" }
  ) {
    rows
    schema { columns { name type nullable } }
    metadata { indexesUsed policyDecisions }
  }
}
```

Aggregation as a planner projection:

```cypher
MATCH (i:Invoice)
WHERE i.status IN ['approved', 'paid']
WITH i.vendor_id AS vendor, sum(i.amount_cents) AS total
RETURN vendor, total ORDER BY total DESC LIMIT 10
```

## Non-Normative Notes

The interpreted-streaming executor and the optional Apache AGE Postgres path
are implementation strategy (ADR-021), not contract. `AxonQuerySchema` and
`AxonQueryMetadata` field-level shapes are pinned by the GraphQL surface
contract (CONTRACT-002).

## Validation Checklist

- [ ] Normative fields and rules are explicit.
- [ ] Compatibility and precedence rules are explicit.
- [ ] Error handling is explicit.
- [ ] At least one executable test can be derived from this contract.
- [ ] Non-normative notes cannot be mistaken for contract requirements.
