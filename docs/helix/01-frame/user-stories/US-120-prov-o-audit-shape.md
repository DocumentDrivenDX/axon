---
ddx:
  id: US-120
  review:
    self_hash: 9f9de2b695714f5b093845b76f3059e1b1ce340757d9d1dd62d8944262b2a443
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# US-120: PROV-O Audit Shape

**Feature**: FEAT-003 — Audit Log
**Feature Requirements**: AUD-13, AUD-14
**PRD Requirements**: FR-15
**Priority**: P0
**Status**: Draft

## Story

**As** Ava, an agent application developer integrating Axon with a provenance-aware system
**I want** audit entries available as W3C PROV-O / JSON-LD
**So that** lineage is interchangeable across systems without bespoke translation

## Context

Renumbered from US-010 (collision with FEAT-004 "CRUD an Entity").
ADR-020 selected document-shaped storage with selective RDF concept adoption;
PROV-O is the standard provenance vocabulary. This story exercises FEAT-003's
provenance-interchange area (AUD-13, AUD-14): an additive serialization of the
same audit entries — the native JSON shape stays canonical in V1. The field
mapping, IRI rules, and negotiation surface are normative in CONTRACT-005
§PROV-O / JSON-LD serialization. Note the naming clash: `prov:Entity` is
broader than an Axon entity; documentation must distinguish them.

## Walkthrough

1. Ava queries audit history requesting the PROV-O serialization (content negotiation or format parameter, per CONTRACT-005).
2. The system returns the same entries as JSON-LD using canonical W3C PROV-O IRIs, with subject IRIs built from Axon's canonical entity URLs (ADR-020 §IRIs).
3. Her lineage tool ingests the output directly, validating it against the PROV-O ontology.
4. Existing consumers of the native JSON audit shape are unaffected.

## Acceptance Criteria

- [ ] **US-120-AC1** — Given an audit query, when the caller selects the PROV-O serialization via the negotiation surface defined in CONTRACT-005, then the response is JSON-LD mapping entries to PROV-O classes and predicates per the CONTRACT-005 mapping table.
- [ ] **US-120-AC2** — Given existing consumers of the native JSON shape, when PROV-O support is active, then native-shape responses are byte-for-byte unaffected (PROV-O is additive).
- [ ] **US-120-AC3** — Given PROV-O output, when validated against the official PROV-O ontology, then validation passes and all class/predicate IRIs are canonical W3C IRIs.
- [ ] **US-120-AC4** — Given PROV-O output, when subject IRIs are inspected, then they use Axon's canonical entity URLs per ADR-020 §IRIs.
- [ ] **US-120-AC5** — Given a set of audit entries, when serialized to PROV-O and re-imported, then all auditable facts are preserved (round-trip, AUD-14).

## Edge Cases

- **Mixed-format pagination**: cursors work identically regardless of serialization; switching formats between pages does not change which entries are returned.
- **Entries for deleted entities**: serialize correctly; the IRI remains valid as an identifier even though dereferencing returns not-found.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Happy path | US-120-AC1 | Entity with create+update history | Query audit with PROV-O selected | JSON-LD with `prov:Activity`/`prov:Agent`/`prov:Entity` per mapping |
| Additive | US-120-AC2 | Same query, native format | Query audit | Native JSON identical to pre-PROV-O behavior |
| Ontology validation | US-120-AC3 | PROV-O response document | Validate against PROV-O ontology | Valid |
| Round-trip | US-120-AC5 | 10 mixed-operation entries | Serialize → re-import → compare | All auditable facts preserved |

## Dependencies

- **Stories**: US-007 (audit query surface)
- **Feature Spec**: FEAT-003
- **Feature Requirements**: AUD-13, AUD-14
- **PRD Requirements**: FR-15
- **External**: CONTRACT-005 (PROV-O mapping, IRI and negotiation rules), ADR-020 (IRI scheme, RDF adoption rationale)

## Out of Scope

- Promoting PROV-O to the canonical wire shape (tracked as a possible future amendment; see FEAT-003 Constraints).
- SPARQL or RDF query surfaces (rejected per ADR-020).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
