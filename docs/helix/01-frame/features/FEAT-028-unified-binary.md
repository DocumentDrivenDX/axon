---
dun:
  id: FEAT-028
  depends_on:
    - helix.prd
    - FEAT-005
    - FEAT-014
---
# Feature Specification: FEAT-028 - Unified Binary & Service Management

**Feature ID**: FEAT-028
**Status**: Draft
**Priority**: P0
**Owner**: Core Team
**Created**: 2026-04-11
**Updated**: 2026-04-11

## Overview

Merge the `axon` CLI and `axon-server` into a single `axon` binary that
serves as both a CLI tool and a server. Add XDG-compliant default paths,
a TOML config file, a `doctor` diagnostic command, an `install`
service-management command (systemd/launchd), and a client mode where CLI
commands talk to a running server over HTTP.

This is the critical path for adoption — other projects need to install
Axon with a single command, start a server, and interact with it from
another terminal.

## Problem Statement

Axon currently ships as two separate binaries (`axon` for embedded CLI,
`axon-server` for HTTP/gRPC) with no config file, no XDG-standard paths,
no service management, and no way for the CLI to talk to a running
server. This means:

- No `curl | sh` install path — users must build from source or manage
  two binaries
- The CLI operates in embedded mode only — it cannot inspect or manage a
  running server
- No standard storage locations — databases are created in the current
  directory
- No service management — users must write their own systemd/launchd
  units
- No single source of configuration — settings are scattered across CLI
  flags and environment variables

## Requirements

### Functional Requirements

#### Single Binary

- A single `axon` binary provides all functionality: CLI operations,
  server, MCP stdio, diagnostics, and service management.
- The `axon-server` crate becomes a library; its binary is removed.
- Docker images use `CMD ["axon", "serve"]`.

#### `axon serve`

- Starts the HTTP gateway in the foreground on port **4170** by default.
- gRPC is opt-in via `--grpc-port` (default 4171 when enabled).
- Ctrl+C triggers graceful shutdown.
- Default storage: SQLite at the XDG data directory.
- On first run, auto-creates a default `config.toml` with comments at
  the XDG config path if none exists.
- Supports all current server flags: `--storage`, `--sqlite-path`,
  `--postgres-dsn`, `--no-auth`, `--guest-role`, `--ui-dir`,
  `--control-plane-path`.

#### `axon mcp`

- Starts the MCP server over stdin/stdout (same as current
  `--mcp-stdio` mode).
- Separate subcommand, not a flag on `serve` — MCP is a distinct
  execution mode.

#### `axon doctor`

- Prints resolved configuration: config file path (and whether it
  exists), data directory path (and whether it exists), storage backend,
  default ports.
- Checks connectivity to the configured server URL — reports whether a
  server is reachable.
- Lists databases if server is reachable or embedded storage is
  accessible.
- Shows Axon version.

#### `axon server install`

- `axon server install` installs Axon as a user service:
  - Linux: writes `~/.config/systemd/user/axon.service`, runs
    `systemctl --user daemon-reload && enable axon`
  - macOS: writes `~/Library/LaunchAgents/com.axon.server.plist`, runs
    `launchctl load`
- `axon server install --global` installs as a system service (requires root):
  - Linux: creates `axon` system user, creates `/var/lib/axon/`, writes
    `/etc/systemd/system/axon.service`
  - macOS: creates `_axon` system user, writes
    `/Library/LaunchDaemons/com.axon.server.plist`
- Service units run `axon serve` using the installed binary, no-auth default,
  and XDG/global data paths for SQLite and control-plane storage.
- `axon server start|stop|restart|status` manage the installed service,
  delegating to `systemctl` (Linux) or `launchctl` (macOS).
- `axon server uninstall` removes the service unit and disables it.

#### Client Mode

- When a server is reachable at the configured URL
  (`http://localhost:4170` by default), CLI commands send HTTP requests
  to the server instead of opening SQLite directly.
- `--embedded` forces embedded SQLite mode regardless of server
  availability.
- `--server <url>` forces client mode against a specific URL.
- Default behavior: attempt HTTP connection to configured server URL
  with 200ms timeout; if unreachable, fall back to embedded mode.
- Output parity: JSON/table/YAML output formats work identically in
  both modes.

#### XDG-Compliant Default Paths

User install (Linux):
- Config: `$XDG_CONFIG_HOME/axon/config.toml` (default
  `~/.config/axon/config.toml`)
- Data: `$XDG_DATA_HOME/axon/` (default `~/.local/share/axon/`)
  - `axon.db` — master/control-plane database
  - `tenants/` — per-tenant SQLite databases

User install (macOS):
- Config: `~/Library/Application Support/axon/config.toml`
- Data: `~/Library/Application Support/axon/`

Global install:
- Config: `/etc/axon/config.toml`
- Data: `/var/lib/axon/`

#### TOML Config File

```toml
[server]
http_port = 4170
# grpc_port = 4171            # uncomment to enable gRPC listener

[storage]
backend = "sqlite"            # sqlite | postgres | memory
data_dir = ""                 # empty = XDG default
# postgres_host = "localhost"
# postgres_port = 5432
# postgres_superuser = ""
# postgres_superpass = ""

[auth]
mode = "no-auth"              # no-auth | guest | tailscale
guest_role = "admin"

[client]
server_url = "http://localhost:4170"
connect_timeout_ms = 200
```

Configuration hierarchy: compiled defaults < config file < environment
variables (`AXON_` prefix, e.g. `AXON_SERVER_HTTP_PORT`) < CLI flags.

#### Install Script

- A shell script downloadable via `curl -fsSL <url> | sh`.
- Detects OS (Linux/macOS) and architecture (x86_64/aarch64).
- Downloads the appropriate release binary from GitHub releases.
- Installs to `~/.local/bin/axon`.
- Creates default config and data directories.
- Warns if `~/.local/bin` is not in `$PATH`.
- macOS and Linux supported. Windows deferred.

### Non-Functional Requirements

- Binary size: acceptable increase from linking server code into CLI.
  Feature-gate `serve` capability behind a cargo feature (default on)
  for embedded-only builds.
- Config file auto-creation must not overwrite an existing config.
- Service units must work on systemd 240+ (Ubuntu 18.04+) and
  launchd (macOS 10.13+).
- Client mode auto-detection must complete within 200ms to avoid
  perceptible delay.

### Dependencies

- FEAT-005 (API Surface) — client mode uses the existing HTTP API.
- FEAT-014 (Multi-Tenancy) — per-tenant database paths use the
  namespace hierarchy.

## User Stories

### Story US-070: Start Axon from a single binary [FEAT-028]

**As a** developer adopting Axon
**I want** to run `axon serve` to start a server
**So that** I can begin using Axon without managing separate binaries

**Acceptance Criteria:**
- [ ] `axon serve` starts an HTTP gateway on port 4170
- [ ] GET `/healthz` returns 200
- [ ] Ctrl+C gracefully shuts down the server
- [ ] Default storage is SQLite at XDG data directory
- [ ] First run creates `config.toml` with defaults and comments

### Story US-071: Diagnose Axon installation [FEAT-028]

**As a** developer troubleshooting Axon
**I want** to run `axon doctor` to see my configuration
**So that** I can verify paths, connectivity, and storage

**Acceptance Criteria:**
- [ ] Shows config file path and whether it exists
- [ ] Shows data directory and storage backend
- [ ] Shows server connectivity status (reachable or not)
- [ ] Shows Axon version

### Story US-072: Install Axon as a system service [FEAT-028]

**As a** developer running Axon persistently
**I want** to run `axon server install` to set up a user service
**So that** Axon starts automatically and survives reboots

**Acceptance Criteria:**
- [ ] `axon server install` creates and enables a user-level service
- [ ] `axon server start` starts the service
- [ ] `axon server status` reports whether the service is running
- [ ] `axon server install --global` creates a system service with dedicated
      user and `/var/lib/axon` storage
- [ ] `axon server uninstall` removes the service

### Story US-073: Use CLI against a running server [FEAT-028]

**As a** developer with `axon serve` running in one terminal
**I want** to run `axon entity list` in another terminal
**So that** CLI commands go through the server instead of opening
  SQLite directly

**Acceptance Criteria:**
- [ ] CLI auto-detects running server and uses HTTP client mode
- [ ] `--embedded` forces direct SQLite access
- [ ] `--server <url>` forces client mode against a specific URL
- [ ] Entity CRUD, collection management, and audit queries work in
      both modes with identical output

### Story US-074: Install Axon with a single command [FEAT-028]

**As a** developer on Linux or macOS
**I want** to run `curl -fsSL <url> | sh` to install Axon
**So that** I can start using it immediately without building from source

**Acceptance Criteria:**
- [ ] Install script detects OS and architecture
- [ ] Binary is placed at `~/.local/bin/axon`
- [ ] Script warns if `~/.local/bin` is not in PATH
- [ ] `axon --version` works after installation

### Story US-075: Configure Axon persistently [FEAT-028]

**As a** developer customizing Axon
**I want** a TOML config file at a standard location
**So that** I don't have to pass flags every time

**Acceptance Criteria:**
- [ ] Config file is read from XDG config directory
- [ ] Environment variables override config file values
- [ ] CLI flags override environment variables
- [ ] `axon config show` prints resolved configuration
- [ ] `axon config path` prints the config file location

## Edge Cases and Error Handling

- **Config file parse error**: `axon serve` prints a clear error message
  with the line number and exits non-zero. Does not fall back to
  defaults silently.
- **Port already in use**: `axon serve` prints "port 4170 already in
  use" and exits non-zero. `axon doctor` reports a running server if
  the port responds.
- **No write access to data dir**: `axon serve` prints the path and
  permission error, suggests `mkdir -p` or `--global` install.
- **Service already installed**: `axon server install` prints "service already
  installed" and exits cleanly. Use `axon server uninstall` first to
  reinstall.
- **Client mode timeout**: 200ms connect timeout. If the server is
  unreachable, CLI falls back to embedded mode with a one-line notice:
  "server unreachable, using embedded mode".
- **Mixed client/embedded state**: If the server and embedded mode have
  different databases, client mode is authoritative (the server is the
  source of truth when running).

## Out of Scope

- Windows support (deferred)
- Automatic updates or self-update mechanism
- GUI installer
- Remote server management (managing Axon on a different machine)
- Per-tenant gRPC routing (HTTP only in V1; gRPC uses default tenant)

## Traceability

### Related Artifacts
- **Parent PRD Section**: Requirements Overview > P0 #12 (CLI),
  P1 #5 (Server mode)
- **User Stories**: US-070, US-071, US-072, US-073, US-074, US-075
- **Architecture**: Plan file `ancient-popping-bunny.md`
- **Implementation**: `crates/axon-config/`, `crates/axon-cli/`,
  `crates/axon-server/` (library)

### Feature Dependencies
- **Depends On**: FEAT-005, FEAT-014
- **Depended By**: FEAT-024 (Application Substrate — `axon init`
  assumes `axon serve` exists)
