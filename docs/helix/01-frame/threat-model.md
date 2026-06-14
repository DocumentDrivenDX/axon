---
ddx:
  id: helix.threat-model
  depends_on:
    - helix.security-requirements
    - helix.prd
  review:
    self_hash: 1f144a1be515827aa3418049388e08e416f920234f4cb258be115026e773a36b
    deps:
      helix.prd: dff98156a6cc934f406611b78b513892d85cee1bd7b4c011f045146fcdfd23e1
      helix.security-requirements: 11163041a22f3ac008d62a7c957593da690306bc15bec244ebb9afdb9c69d0f5
    reviewed_at: "2026-06-14T04:39:42Z"
---

# Threat Model

**Project**: Axon
**Date**: 2026-06-10
**Status**: in_review (requires product-owner ratification)
**Author**: Security Champion (Erik LaBianca)

## Executive Summary

**System Overview**: Axon is a governed transactional entity store whose
value proposition *is* the guardrail: schema-validated entities, data-layer
policy, mutation intents with human approval, and repair-grade audit, below
GraphQL/MCP/CLI/SDK surfaces. The primary adversary is therefore unusual: a
**legitimately credentialed but prompt-injected or compromised agent**
operating inside its granted authority, alongside the classic external
attacker, hostile co-tenant, and malicious insider.

**Key Assets**: tenant business records; policy documents (the rules
themselves); mutation-intent records and tokens; the audit log; credentials
and signing/HMAC secrets.

**Primary Threats**: (1) agent rewrites the policy that constrains it via
governed-looking schema saves; (2) approval-threshold evasion by splitting
risky writes; (3) redaction bypass through aggregate/filter/sort side
channels; (4) storage-level audit tampering (append-only is API-level only);
(5) credential/delegation containment failures (unbound agent identity,
demotion divergence).

**Risk Level**: High — the guardrail thesis concentrates trust in one path;
any bypass falsifies the product promise, not just a feature.

Dispositions: **mitigated** (existing artifact covers it, verify with
tests), **accepted** (explicit V1 boundary, PO-decision-pending — see
[security-requirements](security-requirements.md) §V1 Scope Boundaries), or
**needs-requirement** (maps to an SR-n in
[security-requirements.md](security-requirements.md)).

## System Description

### Boundaries and Components

**In Scope**: the five trust surfaces below — intent lifecycle (FEAT-030),
policy layer (FEAT-029/ADR-019/CONTRACT-004), subject/credential model
(ADR-018), audit chain (FEAT-003/CONTRACT-005), and agent-facing surfaces
(MCP/GraphQL/CLI per CONTRACT-002/003/008).
**Out of Scope**: backing-store internals (PostgreSQL/SQLite hardening),
the LLM/agent runtime itself, downstream CDC consumers' security,
local-first sync (FR-32, not yet designed), BYOC control-plane fleet
operations beyond the data-sovereignty boundary (ADR-017/FR-27).
**Trust Boundaries**: network → auth middleware (ADR-018 verification
order); authenticated subject → policy engine (CONTRACT-004 evaluation
order); agent → human approver (FEAT-030 intent flow); tenant → tenant
(path/`aud`/storage isolation); Axon process → backing store; data plane →
control plane.

### Components

| Component | Description | Trust Level |
|-----------|-------------|-------------|
| Auth middleware | JWT/Tailscale verification, grants install (ADR-018) | Trusted enforcement point |
| Policy compiler + engine | CONTRACT-004 grammar → PolicyPlan; per-request snapshot | Trusted enforcement point |
| Intent store + commit path | FEAT-030/ADR-019 §6 binding and revalidation | Trusted enforcement point |
| Audit append path | CONTRACT-005 entries, atomic with mutation | Trusted enforcement point |
| GraphQL / MCP / CLI / SDK adapters | Protocol adapters over the shared handler | Semi-trusted (no authz decisions allowed) |
| Agents (MCP clients) | LLM-driven callers with delegated credentials | **Untrusted** (assume prompt-injectable) |
| Human approvers / operator UI | FEAT-031 intent inbox, audit views | Trusted humans, untrusted *displayed content* |
| Control plane | Tenants/users/credentials (`/control/*`) | Trusted, higher privilege |
| Backing store | PostgreSQL/SQLite via adapter | Trusted for durability, **not** for integrity attestation |

### Data Flows

- **External Sources**: GraphQL/REST requests, MCP tool calls (stdio +
  HTTP), CLI/SDK calls, control-plane admin operations.
- **Internal Processing**: auth → grants → policy snapshot → row/field/
  transition/envelope evaluation → OCC/transaction → atomic audit append;
  preview forks into intent records awaiting approval.
- **External Destinations**: query/tool results (policy-redacted), CDC
  change feeds (CONTRACT-006), audit query/tail output, MCP notifications.

## Assets

### Data Assets

| Asset | Classification | Confidentiality | Integrity | Availability |
|-------|---------------|-----------------|-----------|--------------|
| Tenant entities/links | Confidential (per tenant) | High | High | Medium |
| Policy documents (`access_control`) | Internal, integrity-critical | Medium | **Critical** | High |
| Mutation-intent records/tokens | Confidential | High | **Critical** | Medium |
| Audit log | Confidential, evidentiary | High | **Critical** | High |
| Credentials, signing key, deployment HMAC secret | Secret | **Critical** | Critical | Medium |
| Schema/ESF documents | Internal | Low | High | High |

### System Assets

| Asset | Criticality | Dependencies |
|-------|-------------|--------------|
| Shared handler path | Critical | All surfaces; Principle "Guardrails Are the Product" |
| Auth middleware + revocation cache | Critical | Control-plane tables, signing key |
| Policy compiler | Critical | Schema versioning (FEAT-017) |
| Approval surfaces (intent inbox) | High | Audit/intent rendering (untrusted content) |

## STRIDE Threat Analysis

Threats are grouped by trust surface; each carries a STRIDE-prefixed ID
(S/T/R/I/D/E per the template). Columns: Impact (I) and Likelihood (L) are
1–5; Risk = I×L. *Likelihood scores are author judgment —
**[ASSUMPTION]** pending product-owner review.*

### Surface A — Intent lifecycle (FEAT-030, ADR-019 §6)

The operation-hash binding is the load-bearing countermeasure. **What it
covers**: the canonical operation content, all listed pre-image versions,
schema/policy/grant versions, subject scope, approval state, expiry,
single-use. **What it does not cover**: (a) state that influences the
policy decision but is not a bound pre-image — notably
`subject.attributes.*` rows resolved from application collections; commit
checks versions and hash but is not specified to re-run full policy
evaluation; (b) the *semantic* gap between a correct diff and a harmful
intent.

| ID | Threat (actor → vector) | I | L | Risk | Existing mitigation (artifact) | Residual risk | Disposition |
|----|--------------------------|---|---|------|-------------------------------|---------------|-------------|
| TM-S-001 | External/network attacker or agent forges an intent token | 4 | 2 | 8 | HMAC(intent_id, deployment_secret); token references server record, not self-authorizing (ADR-019 §6, INT-04/07) | Single static deployment secret with no rotation/storage posture; tokens leakable via logs | needs-requirement → SR-10 |
| TM-S-002 | Agent replays a committed or expired token | 3 | 2 | 6 | Single-use once committed, expiry, approval-state check (INT-06, ADR-019 commit checks) | Low | mitigated (verify via tests) |
| TM-T-001 | TOCTOU between preview and commit: attacker mutates state feeding the decision after approval | 4 | 3 | 12 | Operation-hash + pre-image/schema/policy/grant/subject binding; stale-fail closed (INT-12/13/14, FR-8, PRD approval-safety metric) | Attribute-source rows (`subject.attributes.*`) and related entities outside the pre-image set are not bound; commit not specified to re-evaluate policy fully | needs-requirement → SR-7 |
| TM-R-001 | Approval social engineering: agent crafts a preview that *renders* innocuous (misleading metadata/reason, lookalike values, markup in field values) so the approver authorizes a harmful change | 4 | 4 | 16 | Diff and `review_summary` are server-computed from the canonical operation (INT-02, ADR-023/CONTRACT-005); `reason_required` envelopes (CONTRACT-004) | Approver sees attacker-influenced *content* (field values, metadata); semantic harm with an accurate diff is undetectable — semantic validation explicitly deferred (FEAT-022 Out of Scope) | needs-requirement → SR-15 (rendering); semantic residual accepted → boundary B-6 |
| TM-D-001 | Intent flooding: agent mass-previews to bloat the intent store and bury approvers' inboxes | 3 | 4 | 12 | Expiry; intents are short-lived non-workflow records (INT-06/08) | No creation cap or preview rate limit is specified anywhere | needs-requirement → SR-10 |

### Surface B — Policy layer (FEAT-029, ADR-019, CONTRACT-004)

| ID | Threat (actor → vector) | I | L | Risk | Existing mitigation (artifact) | Residual risk | Disposition |
|----|--------------------------|---|---|------|-------------------------------|---------------|-------------|
| TM-E-001 | Policy self-weakening via schema save: prompt-injected or compromised agent holding `admin` op calls `axon.schema.put`/`putSchema` and rewrites `access_control` — raising envelope thresholds, deleting deny rules, or removing the policy block entirely (reverting to coarse FEAT-012 grants) | 5 | 4 | **20** | Closed grammar rejects code injection (ACL-02, CONTRACT-004); compile gate (ACL-03); `schema.update` audited (CONTRACT-005); `admin` op required (ADR-018 op table) | The change is *grammatically legal*; nothing routes policy changes through approval — the guardrail can be edited by the guarded | **needs-requirement → SR-6** |
| TM-I-001 | Redaction bypass via side channels: aggregate over a redacted field, filter/sort/group-by probing (`amount > X` membership oracle), traversal hop filters, projection differences | 4 | 4 | 16 | Row policies before pagination/counts/aggregates (ACL-09, FR-13); hidden-row not-found semantics (ACL-08, CONTRACT-004 read denial) | Field-level redaction is specified for *reads* (ACL-12) but not for aggregate/filter/sort/group-by predicates over redacted fields (`{c}.aggregate`, `{c}.query` per CONTRACT-003) | needs-requirement → SR-12 |
| TM-I-002 | Hidden-entity existence leak through traversal, counts, nullability, error shapes | 4 | 2 | 8 | ACL-08/09/11; CONTRACT-004 read-denial semantics; ADR-019 validation trigger; PRD P0 contract tests | Low if test suites hold | mitigated (verify) |
| TM-E-002 | Fail-open window during policy reload/activation | 4 | 2 | 8 | Per-request snapshot; atomic swap; in-flight ops unaffected; evaluation errors deny (FEAT-029 NFR, CONTRACT-004 precedence) | In-flight requests finish on the older (possibly broader) snapshot; policy *removal* legally widens to FEAT-012 grants | mitigated; widening leg folded into SR-6 |
| TM-E-003 | Confused deputy via delegated agent credentials: agent uses its delegating user's broad credential for a narrow task; or asserts a false `agent_id`/`delegated_by` to satisfy delegation-aware policy | 4 | 3 | 12 | Per-tenant credentials, grants ≤ issuer role (ADR-018 §4); `subject.delegated_by` first-class in policy (ADR-019 §4) | ADR-018's JWT claim shape has **no agent/delegation fields** — ADR-019's subject model has no specified credential binding; MCP `actor` param validation (CONTRACT-003) is one line, untested | needs-requirement → SR-3 |

### Surface C — Subject/credential model (ADR-018)

Per-tenant credential scoping and per-database physical storage isolation
(ADR-011 §3, as amended by ADR-018) are the countermeasures; residual
channels are the shared layers above storage.

| ID | Threat (actor → vector) | I | L | Risk | Existing mitigation (artifact) | Residual risk | Disposition |
|----|--------------------------|---|---|------|-------------------------------|---------------|-------------|
| TM-S-003 | Credential exfiltration by a delegated agent (JWT leaks into prompts, logs, external tool calls) and replay by an attacker | 4 | 4 | 16 | `aud` = single tenant, `iss` deployment binding, 24 h TTL, `jti` revocation, grants subset (ADR-018 §4); blast-radius rationale (ADR-018 Alt C) | Within TTL + grants the attacker is indistinguishable from the agent; no anomaly detection; revocation latency unbounded in spec | needs-requirement → SR-2 |
| TM-E-004 | Grant escalation via demotion divergence: credential minted while `admin` keeps admin grants after demotion (verification deliberately does not re-check role) | 3 | 3 | 9 | 24 h TTL; explicit `jti` revocation documented as the operator mitigation (ADR-018 §4) | Window is real and acknowledged; relies on operator discipline | accepted (boundary B-3) + SR-2 revoke-on-demotion |
| TM-I-003 | Cross-tenant leakage through shared infrastructure | 5 | 2 | 10 | Tenant as isolation boundary; `aud`-to-path match; per-database storage isolation; control-plane listings filtered to memberships; tenant-fair rate accounting (ADR-018; ADR-011 §3; GRD-06) | Residual channels: control plane (global `users` table, deployment-admin scope), CDC topics (CONTRACT-006 tenant scoping must be proven), cross-database audit/ops queries, observability output | needs-requirement → SR-11 |
| TM-S-004 | Unauthenticated deployment exposure: `--no-auth` synthesizes admin grants for any named tenant/database | 4 | 2 | 8 | Secure-by-default service installs, explicit opt-in + warning, `doctor` visibility (FEAT-028 BIN-10/BIN-06) | Dev-loop `axon serve` default and stdio MCP remain local-trust | mitigated for services (SR-1); dev posture accepted (B-4/B-5) |

### Surface D — Audit chain (FEAT-003, CONTRACT-005)

| ID | Threat (actor → vector) | I | L | Risk | Existing mitigation (artifact) | Residual risk | Disposition |
|----|--------------------------|---|---|------|-------------------------------|---------------|-------------|
| TM-T-002 | Tampering beyond append-only: storage-level writer (DB admin, backing-store compromise, malicious operator) rewrites or deletes audit rows undetectably | 4 | 2 | 8* | Append-only enforced at every public API (AUD-04, CONTRACT-005); atomic write with mutation (FEAT-003 NFR) | No tamper *evidence*: cryptographic chaining is explicitly Out of Scope in FEAT-003, so the evidentiary value of the log rests entirely on storage trust. *Impact is 5 for a regulated customer | needs-requirement → SR-16 **[ASSUMPTION/scope change]** |
| TM-T-003 | Audit poisoning: attacker-controlled fields (`metadata` strings, entity field values in `before`/`after`/`diff`, `review_summary` content) rendered in operator UI/CLI — provenance spoofing, HTML/markdown/ANSI injection into the people doing approvals and forensics | 4 | 3 | 12 | `metadata` is informational-only and cannot affect operations (AUD-06); `actor` and timestamps are server-assigned (CONTRACT-005); tokens/pre-images excluded from preview events | No artifact treats audit content as untrusted *display* input for FEAT-031/CLI surfaces | needs-requirement → SR-15 |
| TM-T-004 | Repair-path abuse: rollback/revert as a destruction primitive — mass reverts, `transaction.rollback`/`collection.rollback`, force-revert bypassing schema validation | 4 | 2 | 8 | Revert is a governed, audited, OCC-checked mutation (AUD-11, CONTRACT-005); rollback routes require `admin` op (ADR-018 op table); rollback previews flow through intents (FEAT-030 edge cases); append-only log preserves recoverability | Admin-granted agent can still preview/commit destructive rollbacks; force option bypasses schema validation; no bulk-rollback cap until FEAT-022/023 | needs-requirement → SR-6 (admin-class governed changes); caps via SR-8 |
| TM-R-002 | Repudiation: anonymous-actor entries in embedded/no-auth mode make mutations unattributable | 2 | 3 | 6 | Documented `anonymous` actor (CONTRACT-005); authenticated-by-default services (BIN-10) | Dev-mode only by policy | accepted (B-4/B-5) |

### Surface E — Agent-facing surfaces (MCP / GraphQL / CLI)

| ID | Threat (actor → vector) | I | L | Risk | Existing mitigation (artifact) | Residual risk | Disposition |
|----|--------------------------|---|---|------|-------------------------------|---------------|-------------|
| TM-E-005 | Prompt-injection-driven tool abuse: hostile content read through Axon (or elsewhere) steers an agent to misuse its *legitimate* tools — including splitting an approval-worthy change into N writes under the discoverable envelope threshold | 4 | 4 | 16 | Policy/intents/audit apply identically regardless of agent intent (FEAT-016 "no second authorization model"; Principle 1); approval envelopes bound single risky writes (CONTRACT-004) | Everything inside the autonomous envelope is reachable by injection; per-operation envelopes do not bound *cumulative* effect; FEAT-022 caps are P1 | needs-requirement → SR-8; interim accepted (B-2) |
| TM-D-002 | Rate-limit evasion via actor multiplication: self-issue many credentials (allowed ≤ own role), vary `agent_id`/`actor` strings, or fan out across servers to multiply per-actor budgets | 3 | 4 | 12 | Per-actor sliding window (CONTRACT-001/ADR-024); tenant-fair accounting (GRD-06); `actor` must validate against authenticated identity (CONTRACT-003) | Limiter keying basis (credential vs user vs tenant) unspecified; per-server only (B-7); self-issuance mints fresh actors legally | needs-requirement → SR-9 |
| TM-I-004 | Schema/introspection information disclosure: GraphQL introspection, MCP tool metadata, and policy envelopes reveal schema shape, redaction lists, and exact approval thresholds to any credentialed caller | 3 | 4 | 12 | Discoverability is deliberate product behavior (PRD Interface Principles; MCP-05); metadata derives from the *caller-visible* compiled plan (ACL-17); enforcement never relies on hiding (FEAT-016 Out of Scope) | Threshold disclosure enables TM-E-005-style skating; accepted-by-design, compensated by cumulative caps | accepted-by-design; compensation → SR-8 |

## Risk Assessment

**Scoring**: Impact (1–5) × Likelihood (1–5). Critical 20–25; High 15–19;
Medium 10–14; Low 1–9. Likelihoods are **[ASSUMPTION]** pending PO review.

### Top Risks

| Risk ID | Threat | Impact | Likelihood | Score | Priority |
|---------|--------|--------|------------|-------|----------|
| TM-E-001 | Agent rewrites its own constraints via policy/schema save | 5 | 4 | 20 | Critical |
| TM-R-001 | Approval social engineering (innocuous-looking preview) | 4 | 4 | 16 | High |
| TM-E-005 | Prompt-injected tool abuse / threshold splitting | 4 | 4 | 16 | High |
| TM-I-001 | Redaction bypass via aggregate/filter/sort side channels | 4 | 4 | 16 | High |
| TM-S-003 | Delegated-agent credential exfiltration | 4 | 4 | 16 | High |
| TM-T-001 | TOCTOU via unbound attribute state | 4 | 3 | 12 | Medium |
| TM-T-003 | Audit poisoning of operator/approver surfaces | 4 | 3 | 12 | Medium |
| TM-T-002 | Storage-level audit tampering (no tamper evidence) | 4–5 | 2 | 8–10 | Medium† |

† Impact is 5 once a regulated/evidentiary customer exists; priority then
rises to High — part of the SR-16 PO decision.

## Mitigation Strategies

### TM-E-001 — Governed policy changes (Critical)
- **Controls**: SR-6 (approval-routed schema/policy saves that widen
  access), SR-8 (caps as defense in depth); preventive: intent flow on
  `putSchema`/`axon.schema.put`; detective: schema-diff widening alarms.
- **Timeline**: before any production tenant grants `admin` ops to agents.
- **Owner**: Policy/handler path (axon-schema, axon-api); PO ratifies scope.
- **Verification**: fixture — admin-granted agent's widening schema save
  returns `needs_approval`, commits nothing.

### TM-R-001 / TM-T-003 — Trustworthy approver and operator views (High)
- **Controls**: SR-15 (untrusted-content rendering, server-computed
  summaries only); corrective: audit lineage for post-hoc unwinding.
- **Timeline**: with FEAT-031 intent inbox delivery.
- **Owner**: Operator UI (FEAT-031) + audit read path.
- **Verification**: injection-corpus rendering tests; semantic residual is
  boundary B-6 (PO decision).

### TM-E-005 / TM-I-004 / TM-D-002 — Cumulative bounds on autonomous agents (High)
- **Controls**: SR-8 (velocity/aggregate caps), SR-9 (multiplication-proof
  limiter keying); detective: per-agent audit anomaly review (GRD-08).
- **Timeline**: with FEAT-022 (currently P1 — boundary B-2 is the interim
  acceptance, PO-decision-pending).
- **Owner**: Guardrail layer (FEAT-022) within the shared handler.
- **Verification**: salami-slicing and multi-credential evasion simulations.

### TM-I-001 — Redaction side-channel closure (High)
- **Controls**: SR-12 — redacted fields excluded or fail-closed in
  aggregate/filter/sort/group-by/traversal predicates.
- **Timeline**: before first multi-role production tenant (nexiq bar).
- **Owner**: Query planner + policy engine (FEAT-029/FEAT-018).
- **Verification**: side-channel fixture suite across GraphQL and MCP.

### TM-S-003 / TM-E-003 / TM-E-004 — Credential and delegation containment (High)
- **Controls**: SR-2 (revocation latency, revoke-on-demotion, key-rotation
  gate), SR-3 (credential-bound agent identity).
- **Timeline**: SR-3 needs an ADR-018 amendment before agent delegation
  ships; SR-2 rotation ADR is the existing v1.0 gate.
- **Owner**: Auth middleware + control plane.
- **Verification**: impersonation and revocation-latency tests.

### TM-T-001 — Commit-time revalidation (Medium)
- **Controls**: SR-7 — full policy re-evaluation at commit, or bind
  attribute-source pre-images.
- **Timeline**: with FEAT-030 implementation (design choice, cheap now).
- **Owner**: Intent commit path (FEAT-030/ADR-019 §6).
- **Verification**: attribute-drift TOCTOU test.

### TM-T-002 — Audit tamper evidence (Medium, rising)
- **Controls**: SR-16 hash chaining or equivalent **[ASSUMPTION — explicit
  scope change vs FEAT-003 Out of Scope; PO must ratify or accept B-1]**.
- **Timeline**: PO decision now; implementation before first regulated
  customer.
- **Owner**: axon-audit; FEAT-003/CONTRACT-005 amendment if ratified.
- **Verification**: storage-tamper detection test.

## Security Controls Summary

- **Preventive**: ADR-018 authentication/verification order and grant
  ceilings; CONTRACT-004 closed grammar, default-deny, fail-closed
  evaluation; FEAT-030 operation-hash/pre-image binding with single-use
  expiring intents; FEAT-028 secure-by-default installs; FEAT-022 scope/rate
  guardrails (P1).
- **Detective**: CONTRACT-005 complete audit capture incl. rejections;
  ADR-018 structured auth-rejection metrics; schema-change audit entries;
  proposed widening-diff alarms (SR-6) and tamper verification (SR-16).
- **Corrective**: audit-powered revert/rollback (FEAT-003/FEAT-023) — itself
  governed and audited; credential revocation; policy version rollback via
  schema versioning.

## Assumptions and Dependencies

- Likelihood scores and the Top Risks ordering are author judgment —
  **[ASSUMPTION]**, product-owner ratification required.
- Backing stores are trusted for durability but **not** for integrity
  attestation; that gap is exactly the SR-16 decision.
- Agents are assumed prompt-injectable at all times; no control here relies
  on agent good faith — only on what the data layer enforces.
- The shared-handler chokepoint (FR-22/FR-28) exists and is the single
  enforcement point; if any surface gains a side path, this entire model is
  invalidated (Principle "Guardrails Are the Product").
- Local-first sync (FR-32) and BYOC fleet operations are unmodeled here and
  must be threat-modeled before their design phases close.
- Companion artifact: [security-requirements.md](security-requirements.md)
  defines the SR-n controls referenced by every `needs-requirement`
  disposition (catalog relationship: security-requirements informs
  threat-model).
