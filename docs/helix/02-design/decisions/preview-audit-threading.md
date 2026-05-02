# Preview-record audit threading decision

## Context

[FEAT-030](../../01-frame/features/FEAT-030-mutation-intents-approval.md)
requires preview, approval, rejection, expiration, and committed-intent
lineage to be queryable from the audit log. Approval, rejection,
expiration, and commit are already audited by their respective lifecycle
helpers. **Preview is the gap.** A preview record is created in
`MutationIntentService::create_preview_record` (crates/axon-api/src/
intent.rs:870-895) without any audit-log side effect — the function takes
only `&mut StorageAdapter` and writes the intent to storage, returning
the `MutationIntentPreviewRecord` to the caller.

GraphQL (crates/axon-graphql/src/dynamic.rs:4817-4910) and MCP
(crates/axon-mcp/src/handlers.rs:674-724) call `create_preview_record`
and then return the result to the client. Neither emits a "preview
created" audit event today.

[axon-e21cad01](../../../../.ddx/beads.jsonl) (4-time intractable) blocked
on this design gap: the implementing agent had nowhere natural to thread
the audit-write call.

## Patterns considered

### Pattern A — `create_preview_record` writes the audit entry itself

Change the signature to take an audit-log handle:

```rust
pub fn create_preview_record<S, A>(
    &self,
    storage: &mut S,
    audit: &mut A,
    intent: MutationIntent,
) -> Result<MutationIntentPreviewRecord, MutationIntentLifecycleError>
where
    S: StorageAdapter,
    A: AuditLog,
```

Single point of truth. Callers cannot forget. Storage and audit writes
sequence under one transaction boundary already established by the
storage trait.

### Pattern B — callers emit the audit event after `create_preview_record` returns

Leave `create_preview_record` unchanged. GraphQL and MCP each call
`audit.append_operational(...)` after a successful preview-record
write, mirroring how later lifecycle events (approve/reject/expire)
are handled today.

Storage stays pure. Easier to unit-test the service in isolation.
Risk: callers drift apart over time and one surface forgets to emit
the event.

## Decision

**Pattern A.** `create_preview_record` takes the audit log as an explicit
parameter and emits the operational event before returning.

### Why

1. **Drift risk is the load-bearing concern.** Approval, rejection,
   expiration, and commit each have their own helper that already
   handles audit. Today's GraphQL and MCP callers correctly use those
   helpers. If preview is the only lifecycle event that requires
   callers to remember to write audit, both surfaces will eventually
   diverge — a bug exactly like the one Codex flagged on
   axon-cf99b8a4 (UI sends `policyOverride`, backend silently
   ignored). The fix is to make audit a non-skippable parameter.

2. **The "storage stays pure" argument is weak here.** The
   `MutationIntentService` is not the storage layer — it's a domain
   service that already orchestrates token signing, decision
   validation, and storage writes. Adding audit-log writes alongside
   storage writes is appropriate for a domain service. Storage adapters
   themselves do not gain an audit-log dependency.

3. **Matches the pattern we *want* for later lifecycle events too.**
   Approve/reject/expire helpers should arguably take audit-log
   parameters for the same reason. This decision opens the door to
   that consolidation; Pattern B would close it.

### Audit event shape

The preview operational event is an audit entry with:

| Field | Value |
|---|---|
| `operation` | `mutation_intent.preview` |
| `actor` | The subject from the intent (`user_id`, `agent_id`, `delegated_by`) |
| `collection` | `__mutation_intents` (the synthetic collection) |
| `entity_id` | The `intent_id` |
| `metadata` | `{ decision, schema_version, policy_version, operation_hash, expires_at }` (no pre-image, no token) |
| `before` | null (preview is a creation) |
| `after` | The intent's `review_summary` only (full pre-images live in storage but should not appear in the audit `after` blob to keep audit-query payloads bounded) |

This matches the operational-event style used elsewhere; it does not
mutate entity state and does not produce a data-mutation audit entry.

## Queryability

The bead's open question — *"queryable by intent ID — does it mean
`auditLog(collection: '__mutation_intents', entityId: intentId)` using
existing filters, or a new dedicated filter?"* — resolves to:

**Use the existing filter.** `auditLog(collection: '__mutation_intents',
entityId: intentId)` returns all lifecycle events for the intent
(preview, approval, rejection, expiration, commit) ordered by
timestamp. No new filter is required. The synthetic
`__mutation_intents` collection is reserved for this purpose; the
existing audit-query path (FEAT-003 US-007) handles it.

Example call:

```graphql
query LineageByIntent($intentId: ID!) {
  auditLog(
    filter: {
      collection: "__mutation_intents"
      entityId: $intentId
    }
    first: 50
  ) {
    edges {
      node {
        operation
        timestamp
        actor { id }
        metadata
      }
    }
  }
}
```

This returns the full lifecycle for the intent in chronological order
without any new GraphQL surface.

## Signature changes

| File | Function | Before | After |
|---|---|---|---|
| crates/axon-api/src/intent.rs:870 | `create_preview_record` | `(&self, &mut S, MutationIntent)` | `(&self, &mut S, &mut A, MutationIntent)` where `A: AuditLog` |
| crates/axon-graphql/src/dynamic.rs:4817-4910 | preview endpoint | calls service with storage | passes audit handle from request context |
| crates/axon-mcp/src/handlers.rs:674-724 | preview endpoint | calls service with storage | passes audit handle from request context |

Existing test callers (intent.rs:1976, 2824, 2865+) must be updated
to pass an audit handle (most can use a mock/stub `MemoryAuditLog`).

## Out of scope

- Refactoring approve/reject/expire helpers to take audit-log parameters
  in the same shape (worth doing later, not part of this bead).
- Introducing a new `auditLog` filter — confirmed unnecessary above.
- Bounded-payload-size policy for the synthetic-collection audit
  entries (track separately if it becomes a problem).

## Follow-up implementation bead

A new implementation bead must be filed (closing
[axon-e21cad01](../../../../.ddx/beads.jsonl)) with:

1. The signature change to `create_preview_record`.
2. Updates to GraphQL and MCP preview handlers to pass the audit
   handle.
3. The operational event emission with the field shape above.
4. Test updates: existing unit tests pass mock audit logs; a new
   integration test asserts that `auditLog(collection:
   '__mutation_intents', entityId: $intentId)` returns the preview
   event for both GraphQL- and MCP-originated previews.

axon-e21cad01 itself can close as **superseded by** the new
implementation bead once filed.
