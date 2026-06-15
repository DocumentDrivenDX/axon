---
ddx:
  id: US-128
  review:
    self_hash: a46c0aa5575d204361678ad066f236e09f2068de00d28aca27d7afc3bb654c02
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# US-128: Install Axon as a system service

**Feature**: FEAT-028 — Unified Binary & Service Management
**Feature Requirements**: BIN-08, BIN-09, BIN-10
**PRD Requirements**: FR-24
**Priority**: P1
**Status**: Draft

## Story

**As a** developer (Ava) running Axon persistently
**I want** to run `axon server install` to set up a service
**So that** Axon starts automatically and survives reboots — without
accidentally exposing an unauthenticated server

## Context

Renumbered from US-072 (collision with FEAT-009 "Explore Graph via
GraphQL"). Hand-written service units are an adoption blocker and a
security hazard. This story exercises BIN-08 and BIN-09 (install,
lifecycle, uninstall on systemd/launchd) and the BIN-10 guardrail:
service installs default to an authenticated mode, and disabling auth
requires an explicit opt-in flag with a printed warning. Unit locations
and flag names are normative in CONTRACT-008.

## Walkthrough

1. Ava runs `axon server install`; a user-level service unit is written
   at the CONTRACT-008 location and enabled, configured for an
   authenticated mode by default.
2. `axon server start` starts the service; `axon server status` reports
   it running.
3. On a shared host she instead runs the install with `--global` as
   root; a system service is created with a dedicated system user and
   the system data directory.
4. A teammate attempts an install with auth disabled; the command
   requires the explicit opt-in flag and prints a prominent warning
   naming the exposure.
5. Later, `axon server uninstall` disables and removes the unit.

## Acceptance Criteria

- [ ] **US-128-AC1** — Given a user session, when `axon server install`
      runs, then a user-level service is created and enabled (systemd user
      unit on Linux, LaunchAgent on macOS, per CONTRACT-008).
- [ ] **US-128-AC2** — Given an installed service, when
      `axon server start` runs, then the service starts; and when
      `axon server status` runs, then it reports whether the service is
      running.
- [ ] **US-128-AC3** — Given root privileges, when the install runs with
      `--global`, then a system service is created with a dedicated system
      user and the system data directory (per CONTRACT-008).
- [ ] **US-128-AC4** — Given an installed service, when
      `axon server uninstall` runs, then the service is disabled and its
      unit removed.
- [ ] **US-128-AC5** — Given a default install, when the service unit is
      inspected, then it runs in an authenticated mode — never
      unauthenticated by default.
- [ ] **US-128-AC6** — Given an operator who wants an unauthenticated
      service, when they install without the explicit no-auth opt-in flag,
      then the install refuses unauthenticated mode; and when they supply
      the flag, then a prominent warning is printed and the mode is
      visible via `axon doctor`.

## Edge Cases

- **Service already installed**: install reports it and exits cleanly;
  reinstall requires uninstall first.
- **`--global` without root**: clear permission error, non-zero exit, no
  partial installation.
- **Unsupported init system**: a descriptive error names the supported
  platforms (systemd 240+, launchd 10.13+).

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| User install | US-128-AC1 | No service installed | `axon server install` | Unit created at CONTRACT-008 path; enabled |
| Lifecycle | US-128-AC2 | Service installed | start, then status | Running reported; stop/restart symmetrical |
| Secure default | US-128-AC5 | Default install | Inspect unit/resolved config | Authenticated mode configured |
| No-auth opt-in | US-128-AC6 | Install requesting no-auth without flag | Run install | Refused; with flag: warning printed, mode visible in doctor |

## Dependencies

- **Stories**: US-126 (`axon serve` is what the unit runs), US-127
  (doctor surfaces the auth mode)
- **Feature Spec**: FEAT-028
- **Feature Requirements**: BIN-08, BIN-09, BIN-10
- **PRD Requirements**: FR-24
- **External**: CONTRACT-008 (unit paths/contents, flags); systemd /
  launchd; ADR-005/ADR-018 (transport authentication modes)

## Out of Scope

- Remote-machine service management; Windows services; auth-mode
  implementation itself (FEAT-012).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
