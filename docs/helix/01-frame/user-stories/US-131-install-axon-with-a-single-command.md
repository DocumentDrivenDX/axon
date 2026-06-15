---
ddx:
  id: US-131
  review:
    self_hash: 9a96af199432bc16b92f7a156eb957a9831890ddb1606896e5d3897466ca07ae
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# US-131: Install Axon with a single command

**Feature**: FEAT-028 — Unified Binary & Service Management
**Feature Requirements**: BIN-13
**PRD Requirements**: FR-24 (adoption path for operator tooling)
**Priority**: P1
**Status**: Draft

## Story

**As a** developer (Ava) on Linux or macOS
**I want** to run a single `curl | sh` command to install Axon
**So that** I can start using it immediately without building from source

## Context

Renumbered from US-074 (collision with FEAT-009 "Pattern query for
ready/blocked queue"). Without a one-command install there is no
`curl | sh` adoption path — users must build from source. This story
exercises BIN-13: an install script that detects platform, fetches the
right release binary, installs it to the user-local bin directory, and
prepares default directories.

## Walkthrough

1. Ava runs the documented `curl -fsSL <url> | sh` command.
2. The script detects her OS (Linux/macOS) and architecture
   (x86_64/aarch64).
3. It downloads the matching release binary and installs it to the
   user-local bin directory, creating default config and data
   directories.
4. The script warns that the install directory is not on her `PATH`; she
   adds it.
5. `axon --version` prints the installed version.

## Acceptance Criteria

- [ ] **US-131-AC1** — Given a supported OS and architecture, when the
      install script runs, then it detects both and selects the matching
      release binary.
- [ ] **US-131-AC2** — Given a successful download, when installation
      completes, then the binary is placed in the user-local bin directory
      and default config/data directories exist.
- [ ] **US-131-AC3** — Given the install directory is not on `PATH`, when
      installation completes, then the script prints a warning with the
      needed `PATH` addition.
- [ ] **US-131-AC4** — Given a completed installation, when
      `axon --version` runs, then it prints the installed version.

## Edge Cases

- **Unsupported platform (e.g., Windows)**: the script exits with a clear
  unsupported-platform message (Windows is deferred).
- **Download failure**: non-zero exit with the failed URL; no partial
  binary left in place.
- **Existing older installation**: the script replaces the binary and
  preserves existing config and data.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Clean install | US-131-AC1 | Fresh Linux x86_64 host | Run install script | Correct binary selected and installed |
| Locations | US-131-AC2 | Install completed | Inspect filesystem | Binary in user-local bin; config/data dirs created |
| PATH warning | US-131-AC3 | `~/.local/bin` not on PATH | Run install script | Warning with remediation printed |
| Version check | US-131-AC4 | Install completed, PATH fixed | `axon --version` | Installed version printed |

## Dependencies

- **Stories**: none
- **Feature Spec**: FEAT-028
- **Feature Requirements**: BIN-13
- **PRD Requirements**: FR-24
- **External**: GitHub releases (binary distribution); CONTRACT-008
  (standard directories)

## Out of Scope

- Automatic updates / self-update; GUI installer; Windows support;
  package-manager distributions (brew/apt) unless added later.

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
