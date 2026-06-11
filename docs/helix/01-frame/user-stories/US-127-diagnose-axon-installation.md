---
ddx:
  id: US-127
---

# US-127: Diagnose Axon installation

**Feature**: FEAT-028 — Unified Binary & Service Management
**Feature Requirements**: BIN-06
**PRD Requirements**: FR-24
**Priority**: P1
**Status**: Draft

## Story

**As a** developer (Ava) troubleshooting Axon
**I want** to run `axon doctor` to see my configuration
**So that** I can verify paths, connectivity, and storage

## Context

Renumbered from US-071 (collision with FEAT-009 "List Entity
Neighbors"). With layered configuration (defaults, file, env, flags), the
first troubleshooting question is "what is Axon actually using?" This
story exercises BIN-06: one command that prints the resolved
configuration, environment state, connectivity, and version.

## Walkthrough

1. Ava's CLI commands behave unexpectedly; she runs `axon doctor`.
2. Doctor prints the config file path and whether it exists, the data
   directory and whether it exists, the storage backend, the configured
   ports, and the resolved auth mode.
3. Doctor reports whether a server is reachable at the configured URL.
4. With the server reachable, doctor lists the visible databases.
5. Doctor prints the Axon version; Ava spots the stale config path and
   fixes it.

## Acceptance Criteria

- [ ] **US-127-AC1** — Given any installation state, when `axon doctor`
      runs, then it shows the config file path and whether it exists.
- [ ] **US-127-AC2** — Given any installation state, when doctor runs,
      then it shows the data directory, its existence, and the storage
      backend.
- [ ] **US-127-AC3** — Given a configured server URL, when doctor runs,
      then it reports whether a server is reachable there.
- [ ] **US-127-AC4** — Given a reachable server or accessible embedded
      storage, when doctor runs, then it lists the databases.
- [ ] **US-127-AC5** — Given any installation state, when doctor runs,
      then it shows the Axon version.
- [ ] **US-127-AC6** — Given a resolved configuration, when doctor runs,
      then the effective auth mode is visible (supports the BIN-10
      secure-default check).

## Edge Cases

- **No config file**: doctor still runs, showing compiled defaults and
  flagging the missing file.
- **Server unreachable**: reported as such (with the configured URL), not
  an error exit.
- **Both server and embedded storage available**: doctor reports both and
  indicates which mode CLI commands would use.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Full report | US-127-AC1 | Config exists, server running | `axon doctor` | Path + exists, data dir, backend, ports, auth mode |
| Unreachable server | US-127-AC3 | No server running | `axon doctor` | "Not reachable" at configured URL; embedded fallback noted |
| Database list | US-127-AC4 | Server with 2 databases | `axon doctor` | Both databases listed |
| Version | US-127-AC5 | Any | `axon doctor` | Version string shown |

## Dependencies

- **Stories**: US-126 (a server to diagnose, for connectivity checks)
- **Feature Spec**: FEAT-028
- **Feature Requirements**: BIN-06
- **PRD Requirements**: FR-24
- **External**: CONTRACT-008 (resolved-config fields, URL configuration)

## Out of Scope

- Fixing problems automatically; service status (US-128); full config
  dump commands (US-134).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
