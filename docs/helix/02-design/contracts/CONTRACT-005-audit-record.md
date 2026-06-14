---
ddx:
  id: CONTRACT-005
  depends_on:
    - FEAT-003
    - FEAT-023
    - FEAT-030
    - ADR-018
  review:
    self_hash: bcb9cb60847f753c4a551a3fb1ffde94527772d5bf4f4b726399a4f3a0ae46ae
    deps:
      ADR-018: 88bbe812ae5dfd953cc504c367b32f176ca8c182318c3bbbb16a60a962f94057
      FEAT-003: 15881e4941cec74cf6e0be6d023da0a34cb4f1f4efb5efbb6a9b8246e037010f
      FEAT-023: 24416c13b9a48e864ae43e3967c63d2711763c745905850dbb4f03768ffc7949
      FEAT-030: 81a89ddb42efe517ddde6ea7481c104b3600481a32072e31bd9d94cd7294922d
    reviewed_at: "2026-06-14T03:52:45Z"
---

# Contract

**Contract ID**: CONTRACT-005
**Type**: schema (audit entry record + serialization surface)
**Version**: 0.1.0
**Status**: draft
**Related**: FEAT-003, FEAT-023, FEAT-030, FEAT-007, FEAT-026, ADR-018, ADR-020, docs/helix/02-design/adr/ADR-023-preview-audit-threading.md

## Purpose

Defines the normative audit entry record: field set, operation taxonomy,
rollback/revert operation literals, mutation-intent lifecycle threading
fields, and the PROV-O / JSON-LD serialization surface. Audit consumers,
storage adapters, CDC projection, and provenance integrators implement
against this document.

## Scope and Boundaries

- In scope: audit entry fields, operation name literals, intent-lifecycle
  audit event shape, PROV-O serialization negotiation and IRI rules.
- Out of scope: audit query API pagination shapes (API contracts), CDC
  envelope projection of audit entries (CONTRACT-006), rollback execution
  semantics (FEAT-023), retention and tiered storage.
- Owning system: `axon-audit`.

## Normative Surface

### Audit entry fields

Every mutation MUST produce exactly one audit entry; no bypass path exists.

| Element | Type / Shape | Required | Rules | Notes |
|---------|--------------|----------|-------|-------|
| `id` | integer | yes | Unique, monotonically increasing within a database; total order per database | Serves as CDC offset (`audit_id`) |
| `timestamp` | UTC timestamp, nanosecond precision | yes | Server-assigned; MUST NOT be caller-supplied | |
| `actor` | string | yes | User ID, agent ID, API key ID, or `"system"`; defaults to `"anonymous"` when unauthenticated | Entry is still created without an actor |
| `operation` | string, dot-namespaced | yes | MUST be one of the taxonomy literals below or a feature-owned extension | |
| `collection` | string | yes | Collection name | Synthetic collections (e.g. `__mutation_intents`) allowed |
| `entity_id` | string | conditional | Required for entity-scoped operations | |
| `before` | JSON object \| null | yes | Full entity state before the operation; `null` for creates | Policy-redacted on read-out, immutable in storage |
| `after` | JSON object \| null | yes | Full entity state after the operation; `null` for deletes | Policy-redacted on read-out |
| `diff` | structured diff | conditional | Present for updates | Changed fields only |
| `metadata` | map\<string, string\> | no | Caller-supplied `audit_metadata` (reason, correlation ID, session); purely informational | MUST NOT affect the operation |

Audit entries MUST be append-only: no API operation may modify or delete an
entry.

### Operation taxonomy

Core V1 literals (extend-only; feature specs may add namespaced operations):

| Operation | Emitted by |
|---|---|
| `entity.create` | Entity creation |
| `entity.update` | Full update and patch |
| `entity.delete` | Entity deletion |
| `entity.revert` | Entity-level rollback (FEAT-003 / FEAT-023). Standard OCC applies: the revert is a new write at current version, not a version rewrite |
| `link.create` | Link creation (FEAT-007) |
| `link.delete` | Link deletion (FEAT-007) |
| `collection.create` | Collection lifecycle |
| `collection.drop` | Collection lifecycle |
| `schema.update` | Schema/policy version change |
| `template.create` / `template.update` / `template.delete` | Markdown template lifecycle (FEAT-026) |

Reserved rollback literals (FEAT-023; reserved now, emitted when the
workflow ships):

| Operation | Meaning |
|---|---|
| `collection.rollback` | Point-in-time rollback of a collection/database; MUST include references to the original mutations it compensates for |
| `transaction.rollback` | Compensating transaction undoing a transaction by ID |

### Mutation-intent lifecycle threading

Intent lifecycle events are operational audit entries in the synthetic
collection `__mutation_intents`, keyed by intent ID. The preview event shape
is normative (ADR-023, preview-audit-threading decision):

| Field | Value |
|---|---|
| `operation` | `mutation_intent.preview` |
| `actor` | Subject from the intent (`user_id`, `agent_id`, `delegated_by`) |
| `collection` | `__mutation_intents` |
| `entity_id` | The `intent_id` |
| `metadata` | `{ decision, schema_version, policy_version, operation_hash, expires_at }` — MUST NOT include the pre-image or the intent token |
| `before` | `null` (preview is a creation) |
| `after` | The intent's `review_summary` only; full pre-images stay in intent storage |

The full intent lifecycle (preview, approval, rejection, expiration, commit)
MUST be queryable through the existing audit filter path as
`auditLog(collection: "__mutation_intents", entityId: <intentId>)`, ordered
by timestamp. No dedicated filter is introduced.

### PROV-O / JSON-LD serialization

PROV-O output is an additive serialization of the same entries; the native
JSON shape is canonical and unchanged.

- Content negotiation: a request MAY select PROV-O via
  `Accept: application/ld+json` with a PROV-O `@context`, or via the query
  parameter `?format=prov`.
- PROV-O serialization MUST use canonical W3C IRIs
  (`http://www.w3.org/ns/prov#Activity`, etc.) and MUST validate against the
  official PROV-O ontology.
- Subject IRIs MUST use Axon's canonical tenant-prefixed entity URLs:
  `/tenants/{tenant}/databases/{database}/collections/{collection}/entities/{id}`
  (ADR-018 / ADR-020 §IRIs).
- Round-trip: native audit JSON → PROV-O → re-import MUST preserve all
  auditable facts.

Mapping (normative):

| Audit field | PROV-O class / predicate |
|---|---|
| `operation` | `prov:Activity` |
| affected entity / link | `prov:Entity` |
| `actor` | `prov:Agent` |
| `before` state | linked via `prov:used` |
| `after` state | linked via `prov:wasGeneratedBy` |
| actor → operation association | `prov:wasAssociatedWith` |
| operation → operation chain (transactions) | `prov:wasInformedBy` |
| `timestamp` | `prov:startedAtTime` / `prov:endedAtTime` |

## Precedence and Compatibility

- Versioning: the operation taxonomy is extend-only. Existing literals MUST
  NOT be renamed or removed; new operations use dot-namespacing owned by the
  defining feature spec.
- Ordering: entries within one database are totally ordered by `id`.
  Cross-database ordering is not guaranteed.
- The native JSON shape is canonical; PROV-O is additive in V1. Promoting
  PROV-O to canonical requires a spec amendment.
- Storage immutability precedes read redaction: entries are immutable in
  storage; `before`/`after`/`diff` are policy-filtered per caller at read
  time (CONTRACT-004).

## Error Semantics

| Condition | Error / Outcome | Retry | Recovery Expectation |
|-----------|------------------|-------|----------------------|
| Attempt to modify or delete an audit entry via API | Operation rejected (no mutable surface exists) | no | None — append-only by construction |
| Revert target fails current schema validation | Revert fails with a clear schema-mismatch error | yes | Use force-revert (bypasses schema validation, with warning) or fix schema |
| Revert conflicts with later writes (OCC) | Conflict error; rollback is a write at current version | yes | Resolve conflict and retry |
| PROV-O requested with unsupported `Accept`/`format` | Native JSON returned or `406` per surface convention | yes | Use `application/ld+json` or `?format=prov` |
| Missing actor context | Entry created with `actor: "anonymous"` | n/a | None |

## Examples

```json
{
  "id": 4211,
  "timestamp": "2026-06-10T14:30:00.000000001Z",
  "actor": "agent_ap_reconciler",
  "operation": "entity.update",
  "collection": "invoices",
  "entity_id": "inv_001",
  "before": { "id": "inv_001", "version": 4, "data": { "status": "submitted" } },
  "after":  { "id": "inv_001", "version": 5, "data": { "status": "approved" } },
  "diff": { "data.status": { "from": "submitted", "to": "approved" } },
  "metadata": { "reason": "auto-reconciliation", "correlation_id": "run-77" }
}
```

Preview lifecycle event:

```json
{
  "operation": "mutation_intent.preview",
  "actor": "agent_ap_reconciler",
  "collection": "__mutation_intents",
  "entity_id": "mint_01H...",
  "before": null,
  "after": { "review_summary": "update invoices/inv_001: amount_cents 90000 -> 1200000" },
  "metadata": {
    "decision": "needs_approval",
    "schema_version": "12",
    "policy_version": "12",
    "operation_hash": "sha256:...",
    "expires_at": "2026-06-10T22:00:00Z"
  }
}
```

## Non-Normative Notes

PROV-O's `prov:Entity` is broader than Axon's entity concept; prefer
"PROV Entity" in documentation to avoid the naming clash. Batch-internal
audit writes, compression, and diff-only storage are implementation
latitude, provided the record surface above is preserved.

## Validation Checklist

- [ ] Normative fields and rules are explicit.
- [ ] Compatibility and precedence rules are explicit.
- [ ] Error handling is explicit.
- [ ] At least one executable test can be derived from this contract.
- [ ] Non-normative notes cannot be mistaken for contract requirements.
