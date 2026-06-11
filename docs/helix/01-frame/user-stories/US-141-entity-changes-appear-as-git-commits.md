---
ddx:
  id: US-141
---

# US-141: Entity Changes Appear as Git Commits

**Feature**: FEAT-027 — Git Mirror (deferred — see `docs/helix/parking-lot.md`)
**Feature Requirements**: GIT-06, GIT-07, GIT-08, GIT-10
**PRD Requirements**: None allocated (deferred); consumes FR-18 change feeds
**Priority**: P2
**Status**: Draft

## Story

**As a** developer reviewing entity changes
**I want** each Axon mutation to appear as a git commit
**So that** I can use `git log`, `git diff`, and `git blame` on entity
history

## Context

Renumbered from US-079 (collision with the FEAT-003 multi-collection
audit tail story claimed by live code tags). The mirror's value is that
data changes read like code changes. This story exercises projection
content (GIT-06..08, GIT-10): one mutation per commit, transactional
atomicity, audit-linked trailers, and history-preserving deletions.
Normative trailer keys will live in a Contract when FEAT-027 is
scheduled.

## Walkthrough

1. An agent creates an invoice; the mirror commits a new file for it.
2. The agent updates the invoice; the mirror commits a modification, and
   `git diff` shows the field-level change in readable JSON.
3. A multi-entity transaction commits; the mirror produces exactly one
   commit covering all affected files.
4. The reviewer reads the commit message trailers and follows the audit
   ID back to the authoritative audit record.
5. The agent deletes the invoice; the mirror commits the file removal,
   and `git log` on the path recovers the full history including the
   deletion.

## Acceptance Criteria

- [ ] **US-141-AC1** — Given mirroring is enabled, when an entity is
      created, then a git commit adds the entity's file at its shard path.
- [ ] **US-141-AC2** — Given an existing mirrored entity, when it is
      updated or patched, then a git commit modifies its file and
      `git diff` shows field-level changes in readable JSON.
- [ ] **US-141-AC3** — Given an existing mirrored entity, when it is
      deleted, then a git commit removes the file and the file's history
      remains recoverable from git history.
- [ ] **US-141-AC4** — Given any mirror commit, when its message is read,
      then machine-readable trailers link it to the audit record (audit
      ID, entity, version, operation, actor, transaction, timestamp).
- [ ] **US-141-AC5** — Given a multi-entity transaction, when it is
      mirrored, then exactly one git commit covers all its file changes.
- [ ] **US-141-AC6** — Given any mirror commit, when its author is read,
      then it reflects the Axon actor (agent or user identity).

## Edge Cases

- **Entity ID with filesystem-invalid characters**: percent-encoded in
  paths; trailers carry the canonical ID.
- **Round-trip**: the JSON file fully reconstructs the entity, including
  system metadata (GIT-06).
- **Batched mode**: multiple transactions in one window produce one
  commit per transaction within the batch push, preserving atomicity.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Create commit | US-141-AC1 | Mirror on `invoices` | Create `INV-1` | Commit adds the entity file at its shard path |
| Readable diff | US-141-AC2 | `INV-1` status draft | Update to approved | `git diff` shows the status field change |
| Transaction atomicity | US-141-AC5 | Transaction touching 3 entities | Commit transaction | One git commit with 3 file changes |
| Audit linkage | US-141-AC4 | Any mirrored mutation | Read commit trailers | Audit ID resolves to the audit record |

## Dependencies

- **Stories**: US-140 (mirror enabled)
- **Feature Spec**: FEAT-027
- **Feature Requirements**: GIT-06, GIT-07, GIT-08, GIT-10
- **PRD Requirements**: none allocated (deferred); FR-15/FR-17 audit
  lineage is the linked authority
- **External**: git tooling; future trailer-format Contract

## Out of Scope

- Shard strategy mechanics (US-142); recovery behavior (US-143);
  markdown-format files (informational only, GIT-06).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
