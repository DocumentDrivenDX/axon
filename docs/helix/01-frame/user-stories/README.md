# User-Story ID Registry — Authoritative Allocation Ledger

**Status**: Authoritative. This registry resolves every duplicate `US-<n>` ID across the
31 feature specs under `docs/helix/01-frame/features/` and prescribes the convention for
extracting stories into standalone files in this directory. Agents extracting stories
from feature specs MUST use the IDs in the [Allocation Ledger](#allocation-ledger) below,
NOT the IDs currently printed in feature-spec headings, wherever the two disagree (see
the [Renumber Map](#renumber-map)).

Survey basis (2026-06-10): 138 story headings across 31 specs; 113 unique IDs; global
maximum `US-119`; 19 IDs defined as different stories in 2+ specs, plus 2 IDs (`US-079`,
`US-080`) whose spec headings (FEAT-027) conflict with live code/test coverage tags that
claim them for FEAT-003/FEAT-004 stories.

---

## Story-File Convention

Derived from the catalog artifact at
`.ddx/plugins/helix/workflows/activities/01-frame/artifacts/user-stories/{template.md,prompt.md,meta.yml}`.

### File path pattern

One file per story (never a monolithic `user-stories.md` — reconcile-alignment flags it):

```
docs/helix/01-frame/user-stories/US-NNN-<slug>.md
```

`meta.yml` (`output:`): `location: docs/helix/01-frame/user-stories/`, `format: markdown`,
`naming: US-{number}-{slug}.md`, `structure: one-per-story`. ID format is `US-NNN`,
project-scoped (`tracking.id_format: US-NNN`, `id_scope: project`). Use the FINAL ID from
the ledger below, zero-padded to 3 digits, and a kebab-case slug of the story title
(e.g. `US-001-create-a-collection.md`, `US-120-prov-o-audit-shape.md`).

### Required frontmatter

Per `template.md`, exactly:

```yaml
---
ddx:
  id: US-NNN
---
```

followed by the H1: `# US-NNN: <Story Title>` and the header block:

```
**Feature**: FEAT-XXX — <Feature Name>
**Feature Requirements**: REQ-01, REQ-02
**PRD Requirements**: FR-n
**Priority**: P0 | P1 | P2
**Status**: Draft | Review | Approved
```

### Required sections

`meta.yml` `validation.required_sections` (template order, with template's two extras):

1. `## Story` — `**As a**` (specific PRD persona) / `**I want**` (user action) /
   `**So that**` (measurable outcome). All three bold markers are checked by
   automated patterns in `meta.yml`.
2. `## Context` — 2–4 sentences; why the story exists, which parent feature
   requirements it exercises.
3. `## Walkthrough` — numbered happy-path journey, present tense, trigger → outcome.
4. `## Acceptance Criteria` — see AC ID convention below.
5. `## Edge Cases` — at least one `**Condition**: expected behavior` entry.
6. `## Test Scenarios` — table `| Scenario | AC ID | Input / State | Action | Expected Result |`
   with concrete values, covering the happy path and ≥1 edge case.
7. `## Dependencies` — `**Stories**`, `**Feature Spec**`, `**Feature Requirements**`,
   `**PRD Requirements**`, `**External**` (Contract IDs, not inline surfaces).
8. `## Out of Scope` (template-required) and `## Review Checklist` (template-required).

Blocking quality rules (`meta.yml` / `prompt.md`): persona must come from the PRD;
every AC independently testable (one Given/When/Then each, no compound criteria);
no exact API/CLI/event/schema/config/telemetry/adapter surfaces inline — reference
Contract IDs; every PRD `FR-n` must map to ≥1 story.

### Acceptance-criterion ID convention

From `prompt.md` ("Each criterion carries a **stable AC ID** of the form `US-<n>-AC<m>`")
and `template.md`:

```markdown
- [ ] **US-NNN-AC1** — Given <specific precondition>, when <specific action>, then <observable outcome>
- [ ] **US-NNN-AC2** — Given <...>, when <...>, then <...>
```

- `<n>` is the story's FINAL registry number; `<m>` is 1-based and never reused or
  renumbered once published — IDs are stable across edits.
- Covering tests cite the AC ID with a coverage tag:

```
@covers US-NNN-ACm
```

  (e.g. in Rust: `// @covers US-120-AC2`; in Playwright titles: `... @covers US-120-AC2`,
  alongside the existing `@US-NNN` story-level tags). The story test plan (STP) owns the
  AC→test matrix; do not duplicate it in the story file.

### Extraction rule for renumbered stories

When extracting a story whose spec heading carries a retired ID (see Renumber Map), the
new story file uses the NEW ID everywhere (filename, `ddx.id`, H1, AC IDs) and includes a
line in `## Context`: `Renumbered from US-0XX (collision with <keeper FEAT>).`
Do NOT edit feature specs or tests as part of extraction; spec-heading updates are a
separate, later change.

---

## Collision Resolution Rules (as applied)

(a) The claimant whose story is referenced by existing test/code coverage tags
(`crates/`, `ui/tests/`) or by FEAT-031's Story Coverage Map keeps the ID.
(b) If neither or both are referenced, the lower-numbered FEAT keeps it.
(c) The losing claimant receives a fresh ID, allocated sequentially from `US-120`
(above the global max `US-119`). Retired pairings are never reused.

Special cases:

- **US-079 / US-080**: only FEAT-027 defines spec headings for these, but live code and
  test tags claim US-079 for the FEAT-003 multi-collection audit tail
  (`crates/axon-server/src/gateway.rs:1975`, `crates/axon-api/src/request.rs:225`) and
  US-080 for the FEAT-004 point-in-time snapshot (`crates/axon-server/tests/snapshot_test.rs:2`,
  `crates/axon-api/src/handler.rs:22574`, `crates/axon-server/src/gateway.rs:1588`).
  Rule (a) applies: the code-claimed stories keep the IDs; FEAT-027's headings are
  renumbered. Follow-up: FEAT-003/FEAT-004 need story headings added for US-079/US-080
  (tracked outside this registry; do not edit specs during extraction).
- **US-074b** (FEAT-019 "Query by Gate Status", `FEAT-019-validation-rules.md:615`): a
  nonconforming legacy ID, referenced by tests (`crates/axon-api/src/handler.rs:20040`).
  It KEEPS the literal ID `US-074b` so coverage tags keep working; treat it as distinct
  from `US-074`. Its extracted file is `US-074b-query-by-gate-status.md` with AC IDs
  `US-074b-AC<m>`. Do not allocate any other suffixed ID.

---

## Allocation Ledger

Every story, listed under its FINAL ID. "Defined at" is the current spec heading.
140 rows = 138 spec headings + 2 code-claimed stories lacking headings (US-079, US-080).

| US ID | Owner FEAT | Title | Defined at | Disposition |
|-------|-----------|-------|-----------|-------------|
| US-001 | FEAT-001 | Create a Collection | `FEAT-001-collections.md:48` | keep |
| US-002 | FEAT-001 | List and Inspect Collections | `FEAT-001-collections.md:61` | keep |
| US-003 | FEAT-001 | Drop a Collection | `FEAT-001-collections.md:73` | keep |
| US-004 | FEAT-002 | Define a Collection Schema | `FEAT-002-schema-engine.md:57` | keep |
| US-005 | FEAT-002 | Get Clear Validation Errors | `FEAT-002-schema-engine.md:70` | keep |
| US-006 | FEAT-002 | Inspect a Schema | `FEAT-002-schema-engine.md:82` | keep |
| US-007 | FEAT-003 | Query the Audit Trail | `FEAT-003-audit-log.md:58` | keep |
| US-008 | FEAT-003 | Revert an Entity to Previous State | `FEAT-003-audit-log.md:71` | keep |
| US-009 | FEAT-003 | Attach Metadata to Mutations | `FEAT-003-audit-log.md:83` | keep |
| US-010 | FEAT-004 | CRUD an Entity | `FEAT-004-entity-operations.md:62` | keep |
| US-011 | FEAT-004 | Query Entities | `FEAT-004-entity-operations.md:75` | keep |
| US-012 | FEAT-004 | Partial Update | `FEAT-004-entity-operations.md:89` | keep |
| US-013 | FEAT-005 | Use Axon from an Agent | `FEAT-005-api-surface.md:106` | keep |
| US-014 | FEAT-005 | Use Axon from the Command Line | `FEAT-005-api-surface.md:122` | keep |
| US-015 | FEAT-006 | Store and Query Beads | `FEAT-006-bead-storage-adapter.md:44` | keep |
| US-016 | FEAT-006 | Track Bead Dependencies | `FEAT-006-bead-storage-adapter.md:59` | keep |
| US-017 | FEAT-007 | Model Entities with Nested Structure | `FEAT-007-entity-graph-model.md:77` | keep |
| US-018 | FEAT-007 | Create and Traverse Links | `FEAT-007-entity-graph-model.md:91` | keep |
| US-019 | FEAT-007 | Query Across Entity-Link Graph | `FEAT-007-entity-graph-model.md:105` | keep |
| US-020 | FEAT-008 | Atomic Multi-Entity Update | `FEAT-008-acid-transactions.md:66` | keep |
| US-021 | FEAT-008 | Concurrent Agent Safety | `FEAT-008-acid-transactions.md:78` | keep |
| US-022 | FEAT-008 | Snapshot Isolation | `FEAT-008-acid-transactions.md:90` | keep |
| US-023 | FEAT-009 | Traverse a Dependency Graph | `FEAT-009-unified-graph-query.md:169` | keep |
| US-024 | FEAT-009 | Explode a Bill of Materials | `FEAT-009-unified-graph-query.md:182` | keep |
| US-025 | FEAT-009 | Check Reachability | `FEAT-009-unified-graph-query.md:195` | keep |
| US-026 | FEAT-010 | Enforce Invoice Approval Workflow | `FEAT-010-entity-state-machines.md` (story extracted to `US-026-enforce-invoice-approval-workflow.md`) | keep |
| US-027 | FEAT-010 | Bead Lifecycle with Dependency Guards | `FEAT-010-entity-state-machines.md` (story extracted to `US-027-bead-lifecycle-with-dependency-guards.md`) | keep |
| US-028 | FEAT-010 | Query Valid Transitions | `FEAT-010-entity-state-machines.md` (story extracted to `US-028-query-valid-transitions.md`) | keep |
| US-031 | FEAT-013 | Declare a Secondary Index | `FEAT-013-secondary-indexes.md:123` | keep |
| US-032 | FEAT-013 | Enforce Uniqueness via Index | `FEAT-013-secondary-indexes.md:137` | keep |
| US-033 | FEAT-013 | Compound Index for Multi-Field Queries | `FEAT-013-secondary-indexes.md:151` | keep |
| US-034 | FEAT-013 | Background Index Build | `FEAT-013-secondary-indexes.md:164` | keep |
| US-035 | FEAT-014 | Create and Use a Database (within a tenant) | `FEAT-014-multi-tenancy.md:350` | keep |
| US-036 | FEAT-014 | Organize Collections with Schemas | `FEAT-014-multi-tenancy.md:369` | keep |
| US-037 | FEAT-014 | Zero-Config Default Tenant for Dev Mode | `FEAT-014-multi-tenancy.md:383` | keep |
| US-038 | FEAT-014 | Scope Access to a Specific Database via Tenant Membership | `FEAT-014-multi-tenancy.md:404` | keep |
| US-039 | FEAT-014 | Register Nodes and Track Placement | `FEAT-014-multi-tenancy.md:420` | keep |
| US-040 | FEAT-011 | Navigate the Tenant and Database Model | `FEAT-011-admin-web-ui.md:135` | keep |
| US-041 | FEAT-011 | Administer Users, Members, and Credentials | `FEAT-011-admin-web-ui.md:148` | keep |
| US-042 | FEAT-011 | Manage Collections and Entities | `FEAT-011-admin-web-ui.md:161` | keep |
| US-043 | FEAT-012 | Authenticate via Tailscale | `FEAT-012-authorization.md:373` | keep |
| US-044 | FEAT-012 | Role-Based Access Control | `FEAT-012-authorization.md:392` | keep |
| US-045 | FEAT-011 | Use Advanced Database Tools | `FEAT-011-admin-web-ui.md:202` | keep |
| US-046 | FEAT-029 | Field-Level Masking | `FEAT-029-access-control.md` (story extracted to `US-046-field-level-masking.md`) | keep (ownership moved FEAT-012 → FEAT-029 during extraction) |
| US-047 | FEAT-029 | Attribute-Based Write Control | `FEAT-029-access-control.md` (story extracted to `US-047-attribute-based-write-control.md`) | keep (ownership moved FEAT-012 → FEAT-029 during extraction) |
| US-048 | FEAT-015 | Query Entities with Relationships | `FEAT-015-graphql-query-layer.md:200` | keep |
| US-049 | FEAT-015 | Discover the API via Introspection | `FEAT-015-graphql-query-layer.md:214` | keep |
| US-050 | FEAT-015 | Subscribe to Entity Changes | `FEAT-015-graphql-query-layer.md:230` | keep |
| US-051 | FEAT-015 | Use GraphQL from the Admin UI | `FEAT-015-graphql-query-layer.md:300` | keep |
| US-052 | FEAT-016 | Agent Discovers Axon via MCP | `FEAT-016-mcp-server.md:152` | keep |
| US-053 | FEAT-016 | Agent CRUDs Entities via MCP | `FEAT-016-mcp-server.md:171` | keep |
| US-054 | FEAT-016 | Agent Queries via GraphQL through MCP | `FEAT-016-mcp-server.md:189` | keep |
| US-055 | FEAT-016 | Agent Subscribes to Changes via MCP | `FEAT-016-mcp-server.md:203` | keep |
| US-056 | FEAT-016 | Local Agent Connects via Stdio | `FEAT-016-mcp-server.md:220` | keep |
| US-057 | FEAT-015 | Mutate Entities via GraphQL | `FEAT-015-graphql-query-layer.md:247` | keep |
| US-058 | FEAT-017 | Detect Breaking Schema Changes | `FEAT-017-schema-evolution.md:119` | keep |
| US-059 | FEAT-017 | Force-Apply a Breaking Change | `FEAT-017-schema-evolution.md:135` | keep |
| US-060 | FEAT-017 | Revalidate Entities Against Current Schema | `FEAT-017-schema-evolution.md:147` | keep |
| US-061 | FEAT-017 | View Schema Diff | `FEAT-017-schema-evolution.md:160` | keep |
| US-062 | FEAT-018 | Count Entities by Field | `FEAT-018-aggregation-queries.md:122` | keep |
| US-063 | FEAT-018 | Compute Numeric Aggregations | `FEAT-018-aggregation-queries.md:135` | keep |
| US-064 | FEAT-018 | Aggregate via GraphQL | `FEAT-018-aggregation-queries.md:149` | keep |
| US-065 | FEAT-018 | Aggregate via MCP | `FEAT-018-aggregation-queries.md:161` | keep |
| US-066 | FEAT-019 | Cross-Field Validation Rules | `FEAT-019-validation-rules.md:583` | keep |
| US-067 | FEAT-019 | Validation Gates | `FEAT-019-validation-rules.md:597` | keep |
| US-068 | FEAT-019 | Actionable Error Messages | `FEAT-019-validation-rules.md:630` | keep |
| US-069 | FEAT-019 | Validate Rules on Schema Save | `FEAT-019-validation-rules.md:645` | keep |
| US-070 | FEAT-009 | Find Link Targets | `FEAT-009-unified-graph-query.md:206` | keep |
| US-071 | FEAT-009 | List Entity Neighbors | `FEAT-009-unified-graph-query.md:219` | keep |
| US-072 | FEAT-009 | Explore Graph via GraphQL | `FEAT-009-unified-graph-query.md:231` | keep |
| US-073 | FEAT-009 | Discover Links via MCP | `FEAT-009-unified-graph-query.md:243` | keep |
| US-074 | FEAT-009 | Pattern query for ready/blocked queue | `FEAT-009-unified-graph-query.md:255` | keep |
| US-074b | FEAT-019 | Query by Gate Status | `FEAT-019-validation-rules.md:615` | keep (legacy suffixed ID; see special cases) |
| US-075 | FEAT-009 | Schema-declared named query | `FEAT-009-unified-graph-query.md:269` | keep |
| US-076 | FEAT-009 | Ad-hoc Cypher query | `FEAT-009-unified-graph-query.md:283` | keep |
| US-077 | FEAT-009 | Subscribe to a named query | `FEAT-009-unified-graph-query.md:296` | keep |
| US-078 | FEAT-015 | JSON-LD Content Negotiation | `FEAT-015-graphql-query-layer.md:319` | keep |
| US-079 | FEAT-003 | Multi-collection audit tail (code-claimed; no spec story heading yet) | _(no heading)_ | keep (spec heading missing — follow-up) |
| US-080 | FEAT-004 | Consistent point-in-time snapshot (code-claimed; no spec story heading yet) | _(no heading)_ | keep (spec heading missing — follow-up) |
| US-081 | FEAT-008 | Idempotent Transaction Submission | `FEAT-008-acid-transactions.md:103` | keep |
| US-087 | FEAT-014 | Create a Tenant with Multiple Databases | `FEAT-014-multi-tenancy.md:309` | keep |
| US-088 | FEAT-014 | Users Are Members of Multiple Tenants | `FEAT-014-multi-tenancy.md:330` | keep |
| US-089 | FEAT-012 | First-Class User with Tailscale Auto-Provisioning | `FEAT-012-authorization.md:645` | keep |
| US-090 | FEAT-012 | JWT Credential for Integration Access | `FEAT-012-authorization.md:667` | keep |
| US-091 | FEAT-012 | User in Multiple Tenants | `FEAT-012-authorization.md:692` | keep |
| US-092 | FEAT-022 | Keep Agent Writes Inside Assigned Scope | `FEAT-022-agent-guardrails.md:100` | keep |
| US-093 | FEAT-022 | Throttle Agent Mutation Bursts | `FEAT-022-agent-guardrails.md:127` | keep |
| US-094 | FEAT-022 | Configure Guardrails Per Agent Identity | `FEAT-022-agent-guardrails.md:150` | keep |
| US-095 | FEAT-023 | Preview Recovery Before Commit | `FEAT-023-rollback-recovery.md:132` | keep |
| US-096 | FEAT-023 | Revert One Entity Safely | `FEAT-023-rollback-recovery.md:157` | keep |
| US-097 | FEAT-023 | Undo a Bad Transaction or Time Window | `FEAT-023-rollback-recovery.md:181` | keep |
| US-098 | FEAT-024 | Generate a Typed Client from Schema | `FEAT-024-application-substrate.md:87` | keep |
| US-099 | FEAT-024 | Generate a Schema-Driven Admin App | `FEAT-024-application-substrate.md:112` | keep |
| US-100 | FEAT-024 | Deploy a Schema-Backed App with One Command | `FEAT-024-application-substrate.md:134` | keep |
| US-101 | FEAT-029 | Hide Inaccessible Entities | `FEAT-029-access-control.md:625` | keep |
| US-102 | FEAT-029 | Redact Sensitive Fields | `FEAT-029-access-control.md:638` | keep |
| US-103 | FEAT-029 | Reject Denied Writes | `FEAT-029-access-control.md:650` | keep |
| US-104 | FEAT-029 | Explain Effective Policy | `FEAT-029-access-control.md:663` | keep |
| US-105 | FEAT-030 | Preview A GraphQL Mutation | `FEAT-030-mutation-intents-approval.md:183` | keep |
| US-106 | FEAT-030 | Route Risky Writes For Approval | `FEAT-030-mutation-intents-approval.md:202` | keep |
| US-107 | FEAT-030 | Prevent Stale Approval Execution | `FEAT-030-mutation-intents-approval.md:220` | keep |
| US-108 | FEAT-030 | Use Mutation Intents From MCP | `FEAT-030-mutation-intents-approval.md:237` | keep |
| US-109 | FEAT-029 | Author And Test Policy Before Activation | `FEAT-029-access-control.md:678` | keep |
| US-110 | FEAT-015 | Enforce Policy Across GraphQL Traversal | `FEAT-015-graphql-query-layer.md:268` | keep |
| US-111 | FEAT-015 | Preview And Commit Mutation Intents | `FEAT-015-graphql-query-layer.md:284` | keep |
| US-112 | FEAT-016 | Agent Discovers Policy Envelopes | `FEAT-016-mcp-server.md:233` | keep |
| US-113 | FEAT-031 | Inspect Effective Policy In The Web UI | `FEAT-031-policy-intents-admin-ui.md:154` | keep |
| US-114 | FEAT-031 | Author And Dry-Run Policies Before Activation | `FEAT-031-policy-intents-admin-ui.md:174` | keep |
| US-115 | FEAT-031 | Browse Entities With Policy-Safe UI Semantics | `FEAT-031-policy-intents-admin-ui.md:192` | keep |
| US-116 | FEAT-031 | Preview And Commit Mutation Intents From The Web UI | `FEAT-031-policy-intents-admin-ui.md:211` | keep |
| US-117 | FEAT-031 | Review, Approve, And Reject Pending Intents | `FEAT-031-policy-intents-admin-ui.md:229` | keep |
| US-118 | FEAT-031 | Handle Stale And Mismatched Intents Safely | `FEAT-031-policy-intents-admin-ui.md:250` | keep |
| US-119 | FEAT-031 | Inspect MCP-Originated Policy And Intent Outcomes | `FEAT-031-policy-intents-admin-ui.md:268` | keep |
| US-120 | FEAT-003 | PROV-O Audit Shape | `FEAT-003-audit-log.md:95` | renumbered-from-US-010 |
| US-121 | FEAT-011 | Manage Schemas Visually | `FEAT-011-admin-web-ui.md:175` | renumbered-from-US-043 |
| US-122 | FEAT-011 | Inspect Audit and Recover Entity State | `FEAT-011-admin-web-ui.md:187` | renumbered-from-US-044 |
| US-123 | FEAT-012 | Development Without Auth | `FEAT-012-authorization.md:416` | renumbered-from-US-045 |
| US-124 | FEAT-012 | Per-Principal Role Assignment | `FEAT-012-authorization.md:604` | renumbered-from-US-048 |
| US-125 | FEAT-017 | Lazy-Read Schema Migration | `FEAT-017-schema-evolution.md:172` | renumbered-from-US-062 |
| US-126 | FEAT-028 | Start Axon from a single binary | `FEAT-028-unified-binary.md:193` | renumbered-from-US-070 |
| US-127 | FEAT-028 | Diagnose Axon installation | `FEAT-028-unified-binary.md:206` | renumbered-from-US-071 |
| US-128 | FEAT-028 | Install Axon as a system service | `FEAT-028-unified-binary.md:218` | renumbered-from-US-072 |
| US-129 | FEAT-028 | Use CLI against a running server | `FEAT-028-unified-binary.md:232` | renumbered-from-US-073 |
| US-130 | FEAT-021 | Emit CDC Events to Kafka | `FEAT-021-change-feeds-cdc.md:133` | renumbered-from-US-074 |
| US-131 | FEAT-028 | Install Axon with a single command | `FEAT-028-unified-binary.md:246` | renumbered-from-US-074 |
| US-132 | FEAT-021 | Replay Events from a Point in Time | `FEAT-021-change-feeds-cdc.md:148` | renumbered-from-US-075 |
| US-133 | FEAT-026 | Define a Markdown Template | `FEAT-026-markdown-templates.md:300` | renumbered-from-US-075 |
| US-134 | FEAT-028 | Configure Axon persistently | `FEAT-028-unified-binary.md:258` | renumbered-from-US-075 |
| US-135 | FEAT-021 | Discover Entity Schemas via Registry | `FEAT-021-change-feeds-cdc.md:164` | renumbered-from-US-076 |
| US-136 | FEAT-026 | Render an Entity as Markdown | `FEAT-026-markdown-templates.md:324` | renumbered-from-US-076 |
| US-137 | FEAT-021 | Stream Changes Without Kafka | `FEAT-021-change-feeds-cdc.md:178` | renumbered-from-US-077 |
| US-138 | FEAT-026 | Template Survives Schema Evolution | `FEAT-026-markdown-templates.md:352` | renumbered-from-US-077 |
| US-139 | FEAT-021 | Link Events in CDC | `FEAT-021-change-feeds-cdc.md:192` | renumbered-from-US-078 |
| US-140 | FEAT-027 | Enable Git Mirror on a Collection | `FEAT-027-git-mirror.md:388` | renumbered-from-US-078 |
| US-141 | FEAT-027 | Entity Changes Appear as Git Commits | `FEAT-027-git-mirror.md:405` | renumbered-from-US-079 |
| US-142 | FEAT-027 | Shard Strategy Organises the Repository | `FEAT-027-git-mirror.md:424` | renumbered-from-US-080 |
| US-143 | FEAT-027 | Mirror Resumes After Failure | `FEAT-027-git-mirror.md:440` | renumbered-from-US-081 |
| US-144 | FEAT-025 | Provision and Register a BYOC Deployment | `FEAT-025-control-plane.md:133` | renumbered-from-US-101 |
| US-145 | FEAT-025 | Observe a Fleet Without Reading Tenant Data | `FEAT-025-control-plane.md:157` | renumbered-from-US-102 |
| US-146 | FEAT-025 | Deprovision with Retention Guarantees | `FEAT-025-control-plane.md:180` | renumbered-from-US-103 |

Notes:

- FEAT-020 (`FEAT-020-link-discovery-and-graph-queries.md`) defines no stories of its
  own; its mapping table assigns US-070..US-073 to FEAT-009 (consistent with this ledger).
- Existing UI test tags `@US-113`..`@US-119` (FEAT-031) and all `crates/` US-ID comments
  resolve to the same stories under this ledger; no test or code tag changes are required.

## Renumber Map

Only rows that change. "In file" is the spec heading carrying the retired pairing.

| Old ID | In file | New ID | Reason |
|--------|---------|--------|--------|
| US-010 | `FEAT-003-audit-log.md:95` | US-120 | US-010 kept by FEAT-004 "CRUD an Entity" — referenced by code/test tag `crates/axon-server/src/service.rs:1208` (rule a) |
| US-043 | `FEAT-011-admin-web-ui.md:175` | US-121 | US-043 kept by FEAT-012 "Authenticate via Tailscale" — test tag `crates/axon-server/src/auth.rs:748` (rule a) |
| US-044 | `FEAT-011-admin-web-ui.md:187` | US-122 | US-044 kept by FEAT-012 "Role-Based Access Control" — test tag `crates/axon-server/src/auth.rs:958` (rule a) |
| US-045 | `FEAT-012-authorization.md:416` | US-123 | US-045 kept by FEAT-011 "Use Advanced Database Tools" — no test/coverage-map refs on either side; lower FEAT wins (rule b) |
| US-048 | `FEAT-012-authorization.md:604` | US-124 | US-048 kept by FEAT-015 "Query Entities with Relationships" — FEAT-031 coverage map line 293 + `crates/axon-graphql/src/lib.rs:8` (rule a) |
| US-062 | `FEAT-017-schema-evolution.md:172` | US-125 | US-062 kept by FEAT-018 "Count Entities by Field" — aggregation code/tests `crates/axon-api/src/handler.rs:4923,18703` (rule a) |
| US-070 | `FEAT-028-unified-binary.md:193` | US-126 | US-070 kept by FEAT-009 "Find Link Targets" — code refs `crates/axon-api/src/request.rs:179`, `handler.rs:21396` (rule a) |
| US-071 | `FEAT-028-unified-binary.md:206` | US-127 | US-071 kept by FEAT-009 "List Entity Neighbors" — code refs `crates/axon-api/src/handler.rs:8073,21197` (rule a) |
| US-072 | `FEAT-028-unified-binary.md:218` | US-128 | US-072 kept by FEAT-009 "Explore Graph via GraphQL" — code ref `crates/axon-graphql/src/graph.rs:1` (rule a) |
| US-073 | `FEAT-028-unified-binary.md:232` | US-129 | US-073 kept by FEAT-009 "Discover Links via MCP" — sibling of test-tagged US-070..072 block, FEAT-020 mapping table assigns it to FEAT-009; also lower FEAT (rules a/b) |
| US-074 | `FEAT-021-change-feeds-cdc.md:133` | US-130 | US-074 kept by FEAT-009 "Pattern query for ready/blocked queue" — code ref `crates/axon-cypher/src/schema.rs:76` (rule a) |
| US-074 | `FEAT-028-unified-binary.md:246` | US-131 | US-074 kept by FEAT-009 (see above, rule a) |
| US-075 | `FEAT-021-change-feeds-cdc.md:148` | US-132 | US-075 kept by FEAT-009 "Schema-declared named query" — code ref `crates/axon-schema/src/schema.rs:187` (rule a) |
| US-075 | `FEAT-026-markdown-templates.md:300` | US-133 | US-075 kept by FEAT-009 (see above, rule a) |
| US-075 | `FEAT-028-unified-binary.md:258` | US-134 | US-075 kept by FEAT-009 (see above, rule a) |
| US-076 | `FEAT-021-change-feeds-cdc.md:164` | US-135 | US-076 kept by FEAT-009 "Ad-hoc Cypher query" — code ref `crates/axon-cypher/src/error.rs:6` (rule a) |
| US-076 | `FEAT-026-markdown-templates.md:324` | US-136 | US-076 kept by FEAT-009 (see above, rule a) |
| US-077 | `FEAT-021-change-feeds-cdc.md:178` | US-137 | US-077 kept by FEAT-009 "Subscribe to a named query" — test block `crates/axon-graphql/src/dynamic.rs:12777` (rule a) |
| US-077 | `FEAT-026-markdown-templates.md:352` | US-138 | US-077 kept by FEAT-009 (see above, rule a) |
| US-078 | `FEAT-021-change-feeds-cdc.md:192` | US-139 | US-078 kept by FEAT-015 "JSON-LD Content Negotiation" — no test/coverage-map refs for any claimant; lowest FEAT wins (rule b) |
| US-078 | `FEAT-027-git-mirror.md:388` | US-140 | US-078 kept by FEAT-015 (rule b, see above) |
| US-079 | `FEAT-027-git-mirror.md:405` | US-141 | US-079 is claimed by live code tags for the FEAT-003 multi-collection audit tail (`crates/axon-server/src/gateway.rs:1975`, `crates/axon-api/src/request.rs:225`); tags must keep working (rule a) |
| US-080 | `FEAT-027-git-mirror.md:424` | US-142 | US-080 is claimed by live code/test tags for the FEAT-004 snapshot story (`crates/axon-server/tests/snapshot_test.rs:2`, `crates/axon-api/src/handler.rs:22574`); tags must keep working (rule a) |
| US-081 | `FEAT-027-git-mirror.md:440` | US-143 | US-081 kept by FEAT-008 "Idempotent Transaction Submission" — test tag `crates/axon-server/tests/api_contract.rs:1900` (rule a) |
| US-101 | `FEAT-025-control-plane.md:133` | US-144 | US-101 kept by FEAT-029 "Hide Inaccessible Entities" — FEAT-031 coverage map line 306 + `policy-enforcement.spec.ts` (rule a) |
| US-102 | `FEAT-025-control-plane.md:157` | US-145 | US-102 kept by FEAT-029 "Redact Sensitive Fields" — FEAT-031 coverage map line 307 (rule a) |
| US-103 | `FEAT-025-control-plane.md:180` | US-146 | US-103 kept by FEAT-029 "Reject Denied Writes" — FEAT-031 coverage map line 308 (rule a) |

## Reserved Ranges and Next Free ID

- **Next free ID for future allocation: `US-147`.** Allocate sequentially upward only.
- `US-120`..`US-146` are consumed by this remediation (renumber targets above).
- Historical gaps `US-029`, `US-030`, `US-082`..`US-086` are RETIRED — never allocate
  them; reuse risks colliding with stale references in alignment reviews, tracker
  beads, or commit history.
- Retired pairings (old ID + losing FEAT) are permanently dead: an old ID now refers
  ONLY to its keeper's story.
- Suffixed IDs (`US-074b`) are legacy; never mint new ones.
