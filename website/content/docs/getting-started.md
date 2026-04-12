---
title: Getting Started
weight: 1
next: /docs/concepts
---

Get Axon running and create your first entity in under 5 minutes.

## Prerequisites

- **Docker** (recommended) — no other dependencies
- Or a Linux/macOS machine with `curl` to install the binary

## Option A: Docker (quickest)

```bash
docker run --rm -p 4170:4170 \
  ghcr.io/documentdrivendx/axon:latest \
  serve --no-auth --storage memory
```

The server starts on port 4170. Test it:

```bash
curl http://localhost:4170/health
# {"status":"ok"}
```

## Option B: Install the binary

```bash
curl -sf https://DocumentDrivenDX.github.io/axon/install.sh | sh
```

This installs `axon` to `~/.local/bin/axon`. Make sure `~/.local/bin` is in your `$PATH`.

## Start a development server

```bash
# In-memory storage, no authentication — for local development only
axon serve --no-auth --storage memory

# Or persist to SQLite:
axon serve --no-auth --sqlite-path ./axon.db
```

The server runs on **port 4170** by default. Use `axon serve --help` for all options.

## Create your first collection

While the server is running, open a second terminal:

```bash
axon collections create tasks
```

```json
{"name": "tasks"}
```

## Define a schema

```bash
axon schema set tasks --schema '{
  "type": "object",
  "properties": {
    "title":  {"type": "string"},
    "status": {"type": "string", "enum": ["open", "in-progress", "done"]},
    "priority": {"type": "integer"}
  },
  "required": ["title", "status"]
}'
```

## Create entities

```bash
axon entities create tasks \
  --id task-001 \
  --data '{"title":"Set up database","status":"done","priority":1}'

axon entities create tasks \
  --id task-002 \
  --data '{"title":"Build REST API","status":"in-progress","priority":2}'
```

## Query entities

```bash
# List all
axon entities list tasks

# Filter by field
axon entities query tasks --filter status=open

# Get a single entity
axon entities get tasks task-001
```

## Link entities

```bash
axon collections create projects
axon entities create projects --id proj-alpha --data '{"name":"Alpha"}'

# Create a link between task-001 and proj-alpha
axon links set tasks task-001 projects proj-alpha --type belongs-to

# See all outbound links from task-001
axon links list tasks task-001
```

## Check the audit log

```bash
axon audit list --collection tasks --limit 5
```

Every create, update, delete, and schema change is recorded with actor, timestamp, and before/after data.

## Diagnose connectivity

```bash
axon doctor
```

`axon doctor` prints config paths, the resolved storage backend, HTTP port, and whether the server is reachable.

## Next steps

{{< cards >}}
  {{< card link="/docs/concepts" title="Core Concepts" subtitle="Understand collections, schema evolution, links, and the audit model." >}}
  {{< card link="/docs/cli" title="CLI Reference" subtitle="Complete reference for every axon subcommand." >}}
  {{< card link="/docs/demos" title="Full Demo" subtitle="Watch the complete lifecycle walkthrough." >}}
{{< /cards >}}
