---
ddx:
  id: helix.product-vision
---

# Product Vision: Axon

## Mission Statement

Axon is a governed transactional entity store for developers building agentic
business applications. It lets humans and agents safely share durable business
records through structured data, reusable policy, mutation guardrails, and
repairable audit history.

## Positioning

For developers building agents and internal workflow systems that mutate
business records, Axon is a governed OLTP data layer that combines
entity/graph modeling, policy enforcement, mutation review, and audit/change
capture behind GraphQL and MCP.

Unlike raw Postgres or SQLite assembled with RLS, triggers, Hasura or
PostGraphile, OpenFGA or Cerbos, custom audit tables, and ad-hoc MCP wrappers,
Axon makes the governed data layer reusable. Low-effort apps and generated MCP
servers inherit the same schema validation, visibility controls, approval
routing, stale-write protection, and audit lineage instead of rebuilding those
controls for every surface.

## Vision

Every agentic application that changes business records has one trusted place
where structured state is modeled, queried, guarded, approved, audited, and
repaired. Humans can delegate bounded work to agents because the data layer
prevents stale writes, explains policy decisions, routes risky changes for
review, and preserves enough history to recover when preventive controls are
not enough.

**North Star**: A developer can ship an agent that performs a real governed business write through Axon in less than one day, with no custom policy, approval, or audit plumbing.

## User Experience

A developer defines `Invoice`, `Vendor`, and `User` collections with typed
links, validation rules, indexes, lifecycle states, and access policies. An
agent discovers generated MCP tools and sees readable fields, autonomous
writes, and approval-routed writes. It proposes an invoice update; Axon returns
a diff, policy explanation, affected records, and a mutation token bound to
the reviewed pre-image.

If the update is low risk, the agent commits it and Axon records the audit
trail. If it needs approval, a finance user reviews the same intent through
GraphQL or the admin UI. At commit time Axon rechecks entity versions, schema
version, policy version, grant version, and operation hash. Later, an operator
can query who or what changed the invoice, why the policy allowed it, which
tool originated it, and what state would be needed to repair or roll it back.

## Target Market

| Attribute | Description |
|-----------|-------------|
| Who | Developers building AI agents and internal workflow systems for procurement, invoicing, compliance, customer operations, document review, and ERP-like business processes |
| Pain | Business state is scattered across databases, spreadsheets, SaaS tools, file stores, and tool-specific APIs with inconsistent schemas, policy, approvals, audit, and rollback paths |
| Current Solution | Postgres or SQLite plus RLS/triggers, Hasura/PostGraphile, OpenFGA/Cerbos/Oso, custom approval services, custom audit tables, Firebase/Supabase, Airtable, Notion, and ad-hoc MCP servers |
| Why They Switch | Agents are now capable enough to mutate real records, but the assembled stack does not give teams confidence that agent and human writes are safe, explainable, reversible, and policy-consistent |

## Key Value Propositions

| Value Proposition | Customer Benefit |
|-------------------|------------------|
| Guardrailed human-agent collaboration | Agents and humans can work on the same durable records without silent overwrites, stale approvals, or unclear authority boundaries |
| Simple structured business data | Developers model entities, typed links, and collections once, then query them in graph-shaped or tabular form without assembling separate document, graph, relational, and indexing systems |
| Reusable access and visibility control | RBAC, ABAC, relationship-aware, field-level, and transition policies are declared once and enforced below GraphQL, MCP, CLI, SDK, and compatibility surfaces |
| Audit, change capture, and repair history | Every mutation carries enough actor, tool, policy, approval, version, and before/after context to investigate failures and repair affected state |
| Agent-native and app-friendly APIs | MCP gives agents a natural tool surface while GraphQL gives applications, operators, and SDKs the same semantics |
| Governed local-first and embedded operation | Teams run Axon embedded in an app or offline on a device as a first-class deployment, keeping the same schema, policy, approval, and audit guarantees when state syncs back |

## Success Definition

| Metric | Target |
|--------|--------|
| Time to first trusted agent write | A competent developer completes the invoice/procurement reference workflow through GraphQL and MCP in less than 1 day |
| Policy reuse confidence | The same subject, resource, and operation produces identical allow, deny, redaction, and approval decisions across handler API, GraphQL, MCP, CLI, and SDK paths |
| Audit completeness | 100% of entity and link mutations produce repair-grade audit records with actor, authority, tool/API origin, policy decision, approval decision, versions, and before/after state |
| Early adoption | 10+ external projects or serious internal integrations use Axon for governed business state within 12 months |

## Why Now

AI agents have crossed from read-only assistants into systems that can draft,
patch, reconcile, and submit bounded business changes. Teams are already
connecting them to databases and SaaS APIs, but safety work is rebuilt app by
app. The missing layer is a reusable governed state layer that makes structured
business data safe for agent and human collaboration before this pattern
hardens into fragile bespoke stacks.

## Review Checklist

- [x] Mission statement is specific - names the user, the problem, and the approach
- [x] Positioning statement differentiates from the current alternative
- [x] Vision describes a desired end state, not a feature list
- [x] North star is a single measurable sentence
- [x] User experience section describes a concrete scenario, not abstract benefits
- [x] Target market identifies specific pain points and switching triggers
- [x] Value propositions map to customer benefits, not internal capabilities
- [x] Success metrics are measurable and time-bound
- [x] Why Now section names a specific change, not a vague opportunity
- [x] Business case details, competitor matrices, requirements, and technical choices are left to their own artifacts
- [x] No implementation details (technology choices, architecture) - those belong in design
