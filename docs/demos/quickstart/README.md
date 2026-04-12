# Axon Quickstart Demo

Full lifecycle walkthrough of the Axon CLI — server, collections, schema, entities, links, graph, audit.

## What it demonstrates

1. Start an in-memory Axon server
2. Create collections (`tasks`, `projects`)
3. Define a JSON Schema with required fields and enums
4. Create entities across both collections
5. List, get, update, and query entities
6. Set typed links between entities
7. List outbound links and traverse the graph
8. Query the audit log
9. Evolve the schema (compatible and breaking changes)
10. Drop a collection

## Prerequisites

- Docker

## Record the demo

```bash
# From the repo root:

# 1. Build the main image
docker build -t axon:demo .

# 2. Build the recording image
docker build -t axon:recording docs/demos/quickstart/

# 3. Record
mkdir -p website/static/demos
docker run --rm \
  -e TERM=xterm-256color \
  -v $(pwd)/website/static/demos:/recordings \
  axon:recording
```

The cast file lands at `website/static/demos/quickstart.cast`.

## Re-run without recording

```bash
docker run --rm --entrypoint bash axon:demo /scripts/demo.sh
```

## Files

| File | Purpose |
|------|---------|
| `demo.sh` | Shell script that drives the demo |
| `Dockerfile` | Recording image (axon:demo + asciinema) |
| `README.md` | This file |
| `recordings/` | Output directory for `.cast` files (not committed) |
