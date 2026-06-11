---
ddx:
  id: US-142
---

# US-142: Shard Strategy Organises the Repository

**Feature**: FEAT-027 — Git Mirror (deferred — see `docs/helix/parking-lot.md`)
**Feature Requirements**: GIT-04, GIT-05
**PRD Requirements**: None allocated (deferred); consumes FR-18 change feeds
**Priority**: P2
**Status**: Draft

## Story

**As a** developer navigating the mirror repo
**I want** entities organised in a predictable directory structure
**So that** I can find entities without knowing the full entity ID

## Context

Renumbered from US-080 (collision with the FEAT-004 point-in-time
snapshot story claimed by live code/test tags). Repository layout
determines whether the mirror is browsable and whether it scales. This
story exercises GIT-04 and GIT-05: the four shard strategies (`flat`,
`id_prefix`, `index_field`, `hash`) and the same-commit file-move rule
when an `index_field` value changes.

## Walkthrough

1. The developer enables a mirror with the `index_field` strategy on the
   `status` field (a declared secondary index, FEAT-013).
2. Entities appear under directories named for their status values.
3. An invoice moves from draft to approved; the mirror removes the old
   path and adds the new path in the same commit, recording both paths in
   the commit message.
4. On another collection the developer uses the default `id_prefix`
   strategy and finds entities under stable ID-prefix directories that
   never move.

## Acceptance Criteria

- [ ] **US-142-AC1** — Given the `id_prefix` strategy, when an entity is
      mirrored, then its file is placed under a directory derived from a
      configurable-length prefix of its ID, and the path never changes.
- [ ] **US-142-AC2** — Given the `index_field` strategy on a declared
      index field, when an entity is mirrored, then its file is placed
      under a directory named for the field's value.
- [ ] **US-142-AC3** — Given an `index_field`-sharded entity, when the
      shard field's value changes, then the file move (remove + add)
      happens in the same commit with old and new paths recorded in the
      commit message.
- [ ] **US-142-AC4** — Given the `flat` strategy, when entities are
      mirrored, then all files are placed directly in the collection root
      directory.
- [ ] **US-142-AC5** — Given an enabled mirror, when the operator wants a
      different shard strategy, then the strategy can only be changed by
      disabling and re-enabling the mirror (full re-snapshot).

## Edge Cases

- **`index_field` on a non-indexed field**: rejected at enable time — the
  field must be a declared secondary index (FEAT-013).
- **High-cardinality `index_field`**: layout works but produces many
  directories; `id_prefix` or `hash` is the recommended alternative.
- **`flat` strategy at scale**: usable but degrades around ~10K entities;
  a documentation-level caution, not an enforced limit.
- **`hash` strategy**: uniform two-level distribution, not
  human-navigable — chosen deliberately for scale.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Prefix layout | US-142-AC1 | `id_prefix`, length 4 | Mirror entity `01J3ABCDEF` | File under the `01J3` directory |
| Field layout | US-142-AC2 | `index_field` on `status` | Mirror approved invoice | File under the `approved` directory |
| Shard move | US-142-AC3 | Invoice in `draft` dir | Approve invoice | One commit: old path removed, new path added, both recorded |
| Flat layout | US-142-AC4 | `flat` strategy | Mirror 3 entities | 3 files in collection root |

## Dependencies

- **Stories**: US-140 (mirror enabled), US-141 (commit semantics)
- **Feature Spec**: FEAT-027
- **Feature Requirements**: GIT-04, GIT-05
- **PRD Requirements**: none allocated (deferred)
- **External**: FEAT-013 (secondary index declaration for `index_field`)

## Out of Scope

- Changing strategy in place (requires re-enable, US-142-AC5); per-field
  content exclusion; multi-remote layouts.

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
