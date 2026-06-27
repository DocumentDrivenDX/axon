# Alignment Review: full-repo (post helix-0.8.0)

**Review Date**: 2026-06-27
**Scope**: Full repository — vision → PRD → feature-registry → 31 features → user-stories → architecture/ADRs/contracts → tests → code.
**Status**: complete
**Method**: HELIX `align` (helix plugin 0.8.0), fan-out across five reconciliation lanes (upper stack, code-surface traceability, AC→test traceability, design layer, plan/backlog freshness).
**Tracker baseline**: 1074 beads, **all closed**; ready/open/in-progress/blocked queues empty (implementation complete).

## 1. Verdict

The repository is **strongly aligned**. Authority order (Vision > PRD > Features >
Architecture/ADRs/Contracts > Tests > Code) is intact: every FEAT frontmatter
declares `depends_on: helix.prd` with matching dep-hash; all 31 FEAT files have
exactly one feature-registry row and vice versa; all 16 crates and every
HTTP/gRPC/CLI/GraphQL/MCP surface trace to a governing feature; ADRs are all
`Accepted` with no proposed-but-implemented or accepted-but-contradicted cases;
the `dbName` GraphQL/REST split (ADR-018) holds in both contract and code; and a
sampled phantom-claim scan (tests, benchmarks, metrics, CI gates) found **zero**
unbacked claims.

Defects are concentrated in: (a) documentation drift around the newest crate
`axon-registry`; (b) **two genuine engineering/product gaps** with no owning
work despite an empty queue; (c) two missing deploy/iterate artifacts; and
(d) breadth (not discipline) of AC→test `@covers` citations.

## 2. Findings requiring a human decision (HARD STOP — not auto-executed)

These are surfaced, **not** filed as execution work, because each requires a
product-scope decision only a human can make.

### H1 — FR-32 (local-first sync, committed P1) has no feature and no implementation
- **Classification**: INCOMPLETE (coverage hole).
- **Evidence**: `docs/helix/01-frame/prd.md:364-367` defines FR-32 as committed
  P1; `prd.md:471-473` records the open question resolved in favor of P1;
  `parking-lot.md:18-20` states local-first sync was "promoted to PRD P1 on
  2026-06-10 and **not tracked here**." Yet `feature-registry.md` Trace Links
  cover only FR-1…FR-31 — FR-32 appears in no feature, no registry row, no spec
  body. No real sync implementation exists in `crates/`. Principle #8
  "Local-First is a Requirement" (`principles.md:67-72`) has no realizing
  feature.
- **Decision needed**: frame `FEAT-032-local-first-sync` and schedule
  implementation, or re-defer FR-32 to P2/parking-lot with a recorded rationale.
- **Handoff**: destination `docs/helix/01-frame/features/FEAT-032-local-first-sync.md`
  (next ID already reserved at `feature-registry.md:210`); deliverable = full
  feature spec covering FR-32 (+ supporting FR-23/FR-26); next mode = **frame**.

### H2 — B-104 serializable isolation was sliced in the plan but never built
- **Classification**: INCOMPLETE (real engineering gap).
- **Evidence**: `implementation-plan.md:112,134` slice B-104 (read-set tracking +
  write-skew DST workload + `axon-sim` CycleWorkload gate + PROP-004 rework).
  No implementation exists — `grep read.set|write.skew|serializable_isolation`
  across `crates/*/src` returns nothing; `axon-sim` has no `CycleWorkload`.
  FEAT-008 still scopes serializable isolation as P1/future
  (`FEAT-008-acid-transactions.md:107,127`). Closed beads matching
  "serializable" are the OCC manager (which the spec says does **not** detect
  write skew), the salvage epic, and review-finding `hx-bf249aa0` (which itself
  notes "PROP-004 tests single-transaction atomicity, not concurrent
  serializability"). The queue is empty, so this gap is silently unowned.
- **Decision needed**: implement B-104 now, or make an explicit product decision
  to defer and record it in FEAT-008 + the implementation plan.
- **Handoff**: destination FEAT-008 / `implementation-plan.md`; next mode =
  **frame** (defer decision) or runtime handoff (implement); evidence as above.

## 3. Findings filed as work-execution beads (safe, scoped, reversible)

| # | Class | Finding | Fix | Bead |
|---|---|---|---|---|
| W1 | STALE_PLAN | `axon-registry` absent from workspace inventories; CLAUDE.md & `architecture.md` say "15 crates", 16 exist | Add `axon-registry` row + bump count in `CLAUDE.md:16-31` and `architecture.md:168-209` | docs |
| W2 | DIVERGENT | `axon-registry` cites CONTRACT-006 in its own header instead of governing FEAT-021; CONTRACT-006 names a non-existent `axon-cdc` crate | `crates/axon-registry/src/lib.rs:2` + `Cargo.toml` → cite FEAT-021 (CONTRACT-006 for wire); `CONTRACT-006:43` owning-system `axon-cdc`→`axon-audit` (cdc module) | docs |
| W3 | UNDERSPECIFIED | Malformed-but-well-shaped data-plane path → 404 (no master fallback) enforced in code but unpinned in any contract — the exact cross-tenant hazard fixed in commits e2fd2910 / 27c015e3 | Add normative rule to `CONTRACT-001` route grammar + ADR-018 note; add FEAT-029 to its `depends_on` | docs |
| W4 | INCOMPLETE | `CONTRACT-002:135` specifies data-plane `currentUser` query; only implemented in control-plane GraphQL | Align contract to shipped reality (scope `currentUser` to control-plane surface); add FEAT-029 to `depends_on` | docs |
| W5 | STALE_PLAN | implementation-plan presents delivered slices (B-101/B-102/B-106) as forward work; improvement-backlog ranks 1–6 marked `open` are delivered; item 10 partially resolved (release-notes shipped, runbook not) | Evolve `implementation-plan.md` + `improvement-backlog.md` to delivered-state with closure evidence | docs |
| W6 | UNCITED_COVERAGE | US-070…US-077 have STP docs naming exact existing, asserting tests but no `@covers` token was added | Citation-only pass — add `@covers US-0NN-ACm` to the named test bodies; no new tests | test |
| W7 | UNDERSPECIFIED | `docs/helix/05-deploy/` holds only release-notes; no runbook or deployment-checklist for the shipped control-plane/unified-binary operating surface | Author `runbook.md` + `deployment-checklist.md` | docs |
| W8 | UNDERSPECIFIED | `docs/helix/06-iterate/` lacks metric-definitions + metrics-dashboard, though code emits metrics (e.g. `axon_auth_rejections_total`, `auth_pipeline.rs:412`) | Author `metric-definitions.md` (catalog emitted metrics) + `metrics-dashboard.md` | docs |

## 4. Recommendations (noted, not filed)

- **AC→test citation breadth**: only 97/710 ACs (14%) carry `@covers`. Discipline
  is sound (0 phantom, 0 stale, 0 asserted-unbacked); the gap is breadth — ~100
  stories have no story-test-plan matrix at all. W6 covers the one fully-scoped
  slice (US-070…077). Closing the rest is a multi-feature STP-authoring epic and
  should be framed deliberately rather than auto-executed.
- **Feature status vocabulary**: 23/31 specs read `Status: draft` though shipped.
  The registry convention (`feature-registry.md:71-82`) deliberately separates
  spec-maturity from build state (tracked in beads), so this is by-design, not a
  lie. If a settled-spec signal is wanted, reconcile statuses in a dedicated
  evolve pass (start with FEAT-030 `in_review`→`approved`).
- **PRD success criteria** (`prd.md:483-500`) are all unchecked though
  implementation is complete; evaluate them with evidence in a validate pass.
- **Audit retention/erasure open question** (`prd.md:477-479`) remains unresolved
  while FEAT-003 audit is shipped — resolve before any compliance commitment.

## 5. Quality gates

Not re-run in this alignment pass (read-only diagnostic). The repo's CI ratchets
(`docs/helix/03-test/ci-ratchets.md`, `.github/workflows/`) remain the authority
for build/test/clippy/coverage/benchmark gates; W5–W8 executors must leave them
green.
