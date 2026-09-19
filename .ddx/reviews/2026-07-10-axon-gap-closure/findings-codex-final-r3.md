### Findings

| Severity | Area | Finding | Evidence | Required change |
|---|---|---|---|---|
| BLOCKING | Replica offline security | Revoked disconnected users may still query materialized data. | Plan auth epoch/replica purge | Define freshness lease, lock/purge behavior, and revocation-disconnect tests. |
| BLOCKING | Delete overlap | Explicit delete + generated cascade is both rejected and de-duplicated. | Plan Phase 5 | Distinguish explicit duplicates from generated overlap. |
| BLOCKING | Graph limit | Named override conflicts with fixed 1M V1 limit. | Plan governing semantics/Phase 8 | Make hard cap or define bounded override contract. |
| BLOCKING | Auth epoch | Query context omits global auth epoch. | Plan Phase 1/8 | Thread/terminate on auth change. |
| BLOCKING | Backup signature | Content signature scope is undefined. | Plan Phase 3 | Define versioned exhaustive signature with exclusions. |
| WARNING | Token wording | Signed token wording conflicts with server handle. | Plan | Use explicit handle classes/confidentiality. |
| WARNING | Consumer evidence | Workload commands/postconditions/skips/traffic are not frozen. | Plan Phase 9/11 | Predeclare per-consumer evidence manifests. |

### Verdict: BLOCK

### Summary

Replica freshness, delete overlap, graph ceiling, auth invalidation, and backup signature scope remain blocking consistency gaps.
