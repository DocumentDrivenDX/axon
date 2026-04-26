---
title: CLI Reference
weight: 3
prev: /docs/concepts
next: /docs/demos
---

The `axon` binary is the unified CLI for the Axon entity store. It covers server management, collection and entity CRUD, schema evolution, graph traversal, and audit queries.

## Global flags

| Flag | Default | Description |
|------|---------|-------------|
| `--server <url>` | `http://localhost:4170` | Force HTTP client mode against a running server |
| `--db <path>` | — | Force embedded SQLite mode (bypass server detection) |
| `--output <format>` | `json` | Output format: `json`, `yaml`, `table` |
| `--config <path>` | XDG default | Path to `config.toml` |

## Mode detection

When neither `--server` nor `--db` is set, Axon auto-detects the mode:

1. Try HTTP to the configured `server_url` with a 200 ms connect timeout
2. If reachable → **client mode** (all commands send HTTP requests)
3. If unreachable → **embedded mode** (commands open SQLite directly)

Use `--db` for scripts and tests to guarantee embedded mode without network probing.

## Command groups

{{< cards >}}
  {{< card title="axon serve" subtitle="Start the HTTP server. Covers storage backends, port, auth, and gRPC options." >}}
  {{< card title="axon collections" subtitle="Create, list, drop, and manage collection templates." >}}
  {{< card title="axon entities" subtitle="Create, get, list, update, delete, and query entities." >}}
  {{< card title="axon schema" subtitle="Set and show collection schemas. Control schema evolution." >}}
  {{< card title="axon links" subtitle="Set links between entities and list outbound edges." >}}
  {{< card title="axon graph" subtitle="Traverse entity graphs to arbitrary depth." >}}
  {{< card title="axon audit" subtitle="Query the immutable audit log." >}}
  {{< card title="axon doctor" subtitle="Diagnose installation, config, and server connectivity." >}}
  {{< card title="axon server" subtitle="Install, start, stop, and manage the system service." >}}
{{< /cards >}}

---

## axon serve

Start the HTTP (and optionally gRPC) server.

```bash
axon serve [flags]
```

**Storage**

| Flag | Default | Description |
|------|---------|-------------|
| `--http-port <n>` | `4170` | HTTP listener port |
| `--grpc-port <n>` | disabled | Enable gRPC on this port |
| `--storage <backend>` | `sqlite` | Storage backend: `sqlite`, `postgres`, `memory` |
| `--sqlite-path <path>` | `axon-server.db` | SQLite database file path |
| `--postgres-dsn <dsn>` | — | PostgreSQL DSN (required when `--storage=postgres`) |
| `--control-plane-path <path>` | `axon-control-plane.db` | Control-plane SQLite path |
| `--ui-dir <path>` | — | Serve admin UI static files from this directory |

**Authentication** (see [Authentication & Authorization](/docs/concepts/authentication) for details)

| Flag | Env var | Default | Description |
|------|---------|---------|-------------|
| `--no-auth` | `AXON_NO_AUTH` | `false` | Disable auth — all requests are anonymous admin (dev only) |
| `--tailscale-socket <path>` | `AXON_TAILSCALE_SOCKET` | `/run/tailscale/tailscaled.sock` | Path to Tailscale daemon socket |
| `--tailscale-default-role <role>` | `AXON_TAILSCALE_DEFAULT_ROLE` | `read` | Role for nodes without a recognized ACL tag |
| `--guest-role <role>` | `AXON_GUEST_ROLE` | *(disabled)* | Enable guest mode with this role (mutually exclusive with `--no-auth`) |
| `--auth-cache-ttl-secs <n>` | `AXON_AUTH_CACHE_TTL_SECS` | `60` | Identity cache TTL in seconds |

```bash
# In-memory, no auth (development)
axon serve --no-auth --storage memory

# SQLite, no auth
axon serve --no-auth --sqlite-path ./axon.db

# Guest mode — unauthenticated callers get read-only access
axon serve --guest-role read

# Production (Tailscale default) with gRPC
axon serve --storage postgres --postgres-dsn "$DSN" --grpc-port 4171

# Custom Tailscale socket path
axon serve --tailscale-socket /var/run/tailscale/tailscaled.sock
```

**TLS**

| Flag | Env var | Default | Description |
|------|---------|---------|-------------|
| `--tls-cert <path>` | `AXON_TLS_CERT` | — | PEM-encoded TLS certificate. Requires `--tls-key`. |
| `--tls-key <path>` | `AXON_TLS_KEY` | — | PEM-encoded TLS private key. Requires `--tls-cert`. |
| `--tls-self-signed` | `AXON_TLS_SELF_SIGNED` | `false` | Generate a self-signed cert on first start (dev only) |
| `--tls-self-signed-san <list>` | `AXON_TLS_SELF_SIGNED_SAN` | — | Comma-separated extra SAN entries (DNS names or IPs) |

When both `--tls-cert` and `--tls-key` are supplied (or any of the equivalent
env vars), the server listens on HTTPS using the operator-provided
certificate. Without TLS flags, the server falls back to plain HTTP.

The auto-generated `--tls-self-signed` certificate covers `localhost`,
`127.0.0.1`, `::1`, `0.0.0.0`, and the local hostname by default. If clients
reach the server over a different name (machine hostname, tailnet name, LAN
IP), extend the SAN list rather than disabling TLS verification:

```bash
# Self-signed cert that browsers and Node clients trust over the LAN
axon serve --tls-self-signed \
  --tls-self-signed-san sindri,sindri.local,100.64.0.5

# Or hand a CA-signed leaf cert directly (e.g. mkcert workflow):
mkcert -install
mkcert sindri sindri.local localhost 127.0.0.1
axon serve --tls-cert ./sindri+3.pem --tls-key ./sindri+3-key.pem
```

The self-signed cert is regenerated only when the cert/key files are
absent. Delete them to refresh the SAN list. Production deployments should
provide a CA-signed cert via `--tls-cert` / `--tls-key`.

---

## axon collections

```bash
axon collections <subcommand>
```

### create

```bash
axon collections create <name>
axon collections create prod.billing.invoices   # namespaced
```

### list

```bash
axon collections list
```

### drop

```bash
axon collections drop <name> --confirm
```

Removes all entities, links, and schema history. Irreversible.

### template

Manage Markdown rendering templates for a collection.

```bash
axon collections template put   <name> --template "# {{title}}"
axon collections template get   <name>
axon collections template delete <name>
```

---

## axon entities

```bash
axon entities <subcommand>
```

### create

```bash
axon entities create <collection> --id <id> --data '<json>'
axon entities create tasks --id task-001 --data '{"title":"Ship it","status":"open"}'

# Omit --id to auto-generate a UUIDv7
axon entities create tasks --data '{"title":"Auto-ID","status":"open"}'
```

### get

```bash
axon entities get <collection> <id>
axon entities get tasks task-001

# Render Markdown template (if defined)
axon entities get tasks task-001 --render markdown
```

### list

```bash
axon entities list <collection> [--limit <n>]
axon entities list tasks --limit 20
```

### update

```bash
axon entities update <collection> <id> --data '<json>' [--expected-version <n>]

# Version auto-fetched if omitted (single-user convenience)
axon entities update tasks task-001 --data '{"title":"Ship it","status":"done"}'

# Explicit version for safe concurrent updates
axon entities update tasks task-001 --expected-version 3 --data '...'
```

### delete

```bash
axon entities delete <collection> <id>
```

### query

```bash
axon entities query <collection> [--filter <field>=<value>]... [--limit <n>]

axon entities query tasks --filter status=open
axon entities query tasks --filter status=done --filter priority=1
```

---

## axon schema

```bash
axon schema <subcommand>
```

### set

```bash
axon schema set <collection> --schema '<json-schema>' [--force] [--dry-run]

# From a file
axon schema set tasks --file schema/tasks.json

# Breaking change — requires --force
axon schema set tasks --schema '...' --force

# Preview without applying
axon schema set tasks --schema '...' --dry-run
```

### show

```bash
axon schema show <collection>
```

---

## axon links

```bash
axon links <subcommand>
```

### set

```bash
axon links set <src-collection> <src-id> <tgt-collection> <tgt-id> --type <link-type>

axon links set tasks task-001 projects proj-alpha --type belongs-to
axon links set tasks task-002 tasks   task-001   --type depends-on
```

### list

```bash
axon links list <collection> <id>
axon links list tasks task-002
```

Returns all entities reachable via outbound links at depth 1.

---

## axon graph

Traverse the entity graph from a starting entity.

```bash
axon graph <collection> <id> [--depth <n>] [--link-type <type>]

axon graph tasks task-002 --depth 2
axon graph tasks task-002 --link-type depends-on --depth 5
```

---

## axon audit

```bash
axon audit list [flags]
```

| Flag | Description |
|------|-------------|
| `--collection <name>` | Filter by collection |
| `--entity-id <id>` | Filter by entity ID |
| `--actor <name>` | Filter by actor |
| `--limit <n>` | Maximum entries to return |

```bash
axon audit list --collection tasks --limit 10
axon audit list --entity-id task-001
axon audit list --actor alice@example.com --limit 20
```

---

## axon doctor

Print diagnostic information about the Axon installation.

```bash
axon doctor
```

Output includes:
- Axon version
- Config file path (and whether it exists)
- Data directory path
- Storage backend
- HTTP port
- Server connectivity check

---

## axon server

Manage the Axon system service.

```bash
axon server <subcommand>
```

| Subcommand | Description |
|------------|-------------|
| `install [--global]` | Install as a systemd/launchd service |
| `uninstall` | Remove the service |
| `start` | Start the service |
| `stop` | Stop the service |
| `restart` | Restart the service |
| `status` | Show service status |

```bash
axon server install          # user-level service
axon server install --global # system-level service (requires sudo)
axon server start
axon server status
```

---

## axon config

```bash
axon config show   # print resolved configuration
axon config path   # print config file path
```
