---
ddx:
  id: US-126
---

# US-126: Start Axon from a single binary

**Feature**: FEAT-028 — Unified Binary & Service Management
**Feature Requirements**: BIN-01, BIN-02, BIN-05
**PRD Requirements**: FR-23, FR-24
**Priority**: P1
**Status**: Draft

## Story

**As a** developer (Ava) adopting Axon
**I want** to run `axon serve` to start a server
**So that** I can begin using Axon without managing separate binaries

## Context

Renumbered from US-070 (collision with FEAT-009 "Find Link Targets").
First-run experience is the adoption gate: one binary, one command, sane
defaults. This story exercises BIN-01, BIN-02, and BIN-05 — foreground
serve with graceful shutdown, default embedded storage at standard paths,
and safe config auto-creation. Default ports, paths, and flags are
normative in CONTRACT-008.

## Walkthrough

1. Ava runs `axon serve` on a clean machine.
2. The HTTP gateway starts in the foreground on the default port
   (CONTRACT-008) with embedded SQLite storage at the standard data
   directory.
3. A commented default config file is created at the standard config
   path, since none existed.
4. Ava confirms the health endpoint responds.
5. She presses Ctrl+C; the server shuts down gracefully.

## Acceptance Criteria

- [ ] **US-126-AC1** — Given a machine with Axon installed, when
      `axon serve` runs, then the HTTP gateway starts on the CONTRACT-008
      default port.
- [ ] **US-126-AC2** — Given a running server, when the health endpoint is
      requested, then it reports healthy.
- [ ] **US-126-AC3** — Given a running foreground server, when it receives
      an interrupt (Ctrl+C), then it shuts down gracefully.
- [ ] **US-126-AC4** — Given no explicit storage configuration, when the
      server starts, then storage is embedded SQLite at the CONTRACT-008
      standard data directory.
- [ ] **US-126-AC5** — Given no config file exists, when the server first
      runs, then a commented default config file is created at the
      standard path — and an existing config file is never overwritten.

## Edge Cases

- **Port already in use**: the server reports the conflicting port and
  exits non-zero.
- **Config file parse error**: clear error with line number, non-zero
  exit; no silent fallback to defaults.
- **Data directory not writable**: the path and permission error are
  printed with remediation suggestions.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| First run | US-126-AC1 | Clean machine, no config | `axon serve` | Gateway up on default port; config auto-created |
| Health | US-126-AC2 | Server running | Request health endpoint | Healthy response |
| Graceful stop | US-126-AC3 | Server running, in-flight request | Ctrl+C | In-flight request completes; clean exit |
| Existing config preserved | US-126-AC5 | Customized config present | `axon serve` | Config untouched; customized values in effect |

## Dependencies

- **Stories**: none
- **Feature Spec**: FEAT-028
- **Feature Requirements**: BIN-01, BIN-02, BIN-05
- **PRD Requirements**: FR-23, FR-24
- **External**: CONTRACT-008 (ports, paths, flags, config schema)

## Out of Scope

- Service installation (US-128); client mode (US-129); config precedence
  details (US-134); MCP mode (`axon mcp`, CONTRACT-003).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
