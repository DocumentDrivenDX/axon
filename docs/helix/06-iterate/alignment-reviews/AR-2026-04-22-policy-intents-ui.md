# Alignment Review: policy-intents-ui

**Review Date**: 2026-04-22
**Scope**: policy-intents-ui (UI surfaces supporting FEAT-029 access-control policies and FEAT-030 mutation intents and approval)
**Status**: complete
**Review Epic**: hx-4709e387
**Governing Align Bead**: hx-935600fc
**Primary Governing Artifact**: `docs/helix/01-frame/features/FEAT-029-access-control.md`, `docs/helix/01-frame/features/FEAT-030-mutation-intents-approval.md`, `docs/helix/02-design/adr/ADR-019-policy-authoring-and-intents.md`, `docs/helix/01-frame/features/FEAT-011-admin-web-ui.md`

## 1. Review Metadata

| Field | Value |
|-------|-------|
| Review date | 2026-04-22 |
| Scope | policy-intents-ui |
| Status | complete |
| Governing align bead | `hx-935600fc` |
| Review epic | `hx-4709e387` |
| Review issues | `hx-511e279d` (planning stack), `hx-946bbd3c` (FEAT-029 UI), `hx-9d9b0fe0` (FEAT-030 UI), `hx-e511ec17` (FEAT-011 carrier), `hx-e93b9527` (backend prerequisites), `hx-29d49f99` (test-plan coverage) |
| Prior durable repo review | `docs/helix/06-iterate/alignment-reviews/AR-2026-04-16-repo.md` (did not classify FEAT-029/030) |
| Commands run | `cargo fmt --all -- --check` (PASS); `cargo clippy --workspace --all-targets --no-deps -- -D warnings -D clippy::todo -D clippy::unimplemented -D clippy::dbg_macro -D clippy::print_stdout -D clippy::unwrap_used` (FAIL — not in scope of this review, see §7 Quality Findings); `cargo deny check advisories` (FAIL — RUSTSEC-2026-0104, not in scope); `cargo test --workspace` (subset PASS; not re-run in full for this scope). UI gates (`bun run typecheck/lint/test/build`) not re-run in this scope — last repo review AR-2026-04-10-repo.md recorded PASS. |

## 2. Scope and Governing Artifacts

### Scope

UI surfaces that would let operators and developers exercise FEAT-029
(data-layer access control) and FEAT-030 (mutation intents and approval)
workflows from the admin web UI (FEAT-011). Concretely:

- Policy introspection and effective-policy surfaces (`effectivePolicy`,
  `explainPolicy` GraphQL fields rendered in UI).
- Pending mutation intent review queue.
- Mutation intent detail, diff, policy explanation, approve/reject controls.
- Audit integration for approvals, rejections, and committed intents.
- Test coverage (Playwright E2E) for the UI workflows.

### Functional Review Areas

1. Planning-stack reconciliation for policy/intents UI — `hx-511e279d`
2. FEAT-029 policy UI surfaces — `hx-946bbd3c`
3. FEAT-030 mutation-intent UI surfaces — `hx-9d9b0fe0`
4. FEAT-011 admin-UI carrier gaps — `hx-e511ec17`
5. Backend prerequisites (FEAT-029/030 Rust implementation) — `hx-e93b9527`
6. Test-plan coverage for policy-intents UI — `hx-29d49f99`

### Governing Artifacts

- `docs/helix/00-discover/product-vision.md` (lines 22–24, 35, 42)
- `docs/helix/01-frame/prd.md` (lines 28, 42–44, 233–234)
- `docs/helix/01-frame/features/FEAT-011-admin-web-ui.md`
- `docs/helix/01-frame/features/FEAT-029-access-control.md`
- `docs/helix/01-frame/features/FEAT-030-mutation-intents-approval.md`
- `docs/helix/02-design/adr/ADR-006-admin-ui-sveltekit-bun.md`
- `docs/helix/02-design/adr/ADR-012-graphql-query-layer.md`
- `docs/helix/02-design/adr/ADR-013-mcp-server.md`
- `docs/helix/02-design/adr/ADR-019-policy-authoring-and-intents.md`
- `docs/helix/03-test/test-plan.md` (SCN-017 at lines 578–613)
- `docs/helix/03-test/feature-story-e2e-traceability.md` (FEAT-029/030 rows at lines 87–88)
- `docs/helix/04-build/implementation-plan.md` (§3 line 83–84; §3a lines 122–165)

## 3. Intent Summary

- **Vision**: Axon is "policy-governed" — "Agents can discover what they may do,
  preview risky mutations, route high-risk writes for approval, and commit only
  under the active schema and policy version." (product-vision.md:22–24). The
  thesis explicitly couples "GraphQL policy and mutation semantics used by human
  and UI clients." (product-vision.md:27).
- **Requirements**: PRD lists "data-layer policies, and mutation
  preview/approval" in V1 value (prd.md:28), names "GraphQL-primary application
  surface — generated read/write GraphQL with policy-aware traversal,
  pagination, and mutation intents" (prd.md:42), "Agent-native MCP surface —
  generated tools/resources with the same policy model as GraphQL"
  (prd.md:43), and "Governed mutation intents — preview, explain, approve, and
  transactionally bind risky writes" (prd.md:44). Launch success metrics
  include GraphQL policy correctness and approval safety (prd.md:233–234).
  The PRD does **not** bind these value propositions to a specific UI surface.
- **Features / Stories**: FEAT-029 (Specified) and FEAT-030 (Specified) define
  nine user stories (US-101..US-109) that all describe GraphQL/MCP contract
  behavior and audit expectations; none contain UI acceptance criteria.
  Both features explicitly mark UI as out of scope:
  - FEAT-029:705–712 Out-of-Scope: "Policy authoring UI beyond
    introspection/dry-run API."
  - FEAT-030:247–253 Out-of-Scope: "Approval UI design beyond the GraphQL/MCP
    contract."
  FEAT-011 (Implemented) lists 12 Svelte pages (tenants, databases,
  collections, schemas, entities, audit, rollback, users, members, credentials,
  GraphQL playground) and six reverse route coverage tables, none of which
  mention policy authoring, policy introspection, pending intents, or approval
  actions. FEAT-011 Out-of-Scope (lines 308–315) also does not list
  policy/intents — the omission is silent, not explicit.
- **Architecture / ADRs**: ADR-019 (Accepted 2026-04-22) defines ESF
  `access_control` as the policy source of truth, the `PolicyPlan` compile
  pipeline, and the GraphQL/MCP mutation-intent workflow. Authoring is defined
  as text editing + dry-run programmatic compile report (ADR-019:310–323). No
  ADR specifies a policy-intents UI. ADR-006 (admin UI) predates
  FEAT-029/030 and does not mention policy surfaces.
- **Test Plans**: test-plan.md SCN-017 (lines 578–613) specifies the
  trusted-agent invoice write scenario spanning GraphQL, MCP, audit, preview,
  approval, stale-intent detection, and contractor redaction. All eight
  scenario checks are GraphQL/MCP-level; none reference UI. The feature-story
  E2E traceability file marks FEAT-029 and FEAT-030 rows as "None yet." for
  existing E2E coverage (feature-story-e2e-traceability.md:87–88).
- **Implementation Plans**: implementation-plan.md §3 marks FEAT-029 and
  FEAT-030 as "Not started" P0 product hardening (lines 83–84). §3a Priority
  Order names them #1 and #2, plus #5 "Operator controls: pending intent
  review, break-glass audit, credential rotation, cost/quota visibility"
  (implementation-plan.md:153–154). The operator-controls priority is the
  only planning-stack reference that implies a UI is needed for pending
  intents; it is not governed by any feature spec or ADR.

## 4. Planning Stack Findings

| Finding | Type | Evidence | Impact | Review Issue |
|---------|------|----------|--------|-------------|
| Vision and PRD position policy-governed writes and mutation preview/approval as P0 value props but never specify a UI surface for them | underspecified | product-vision.md:22–24, 27, 35, 42; prd.md:28, 42–44, 233–234 | Ambiguity about whether V1 expects a UI for operators/approvers | hx-511e279d |
| FEAT-011 admin UI feature spec does not mention policy introspection, pending intents, or approval workflows in scope, out-of-scope, or reverse coverage tables | underspecified | FEAT-011-admin-web-ui.md (entire file, especially §Scope, §User Stories US-040..US-045, §Reverse Route Coverage, §Out Of Scope lines 308–315) | Operators cannot tell from the carrier spec whether policy/intents UI is deferred, excluded, or will be added as stories | hx-511e279d, hx-e511ec17 |
| FEAT-029 explicitly excludes "Policy authoring UI beyond introspection/dry-run API" but does not name the introspection UI or list stories for it | same-layer conflict risk | FEAT-029-access-control.md:705–712; absent story for UI | The read-only introspection surface allowed by the carve-out has no spec; scope boundary is ambiguous | hx-946bbd3c |
| FEAT-030 explicitly excludes "Approval UI design beyond the GraphQL/MCP contract" yet approval workflows are identified as V1 success metric (prd.md:234) | same-layer conflict | FEAT-030-mutation-intents-approval.md:247–253 vs prd.md:234 vs implementation-plan.md:153 "Operator controls: pending intent review" | Operators cannot review pending intents without a UI, a CLI, or a documented GraphQL playground workflow | hx-9d9b0fe0, hx-e511ec17 |
| ADR-019 accepted 2026-04-22 governs policy authoring and intent workflows but is not listed under FEAT-011's dependencies | missing link | FEAT-011 §Dependencies (lines 294–305) does not reference ADR-019; FEAT-029/030 correctly cite ADR-019 | Downstream UI work will be unguided by the governing ADR unless FEAT-011 (or a successor) cites ADR-019 | hx-511e279d, hx-e511ec17 |
| test-plan.md SCN-017 is the only scenario that exercises FEAT-029+FEAT-030 end-to-end but defines only GraphQL/MCP checks | underspecified | test-plan.md:578–613 — all eight check bullets are GraphQL/MCP or audit; no UI criterion | If policy/intents UI is ever specified, SCN-017 will not cover it without amendment | hx-29d49f99 |
| feature-story-e2e-traceability.md FEAT-029/030 rows say "None yet." in the existing coverage column | stale / underspecified | feature-story-e2e-traceability.md:87–88 | E2E coverage gap is documented but not assigned to any test file or bead | hx-29d49f99 |
| implementation-plan.md §3a Priority Order #5 lists "Operator controls: pending intent review" without any governing spec | underspecified | implementation-plan.md:153–154 | Implementation direction implies UI work but no feature spec or ADR authorizes the scope | hx-511e279d |

## 5. Implementation Map

- **Topology**: Admin UI lives at `ui/` (SvelteKit + Bun + Vite, per ADR-006).
  Routes under `ui/src/routes/`: `+layout.svelte`, `+page.svelte`, `users/`,
  `tenants/`, `tenants/[tenant]/`, `tenants/[tenant]/members/`,
  `tenants/[tenant]/credentials/`, `tenants/[tenant]/databases/`,
  `tenants/[tenant]/databases/[database]/`, and database-scoped sub-sections
  `collections/`, `schemas/`, `audit/`, `graphql/`.
  Components: `JsonTree.svelte`, `TenantPicker.svelte`, and
  `json-tree-types.ts` under `ui/src/lib/components/`.
- **No policy-or-intent UI routes exist**: exhaustive glob for
  `*policy*`, `*polic*`, `*intent*`, `*approval*`, `*approve*`,
  `*preview*`, `*redact*`, `*effective*`, `*explain*` under
  `ui/src/routes/` and `ui/src/lib/` returns zero hits other than the
  existing rollback preview and markdown-template preview flows (both
  unrelated to FEAT-029/030).
- **No policy-or-intent GraphQL operations referenced from the UI**: searches
  for `effectivePolicy`, `explainPolicy`, `previewMutation`,
  `commitMutationIntent`, `approveMutationIntent`, `rejectMutationIntent`,
  `pendingMutationIntents`, `mutationIntent(` under `ui/` return zero hits.
- **Playwright E2E suites** under `ui/tests/e2e/` (eight spec files plus
  helpers and fixture cleanup): `smoke-restructure.spec.ts`,
  `tenant-isolation.spec.ts`, `tenant-admin.spec.ts`,
  `schema-editing.spec.ts`, `entity-crud.spec.ts`, `audit-route.spec.ts`,
  `wave1-capabilities.spec.ts`, `wave2-rollback.spec.ts`. None of them
  exercise policy introspection, intent preview, or approval workflows.
- **Backend crates**: `axon-schema`, `axon-api`, `axon-graphql`, `axon-mcp`
  contain no `access_control`, `PolicyPlan`, `MutationIntent`,
  `previewMutation`, `commitMutationIntent`, `approveMutationIntent`,
  `rejectMutationIntent`, `pendingMutationIntents`, `effectivePolicy`, or
  `explainPolicy` identifiers. The backend precondition for any UI work is
  absent. Implementation-plan.md §3 confirms "Not started" for both features
  (lines 83–84).
- **Concern drift (scope-local)**: `typescript-bun` concern governs `ui/`.
  Project overrides declare SvelteKit + Bun + Vite and the quality gate
  `cd ui && bun run typecheck && bun run lint && bun test && bun run build`.
  No policy/intent UI code exists to drift, so the scope-local concern drift
  is **zero**. `security-owasp` input-validation practice would apply to any
  future policy authoring UI but there is nothing to evaluate yet.

## 6. Acceptance Criteria Status

All criteria below are drawn from FEAT-029 §US-101..US-104, US-109 and
FEAT-030 §US-105..US-108. The implementation plan marks both features
"Not started" (implementation-plan.md:83–84); no code exists on either the
backend or the UI. Criteria are classified against the current repo state.

| Story / Feature | Criterion | Test Reference | Status | Evidence |
|-----------------|-----------|----------------|--------|----------|
| US-101 Hide inaccessible entities (FEAT-029) | Point reads for hidden entities return `not_found`/`null`, not `forbidden` | none | UNIMPLEMENTED | No policy enforcement in `axon-api`; no GraphQL filter lane in `axon-graphql` |
| US-101 | List/connection results omit hidden rows | none | UNIMPLEMENTED | Same |
| US-101 | Pagination and total counts are computed after policy filtering | none | UNIMPLEMENTED | Same |
| US-101 | Link traversal does not materialize hidden target entities | none | UNIMPLEMENTED | Same |
| US-102 Redact sensitive fields (FEAT-029) | GraphQL generated fields that may be redacted are nullable | none | UNIMPLEMENTED | No `PolicyPlan` compile → GraphQL nullability generation in `axon-graphql` |
| US-102 | Redacted fields return `null`; REST/compat/audit apply the same redaction | none | UNIMPLEMENTED | Same |
| US-103 Reject denied writes (FEAT-029) | Row/field-denied writes return `forbidden` with stable `reason` and `field_path` | none | UNIMPLEMENTED | No policy denial path in `axon-api` or `axon-graphql`/`axon-mcp` |
| US-104 Explain effective policy (FEAT-029) | GraphQL exposes `effectivePolicy` + `explainPolicy` introspection | none | UNIMPLEMENTED | No such fields in `axon-graphql` |
| US-109 Author and test policy before activation (FEAT-029) | Dry-run schema update returns a policy compile report | none | UNIMPLEMENTED | No compiler exists in `axon-schema` |
| US-105 Preview a GraphQL mutation (FEAT-030) | Preview returns diff, affected fields, pre-image versions, policy decision, intent token | none | UNIMPLEMENTED | `previewMutation` not present in `axon-graphql` |
| US-106 Route risky writes for approval (FEAT-030) | Envelope `needs_approval` includes intent ID, approval role, reason requirement | none | UNIMPLEMENTED | No envelope evaluator in `axon-api` |
| US-107 Prevent stale approval execution (FEAT-030) | Stale entity/policy/schema versions yield `intent_stale` | none | UNIMPLEMENTED | No intent record in `axon-api`/`axon-storage` |
| US-108 Use intents from MCP (FEAT-030) | MCP tool results expose structured `needs_approval`, `denied`, `conflict` | none | UNIMPLEMENTED | `axon-mcp` has no policy envelope metadata |
| SCN-017 (test-plan.md:578–613) | Eight end-to-end GraphQL+MCP checks | none | UNIMPLEMENTED (not coded) | `feature-story-e2e-traceability.md:87–88` reports "None yet." |
| policy-intents-ui carrier (no story exists) | Admin UI lists and reviews pending intents; renders diff and policy explanation; exposes approve/reject | none | UNSPECIFIED (no governing story or feature spec) | FEAT-011 omits these; FEAT-029/030 OOS sections exclude a UI; implementation-plan.md:153 mentions "Operator controls: pending intent review" without a governing spec |

Key observation: **no existing user story specifies a UI for policy
introspection, pending-intent review, or approval.** The SCN-017 scenario is
GraphQL/MCP-only, and FEAT-011's reverse route coverage does not list policy
or intent routes. The UI-specific acceptance criteria that would unlock
policy-intents-ui do not exist yet and must be authored before UI
implementation can begin.

## 7. Gap Register

| Area | Classification | Planning Evidence | Implementation Evidence | Resolution Direction | Review Issue | Notes |
|------|----------------|-------------------|------------------------|----------------------|--------------|-------|
| Planning stack — whether V1 scope includes a policy-intents UI at all | UNDERSPECIFIED | PRD V1 value props include policy governance and approval (prd.md:28, 42–44, 233–234); FEAT-029/030 OOS exclude UI; FEAT-011 silent; implementation-plan.md:153 mentions "Operator controls: pending intent review" | UI has no policy/intents routes (see §5); no backend support (see §5) | decision-needed then plan-to-code | hx-511e279d | Decide scope before authoring; produce a feature amendment or new FEAT if in scope |
| FEAT-029 policy introspection / authoring UI | UNDERSPECIFIED (intentionally deferred by FEAT-029 OOS) | FEAT-029:705–712 | UI: absent; backend `effectivePolicy`/`explainPolicy` absent | decision-needed then plan-to-code | hx-946bbd3c | Carve-out "beyond introspection/dry-run API" implies an introspection UI could be allowed, but no story exists |
| FEAT-030 mutation-intent review / approval UI | UNDERSPECIFIED (intentionally deferred by FEAT-030 OOS) | FEAT-030:247–253 | UI: absent; backend `previewMutation`/`commitMutationIntent`/etc. absent | decision-needed then plan-to-code | hx-9d9b0fe0 | "Approval UI design beyond the GraphQL/MCP contract" is OOS but PRD §V1 success metrics imply someone must approve |
| FEAT-011 admin-UI carrier coverage | UNDERSPECIFIED | FEAT-011-admin-web-ui.md — no mention of policy/intents in scope, stories, reverse coverage, or OOS (lines 308–315) | UI has 12 Svelte pages, none policy/intent | plan-to-code | hx-e511ec17 | FEAT-011 must either explicitly defer policy/intents to V2 or be amended with new stories when the decision above is made |
| Backend prerequisites (FEAT-029 + FEAT-030 Rust implementation) | UNIMPLEMENTED / BLOCKED for UI | FEAT-029, FEAT-030, ADR-019; implementation-plan.md:83–84 "Not started"; §3a Priority #1 and #2 | Crates `axon-schema`, `axon-api`, `axon-graphql`, `axon-mcp` have no `access_control` / `PolicyPlan` / `MutationIntent` code | plan-to-code | hx-e93b9527 | Any UI work is BLOCKED on the backend. No tracker bead currently governs the backend work |
| SCN-017 and E2E traceability coverage | STALE_PLAN / UNDERSPECIFIED | test-plan.md:578–613 lists GraphQL/MCP checks only; feature-story-e2e-traceability.md:87–88 says "None yet." | No Playwright spec touches policy/intents | plan-to-code | hx-29d49f99 | Once UI scope is decided, add a Playwright E2E spec and/or amend SCN-017 to include the UI path |

### Quality Findings

Quality findings below capture concerns surfaced by this review that are not
direct UI gaps but relate to the governing stack that would support a
policy-intents UI. Severity, owner, and resolution direction are listed.

| Area | Dimension | Concern | Severity | Resolution | Issue |
|------|-----------|---------|----------|------------|-------|
| ADR-019 / FEAT-011 linkage | maintainability | FEAT-011 does not reference ADR-019 even though ADR-019 §7–8 prescribes GraphQL/MCP surfaces that the admin UI will ultimately consume | low | Add ADR-019 to FEAT-011 §Dependencies on next amendment | hx-e511ec17 |
| operator-pending-intent-review affordance | robustness | implementation-plan.md:153 names "Operator controls: pending intent review" without specifying how (UI? GraphQL playground? CLI?); without the affordance, approvers cannot complete the governed-mutation loop | medium | Capture the affordance as a new user story on FEAT-011 or a new feature spec | hx-511e279d |
| planning concurrency | maintainability | Prior repo align (AR-2026-04-16) did not classify FEAT-029 or FEAT-030 — they are entirely absent from the gap register and traceability matrix | medium | Next repo align must classify them explicitly | (to be surfaced in next repo align) |
| Repo quality gates (OUT OF SCOPE for this review but observed) | robustness | `cargo clippy` strict gate fails (`axon-graphql/src/dynamic.rs:5730` ignored_unit_pattern; `unwrap_used` in lib tests) and `cargo deny check advisories` fails (RUSTSEC-2026-0104 rustls-webpki reachable panic) | high | Fold into next repo-scope align (not this policy-intents-ui review) | none here |

Quality findings do not change the gap classification above. The first two
concerns are covered by review issues hx-511e279d and hx-e511ec17 and by
the execution beads created in §10.

## 8. Traceability Matrix

| Vision | Requirement | Feature/Story | Arch/ADR | Design | Tests | Impl Plan | Code Status | Classification |
|--------|-------------|---------------|----------|--------|-------|-----------|-------------|----------------|
| Policy-governed writes (product-vision.md:22–24) | V1 value prop (prd.md:28) | FEAT-029 US-101..US-104, US-109 | ADR-019 | ADR-019 §1–5 | SCN-017 (test-plan.md:578–613) | implementation-plan.md:83, §3a #1 | Not started (backend absent; UI absent) | UNIMPLEMENTED (backend); UNDERSPECIFIED (UI) |
| Mutation preview / approval (product-vision.md:23, prd.md:44) | V1 value prop + success metric (prd.md:233–234) | FEAT-030 US-105..US-108 | ADR-019 | ADR-019 §6–8 | SCN-017 (test-plan.md:578–613) | implementation-plan.md:84, §3a #2 | Not started (backend absent; UI absent) | UNIMPLEMENTED (backend); UNDERSPECIFIED (UI) |
| Agent-native MCP surface with same policy model (prd.md:43) | V1 value prop | FEAT-016 MCP + FEAT-029/030 | ADR-013 + ADR-019 | ADR-013 §tools/resources | feature-story-e2e-traceability.md:74 | implementation-plan.md:79 "Done; policy hardening pending" | Backend MCP framework shipped; policy hooks absent | INCOMPLETE (from MCP side) / UNIMPLEMENTED (from policy side) |
| Human-friendly admin operations (vision → FEAT-011) | P1 admin UI | FEAT-011 US-040..US-045 | ADR-006 | ADR-006 §scope | ui/tests/e2e/*.spec.ts (8 specs) | implementation-plan.md:92 Done | 12 Svelte pages shipped; policy/intents not covered | ALIGNED (current FEAT-011 scope) but UNDERSPECIFIED for policy-intents extension |
| Operator controls: pending intent review (implementation-plan.md:153) | Implied by PRD approval metric | none | none | none | none | implementation-plan.md:153 (§3a Priority #5) | No UI, no CLI, no documented GraphQL playground workflow | UNDERSPECIFIED |
| Policy compile / dry-run authoring (ADR-019 §9, FEAT-029 US-109) | P0 | FEAT-029 US-109 | ADR-019 | ADR-019 §9 | SCN-017 indirect | implementation-plan.md:83, §3a #4 "Developer policy test harness" | Absent | UNIMPLEMENTED |
| Audit approval lineage (FEAT-030 §Audit And Observability) | P0 | FEAT-030 §Audit | ADR-019 §6 | — | SCN-017 criterion 7 (audit) | implementation-plan.md:84, §3a #3 agent identity/delegation fields | Audit attribution shipped (AR-2026-04-16 §5); policy/approval metadata absent | INCOMPLETE |

**Matrix coverage summary**: All policy-intents-ui-relevant lanes converge on
one conclusion — the backend is unimplemented (FEAT-029/030 Not started) and
the UI lane has no governing spec. The UI cannot ALIGN until the backend
ALIGNs first, and before either can happen a decision is required about
whether V1 ships with a dedicated UI or relies on GraphQL playground + CLI.

## 9. Review Issue Summary

| Review Issue | Functional Area | Status | Key Findings | Follow-up |
|--------------|-----------------|--------|--------------|-----------|
| hx-511e279d | Planning stack for policy-intents UI | UNDERSPECIFIED | No feature spec governs a policy-intents UI. FEAT-011 silent; FEAT-029/030 OOS excludes UI; implementation-plan.md §3a implies operator UI without a governing spec | Decision bead + planning bead |
| hx-946bbd3c | FEAT-029 UI surfaces | UNIMPLEMENTED / UNDERSPECIFIED | `effectivePolicy`/`explainPolicy` absent from `axon-graphql`; no UI pages for policy introspection | Planning/decision bead after backend specification |
| hx-9d9b0fe0 | FEAT-030 mutation-intent UI | UNIMPLEMENTED / UNDERSPECIFIED | `previewMutation`/`commitMutationIntent`/`approveMutationIntent`/`rejectMutationIntent`/`pendingMutationIntents` absent; no pending-intent UI | Planning/decision bead after backend specification |
| hx-e511ec17 | FEAT-011 admin-UI carrier gaps | UNDERSPECIFIED | FEAT-011 does not mention policy/intents anywhere — silent omission | Planning bead to amend FEAT-011 or create a new feature |
| hx-e93b9527 | Backend prerequisites (FEAT-029 + FEAT-030 Rust) | UNIMPLEMENTED (BLOCKING UI) | No `access_control`, `PolicyPlan`, `MutationIntent` code in any crate | Planning beads for backend standup (two beads: design slice + implementation slice) |
| hx-29d49f99 | Test-plan coverage for policy-intents UI | UNDERSPECIFIED | SCN-017 is GraphQL/MCP-only; E2E traceability "None yet." | Planning bead to extend test plan once UI scope is decided |

## 10. Execution Issues Generated

Execution beads are produced for the deterministic next actions. All are
`kind:planning` or `kind:triage` (not `kind:build`) because the scope has no
unimplemented code gaps that ALIGN with an existing specification — every
gap requires a specification or decision first.

| Issue ID | Type | HELIX Labels | Goal | Dependencies | Verification |
|----------|------|--------------|------|--------------|-------------|
| hx-752c217d | task (decision) | helix,phase:frame,kind:planning,kind:decision,area:ui,area:api | Decide whether V1 Axon ships a dedicated UI for policy introspection and mutation-intent review, or whether V1 operators rely on the GraphQL playground plus CLI. Must explicitly reconcile FEAT-029 OOS §705–712, FEAT-030 OOS §247–253, and implementation-plan.md:153 "Operator controls: pending intent review." | hx-935600fc (this align), hx-511e279d, hx-e511ec17 | A written decision note in `docs/helix/02-design/adr/` (new ADR) or an amendment to FEAT-011/029/030 OOS; align bead hx-935600fc closed with the decision recorded |
| hx-c4fa6055 | task | helix,phase:frame,kind:planning,area:ui | Conditional on the decision above, author either a FEAT-011 amendment (new stories US-046..US-04n) or a new FEAT-031 "Policy and Intents Admin UI" that specifies: policy introspection route, pending-intent list route, intent-detail route with diff/policy-explanation, approve/reject actions, stale-intent handling, and audit-linkage from approval to entity timeline. Cite ADR-019 in dependencies. | hx-752c217d, hx-946bbd3c, hx-9d9b0fe0, hx-e511ec17 | New or amended feature spec merged; reverse-route coverage table updated; ADR-019 added to FEAT-011 dependencies if amended |
| hx-714351e1 | task | helix,phase:test,kind:planning,area:ui,area:api | Extend `docs/helix/03-test/test-plan.md` SCN-017 with UI acceptance criteria covering operator pending-intent review, approval, rejection, stale-intent rendering, and audit follow-through. Update `docs/helix/03-test/feature-story-e2e-traceability.md` rows for FEAT-029 and FEAT-030 to point at new Playwright specs when authored. Conditional on hx-c4fa6055 deciding a UI is in scope. | hx-c4fa6055, hx-29d49f99 | SCN-017 revision present; traceability rows updated |
| hx-61a21e71 | task | helix,phase:design,kind:planning,area:data,area:api | Stand up an execution plan for FEAT-029 (policy compiler, row/field enforcement, GraphQL/MCP surfaces) as a set of triaged implementation beads with design → test → build sequencing. No backend bead currently governs this work; implementation-plan.md §3a Priority #1 names it but has no tracker entry. | hx-e93b9527 | A parent bead plus decomposed build beads exist; implementation-plan.md §3 and §3a references updated if needed |
| hx-f69b3662 | task | helix,phase:design,kind:planning,area:api,area:data | Stand up an execution plan for FEAT-030 (preview, intent storage, approve/reject/commit, stale detection, MCP mirrors, audit metadata). No backend bead currently governs this work; implementation-plan.md §3a Priority #2 names it but has no tracker entry. Depends on hx-61a21e71 (FEAT-029 backend required for policy decisions). | hx-61a21e71, hx-e93b9527 | A parent bead plus decomposed build beads exist; implementation-plan.md §3 and §3a references updated if needed |

Creation commands are recorded below in the "Issue creation log" at the end
of this report. Beads include `<context-digest>` blocks with principles,
concerns, and governing-artifact citations at creation time.

## 11. Issue Coverage Verification

| Gap / Criterion | Covering Issue | Status |
|-----------------|----------------|--------|
| Planning stack: UI scope undecided | hx-752c217d (decision) + hx-511e279d (review) | covered |
| FEAT-011 carrier: no policy/intents stories | hx-c4fa6055 (planning) + hx-e511ec17 (review) | covered |
| FEAT-029 UI surfaces absent | hx-c4fa6055 (planning, conditional) + hx-946bbd3c (review) | covered conditionally on decision |
| FEAT-030 mutation-intent UI absent | hx-c4fa6055 (planning, conditional) + hx-9d9b0fe0 (review) | covered conditionally on decision |
| Backend prerequisites (FEAT-029) | hx-61a21e71 (planning) + hx-e93b9527 (review) | covered |
| Backend prerequisites (FEAT-030) | hx-f69b3662 (planning) + hx-e93b9527 (review) | covered |
| Test-plan coverage for UI | hx-714351e1 (planning, conditional) + hx-29d49f99 (review) | covered conditionally on decision |
| ADR-019 not cited by FEAT-011 | Rolled into hx-c4fa6055 (amendment bead) | covered |
| AR-2026-04-16 omission of FEAT-029/030 | Deferred to next repo-scope align (out of scope here) | deferred |
| Strict clippy + cargo deny failures (repo-wide, out of scope) | Deferred to next repo-scope align | deferred |

Coverage result: **PASS** — every non-ALIGNED, in-scope gap has at least one
covering planning or decision bead. Conditional items were unblocked by
closing `hx-752c217d` after FEAT-031 captured the V1 decision: dedicated Axon
web UI coverage is required for the policy/intents slice.

## 12. Execution Order

Post-review amendments resolved the decision and planning layer. The current
execution order is:

1. FEAT-029 backend policy engine parent `axon-d556e197`, starting with
   `axon-fac5ca31` (`access_control` → `PolicyPlan`) and then
   `axon-2366e2b8` (axon-api enforcement).
2. FEAT-030 backend intent work parent `axon-c7111156`, starting in parallel
   where possible with `axon-1cbd17c3` (intent record storage/token lifecycle)
   while policy enforcement lands.
3. FEAT-031 UI parent `axon-c5a64173`, blocked on the FEAT-029/030 backend
   contracts it consumes.

**Critical path**: `axon-fac5ca31` → `axon-2366e2b8` →
`axon-acddb562`/`axon-50a8ccb4` → FEAT-030 GraphQL/MCP intent contracts →
FEAT-031 UI routes and Playwright coverage.

## 13. Decisions At Review Time

Post-review amendment: the product decision is now resolved. V1 includes a
dedicated Axon web UI for policy and mutation-intent workflows, governed by
FEAT-031. FEAT-029 and FEAT-030 remain GraphQL/MCP backend specs; their
out-of-scope lists now exclude only custom policy builders, broad REST parity,
and application-specific workflow builders.

| Decision | Resolution |
|----------|------------|
| Does V1 Axon ship a dedicated UI for policy-intents? | Yes. FEAT-031 is the governing Axon web UI spec. |
| Amend FEAT-011 or create FEAT-031? | Create FEAT-031 and treat FEAT-011 as the implemented carrier UI. |
| How do policy/intents integrate with audit? | FEAT-031 requires intent/audit deep links and redacted audit lineage. |
| Should MCP `needs_approval` be visible in the UI? | Yes. FEAT-031 requires MCP envelope preview and MCP-originated intent visibility. |

## 14. Queue Health and Exhaustion Assessment

- **Queue before this review**: All prior align beads and repo-scope execution
  beads closed. No open beads in the system (`ddx bead list --status open`
  returned 0). The repo queue was drained as of AR-2026-04-16 close-out.
- **Queue after this review and post-review amendments**: The governing align
  bead, review epic, six review issues, and five planning beads are closed.
  Three execution parents remain open: `axon-d556e197` (FEAT-029),
  `axon-c7111156` (FEAT-030), and `axon-c5a64173` (FEAT-031), with concrete
  child beads for backend contracts and Playwright UI coverage.
- **Gate status at review time**:
  - `cargo fmt --all -- --check` — PASS.
  - Strict `cargo clippy` gate — FAIL (repo-wide drift not in scope).
  - `cargo deny check advisories` — FAIL (RUSTSEC-2026-0104 rustls-webpki
    reachable panic; not in scope).
  - `cargo test` — subset PASS observed; full run not re-executed for this
    scope.
- **Scope-level exhaustion**: Resolved. The lane now has ready FEAT-029 and
  FEAT-030 build work plus blocked FEAT-031 UI work that will unblock when the
  backend GraphQL/MCP contracts exist.
- **Repo-level observation (out of scope)**: New clippy and cargo deny
  regressions have appeared since AR-2026-04-16 closed. These are not
  covered by this review and should be surfaced in the next repo-scope
  align.

## 15. Measurement Results

| Check | Result | Evidence |
|-------|--------|----------|
| Completeness: all functional areas have a gap classification | PASS | §7 classifies six functional areas |
| Traceability: matrix covers all in-scope governing lanes | PASS | §8 covers vision → code for policy, intents, MCP, admin UI, operator controls |
| Issue coverage: every non-ALIGNED, non-deferred gap has at least one execution or planning bead | PASS | §11 maps every gap to a covering bead |
| Concern drift (scope-local): `typescript-bun` concern violations in policy/intents code | PASS | No such code exists; no drift to record |
| Test-plan coverage recorded: every UNTESTED/UNIMPLEMENTED AC has a follow-on bead | PASS | hx-714351e1 covers test-plan extension; backend parents `axon-d556e197` and `axon-c7111156` cover backend tests |
| Governing bead acceptance satisfied | PASS | Report written; planning-stack conflicts recorded; backend and UI implementation status classified; traceability matrix produced; execution or planning beads created for every non-ALIGNED gap; `hx-935600fc` closed |

Measurement verdict: **PASS**.

## 16. Follow-On Beads Created

Planning and decision beads created by this review (full details in §10):

- hx-752c217d — decision: V1 scope of policy-intents UI
- hx-c4fa6055 — planning: feature spec for policy-intents UI (conditional)
- hx-714351e1 — planning: extend SCN-017 with UI criteria (conditional)
- hx-61a21e71 — planning: stand up FEAT-029 backend execution plan
- hx-f69b3662 — planning: stand up FEAT-030 backend execution plan

Post-review amendments closed those five planning beads and created concrete
execution parents:

- `axon-d556e197` — implement FEAT-029 data-layer access control policies
- `axon-c7111156` — implement FEAT-030 mutation intents and approval
- `axon-c5a64173` — implement FEAT-031 policy and intents admin UI

## Issue creation log

Tracker commands used during this review, for replay and audit:

```
ddx bead create "align: policy-intents-ui" --type task \
  --labels helix,kind:planning,action:align,area:ui,area:api \
  --set spec-id=FEAT-029 --description ... --acceptance ...
  # → hx-935600fc

ddx bead update hx-935600fc --claim

ddx bead create "HELIX alignment review: policy-intents-ui" --type epic \
  --labels helix,phase:review,kind:review,area:ui,area:api \
  --set spec-id=FEAT-029 --description ... --acceptance ...
  # → hx-4709e387

ddx bead create "review: planning-stack for policy-intents-ui" --type task \
  --parent hx-4709e387 --labels helix,phase:review,kind:review,area:api,area:ui \
  --set spec-id=FEAT-029 --description ... --acceptance ...
  # → hx-511e279d

# Additional review issues:
# hx-946bbd3c — review: FEAT-029 policy UI surfaces
# hx-9d9b0fe0 — review: FEAT-030 mutation-intent UI surfaces
# hx-e511ec17 — review: admin-UI carrier gaps for policy/intents
# hx-e93b9527 — review: backend prerequisites for policy-intents UI
# hx-29d49f99 — review: test-plan coverage for policy-intents UI

# Execution planning beads (see §10):
# hx-752c217d — decision: V1 UI scope
# hx-c4fa6055 — planning: FEAT-011 amendment / FEAT-031
# hx-714351e1 — planning: SCN-017 UI extension
# hx-61a21e71 — planning: FEAT-029 backend execution plan
# hx-f69b3662 — planning: FEAT-030 backend execution plan
```

---

```text
ALIGN_STATUS: COMPLETE
GAPS_FOUND: 6
EXECUTION_ISSUES_CREATED: 5
MEASURE_STATUS: PASS
BEAD_ID: hx-935600fc
FOLLOW_ON_CREATED: 5
```
