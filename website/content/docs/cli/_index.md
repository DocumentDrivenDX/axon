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

| Flag | Default | Description |
|------|---------|-------------|
| `--http-port <n>` | `4170` | HTTP listener port |
| `--grpc-port <n>` | disabled | Enable gRPC on this port |
| `--storage <backend>` | `sqlite` | Storage backend: `sqlite`, `postgres`, `memory` |
| `--sqlite-path <path>` | XDG data dir | SQLite database file path |
| `--no-auth` | — | Disable authentication (dev mode) |
| `--control-plane-path <path>` | XDG data dir | Control-plane SQLite path |
| `--ui-dir <path>` | — | Serve admin UI static files from this directory |

```bash
# In-memory, no auth (development)
axon serve --no-auth --storage memory

# SQLite, no auth
axon serve --no-auth --sqlite-path ./axon.db

# Production with gRPC
axon serve --storage postgres --grpc-port 4171
```

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
