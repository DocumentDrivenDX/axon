### Findings

| Severity | Area | Finding | Evidence | Required change |
|---|---|---|---|---|
| BLOCKING | Atomic bootstrap arithmetic | Required-link targets may need co-creation, but the plan's 99-type bound assumes existing targets. | Final plan §4/§5 | State pre-existing vs co-created target behavior and exact request preflight accounting. |
| BLOCKING | Legacy migration | Unmapped discovered collection/link behavior is not explicit, risking inaccessible data. | Final plan §3 | Dry-run/apply must enumerate and fail on every unmapped discovery before install. |
| BLOCKING | Migration concurrency | Epoch/signature/installation ordering leaves a possible old-epoch writer race. | Final plan §3 | Bump epoch before snapshot, drain/reject old writers, capture/verify/apply under maintenance, then reopen. |
| BLOCKING | Policy substrate | Correcting stale `__axon_policies__` docs can leave no durable policy version/hash for graph snapshots and replica tokens. | Final plan §1/§8/§10 | Define and implement one durable policy catalog/epoch/hash independent of the stale name. |
| WARNING | Cardinality | Endpoint caps should be explicit. | Final plan §5 | State exact outbound/inbound ≤1 semantics. |
| WARNING | Canonical floats | Ryu mode, integer-valued floats, exponents, and negative zero are underdefined. | Final plan §6 | Pin reference formatter and golden byte outputs; version changes. |
| WARNING | Raw SPI | `storage_mut` accessors/re-export are not explicitly sealed. | Repository handler/storage public API; final plan §2 | Remove or test-gate accessors and make sealing compile-time. |
| WARNING | Schema errors | `schema_mismatch` vs `schema_activation_changed` trigger boundary is unclear. | Final plan §4 | Define exact precedence/triggers. |
| WARNING | Schemaless surfaces | Existing CLI/evolution fail-open paths are not explicitly removed. | CLI `--schemaless`; schema evolution tests | Enumerate removal/legacy-only repurposing. |
| WARNING | Replica backend | Memory is not explicitly prohibited as FR-32 source. | Final plan §7/§10 | Require SQLite/PostgreSQL 16 durable source. |
| NOTE | Consumers | Consumer SHAs are not pinned. | Final plan §9 | Pin in release matrix before execution. |
| NOTE | Memory audit view | Derived audit visibility timing is not atomic. | Final plan §7 | Update/query under the same commit/read boundary. |

### Verdict: REQUEST_CHANGES

### Summary

The diagnosis is repository-grounded, but required-link bootstrap, unmapped legacy data, migration epoch ordering, and durable policy versioning remain blocking ambiguities. The listed warnings also need tightening before execution.
