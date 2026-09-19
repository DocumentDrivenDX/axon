# Axon Gap-Closure Adversarial Review — Round 2 Aggregate

## Harness verdicts

| Harness | Verdict | Valid output |
|---|---|---|
| Codex | REQUEST_CHANGES | Yes |
| Claude | REQUEST_CHANGES | Yes |
| Fiz | N/A | No: local endpoint refused connection; explicit OpenRouter retry lacked credentials |

## Accepted blockers

- Preserve source transaction boundaries in CDC/replica batches and advance a
  cursor only after a complete visible transaction projection commits.
- Materialize an immutable, policy-filtered entity-and-link snapshot with an
  as-of audit boundary and expiring continuation state; do not page live state.
- Replace public “legacy recovery reads” with an explicit `LegacyUnbound`
  catalog state reachable only by the offline migration/export tool.
- Enforce migration exclusivity with backend locks, maintenance/catalog epochs,
  and revalidation under lock.
- Use one RFC 8785/JCS measurement function in core and exact transport/logical
  limits; do not let surfaces measure independently.
- Define one entity-delete algorithm for inbound+outbound links and one
  deterministic logical audit order.
- Extend FEAT-017 compatibility/repair behavior to required links,
  cardinality, target, metadata, and link-type removal.
- Replace the incomplete three-name internal registry with a complete typed
  namespace taxonomy and positive guards on every generic handler/transaction
  path and raw `put` call-site inventory.
- Reclassify PostgreSQL `SERIALIZABLE`, memory audit co-commit, force cascade,
  PostgreSQL restart, and backup/restore as implementation work where code is
  absent.
- Use a new `schema_activation_changed` code rather than overloading the
  existing manifest `schema_mismatch` code.
- Define staged final-state link validation, fix replica composite keys, test
  client local-state redaction, and explicitly break/migrate raw `audit_id`
  public cursors.
- Freeze graph benchmark fixtures/thresholds before implementation and make
  Nexiq, DDx, and Cayce required pilot consumers. Remove GA from this plan's
  attainable verdicts.

## Evidence corrections and disagreements

- Claude inspected the stale dirty checkout's tracker when questioning the
  readiness IDs/count. The prior review independently read
  `origin/master:.ddx/beads.jsonl`, where the IDs and 19-open count exist.
  Even so, v3 treats those as seed observations only and derives all IDs/status
  after the mandatory fetch.
- `__axon_beads__` is live and governed. `__axon_policies__` appears only in
  comments on the pinned tree, with no collection construction or storage
  call. v3 requires Phase 1 to either register/implement it deliberately or
  correct the stale claim; it does not assume a load-bearing collection that
  code does not use.
- `__mutation_intents` is an audit-only logical subject and must be classified
  even though it is not a collection-backed public data store.

## Round 2 result

REQUEST_CHANGES. The fixes are incorporated in the v3 overlay and require a
final independent review round.
