---
title: Supply Chain BOM Reel
weight: 13
prev: ../
---

# Supply Chain BOM Reel

Release target: Axon 0.7.1

ERP-style bill-of-materials traversal, recursive graph queries, reachability checks, aggregation, and link metadata.

- Sample project: [supply-chain-bom](https://github.com/DocumentDrivenDX/axon/tree/master/examples/supply-chain-bom)
- Script source: [`docs/demos/reels/supply-chain-reel.md`](https://github.com/DocumentDrivenDX/axon/blob/master/docs/demos/reels/supply-chain-reel.md)
- Coverage entries: 19

## Storyboard

1. Create a finished good, sub-assembly, component parts, and a build order.
2. Attach contains links that encode the BOM graph.
3. Traverse to depth three, then aggregate component demand.
4. Use reachability checks to catch dependency cycles before commit.

## Covered HELIX Entries

| Type | ID | Title | Source | Sample | Demo reel |
|---|---|---|---|---|---|
| feature | FEAT-009 | Unified Graph Query (Cypher) | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/features/FEAT-009-unified-graph-query.md) | [supply-chain-bom](https://github.com/DocumentDrivenDX/axon/tree/master/examples/supply-chain-bom) | [supply-chain-reel](../supply-chain-reel/) |
| feature | FEAT-018 | Aggregation Queries | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/features/FEAT-018-aggregation-queries.md) | [supply-chain-bom](https://github.com/DocumentDrivenDX/axon/tree/master/examples/supply-chain-bom) | [supply-chain-reel](../supply-chain-reel/) |
| scenario | SCN-004 | ERP - BOM Explosion via Recursive Traversal | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/03-test/test-plan.md) | [supply-chain-bom](https://github.com/DocumentDrivenDX/axon/tree/master/examples/supply-chain-bom) | [supply-chain-reel](../supply-chain-reel/) |
| story | US-023 | Traverse a Dependency Graph | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-023-traverse-a-dependency-graph.md) | [supply-chain-bom](https://github.com/DocumentDrivenDX/axon/tree/master/examples/supply-chain-bom) | [supply-chain-reel](../supply-chain-reel/) |
| story | US-024 | Explode a Bill of Materials | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-024-explode-a-bill-of-materials.md) | [supply-chain-bom](https://github.com/DocumentDrivenDX/axon/tree/master/examples/supply-chain-bom) | [supply-chain-reel](../supply-chain-reel/) |
| story | US-025 | Check Reachability | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-025-check-reachability.md) | [supply-chain-bom](https://github.com/DocumentDrivenDX/axon/tree/master/examples/supply-chain-bom) | [supply-chain-reel](../supply-chain-reel/) |
| story | US-062 | Count Entities by Field | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-062-count-entities-by-field.md) | [supply-chain-bom](https://github.com/DocumentDrivenDX/axon/tree/master/examples/supply-chain-bom) | [supply-chain-reel](../supply-chain-reel/) |
| story | US-063 | Compute Numeric Aggregations | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-063-compute-numeric-aggregations.md) | [supply-chain-bom](https://github.com/DocumentDrivenDX/axon/tree/master/examples/supply-chain-bom) | [supply-chain-reel](../supply-chain-reel/) |
| story | US-064 | Aggregate via GraphQL | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-064-aggregate-via-graphql.md) | [supply-chain-bom](https://github.com/DocumentDrivenDX/axon/tree/master/examples/supply-chain-bom) | [supply-chain-reel](../supply-chain-reel/) |
| story | US-065 | Aggregate via MCP | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-065-aggregate-via-mcp.md) | [supply-chain-bom](https://github.com/DocumentDrivenDX/axon/tree/master/examples/supply-chain-bom) | [supply-chain-reel](../supply-chain-reel/) |
| story | US-070 | Find Link Targets | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-070-find-link-targets.md) | [supply-chain-bom](https://github.com/DocumentDrivenDX/axon/tree/master/examples/supply-chain-bom) | [supply-chain-reel](../supply-chain-reel/) |
| story | US-071 | List Entity Neighbors | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-071-list-entity-neighbors.md) | [supply-chain-bom](https://github.com/DocumentDrivenDX/axon/tree/master/examples/supply-chain-bom) | [supply-chain-reel](../supply-chain-reel/) |
| story | US-072 | Explore Graph via GraphQL | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-072-explore-graph-via-graphql.md) | [supply-chain-bom](https://github.com/DocumentDrivenDX/axon/tree/master/examples/supply-chain-bom) | [supply-chain-reel](../supply-chain-reel/) |
| story | US-073 | Discover Links via MCP | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-073-discover-links-via-mcp.md) | [supply-chain-bom](https://github.com/DocumentDrivenDX/axon/tree/master/examples/supply-chain-bom) | [supply-chain-reel](../supply-chain-reel/) |
| story | US-074 | Pattern Query for Ready/Blocked Queue | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-074-pattern-query-for-ready-blocked-queue.md) | [supply-chain-bom](https://github.com/DocumentDrivenDX/axon/tree/master/examples/supply-chain-bom) | [supply-chain-reel](../supply-chain-reel/) |
| story | US-075 | Schema-Declared Named Query | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-075-schema-declared-named-query.md) | [supply-chain-bom](https://github.com/DocumentDrivenDX/axon/tree/master/examples/supply-chain-bom) | [supply-chain-reel](../supply-chain-reel/) |
| story | US-076 | Ad-hoc Cypher Query | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-076-ad-hoc-cypher-query.md) | [supply-chain-bom](https://github.com/DocumentDrivenDX/axon/tree/master/examples/supply-chain-bom) | [supply-chain-reel](../supply-chain-reel/) |
| story | US-077 | Subscribe to a Named Query | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-077-subscribe-to-a-named-query.md) | [supply-chain-bom](https://github.com/DocumentDrivenDX/axon/tree/master/examples/supply-chain-bom) | [supply-chain-reel](../supply-chain-reel/) |
| use case | USE-005 | ERP (Enterprise Resource Planning) | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/00-discover/use-case-research.md) | [supply-chain-bom](https://github.com/DocumentDrivenDX/axon/tree/master/examples/supply-chain-bom) | [supply-chain-reel](../supply-chain-reel/) |
