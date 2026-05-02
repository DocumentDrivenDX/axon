---
ddx:
  id: FEAT-020
  depends_on:
    - FEAT-009
---
# Feature Specification: FEAT-020 — Link Discovery and Graph Queries

**Feature ID**: FEAT-020
**Status**: Retired (Superseded by FEAT-009)
**Priority**: —
**Owner**: Core Team
**Created**: 2026-04-05
**Retired**: 2026-05-02

## Status

This feature is **retired**. Its scope has been folded into FEAT-009
(Unified Graph Query) as of 2026-05-02 per ADR-020 (data model decision)
and ADR-021 (Cypher subset selection).

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

- **FEAT-009** — Unified Graph Query (Cypher).
- **ADR-020** — Data Model: Document-Shaped Entities, Not Native RDF.
- **ADR-021** — Graph Query Language: openCypher Subset.
