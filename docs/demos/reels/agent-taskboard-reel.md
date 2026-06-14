# Agent Taskboard Reel

Release target: Axon 0.7.1

A governed task and bead queue that demonstrates collections, schemas, entity CRUD, links, graph traversal, audit, optimistic concurrency, MCP-oriented task discovery, and the unified CLI.

Sample project: `examples/agent-taskboard`

## Storyboard

1. Create project, bead, and agent collections with schema validation.
2. Seed beads and typed dependency links.
3. Query ready work, claim one bead with optimistic concurrency, then inspect audit history.
4. Traverse dependencies so agents can explain why work is ready or blocked.

## Coverage Entries

- feature: FEAT-001 - Collections
- feature: FEAT-004 - Entity Operations
- feature: FEAT-005 - API Surface
- feature: FEAT-006 - Bead Storage Adapter
- feature: FEAT-016 - MCP Server
- feature: FEAT-028 - Unified Binary & Service Management
- scenario: SCN-006 - Issue Tracking - Dependency DAG and Ready Queue
- scenario: SCN-007 - Agentic - Bead Lifecycle with Concurrent Agents
- story: US-001 - Create a Collection
- story: US-002 - List and Inspect Collections
- story: US-003 - Drop a Collection
- story: US-010 - CRUD an Entity
- story: US-011 - Query Entities
- story: US-012 - Partial Update
- story: US-013 - Use Axon from an Agent
- story: US-014 - Use Axon from the Command Line
- story: US-015 - Store and Query Beads
- story: US-016 - Track Bead Dependencies
- story: US-052 - Agent Discovers Axon via MCP
- story: US-053 - Agent CRUDs Entities via MCP
- story: US-054 - Agent Queries via GraphQL through MCP
- story: US-055 - Agent Subscribes to Changes via MCP
- story: US-056 - Local Agent Connects via Stdio
- story: US-080 - Consistent Point-in-Time Snapshot
- story: US-112 - Agent Discovers Policy Envelopes
- story: US-126 - Start Axon from a single binary
- story: US-127 - Diagnose Axon installation
- story: US-128 - Install Axon as a system service
- story: US-129 - Use CLI against a running server
- story: US-131 - Install Axon with a single command
- story: US-134 - Configure Axon persistently
- use_case: USE-006 - Issue Tracking
- use_case: USE-010 - Agentic Applications
