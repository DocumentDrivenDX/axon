---
ddx:
  id: ADR-023
  depends_on:
    - ADR-019
    - FEAT-003
    - FEAT-030
  review:
    self_hash: 5bc98e9d3323a427f9ee1b4b6338c74b227b18cd738f6b12109c4bc4761506ab
    deps:
      ADR-019: 3ec156d9ec6696d67e0f12a6c80495c9166470525128ac475b95dae0b5647f7e
      FEAT-003: 15881e4941cec74cf6e0be6d023da0a34cb4f1f4efb5efbb6a9b8246e037010f
      FEAT-030: 81a89ddb42efe517ddde6ea7481c104b3600481a32072e31bd9d94cd7294922d
    reviewed_at: "2026-07-11T04:22:34Z"
---
# ADR-023: Preview-Record Audit Threading

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-05-02 | Accepted | Erik LaBianca | ADR-019, FEAT-003, FEAT-030, CONTRACT-005 | High |

> Promoted 2026-06-10 from `02-design/decisions/preview-audit-threading.md`
> to ADR form; the decision content is unchanged.

## Context

[FEAT-030](../../01-frame/features/FEAT-030-mutation-intents-approval.md)
requires preview, approval, rejection, expiration, and committed-intent
lineage to be queryable from the audit log. Approval, rejection, expiration,
and commit were already audited by their respective lifecycle helpers.
**Preview was the gap**: `MutationIntentService::create_preview_record`
(`crates/axon-api/src/intent.rs`) took only `&mut StorageAdapter`, wrote the
intent to storage, and returned — no audit-log side effect. The GraphQL and
MCP preview handlers called it and returned to the client without emitting a
"preview created" audit event. Bead axon-e21cad01 (4-time intractable)
blocked on this design gap: the implementing agent had nowhere natural to
thread the audit-write call.

| Aspect | Description |
|--------|-------------|
| Problem | Preview was the only intent-lifecycle event with no audit emission point |
| Current State | Approve/reject/expire/commit audited by lifecycle helpers; `create_preview_record` storage-only; GraphQL and MCP callers emit nothing |
| Requirements | FEAT-030: full intent lineage queryable from the audit log by intent ID |
| Decision Drivers | Drift risk between GraphQL and MCP surfaces; audit must be non-skippable; precedent of the axon-cf99b8a4 silent-divergence bug |

## Decision

We will adopt **Pattern A**: `create_preview_record` takes the audit log as
an explicit parameter and emits the operational event before returning.

**Key Points**: Audit is a non-skippable parameter | The domain service (not
storage adapters) owns the audit write | Opens the door to consolidating
approve/reject/expire helpers on the same shape

Why:

1. **Drift risk is the load-bearing concern.** If preview were the only
   lifecycle event requiring callers to remember audit, the GraphQL and MCP
   surfaces would eventually diverge — the same class of bug Codex flagged on
   axon-cf99b8a4 (UI sends `policyOverride`, backend silently ignored).
2. **The "storage stays pure" argument is weak here.** `MutationIntentService`
   is a domain service that already orchestrates token signing, decision
   validation, and storage writes; storage adapters themselves gain no
   audit-log dependency.
3. **Matches the pattern wanted for the other lifecycle events too** —
   Pattern B would close the door on that consolidation.

### Audit event shape

The normative preview audit-event field shape (operation
`mutation_intent.preview`, synthetic collection `__mutation_intents`,
bounded `after` payload, no pre-image/token in metadata) is now owned by
[CONTRACT-005](../contracts/CONTRACT-005-audit-record.md); this ADR is the
decision-time record. Decision-time summary: the event is an operational
(non-mutation) entry keyed by intent ID, with `before: null` and `after`
limited to the intent's `review_summary` so audit-query payloads stay
bounded.

### Queryability

The bead's open question — does "queryable by intent ID" need a new audit
filter? — resolves to: **use the existing filter**.
`auditLog(collection: "__mutation_intents", entityId: intentId)` returns all
lifecycle events for the intent (preview, approval, rejection, expiration,
commit) in chronological order via the existing FEAT-003 US-007 audit-query
path. No new GraphQL surface is required; the synthetic
`__mutation_intents` collection is reserved for this purpose (normative in
CONTRACT-005).

### Signature changes

The normative caller/handler surface is now owned by
[CONTRACT-005](../contracts/CONTRACT-005-audit-record.md) and the
implementation; the decision-time record is: `create_preview_record` gains a
`&mut A where A: AuditLog` parameter, the GraphQL and MCP preview handlers
pass the audit handle from request context, and existing test callers pass a
mock/stub `MemoryAuditLog`.

## Alternatives

| Option | Pros | Cons | Evaluation |
|--------|------|------|------------|
| Pattern B: callers emit the audit event after `create_preview_record` returns | Service stays storage-pure; easier to unit-test in isolation | GraphQL and MCP callers drift apart over time; one surface forgets to emit | Rejected: drift risk is the load-bearing concern |
| **Pattern A: `create_preview_record` takes an audit-log handle and emits the event itself** | Single point of truth; callers cannot forget; sequences storage and audit writes together | Service signature grows; test callers must pass an audit handle | **Selected: makes audit non-skippable** |

## Consequences

| Type | Impact |
|------|--------|
| Positive | Preview lineage queryable by intent ID with zero new GraphQL surface; both GraphQL- and MCP-originated previews audited identically by construction |
| Positive | Establishes the helper shape (explicit audit parameter) that approve/reject/expire helpers can converge on later |
| Negative | Signature change ripples through GraphQL/MCP handlers and existing unit tests |
| Neutral | Bounded-payload-size policy for synthetic-collection audit entries tracked separately if it becomes a problem |

## Risks

| Risk | Prob | Impact | Mitigation |
|------|------|--------|------------|
| `after` payload growth bloats audit queries | L | M | `after` limited to `review_summary`; full pre-images stay in storage |
| Other lifecycle helpers remain on the old shape and drift | M | L | Consolidation explicitly noted as follow-on work |

## Validation

| Success Metric | Review Trigger |
|----------------|----------------|
| `auditLog(collection: "__mutation_intents", entityId: $intentId)` returns the preview event for both GraphQL- and MCP-originated previews (integration test) | Any missing lifecycle event |
| Preview event shape matches CONTRACT-005 | Contract-test divergence |

## Supersession

- **Supersedes**: None
- **Superseded by**: None

## Concern Impact

- **Concern selection**: Strengthens the auditability concern — every intent
  lifecycle step, including preview, is an audit event; audit emission is
  structurally non-skippable in the domain service.
- **Practice override**: None.

## References

- [ADR-019: Policy Authoring and Mutation Intents](ADR-019-policy-authoring-and-intents.md)
- [CONTRACT-005: Audit Record](../contracts/CONTRACT-005-audit-record.md)
- [FEAT-003: Audit Log](../../01-frame/features/FEAT-003-audit-log.md)
- [FEAT-030: Mutation Intents and Approval](../../01-frame/features/FEAT-030-mutation-intents-approval.md)
- Beads: axon-e21cad01 (design, closed), axon-648120e4 (Pattern A
  implementation, closed), axon-ab2e52e0 (parent, closed)
