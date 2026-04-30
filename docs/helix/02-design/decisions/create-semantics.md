# Create semantics decision

## Decision

Choose **Pattern B**: keep the storage adapter `put` contract as overwrite/upsert, and make the strict duplicate rejection behavior apply only to the surfaces that already route through transaction-style create validation: typed GraphQL `createXxx` and `commitTransaction` `op:create`.

This matches the existing storage abstraction and avoids turning the adapter into a semantics layer for all callers. It also preserves current HTTP and gRPC behavior, which are already implemented on top of the same overwrite-capable storage path.

## Why Pattern B

1. **Storage contract already means overwrite**
   The storage adapters implement `put` as an idempotent upsert. SQLite/Postgres/memory all write through without rejecting an existing row, and the adapter docs describe `put` as the standard persistence operation rather than a uniqueness guard.

2. **The strict duplicate check already exists above storage**
   Transaction commits perform the rejection for staged `create` operations before applying them. That means the domain already has a distinct "create must fail if entity exists" path without requiring storage-level rejection.

3. **Preserves existing transport semantics**
   HTTP `/entities` POST and gRPC `CreateEntity` currently follow the upsert path. Changing storage to reject duplicates would alter both transports and likely break callers that rely on overwrite semantics.

4. **Smallest behavioral change with clearest contract**
   The typed GraphQL `createXxx` mutation and transaction `op:create` can be documented as strict create semantics, while HTTP and gRPC remain documented as upsert/create-or-replace operations.

## Survey of current behavior

| Surface | Current behavior | Duplicate-id result | Notes |
|---|---|---|---|
| Typed GraphQL `createXxx` | Calls `handler.create_entity_with_caller(...)` | **Rejects** via handler-side validation / create path | This is the strict surface under review. |
| Untyped `commitTransaction` `op:create` | Staged transaction create path | **Rejects** duplicates before commit | This is already enforced in `commit_transaction`. |
| HTTP `/entities/{collection}/{id}` POST | Direct create handler backed by storage `put` | **Overwrites / upserts** | No duplicate rejection in the adapter path. |
| gRPC `CreateEntity` | Direct create RPC backed by storage `put` | **Overwrites / upserts** | Same storage contract as HTTP. |
| Storage adapter `put` | Pure persistence write | **Overwrites / upserts** | Not a duplicate guard; intended as the base contract. |

## Downstream contract impact: nexiq

Nexiq already routes around the current bug by using `commitTransaction` for create semantics, so the migration cost is **non-zero but contained**:

- If nexiq stays on `commitTransaction`, migration cost is effectively **zero**; existing behavior remains valid.
- If nexiq has tests that assumed typed GraphQL `createXxx` would upsert, those tests will need to be updated or unskipped to match the strict-create contract.
- HTTP/gRPC callers do **not** need migration for this decision, because Pattern B keeps their overwrite behavior intact.

## Follow-up implementation bead

File an implementation bead to ensure the documentation and tests explicitly describe:

- typed GraphQL `createXxx` and `commitTransaction op:create` as strict duplicate-rejecting create paths;
- HTTP and gRPC create endpoints as overwrite/upsert paths;
- storage `put` as the shared overwrite contract.

