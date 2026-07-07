---
ddx:
  id: helix.security-architecture
  depends_on:
    - helix.architecture
    - helix.prd
    - helix.security-requirements
    - helix.threat-model
    - helix.decision-2026-07-06-release-readiness
  review:
    self_hash: 36f3da17e713faa62c95e2f743466329602d96e28e1685e77328778fbf67c24b
    deps:
      helix.architecture: e425895dfe4a3486e735c0ac272c86aeeec950c828d3a91592ea803fd98fe075
      helix.decision-2026-07-06-release-readiness: 3b7f186b93db0ce5a81c9a46c72b20ab718d22b3feffc693a39b3ac4bbb836b0
      helix.prd: b11053b18982ec8f95158d284546dc20773f504bca99ec6c1970d71628f703ad
      helix.security-requirements: c816ccf54949816bc66e86f91be30ad8c3bac6c7d734a024c155350786963f8e
      helix.threat-model: c5d122663a894af8e1e1dbd5cc7a4b9e790e49fc0f922eec0282458517ac1d64
    reviewed_at: "2026-07-07T21:53:10Z"
---

# Security Architecture and Threat-Model Readiness

**Project**: Axon
**Date**: 2026-07-07
**Status**: Accepted readiness artifact

## Purpose

This document is the design-layer companion to the 01-frame security
requirements and threat model. It summarizes the security architecture that
HELIX readiness depends on and records the Phase-0 dispositions for audit
retention/erasure and tamper-evident audit chaining. It does not add new
product requirements; it explains how the current design covers the modeled
risks.

## Readiness Summary

| Risk theme | Modeled threats | Architecture response | Disposition |
|---|---|---|---|
| Cross-tenant leakage | TM-I-003, TM-I-001, TM-I-002 | Request path binds `(tenant, database)` to the authenticated identity; storage is partitioned per database; policy redaction applies before projection; CDC, audit, and control-plane listings are tenant-scoped. | Mitigated, verify with channel coverage tests. |
| Policy bypass and self-modification | TM-E-001, TM-E-002, TM-E-005, TM-D-002 | There is one governed request path; policy/schema changes are themselves governed writes; approval-routed policy widening is required when access expands; cumulative autonomous-write limits and actor-keying are explicit guardrails. | Mitigated, with P1 follow-on work still explicit. |
| Credential misuse and delegation abuse | TM-S-001, TM-S-003, TM-E-003, TM-E-004, TM-S-004 | Credentials are tenant-scoped, time-bounded, revocable by `jti`, and bound to authenticated identity; demotion revocation closes the divergence window; agent identity cannot be self-asserted. | Mitigated for service surfaces; local/dev trust boundaries remain explicit. |
| Approval social engineering | TM-R-001, TM-T-003, TM-D-001 | Mutation previews use server-computed diffs and summaries; operator views render untrusted content safely; high-risk writes route through approval before commit. | Mitigated for rendering and routing; semantic judgment remains a human boundary. |
| Audit tampering | TM-T-002 | V1 guarantees append-only audit at the API layer, but does not ship storage-level hash chaining. | Accepted V1 boundary; see Phase-0 disposition. |
| Retention and erasure | SR-13, B-9 | V1 retains all audit data and does not provide an erasure or crypto-shredding path. | Deferred V1 scope; see Phase-0 disposition. |

## Security Architecture

### Governing Path

Every mutation uses the same staged path:

1. Authenticate the caller and resolve tenant/database scope.
2. Apply guardrails and rate controls.
3. Evaluate policy from the compiled `access_control` plan.
4. Route approval-worthy writes into mutation intents.
5. Revalidate the intent at commit time.
6. Commit to storage.
7. Append the audit record.
8. Project change data for downstream consumers.

No GraphQL, MCP, CLI, SDK, or embedded path bypasses this sequence.

### Trust Boundaries

| Boundary | Assets | Control | Readiness note |
|---|---|---|---|
| Network request -> auth | JWTs, membership, grants, tenant/database scope | ADR-018 verification order, revocation, route checks | Blocks unauthenticated or cross-tenant entry. |
| Authenticated subject -> policy engine | Policy documents, subject attributes, redaction rules | Shared handler path, compiled policy snapshot, closed grammar | Prevents ad hoc policy bypass and self-modification. |
| Preview -> approval | Intent preview, diffs, review summary | Server-computed preview, encoded rendering, approval routing | Reduces social-engineering risk without hiding the diff. |
| Commit -> storage | Entity rows, link rows, audit rows | OCC, atomic transaction, append-only audit | Preserves repairable history. |
| Audit history -> operator review | Audit content, metadata, diff text | Untrusted-content rendering and provenance fields | Prevents audit poisoning in UI/CLI surfaces. |
| Retention/erasure -> compliance | Audit history, customer records | Retain-all V1; future crypto-shredding sketch in ADR-019 | Explicitly deferred, not omitted. |
| Storage -> tamper evidence | Audit rows at rest | API-layer append-only only in V1 | Storage-level tamper evidence remains out of scope. |

### Control Map

| Control | What it defends | Source |
|---|---|---|
| Tenant/database scoping and membership checks | Cross-tenant leakage and unauthorized enumeration | ADR-018, ADR-011, PRD FR-11 / FR-25, SR-11 |
| Policy-as-code with fail-closed evaluation | Policy bypass and malformed rules | ADR-019, CONTRACT-004, SR-5, SR-14 |
| Governed policy/schema writes | Self-weakening policy changes | SR-6, SR-8, TM-E-001 |
| Credential TTL, `jti` revocation, revoke-on-demotion | Credential replay and delegated-agent misuse | ADR-018, SR-2 |
| Credential-bound agent identity and actor validation | Spoofed `agent_id` / `delegated_by` / `actor` fields | SR-3 |
| Server-computed previews and safe rendering | Approval social engineering and audit poisoning | ADR-023, CONTRACT-005, SR-15 |
| Append-only audit with no V1 tamper chain | Repairable history with explicit integrity boundary | CONTRACT-005, SR-16, B-1 |
| Retain-all V1 audit policy | Regulatory hold, legal discovery, and repair history | PRD disposition, SR-13, B-9 |
| Cumulative autonomous-write limits | Threshold-skating and salami slicing | SR-8, TM-E-005 |

## Phase-0 Dispositions

### Audit Retention and Erasure

The 2026-07-06 release/readiness decision resolves the PRD open question on
audit retention and erasure:

- V1 retains all audit data.
- No erasure or crypto-shredding path ships in V1.
- The encryption-key plus erasure-tombstone sketch in ADR-019 remains a
  future design, not a committed readiness requirement.
- Revisit only if a named regulated customer or contractual obligation
  cannot be satisfied by retain-all behavior.

This is a deliberate readiness boundary, not a gap in the artifact set.

### Tamper-Evident Audit Chain

The same decision record resolves the tamper-evident chain question:

- SR-16 is not ratified for V1.
- The audit log is append-only at the API layer, but not hash-chained in
  storage.
- V1 trusts the backing store for integrity attestation.
- A hash chain or equivalent verifiable structure becomes a scope change if
  a serious adopter or compliance requirement demands proof against a
  compromised storage-level writer.

This is also a deliberate readiness boundary, not an unresolved design issue.

## Residual Risk

The remaining risks are the ones already called out by the 01-frame threat
model and PRD:

- prompt-injected agents can still exercise legitimate authority inside
  their grant set;
- threshold-skating and actor multiplication remain relevant until the
  cumulative guardrails land;
- local trust boundaries for developer-mode surfaces remain explicit;
- semantic review can still be fooled by a technically accurate but
  business-harmful change.

Those risks are known, traced, and either mitigated or explicitly deferred.
They do not require this bead to invent new controls.

## Verification Hooks

Readiness is supported by the following evidence and checks:

- cross-tenant isolation tests across data, CDC, audit, and control-plane
  channels;
- policy parity and guardrail tests across GraphQL, MCP, CLI, SDK, and
  embedded paths;
- impersonation and revocation tests for credential misuse;
- preview rendering tests for audit and approval surfaces;
- audit coverage tests for every mutation;
- document evidence for the Phase-0 retention/erasure and tamper-evidence
  decisions.
