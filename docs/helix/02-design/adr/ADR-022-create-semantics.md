---
ddx:
  id: ADR-022
  depends_on:
    - ADR-004
    - ADR-012
    - FEAT-004
    - FEAT-008
  review:
    self_hash: c69f9d446653d563314eba51e7718aff6e33217d0063aae1b3620238fa276e7d
    deps:
      ADR-004: 4c8ee66b80980bed2298d511d223f7faaace03864610faf8333af8659c4ce570
      ADR-012: cea81e56e4101b53f6b9a2e98c796278756bc657b895398ae226b6bc4f1f0188
      FEAT-004: 1ba0ba90778c2e6b4a38b11632d8ca73d3b328ac19ad326e151534c26ecd0b46
      FEAT-008: de4e47fda5c2045ef2c4765371cac1caf29353ec4b5c78dbffb651d02b6eab82
    reviewed_at: "2026-06-14T04:39:42Z"
---
# ADR-022: Create Semantics — Storage Upsert, Strict Create at Typed Surfaces

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-04-29 | Accepted | Erik LaBianca | ADR-004, ADR-012, FEAT-004, FEAT-008, CONTRACT-001 | High |

> Promoted 2026-06-10 from `02-design/decisions/create-semantics.md` to ADR
> form; the decision content is unchanged.

## Context

The storage adapters implement `put` as an idempotent upsert: SQLite,
PostgreSQL, and memory all write through without rejecting an existing row,
and the adapter docs describe `put` as the standard persistence operation
rather than a uniqueness guard. Above storage, transaction commits already
reject staged `create` operations for existing IDs — so a strict
"create must fail if entity exists" path existed in the domain layer while
HTTP `/entities` POST and gRPC `CreateEntity` followed the upsert path. The
typed GraphQL `createXxx` surface needed a decided contract: strict create or
upsert?

| Aspect | Description |
|--------|-------------|
| Problem | Duplicate-ID behavior on create differed by surface with no governing decision; nexiq routed around it via `commitTransaction` |
| Current State | Storage `put` = upsert; transaction `op:create` rejects duplicates; HTTP/gRPC create = upsert; typed GraphQL create behavior undecided |
| Requirements | One documented contract per surface; no breaking change to HTTP/gRPC callers relying on overwrite semantics |
| Decision Drivers | Storage contract already means overwrite; strict duplicate check already exists above storage; smallest behavioral change with clearest contract |

## Decision

We will adopt **Pattern B**: keep the storage adapter `put` contract as
overwrite/upsert, and apply strict duplicate rejection only at the surfaces
that already route through transaction-style create validation — typed
GraphQL `createXxx` and `commitTransaction` `op:create`. HTTP `/entities`
POST and gRPC `CreateEntity` remain documented upsert/create-or-replace
operations.

**Key Points**: Storage `put` stays a pure overwrite contract | Strict create
lives in the domain layer (typed GraphQL create, transaction `op:create`) |
HTTP/gRPC transport semantics preserved

### Per-surface duplicate-ID behavior

The normative per-surface duplicate-ID behavior table is now owned by
[CONTRACT-001](../contracts/CONTRACT-001-http-api-surface.md) (with
[CONTRACT-002](../contracts/CONTRACT-002-graphql-surface.md) for the typed
GraphQL create mutations); the decision-time record is: typed GraphQL
`createXxx` and `commitTransaction` `op:create` **reject** duplicates; HTTP
POST, gRPC `CreateEntity`, and storage `put` **overwrite/upsert**.

## Alternatives

| Option | Pros | Cons | Evaluation |
|--------|------|------|------------|
| Pattern A: storage `put` rejects duplicates (strict create everywhere) | One uniform semantic across all surfaces | Turns the adapter into a semantics layer for all callers; changes HTTP and gRPC behavior; likely breaks callers relying on overwrite | Rejected: largest blast radius for the least need |
| **Pattern B: storage upsert, strict create at typed GraphQL + transaction surfaces** | Matches existing storage abstraction; strict path already exists in `commit_transaction`; preserves transport semantics | Duplicate-ID behavior differs by surface and must be clearly documented | **Selected: smallest behavioral change with clearest contract** |

## Consequences

| Type | Impact |
|------|--------|
| Positive | No storage-layer or transport-layer behavior change; the strict-create contract is enforced where transactional create validation already runs |
| Positive | nexiq migration cost is contained: staying on `commitTransaction` costs zero; HTTP/gRPC callers need no migration |
| Negative | Per-surface divergence: documentation and tests must state which surfaces upsert and which reject (tracked by the follow-up bead) |
| Neutral | nexiq tests assuming typed GraphQL `createXxx` upserts must be updated/unskipped to the strict-create contract |

## Risks

| Risk | Prob | Impact | Mitigation |
|------|------|--------|------------|
| Callers assume one create semantic across surfaces | M | M | CONTRACT-001/002 document per-surface behavior; contract tests per surface |
| New surfaces (SDK, CLI) pick the wrong default | L | M | CONTRACT-008/009 inherit the per-surface table from CONTRACT-001 |

## Validation

| Success Metric | Review Trigger |
|----------------|----------------|
| Typed GraphQL `createXxx` and `commitTransaction op:create` reject duplicate IDs; HTTP/gRPC create upserts | Any contract-test divergence |
| Documentation and tests state the per-surface contract explicitly | Caller confusion reports |

## Supersession

- **Supersedes**: None
- **Superseded by**: None

## Concern Impact

- **Concern selection**: Constrains the API-surface concern: storage adapters
  stay semantics-free; create-strictness is a domain/surface concern.
- **Practice override**: None.

## References

- [ADR-004: Transaction Model](ADR-004-transaction-model.md)
- [ADR-012: GraphQL Query Layer](ADR-012-graphql-query-layer.md)
- [CONTRACT-001: HTTP API Surface](../contracts/CONTRACT-001-http-api-surface.md)
- [CONTRACT-002: GraphQL Surface](../contracts/CONTRACT-002-graphql-surface.md)
- [FEAT-004: Entity Operations](../../01-frame/features/FEAT-004-entity-operations.md)
- [FEAT-008: ACID Transactions](../../01-frame/features/FEAT-008-acid-transactions.md)
- Follow-up implementation bead: document/test the per-surface create
  contract (typed GraphQL + transaction strict; HTTP/gRPC/storage upsert)
