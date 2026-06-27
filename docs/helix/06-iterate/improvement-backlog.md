---
ddx:
  id: helix.improvement-backlog
  depends_on:
    - helix.prd
    - helix.implementation-plan
  review:
    self_hash: 56f5060ac1eb87c3e86b3a20ae42012b3e1aa7690c20b0f35f4476dd664f3a25
    deps:
      helix.implementation-plan: c00ab6585798f23953b7f0a7a496bdd4e6d4c8668cdb0557c40dc2ac40b55c03
      helix.prd: dff98156a6cc934f406611b78b513892d85cee1bd7b4c011f045146fcdfd23e1
    reviewed_at: "2026-06-15T00:35:16Z"
---
# Improvement Backlog

**Iteration**: Axon 0.7.1 release-alignment overlay (2026-06-14) on the
post-0.2.8 / helix 0.6.1 spec realignment baseline
**Source Learnings**: CONTRACT-001..010 authoring pass (2026-06-10),
implementation-plan baseline audit, feature-spec 0.6.1 rewrite, sibling-agent
reports (security-requirements, story-test-plan allocation)

Closure rule: per
`docs/helix/06-iterate/review-malfunction-audit-2026-04-20.md`, no item here
may be marked done without durable closure evidence (commit ref, execution
bundle, or explicit notes on the bead).

## Prioritization Rules

- Contract/spec violations of already-accepted decisions (ADR-018,
  CONTRACT-001..010) outrank new scope: drift compounds.
- Safety- and auth-relevant items (auth defaults, tamper evidence) outrank
  equivalently sized non-safety items.
- Items needing a product-owner or design decision are routed to the right
  HELIX mode rather than built speculatively; effort shown is the decision
  effort, not the eventual build.
- Within a priority band, order by confidence (high first), then by lower
  effort.

## Backlog Items

| Rank | Priority | Item | Evidence | Tracker or Follow-Up Target | Why Now | Confidence | Effort | Status |
|------|----------|------|----------|-----------------------------|---------|------------|--------|--------|
| 1 | P0 | Drop `dbName`/`dbPath` from control-plane GraphQL + SDK (ADR-018 dropped them; CONTRACT-002/-009 omit them) | CONTRACT authoring 2026-06-10; `sdk/typescript/src/graphql-client.ts` still exposes them | `axon-b8078b63` (closed) | Contract-frozen surface diverges from code | high | M | resolved — `grep -r 'dbName\|dbPath' sdk/typescript/src crates/` returns nothing; bead axon-b8078b63 closed (plan slice B-101) |
| 2 | P0 | Retire live un-prefixed legacy routes (`/auth/me`, `/databases/*`) violating ADR-018 | CONTRACT authoring 2026-06-10; routes live in `crates/axon-server/src/{gateway,path_router,control_plane_routes}.rs` | `axon-b684338f` (closed) | ADR-018 claims "no legacy routes"; reality disagrees | high | M | resolved — legacy routes retired; bead axon-b684338f closed (plan slice B-101) |
| 3 | P0 | Add SDK governed-workflow methods (`previewMutation`, `commitIntent`, `approveIntent`, `rejectIntent`, `explainPolicy`, `queryAudit`, `rollbackDryRun`) absent from `sdk/typescript` | CONTRACT-009 (`docs/helix/02-design/contracts/CONTRACT-009-sdk-surface.md`) vs `sdk/typescript/src` grep (no hits) | `axon-784bc974` (closed) | Server side shipped (FEAT-029/030); SDK can't drive the governed workflow | high | M | resolved — SDK governed-workflow methods shipped; bead axon-784bc974 closed (plan slice B-101) |
| 4 | P0 | Amend CONTRACT-008: FEAT-028 BIN-10 now requires authenticated-by-default service installs; contract still records no-auth default | FEAT-028 BIN-10 (`docs/helix/01-frame/features/FEAT-028-unified-binary.md`) vs `docs/helix/02-design/contracts/CONTRACT-008-cli-and-config.md` | `axon-87fee98b` (closed) | Auth-default mismatch is a safety-relevant spec conflict | high | S | resolved — CONTRACT-008 amended for auth-by-default; bead axon-87fee98b closed (plan slice B-101) |
| 5 | P1 | Tenant-aware MCP endpoints/resource URIs | CONTRACT-003 (`docs/helix/02-design/contracts/CONTRACT-003-mcp-surface.md`) vs `crates/axon-server/src/mcp_http.rs` | `axon-95b137d0` (closed) | MCP is the agent-native surface; URI shape is contract-bound | high | M | resolved — tenant-aware MCP endpoints/URIs shipped; bead axon-95b137d0 closed (plan slice B-101) |
| 6 | P1 | Deprecate `Idempotency-Key` header (body field canonical) | CONTRACT-001 (`docs/helix/02-design/contracts/CONTRACT-001-http-api-surface.md`) | `axon-c62971d9` (closed) | Cheap while the contract is fresh; avoids a third idempotency convention | high | S | resolved — `Idempotency-Key` header deprecated; bead axon-c62971d9 closed (plan slice B-101) |
| 7 | P1 | Tamper-evident audit chain decision (hash-chained audit records) | Security-requirements agent is proposing an SR — `docs/helix/01-frame/security-requirements.md`; FEAT-003 audit immutability scope | Follow-up: frame/design decision (accept SR, then ADR) — no bead until SR lands | Audit log is the product's trust anchor; SR is in flight now | med | M (decision) | open |
| 8 | P1 | FEAT-028 priority question: spec demoted P0→P1 to match PRD; if unified binary is truly P0, the PRD FR priority must change | FEAT-028 0.6.1 rewrite vs PRD v0.7.1 FR priorities | Follow-up: product owner decision (frame mode, PRD amendment) | Priority disagreement between PRD and spec blocks honest sequencing | high | S | open |
| 9 | P1 | Story-test-plans pending for remaining features per test-plan §AC allocation (non-guardrail features; exact list owed by sibling agent) | `docs/helix/03-test/test-plan.md`, `feature-story-e2e-traceability.md` | Follow-up: test mode (author STPs); tracked as plan slice B-107 | 0.6.1 specs re-anchored ACs; unallocated ACs are unverifiable | med | L | open |
| 10 | P1 | 05-deploy artifacts (runbook, release-notes) becoming due — installs, TLS bootstrap, and control plane have shipped | `docs/helix/05-deploy/` now exists with `release-notes-0.7.1.md`; runbook + deployment-checklist still missing; FEAT-025/028 shipped per implementation-plan baseline | Follow-up: deploy mode (author runbook + deployment-checklist) | Operating surface exists; release-notes delivered, runbook still owed | high | M | partial — release-notes delivered (`docs/helix/05-deploy/release-notes-0.7.1.md`); runbook/deployment-checklist outstanding |
| 11 | P2 | DST harness design ADR backfill: `axon-sim` is governed only by 00-discover research (`docs/helix/00-discover/foundationdb-dst-research.md`) | No ADR covers the simulation framework design | Follow-up: design mode (backfill ADR) | Harness gates correctness claims but has no design authority | high | S | open |
| 12 | P2 | FDB/fjall benchmark question (SPIKE-001 closed as overtaken; storage-backend bake-off never re-run) | `docs/helix/02-design/spikes/`; implementation-plan storage table (FoundationDB not started) | Follow-up: design spike decision (re-open or formally retire) | Cheap to decide; informs FoundationDB backend P2 item | med | S | open |
| 13 | P2 | FEAT-006 PRD anchor: no FR covers the bead adapter; PO may mint one or leave it as dogfooding | FEAT-006 (`docs/helix/01-frame/features/FEAT-006-bead-storage-adapter.md`) traceability gap | Follow-up: product owner decision (frame mode) | Untraced feature weakens the authority chain | high | S | open |
| 14 | P2 | Market-sizing research question: competitive-analysis growth-rate gap | `docs/helix/00-discover/competitive-analysis.md` | Follow-up: discover mode (research task) | Low cost; informs roadmap claims | med | S | open |

## Selection for Next Iteration

- **Delivered (prior iteration): ranks 1–6** — the contract-conformance bead
  set (`axon-b8078b63`, `axon-b684338f`, `axon-784bc974`, `axon-87fee98b`,
  `axon-95b137d0`, `axon-c62971d9`), all closed. They were implementation-plan
  slice B-101; every later slice (Kafka transport, guardrails, serializable
  isolation, BYOC) extends the same surfaces, so conformance debt was paid
  once. Kafka transport (B-102) and the BYOC control plane (B-106) have since
  shipped as well; all 1074 tracker beads are now closed and the queues are
  empty.
- **Open work**: serializable isolation (B-104, item routed via
  `AR-2026-06-27-full-repo.md` §2 H2), local-first sync design (B-105), the
  05-deploy runbook/deployment-checklist (item 10 remainder), and the
  remaining P1/P2 decision items (ranks 7–9, 11–14) awaiting product/design
  routing.

## Review Checklist

- [x] Each item cites evidence
- [x] Tracker references are included (beads for build-ready items; explicit follow-up targets for decision items)
- [x] Ordering is deterministic (priority band, then confidence, then effort)
