---
ddx:
  id: helix.security-requirements
  depends_on:
    - helix.prd
    - helix.principles
  review:
    self_hash: 11163041a22f3ac008d62a7c957593da690306bc15bec244ebb9afdb9c69d0f5
    deps:
      helix.prd: dff98156a6cc934f406611b78b513892d85cee1bd7b4c011f045146fcdfd23e1
      helix.principles: 68d05c2f025124f224f952adb2e7b93671c8f099011975fcbb3619e18fde38dd
    reviewed_at: "2026-06-15T00:35:16Z"
---

# Security Requirements

**Project**: Axon
**Date**: 2026-06-10
**Status**: in_review (requires product-owner ratification)
**Security Champion**: Erik LaBianca

## Overview

Axon's product thesis is that the data layer is the safety guardrail for
agentic applications ("Guardrails Are the Product",
`docs/helix/01-frame/principles.md` §1). The security scope is therefore the
product itself: the policy/intent/audit path that lets humans delegate
bounded work to agents must hold against an adversarial agent, a compromised
credential, a hostile tenant co-resident, and an operator-facing attacker —
not just against accidental misuse.

Protection goals, in priority order:

1. **Guardrail non-bypass** — no surface reaches storage outside the shared
   policy/intent/audit handler path.
2. **Tenant isolation** — no tenant observes or influences another tenant's
   data through any channel.
3. **Audit trustworthiness** — the audit chain is complete, attributable,
   and safe to rely on for investigation and repair.
4. **Credential and delegation containment** — a leaked or over-broad
   credential, or a prompt-injected agent, has a bounded, observable blast
   radius.

Requirements are numbered `SR-n` and desired-future-state: they state what
must be true, not what is implemented. Every invented numeric target is
marked **[ASSUMPTION]** and needs product-owner ratification. Threat
traceability lives in the companion
[threat model](threat-model.md); rows dispositioned `needs-requirement`
there map to SRs here.

## Required Controls

### Authentication

- **SR-1 — Authenticated by default.** Every network-reachable surface
  (REST, GraphQL, MCP-over-HTTP, control plane) authenticates per ADR-018
  (JWT credentials or Tailscale identity) before any handler logic runs.
  Unauthenticated operation requires the explicit `--no-auth` opt-in, and
  service installs default to authenticated mode with a prominent warning on
  opt-out (FEAT-028 BIN-10). *Acceptance*: install-path tests show zero
  unauthenticated service installs without the opt-in flag; route-inventory
  test shows no data-plane route skips the auth middleware.
- **SR-2 — Credential lifecycle.** Credentials carry bounded TTLs (24 h
  default per ADR-018), are individually revocable by `jti`, and revocation
  takes effect on the next request with at most **60 seconds [ASSUMPTION]**
  of cache-induced latency. Role demotion or tenant-membership removal
  triggers revocation of the affected user's outstanding credentials
  (closing ADR-018's documented divergence window). The signing-key rotation
  ADR (key `kid` versioning, overlap window, emergency runbook, rotation
  audit event) is a v1.0 release gate, as ADR-018 already requires.
  *Acceptance*: revocation integration test (cache-hit and SQL-hit paths);
  demotion-revocation test; rotation ADR exists and is exercised.
- **SR-3 — Agent identity is credential-bound, never self-asserted.**
  `subject.agent_id` and `subject.delegated_by` (ADR-019 §4) must derive
  from the authenticated credential or a server-validated binding — not from
  a free-form request field. The optional MCP `actor` parameter
  (CONTRACT-003) must be validated against the authenticated identity's
  permissions, with a contract test proving a caller cannot impersonate a
  different actor in policy decisions or audit attribution. *Acceptance*:
  parity fixture where a forged `actor`/`agent_id` is rejected or rewritten
  to the authenticated subject on every surface. **[ASSUMPTION: the JWT
  claim shape for agent delegation is not yet specified in ADR-018 and
  needs a design amendment.]**

### Authorization

- **SR-4 — Verification order and grant ceilings are enforced and tested.**
  The ADR-018 verification order (signature, expiry, revocation, `aud`
  /tenant match, membership, per-route op check) and the
  grants-≤-issuer-role ceiling at issuance are normative and covered by
  contract tests, including the strict rejection of unknown `grants` keys
  (fail-closed forward compatibility). *Acceptance*: per-route required-op
  contract test; grants-ceiling unit tests.
- **SR-5 — Guardrail non-bypass.** No surface — GraphQL, MCP, CLI, SDK,
  REST compatibility, embedded mode, sync, or rollback/repair tooling —
  reaches storage without traversing the shared schema/policy/intent/audit
  handler path (PRD FR-11, FR-12, FR-22, FR-28; Principle "Guardrails Are
  the Product"). Approval-routed writes commit only through the FEAT-030
  intent flow. *Acceptance*: shared parity fixture suite proves identical
  allow/deny/redaction/approval decisions across all surfaces; conformance
  test shows zero authorization decisions made in any protocol adapter.
- **SR-6 — Policy and schema changes are governed writes.** Saving a schema
  or `access_control` policy document for a production tenant must itself
  flow through the governed write path: at minimum approval-routed
  (FEAT-030 intent with a second-party approver) for changes that widen
  access, weaken redaction, raise envelope thresholds, or alter approval
  routes — so a prompt-injected or compromised agent holding an `admin`
  grant cannot silently rewrite the rules that constrain it (threats
  TM-E-001, TM-T-004, TM-E-002). Compile reports must flag
  widening/narrowing per FEAT-017 classification. **[ASSUMPTION: which
  tenants/environments require second-party approval is a product-owner
  decision — PO-decision-pending.]** *Acceptance*: fixture where an
  admin-granted agent's `putSchema`/`axon.schema.put` that widens policy
  returns `needs_approval` and commits nothing.
- **SR-7 — Commit-time revalidation covers attribute state.** Intent commit
  must either (a) re-run full policy evaluation (including
  `subject.attributes.*` lookups) at commit time, or (b) bind the attribute
  source rows consulted during preview as additional pre-images — so the
  operation-hash/version binding cannot be sidestepped by mutating state
  that feeds policy predicates but is not in the bound pre-image set
  (threat TM-T-001). *Acceptance*: TOCTOU test that changes an attribute
  source row between approval and commit and asserts stale-or-reevaluated
  behavior, never a stale-authority commit.
- **SR-8 — Cumulative autonomous-write limits.** Per-agent-identity
  velocity and aggregate caps bound the total effect of autonomous
  (envelope-`allow`) writes within a window — e.g. **max cumulative
  envelope-bounded value change and max autonomous mutations per actor per
  hour [ASSUMPTION — thresholds are application-tunable; defaults need PO
  ratification]** — so an agent cannot decompose one approval-worthy change
  into N under-threshold writes ("threshold skating"; threats TM-E-005,
  TM-I-004). Builds on FEAT-022 GRD-04/GRD-07. *Acceptance*:
  runaway/salami-slicing simulation is bounded with zero out-of-bound
  commits.
- **SR-9 — Rate-limit keying resists actor multiplication.** Rate and
  blast-radius accounting aggregates across the identities one principal
  can mint: keyed at minimum by (tenant, user) in addition to
  (credential, agent), so self-issuing N credentials or rotating `agent_id`
  strings does not multiply the budget (threat TM-D-002; extends FEAT-022
  GRD-06 and CONTRACT-001/ADR-024 semantics). *Acceptance*: evasion test
  with multiple self-issued credentials hits the shared budget.
- **SR-10 — Intent anti-abuse.** Pending mutation intents are bounded per
  actor and per tenant (**cap and preview rate limits [ASSUMPTION]**);
  intent tokens are never written to logs or audit metadata (CONTRACT-005
  already forbids token/pre-image in preview events); the
  `deployment_secret` backing intent-token HMACs has a documented rotation
  and storage posture aligned with the signing-key requirements of SR-2
  (threats TM-S-001, TM-D-001). *Acceptance*: flood test rejects intent
  creation beyond cap with a retryable signal; log-scrub test finds no
  tokens.

### Data Protection

- **SR-11 — Tenant isolation guarantees.** A credential scoped to tenant A
  must not be able to read, mutate, enumerate, or infer tenant B's data
  through any channel: data-plane routes (`aud`-to-path match, ADR-018),
  storage (per-database isolation, ADR-011 §3), CDC topics/change feeds
  (CONTRACT-006 envelopes must be tenant-scoped and subscriptions
  tenant-authorized), audit queries and the `__mutation_intents` synthetic
  collection (database-scoped), control-plane listings (results filtered to
  caller memberships), rate-limiter budgets (GRD-06), and error/observability
  output. *Acceptance*: cross-tenant isolation scenario suite covering each
  named channel, including the CDC and audit legs.
- **SR-12 — Redaction is complete across query shapes.** A field redacted
  for a caller (CONTRACT-004 `fields.*.read.deny`) must not be recoverable
  through aggregation (`{c}.aggregate` over the redacted field), filter
  predicates (membership/range probing via `{c}.query` filters), sort
  order, group-by keys, traversal hop filters, or projection differences:
  such operations either exclude the redacted field for that caller or fail
  closed with a stable reason code (extends FR-13 and ACL-12 from row-level
  to field-level side channels; threat TM-I-001). *Acceptance*: side-channel
  fixture suite proves a redacted value cannot be reconstructed via
  aggregate/filter/sort probing on any surface.

### Privacy

- **SR-13 — Erasure and retention readiness.** Audit immutability must
  coexist with erasure: the field/tenant encryption-key + crypto-shredding +
  erasure-tombstone design sketched in ADR-019 §10 becomes a committed
  requirement before the first regulated customer, and policy explanations
  must not reveal redacted values post-erasure. **[PO-decision-pending —
  the PRD open question on retention/erasure guarantees for the first
  regulated customer blocks this; until decided, V1 retains all audit data
  (FEAT-003) with no erasure path.]**

### Input Validation

- **SR-14 — Fail closed everywhere.** Any policy-evaluation error denies
  the operation (FEAT-029 NFR); unevaluable guardrail state rejects the
  mutation (FEAT-022 GRD reliability); malformed or forward-incompatible
  credentials are rejected (`credential_malformed`, ADR-018); the policy
  grammar remains closed — arbitrary code, SQL, or resolvers in policy
  documents are rejected at compile time (ACL-02, CONTRACT-004).
  *Acceptance*: fault-injection tests on each evaluation path assert deny,
  never allow.
- **SR-15 — Audit-rendering safety.** Caller-influenced audit content —
  `metadata` key-values, actor strings, entity field values inside
  `before`/`after`/`diff` and intent `review_summary` — is untrusted data:
  operator UI, CLI, and approval surfaces must encode/escape it (no HTML/
  markdown/ANSI injection into the approver's view), and approval summaries
  must be server-computed from the canonical operation, never
  caller-supplied (threats TM-T-003, TM-R-001). *Acceptance*: injection
  corpus rendered in the intent inbox and audit views without execution or
  spoofed layout.

### Logging and Audit

- **SR-16 — Audit integrity and tamper evidence.** Append-only behavior is
  enforced at the API (AUD-04, CONTRACT-005) **and** the log is
  tamper-evident against a storage-level writer: each entry carries a hash
  chained over its predecessor (or an equivalent verifiable structure),
  with a verification command and a documented anchor/checkpoint practice.
  **[ASSUMPTION — no existing artifact commits to tamper evidence;
  FEAT-003 currently lists "audit tamper detection / cryptographic
  chaining" as Out of Scope, so adopting this SR is an explicit scope
  change requiring product-owner ratification (threat TM-T-002).]**
  *Acceptance*: mutate-one-row-in-storage test makes verification fail.
- **SR-17 — Complete, attributable capture.** Every mutation produces
  exactly one audit entry with actor, delegated authority, tool/API origin,
  policy decision, approval context, versions, and before/after state
  (FR-15–FR-17, CONTRACT-005); auth rejections and guardrail rejections are
  observable (ADR-018 observability envelope; GRD-03/GRD-08). *Acceptance*:
  audit-coverage contract suite shows zero gaps.

## Requirements Matrix

| ID | Requirement (one line) | Source | Risk Level | Verification |
|----|------------------------|--------|------------|--------------|
| SR-1 | All reachable surfaces authenticated by default; explicit no-auth opt-in only | ADR-018, FEAT-028 BIN-10 | High | Install-path + route-inventory tests |
| SR-2 | Credential TTL, ≤60 s revocation **[ASSUMPTION]**, revoke-on-demotion, key-rotation ADR before v1.0 | ADR-018; TM-S-003/TM-E-004 | High | Revocation + rotation tests |
| SR-3 | Agent identity credential-bound; `actor` param cannot impersonate | ADR-019 §4, CONTRACT-003; TM-E-003 | High | Impersonation parity fixture |
| SR-4 | ADR-018 verification order + grant ceilings contract-tested | ADR-018 | High | Per-route op contract tests |
| SR-5 | No path to storage outside policy/intent/audit handler | Principle 1; FR-11/12/22/28 | Critical | Cross-surface parity fixtures |
| SR-6 | Policy/schema changes approval-routed when they widen access | TM-E-001/TM-T-004 **[PO-pending]** | Critical | Widening-change needs_approval fixture |
| SR-7 | Intent commit re-evaluates attribute-dependent policy state | ADR-019 §6 gap; TM-T-001 | High | Attribute-drift TOCTOU test |
| SR-8 | Cumulative caps prevent threshold-splitting of autonomous writes | TM-E-005/TM-I-004 **[ASSUMPTION]** | High | Salami-slicing simulation |
| SR-9 | Rate limits keyed to defeat actor multiplication | TM-D-002; FEAT-022 GRD-06 | Medium | Multi-credential evasion test |
| SR-10 | Intent flood caps; tokens never logged; HMAC secret rotation | TM-S-001/TM-D-001 **[ASSUMPTION]** | Medium | Flood + log-scrub tests |
| SR-11 | Tenant isolation across data plane, CDC, control plane, audit, limiter | ADR-018/011; TM-I-003 | Critical | Cross-tenant channel suite |
| SR-12 | Redacted fields unrecoverable via aggregate/filter/sort/projection | FR-13, ACL-12; TM-I-001 | High | Side-channel fixture suite |
| SR-13 | Erasure/crypto-shredding design before first regulated customer | ADR-019 §10; PRD open question **[PO-pending]** | Medium | Design review gate |
| SR-14 | All evaluation errors fail closed | FEAT-029 NFR, FEAT-022, CONTRACT-004 | High | Fault-injection deny tests |
| SR-15 | Caller-influenced audit/intent content treated as untrusted in operator surfaces | TM-T-003/TM-R-001 | High | Injection-corpus rendering tests |
| SR-16 | Audit log tamper-evident (hash chain or equivalent) | TM-T-002 **[ASSUMPTION — scope change vs FEAT-003]** | High | Storage-tamper verification test |
| SR-17 | 100% attributable audit capture incl. rejections | FR-15–17, CONTRACT-005 | Critical | Audit-coverage contract suite |
| SR-18 | Supply-chain hygiene: vetted deps, lockfiles, provenance | Workspace conventions | Medium | CI policy checks |

## Compliance Requirements

**Applicable Regulations**: None committed yet. The PRD's open question on
audit retention and erasure guarantees for the first regulated customer is
the gating decision **[PO-decision-pending]**.
**Applicable Standards**: OWASP ASVS is the engineering verification anchor
for the controls above (per the security-requirements catalog references);
no certification target is committed for V1.

- Audit retention default (7 years, cold-storage archive on tenant
  deletion, per ADR-018 Implementation Notes) stands until a regulated
  customer's requirements supersede it.

## Security Risks

### High-Risk Areas

1. **Policy self-modification by agents (TM-E-001)**: an `admin`-granted
   agent can rewrite the policy that constrains it via `axon.schema.put` /
   `putSchema`. Mitigation: SR-6 governed policy changes; SR-8 caps.
2. **Approval-threshold evasion (TM-E-005/TM-I-004)**: envelope thresholds
   are discoverable by design; injected agents can skate under them.
   Mitigation: SR-8 cumulative limits; SR-15 trustworthy approver views.
3. **Redaction side channels (TM-I-001)**: aggregates/filters/sorts over
   redacted fields. Mitigation: SR-12.
4. **Storage-level audit tampering (TM-T-002)**: append-only is API-level
   only today. Mitigation: SR-16 (proposed scope change).
5. **Credential/delegation containment (TM-S-003/TM-E-003/TM-E-004)**:
   leaked JWTs, unbound agent identity, demotion divergence. Mitigation:
   SR-2, SR-3.

## Security Architecture Requirements

- [ ] Tenant/database isolation verified per channel (SR-11)
- [ ] Shared-handler chokepoint with surface parity fixtures (SR-5)
- [ ] Application security testing in CI (parity, isolation, side-channel suites)
- [ ] Dependency vulnerability scanning (`cargo audit` / `cargo deny`) and
      lockfile-enforced builds; pinned CI actions; release artifacts built
      from CI with provenance (SR-18 — supply-chain posture, kept brief by
      design)
- [ ] Server hardening: secure-by-default service units (FEAT-028), signing
      keys in vault/KMS only (ADR-018)
- [ ] Patch management for the Rust toolchain and dependency tree
- [ ] Backup and recovery exercised against the audit/repair path

## Security Testing Requirements

- [ ] Penetration testing focused on the five trust surfaces of the threat
      model before first multi-tenant production deployment
- [ ] Vulnerability assessments per release (dependency + config)
- [ ] Security code review for the auth middleware, policy compiler, intent
      commit path, and audit append path
- [ ] Automated security scanning in CI (clippy security lints, cargo
      audit/deny, secret scanning)
- [ ] Adversarial agent simulation: prompt-injected agent in the loop with
      real MCP tools attempting policy rewrite, threshold-splitting, intent
      flooding, and actor multiplication (verifies SR-3/6/8/9/10)

## V1 Scope Boundaries (explicitly accepted risk — each PO-decision-pending)

| # | Accepted-for-V1 boundary | Source | Status |
|---|--------------------------|--------|--------|
| B-1 | No cryptographic audit tamper evidence (storage layer trusted) unless SR-16 is ratified | FEAT-003 Out of Scope | PO-decision-pending |
| B-2 | FEAT-022 rate/scope guardrails are P1 — until shipped, agents are bounded only by policy envelopes and intents | FEAT-022, FR-9 | PO-decision-pending |
| B-3 | Grant divergence after demotion within the 24 h credential TTL (absent SR-2 revoke-on-demotion) | ADR-018 §4 | PO-decision-pending |
| B-4 | MCP stdio transport is unauthenticated (local `--no-auth` trust boundary) | CONTRACT-003, FEAT-016 | PO-decision-pending |
| B-5 | Developer-loop `axon serve` keeps its current local default auth posture | FEAT-028 constraints | PO-decision-pending |
| B-6 | Semantic validation of mutation content deferred (innocuous-preview residual stands) | FEAT-022 Out of Scope / parking lot | PO-decision-pending |
| B-7 | Rate limiting is per-server, not fleet-coordinated | CONTRACT-001, ADR-024, FEAT-022 | PO-decision-pending |
| B-8 | Signing-key rotation protocol deferred to a future ADR (hard gate before v1.0) | ADR-018 Implementation Notes | PO-decision-pending |
| B-9 | No erasure/crypto-shredding path in V1; all audit data retained | FEAT-003, PRD open question | PO-decision-pending |

## Assumptions and Dependencies

- All numeric targets above marked **[ASSUMPTION]** (revocation latency,
  intent caps, cumulative-write thresholds) are invented for review and
  require product-owner ratification.
- SR-16 contradicts FEAT-003's current Out of Scope list; adopting it is a
  scope change to FEAT-003/CONTRACT-005, not a reinterpretation.
- SR-3 assumes an ADR-018 amendment defining how agent delegation appears
  in credential claims; today the JWT shape has no `agent_id`/`delegated_by`.
- The companion [threat model](threat-model.md) supplies threat-level
  traceability; this document supplies the testable controls
  (catalog relationship: security-requirements *informs* threat-model).
- Depends on ADR-018 (identity/credentials), ADR-019 + CONTRACT-004
  (policy/intents), CONTRACT-005 (audit record), CONTRACT-003 (MCP),
  FEAT-022/028/029/030 (guardrails, secure defaults, policy, intents).
