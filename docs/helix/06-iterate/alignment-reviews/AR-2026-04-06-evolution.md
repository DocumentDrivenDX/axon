# Evolution Report: Strategic Design Review

**Date**: 2026-04-06
**Source**: Strategic design discussion (April 2026)
**Type**: Artifact stack evolution

---

## Summary

Threaded a strategic design review through the full artifact stack. The
review refined Axon's positioning as an entity-first OLTP store, introduced
four new capabilities, and sharpened existing requirements.

## Artifacts Updated

### Product Vision (`docs/helix/00-discover/product-vision.md`)

| Change | Detail |
|--------|--------|
| Mission statement | Reframed as "entity-first OLTP store" (was "cloud-native data store") |
| Core Thesis section | NEW — four pillars: entity-aware, schema-driven, auditable, agent-accessible |
| Agent-First, Human-Friendly | NEW — positioning for three audiences (frontend, backend, agent) |
| What Axon Is Not | NEW — not analytics, not general-purpose DB, not distributed |
| Beyond Agent Platforms | NEW — ERP, CDP, artifact management as target domains |
| Commercial Model | NEW — BYOC, source-available license, separate GitHub org |
| Version | Revised date added (2026-04-06) |

### PRD (`docs/helix/01-frame/prd.md`)

| Change | Detail |
|--------|--------|
| Version | 0.1.0 → 0.2.0 |
| Executive Summary | Rewritten with OLTP framing and EAV storage model explanation |
| Strategic Fit | Added "application substrate" bullet |
| P1 #1 Schema Evolution | Expanded with zero-downtime, required field defaults, constraint tightening, per-entity version tracking |
| P1 #16 Agent Guardrails | NEW — scope constraints, rate limiting, semantic validation hooks |
| P1 #17 Rollback/Recovery | NEW — point-in-time, entity-level, transaction-level, dry-run |
| P2 #8 Application Substrate | NEW — auto-generated TS client, admin UI, deployment templates |
| P2 #9 Control Plane | NEW — multi-tenant management, BYOC support |
| Not Scheduled | Added PostgreSQL-compatible SQL, Git backend |
| Risks | Added 4 new risks: EAV performance, heterogeneous indexes, agent semantic misuse, backend abstraction leakage |
| Section 14 | NEW — Organizational and licensing (GitHub org, source-available, BYOC) |

### Technical Requirements (`docs/helix/01-frame/technical-requirements.md`)

| Change | Detail |
|--------|--------|
| Version | 0.1.0 → 0.2.0 |
| Section 2a | NEW — EAV Storage Model (explicitly documents the internal model) |
| Section 4b | Added Future Index Types (vector, BM25) with co-location risk |
| Section 9 | NEW — Control Plane (P2) architecture |
| Section 10 | NEW — Client-Side Validation (TS validator from ESF) |
| Traceability | Added 7 new entries for new features and sections |

### New Feature Specifications

| Feature | Priority | File |
|---------|----------|------|
| FEAT-022 Agent Guardrails | P1 | `features/FEAT-022-agent-guardrails.md` |
| FEAT-023 Rollback/Recovery | P1 | `features/FEAT-023-rollback-recovery.md` |
| FEAT-024 Application Substrate | P2 | `features/FEAT-024-application-substrate.md` |
| FEAT-025 Control Plane | P2 | `features/FEAT-025-control-plane.md` |

### Updated Feature Specifications

| Feature | Changes |
|---------|---------|
| FEAT-017 Schema Evolution | Added zero-downtime guarantees, required field defaults, constraint tightening validation, per-entity schema version tracking |

## Tracker Issues Created

| Bead ID | Title | Priority | Labels |
|---------|-------|----------|--------|
| `axon-91bfabea` | FEAT-022: Agent Guardrails | P2 (P1) | feat, p1, agent-guardrails |
| `axon-3f15171a` | FEAT-023: Rollback and Recovery | P2 (P1) | feat, p1, rollback |
| `axon-61a657c7` | FEAT-024: Application Substrate | P3 (P2) | feat, p2, app-substrate |
| `axon-bd579636` | FEAT-025: Control Plane | P3 (P2) | feat, p2, control-plane |
| `axon-8c5c2442` | FEAT-017: Schema Evolution Refinements | P2 (P1) | feat, p1, schema-evolution |
| `axon-985d7cae` | Doc Evolution Tracking | P1 | chore, docs (CLOSED) |

## Conflict Resolutions (2026-04-06)

| # | Conflict | Decision | Action Taken |
|---|----------|----------|-------------|
| 1 | EAV: full storage model or index strategy? | **Index strategy only** — entity data stays as opaque JSON blobs | Clarified in Technical Requirements Section 2a and PRD executive summary |
| 2 | Semantic validation hooks in FEAT-022? | **Deferred** — design when scope constraints and rate limiting are proven | FEAT-022 hooks section marked deferred, acceptance criterion struck |
| 3 | Cloudflare Workers deployment? | **Deferred** — Cloud Run only for now | Removed Workers from FEAT-024 deployment templates |
| 4 | Control plane priority? | **Promoted to P1** — needed soon for multi-tenant ops | FEAT-025 promoted to P1, moved from P2 to P1 in PRD |
| 5 | GitHub org and licensing? | **Deferred** — decide before public release | No changes; noted in Vision as intent, not committed |

## Items Not Changed (Already Aligned)

The following items from the evolution prompt were already well-covered
in the existing artifact stack:

- Entity storage, schema validation, audit trail (PRD core)
- Graph relationships and link model (PRD Section 4)
- JSON API, GraphQL, MCP interfaces (FEAT-005, FEAT-015, FEAT-016)
- ACID transactions and OCC (PRD Section 5, ADR-004)
- Multi-tenancy and namespace hierarchy (FEAT-014, ADR-011)
- CDC/Change feeds with Debezium (FEAT-021, ADR-014)
- Admin UI (FEAT-011, ADR-006)
- SQLite and PostgreSQL backends (Technical Requirements Section 3)
- Schema evolution core mechanics (FEAT-017, ADR-007, ADR-008)

## Authority Chain

All changes follow the authority order:
Vision > PRD > Technical Requirements > Features > Tests > Code

No downstream artifacts (tests, code) were modified — those changes
will flow through the tracker issues created above.
