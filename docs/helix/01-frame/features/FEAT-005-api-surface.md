---
dun:
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
**Updated**: 2026-04-04

## Overview

The API surface is how agents, applications, and humans interact with Axon. It provides a unified interface for collection management, entity operations, schema inspection, and audit queries. The API is designed for programmatic consumption — structured requests, structured responses, structured errors — while also being usable via CLI for human operators.

## Problem Statement

Agents need a well-defined, self-documenting API that they can call reliably. Existing database APIs are either too low-level (SQL), too unstructured (REST with ad-hoc JSON), or too vendor-specific (Firebase SDK). Axon needs an API that is standard, typed, and equally usable by agents, client SDKs, and CLI tools.

## Requirements

### Functional Requirements

- **Protocol**: gRPC as primary protocol (strongly typed, streaming support, code generation). HTTP/JSON gateway for broad compatibility
- **Operations**: Full coverage of collection, document, schema, and audit operations as defined in FEAT-001 through FEAT-004
- **Self-describing**: API schema (protobuf definitions) is the source of truth. Client SDKs are generated from protobuf
- **Streaming**: Support server-streaming for change feeds (P1) and large query results
- **Error model**: Structured errors with error code, message, field-level details, and suggested action. gRPC status codes map cleanly to HTTP status codes
- **Embedded API**: In embedded mode, the same API is available as a native library call (no network overhead). Same types, same behavior
- **CLI**: `axon` CLI wraps the API for human operators. Every API operation has a CLI equivalent

### CLI Requirements

- **Collection management**: `axon collection create|list|describe|drop`
- **Document operations**: `axon doc create|get|update|delete|list|query`
- **Schema operations**: `axon schema show|validate`
- **Audit operations**: `axon audit list|show|revert`
- **Output formats**: Human-readable table (default), JSON, YAML
- **Configuration**: `axon config` for connection settings, defaults

### Non-Functional Requirements

- **Latency**: Network overhead < 1ms for local server. gRPC keeps connections warm
- **Compatibility**: HTTP gateway supports any HTTP client. No SDK required for basic operations
- **Documentation**: Protobuf definitions include comments. OpenAPI spec generated from gateway
- **Versioning**: API is versioned (v1). Breaking changes require version bump

## User Stories

### Story US-013: Use Axon from an Agent [FEAT-005]

**As an** agent framework
**I want** a typed API for Axon operations
**So that** I can store and query state without hand-assembling HTTP requests

**Acceptance Criteria:**
- [ ] gRPC client SDK available for Go and TypeScript
- [ ] Create, read, update, delete, query documents via SDK
- [ ] Structured error types that agents can match on programmatically
- [ ] SDK works identically against embedded and server modes

### Story US-014: Use Axon from the Command Line [FEAT-005]

**As a** developer managing Axon
**I want** CLI commands for all operations
**So that** I can inspect and manage data without writing code

**Acceptance Criteria:**
- [ ] `axon doc list <collection>` shows documents in a readable table
- [ ] `axon doc query <collection> --filter "status=pending"` returns matching docs
- [ ] `axon audit list --collection <name> --last 10` shows recent changes
- [ ] `--output json` flag returns machine-parseable output
- [ ] `axon` with no args shows help

## Edge Cases and Error Handling

- **Server unavailable**: Client SDKs return connection error with retry guidance
- **Invalid request**: Malformed requests return 400 with specific field-level errors
- **Auth failure**: Missing or invalid credentials return 401/403 with clear message
- **Rate limiting**: V1 does not rate limit. P2 may add configurable rate limits
- **Large responses**: Paginated by default. Streaming for very large result sets

## Dependencies

- **FEAT-001** through **FEAT-004**: API exposes all their operations
- Protobuf toolchain for code generation
- gRPC-gateway for HTTP bridge

## Out of Scope

- WebSocket API (change feeds via gRPC streaming instead)
- GraphQL endpoint (P2 consideration given entity-graph model)
- Admin dashboard / web UI (P2)

## Traceability

### Related Artifacts
- **Parent PRD Section**: Requirements Overview > P0 #6 (API Surface), P0 #8 (CLI)
- **User Stories**: US-013, US-014
- **Test Suites**: `tests/FEAT-005/`
- **Implementation**: `src/api/`, `src/cli/`, `proto/` or equivalent

### Feature Dependencies
- **Depends On**: FEAT-001, FEAT-002, FEAT-003, FEAT-004
- **Depended By**: FEAT-006 (Bead Storage Adapter)
