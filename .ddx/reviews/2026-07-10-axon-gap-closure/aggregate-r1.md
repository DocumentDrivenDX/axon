# Axon Gap-Closure Adversarial Review — Round 1 Aggregate

## Harness verdicts

| Harness | Verdict | Valid output |
|---|---|---|
| Codex | BLOCK | Yes |
| Claude | REQUEST_CHANGES | Yes |
| Fiz | REQUEST_CHANGES | Yes |
| Gemini | N/A | No: the installed DDx Gemini route advertised no supported text model |

## Aggregated findings and adjudication

| Area | Review agreement | Adjudication for v2 |
|---|---|---|
| Baseline and tracker truth | Codex and Claude found stale-ref and readiness-wiring risk; Claude found closed beads whose claimed behavior is absent. | Accepted. Fetch and pin the remote baseline, audit both open and closed readiness beads against behavior, reopen or supersede false closures, compare the user's v0.3.2 edits to the pinned baseline, and express the final dependency chain explicitly. |
| Reserved/system collections | All three reviews found an undefined public/internal boundary or migration bypass. Claude additionally found the public `__` naming exemption and unaudited `_cdc_cursors` writes. | Accepted. Define a sealed internal collection registry, reject reserved names at public boundaries, enumerate audit-exempt derived-state mutations, and make the upgrade tool offline-only and idempotent. |
| Schema evolution and migration | Codex and Fiz raised existing-data migration and active-schema ambiguity; Claude raised the migration trust boundary. | Accepted with governing-artifact correction. v2 binds active-schema semantics to ADR-007 and FEAT-017, adds schema-version conflict checks, defines the legacy upgrade flow, and preserves FEAT-017's explicit forced-breaking-change behavior. Fiz's reference to “1128 closed beads' historical data” is rejected: 1128 is a tracker count, not evidence about stored Axon entities. |
| Link and mixed-transaction atomicity | Codex and Fiz found incomplete link/transaction semantics. | Accepted. v2 enumerates the supported V1 mutations, delete/cascade behavior, mixed entity/link transactions, durable audit, and deterministic fault/concurrency tests. Fiz's proposed `SELECT FOR UPDATE`/pessimistic locking is rejected because FEAT-008 and ADR-004 mandate OCC and explicitly prohibit it. |
| Payload and error contracts | All three reviews found missing exact measurement and cross-surface mappings. | Accepted. v2 adds a contract-first decision for canonical byte measurement and aggregate transaction limits, preserves CONTRACT-001's 400/422 split, and defines a shared structured core error plus per-surface mappings. |
| PostgreSQL qualification | Codex and Fiz marked it blocking; Claude required a named support matrix. | Accepted with repository correction. The repo already uses testcontainers locally and a PostgreSQL 16 CI service, so “replace with testcontainers” is not the fix. v2 makes PostgreSQL 16 the sole 0.4.x release-qualified major, consolidates fixtures, hard-fails release skips, bounds readiness, removes global serialization, and uses per-test isolation. |
| Backend capability claims | Codex found the memory/durability claim misleading; Claude found concurrency-harness ambiguity. | Accepted. v2 separates logical atomicity parity from durable restart guarantees and uses deterministic simulation plus real SQL concurrency tests. |
| Graph scope and policy | Codex found policy-order ambiguity; Fiz raised missing limits; Claude distinguished citation gaps from missing features. | Accepted. v2 cites CONTRACT-007's exact policy order and limits (depth 10, threshold 1,000, cardinality 1M, timeout 30s), has no invented fan-out limit, and splits implementation from citation work. |
| Local replica | All reviewers found incomplete cursor/scope/change semantics. | Accepted with governing-artifact correction. CONTRACT-006 already selects `audit_id`, so Fiz's PostgreSQL LSN recommendation is rejected. v2 freezes FEAT-032 acceptance criteria first, binds opaque tokens to full scope, defines snapshot/tail/dedup/order behavior, and invalidates/rebootstraps on policy or schema hash change. |
| Final gates | Codex found incomplete blockers; Claude found missing PRD criteria and an unmeasurable finish line B. | Accepted. v2 adds invariant-specific PRD criteria, keeps the existing pilot gate separate from a new FEAT-032 completion gate, and requires durable evidence before either verdict. |

## Round 1 result

BLOCK. The plan requires contract decisions and tracker repair before implementation can safely begin. Round 2 reviews the revised plan rather than the original.
