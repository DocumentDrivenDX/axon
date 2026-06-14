---
title: Agent Taskboard Reel
weight: 10
prev: ../
---

# Agent Taskboard Reel

Release target: Axon 0.7.1

A governed task and bead queue that demonstrates collections, schemas, entity CRUD, links, graph traversal, audit, optimistic concurrency, MCP-oriented task discovery, and the unified CLI.

- Sample project: [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard)
- Script source: [`docs/demos/reels/agent-taskboard-reel.md`](https://github.com/DocumentDrivenDX/axon/blob/master/docs/demos/reels/agent-taskboard-reel.md)
- Coverage entries: 33

## Storyboard

1. Create project, bead, and agent collections with schema validation.
2. Seed beads and typed dependency links.
3. Query ready work, claim one bead with optimistic concurrency, then inspect audit history.
4. Traverse dependencies so agents can explain why work is ready or blocked.

## Covered HELIX Entries

| Type | ID | Title | Source | Sample | Demo reel |
|---|---|---|---|---|---|
| feature | FEAT-001 | Collections | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/features/FEAT-001-collections.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| feature | FEAT-004 | Entity Operations | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/features/FEAT-004-entity-operations.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| feature | FEAT-005 | API Surface | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/features/FEAT-005-api-surface.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| feature | FEAT-006 | Bead Storage Adapter | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/features/FEAT-006-bead-storage-adapter.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| feature | FEAT-016 | MCP Server | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/features/FEAT-016-mcp-server.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| feature | FEAT-028 | Unified Binary & Service Management | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/features/FEAT-028-unified-binary.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| scenario | SCN-006 | Issue Tracking - Dependency DAG and Ready Queue | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/03-test/test-plan.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| scenario | SCN-007 | Agentic - Bead Lifecycle with Concurrent Agents | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/03-test/test-plan.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| story | US-001 | Create a Collection | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-001-create-a-collection.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| story | US-002 | List and Inspect Collections | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-002-list-and-inspect-collections.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| story | US-003 | Drop a Collection | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-003-drop-a-collection.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| story | US-010 | CRUD an Entity | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-010-crud-an-entity.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| story | US-011 | Query Entities | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-011-query-entities.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| story | US-012 | Partial Update | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-012-partial-update.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| story | US-013 | Use Axon from an Agent | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-013-use-axon-from-an-agent.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| story | US-014 | Use Axon from the Command Line | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-014-use-axon-from-the-command-line.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| story | US-015 | Store and Query Beads | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-015-store-and-query-beads.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| story | US-016 | Track Bead Dependencies | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-016-track-bead-dependencies.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| story | US-052 | Agent Discovers Axon via MCP | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-052-agent-discovers-axon-via-mcp.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| story | US-053 | Agent CRUDs Entities via MCP | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-053-agent-cruds-entities-via-mcp.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| story | US-054 | Agent Queries via GraphQL through MCP | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-054-agent-queries-via-graphql-through-mcp.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| story | US-055 | Agent Subscribes to Changes via MCP | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-055-agent-subscribes-to-changes-via-mcp.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| story | US-056 | Local Agent Connects via Stdio | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-056-local-agent-connects-via-stdio.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| story | US-080 | Consistent Point-in-Time Snapshot | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-080-consistent-point-in-time-snapshot.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| story | US-112 | Agent Discovers Policy Envelopes | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-112-agent-discovers-policy-envelopes.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| story | US-126 | Start Axon from a single binary | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-126-start-axon-from-a-single-binary.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| story | US-127 | Diagnose Axon installation | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-127-diagnose-axon-installation.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| story | US-128 | Install Axon as a system service | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-128-install-axon-as-a-system-service.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| story | US-129 | Use CLI against a running server | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-129-use-cli-against-a-running-server.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| story | US-131 | Install Axon with a single command | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-131-install-axon-with-a-single-command.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| story | US-134 | Configure Axon persistently | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-134-configure-axon-persistently.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| use case | USE-006 | Issue Tracking | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/00-discover/use-case-research.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
| use case | USE-010 | Agentic Applications | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/00-discover/use-case-research.md) | [agent-taskboard](https://github.com/DocumentDrivenDX/axon/tree/master/examples/agent-taskboard) | [agent-taskboard-reel](../agent-taskboard-reel/) |
