---
ddx:
  id: US-143
---

# US-143: Mirror Resumes After Failure

**Feature**: FEAT-027 — Git Mirror (deferred — see `docs/helix/parking-lot.md`)
**Feature Requirements**: GIT-11, GIT-12, GIT-13, GIT-14
**PRD Requirements**: None allocated (deferred); consumes FR-18 change feeds
**Priority**: P2
**Status**: Draft

## Story

**As an** operator
**I want** the mirror to recover automatically after transient failures
**So that** temporary network or auth issues don't leave the mirror
permanently behind

## Context

Renumbered from US-081 (collision with FEAT-008 "Idempotent Transaction
Submission"). A projection that silently falls behind is worse than no
projection. This story exercises sync and recovery (GIT-11..14): retry
with backoff, resumption from the last mirrored audit position,
stuck-mode fallback, divergence handling, and operator-forced
re-snapshot. Mirror failures must never affect Axon writes.

## Walkthrough

1. The mirror's git push fails due to a network outage; Axon writes
   continue unaffected.
2. The worker retries with exponential backoff and catches up when the
   remote recovers; status shows the lag closing.
3. During a longer outage, 10 consecutive realtime pushes fail; the
   mirror falls back to batched mode and emits a "mirror stuck" audit
   event for the operator.
4. Axon restarts; the worker resumes from the last mirrored audit
   position without re-snapshotting.
5. After resolving a divergence (someone pushed to the mirror branch),
   the operator forces a full re-snapshot to restore a canonical mirror.

## Acceptance Criteria

- [ ] **US-143-AC1** — Given a failed push, when the worker retries, then
      retries use exponential backoff and Axon writes are never blocked.
- [ ] **US-143-AC2** — Given an Axon restart, when the mirror worker
      starts, then it resumes from the last mirrored audit position
      without re-snapshotting.
- [ ] **US-143-AC3** — Given 10 consecutive push failures in realtime
      mode, when the threshold is reached, then the mirror falls back to
      batched mode and a "mirror stuck" audit event is emitted.
- [ ] **US-143-AC4** — Given failures have occurred, when the operator
      requests mirror status, then it shows current lag and failure count.
- [ ] **US-143-AC5** — Given an operator decision, when a re-snapshot is
      forced, then the mirror rebuilds from a fresh full snapshot.
- [ ] **US-143-AC6** — Given a non-fast-forward push (consumer pushed to
      the mirror branch), when the worker detects it, then it does not
      force-push: it publishes to a recovery branch and emits a divergence
      audit event.

## Edge Cases

- **Remote unavailable at startup**: the worker retries with backoff,
  never blocks Axon startup, and logs periodic unavailability events.
- **Credential revoked mid-operation**: treated as push failure → backoff
  → stuck fallback; status names the auth failure.
- **Multiple server instances**: exactly one elected mirror worker per
  collection; failover re-elects and resumes from the persisted position.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Transient outage | US-143-AC1 | Remote down 2 min | Mutate entities | Writes succeed; mirror catches up after recovery |
| Restart resume | US-143-AC2 | 50 mutations mirrored; restart | Start Axon | Mirroring continues from position 50; no snapshot |
| Stuck fallback | US-143-AC3 | 10 consecutive push failures | 11th mutation | Batched mode active; stuck audit event emitted |
| Divergence | US-143-AC6 | Consumer pushed to mirror branch | Worker pushes | Recovery branch created; divergence audit event; no force-push |

## Dependencies

- **Stories**: US-140 (mirror enabled), US-141 (commit semantics)
- **Feature Spec**: FEAT-027
- **Feature Requirements**: GIT-11, GIT-12, GIT-13, GIT-14
- **PRD Requirements**: none allocated (deferred)
- **External**: FEAT-003 (audit log replay source); git remote

## Out of Scope

- Manual divergence resolution procedure (operator runbook material);
  enable/disable lifecycle (US-140); repo lifecycle management (archival,
  `git gc`).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
