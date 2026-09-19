---
ddx:
  id: ADR-025
  depends_on:
    - ADR-014
    - FEAT-021
    - FEAT-032
  links:
    references:
      - CONTRACT-006
      - CONTRACT-009
  review:
    self_hash: 44fd65fb85d5dfc58aa7ec04039f44f397e66a45a1095738acff0fd6fafb79ef
    deps:
      ADR-014: 6b9f2190081dd7dae202942b25247ee638b0359a4ead7109987b5bc4440c7347
      FEAT-021: 6165a271de0b5e5c978f97ab9393596e651a680c51db80153fb85167ed93d993
      FEAT-032: 8102df1f7f6c66bd3b06f2158d7eb719547aad0f2b5c71d2867fe5aba9e0a3f2
    reviewed_at: "2026-07-11T04:22:34Z"
---
# ADR-025: Client-Projection Cursor API — Opaque, Restart/Schema-Stable Resume Tokens

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-06-27 | Accepted | Erik LaBianca | ADR-014, FEAT-032, FEAT-021, CONTRACT-006, CONTRACT-009 | High |

> **Scope.** This ADR records only the *new* decision: the SDK
> client-projection cursor API. It does **not** restate the change-feed
> envelope, topic naming, snapshot procedure, or audit-as-source-of-truth
> decision — those live in [ADR-014](ADR-014-change-feeds-debezium-cdc.md) and
> [CONTRACT-006](../contracts/CONTRACT-006-cdc-envelope.md). The client local
> read replica is FEAT-032.

## Context

| Aspect | Description |
|--------|-------------|
| Problem | The governed local read replica (FR-32, FEAT-032) needs to bootstrap and then resume the change stream across disconnects, server restarts, schema-compatible migrations, and policy/auth epoch changes. The token codec and storage-backed cursor store already exist in-tree (`crates/axon-audit/src/cursor_token.rs`, `crates/axon-storage/src/cursor_store.rs`), but the GraphQL subscription resume point is still a raw `audit_id` string (`crates/axon-graphql/src/subscriptions.rs`) and the end-to-end wiring is partial/unwired. A raw `audit_id` leaks internal sequence numbering and is not contractually restart/schema-stable. |
| Current State | ADR-014 established CDC as a projection of the audit log with `audit_id` as the offset. CONTRACT-006 §Cursor semantics already *requires* an opaque cursor token that stays valid across producer restarts and schema-compatible migrations, and purges it on incompatible schema, policy, or auth epoch changes; §Cursor vocabulary parity requires one vocabulary across GraphQL, MCP, SDK, and CDC. The implementation now has the token/store primitives, but not every consumer is wired to them yet. |
| Requirements | Opaque resume tokens; tokens valid across server restart and schema-compatible migration; incompatible schema/policy/auth epoch changes purge and force rebootstrap; snapshot+tail bootstrap; one cursor vocabulary across GraphQL subscriptions, MCP resource notifications, SDK change readers, and CDC sinks; durable cursor storage. |
| Decision Drivers | FEAT-032 cannot rely on raw `audit_id` (leaky, not schema-stable); CONTRACT-006 already mandates an opaque token and explicit invalidation rules, so the implementation must converge to it; one vocabulary avoids per-surface resume logic drift. |

## Decision

We will define a single **client-projection cursor API**, surfaced through the
SDK (CONTRACT-009) and shared by all stream consumers, with these properties:

**Key Points**:
- **Opaque tokens** — the resume token is a random, opaque, server-resolved
  string, not a raw `audit_id`. Clients store and replay it verbatim; its
  internal structure (audit offset + scope + sink/consumer identity) is
  server-owned per CONTRACT-006 §Cursor token.
- **Restart- and schema-compatible stable** — a token issued before a server
  restart or a schema-compatible migration of the scoped collections remains
  valid afterward; resuming with it neither errors nor silently skips events.
- **Incompatible changes purge and rebootstrap** — incompatible schema, policy,
  or auth epoch changes invalidate outstanding tokens; clients discard them and
  rebootstrap from a fresh snapshot rather than guessing at replay position.
- **Snapshot + tail** — first subscription returns a snapshot (`op: "r"`) for
  the subject-scoped data, then a token positioned at the snapshot boundary
  from which the client tails live events with no gap (CONTRACT-006 §Snapshot).
- **One cursor vocabulary** — GraphQL subscriptions, MCP resource
  notifications, SDK change readers, and CDC sinks use the same token vocabulary
  (CONTRACT-006 §Cursor vocabulary parity). The raw `audit_id` resume point in
  GraphQL subscriptions is replaced by this opaque token.
- **Surface wiring is partial/unwired today** — the token codec and durable
  cursor store already exist in-tree, but the raw `audit_id` reconnect path
  still needs to be retired everywhere so every consumer speaks the same
  vocabulary.
- **Pre-1.0 hard cut** — because this line is pre-1.0, a hard cursor cut is
  allowed when the token codec or replay epoch cannot be bridged, but it must
  be explicit: purge the old token and require rebootstrap.
- **Durable backing** — tokens are honored by a durable `CdcCursorStore`
  backend so resume survives server restarts (CONTRACT-006 §Cursor semantics).
  The backend itself exists in-tree; the remaining work is converging every
  consumer onto it.

Normative token format, expiry/aging, and SDK method shape are owned by
CONTRACT-006 and CONTRACT-009; this ADR fixes the *decision* that the token is
opaque, random, server-resolved, shared, restart/schema-compatible stable, and
purged on incompatible schema, policy, or auth epoch changes, not its byte
layout.

## Alternatives

| Option | Pros | Cons | Evaluation |
|--------|------|------|------------|
| Keep raw `audit_id` as the resume token | Already implemented; trivial | Leaks internal sequence numbering; not contractually schema-stable; violates CONTRACT-006 §Cursor token; couples clients to storage internals | Rejected: contradicts the existing contract and is not schema-stable |
| Per-surface resume tokens (GraphQL vs MCP vs SDK vs CDC each different) | Each surface optimizes its own format | Four divergent resume implementations to keep correct; breaks CONTRACT-006 §Cursor vocabulary parity; replica behaves differently per transport | Rejected: vocabulary drift is exactly what parity exists to prevent |
| **Single opaque, restart/schema-stable token shared across surfaces, durable-backed** | One implementation; matches CONTRACT-006; hides internals; works for the local replica and every existing consumer | Requires building the durable cursor backend and a token codec | **Selected: satisfies FEAT-032 and converges the implementation onto the existing contract** |

## Consequences

| Type | Impact |
|------|--------|
| Positive | FEAT-032 local replica gets correct resume across restart/schema change; GraphQL/MCP/SDK/CDC share one resume vocabulary; storage internals stay hidden from clients; the implementation converges onto CONTRACT-006 rather than diverging from it |
| Negative | Requires implementing a durable `CdcCursorStore` backend and an opaque-token codec; the raw-`audit_id` resume path in `subscriptions.rs` must be migrated and then hard-cut at the pre-1.0 boundary |
| Neutral | No change to the Debezium envelope, topic naming, or audit-as-source-of-truth decisions (ADR-014 stands) |

## Risks

| Risk | Prob | Impact | Mitigation |
|------|------|--------|------------|
| Migrating existing `audit_id` resume callers breaks live subscribers | M | M | Pre-1.0 allows a hard cursor cut; make the cut explicit and require rebootstrap rather than preserving a long compatibility window |
| Token must encode enough scope to be schema-stable, growing in size | L | L | Token is opaque and server-resolved; keep scope server-side keyed by a compact handle |
| Durable cursor backend adds write load | L | L | Cursor writes are low-frequency (per-batch, not per-event); reuse the storage adapter |

## Validation

| Success Metric | Review Trigger |
|----------------|----------------|
| A token issued before a server restart resumes correctly afterward | Any restart-resume failure |
| A token issued before a schema-compatible migration resumes correctly afterward | Any schema-compatible resume failure |
| An incompatible schema/policy/auth epoch change purges the old token and forces rebootstrap | Any stale-token reuse after incompatible change |
| GraphQL, MCP, SDK, and CDC consumers resume with the same token vocabulary | Any surface requiring a bespoke resume token |
| No client ever receives a raw internal `audit_id` as its resume handle | A raw `audit_id` is exposed as a client resume token |

## Supersession

- **Supersedes**: None. Replaces the raw-`audit_id` GraphQL subscription resume
  point with an opaque token (a refinement within ADR-014's projection model,
  not a supersession of ADR-014).
- **Superseded by**: None.

## Concern Impact

- **Concern selection**: Extends the change-feed projection concern (ADR-014) to
  a client-resident consumer; constrains all stream consumers to one opaque
  cursor vocabulary.
- **Practice override**: None.

## References

- [ADR-014: Change Feeds — Debezium-Compatible CDC](ADR-014-change-feeds-debezium-cdc.md) (referenced, not restated)
- [CONTRACT-006: CDC Envelope and Cursor Semantics](../contracts/CONTRACT-006-cdc-envelope.md)
- [CONTRACT-009: SDK Surface](../contracts/CONTRACT-009-sdk-surface.md)
- [FEAT-032: Local Replica Projection](../../01-frame/features/FEAT-032-local-replica-projection.md)
- [FEAT-021: Change Feeds (CDC)](../../01-frame/features/FEAT-021-change-feeds-cdc.md)
- PRD FR-32 (governed local read replica), FR-31 (resumable scoped change streams)

## Review Checklist

Use this checklist when reviewing an ADR:

- [x] Context names a specific problem — not "we need to decide about X"
- [x] Decision statement is actionable — "we will" not "we should consider"
- [x] At least two alternatives were evaluated
- [x] Each alternative has concrete pros and cons, not vague assessments
- [x] Selected option's rationale explains why it wins over the best alternative
- [x] Consequences include both positive and negative impacts
- [x] Negative consequences have documented mitigations
- [x] Risks are specific with probability and impact assessments
- [x] Validation section defines how we'll know if the decision was right
- [x] Review triggers define conditions for reconsidering the decision
- [x] Concern impact section is complete
- [x] ADR is consistent with governing feature spec (FEAT-032) and PRD requirements (FR-32)
