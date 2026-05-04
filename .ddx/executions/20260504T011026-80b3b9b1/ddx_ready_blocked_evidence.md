# axon-8b91e47d Evidence

## Acceptance Mapping

- AC1: `crates/axon-api/src/handler.rs` includes `handle_put_schema_ddx_ready_and_blocked_named_queries_activate`, which activates a `ddx_beads` schema with `ready_beads` and `blocked_beads` schema-declared Cypher named queries.
- AC2: `crates/axon-cypher/benches/ddx_benchmark.rs` defines the DDx ready/blocked benchmark, 1k and 10k fixtures, and explicit p99 measurement gates.
- AC3: `cargo bench -p axon-cypher --bench ddx_benchmark -- --sample-size 10` passed the 1k p99 assertions; Criterion reported 4.5568-4.9914 us sampled timings for the 1k ready/blocked fixtures.
- AC4: The same benchmark passed the 10k p99 assertions; Criterion reported 61.483-69.390 us sampled timings for the 10k ready/blocked fixtures.
- AC5: `ddx bead update axon-05c1019d --notes ...` and `ddx bead close axon-05c1019d` were run after re-reading the bead; `ddx bead show axon-05c1019d` reported `Status: closed`.
- AC6: `ddx bead update axon-82b6f7b2 --notes ...` was run; `ddx bead show axon-82b6f7b2` reported the epic updated on 2026-05-04.

## Verification Commands

```text
cargo fmt --check
cargo check
cargo test
cargo clippy -- -D warnings
cargo clippy -p axon-cypher --bench ddx_benchmark -- -D warnings
cargo bench -p axon-cypher --bench ddx_benchmark -- --sample-size 10
```
