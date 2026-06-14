---
ddx:
  id: US-124
  review:
    self_hash: a76b33f314e1bc800ed6f1042e158ceb0ae3d1516f74007265ce2b59830688ee
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# US-124: Per-Principal Role Assignment

**Feature**: FEAT-012 — Authentication, Identity, and Authorization
**Feature Requirements**: AUZ-24
**PRD Requirements**: FR-25
**Priority**: P1
**Status**: Approved

## Story

**As an** operator (Wei, Business Workflow Builder persona)
**I want** to assign roles directly to user principals
**So that** I control authorization in Axon without needing to configure Tailscale ACL tags

## Context

Renumbered from US-048 (collision with FEAT-015). Tag-derived roles require
control over the tailnet ACL configuration, which many operators do not have.
Explicit per-principal assignments give Axon-native role control that
overrides tag mapping, persists across restarts, and is manageable from the
CLI and the control-plane API (surfaces per CONTRACT-008 and CONTRACT-001).

## Walkthrough

1. Operator assigns the admin role to a principal via the CLI.
2. System persists the assignment; within the identity cache TTL the
   principal's requests carry the admin role.
3. Operator lists assignments and sees the new entry.
4. Operator revokes the assignment; the principal falls back to its
   tag-derived role, then to the default role.

## Acceptance Criteria

- [ ] **US-124-AC1** — Given an operator with admin authority, when they
  assign a role to a principal, then subsequent requests from that principal
  carry the assigned role.
- [ ] **US-124-AC2** — Given an explicit assignment exists, when it is
  revoked, then the principal falls back to its tag-derived role and, absent
  tags, to the configured default role.
- [ ] **US-124-AC3** — Given multiple assignments, when the operator lists
  them (CLI or control-plane API), then all explicit assignments are
  returned; management operations are admin-only.
- [ ] **US-124-AC4** — Given assignments exist, when the server restarts,
  then the assignments survive.
- [ ] **US-124-AC5** — Given an assignment change, when the identity cache
  TTL elapses, then the change is in effect.
- [ ] **US-124-AC6** — Given both an explicit assignment and a tag-derived
  role, when authorization is evaluated, then the explicit assignment takes
  priority.

## Edge Cases

- **Assignment for an unknown principal**: accepted ahead of first login
  (consistent with pre-provisioned users) or rejected with a structured
  error — behavior must be consistent between CLI and API surfaces.
- **Non-admin caller**: assignment management attempts are rejected as
  forbidden.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Grant role | US-124-AC1 | Principal with `read` tag role | Assign `admin` | Requests carry admin after TTL |
| Revoke and fall back | US-124-AC2 | Explicit `admin` + `write` tag | Revoke assignment | Role falls back to `write` |
| List assignments | US-124-AC3 | 2 assignments | List via CLI and API | Both returned; non-admin denied |
| Restart persistence | US-124-AC4 | 1 assignment | Restart server | Assignment still effective |
| TTL propagation | US-124-AC5 | Change role | Wait cache TTL | New role enforced |
| Priority | US-124-AC6 | Tag says `read`, assignment says `write` | Authorize a write | Write allowed |

## Dependencies

- **Stories**: US-043 (identity resolution), US-089 (stable users)
- **Feature Spec**: FEAT-012
- **Feature Requirements**: AUZ-24
- **PRD Requirements**: FR-25
- **External**: CONTRACT-008 (CLI commands and flags), CONTRACT-001
  (control-plane routes)

## Out of Scope

- Per-collection or per-row authorization (FEAT-029).
- Tenant membership roles (US-091) — this story covers deployment-level
  principal assignments.

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
