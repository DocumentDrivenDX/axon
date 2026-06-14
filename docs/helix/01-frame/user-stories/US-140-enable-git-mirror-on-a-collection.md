---
ddx:
  id: US-140
  review:
    self_hash: c2ac6aeaf0321d525f47671da6384562ade32c611e0d72c8e3ff5e8c8abea1ad
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# US-140: Enable Git Mirror on a Collection

**Feature**: FEAT-027 — Git Mirror (deferred — see `docs/helix/parking-lot.md`)
**Feature Requirements**: GIT-01, GIT-02, GIT-09
**PRD Requirements**: None allocated (deferred); consumes FR-18 change feeds
**Priority**: P2
**Status**: Draft

## Story

**As a** developer or operator
**I want** to mirror a collection to a git repo
**So that** entity changes are reviewable with standard git tooling

## Context

Renumbered from US-078 (collision with FEAT-015). The mirror's lifecycle
must be a one-command experience with observable health. This story
exercises mirror configuration and lifecycle (GIT-01, GIT-02) and the
initial snapshot boundary (GIT-09). The normative config surface will
live in a Contract when FEAT-027 is scheduled; this story is written at
the behavioral level.

## Walkthrough

1. The operator enables mirroring on the `invoices` collection via the
   CLI, naming the remote repository.
2. Axon creates an initial snapshot: all current entities committed as a
   single commit identifying the as-of point.
3. Subsequent mutations are committed incrementally.
4. The operator checks mirror status and sees the last mirrored audit
   position, lag, and no errors.
5. Later, the operator disables the mirror; mirroring stops and the
   remote repository is left intact.

## Acceptance Criteria

- [ ] **US-140-AC1** — Given a collection with existing entities, when the
      operator enables a mirror with a remote repository, then mirroring
      is enabled and an initial snapshot commit is triggered.
- [ ] **US-140-AC2** — Given the initial snapshot, when it is created,
      then all current entities are committed in one commit marked as a
      snapshot with its as-of point.
- [ ] **US-140-AC3** — Given an enabled mirror, when the operator requests
      mirror status, then it shows the last mirrored audit position, lag,
      and any errors.
- [ ] **US-140-AC4** — Given an enabled mirror, when the operator disables
      it, then mirroring stops and the remote repository is not deleted.
- [ ] **US-140-AC5** — Given an enabled mirror, when its configuration is
      requested via the API, then the active configuration is returned
      (credential material never in plaintext).

## Edge Cases

- **Markdown format without a FEAT-026 template**: enable fails with a
  descriptive error.
- **Unreachable remote at enable time**: enable reports the failure;
  retries follow recovery behavior (US-143); Axon writes are unaffected.
- **Realtime mode on a high-write collection (> 10 mutations/sec
  average)**: a warning recommends batched mode.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Enable + snapshot | US-140-AC1 | `invoices` with 2,847 entities | Enable mirror | Single snapshot commit containing all entities |
| Status | US-140-AC3 | Mirror enabled, 3 mutations mirrored | Request status | Last audit position, zero lag, no errors |
| Disable | US-140-AC4 | Mirror enabled | Disable mirror | No further commits; remote repo intact |

## Dependencies

- **Stories**: none
- **Feature Spec**: FEAT-027
- **Feature Requirements**: GIT-01, GIT-02, GIT-09
- **PRD Requirements**: none allocated (deferred; FR-18 consumer)
- **External**: git remote (SSH/HTTPS); future mirror-config Contract;
  FEAT-021 change feed

## Out of Scope

- Commit content and trailers (US-141); shard layouts (US-142); failure
  recovery (US-143); two-way sync (FEAT-027 Out of Scope).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
