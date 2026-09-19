### Findings

| Severity | Area | Finding | Evidence | Required change |
|---|---|---|---|---|
| BLOCKING | Cursor contract | Current CONTRACT-006/ADR-025/FEAT-032 require schema-stable tokens, conflicting with planned invalidation. | Plan §10 and governing docs | Explicitly supersede those clauses with schema/policy invalidation + rebootstrap. |
| BLOCKING | Revocation scope | Per-database policy epoch does not map cleanly to globally stored JTI revocation. | Plan policy catalog; current adapter | Define global auth epoch or deterministic fan-out. |
| BLOCKING | Cascade expansion | Duplicate logical link deletes can arise globally from multiple expanded ops. | Plan §5 | Globally reject or canonical-deduplicate with one audit attribution/order. |
| BLOCKING | Raw sealing | Additional public escape hatches remain (`into_storage`, audit mutators, cursor-store mutators, adapter re-exports). | Plan §2 and repository APIs | Enumerate fate of every escape hatch and add external compile-fail tests. |
| WARNING | Audit cursor terminology | Opaque cursor wording can be misread to remove admin audit IDs/pagination/rollback targets. | Plan §10 and current API | Name exact CDC/resume fields changing; preserve or change admin IDs explicitly. |

### Verdict: BLOCK

### Summary

Migration, limits, and backup gaps are resolved, but cursor supersession, revocation epoch scope, global cascade de-duplication, and complete raw API sealing remain blocking.
