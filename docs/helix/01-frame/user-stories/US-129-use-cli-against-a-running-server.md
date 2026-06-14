---
ddx:
  id: US-129
  review:
    self_hash: b39793e91c24e3a1b5799b61332c73e5a6710eece5d22e3a0195e77952403382
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# US-129: Use CLI against a running server

**Feature**: FEAT-028 — Unified Binary & Service Management
**Feature Requirements**: BIN-11, BIN-12
**PRD Requirements**: FR-23
**Priority**: P1
**Status**: Draft

## Story

**As a** developer (Ava) with `axon serve` running in one terminal
**I want** to run `axon entity list` in another terminal
**So that** CLI commands go through the server instead of opening SQLite
directly

## Context

Renumbered from US-073 (collision with FEAT-009 "Discover Links via
MCP"). Embedded and server modes must behave identically (PRD FR-23);
the CLI is where that parity is felt daily. This story exercises BIN-11
and BIN-12: transparent client-mode auto-detection with explicit
overrides, and identical output across modes. Detection timeout, URL
configuration, and override flags are normative in CONTRACT-008.

## Walkthrough

1. With a server running, Ava runs `axon entity list` in a second
   terminal.
2. The CLI detects the server within the CONTRACT-008 timeout and sends
   the request over HTTP instead of opening SQLite.
3. She forces embedded mode with the `--embedded` override and gets the
   same command shape against local storage.
4. She targets a different instance with the `--server <url>` override.
5. With the server stopped, the same command falls back to embedded mode
   with a one-line notice — output format unchanged.

## Acceptance Criteria

- [ ] **US-129-AC1** — Given a reachable server at the configured URL,
      when a CLI data command runs, then it is executed via HTTP client
      mode (no direct storage access).
- [ ] **US-129-AC2** — Given the `--embedded` override, when a CLI command
      runs, then it accesses embedded storage directly regardless of
      server availability.
- [ ] **US-129-AC3** — Given the `--server <url>` override, when a CLI
      command runs, then it executes in client mode against that URL.
- [ ] **US-129-AC4** — Given entity CRUD, collection management, and audit
      queries, when each runs in client mode and in embedded mode, then
      the output is identical across all output formats.
- [ ] **US-129-AC5** — Given no reachable server, when a CLI command runs
      without overrides, then it falls back to embedded mode within the
      detection timeout and prints a one-line notice.

## Edge Cases

- **Server and embedded storage hold different databases**: client mode
  is authoritative when a server is running — the server is the source of
  truth.
- **Slow/hung server**: detection respects the CONTRACT-008 timeout so
  CLI startup has no perceptible delay.
- **Auth-protected server**: client mode presents the configured
  credentials; an auth failure is a clear error, not a silent embedded
  fallback.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Auto-detect | US-129-AC1 | Server running | `axon entity list` | Request served via HTTP; result matches server data |
| Forced embedded | US-129-AC2 | Server running | Same command with `--embedded` | Local storage read; no HTTP request |
| Output parity | US-129-AC4 | Same dataset in both modes | Run CRUD + audit commands in both | Byte-identical output per format |
| Fallback | US-129-AC5 | No server | `axon entity list` | One-line notice; embedded result within timeout |

## Dependencies

- **Stories**: US-126 (a running server)
- **Feature Spec**: FEAT-028
- **Feature Requirements**: BIN-11, BIN-12
- **PRD Requirements**: FR-23
- **External**: CONTRACT-008 (detection timeout, overrides, URL config);
  CONTRACT-001 (HTTP routes used in client mode); FEAT-005

## Out of Scope

- Remote server management; per-tenant gRPC routing (FEAT-028 Out of
  Scope); server startup (US-126).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
