---
ddx:
  id: US-081
  review:
    self_hash: af2b0ddb1cb0806d024964a9bf0b6f7a51544ebc016bda701262537fb3f40cdd
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# US-081: Idempotent Transaction Submission

**Feature**: FEAT-008 — ACID Transactions
**Feature Requirements**: TXN-09
**PRD Requirements**: FR-5, FR-6
**Priority**: P0
**Status**: Draft

## Story

**As** Ava, an agent application developer whose client submits transactions over an unreliable network
**I want** to retry a transaction safely when I never received the response
**So that** a lost response causes neither duplicate writes nor a confusing conflict against my own prior write

## Context

Downstream consumers (for example, nexiq's sync flush) can lose a response
after the server applied the transaction; a naive retry then either
double-applies or conflicts with the client's own committed write. This story
exercises TXN-09: at-most-once application keyed by an idempotency key. The
full protocol — key format, scope, TTL, caching, in-flight behavior — is
normative in CONTRACT-001 §Transaction and idempotency protocol.

## Walkthrough

1. Ava's client submits a transaction with an idempotency key (per CONTRACT-001).
2. The response is lost in transit, but the server committed and cached the response under the key.
3. The client retries with the same key inside the TTL; the server returns the original response without re-executing, flagging the replay.
4. The client proceeds as if the first response had arrived.

## Acceptance Criteria

- [ ] **US-081-AC1** — Given a successful keyed transaction, when the same key is resubmitted within the TTL, then the original response is returned without re-execution and the replay is flagged (per CONTRACT-001).
- [ ] **US-081-AC2** — Given a keyed transaction whose TTL has expired, when the key is resubmitted, then the transaction re-executes (the key has no memory).
- [ ] **US-081-AC3** — Given the original keyed transaction failed with a schema or version-conflict error, when the key is resubmitted, then the transaction re-executes rather than replaying the cached failure (terminal policy denials excepted, per CONTRACT-001).
- [ ] **US-081-AC4** — Given a keyed transaction still in flight, when a concurrent duplicate with the same key arrives, then it is rejected as retryable with a wait hint (per CONTRACT-001).
- [ ] **US-081-AC5** — Given the same key used against two different databases, when both are submitted, then they are independent transactions (keys are scoped per tenant + database).

## Edge Cases

- **Same key, different payload within TTL**: the original cached success is returned until expiry; clients must mint a fresh key per logical transaction (CONTRACT-001).
- **Unkeyed retry**: behaves as a new transaction and may legitimately conflict — keys are the opt-in safety mechanism.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Replay | US-081-AC1 | Keyed txn committed; response lost | Resubmit same key < TTL | Original response, replay flagged, no re-execution |
| TTL expiry | US-081-AC2 | Key older than TTL | Resubmit | Re-executes |
| Failure not cached | US-081-AC3 | Original aborted on version conflict | Resubmit same key | Re-executes (may now succeed) |
| In-flight duplicate | US-081-AC4 | First request still executing | Concurrent duplicate | Retryable rejection with wait hint |
| Scope | US-081-AC5 | Key K in db1 and db2 | Submit both | Two independent commits |

## Dependencies

- **Stories**: US-020 (transaction protocol)
- **Feature Spec**: FEAT-008
- **Feature Requirements**: TXN-09
- **PRD Requirements**: FR-5, FR-6
- **External**: CONTRACT-001 §Transaction and idempotency protocol (normative key rules, TTL, caching, in-flight semantics)

## Out of Scope

- Idempotency for non-transaction endpoints.
- Durable exactly-once delivery to downstream consumers (FEAT-021 CDC concerns).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
