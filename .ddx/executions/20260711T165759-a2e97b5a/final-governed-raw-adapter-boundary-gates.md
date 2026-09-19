# Final Governed Raw Adapter Boundary Gates

Bead: axon-gap-closure-77e2f8ac
Date: 2026-07-11

## PostgreSQL qualification environment

- `AXON_TEST_POSTGRES`: set; supplied PostgreSQL 16 qualification DSN was inherited by the workspace test. The DSN value, username, password, host, and database name are intentionally not recorded.
- `RUST_TEST_THREADS`: `1` for the workspace test.
- Environment-only retries: none.

## Command results

| AC | Command | Exit | Test count / evidence |
| --- | --- | ---: | --- |
| 1 | `cargo test -p axon-api --test raw_access_compile_fail raw_access_compile_fail_cases` | 0 | `1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`. Covers the raw-access compile-fail fixture suite for handler raw storage/audit access and `StorageCursorStore` mutable/owned extraction. |
| 2a | `! rg -n "pub fn (storage_mut\|storage_and_audit_mut\|into_storage\|audit_log_mut)" crates/axon-api/src/handler.rs` | 0 | No supported production handler raw mutator exports found. |
| 2b | `rg -U -n "#\[cfg\(test\)\]\n\s*pub\s*\n?\s*fn (storage_mut\|into_inner)" crates/axon-storage/src/cursor_store.rs` | 0 | Found both `#[cfg(test)]` escape hatches: `into_inner` at lines 46-48 and `storage_mut` at lines 59-61. |
| 3 | `cargo metadata --no-deps --format-version 1 \| jq -e "[.packages[] \| select(.name == \"axon-storage\") \| .publish] \| all(. == [])"` | 0 | Output: `true`. |
| 4 | `cargo test -p axon-server --test governed_handler_routes` | 0 | `3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`. Covered embedded shared server paths, gRPC transaction/audit query, and HTTP gateway transaction/audit query. |
| 5 | `AXON_TEST_POSTGRES=$AXON_TEST_POSTGRES RUST_TEST_THREADS=1 cargo test --workspace` | 0 | Aggregate from 83 harness summaries: `2244 passed; 0 failed; 7 ignored; 0 measured; 0 filtered out`. The ignored items were pre-existing doctest examples only: `axon_server` 3, `axon_sim` 1, `axon_storage` 3. Raw compile-fail ran and passed inside the workspace run. PostgreSQL storage conformance ran 43 assertions, all `ok`; no raw-boundary or PostgreSQL conformance test was ignored. |
| 6 | `cargo clippy -- -D warnings` | 0 | Passed with warnings denied. |
| 7 | `cargo fmt --check` | 0 | Passed. |

## Notes

No code changes or small integration fixes were required. The prior attempt's unrelated doctest relabeling was not repeated; existing ignored documentation examples were left unchanged.
