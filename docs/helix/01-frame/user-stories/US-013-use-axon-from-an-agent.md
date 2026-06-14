---
ddx:
  id: US-013
  review:
    self_hash: 7b51e45dcf8c5000b863429086d35d4075b59ebe6d0428e10748754115ff514f
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# US-013: Use Axon from an Agent

**Feature**: FEAT-005 — API Surface
**Feature Requirements**: API-08, API-09, API-15
**PRD Requirements**: FR-22, FR-29
**Priority**: P0
**Status**: Draft

## Story

**As** Ava, an agent application developer building an agent framework
integration
**I want** a typed, GraphQL-first client SDK for Axon operations
**So that** my agents can store, query, and safely mutate state without
hand-assembling HTTP requests or guardrail code

## Context

Agent frameworks need programmatic access to Axon with stable, structured
semantics: typed calls, machine-matchable errors, and the governed write
workflow (preview, intent, approval, commit) as first-class verbs. This story
exercises FEAT-005's SDK surface requirements (API-08, API-09) and the shared
error model (API-15). The normative SDK surface is CONTRACT-009.

## Walkthrough

1. Ava installs the first-party TypeScript SDK and constructs a client scoped
   to her tenant and database.
2. Her agent creates, reads, updates, deletes, and queries entities through
   typed SDK calls.
3. For a risky write, the agent previews the mutation, receives a diff with a
   policy decision and an intent token, and submits the intent for approval.
4. A reviewer approves the intent; the agent commits it and receives the
   committed result with audit references.
5. When something fails — validation, version conflict, policy denial — the
   SDK surfaces a structured error the agent matches on programmatically and
   handles without parsing message strings.

## Acceptance Criteria

- [ ] **US-013-AC1** — Given the published first-party TypeScript SDK, when
  Ava's agent performs create, read, update, delete, and query operations,
  then each operation succeeds through typed SDK calls per CONTRACT-009.
- [ ] **US-013-AC2** — Given an approval-routed write, when the agent uses
  the SDK's governed-workflow verbs (preview mutation, commit intent, approve
  intent, reject intent, explain policy, query audit, rollback dry-run per
  CONTRACT-009), then the full preview-to-commit workflow completes without
  any direct HTTP assembly.
- [ ] **US-013-AC3** — Given a failed operation, when the SDK returns an
  error, then the error is a structured type with a stable code the agent can
  match on programmatically (error types per CONTRACT-009).
- [ ] **US-013-AC4** — Given a policy denial, stale intent, or version
  conflict, when the SDK surfaces the error, then policy, intent, conflict,
  stale-dimension, and audit-reference fields from the shared handler
  contract are preserved on the error object.
- [ ] **US-013-AC5** — Given the same SDK program, when it runs against an
  embedded-mode Axon and a server-mode Axon, then observable behavior and
  results are identical.

## Edge Cases

- **Server unreachable**: The SDK returns a connection error with retry
  guidance rather than hanging or throwing an untyped error.
- **Stale intent commit**: Committing an intent whose pre-image changed
  returns a structured stale error naming the stale dimension; no mutation
  applies.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Typed CRUD round-trip | US-013-AC1 | Empty `invoices` collection with active schema | SDK create, get, update, query, delete an invoice | All calls succeed with typed results; version increments on update |
| Governed write end-to-end | US-013-AC2 | Policy routes invoice amount > 10000 for approval | Preview a 12000 update, approve the intent, commit it | Preview returns diff + intent token; commit succeeds after approval with audit reference |
| Structured error matching | US-013-AC3 | Entity at version 5 | SDK update with expected version 4 | Error with stable conflict code; agent branch on code succeeds |
| Error field preservation | US-013-AC4 | Policy denies field write for subject | SDK attempts the denied write | Error carries policy explanation and denied field path fields |
| Mode parity | US-013-AC5 | Same fixture data embedded and server | Run identical SDK script in both modes | Byte-equivalent logical results (ignoring timestamps/IDs) |

## Dependencies

- **Stories**: None
- **Feature Spec**: FEAT-005
- **Feature Requirements**: API-08, API-09, API-15
- **PRD Requirements**: FR-22, FR-29
- **External**: CONTRACT-009 (SDK surface), CONTRACT-002 (GraphQL documents
  the SDK emits), CONTRACT-001 (compatibility routes)

## Out of Scope

- SDKs for languages other than TypeScript (first-party V1 scope).
- MCP tool access for agents (US-052 through US-056, FEAT-016).
- Defining SDK method names or signatures here — CONTRACT-009 is normative.

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
