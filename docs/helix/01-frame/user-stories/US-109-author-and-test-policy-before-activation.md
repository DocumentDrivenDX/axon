---
ddx:
  id: US-109
---

# US-109: Author And Test Policy Before Activation

**Feature**: FEAT-029 — Data-Layer Access Control Policies
**Feature Requirements**: ACL-01, ACL-02, ACL-03, ACL-04
**PRD Requirements**: FR-10, FR-14
**Priority**: P0
**Status**: Approved

## Story

**As an** application developer (Ava, Agent Application Developer persona)
**I want** schema policy changes to compile, explain, and run against fixtures before activation
**So that** I can prove row, field, relationship, and approval behavior before agents touch live data

## Context

Policies are schema changes: compiled, type-checked, diffed, and audited.
The authoring loop — edit, dry-run compile, fixture-test, optionally replay
audit history, activate — is what keeps policy mistakes from reaching live
agents. The grammar and compile-failure codes are defined in CONTRACT-004.

## Walkthrough

1. Developer edits a collection's access-control block alongside the schema.
2. Developer requests a dry-run compile; the system returns a compile report
   without changing the active policy version.
3. Developer evaluates fixture subjects and sample mutations against the
   candidate policy.
4. Developer activates; the system applies the schema/policy version
   atomically, refreshing generated GraphQL and MCP views, and audits the
   change.

## Acceptance Criteria

- [ ] **US-109-AC1** — Given a candidate policy, when a dry-run schema update
  is submitted, then a policy compile report is returned and the active
  policy version is unchanged.
- [ ] **US-109-AC2** — Given invalid field paths, subject references, or
  relationship-policy cycles, when the schema write is submitted, then it is
  rejected at write time with the stable invalid-expression reason
  (CONTRACT-004).
- [ ] **US-109-AC3** — Given a compile report, when redaction makes generated
  GraphQL fields nullable, then the report names those fields.
- [ ] **US-109-AC4** — Given fixture subjects and sample mutations, when they
  are evaluated against the candidate policy, then per-fixture decisions are
  returned without touching live data.
- [ ] **US-109-AC5** — Given fixture evaluations across surfaces, when
  GraphQL, MCP, SDK, and CLI policy metadata are compared for the same
  subject, resource, operation, and policy version, then they match.
- [ ] **US-109-AC6** — Given an activation, when the policy change applies,
  then it is audited with the old and new policy versions.

## Edge Cases

- **Unindexable predicates**: the compile report surfaces predicates that
  cannot be index-assisted and the index they require.
- **Concurrent activation**: in-flight requests keep their policy snapshot;
  the new version applies only to requests starting after activation.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Dry-run compile | US-109-AC1 | Candidate policy v(n+1) | Dry-run | Report returned; active stays v(n) |
| Invalid policy | US-109-AC2 | Rule with bad field path | Schema write | Rejected, invalid-expression reason |
| Nullability report | US-109-AC3 | New redaction rule | Compile | Report names affected GraphQL fields |
| Fixture evaluation | US-109-AC4 | Named subjects + sample mutation | Evaluate | Decisions returned, no data touched |
| Surface parity | US-109-AC5 | Same fixture tuple | Compare 4 surfaces | Identical metadata |
| Audited activation | US-109-AC6 | Activate v(n+1) | Read audit | Entry with v(n) → v(n+1) |

## Dependencies

- **Stories**: None
- **Feature Spec**: FEAT-029
- **Feature Requirements**: ACL-01 through ACL-04
- **PRD Requirements**: FR-10, FR-14
- **External**: CONTRACT-004 (grammar, compile semantics, reason codes),
  CONTRACT-010 (ESF schema format), CONTRACT-002 (generated GraphQL)

## Out of Scope

- The web UI authoring experience (FEAT-031, US-114).
- Audit-replay tooling beyond identifying changed decisions.

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
