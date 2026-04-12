---
title: Axon
layout: hextra-home
---

{{< hextra/hero-badge link="https://github.com/DocumentDrivenDX/axon" >}}
  <span>Open Source · MIT</span>
  {{< icon name="arrow-circle-right" attributes="height=14" >}}
{{< /hextra/hero-badge >}}

<div class="hx-mt-6 hx-mb-6">
{{< hextra/hero-headline >}}
  Agent-native state management.&nbsp;<br class="sm:hx-block hx-hidden" />Schema-first. Auditable by default.
{{< /hextra/hero-headline >}}
</div>

<div class="hx-mb-12">
{{< hextra/hero-subtitle >}}
  Axon is a transactional entity store for agentic applications — structured storage with schema validation, immutable audit logs, graph relationships, and APIs designed for how agents actually consume data.
{{< /hextra/hero-subtitle >}}
</div>

<div class="hx-mb-12">
{{< hextra/hero-button text="Get Started" link="docs/getting-started" >}}
{{< hextra/hero-button text="Watch Demo" link="docs/demos" style="alt" >}}
</div>

<div class="hx-mt-8"></div>

{{< hextra/feature-grid >}}
  {{< hextra/feature-card
    title="Schema-first collections"
    subtitle="Every collection has a JSON Schema. All writes are validated before they reach storage. Schema evolution is tracked, diffed, and controlled."
    style="background: radial-gradient(ellipse at 50% 80%,rgba(72,120,198,0.15),hsla(0,0%,100%,0));"
  >}}
  {{< hextra/feature-card
    title="Immutable audit log"
    subtitle="Every mutation — create, update, delete, schema change — produces an immutable record with actor, timestamp, and before/after data."
    style="background: radial-gradient(ellipse at 50% 80%,rgba(120,72,198,0.15),hsla(0,0%,100%,0));"
  >}}
  {{< hextra/feature-card
    title="Graph relationships"
    subtitle="Link entities across collections with typed edges. Traverse graphs to arbitrary depth. Model dependencies, hierarchies, and ownership natively."
    style="background: radial-gradient(ellipse at 50% 80%,rgba(72,198,120,0.15),hsla(0,0%,100%,0));"
  >}}
  {{< hextra/feature-card
    title="Agent-native APIs"
    subtitle="REST, gRPC, GraphQL, and MCP — all served from the same unified binary. Agents get transactional writes, optimistic concurrency, and structured queries."
  >}}
  {{< hextra/feature-card
    title="Unified CLI"
    subtitle="One binary covers the full lifecycle: serve, manage collections, CRUD entities, evolve schemas, inspect audit logs, traverse graphs."
  >}}
  {{< hextra/feature-card
    title="Embedded or server"
    subtitle="Run Axon in-process with SQLite or as a standalone server with PostgreSQL. Same API, same CLI, same audit guarantees."
  >}}
{{< /hextra/feature-grid >}}

<div class="hx-mt-16"></div>

## Why Axon?

Agent state management is an unsolved infrastructure problem. Agents modify state without provenance, schemas drift silently, and concurrent operations produce corrupt state. Current solutions — Firebase, Supabase, PocketBase, DoltDB — were built for human-driven UIs, not for agents that need audit trails, strict schemas, and graph-aware queries.

Axon is purpose-built for this gap:

- **Audit-first** — provenance is not a feature, it is the architecture. Every change is immutable and queryable.
- **Schema-first** — define structure once, get validation, diffing, and documentation everywhere.
- **Agent-friendly** — MCP server, GraphQL introspection, structured filters, and optimistic concurrency give agents the guarantees they need to act safely.
- **Local-first** — run embedded with SQLite in a laptop, serve with PostgreSQL in production. Same binary, zero reconfiguration.

```bash
# Install
curl -sf https://DocumentDrivenDX.github.io/axon/install.sh | sh

# Start a server (in-memory, no auth — for development)
axon serve --no-auth --storage memory

# Create a collection and define its schema
axon collections create tasks
axon schema set tasks --schema '{"type":"object","properties":{"title":{"type":"string"},"status":{"type":"string"}},"required":["title","status"]}'

# Create an entity
axon entities create tasks --id task-001 --data '{"title":"Ship it","status":"open"}'
```
