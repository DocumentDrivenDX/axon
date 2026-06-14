---
ddx:
  id: US-089
  review:
    self_hash: f5ef9c92e022907c992bc43fcba94818f678c3a622243b4231b0f22a6891af61
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# US-089: First-Class User with Tailscale Auto-Provisioning

**Feature**: FEAT-012 — Authentication, Identity, and Authorization
**Feature Requirements**: AUZ-01, AUZ-02, AUZ-03, AUZ-04, AUZ-05, AUZ-21
**PRD Requirements**: FR-25
**Priority**: P1
**Status**: Approved

## Story

**As a** developer logging in via Tailscale (Ava, Agent Application Developer persona)
**I want** my Tailscale identity to resolve to a stable user row
**So that** my audit entries are attributed to a consistent user even if my tailnet handle changes

## Context

Users are first-class and provider-independent: the external identity is just
a federation mapping onto a stable user. Auto-provisioning preserves the
zero-onboarding tailnet experience while upgrading the underlying identity
model (ADR-018).

## Walkthrough

1. Developer makes a first request from a previously-unseen tailnet identity.
2. System creates a user (display name from the tailnet) and the federation
   mapping atomically.
3. Developer makes subsequent requests; the system resolves the same user via
   the federation lookup.
4. An operator renames the user; the next audit entry uses the new display
   name while the user ID stays constant.

## Acceptance Criteria

- [ ] **US-089-AC1** — Given a previously-unseen tailnet identity, when its
  first request arrives, then a user and its Tailscale federation mapping are
  created together.
- [ ] **US-089-AC2** — Given an auto-provisioned user, when subsequent
  requests arrive from the same tailnet identity, then they resolve to the
  same user ID via the federation lookup.
- [ ] **US-089-AC3** — Given a resolved user, when audit entries are written,
  then they carry the user's display name or email, not the raw tailnet
  handle.
- [ ] **US-089-AC4** — Given an operator renames the user, when the user next
  mutates data, then the audit entry uses the new display name.
- [ ] **US-089-AC5** — Given an operator changes the tailnet-handle mapping,
  when the mapped identity authenticates, then the existing user record is
  reused and no duplicate user is created.
- [ ] **US-089-AC6** — Given no-auth mode, when requests are served, then no
  user records are persisted and an anonymous context is synthesized in
  memory.

## Edge Cases

- **Concurrent first-seen requests**: parallel requests for the same external
  identity converge on a single user record (ADR-018 concurrency invariant).
- **Suspended user**: a suspended user's requests are rejected even with a
  valid federation mapping.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| First contact | US-089-AC1 | Unseen handle `dana@tailnet` | First request | User + mapping created atomically |
| Stable resolution | US-089-AC2 | Provisioned `dana` | Second request | Same user ID resolved |
| Display attribution | US-089-AC3 | `dana` mutates entity | Read audit | Actor is display identity |
| Rename | US-089-AC4 | Rename to `Dana Q.` | Mutate again | New display name in audit |
| Remap without dup | US-089-AC5 | Handle remapped | Authenticate | Existing user reused |
| No-auth purity | US-089-AC6 | `--no-auth` server | Serve requests | Zero persisted users/tenants |

## Dependencies

- **Stories**: US-043
- **Feature Spec**: FEAT-012
- **Feature Requirements**: AUZ-01 through AUZ-05, AUZ-21
- **PRD Requirements**: FR-25
- **External**: ADR-018 (identity model, concurrency invariant), ADR-005
  (Tailscale provider), CONTRACT-001 (user management routes)

## Out of Scope

- Additional federation providers (OIDC, email+password).
- Tenant membership semantics (US-091).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
