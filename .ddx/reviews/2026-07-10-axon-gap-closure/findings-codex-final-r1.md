### Findings

| Severity | Area | Finding | Evidence | Required change |
|---|---|---|---|---|
| BLOCKING | Cascade limits | A single `force=true` entity delete can expand into unbounded logical link deletes, but the 100-op and 10MB transaction limits do not say whether expanded cascade operations count. | Final plan §5/§6 | Count expanded operations and bytes before mutation, or define a separate bounded cascade limit and stable error. |
| BLOCKING | Audit ordering | Deterministic audit order is defined only for entity-delete expansion, not all mixed transactions and filtered replica batches. | Final plan §5/§10; CONTRACT-005 | Freeze user-op order, deterministic expansion order, transaction index assignment, and filtered batch order. |
| BLOCKING | Migration backup safety | “Verified backup” lacks backend-specific restoreability criteria. | Final plan §3/§7 | Restore into an isolated target, verify catalog/content signature, record checksums/commands, and refuse apply without proof. |

### Verdict: BLOCK

### Summary

Cascade accounting, complete audit ordering, and executable backup verification remain ambiguous. All three must be specified before execution.
