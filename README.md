# Axon

Cloud-native, auditable, schema-first entity-graph-relational data store for agentic applications and business workflows.

> ⚠️ **Pre-release.** The wire protocol, data model, and SDK surface are not
> yet frozen. Breaking changes may land in any minor version until v1.0
> without a deprecation period. Production use is at the operator's own
> risk. Pin a specific version and track the CHANGELOG for API-impacting
> changes. See the [Product Vision — Stability Status](docs/helix/00-discover/product-vision.md#stability-status)
> for the full stability contract.

**Status**: Early design phase

## What is Axon?

Axon is the central nervous system for agentic applications. It provides a transactional data store where entities (deeply nested, schema-validated structures) are connected by typed links (directional relationships), with ACID transactions, immutable audit trails, and enforced schemas as guarantees.

Agents and humans share a trustworthy substrate for structured, interconnected state.

## Key Documents

| Document | Description |
|----------|-------------|
| [Product Vision](docs/helix/00-discover/product-vision.md) | Mission, vision, target market |
| [PRD](docs/helix/01-frame/prd.md) | Requirements, data model, transaction model, value propositions |
| [Principles](docs/helix/01-frame/principles.md) | Design principles with testable criteria |
| [Technical Requirements](docs/helix/01-frame/technical-requirements.md) | Stateless servers, multi-backend, data shape limits, correctness, ESF schema |
| [Competitive Analysis](docs/helix/01-frame/competitive-analysis.md) | 18-system comparison across graph, relational, NoSQL, and hybrid databases |
| [Feature Specs](docs/helix/01-frame/features/) | FEAT-001 through FEAT-010 |
| [Use Case Research](docs/helix/00-discover/use-case-research.md) | 10 domains: CRM, CDP, AP/AR, ERP, Issue Tracking, and more |
| [Schema Format Research](docs/helix/00-discover/schema-format-research.md) | 19 schema formats evaluated (JSON Schema, OWL, SHACL, EdgeDB SDL, etc.) |
| [FoundationDB DST Research](docs/helix/00-discover/foundationdb-dst-research.md) | Deterministic simulation testing approach for correctness |
| [ADR-001: Rust](docs/helix/02-design/adr/ADR-001-implementation-language.md) | Implementation language decision |
| [ADR-002: Schema Format](docs/helix/02-design/adr/ADR-002-schema-format.md) | Hybrid JSON Schema + Axon vocabulary |
| [SPIKE-001: Backing Stores](docs/helix/02-design/spikes/SPIKE-001-backing-store-evaluation.md) | PostgreSQL, SQLite, FoundationDB, fjall evaluation |
| [Test Plan](docs/helix/03-test/test-plan.md) | Correctness invariants, business scenarios, property tests, benchmarks |

## Related Projects

- [DDx](https://github.com/DocumentDrivenDX/ddx) — Document-driven development infrastructure (CLI, server, library)
- [HELIX](https://github.com/DocumentDrivenDX/helix) — Development workflow framework used to build Axon
- [tablespec](https://github.com/DocumentDrivenDX/tablespec) — Universal Metadata Format (UMF) for table schemas
- [dun](https://github.com/DocumentDrivenDX/dun) — Document dependency tracking
