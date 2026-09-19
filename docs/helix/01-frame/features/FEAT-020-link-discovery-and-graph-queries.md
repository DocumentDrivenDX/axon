---
ddx:
  id: FEAT-020
  depends_on:
    - helix.prd
    - FEAT-009
  review:
    self_hash: e6114f00749d8d9f548b7a400069f2d042d245926439e21ad11260795957fca1
    deps:
      FEAT-009: 08784dee672189395e039843c292e6513155f125f9c9ec50bb29f2cc593c7bca
      helix.prd: 6703170c71275bba7d108c4f9c329d32e4104f9c965278db888ad43cdc3ca367
    reviewed_at: "2026-07-11T04:03:36Z"
---
# Feature Specification: FEAT-020 — Link Discovery and Graph Queries

**Feature ID**: FEAT-020
**Status**: superseded
**Priority**: —
**Owner**: Core Team
**Superseded**: 2026-05-02 (by FEAT-009)

## Status

This feature is **superseded**. Its scope has been folded into
[FEAT-009 — Unified Graph Query](FEAT-009-unified-graph-query.md) as of
2026-05-02 per ADR-020 (data model decision) and ADR-021 (Cypher subset
selection).

The reasoning: link discovery, neighbor queries, and graph exploration are
not separate primitives from filter, sort, aggregate, and traversal. They
all share a query planner, an index strategy, and a policy enforcement
point. Maintaining them as separate features multiplied spec, test, and
policy surface without any corresponding gain.

## Where the user stories now live

| Original story | New home |
|---|---|
| US-070: Find Link Targets | FEAT-009 US-070 |
| US-071: List Entity Neighbors | FEAT-009 US-071 |
| US-072: Explore Graph via GraphQL | FEAT-009 US-072 |
| US-073: Discover Links via MCP | FEAT-009 US-073 |

The structured-API and GraphQL/MCP shapes described in this feature have
been replaced by the openCypher-subset surface in FEAT-009. Named queries
in the collection schema express link-candidate and neighbor lookups as
typed GraphQL fields and MCP tools; the underlying planner uses the same
links-table indexes (ADR-010) the original FEAT-020 design called out.

## See

- [FEAT-009 — Unified Graph Query (Cypher)](FEAT-009-unified-graph-query.md)
- **ADR-020** — Data Model: Document-Shaped Entities, Not Native RDF.
- **ADR-021** — Graph Query Language: openCypher Subset.
