---
ddx:
  id: US-104
  review:
    self_hash: dd5ae5a9e12a71e898814cc2f50fda0333bbdf372b2badd70c2cd36a35f9ea56
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# US-104: Explain Effective Policy

**Feature**: FEAT-029 — Data-Layer Access Control Policies
**Feature Requirements**: ACL-16, ACL-17
**PRD Requirements**: FR-12, FR-14
**Priority**: P0
**Status**: Approved

## Story

**As a** browser client (application built by Ava, Agent Application
Developer persona)
**I want** to query my effective collection/entity policy
**So that** I can hide unavailable controls without trusting the browser for security

## Context

Clients need advisory policy metadata to build honest UIs: which operations
are available, which fields will be redacted, and why a proposed operation
would be denied. Metadata must be identical across every generated surface,
and enforcement must always repeat in the real execution path.

## Walkthrough

1. Client queries its effective policy for a collection.
2. System returns allowed operations, redacted and denied fields, and the
   policy version (GraphQL fields per CONTRACT-002).
3. Client requests a dry-run explanation for a proposed operation.
4. System returns the decision, reason, matching policy, and field paths
   without executing anything.

## Acceptance Criteria

- [ ] **US-104-AC1** — Given an authenticated client, when it queries
  effective collection policy metadata, then allowed operations, redacted
  fields, denied fields, and policy version are returned.
- [ ] **US-104-AC2** — Given a proposed operation, when the client requests a
  dry-run explanation, then the decision, reason, matching policy, and field
  paths are returned without executing the operation.
- [ ] **US-104-AC3** — Given the same subject, resource, operation, and
  policy version, when policy metadata is read via MCP, SDK, CLI, or operator
  surfaces, then it preserves the same policy version, decision, reason,
  redacted fields, and approval route as GraphQL.
- [ ] **US-104-AC4** — Given any introspection result, when the real
  operation is executed, then enforcement is evaluated again in the execution
  path regardless of the advisory answer.

## Edge Cases

- **Policy version change between explain and execute**: the execution-path
  decision wins; the client can detect the drift via the policy version.
- **Hidden entity in explanation**: explanations never confirm the existence
  of entities the subject cannot see.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Effective policy | US-104-AC1 | Contractor subject, engagements | Query effective policy | Caps + redacted fields + version |
| Dry-run explain | US-104-AC2 | Proposed denied update | Explain | Decision deny, named policy, field paths; no mutation |
| Surface parity | US-104-AC3 | Same tuple via GraphQL/MCP/SDK/CLI | Compare metadata | Identical machine-readable fields |
| Advisory only | US-104-AC4 | Stale allow explanation | Execute after policy narrows | Execution denies |

## Dependencies

- **Stories**: US-109 (active policy exists)
- **Feature Spec**: FEAT-029
- **Feature Requirements**: ACL-16, ACL-17
- **PRD Requirements**: FR-12, FR-14
- **External**: CONTRACT-002 (GraphQL policy fields), CONTRACT-004
  (introspection semantics), CONTRACT-003 (MCP envelopes), CONTRACT-009 (SDK
  metadata)

## Out of Scope

- Mutation preview with bound intent tokens (FEAT-030).
- Operator UI rendering of explanations (FEAT-031).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
