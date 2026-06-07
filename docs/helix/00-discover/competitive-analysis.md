---
ddx:
  id: helix.competitive-analysis
  depends_on:
    - helix.product-vision
    - helix.prd
---

# Competitive Analysis: Axon

**Version**: 0.2.0
**Date**: 2026-04-04
**Revised**: 2026-06-06
**Status**: Draft
**Author**: Erik LaBianca

## Market Landscape

| Attribute | Assessment |
|-----------|------------|
| Market Maturity | Emerging. The base technologies are mature, but "governed agent writes to business records" is a new category. |
| Growth Rate | Not quantified in this artifact. Treat market sizing as a follow-up research item. |
| Key Trends | Agents moving from read-only assistance to bounded business mutations; MCP becoming a common tool surface; teams assembling Postgres, GraphQL, policy, and audit stacks by hand; multi-model databases positioning around AI context. |
| Entry Barriers | Medium. Storage, GraphQL, policy engines, and audit logs are commoditized separately; the hard part is proving one coherent, safe write path across human and agent surfaces. |
| Buyer Power | High. Early buyers can keep assembling existing tools unless Axon demonstrates a much faster path to trusted agent writes. |

Axon competes less with one database than with a stack assembled to make a
general database safe enough for agents: PostgreSQL or SQLite, RLS, triggers,
Hasura or PostGraphile, OpenFGA/Oso/Cerbos, custom approval services, custom
MCP wrappers, and custom audit tables.

## Competitive Forces

| Force | Pressure | Evidence / Confidence | Implication |
|-------|----------|-----------------------|-------------|
| Direct Rivalry | Medium | SurrealDB now markets itself around multi-model data for AI agents; Gel/EdgeDB has a strong object/graph-relational model; DoltDB owns "Git for data." Confidence: medium-high, official positioning checked 2026-06-06. | Axon must avoid generic "multi-model database" positioning and lead with governed business-state writes. |
| Substitutes | High | The DIY Postgres stack can cover storage, GraphQL, policy, approval, and audit with enough engineering effort. Confidence: high, based on current ecosystem maturity and PRD alternatives. | The primary win condition is lower assembly cost and stronger safety guarantees than the assembled stack. |
| New Entrants | Medium | Agent infrastructure and context-layer products are moving quickly, and MCP lowers the cost of generating database tools. Confidence: medium. | Defensibility must come from policy/audit/approval semantics, not just generated tools. |
| Buyer Power | High | Developers can defer adoption, use Postgres/Supabase/Firebase, or wrap existing systems with MCP tools. Confidence: high. | Axon needs a reference workflow that proves trusted agent writes in less than one day. |

## Competitor Profiles

| Competitor | Type | Positioning | Target Segment | Strengths | Weaknesses vs Axon | Source / Confidence |
|------------|------|-------------|----------------|-----------|--------------------|---------------------|
| DIY Postgres stack: Postgres/SQLite + RLS + triggers + Hasura/PostGraphile + OpenFGA/Cerbos + custom approvals/audit/MCP | Substitute | Build a governed data layer from proven parts | Serious teams with platform engineering capacity | Mature storage, known operations, flexible authorization choices, fast GraphQL generation | Policy drift across layers; custom stale-preview/approval binding; audit often semantic-thin; MCP tools must be wrapped and governed manually | Ecosystem synthesis; high |
| Supabase / Firebase | Indirect | Fast app backend/BaaS | Web/mobile/internal-app teams optimizing for speed | Excellent onboarding, auth, realtime, managed services, broad adoption | Not focused on repair-grade audit, agent mutation intents, GraphQL/MCP parity, or reusable data-layer policy across arbitrary tools | Supabase docs; Firebase official site; high |
| PocketBase | Indirect | Single-file open-source backend with SQLite, auth, realtime, and admin UI | Solo developers, prototypes, small internal apps | Embeddable simplicity and low setup cost | Limited graph semantics, policy depth, repair-grade audit, and governed agent write flow | PocketBase GitHub; high |
| SurrealDB | Direct-adjacent | Native multi-model database, increasingly positioned for AI agents/context | Teams wanting one engine for document, graph, vector, time-series, and relational data | Strong multi-model story, graph/document support, SurrealQL, cloud/edge/on-prem positioning | Broad multi-model scope can dilute guarantees; audit/approval/repair-grade lineage is not the core product promise; Axon should not compete on vector/RAG breadth | SurrealDB docs/site; high |
| Gel / EdgeDB | Direct-adjacent | Object/graph-relational database with EdgeQL and rigorous type system | Developers wanting better relational modeling and query ergonomics | Strong schema/type model, links, PostgreSQL-backed reliability, powerful query language | Server-oriented, novel query language, no native agent write governance or repair-grade audit as core differentiator | Gel docs; high |
| DoltDB | Indirect | Version-controlled SQL database, "Git for data" | Teams needing data diff, branch, merge, and time travel | Strong data versioning story, MySQL compatibility, diff/time-travel workflow | Versioning is not equivalent to per-mutation policy/approval/tool lineage; no graph-first business model or MCP-safe write path | DoltHub; high |
| Hasura / PostGraphile | Substitute component | Instant GraphQL over existing databases | Teams exposing database-backed APIs quickly | Productive GraphQL API generation, authorization hooks/permissions, subscriptions | Does not own schema/policy/audit/approval as one invariant below GraphQL and MCP; still leaves MCP and repair history to the application | Hasura site/GitHub; high |
| OpenFGA / Cerbos / Oso | Substitute component | External authorization and policy decisioning | Teams standardizing authz across services | Strong RBAC/ReBAC/ABAC modeling, independent policy lifecycle | They decide access but do not own entity storage, mutation preview, audit lineage, or repair/change capture for business records | OpenFGA and Cerbos docs; high |
| KurrentDB / EventStoreDB | Indirect | Event-native database for event sourcing and CQRS | Event-sourced architectures needing immutable event streams | Excellent history-first model and streaming semantics | Event sourcing is a different application architecture; not a schema-first entity graph with generated GraphQL/MCP and data-layer policy | Kurrent docs; high |
| Neo4j / graph databases | Indirect | Native graph storage and traversal | Graph-heavy applications, recommendations, fraud, knowledge graphs | Mature graph querying and visualization | Graph traversal is strong, but schema enforcement, repair-grade audit, app/MCP policy parity, and embedded/onboarding story are not Axon's target bundle | Prior research + official category knowledge; medium |

**Indirect Competitors**: spreadsheets/Airtable/Notion for informal workflow
state; existing SaaS APIs with custom MCP wrappers; workflow engines such as
Temporal, Restate, Inngest, DBOS, and LangGraph that still need a governed
system of record.

## Feature Comparison

| Current PRD Capability | DIY Postgres Stack | BaaS: Supabase/Firebase | Multi-model DB: SurrealDB | Object/Graph DB: Gel | Data Versioning: DoltDB | Authz: OpenFGA/Cerbos | Event Store: KurrentDB | Axon Target |
|------------------------|--------------------|-------------------------|---------------------------|----------------------|-------------------------|----------------------|------------------------|-------------|
| Guardrailed human-agent writes | Partial/custom | Partial | Partial | Partial | Partial | Partial authz only | Partial via events | Full |
| Simple structured graph/tabular business data | Partial/custom | Partial | Full | Full | Partial | None | Partial via projections | Full |
| Reusable data-layer RBAC/ABAC/visibility controls below every surface | Partial/custom | Partial | Partial | Partial | Partial SQL grants | Full authz, no data layer | Partial | Full |
| Policy-safe generated MCP tools | Custom | None/Custom | Partial | None/Custom | None/Custom | None | None | Full |
| Mutation preview and approval bound to pre-image/policy/grant/operation hash | Custom | None/Custom | None/Custom | None/Custom | None | None | Custom | Full |
| Repair-grade audit with actor/tool/policy/approval/before-after lineage | Custom/Partial | Partial/None | Partial/None | None/Custom | Partial version history | None | Full events, different model | Full |
| GraphQL and MCP semantic parity | Custom | Partial | Partial | Partial | None | None | None | Full |
| Embedded and server modes behind one governed API | Partial | None | Partial | None | Partial | N/A | Server-oriented | Full |
| Time to first trusted agent write | Slow unless prebuilt | Fast app write, slower trusted agent write | Fast data modeling, custom governance | Fast modeling, custom governance | Fast versioned SQL, custom governance | Only authz component | Requires event-sourcing architecture | <1 day target |

**Legend**: Full = core product behavior; Partial = meaningful support but not
the complete PRD capability; Custom = achievable with application/platform work;
None = not a core offering; N/A = not the product's layer.

## Differentiation Strategy

| Differentiator | Why It Matters | Defensibility |
|----------------|----------------|---------------|
| One governed write path for humans and agents | Prevents silent data loss, stale approvals, and direct-write bypasses when agents and humans share records | High |
| Schema, policy, transaction, approval, and audit as one data-layer invariant | Low-effort apps and MCP servers inherit controls instead of reimplementing them | High |
| Repair-grade audit rather than raw CDC or version history | Operators can reconstruct intent and repair state when preventive guardrails fail | High |
| MCP-safe generated tools over governed data | Agents get discoverable tools without weakening authorization, redaction, approval, or audit | Medium-high |
| Entity/link model with graph-shaped and tabular reads | Business apps can model invoices, vendors, users, tasks, approvals, and dependencies naturally | Medium |
| Embedded-first developer experience with server-mode parity | Matches PocketBase/SQLite ease while preserving production path | Medium |

**Positioning**: For developers building agentic business applications that
mutate durable records, Axon is a governed transactional entity store that
makes structured business state safe for both humans and agents. Unlike a DIY
Postgres/GraphQL/authz/audit/MCP stack, Axon provides one schema, policy,
mutation-intent, and audit model below every application and tool surface.

## Strategic Implications

- **Attack**: Compete against assembled Postgres stacks for procurement,
  invoice, compliance, customer-operations, and internal workflow use cases
  where agent writes require approval, redaction, and repair-grade audit.
- **Attack**: Publish an invoice/procurement reference workflow that proves
  time to first trusted agent write in less than one day across GraphQL and MCP.
- **Defend**: Make cross-surface policy parity and stale intent rejection
  executable contract tests, not marketing claims.
- **Defend**: Keep embedded startup simple enough to compete with PocketBase
  while preserving the production path through PostgreSQL/server mode.
- **Avoid**: Do not compete as a generic BaaS, universal authorization service,
  all-purpose multi-model/vector database, analytics engine, or durable
  workflow orchestrator.
- **Avoid**: Do not make "MCP for databases" the headline; MCP is a surface,
  not the defensible category.

## Evidence Gaps and Follow-Up Research

- Quantify market demand: number of teams building agentic business workflows,
  willingness to adopt a new data layer, and expected budget owner.
- Run buyer interviews against the DIY stack: what do teams actually spend to
  implement preview, approval, policy parity, and repair-grade audit today?
- Validate the invoice/procurement wedge against at least three domains:
  procurement/AP, compliance/customer operations, and internal approval tools.
- Re-check competitor audit and agent-tool capabilities quarterly. SurrealDB,
  BaaS platforms, and authz vendors are moving fastest around AI positioning.
- Resolve naming/search risk with Axon Framework/AxonIQ before public launch.

## Sources

Primary sources checked on 2026-06-06:

- [Supabase features](https://supabase.com/docs/guides/getting-started/features)
- [Firebase official site](https://firebase.google.com/)
- [Hasura official site](https://hasura.io/)
- [OpenFGA official docs](https://openfga.dev/)
- [Cerbos policy docs](https://docs.cerbos.dev/cerbos/latest/policies/index.html)
- [SurrealDB documentation](https://surrealdb.com/docs/surrealdb)
- [Gel EdgeQL docs](https://docs.geldata.com/reference/edgeql)
- [DoltHub / DoltDB](https://www.dolthub.com/)
- [PocketBase GitHub](https://github.com/pocketbase/pocketbase)
- [KurrentDB ecosystem docs](https://docs.kurrent.io/getting-started/kurrent-ecosystem)
