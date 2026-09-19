---
ddx:
  id: CONTRACT-008
  depends_on:
    - FEAT-001
    - FEAT-005
    - FEAT-017
    - FEAT-023
    - FEAT-026
    - FEAT-028
  review:
    self_hash: 9dbb01dffcecc2282b2dc8e819f7e182e1204ba6a8d40c0d0b02b6b9c767e613
    deps:
      FEAT-001: fef81eac3824d481ea889c8402ec5f2d7e6ecfa7f396186f18fa49ed8319a1cf
      FEAT-005: 1fab4e58214106451af84deee1a1bfb5c2b520333e6be2a7cd723153730c829c
      FEAT-017: 2651b9abea59b48d54097bc01b2643320f9b6cc04b7101f2ebb964a89bca1ff9
      FEAT-023: 24416c13b9a48e864ae43e3967c63d2711763c745905850dbb4f03768ffc7949
      FEAT-026: 8751e34ac2140fb80077b881290769d82b1d39e7cb1fbaa60404bc82eae1b07b
      FEAT-028: eaf50210678ba364441138764677456c6bf02edcc09282dfaa7d1b312f3fea20
    reviewed_at: "2026-07-11T04:22:34Z"
---

# Contract

**Contract ID**: CONTRACT-008
**Type**: CLI
**Version**: 0.2.0
**Status**: draft
**Related**: FEAT-001, FEAT-005, FEAT-017, FEAT-023, FEAT-026, FEAT-028, CONTRACT-001 (HTTP routes used in client mode)

## Purpose

Defines the normative `axon` CLI command tree, global flags and output
formats, the TOML configuration schema, environment-variable and flag
precedence, default ports and paths, and client-mode connection rules.

## Scope and Boundaries

- In scope: command names and arguments, flags, output formats, config
  file keys, env-var naming, precedence, mode selection, service
  management commands.
- Out of scope: HTTP wire shapes the CLI calls (CONTRACT-001/002), MCP
  protocol (CONTRACT-003), per-command human-readable output text.
- Owning system: `axon-cli` (single `axon` binary; `axon-server` is a
  library, its standalone binary is removed).

## Normative Surface

### Command tree

| Command | Description |
|---------|-------------|
| `axon` (no args) | Show help |
| `axon serve` | Start HTTP gateway in the foreground (port 4170); Ctrl+C graceful shutdown; auto-creates commented `config.toml` on first run if none exists |
| `axon mcp` | MCP server over stdin/stdout (distinct execution mode, not a `serve` flag) |
| `axon doctor` | Print resolved config (config path + exists?, data dir + exists?, storage backend, default ports, **resolved auth mode** — a `no-auth` mode is surfaced prominently), server reachability, database list, version |
| `axon server install [--global]` | Install user (systemd user unit / LaunchAgent) or system service (root: system user + unit); installs default to **authenticated mode** — installing with auth disabled requires the explicit `--no-auth` opt-in flag and prints a prominent warning naming the exposure (FEAT-028 BIN-10) |
| `axon server uninstall` | Remove and disable the service unit |
| `axon server start\|stop\|restart\|status` | Manage installed service via `systemctl`/`launchctl` |
| `axon config` | Connection settings and defaults |
| `axon collection create <name> --schema <path>` | Create collection (schema validated first) |
| `axon collection list` | Name, schema version, entity count |
| `axon collection describe <name>` | Full metadata including schema |
| `axon collection drop <name>` | Drop (with confirmation) |
| `axon collection template put <collection> --template <body>` | Save/update markdown template |
| `axon collection template get <collection>` | Retrieve template |
| `axon collection template delete <collection>` | Remove template |
| `axon entity create\|get\|update\|delete\|list\|query <collection> ...` | Entity operations; subcommand is `entity` — no `doc` alias |
| `axon entity get <collection> <id> --render markdown` | Markdown-rendered single entity |
| `axon entity query <collection> --filter "<expr>"` | Filtered query (e.g. `status=pending`) |
| `axon schema show <collection>` | Show active schema |
| `axon schema validate` | Validate a schema document |
| `axon schema diff <collection> <v1> <v2>` | Field-level diff (added/removed/modified fields, type/constraint/enum changes); works across non-adjacent versions |
| `axon schema revalidate <collection>` | Scan entities; report which are invalid against the active schema |
| `axon policy explain\|test` | Policy explanation and testing |
| `axon mutation preview\|commit\|approve\|reject` | Governed-write intent workflow |
| `axon audit list [--collection <name>] [--last N]` | Recent audit entries |
| `axon audit show <id>` | Single audit record |
| `axon audit diff` / `axon audit blame` | Changed fields, actor/tool origin, policy decision, approval decision, transaction ID, audit IDs |
| `axon rollback dry-run` | Compensating operations + conflicts, no state mutation |
| `axon rollback commit` | Execute rollback (governed write) |

Schema-evolution flags (FEAT-017): schema application supports `--dry-run`
(report compatibility classification without applying) and `--force`
(apply a breaking change; audit entry records classification + diff).

### Global flags and output

| Flag | Rule |
|------|------|
| `--output json\|yaml\|table` | `table` is default (human-readable); `--output json` MUST be machine-parseable; identical in embedded and client modes |
| `--embedded` | Force embedded SQLite mode regardless of server availability |
| `--server <url>` | Force client mode against a specific URL |

`serve` flags: `--storage`, `--sqlite-path`, `--postgres-dsn`, `--no-auth`
(explicit opt-in to unauthenticated mode; prints a prominent warning naming
the exposure — see BIN-10 rules under the config table), `--guest-role`,
`--ui-dir`, `--control-plane-path`, `--grpc-port` (gRPC is opt-in; default
4171 when enabled).

### Client-mode connection rules (FEAT-005 / FEAT-028)

1. Default: attempt an HTTP connection to the configured server URL
   (`http://localhost:4170` default) with a **200ms** timeout.
2. Reachable → client mode: commands issue HTTP requests against the
   CONTRACT-001 routes (no new protocol).
3. Unreachable within 200ms → fall back to embedded SQLite mode.
4. `--embedded` and `--server <url>` override auto-detection.
5. When a server is running, client mode is the expected path; embedded
   mode is for offline/development use. Output parity holds in both modes.

### TOML configuration schema

Default config locations (first existing wins per install type):

| Install | Config | Data |
|---------|--------|------|
| Linux user | `$XDG_CONFIG_HOME/axon/config.toml` (default `~/.config/axon/config.toml`) | `$XDG_DATA_HOME/axon/` (default `~/.local/share/axon/`) — `axon.db` control-plane DB + `tenants/` per-tenant SQLite |
| macOS user | `~/Library/Application Support/axon/config.toml` | `~/Library/Application Support/axon/` |
| Global | `/etc/axon/config.toml` | `/var/lib/axon/` |

| Key | Type | Default | Rules |
|-----|------|---------|-------|
| `server.http_port` | integer | `4170` | HTTP gateway port |
| `server.grpc_port` | integer | `4171` (commented out) | gRPC listener enabled only when set |
| `storage.backend` | string | `"sqlite"` | enum: `sqlite` \| `postgres` \| `memory` |
| `storage.data_dir` | string | `""` | empty = XDG default |
| `storage.postgres_host` | string | `"localhost"` | optional |
| `storage.postgres_port` | integer | `5432` | optional |
| `storage.postgres_superuser` / `postgres_superpass` | string | `""` | optional |
| `auth.mode` | string | `"tailscale"` | enum: `no-auth` \| `guest` \| `tailscale`; authenticated mode is the default (FEAT-028 BIN-10). `mode = "no-auth"` takes effect only with the explicit `--no-auth` opt-in flag at install/serve time and MUST print a prominent warning naming the exposure; the default path never produces an unauthenticated service. `axon doctor` surfaces the resolved auth mode |
| `auth.guest_role` | string | `"admin"` | role assumed by guest callers |
| `client.server_url` | string | `"http://localhost:4170"` | client-mode probe target |
| `client.connect_timeout_ms` | integer | `200` | client-mode probe timeout |

Config auto-creation MUST NOT overwrite an existing config file.

### Configuration precedence

```
compiled defaults  <  config file  <  environment variables  <  CLI flags
```

Environment variables use the `AXON_` prefix with the TOML path upper
snake-cased: `AXON_SERVER_HTTP_PORT`, `AXON_STORAGE_BACKEND`,
`AXON_AUTH_MODE`, `AXON_CLIENT_SERVER_URL`, etc. GraphQL limit overrides
`AXON_GRAPHQL_MAX_DEPTH` / `AXON_GRAPHQL_MAX_COMPLEXITY` follow the same
prefix convention (CONTRACT-002).

## Precedence and Compatibility

- **Change history**: 0.2.0 (2026-06-10) — auth default flipped from
  `no-auth` to authenticated (`tailscale`); `no-auth` is now an explicit
  opt-in with a prominent warning, surfaced by `doctor`, per FEAT-028
  BIN-10. 0.1.0 — initial draft (service installs defaulted to no-auth).
- The four-level precedence chain above is normative and total; a value
  set at a higher level always wins.
- The `entity` subcommand name is fixed; no `doc` alias will be added
  (FEAT-005 naming decision).
- Every API operation has a CLI equivalent; new API surface implies new
  CLI surface in the same release.
- Service units MUST work on systemd 240+ and launchd (macOS 10.13+).
- `--output json` shapes are stable for scripting; additive fields only.

## Error Semantics

| Condition | Error / Outcome | Retry | Recovery Expectation |
|-----------|-----------------|-------|----------------------|
| Server unreachable, no fallback possible | connection error with retry guidance, non-zero exit | Yes | Check `axon doctor` |
| Server unreachable in auto mode | silent fallback to embedded within 200ms | N/A | Use `--server` to force client mode |
| Invalid command/flags | usage error + help, non-zero exit | No | Correct invocation |
| API error in client mode | the CONTRACT-001 `{code, detail}` envelope is surfaced; `--output json` preserves it verbatim | Per code | Switch on `code` |
| Breaking schema apply without `--force` | rejected with compatibility report | No | Re-run with `--force` or amend |
| `rollback dry-run` | never mutates state; exit 0 with plan | N/A | Review then `rollback commit` |
| Existing config on first run | not overwritten | N/A | — |

## Examples

```bash
axon collection create tasks --schema ./tasks.esf.yaml
axon entity query tasks --filter "status=pending" --output json
axon audit list --collection tasks --last 10
axon schema diff tasks 1 3
axon rollback dry-run --transaction tx-018f4f9c
AXON_SERVER_HTTP_PORT=8080 axon serve --no-auth
```

## Non-Normative Notes

- The install script (`curl -fsSL <url> | sh`) detects OS/arch, installs
  to `~/.local/bin/axon`, creates default config/data dirs, and warns when
  `~/.local/bin` is not on `$PATH`. Windows is deferred.

## Validation Checklist

- [x] Normative fields and rules are explicit.
- [x] Compatibility and precedence rules are explicit.
- [x] Error handling is explicit.
- [x] At least one executable test can be derived from this contract.
- [x] Non-normative notes cannot be mistaken for contract requirements.
