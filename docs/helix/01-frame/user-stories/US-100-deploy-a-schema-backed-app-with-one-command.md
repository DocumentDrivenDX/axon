---
ddx:
  id: US-100
  review:
    self_hash: 9f46c8246e5f3fde22bb75d055cfc040f346966a44034a49b362443bf401a395
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# US-100: Deploy a Schema-Backed App with One Command

**Feature**: FEAT-024 — Application Substrate
**Feature Requirements**: SUB-10, SUB-11, SUB-12, SUB-13
**PRD Requirements**: PRD P2 #1 (Application substrate)
**Priority**: P2
**Status**: Draft

## Story

**As an** application developer (Ava)
**I want** a template that packages Axon, generated client code, generated
UI, and deployment config
**So that** I can move from schema to running app without bespoke
infrastructure work

## Context

Even with a generated client and UI, deployment is where projects stall.
This story exercises the deployment-template area of FEAT-024
(SUB-10..13): one command to a running, health-checked local instance, and
configuration for the V1 reference cloud target (Cloud Run — recorded as
an assumption in the feature spec). Regeneration must never clobber
operator-owned configuration.

## Walkthrough

1. Ava scaffolds her app from the deployment template.
2. The template builds a container holding the Axon server (FEAT-028
   unified binary), generated UI assets, and generated client artifacts.
3. She runs the single start command; a local instance comes up with a
   passing health check, a default tenant/database, and the generated UI
   served.
4. She deploys the same artifact using the included Cloud Run
   configuration, with health checks and persistent storage configured.
5. After a schema change she regenerates; client and UI artifacts update
   while her operator-owned configuration is untouched.

## Acceptance Criteria

- [ ] **US-100-AC1** — Given the template, when it is built, then the
      resulting container contains the Axon server, generated UI assets,
      and generated client artifacts.
- [ ] **US-100-AC2** — Given a built template, when the single start
      command runs, then a local instance is up with a passing health
      check, a default tenant/database, and the generated UI served.
- [ ] **US-100-AC3** — Given the reference cloud configuration, when it is
      inspected/deployed, then it includes health checks and persistent
      storage configuration for Cloud Run.
- [ ] **US-100-AC4** — Given operator-owned configuration files, when
      generation is re-run after a schema change, then client and UI
      artifacts update and operator-owned files are not deleted or
      overwritten.
- [ ] **US-100-AC5** — Given the template documentation, when a developer
      reads it, then it identifies which files are generated and which are
      safe for application-specific edits.

## Edge Cases

- **Start command with a port conflict**: fails with a clear error naming
  the port, consistent with FEAT-028 serve behavior.
- **Cloud deployment without persistent storage configured**: the template
  fails validation rather than deploying an instance that loses data on
  restart.
- **Regeneration on a dirty working tree**: only generated paths change;
  the generated/editable boundary makes review trivial.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Container contents | US-100-AC1 | Template for invoice app | Build container | Server binary, UI assets, client artifacts present |
| One-command run | US-100-AC2 | Built container | Run start command | Health check passes; UI reachable; default tenant/database exists |
| Safe regeneration | US-100-AC4 | Operator-edited config + schema change | Re-run generation | Generated artifacts updated; operator file untouched |

## Dependencies

- **Stories**: US-098, US-099 (artifacts the template packages)
- **Feature Spec**: FEAT-024
- **Feature Requirements**: SUB-10, SUB-11, SUB-12, SUB-13
- **PRD Requirements**: PRD P2 #1
- **External**: FEAT-028 (unified binary / `axon serve`); container
  tooling; Cloud Run (V1 reference target)

## Out of Scope

- Other cloud platforms (Cloudflare Workers etc. — FEAT-024 Out of
  Scope); DNS/TLS/secrets/autoscaling operations; client and UI
  generation logic (US-098, US-099).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
