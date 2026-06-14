---
ddx:
  id: US-098
  review:
    self_hash: 292020bd6f3751ab85bb768de7b87b2168e06dcbf9c86bc6394f30d5e0b5281f
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# US-098: Generate a Typed Client from Schema

**Feature**: FEAT-024 — Application Substrate
**Feature Requirements**: SUB-01, SUB-02, SUB-03, SUB-04
**PRD Requirements**: PRD P2 #1 (Application substrate); builds on FR-20
**Priority**: P2
**Status**: Draft

## Story

**As a** developer (Ava) starting an Axon-backed application
**I want** a TypeScript client generated from my ESF schemas
**So that** application code gets typed entity operations and local
validation without hand-written API wrappers

## Context

Application teams currently hand-write API wrappers and re-implement
validation that the schema already defines. This story exercises the
typed-client area of FEAT-024 (SUB-01..04): generation from ESF schemas
(CONTRACT-010) into a compilable, tree-shakeable client whose validation
matches the server exactly.

## Walkthrough

1. Ava runs the client generation command against her ESF schema files.
2. Generation emits a TypeScript package with entity types and create,
   read, update, delete, query, and link operations per collection.
3. Ava imports the package into a standard Vite/TypeScript app; it
   compiles without bundler-specific workarounds.
4. Her form code calls the generated validators; an invalid entity is
   rejected locally with the same outcome the server would produce.
5. After a schema change, Ava regenerates: affected types update,
   unaffected collections' artifacts remain stable.

## Acceptance Criteria

- [ ] **US-098-AC1** — Given one or more ESF schemas (CONTRACT-010), when
      client generation runs, then it emits compilable TypeScript.
- [ ] **US-098-AC2** — Given a generated client, when its surface is
      inspected, then it includes entity create, read, update, delete,
      query, and link operations for each collection.
- [ ] **US-098-AC3** — Given any supported constraint type, when the same
      payload is validated client-side and server-side, then both produce
      the same accept/reject outcome.
- [ ] **US-098-AC4** — Given a schema change to one collection, when
      generation is re-run, then affected client types are updated and
      artifacts for unaffected collections are byte-stable.
- [ ] **US-098-AC5** — Given a minimal Vite/TypeScript app, when the
      generated package is imported, then it builds without
      bundler-specific hacks.

## Edge Cases

- **Unsupported constraint construct**: generation fails naming the
  construct and collection; it never emits weaker validation than the
  server enforces.
- **Empty schema set**: generation reports there is nothing to generate
  rather than emitting an empty package.
- **Client older than server schema**: server validation remains
  authoritative; the mismatch surfaces as a server-side validation error.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Compilable output | US-098-AC1 | `invoice.esf`, `vendor.esf` | Run generation; `tsc` | Zero compile errors |
| Operation coverage | US-098-AC2 | Generated client for `invoices` | Inspect exports | CRUD + query + link operations present |
| Validation parity | US-098-AC3 | Invoice missing required `amount` | Validate client-side and POST server-side | Both reject with matching constraint identification |
| Stable regeneration | US-098-AC4 | Add optional field to `invoices` only | Regenerate | `vendors` artifacts unchanged byte-for-byte |

## Dependencies

- **Stories**: none
- **Feature Spec**: FEAT-024
- **Feature Requirements**: SUB-01, SUB-02, SUB-03, SUB-04
- **PRD Requirements**: PRD P2 #1; FR-20
- **External**: CONTRACT-010 (ESF schema format); FEAT-005/FEAT-015
  transports the client wraps

## Out of Scope

- Admin UI generation (US-099); deployment template (US-100);
  non-TypeScript clients (FEAT-024 Out of Scope).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
