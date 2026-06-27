---
ddx:
  id: helix.parking-lot
  depends_on: [helix.prd]
  review:
    # TODO: refresh review stamp (offline-write reconciliation deferred, 2026-06-27)
    self_hash: 055b7a6710086e4b97c452947b79dab22cbc0c81834bfa40d1cc40f24a9870ee
    deps:
      helix.prd: dff98156a6cc934f406611b78b513892d85cee1bd7b4c011f045146fcdfd23e1
    reviewed_at: "2026-06-15T00:35:16Z"
---

# Parking Lot (Deferred / Future Work)

## Purpose

Tracks work the Axon PRD deliberately defers — capabilities named in the
PRD's Non-Goals or remaining P2 list — so it stays findable without
distorting V1 scope, which is proving governed agent writes. The BYOC fleet
control plane was promoted to PRD P1 on 2026-06-10 and is not tracked here.
Local-first was re-scoped on 2026-06-27: the governed local **read replica**
is committed at P1 (FR-32, FEAT-032), while the offline-write +
reconciliation half is **deferred here** (FR-33) — see "Offline Local Writes
and Deterministic Reconciliation" below.

## Policy
- Rejected items do not belong here; close or cancel them instead.
- Active work does not belong here; track it in the Feature Registry and DDx.
- Deferred items must include rationale and revisit trigger.
- Revisit triggers must be objective enough for another agent to evaluate.
- Any parked artifact must set `ddx.parking_lot: true` in its frontmatter.

## Deferred / Future Items

### Distributed Placement and Migration
- **Type**: Deferred
- **Artifact Type**: Feature Spec
- **Source**: PRD Non-Goals ("Multi-region distributed database in V1") and P2 #3; PRD FR-27 defers placement/routing
- **Rationale**: V1 is a single deployment fronting one backing store; distributed node placement, database migration, and routing add consistency and operational scope before single-deployment semantics are proven
- **Impact if Omitted**: BYOC fleets cannot rebalance or migrate databases across nodes; multi-region customers are unserved
- **Dependencies**: Stable single-deployment data-plane contract; BYOC fleet control plane (FR-27)
- **Revisit Trigger**: V1 P0 success criteria are green and a BYOC adopter needs multi-node placement, migration, or multi-region deployment
- **Target Activity/Milestone**: Post-V1 frame
- **Owner**: Erik LaBianca (product owner)
- **Last Reviewed**: 2026-06-10

### REST/JSON Compatibility Parity
- **Type**: Deferred
- **Artifact Type**: Feature Spec
- **Source**: PRD Non-Goals ("REST-first BaaS")
- **Rationale**: GraphQL is the primary application surface and MCP the primary agent surface; a full REST parity surface would triple interface maintenance before parity fixtures are proven on the primary surfaces
- **Impact if Omitted**: Teams that mandate REST integrate through thin wrappers or skip Axon
- **Dependencies**: Shared handler path and policy-parity fixture suite (FR-11, FR-12, FR-22)
- **Revisit Trigger**: Multiple serious adopters are blocked specifically by the absence of a REST surface, or a compatibility integration requires it
- **Target Activity/Milestone**: Post-V1 frame
- **Owner**: Erik LaBianca (product owner)
- **Last Reviewed**: 2026-06-10

### Advanced Indexes and Search (Vector, Full-Text/BM25)
- **Type**: Deferred
- **Artifact Type**: Feature Spec
- **Source**: PRD P2 #2 ("Advanced indexes and search")
- **Rationale**: Vector, full-text, and specialized search are retrieval features that should build on the proven governed core rather than expand V1 query scope
- **Impact if Omitted**: Agent retrieval workloads (semantic search, document ranking) must pair Axon with an external search system
- **Dependencies**: Policy-aware read model and secondary indexes (FR-3, FR-4); visibility-leak guarantees (FR-13) extended to ranked results
- **Revisit Trigger**: V1 P0 success criteria are green and an adopter needs governed semantic or full-text retrieval over Axon-held records
- **Target Activity/Milestone**: Post-V1 frame
- **Owner**: Erik LaBianca (product owner)
- **Last Reviewed**: 2026-06-10

### Application Substrate
- **Type**: Future
- **Artifact Type**: Feature Spec
- **Source**: PRD P2 #1 ("Application substrate")
- **Rationale**: Generating low-effort Axon-backed apps, SDKs, and admin surfaces from schema is a platform expansion; the core product must first prove the governed data layer those apps would inherit
- **Impact if Omitted**: Low-effort app teams assemble their own UI/scaffolding on top of Axon's generated GraphQL/MCP surfaces
- **Dependencies**: Stable schema, policy, and generated-surface contracts (FR-10, FR-20, FR-21, FR-28)
- **Revisit Trigger**: V1 P0 success criteria are green and multiple adopters request generated app scaffolding beyond the admin UI (FR-24)
- **Target Activity/Milestone**: Post-V1 frame
- **Owner**: Erik LaBianca (product owner)
- **Last Reviewed**: 2026-06-10

### Semantic Validation Hooks for Agent Guardrails
- **Type**: Deferred
- **Artifact Type**: Feature Spec (FEAT-022 functional area)
- **Source**: FEAT-022 Agent Guardrails (Out of Scope); PRD FR-9 names semantic validation hooks as post-intent-proof scope
- **Rationale**: Content-aware validation of proposed mutations (external validators examining a write in context before commit) is an open research problem; designing the hook interface before FEAT-029 policy enforcement and FEAT-030 mutation intents are proven in production would freeze the wrong abstraction
- **Impact if Omitted**: Structurally valid but semantically wrong agent writes (e.g., an invoice amount of $0.01 instead of $10,000) are caught only by approval routing, validation gates, and post-hoc audit/rollback rather than by automated content-aware checks
- **Dependencies**: FEAT-029 data-layer policies and FEAT-030 mutation intents proven in production; FEAT-022 scope and rate guardrails shipped
- **Revisit Trigger**: First customer need for content-aware validation of agent mutations that cannot be expressed as FEAT-019 validation rules, FEAT-029 policies, or FEAT-030 approval routing
- **Target Activity/Milestone**: Post-V1 frame
- **Owner**: Erik LaBianca (product owner)
- **Last Reviewed**: 2026-06-10

### Git Mirror (Git-Visible Projection of Collection State)
- **Type**: Deferred
- **Artifact Type**: Feature Spec
- **Source**: FEAT-027 (`docs/helix/01-frame/features/FEAT-027-git-mirror.md`); PRD Non-Goals lineage ("Git backend" reframed as a read-only mirror/projection)
- **Rationale**: Mirroring collection state into git (entity-per-file, commit-per-mutation, audit-linked trailers) is a change-feed consumer that adds git operational scope (credentials, push recovery, repo lifecycle) before the governed core and change feeds (FR-18) it depends on are proven; the audit log already provides the authoritative record
- **Impact if Omitted**: Teams cannot review agent output or entity history with standard git tooling (`git log`/`diff`/`blame`, PR review); compliance exports require an Axon client or custom tooling
- **Dependencies**: Change feeds (FEAT-021, FR-18); audit log (FEAT-003); markdown templates (FEAT-026, markdown format only); a future Contract for the normative mirror config/trailer surface
- **Revisit Trigger**: Adopter demand for git-visible spec mirroring — a serious adopter asks to review or consume Axon-held records through git tooling rather than through Axon surfaces
- **Target Activity/Milestone**: Post-V1 frame
- **Owner**: Erik LaBianca (product owner)
- **Last Reviewed**: 2026-06-10

### Offline Local Writes and Deterministic Reconciliation
- **Type**: Deferred
- **Artifact Type**: PRD Requirement (FR-33)
- **Source**: PRD FR-33 (deferred/parked); PRD Non-Goals ("Offline-write reconciliation"); re-scope of the former offline read+write FR-32; disposition of `docs/helix/06-iterate/alignment-reviews/AR-2026-06-27-full-repo.md` §2 H1
- **Rationale**: Agentic-world reprioritization (2026-06-27): in an agentic world clients talk to the server, so local-first's value (responsive UIs via local search/sort/filter/query) is delivered as a read-only CQRS projection (FR-32, FEAT-032). Offline-write convergence adds a deterministic conflict-resolution protocol — the hardest part of local-first — before the read replica it would build on is proven, and before any concrete demand exists
- **Impact if Omitted**: Clients cannot accept writes while disconnected; all writes require server connectivity. The local read replica (FR-32) still serves disconnected reads, search, sort, and filter
- **Dependencies**: Local read replica (FR-32, FEAT-032) proven; resumable scoped change stream (FR-31, FEAT-021); intent/policy/audit machinery (FEAT-029, FEAT-030, FEAT-003) extended to sync-time writes
- **Revisit Trigger**: A serious adopter or customer presents a concrete, blocking need to accept and durably commit writes on a client with no network connectivity (e.g., a field/edge workflow that cannot reach the server at write time)
- **Target Activity/Milestone**: Post-V1 frame
- **Owner**: Erik LaBianca (product owner)
- **Last Reviewed**: 2026-06-27
- **Note**: FR-33 is a deferred PRD requirement line, not a standalone artifact file, so there is no separate artifact frontmatter to carry `ddx.parking_lot: true`; the disposition is recorded inline in `prd.md` (FR-33) and here.

## Parked Artifacts (Links)

- [FEAT-027 — Git Mirror](01-frame/features/FEAT-027-git-mirror.md) — deferred; see "Git Mirror" entry above
