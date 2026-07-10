### Findings

| Severity | Area | Finding | Evidence | Required change |
|---|---|---|---|---|
| BLOCKING | Replica schema stability | Accepted ADR-025 requires compatible schema migrations to remain resumable. | Plan token invalidation; ADR-025/CONTRACT-006 | Preserve compatible migration tokens or explicitly supersede. |
| BLOCKING | Token confidentiality | Signed base64 JSON is readable and can leak audit boundaries/hidden volume; ADR-025 calls for server-resolved handles. | Current `cursor_token.rs`; plan §10; ADR-025 | Use durable server-resolved random handles or authenticated encryption; test no recoverable boundary. |
| BLOCKING | Graph snapshot | Production policy/schema is evaluated live per hop; no one snapshot is threaded through the production executor/subscriptions. | Plan §8; handler/policy code | Add query execution context and mid-query/subscription change tests on GraphQL/MCP. |
| BLOCKING | PostgreSQL concurrency | One-connection pool cannot exercise SERIALIZABLE conflicts. | Current postgres adapter; plan §5/§7 | Redesign connection-per-transaction/pool and require an observed serialization conflict. |
| WARNING | Sealing replacement | GraphQL/MCP currently use raw mutable handler accessors. | Current call sites; plan §2 | Add governed replacement APIs before compile-time sealing. |
| WARNING | Graph production path | Planner limits are not proven through production GraphQL/MCP; named queries disable budget. | Planner/schema code; STPs | Route production through planner and make named override explicit/reviewed. |
| WARNING | Cursor migration | Hard opaque-only cut conflicts with ADR deprecation mitigation. | Plan/ADR-025 | Choose and amend explicitly. |
| WARNING | Link metadata redaction | Visible-link properties are not explicitly redacted in graph/replica tests. | Handler traversal; CONTRACT-007 | Add requirement and E2E fixtures. |
| WARNING | Canonical parser | serde_json features and integer/float classification are not pinned. | Plan §6/current config | Freeze parser profile and boundary vectors. |
| WARNING | Transport limit | A post-parse decompressed cap cannot prevent compression bombs. | Plan §6/server | Enforce before/during decompression or reject compression. |
| WARNING | Schemaless CLI | The real path is omitted optional schema, not a `--schemaless` flag. | CLI/API | Retarget removal and test null/omitted schema. |
| WARNING | Expanded overlap | De-duplication must apply to the fully expanded global plan. | Plan §5 | Define global collision resolution. |
| NOTE | Memory single-op | Single mutations still need the co-commit path. | Current handler/memory | Route all governed single ops through unified plan. |
| NOTE | Scalar redaction | Test scalar redacted projection on every surface. | Named-query compiler/CONTRACT-007 | Add fixtures or compile check. |
| NOTE | CJSON naming | AXON-CJSON-1 differs from JCS. | Plan/earlier review | Explicitly state non-JCS profile. |

### Verdict: BLOCK

### Summary

Confidential/stable token semantics, production query snapshots, and observable PostgreSQL serialization remain blocking. The surface and canonicalization warnings also need explicit closure.
