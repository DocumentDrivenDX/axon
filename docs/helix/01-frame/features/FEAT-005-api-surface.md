---
ddx:
  id: FEAT-005
  depends_on:
    - helix.prd
    - FEAT-001
    - FEAT-004
---
# Feature Specification: FEAT-005 - API Surface

**Feature ID**: FEAT-005
**Status**: Draft
**Priority**: P0
**Owner**: Core Team
**Created**: 2026-04-04
**Updated**: 2026-04-22

## Overview

The API surface is how agents, applications, and humans interact with Axon. It
provides a unified foundation for collection management, entity operations,
schema inspection, and audit queries. The API is designed for programmatic
consumption - structured requests, structured responses, structured errors -
while also being usable via CLI for human operators.

FEAT-005 defines the internal and compatibility API foundation. FEAT-015
defines GraphQL as the primary public application API. FEAT-016 defines MCP as
the agent-native surface. REST/JSON and gRPC may remain useful compatibility,
SDK, or operational surfaces, but they are not the product-defining interface.

## Problem Statement

Agents need a well-defined, self-documenting API that they can call reliably.
Existing database APIs are either too low-level (SQL), too unstructured
(ad-hoc JSON), too endpoint-centric for graph traversal, or too vendor-specific
(Firebase SDK). Axon needs a structured operation foundation that GraphQL, MCP,
SDKs, and CLI tools can share.

## Requirements

### Functional Requirements

- **Operation foundation**: Native handler traits and typed request/response
  structures are the canonical implementation boundary below GraphQL, MCP, CLI,
  SDKs, and compatibility routes
- **Public application protocol**: GraphQL is primary; see FEAT-015
- **Agent protocol**: MCP mirrors GraphQL semantics for agents; see FEAT-016
- **Compatibility protocols**: gRPC/protobuf and HTTP/JSON may be exposed for
  SDK compatibility, operational integrations, and cases where GraphQL is
  intractable
- **Operations**: Full coverage of collection, entity, schema, and audit operations as defined in FEAT-001 through FEAT-004
- **Self-describing**: GraphQL schema, MCP tool schemas, and any protobuf
  definitions are generated views of the same Axon operation and ESF metadata
- **Streaming**: Support server-streaming for change feeds (P1) and large query results
- **Error model**: Structured errors with error code, message, field-level
  details, policy/intent detail where applicable, and suggested action.
  GraphQL error extensions and MCP tool errors preserve these fields
- **Embedded API**: In embedded mode, the same API is available as a native library call (no network overhead). Same types, same behavior
- **CLI**: `axon` CLI wraps the API for human operators. Every API operation has a CLI equivalent

### CLI Requirements

- **Collection management**: `axon collection create|list|describe|drop`
- **Entity operations**: `axon entity create|get|update|delete|list|query` (CLI subcommand is `entity` to match the data model; `doc` is not provided as an alias — see decision note below)
- **Schema operations**: `axon schema show|validate`
- **Audit operations**: `axon audit list|show|revert`
- **Output formats**: Human-readable table (default), JSON, YAML
- **Configuration**: `axon config` for connection settings, defaults

#### CLI Subcommand Naming Decision

**Decision**: The CLI subcommand for entity operations is `axon entity` (not `axon doc`).

**Rationale**: The entire spec stack uses "entity" as the canonical term for Axon data records (established in commit 7d905a7 / FEAT-001 through FEAT-004). Using `axon doc` would create a permanent terminology split between the data model vocabulary ("entity") and the CLI vocabulary ("doc"). No `doc` alias is provided — a consistent name is clearer than a short alias that perpetuates the old terminology.

### Non-Functional Requirements

- **Latency**: Compatibility network overhead < 1ms for local server; GraphQL
  and MCP overhead targets are owned by FEAT-015 and FEAT-016
- **Compatibility**: HTTP gateway supports any HTTP client. No SDK required for basic operations
- **Documentation**: GraphQL schema, MCP tool schemas, and compatibility
  protocol definitions include comments. OpenAPI may be generated for fallback
  HTTP routes
- **Versioning**: API is versioned (v1). Breaking changes require version bump

## User Stories

### Story US-013: Use Axon from an Agent [FEAT-005]

**As an** agent framework
**I want** a typed GraphQL/MCP-backed API for Axon operations
**So that** I can store and query state without hand-assembling HTTP requests

**Acceptance Criteria:**
- [ ] GraphQL-first client SDK available for TypeScript
- [ ] Create, read, update, delete, query, preview, and approve entities via SDK
- [ ] Structured error types that agents can match on programmatically
- [ ] SDK works identically against embedded and server modes

### Story US-014: Use Axon from the Command Line [FEAT-005]

**As a** developer managing Axon
**I want** CLI commands for all operations
**So that** I can inspect and manage data without writing code

**Acceptance Criteria:**
- [ ] `axon entity list <collection>` shows entities in a readable table
- [ ] `axon entity query <collection> --filter "status=pending"` returns matching entities
- [ ] `axon audit list --collection <name> --last 10` shows recent changes
- [ ] `--output json` flag returns machine-parseable output
- [ ] `axon` with no args shows help

### Client Mode (added by FEAT-028)

- **Client mode**: When a server is reachable at the configured URL
  (`http://localhost:4170` by default), CLI commands issue HTTP requests
  to the server API. The CLI uses the same HTTP routes already defined
  by the gateway — no new protocol.
- **Mode selection**: `--embedded` forces embedded SQLite mode.
  `--server <url>` forces client mode against a specific URL. Default
  behavior: attempt HTTP connection to configured server URL; if
  unreachable within 200ms, fall back to embedded.
- **Output parity**: JSON/table/YAML output formats work identically in
  both modes.
- **Server as source of truth**: When a server is running, client mode
  is the expected path. Embedded mode is for offline or development use
  without a server.

## Edge Cases and Error Handling

- **Server unavailable**: Client SDKs return connection error with retry guidance
- **Invalid request**: Malformed requests return 400 with specific field-level errors
- **Auth failure**: Missing or invalid credentials return 401/403 with clear message
- **Rate limiting**: V1 does not rate limit. P2 may add configurable rate limits
- **Large responses**: Paginated by default. Streaming for very large result sets

## Dependencies

- **FEAT-001** through **FEAT-004**: API exposes all their operations
- **FEAT-015**: GraphQL is the primary public application surface
- **FEAT-016**: MCP is the agent-native surface
- Protobuf/gRPC and OpenAPI tooling only where compatibility surfaces are kept

## Out of Scope

- Redefining FEAT-015 GraphQL semantics
- Redefining FEAT-016 MCP semantics
- Treating REST/gRPC parity as a blocker for policy, preview, or approval
- Admin dashboard / web UI (owned by FEAT-011)

## Traceability

### Related Artifacts
- **Parent PRD Section**: Requirements Overview > P0 #10 (GraphQL-first API
  surface), P0 #11 (MCP surface), P0 #15 (CLI)
- **User Stories**: US-013, US-014
- **Test Suites**: `tests/FEAT-005/`
- **Implementation**: `src/api/`, `src/cli/`, `proto/` or equivalent

### Feature Dependencies
- **Depends On**: FEAT-001, FEAT-002, FEAT-003, FEAT-004
- **Depended By**: FEAT-006 (Bead Storage Adapter), FEAT-015 (GraphQL),
  FEAT-016 (MCP), FEAT-028 (Unified Binary)
