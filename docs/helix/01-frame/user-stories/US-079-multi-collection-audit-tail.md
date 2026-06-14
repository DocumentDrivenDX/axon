---
ddx:
  id: US-079
  review:
    self_hash: 41a0bdb40e3db24f7a9e6d0303c27c5e613e61fa8cf74db4e9372d8f5fdf7228
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---

# US-079: Multi-Collection Audit Tail

**Feature**: FEAT-003 — Audit Log
**Feature Requirements**: AUD-05, AUD-08, AUD-09
**PRD Requirements**: FR-16
**Priority**: P0
**Status**: Draft

## Story

**As** Wei, a business workflow builder observing a cross-collection workflow
**I want** to follow one ordered audit stream spanning several collections
**So that** I can watch a business process (invoice → approval → ledger) without merging per-collection queries myself

## Context

> **Reconstructed story (2026-06-10).** This story ID is claimed by live code
> and test coverage tags but had no spec heading; its content is reconstructed
> from FEAT-003's audit-query requirements (AUD-05 ordering, AUD-08 filters,
> AUD-09 multi-collection tail) per the user-story ID registry. Treat the
> acceptance criteria as the governing statement going forward.

Business processes span collections, but a per-collection audit query forces
client-side merging with no ordering guarantee. This story exercises AUD-09:
one ordered tail over a chosen set of collections.

## Walkthrough

1. Wei starts an audit tail naming several collections (surface per CONTRACT-001 audit tail endpoint).
2. The system delivers existing entries matching the filter in entry-ID order, then continues streaming new entries as mutations commit.
3. Entries from all named collections appear interleaved in one totally ordered stream.
4. Wei stops the tail; resuming from the last seen entry ID yields no gaps or duplicates.

## Acceptance Criteria

- [ ] **US-079-AC1** — Given mutations committed across three collections, when Wei tails audit entries for those collections, then entries from all three appear in one stream ordered by audit entry ID.
- [ ] **US-079-AC2** — Given an active tail, when a new mutation commits in any named collection, then its entry is delivered to the stream without polling.
- [ ] **US-079-AC3** — Given a tail restricted to two of three collections, when mutations commit in all three, then only entries from the two named collections are delivered.
- [ ] **US-079-AC4** — Given a tail resumed from the last received entry ID, when new entries exist, then delivery continues with no gaps and no duplicates.

## Edge Cases

- **Collection dropped mid-tail**: previously delivered entries stand; the drop event itself is delivered as an audited operation; the stream continues for remaining collections.
- **No matching collections**: the tail starts and remains empty rather than erroring.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Happy path | US-079-AC1 | Writes to `invoices`, `approvals`, `ledger` | Tail all three | Single stream, entry-ID ordered, all three collections present |
| Live delivery | US-079-AC2 | Tail open | Commit a new `invoices` write | Entry arrives on the stream |
| Scoping | US-079-AC3 | Tail `invoices`,`ledger` only | Commit to `approvals` | No `approvals` entry delivered |
| Resume | US-079-AC4 | Tail stopped after entry #120 | Resume after #120 | Delivery starts at #121, no duplicates |

## Dependencies

- **Stories**: US-007 (audit query filters)
- **Feature Spec**: FEAT-003
- **Feature Requirements**: AUD-05, AUD-08, AUD-09
- **PRD Requirements**: FR-16
- **External**: CONTRACT-001 (audit tail endpoint and filters), CONTRACT-005 (entry shape and ordering)

## Out of Scope

- Cross-database tails (ordering is per-database, AUD-05).
- Durable change-feed delivery with consumer offsets (FEAT-021 CDC).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
