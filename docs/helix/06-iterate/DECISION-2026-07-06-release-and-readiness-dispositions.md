---
ddx:
  id: helix.decision-2026-07-06-release-readiness
  depends_on:
    - helix.prd
  review:
    reviewed_at: "2026-07-06T00:00:00Z"
---

# Decision Record: Release Target and Readiness Dispositions (2026-07-06)

**Decision owner**: Erik LaBianca (operator/product owner) — no newer
checked-in artifact names a different decision owner for these items.
**Date**: 2026-07-06
**Driving bead**: `axon-86f6dba4`, child of the readiness epic `axon-01b14163`.

## Purpose

Before Axon can be called ready to use, the operator-owned release target and
several unresolved readiness decisions need durable, provable dispositions.
This record is the release target disposition and readiness decision
disposition artifact: it states what was decided, who decided it, and what
evidence backs each call. Downstream doc-alignment work (`axon-72b6f0b4`)
sweeps the rest of the HELIX doc set to agree with these dispositions; this
record is the source of truth it aligns to.

## 1. Release target disposition

**Disposition: REVOKE the 0.7.1 documentation-only target. CONFIRM 0.4.x as
the current, evidence-backed pilot release target.**

- **Prior state**: `docs/helix/01-frame/prd.md` and
  `docs/helix/01-frame/feature-registry.md` recorded an operator-requested
  Axon **0.7.1** release target as of 2026-06-14
  (`docs/helix/06-iterate/alignment-reviews/AR-2026-06-14-release-0.7.1-website.md`).
  That alignment pass explicitly flagged the target as documentation/planning
  authority only: local and `origin` Git tags ended at `v0.2.8` and
  `Cargo.toml` still declared workspace version `0.2.8` at the time, so no
  `v0.7.1` release, tag, or package had actually shipped.
- **Current evidence (2026-07-06)**: `Cargo.toml` declares workspace version
  `0.4.0`; `git tag --sort=-v:refname` and `git ls-remote --tags origin` both
  show the newest published tag as `v0.4.0`. No `v0.7.1` tag, release, or
  package exists locally or on `origin` at decision time.
- **Reasoning**: the repository's real, shippable release train advanced past
  the recorded evidence baseline (`v0.2.8`) to `v0.4.0` without ever reaching
  `0.7.1`. Continuing to plan against `0.7.1` keeps the PRD, feature registry,
  build plan, and deployment docs describing a release that has never
  existed in any tag, package, or binary artifact. `0.4.x` is the release
  target that actual repository evidence supports today, and is adopted as
  the operator's pilot-readiness target: Axon's V1 readiness verdict
  (`axon-5744d96b`) is evaluated against a `0.4.x` pilot release, not a
  `0.7.1` release.
- **What this does not decide**: whether or when to cut an actual `v0.4.x`
  GitHub release/tag/package artifact remains release-workflow work outside
  this decision (tracked by the deployment/readiness beads under
  `axon-01b14163`). This disposition only fixes the *target version line*
  that HELIX docs align to.
- **Follow-up**: `axon-72b6f0b4` sweeps `docs/helix/01-frame/prd.md`,
  `feature-registry.md`, `docs/helix/04-build/implementation-plan.md`,
  `docs/helix/05-deploy/*`, ADRs, contracts, and alignment reviews so every
  `0.7.1` release-target claim is replaced with the confirmed `0.4.x` target
  or explicitly marked historical.

## 2. Audit retention and erasure disposition

**Disposition: DEFERRED.** No change to V1 scope.

- **Source of the open question**: `docs/helix/01-frame/prd.md` Open
  Questions ("Which audit retention and erasure guarantees are required for
  the first regulated customer?"); `docs/helix/01-frame/security-requirements.md`
  SR-13 and V1 Scope Boundary B-9.
- **Decision**: V1 retains all audit data with no erasure/crypto-shredding
  path, per the existing `FEAT-003` scope. The field/tenant encryption-key +
  crypto-shredding + erasure-tombstone design sketched in ADR-019 §10 remains
  a design sketch, not a committed V1 requirement. This is a deferral, not a
  rejection: the design remains available to promote to committed scope once
  its trigger fires.
- **Revisit trigger**: a specific, named regulated customer or contractual
  requirement presents a concrete retention/erasure obligation V1's
  retain-everything behavior cannot satisfy.
- **Decision owner**: Erik LaBianca (operator/product owner).

## 3. Tamper-evident audit chain disposition

**Disposition: OUT OF SCOPE for V1.** No change to `FEAT-003` scope.

- **Source of the open question**: `docs/helix/01-frame/security-requirements.md`
  SR-16 and V1 Scope Boundary B-1, both flagged `[ASSUMPTION — scope change
  vs FEAT-003]` because `FEAT-003` currently lists "audit tamper detection /
  cryptographic chaining" as explicitly Out of Scope.
  Storage-level tamper evidence (hash-chained audit entries or an equivalent
  verifiable structure) is not adopted as a V1 requirement.
- **Decision**: SR-16 is not ratified for V1. The audit log's append-only
  guarantee is enforced at the API layer (AUD-04, CONTRACT-005) and trusts
  the storage layer; no cryptographic hash-chaining or storage-level tamper
  detection ships in V1. `FEAT-003`'s existing Out of Scope framing for
  tamper detection/cryptographic chaining stands unchanged.
- **Revisit trigger**: a serious adopter or compliance requirement demands
  verifiable proof against a compromised or malicious storage-level writer,
  not just API-layer append-only enforcement.
- **Decision owner**: Erik LaBianca (operator/product owner).

## 4. Whole-consumer workload deferrals disposition

**Disposition: NONE DEFERRED.** All three named downstream consumers remain
in scope for release qualification; no whole-consumer workload is dropped
from the readiness gate.

- **Source**: `docs/helix/03-test/consumer-workload-gate.md` already defines
  per-consumer status handling for Nexiq, DDx, and Cayce, and the open
  readiness beads (`axon-3d8dac83`, `axon-46c878f7`, `axon-6026b76b`,
  `axon-89fa770a`, `axon-7503a7ed`, `axon-ab7dea0f`, `axon-bb96959d`,
  `axon-cf47a0fc`) actively pursue evidence for all three.
- **Decision**: confirms, at the decision-of-record level, that this
  handling is the operator-approved disposition, not merely a testing
  convenience:
  - **Nexiq** — required now; the first real workload; must pass contract
    and e2e evidence for release qualification.
  - **DDx** — required for release qualification; PR/nightly runs may show
    `blocked` / `contract_gap` until a real Axon wire-call contract exists,
    but release qualification still fails on that status per the gate's
    status matrix.
  - **Cayce** — required for release qualification; PR/nightly runs may show
    `missing` / `missing_workload` when no source/export checkout is
    configured, but release qualification still fails on that status per the
    gate's status matrix.
  - No consumer workload is marked out-of-scope or permanently deferred for
    the pilot release; "missing" and "blocked" statuses are transitional
    states for PR/nightly convenience only, never for release qualification.
- **Revisit trigger**: none required to keep this disposition in force; it
  would only change if the operator explicitly drops a named consumer from
  the pilot scope in a future checked-in artifact.
- **Decision owner**: Erik LaBianca (operator/product owner).

## Evidence index

| Disposition | Evidence |
|---|---|
| Release target: 0.4.x confirmed, 0.7.1 revoked | `Cargo.toml:22` (`version = "0.4.0"`); `git tag --sort=-v:refname` (newest `v0.4.0`); `git ls-remote --tags origin` (newest `v0.4.0`); prior target record `docs/helix/06-iterate/alignment-reviews/AR-2026-06-14-release-0.7.1-website.md` |
| Audit retention/erasure: deferred | `docs/helix/01-frame/prd.md` Open Questions; `docs/helix/01-frame/security-requirements.md` SR-13, B-9 |
| Tamper-evident audit chain: out of scope | `docs/helix/01-frame/security-requirements.md` SR-16, B-1; `docs/helix/01-frame/features/FEAT-003` Out of Scope |
| Whole-consumer workloads: none deferred | `docs/helix/03-test/consumer-workload-gate.md` |
