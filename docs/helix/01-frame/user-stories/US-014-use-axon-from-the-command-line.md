---
ddx:
  id: US-014
  review:
    self_hash: 11d1436fc838d8d638f712b62d50b193433184f6819c80b173220ac63abfe314
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# US-014: Use Axon from the Command Line

**Feature**: FEAT-005 — API Surface
**Feature Requirements**: API-10, API-11, API-12, API-13
**PRD Requirements**: FR-22, FR-24
**Priority**: P0
**Status**: Draft

## Story

**As** Wei, a business workflow builder managing an Axon deployment
**I want** CLI commands for every Axon operation
**So that** I can inspect and manage data, policy, and history without
writing code

## Context

Operators need terminal access to the same governed operations applications
use: listing and querying entities, inspecting audit history with diff/blame
ergonomics, and dry-running recovery — with machine-readable output for
scripting. This story exercises FEAT-005's CLI surface requirements (API-10
through API-13). The normative command tree, flags, and client-mode rules are
CONTRACT-008.

## Walkthrough

1. Wei runs the CLI with no arguments and sees help describing the command
   tree.
2. Wei lists entities in a collection and reads them in a human-readable
   table.
3. Wei narrows the listing with a filter expression and sees only matching
   entities.
4. Wei inspects recent audit history for the collection, then drills into a
   diff/blame view showing what changed, who changed it, the policy and
   approval decisions, and the transaction and audit identifiers.
5. Wei dry-runs a rollback and reviews the compensating operations and
   conflicts without mutating state.
6. Wei re-runs a command with JSON output and pipes it into a script.

## Acceptance Criteria

- [ ] **US-014-AC1** — Given a collection with entities, when Wei lists
  entities via the CLI (command form per CONTRACT-008), then they render in a
  readable table by default.
- [ ] **US-014-AC2** — Given entities with mixed field values, when Wei
  queries with a filter expression (per CONTRACT-008), then only matching
  entities are returned.
- [ ] **US-014-AC3** — Given recent mutations, when Wei lists audit entries
  scoped to a collection with a result limit, then the most recent changes
  are shown.
- [ ] **US-014-AC4** — Given an audited mutation, when Wei uses the audit
  diff and blame views, then the output shows changed fields, actor and tool
  origin, policy decision, approval decision, transaction ID, and audit IDs.
- [ ] **US-014-AC5** — Given a recoverable bad write, when Wei runs a
  rollback dry-run, then the compensating operations and conflicts are shown
  and no state is mutated.
- [ ] **US-014-AC6** — Given any CLI read command, when Wei requests JSON
  output, then the output is machine-parseable and identical in embedded and
  client modes.
- [ ] **US-014-AC7** — Given no arguments, when Wei runs the CLI, then help
  is shown.
- [ ] **US-014-AC8** — Given a reachable server at the configured URL, when
  Wei runs a CLI command without mode flags, then the command executes in
  client mode against the server; given no reachable server, it falls back to
  embedded mode within the CONTRACT-008 connection timeout.

## Edge Cases

- **Server stops between commands**: The next command falls back to embedded
  mode per the CONTRACT-008 connection rules; output format is unchanged.
- **Filter matches nothing**: An empty table (or empty JSON array) is
  returned with a zero exit code, not an error.
- **Dry-run conflicts**: A rollback dry-run that detects conflicting later
  writes reports them explicitly instead of silently planning over them.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Table listing | US-014-AC1 | `beads` collection with 5 entities | List entities via CLI | 5 rows in a readable table |
| Filtered query | US-014-AC2 | 3 pending, 2 done beads | Query with `status=pending` filter | Exactly the 3 pending beads |
| Audit tail | US-014-AC3 | 12 mutations on `beads` | List audit, last 10, collection-scoped | 10 most recent entries |
| Blame view | US-014-AC4 | Approved high-value invoice update | Audit diff/blame on the entity | Changed fields + actor/tool + policy/approval + txn/audit IDs |
| Safe dry-run | US-014-AC5 | Bad transaction in history | Rollback dry-run | Compensating ops listed; subsequent reads show unchanged state |
| JSON output | US-014-AC6 | Any listing | Re-run with JSON output flag | Valid JSON, parses in a script, same content as table run |
| Mode fallback | US-014-AC8 | No server running | Run a list command without flags | Command completes in embedded mode |

## Dependencies

- **Stories**: None
- **Feature Spec**: FEAT-005
- **Feature Requirements**: API-10, API-11, API-12, API-13
- **PRD Requirements**: FR-22, FR-24
- **External**: CONTRACT-008 (CLI and config), CONTRACT-001 (HTTP routes used
  in client mode)

## Out of Scope

- Admin web UI workflows (FEAT-011).
- Defining exact commands, flags, or config keys here — CONTRACT-008 is
  normative.
- Rollback commit execution semantics (FEAT-023 stories).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
