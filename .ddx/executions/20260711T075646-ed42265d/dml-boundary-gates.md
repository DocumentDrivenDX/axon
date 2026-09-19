# DML Boundary Final Gate Report

Bead: `axon-gap-closure-8711ce2a`
Date: 2026-07-11

## Results

| AC | Command | Result |
| --- | --- | --- |
| 1 | `cargo xtask audit-dml-boundary` | Passed. Auditor checked 152 governed SQL records. |
| 2 | `cargo test -p xtask audit_dml_boundary` | Passed. 5 targeted xtask tests passed, covering DML boundary fixtures and repository inventory. |
| 3 | `cargo test --workspace raw_access_compile_fail` | Passed. External trybuild raw access compile-fail case passed. |
| 4 | `cargo test -p axon-storage postgres_mutating_routines_unavailable_to_runtime` | Passed. The filtered PostgreSQL routine privilege test executed with 1 passed, 0 ignored. |
| 5 | `cargo check` | Passed. |
| 6 | `cargo test` | Passed. Full workspace tests and doctests completed successfully. |
| 7 | `cargo clippy -- -D warnings` | Passed. |
| 8 | `cargo fmt --check` | Passed. |

No production code changes were required for this final integration pass.
