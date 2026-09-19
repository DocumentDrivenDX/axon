### Findings

| Severity | Area | Finding | Evidence | Required change |
|---|---|---|---|---|
| BLOCKING | Canonical JSON | Duplicate JSON keys can collapse differently before canonicalization. | Plan Phase 6 | Reject duplicates at every raw JSON ingress with stable parity tests. |
| BLOCKING | Graph redaction | Ordering/grouping/distinct/cursors may leak redacted values. | Plan Phase 8 | Ban redacted fields from every comparison/order/group/cursor path. |
| WARNING | Hidden-tail liveness | No advancement behavior when only hidden groups occur. | Plan Phase 10 | Define fixed opaque checkpoint/heartbeat behavior. |
| WARNING | Repair rule | “Reduce” and “must end valid” conflict. | Plan Phase 4 | Choose final-valid-only or bounded partial. |
| WARNING | Multi-schema error | Singular version fields do not cover mixed transactions. | Plan Phase 4/5 | Return sorted per-schema changes with op indexes. |

### Verdict: REQUEST_CHANGES

### Summary

Duplicate-key parsing and redaction through ordering/cursors remain blocking; the other findings are narrow consistency fixes.
