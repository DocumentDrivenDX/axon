# Create semantics decision

## Decision

**Pattern B**: keep the storage adapter `put` contract as overwrite/upsert, and make the typed GraphQL `createXxx` path plus `commitTransaction` `op:create` the strict duplicate-rejecting surfaces.

## Rationale

The repository already treats storage `put` as an overwrite primitive. That contract is documented in `crates/axon-storage/src/adapter.rs`, and the SQLite/Postgres implementations both implement `put` as an unconditional insert-or-replace / on-conflict-do-update path. Changing the storage adapter to reject duplicates would break the existing low-level contract and would ripple into code paths that intentionally rely on overwrite semantics.

At the same time, the higher-level API behavior is not uniform today:

- typed GraphQL `createXxx` currently routes through `handler.create_entity_with_caller(...)` and then `storage.put(...)`
- `commitTransaction` enforces duplicate rejection for staged `op:create` before it reaches `storage.put(...)`
- HTTP entity create currently appears to use the same create handler as GraphQL, so it is also an upsert today
- gRPC create should therefore also track the same storage behavior unless it has an explicit guard elsewhere

Given that the storage layer is shared by many write paths, the safest interpretation is: **storage is an upsert primitive; strict create semantics belong at the API/orchestration layer**. That lets us preserve the current storage contract while making the strict surfaces explicit and testable.

## Behavior survey

| Surface | Current behavior | Duplicate id on create | Notes |
| --- | --- | --- | --- |
| Typed GraphQL `createXxx` | Calls `create_entity_with_caller` | **Rejects** only if the handler path has been fixed; survey should verify current code/tests | This is the surface the original bug report focused on |
| Untyped `commitTransaction` `op:create` | Pre-checks existence in `enforce_transaction_policy` before `storage.put` | **Rejects** with `AlreadyExists` | This is already the strict create path |
| HTTP `POST /entities` | Uses the entity create handler | **Upsert / overwrite** unless a separate guard is added | Documented as storage-backed create, not strict create |
| gRPC `CreateEntity` | Uses the entity create handler through the service layer | **Upsert / overwrite** unless a separate guard is added | Should stay aligned with HTTP unless a dedicated strict RPC is introduced |
| Storage adapter `put` | Unconditional insert-or-replace / on-conflict-do-update | **Overwrite** | Low-level primitive; not a duplicate guard |

## Downstream contract: nexiq

Nexiq already routes around the bug via `commitTransaction`, so their create flow is on the strict path. Under Pattern B, their migration cost is **zero** for the contract they already use: they only need to unskip or re-enable tests that were previously bypassing the broken typed GraphQL path. No data migration or caller rewrite is required for the transaction-based flow.

If Nexiq also depends on typed GraphQL `createXxx` being strict, that remains a behavioral fix for the GraphQL surface itself, but it does not require any storage-layer or transaction-contract changes.

## Follow-up implementation bead

File a follow-up implementation bead that:

1. keeps storage `put` overwrite semantics intact,
2. makes typed GraphQL `createXxx` strictly reject duplicates,
3. preserves `commitTransaction` `op:create` duplicate rejection,
4. updates/expands HTTP and gRPC docs to state they are upsert surfaces unless a dedicated strict variant is added later,
5. adds tests covering the survey table above.

