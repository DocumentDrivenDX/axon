# Supply Chain BOM Reel

Release target: Axon 0.7.1

ERP-style bill-of-materials traversal, recursive graph queries, reachability checks, aggregation, and link metadata.

Sample project: `examples/supply-chain-bom`

## Storyboard

1. Create a finished good, sub-assembly, component parts, and a build order.
2. Attach contains links that encode the BOM graph.
3. Traverse to depth three, then aggregate component demand.
4. Use reachability checks to catch dependency cycles before commit.

## Coverage Entries

- feature: FEAT-009 - Unified Graph Query (Cypher)
- feature: FEAT-018 - Aggregation Queries
- scenario: SCN-004 - ERP - BOM Explosion via Recursive Traversal
- story: US-023 - Traverse a Dependency Graph
- story: US-024 - Explode a Bill of Materials
- story: US-025 - Check Reachability
- story: US-062 - Count Entities by Field
- story: US-063 - Compute Numeric Aggregations
- story: US-064 - Aggregate via GraphQL
- story: US-065 - Aggregate via MCP
- story: US-070 - Find Link Targets
- story: US-071 - List Entity Neighbors
- story: US-072 - Explore Graph via GraphQL
- story: US-073 - Discover Links via MCP
- story: US-074 - Pattern Query for Ready/Blocked Queue
- story: US-075 - Schema-Declared Named Query
- story: US-076 - Ad-hoc Cypher Query
- story: US-077 - Subscribe to a Named Query
- use_case: USE-005 - ERP (Enterprise Resource Planning)
