---
dun:
  id: FEAT-006
  depends_on:
    - helix.prd
    - FEAT-001
    - FEAT-004
---
# Feature Specification: FEAT-006 - Bead Storage Adapter

**Feature ID**: FEAT-006
**Status**: Draft
**Priority**: P1
**Owner**: Core Team
**Created**: 2026-04-04
**Updated**: 2026-04-04

## Overview

The bead storage adapter is a purpose-built collection schema and API layer for storing beads — the portable work items used by agentic frameworks like steveyegge/beads, br, and DDx. It provides a pre-defined schema, lifecycle state management, dependency tracking, and ready-queue queries. This is Axon's first "opinionated module" — a domain-specific layer built on the generic collection/document primitives.

## Problem Statement

Every agentic framework that uses beads (or bead-like work items) reinvents storage: file-based JSON, SQLite, custom CRUD APIs. The bead data model — with its lifecycle states, dependency DAGs, and ready-queue semantics — is common enough to standardize. Axon should provide a bead collection that works out of the box.

## Requirements

### Functional Requirements

- **Pre-defined bead schema**: A built-in schema for bead documents covering: id, type, status, title, description, content, dependencies, dependents, metadata, tags, assignee, priority, timestamps
- **Lifecycle states**: Beads have a status lifecycle: `draft` -> `pending` -> `ready` -> `in_progress` -> `review` -> `done` (with `blocked` and `cancelled` as terminal/side states)
- **Dependency tracking**: Beads can declare dependencies on other beads. The dependency graph is queryable
- **Ready queue**: Query for beads that are `pending` with all dependencies satisfied (i.e., all deps are `done`). These are "ready" beads
- **Bead-specific queries**: Find beads by status, type, assignee, tag, dependency state
- **Import/export**: Import beads from JSON (compatible with steveyegge/beads format). Export beads to JSON

### Non-Functional Requirements

- **Compatibility**: Bead schema is compatible with steveyegge/beads and DDx bead tracker formats (superset, not subset)
- **Performance**: Ready-queue query < 50ms for collections with < 10,000 beads

## User Stories

### Story US-015: Store and Query Beads [FEAT-006]

**As an** agent framework
**I want** a purpose-built bead collection in Axon
**So that** I don't have to reinvent bead storage and lifecycle management

**Acceptance Criteria:**
- [ ] `axon bead init` creates a bead collection with the standard schema
- [ ] `axon bead create --type task --title "Review PR"` creates a bead
- [ ] `axon bead list --status pending` lists pending beads
- [ ] `axon bead ready` lists beads with all dependencies satisfied
- [ ] Bead status transitions are validated (can't go from `draft` to `done` directly)

### Story US-016: Track Bead Dependencies [FEAT-006]

**As an** agent managing a work queue
**I want** to declare and query bead dependencies
**So that** I can determine which beads are ready to work on

**Acceptance Criteria:**
- [ ] Beads can declare dependencies on other bead IDs
- [ ] Creating a circular dependency is detected and rejected
- [ ] `axon bead deps <id>` shows the dependency tree for a bead
- [ ] Ready-queue computation correctly identifies beads with all deps satisfied

## Dependencies

- **FEAT-001** (Collections): Bead collection is a regular collection with a pre-defined schema
- **FEAT-004** (Document Operations): Bead CRUD uses standard document operations
- **FEAT-005** (API Surface): Bead-specific CLI commands and API endpoints

## Out of Scope

- Bead execution / agent dispatch (beads stores state, doesn't run agents)
- Cross-Axon-instance bead sync
- Bead visualization / UI

## Traceability

### Related Artifacts
- **Parent PRD Section**: Requirements Overview > P1 #7 (Bead Storage Adapter)
- **User Stories**: US-015, US-016
- **Prior Art**: steveyegge/beads, DDx bead tracker
- **Test Suites**: `tests/FEAT-006/`
- **Implementation**: `src/adapters/beads/` or equivalent

### Feature Dependencies
- **Depends On**: FEAT-001, FEAT-004, FEAT-005
- **Depended By**: None (leaf feature)
