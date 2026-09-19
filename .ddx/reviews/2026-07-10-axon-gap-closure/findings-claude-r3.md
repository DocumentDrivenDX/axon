### Findings

| Severity | Area | Finding | Evidence | Required change |
|---|---|---|---|---|
| BLOCKING | Canonical payload measurement | Bare RFC 8785 JCS restricts numbers to I-JSON/IEEE-754 semantics, while Axon can hold exact integers above 2^53; Rust and TypeScript could diverge or measurement could lose distinctions. | v3 §6; RFC 8785 / I-JSON number model | Define an Axon-specific number policy that preserves or rejects out-of-range values and add shared >2^53/high-precision vectors. |
| WARNING | Namespace inventory | An underscore-prefix source scan can miss non-underscore internal tables or collection-addressable internals. | v3 §2 | Key structural checks on all CollectionId/table construction and raw storage mutation sites, not naming convention. |
| WARNING | CDC / snapshot paging | `cursor_expired` plus a retention configuration requirement conflicts with the plan's retention/erasure non-goal when audit is append-only. | v3 §9; v2 non-goals | Identify the mechanism or make it defensive-only and remove it from the finish-line-B gate. |
| WARNING | Composed-plan consistency | Superseded v2 text remains readable and can mislead phase-scoped implementation despite v3 precedence. | v2 vs v3 schema error, GA, and PostgreSQL isolation text | Consolidate corrections into one authoritative plan before implementation. |
| WARNING | Migration exclusivity | PostgreSQL advisory locks are cooperative; the catalog epoch, not the advisory lock, is the enforced boundary. | v3 §3 | Require epoch checks on every governed write and test a writer that ignores the advisory lock. |
| NOTE | Required-link construction vs op cap | A schema whose minimum required-link bootstrap exceeds 100 operations is unconstructible without a diagnostic. | v2 Phase 4; v3 §5/§6 | Reject or diagnose the schema; do not add a non-atomic exception. |
| NOTE | Phase 1 ordering | If `__axon_policies__` is implemented, that work lacks a later implementation phase. | v3 §1 vs v2 Phase 1 | Phase 1 records the decision and assigns later implementation. |

### Verdict: REQUEST_CHANGES

### Summary

The v3 overlay resolves every accepted round-2 blocker. One blocker remains: bare RFC 8785 is not a valid canonical measurement contract for Axon's exact integers above JavaScript's safe range. Consolidation and the listed warnings should also be closed before beads begin.
