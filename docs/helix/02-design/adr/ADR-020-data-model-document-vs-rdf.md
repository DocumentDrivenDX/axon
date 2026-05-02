---
ddx:
  id: ADR-020
  depends_on:
    - helix.prd
    - ADR-002
    - ADR-010
    - ADR-018
    - FEAT-002
    - FEAT-003
    - FEAT-007
    - FEAT-009
    - FEAT-015
---
# ADR-020: Data Model — Document-Shaped Entities, Not Native RDF

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-05-02 | Accepted | Erik LaBianca | ADR-002, ADR-010, FEAT-007, FEAT-009, FEAT-003, FEAT-015 | High |

## Context

Axon's data model is described in the PRD as **entity-graph-relational** —
deeply nested JSON-shaped entities, typed first-class links between entities,
collections that group entities of like kind, and SQL-like queries layered on
top. ADR-002 chose JSON Schema (plus an Axon-specific link-type vocabulary)
as the schema format and explicitly rejected SHACL/ShEx and OWL/RDFS for that
narrow concern.

The broader data-model question — *should Axon be RDF-shaped at all?* — was
not decided in ADR-002. It surfaced again when we began planning a unified
read-side query language (FEAT-009 rewrite) and the natural alternatives
included SPARQL alongside Cypher. SPARQL is the RDF query language; choosing
it implies, or at least invites, RDF-shaped semantics underneath. Before
picking a language, we needed to decide whether the data model below the
language is RDF or remains document-and-link.

This ADR records that decision and the reasoning behind it. It also captures
which RDF concepts are worth adopting opportunistically without committing to
the full model.

## Three flavors

"RDF-shaped" is not a single commitment. Three distinct flavors are worth
distinguishing because they have very different implications:

| Flavor | What changes | Effort |
|---|---|---|
| **1. Native RDF** | Storage, wire, schema, and query are all RDF. Triples are primitive; entities are emergent views. Schema is SHACL. Query is SPARQL. Serialization is Turtle / N-Triples / JSON-LD. | Total rewrite |
| **2. RDF as projection** | Storage and primary surface stay document-shaped. SPARQL added alongside Cypher. JSON-LD added as alternative serialization. Audit log emits PROV-O. | Moderate–large |
| **3. Borrow concepts** | Adopt IRIs as identifiers, JSON-LD as a serialization option, PROV-O vocabulary names. No SPARQL, no SHACL, no open-world semantics. | Small |

This ADR is primarily about flavor 1 — whether Axon's *primitive* shape is RDF
— since the other two flavors are dilutions of that decision.

## Decision

**Axon is and remains document-shaped. Entities are first-class, deeply nested,
schema-validated JSON-shaped objects. Links are first-class objects with
metadata, audit, schema, and lifecycle. Cypher (read-only openCypher subset;
selection recorded in ADR-021, forthcoming) is the unified read-side query
language. Native RDF is rejected.**

We adopt three RDF-adjacent concepts opportunistically (flavor 3):

1. **Entity URLs are dereferenceable IRIs.** PRD §4c already specifies
   `/tenants/{t}/databases/{d}/collections/{c}/entities/{id}` as the canonical
   URL. We document explicitly that these URLs are IRIs in the linked-data
   sense. Future RDF-adjacent work (JSON-LD, PROV-O) builds on this anchor.

2. **JSON-LD as an alternative GraphQL serialization.** A client may request
   `Accept: application/ld+json` and receive entity payloads with a generated
   `@context` (derived from ESF), `@id` set to the entity's canonical URL,
   and `@type` derived from the schema. Default content type stays
   `application/json` (plain). This earns linked-data composability for clients
   that want it, without changing storage, schema, or the primary surface.

3. **PROV-O vocabulary for the audit log.** Audit entries map to PROV-O
   classes — `prov:Activity` for the operation, `prov:Entity` for the affected
   record, `prov:Agent` for the actor, with `prov:used` / `prov:wasGeneratedBy`
   / `prov:wasAssociatedWith` predicates. Whether PROV-O is the canonical wire
   shape or an additive serialization (e.g. `?format=prov`) is left to the
   FEAT-003 amendment that follows from this ADR.

   Naming clash to flag for readers: PROV-O's `prov:Entity` is broader than
   Axon's notion of an entity — in PROV-O it covers any data item, real-world
   object, or abstract concept that an Activity acts on. The terms collide
   harmlessly in context, but the FEAT-003 amendment should call out the
   distinction explicitly.

## What "RDF-shaped" would have entailed

For the record, here is what flavor 1 would have meant concretely:

- Every fact is a triple `(subject, predicate, object)`, optionally a quad with
  named graph `(s, p, o, g)`.
- Subjects are IRIs (or blank nodes); predicates are IRIs from a global
  vocabulary; objects are IRIs, blank nodes, or typed literals.
- "Entity" is not a primitive. An entity is whatever set of triples shares a
  subject IRI. There is no nesting — nested fields become chained blank nodes
  or IRIs.
- Collections become either named graphs or class memberships
  (`?s rdf:type :Bead`). Neither is a Collection in the Axon sense.
- Links are triples like any other. Link metadata requires either reification
  (4–5 triples per logical link) or RDF-star (now standardized but with
  uneven tooling coverage).
- Schema is SHACL. Query is SPARQL. Serialization is Turtle / N-Triples /
  JSON-LD. Reasoning (optional) is OWL / RDFS.

Storage actually maps cleanly: SPO/POS/OSP triple indexes are EAV under
different names, and ADR-010's EAV-for-secondary-indexes strategy already
runs in that family. **Storage is not where RDF hurts.** The mismatch is
above the storage layer.

## Why we rejected native RDF

### Loss of first-class entities

The vision opens with "model the world as humans and agents think about it —
people, invoices, projects, tasks — not as rows in tables." A nested invoice
is *natively* a document. In RDF it is a forest of blank nodes or chained
IRIs. The "PUT JSON, GET JSON" mental model requires JSON-LD framing on every
read and write. Framing is finicky, and most agent developers have never used
it.

### Loss of first-class links with metadata

ADR-010 makes links a first-class table with native referential integrity,
typed metadata, audit, and lifecycle. RDF triples are not natively annotatable.
Reification balloons storage and complicates queries; RDF-star is in
late-stage W3C standardization (RDF 1.2) and tooling coverage is uneven;
property-graph emulation in RDF ends up looking like reification with extra
steps. None match the cleanliness
of "a link is a row in the links table."

### Mutation intent and approval flow become harder

FEAT-030 binds mutation intents to entity pre-image versions. In RDF, an
entity is a set of triples — there is no atomic "version of bead-1" unless we
synthesize one (per-named-graph versions, or composite version derived from
member triples). Either approach diffuses the elegance of "approve this diff
to bead-1 at version 5" into "approve this set of triple additions and
deletions."

### Closed-world semantics are load-bearing

Axon today is closed-world: a bead has the fields its schema declares; others
are invalid. RDF is open-world by default — absence is not falsity, and any
triple about any subject is valid until SHACL says otherwise. Closed SHACL
shapes exist but are opt-in and verbose. Closed-world matters for validation
errors, missing-field detection, agent guardrails, and the "schema is the
single source of truth" claim in PRD §10.

### Persona and ecosystem fit

The PRD's primary persona ("Ava the agent developer") is comfortable with
JSON, GraphQL, and REST. SPARQL is more powerful than Cypher in some
dimensions (formal semantics, federation, property paths) but the syntax is
verbose and the mental model unfamiliar. LLMs handle SPARQL but generate it
less reliably than Cypher or SQL — there is simply less training data.
Internal projects (tablespec, niflheim, DDx, beads) are document-shaped;
every integration boundary would become a JSON ↔ RDF translation.

### OLTP vs OLAP gravity

RDF stores have historically been OLAP-shaped — knowledge graphs, integration
warehouses, semantic layers. Production OLTP triple stores exist but are a
minority of the RDF ecosystem. The PRD's performance targets — <10ms p99
single-entity write — are commodity territory for document/relational OLTP
and a stretch for triple stores at the schema sizes we anticipate.

### Vision drift

The vision positions Axon as "Firebase, but audit-first, schema-first,
policy-aware, agent-native." Firebase is document-shaped. An RDF-shaped Axon
would be positioned closer to Stardog or AllegroGraph — a different product
in a different category, with different competitors and a different
go-to-market.

## What we acknowledge giving up

Native RDF would have offered real wins. We name them so future readers
understand the trade:

- **W3C standards stack.** RDF 1.1 + SPARQL 1.1 + SHACL + JSON-LD + PROV-O is
  a complete, peer-reviewed, formally-specified ecosystem. Cypher is moving to
  ISO GQL but is not there yet.
- **Vocabulary composability.** A bead being simultaneously `:Bead`,
  `prov:Activity`, and `schema:Action` is genuinely powerful for knowledge-
  graph use cases (Helix artifact graph, MDM identity lineage, CDP entity
  resolution).
- **Federation.** SPARQL 1.1 federation lets a query span endpoints
  transparently. Useful if the artifact graph crosses repos or if niflheim
  ever exposes SPARQL.
- **Audit-as-PROV.** The audit log could literally *be* PROV triples,
  universally interchangeable with any provenance-aware system.
- **Open-world extensibility.** Adding fields never breaks anyone; SHACL
  validates only the subset you care about.
- **Reasoning.** OWL inference can materialize implicit triples
  (`owl:TransitiveProperty` and friends).

We address the most concrete of these — audit-as-PROV — through the PROV-O
vocabulary adoption above. The others remain available later if a real use
case (e.g. Helix artifact graph stored in Axon) demands them; the
document-shaped storage and the IRI-aware addressing leave the door open
without committing now.

## What stays the same

The following are not affected by the document-vs-RDF decision and apply
under either model:

- Stateless servers (PRD §2)
- Multi-backend storage (ADR-003, ADR-010)
- Audit-first commitment (FEAT-003)
- Path-based addressing (PRD §4c, ADR-018) — already aligned with linked-data
  principles
- Policy enforcement layer (FEAT-029, ADR-019)
- MCP exposure (FEAT-016, ADR-013)
- EAV secondary index strategy (ADR-010, FEAT-013) — SPO/POS/OSP would have
  been the same shape with different names

## Alternatives

| Option | Pros | Cons | Evaluation |
|---|---|---|---|
| A. Native RDF (flavor 1) | W3C standards, vocabulary composability, federation, audit-as-PROV native, open-world extensibility | Vision drift, persona mismatch, loss of first-class entities and links, harder mutation-intent binding, OLTP performance risk, ecosystem split at every internal boundary | **Rejected** |
| B. RDF as projection (flavor 2) | Earns SPARQL + JSON-LD + PROV interop without abandoning entities; SPARQL alongside Cypher over one planner | Two query languages to maintain, two schema models (ESF + SHACL), broader test surface; benefits unproven for current consumers | **Rejected for V1** — reconsider when (a) federation across deployments is on the roadmap, (b) the Helix artifact graph or another knowledge-graph use case needs first-class RDF, or (c) an external integration explicitly requires SPARQL |
| **C. Document-shaped + selective RDF concept adoption (flavor 3)** | Smallest deviation from current vision and persona; preserves first-class entities and links; earns linked-data composability for clients that want it (JSON-LD); audit log gets the standard provenance vocabulary (PROV-O); IRI commitment leaves room for future flavor-2 adoption | Not a full RDF citizen — federation, SHACL validation, OWL reasoning, and SPARQL queries are not available; some knowledge-graph users will reach for those | **Selected** |
| D. Document-shaped, no RDF concepts at all | Smallest spec surface | Forgoes cheap wins (PROV-O for audit is genuinely elegant; JSON-LD for clients that want it is nearly free); no anchor for any future RDF-adjacent work | Rejected |

## Consequences

| Type | Impact |
|---|---|
| Positive | Vision and persona stay aligned. First-class entities and links remain. Mutation-intent flow keeps clean entity-version semantics. Cypher (ADR-021) is the natural language choice. JSON-LD and PROV-O adoption are bounded, additive, and reversible. IRI documentation costs nothing. |
| Negative | We do not get SPARQL, SHACL, OWL reasoning, or federation. Clients that already speak RDF need translation. The Helix artifact-graph use case (if it lands in Axon) will work but won't get the knowledge-graph-native treatment some might expect. |
| Neutral | Storage layer unchanged (EAV indexes are EAV indexes). Path addressing unchanged. Multi-backend strategy unchanged. ADR-002's schema-format decision stands. |

## Implementation impact

This ADR primarily *constrains* and *unblocks* other work; it does not by
itself produce code changes. Direct follow-ups:

1. **ADR-021 (graph query language, forthcoming)** — will record the
   openCypher subset selection. This ADR removes SPARQL from contention,
   making Cypher the default.
2. **FEAT-009 rewrite** — unified Cypher-based graph query feature. Absorbs
   FEAT-020.
3. **FEAT-015 amendment** — JSON-LD content negotiation user story.
   `Accept: application/ld+json` → entity payload with generated `@context`,
   `@id` = canonical URL, `@type` from schema. Default content type unchanged.
4. **FEAT-003 amendment** — PROV-O audit shape user story. Decision on
   canonical-vs-additive serialization left to that amendment.
5. **PRD §4c clarification** — explicit statement that entity URLs are
   dereferenceable IRIs, anchoring the JSON-LD and PROV-O work.

## Open questions

- **Whether to revisit flavor 2 if the Helix artifact graph lands in Axon.**
  Vision → PRD → spec → ADR → bead is a knowledge graph by nature. If we
  decide to host it in Axon, the case for adding SPARQL alongside Cypher
  strengthens. Track as a future decision, not a current commitment.
- **Whether SHACL export is worth offering as a schema bridge.** ADR-002 has
  ESF → JSON Schema, ESF → SQL DDL, ESF → Protobuf, ESF → TypeScript bridges.
  ESF → SHACL is feasible and would let RDF-shaped consumers validate Axon
  payloads natively. Defer until a consumer asks.
- **PROV-O canonical vs additive.** The FEAT-003 amendment will pick. Both
  are reasonable; canonical is more elegant but breaks any existing
  audit-log consumers.

## References

- W3C RDF 1.1 Concepts: https://www.w3.org/TR/rdf11-concepts/
- W3C SPARQL 1.1 Query Language: https://www.w3.org/TR/sparql11-query/
- W3C SHACL: https://www.w3.org/TR/shacl/
- W3C JSON-LD 1.1: https://www.w3.org/TR/json-ld11/
- W3C PROV-O: https://www.w3.org/TR/prov-o/
- ISO/IEC 39075:2024 (GQL — Cypher's ISO standard, published April 2024)
- ADR-002: Schema Format — JSON Schema + Link-Type Definitions
- ADR-010: Physical Storage and Secondary Indexes
- ADR-018: Tenant / User / Credential Model
