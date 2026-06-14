---
ddx:
  id: FEAT-028
  depends_on:
    - helix.prd
  review:
    self_hash: eaf50210678ba364441138764677456c6bf02edcc09282dfaa7d1b312f3fea20
    deps:
      helix.prd: dff98156a6cc934f406611b78b513892d85cee1bd7b4c011f045146fcdfd23e1
    reviewed_at: "2026-06-14T04:39:42Z"
---
# Feature Specification: FEAT-028 — Unified Binary & Service Management

**Feature ID**: FEAT-028
**Status**: draft
**Priority**: P1
**Owner**: Core Team
**Covered PRD Subsystem(s)**: API and Deployment Surfaces
**Covered PRD Requirements**: FR-23, FR-24
**Cross-Subsystem Rationale**: None — single subsystem.
**FR Prefix**: BIN

## Overview

One `axon` binary serves as both a CLI tool and a server: it runs the HTTP
gateway, the MCP stdio server, diagnostics, and service management, and
its CLI commands can operate against a running server or against embedded
storage with identical behavior (PRD FR-23). Operators get standard config
locations, a single config file, a diagnostic command, and one-command
service installation (PRD FR-24).

This is the critical adoption path: a developer installs Axon with a
single command, starts a server, and interacts with it from another
terminal. The normative command tree, flags, TOML configuration schema,
environment-variable precedence, default ports and paths, service-unit
locations, and client-mode connection rules are defined in
[CONTRACT-008 — CLI and config](../../02-design/contracts/CONTRACT-008-cli-and-config.md).

## Ideal Future State

A developer runs one install command, then `axon serve`, and has a working
server with sane default storage at standard paths and a commented config
file they can grow into. From another terminal, the same binary's CLI
commands transparently talk to the running server — or to embedded storage
when no server is up — with identical output. `axon doctor` answers "what
is my installation actually doing" in one screen. Installing Axon as a
service is one command and is safe by default: a service-installed Axon
requires authentication unless the operator explicitly and knowingly opts
out.

## Problem Statement

- **Current situation**: Axon's heritage is two separate binaries (CLI and
  server) with no config file, no standard storage paths, no service
  management, and no way for the CLI to talk to a running server.
- **Pain points**: No `curl | sh` install path; the CLI cannot inspect or
  manage a running server; databases land in the current directory;
  users hand-write systemd/launchd units; settings are scattered across
  flags and environment variables.
- **Desired outcome**: Single binary, single config story, one-command
  install and service setup, and CLI/server interaction out of the box.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Single binary and execution modes | "Run the server, the MCP server, or CLI commands from one tool" | One binary with `serve`, `mcp`, and CLI subcommands |
| Configuration and diagnostics | "Where is my config, data, and what is Axon actually using?" | Layered config (file/env/flags), standard paths, `doctor` |
| Service management | "Make Axon start at boot, safely" | User/system service install, lifecycle commands, secure defaults |
| Client mode | "Use the CLI against my running server" | Auto-detection, explicit overrides, output parity |
| Installation | "Get the binary onto my machine in one command" | Install script with OS/arch detection |

## Requirements

### Functional Requirements by Area

#### Single Binary and Execution Modes

- **BIN-01**. A single `axon` binary MUST provide all functionality: CLI
  operations, HTTP server, MCP stdio server, diagnostics, and service
  management. The command tree is defined in CONTRACT-008. No separate
  server binary ships.
- **BIN-02**. `axon serve` MUST start the HTTP gateway in the foreground
  on the default port, shut down gracefully on interrupt, and use embedded
  SQLite storage at the standard data directory by default. Default ports,
  storage flags, and paths are defined in CONTRACT-008.
- **BIN-03**. The MCP stdio server MUST be a distinct subcommand
  (`axon mcp`), not a flag on `serve` — it is a separate execution mode
  (surface per CONTRACT-003/CONTRACT-008).

#### Configuration and Diagnostics

- **BIN-04**. Configuration MUST be layered with the precedence defined in
  CONTRACT-008: compiled defaults, then config file, then environment
  variables, then CLI flags. The TOML schema, env-var naming, and standard
  (XDG-style) config/data paths for user and global installs are defined
  in CONTRACT-008.
- **BIN-05**. On first run, `axon serve` MUST auto-create a commented
  default config file at the standard location if none exists, and MUST
  never overwrite an existing config file.
- **BIN-06**. `axon doctor` MUST print the resolved configuration (config
  file path and existence, data directory and existence, storage backend,
  ports, auth mode), report whether a server is reachable at the
  configured URL, list databases when server or embedded storage is
  accessible, and show the Axon version.
- **BIN-07**. The CLI MUST provide commands to print the resolved
  configuration and the config file location (per CONTRACT-008).

#### Service Management

- **BIN-08**. `axon server install` MUST install Axon as a user service
  (systemd user unit on Linux, LaunchAgent on macOS), and
  `--global` MUST install a system service with a dedicated system user
  and system data directory (requires root). Unit file locations, paths,
  and generated unit contents are defined in CONTRACT-008.
- **BIN-09**. `axon server start|stop|restart|status` MUST manage the
  installed service by delegating to the platform service manager, and
  `axon server uninstall` MUST disable and remove the unit.
- **BIN-10**. **Secure-by-default service installs**: service
  installations MUST default to an authenticated mode. Installing or
  serving with authentication disabled (`no-auth`) MUST require an
  explicit opt-in flag and MUST print a prominent warning naming the
  exposure; the resolved auth mode is visible via `axon doctor` (BIN-06).
  Flag names per CONTRACT-008.

#### Client Mode

- **BIN-11**. When a server is reachable at the configured URL, CLI
  commands MUST send requests to the server instead of opening embedded
  storage directly; when no server is reachable within the detection
  timeout, the CLI MUST fall back to embedded mode with a one-line notice.
  Detection timeout, URL configuration, and the `--embedded` /
  `--server <url>` overrides are defined in CONTRACT-008.
- **BIN-12**. Entity CRUD, collection management, and audit queries MUST
  work in both client and embedded modes with identical output across all
  output formats (JSON/table/YAML).

#### Installation

- **BIN-13**. A shell install script (`curl | sh`) MUST detect OS
  (Linux/macOS) and architecture, download the matching release binary,
  install it to the user-local bin directory, create default config and
  data directories, and warn when the install directory is not on `PATH`.

### Non-Functional Requirements

- **Binary size**: server capability is feature-gated behind a cargo
  feature (default on) so embedded-only builds remain possible.
- **Safety**: config auto-creation never overwrites an existing file
  (BIN-05).
- **Compatibility**: service units work on systemd 240+ (Ubuntu 18.04+)
  and launchd (macOS 10.13+).
- **Responsiveness**: client-mode auto-detection completes within the
  CONTRACT-008 detection timeout (200 ms) so CLI startup has no
  perceptible delay.

## User Stories

- [US-126 — Start Axon from a single binary](../user-stories/US-126-start-axon-from-a-single-binary.md)
- [US-127 — Diagnose Axon installation](../user-stories/US-127-diagnose-axon-installation.md)
- [US-128 — Install Axon as a system service](../user-stories/US-128-install-axon-as-a-system-service.md)
- [US-129 — Use CLI against a running server](../user-stories/US-129-use-cli-against-a-running-server.md)
- [US-131 — Install Axon with a single command](../user-stories/US-131-install-axon-with-a-single-command.md)
- [US-134 — Configure Axon persistently](../user-stories/US-134-configure-axon-persistently.md)

## Edge Cases and Error Handling

- **Config file parse error**: `axon serve` prints a clear error with the
  line number and exits non-zero; it never silently falls back to
  defaults.
- **Port already in use**: `axon serve` reports the conflicting port and
  exits non-zero; `axon doctor` reports a running server if the port
  responds.
- **No write access to the data directory**: `axon serve` prints the path
  and permission error and suggests remediation (create the directory or
  use a global install).
- **Service already installed**: `axon server install` reports it and
  exits cleanly; reinstalling requires `axon server uninstall` first.
- **Client-mode timeout**: if the server is unreachable within the
  detection timeout, the CLI falls back to embedded mode with a one-line
  notice.
- **Mixed client/embedded state**: if the server and embedded mode see
  different databases, client mode is authoritative — the running server
  is the source of truth.
- **No-auth opt-in on a service install**: the warning is printed at
  install time and the auth mode remains visible in `doctor` output; the
  default path never produces an unauthenticated service (BIN-10).

## Success Metrics

- Install-to-first-successful-`axon serve` in under 5 minutes on a clean
  Linux or macOS machine.
- 100% of CLI data commands produce identical output in client and
  embedded modes in the parity fixture suite.
- Zero service installs end up unauthenticated without the explicit
  opt-in flag (verified by install-path tests).
- `axon doctor` diagnoses the four common misconfigurations (missing
  config, unreachable server, wrong data dir, port conflict) without
  reading source code or docs.

## Constraints and Assumptions

- The exact command tree, flag names, TOML keys, env-var names, ports,
  paths, and service-unit contents are owned by CONTRACT-008; this spec
  constrains behavior only.
- Authenticated-by-default service installs assume an available local
  authentication mode (per ADR-005/ADR-018 transport authentication);
  developer-loop `axon serve` in the foreground retains its current
  default for fast local iteration, with the same explicit-flag rule for
  exposed deployments.
- Linux and macOS are the supported platforms for V1.

## Dependencies

- **Other features**:
  - FEAT-005 (API Surface) — client mode uses the existing HTTP API.
  - FEAT-014 (Multi-Tenancy) — per-tenant database paths use the
    namespace hierarchy.
- **External services**: GitHub releases (install script download
  source); systemd/launchd; exact surface in CONTRACT-008.
- **PRD requirements**: FR-23 (P1), FR-24 (P1).

## Out of Scope

- **Windows support** (deferred).
- **Automatic updates or self-update mechanism.**
- **GUI installer.**
- **Remote server management** (managing Axon on a different machine).
- **Per-tenant gRPC routing** (HTTP only in V1; gRPC uses the default
  tenant).
