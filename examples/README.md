# Axon Sample Projects

Generated from HELIX sources for Axon 0.7.1. These projects are the
sample-project side of the website coverage catalog.

| Project | Purpose | Demo reel | Coverage entries |
|---|---|---|---:|
| `agent-taskboard` | A governed task and bead queue that demonstrates collections, schemas, entity CRUD, links, graph traversal, audit, optimistic concurrency, MCP-oriented task discovery, and the unified CLI. | `agent-taskboard-reel` | 33 |
| `invoice-approval-guardrails` | A finance workflow covering AP/AR partial payment, state machines, policy envelopes, mutation intent preview/approval, rollback, and audit-safe trusted agent writes. | `finance-guardrails-reel` | 39 |
| `customer-identity-graph` | CRM, CDP, and MDM flows for contact merge, identity resolution, golden record survivorship, relationship traversal, and provenance. | `customer-identity-reel` | 11 |
| `supply-chain-bom` | ERP-style bill-of-materials traversal, recursive graph queries, reachability checks, aggregation, and link metadata. | `supply-chain-reel` | 19 |
| `tenant-control-plane` | Multi-tenant path routing, users, credentials, JWT grant bounds, revocation, deployment registration, and BYOC isolation. | `tenant-control-reel` | 26 |
| `schema-release-sync` | Schema evolution, secondary indexes, validation rules, CDC, markdown templates, git mirror output, and rollback preview. | `schema-release-reel` | 42 |
| `admin-policy-workbench` | Policy authoring, dry-runs, redacted browsing, intent queues, approval review, stale-intent handling, and UI parity with GraphQL/MCP. | `admin-policy-reel` | 28 |

Run a project demo with:

```bash
cd examples/<project>
bash demo.sh
```
