---
ddx:
  id: metric-definitions
---

# Metric Definitions: Emitted-Metric Catalog

> Index of every metric the Axon code **actually emits**, with type, labels,
> purpose, and source. Per-metric contracts live as canonical YAML in
> `docs/helix/06-iterate/metrics/[name].yaml`; this file is the catalog that
> grounds the metrics dashboard.

**Status**: draft
**Last surveyed**: 2026-06-27 (codebase grep of `crates/`)

## Survey Method

The catalog was built by enumerating emission sites in the workspace:

```bash
grep -rnE 'metrics::(counter|gauge|histogram)!|counter!|gauge!|histogram!' crates/
```

Only metrics with a real emission call site are listed. Metrics a dashboard
*would* want but that the code does not yet emit are recorded in the
[Gaps](#gaps--assumptions) section as assumptions, never as existing metrics
(FEAT-016 phantom-claim guard).

## Emitted Metrics

| Metric | Type | Labels | Purpose | Source |
|--------|------|--------|---------|--------|
| `axon_auth_rejections_total` | counter | `error_code` | Counts data-plane auth/authz rejections, partitioned by reason | `crates/axon-server/src/auth_pipeline.rs:412` |

That single row is the complete set of emitted metrics in the workspace as of
the survey date.

### `axon_auth_rejections_total`

- **Type**: counter (monotonic; incremented by 1 per rejection).
- **Emission**: `metrics::counter!("axon_auth_rejections_total", "error_code" => err.error_code()).increment(1)` at `crates/axon-server/src/auth_pipeline.rs:412`.
- **Label `error_code`**: one of 15 fixed values from `AuthError::error_code()`
  at `crates/axon-core/src/auth.rs:1129-1147`:
  `unauthenticated`, `credential_malformed`, `credential_invalid`,
  `credential_expired`, `credential_not_yet_valid`, `credential_revoked`,
  `credential_foreign_issuer`, `credential_wrong_tenant`, `user_suspended`,
  `not_a_tenant_member`, `database_not_granted`, `op_not_granted`,
  `grants_exceed_issuer_role`, `grants_exceed_role`, `grants_malformed`.
  The variant count is pinned at `AUTH_ERROR_VARIANT_COUNT = 15`
  (`crates/axon-core/src/auth.rs:1186`), so the label cardinality is bounded and
  stable.
- **Purpose**: surfaces the volume and reason distribution of rejected
  data-plane requests. Authentication-class codes (`credential_*`,
  `unauthenticated`) signal client/token health; authorization-class codes
  (`not_a_tenant_member`, `database_not_granted`, `op_not_granted`,
  `grants_exceed_*`, `credential_wrong_tenant`) signal tenant-boundary pressure
  or probing.
- **Direction**: lower is better.
- **Canonical definition**: `docs/helix/06-iterate/metrics/axon-auth-rejections-total.yaml`.

## Gaps & Assumptions

- **No recorder/exporter is installed.** The workspace depends on the `metrics`
  facade (`metrics 0.24`, `crates/axon-server/Cargo.toml:47`) but contains no
  `metrics-exporter-prometheus` dependency, no `PrometheusBuilder`, no
  `install_recorder` / `set_global_recorder` call, and no handler serving the
  reserved `/metrics` path (`crates/axon-server/src/path_router.rs:26`). The
  counter is recorded into a no-op recorder, so **no metric is currently
  scrapable**. This is the top gap: the metric is defined in code but not
  observable in production. (Assumption: a Prometheus exporter is the intended
  surface, inferred from the reserved `/metrics` path; unverified.)
- **No latency/throughput/saturation metrics exist.** There are no request-rate,
  request-duration histograms, error-rate, queue-depth, storage, or audit-write
  metrics emitted anywhere in `crates/`. A full SRE-style RED/USE dashboard
  cannot yet be grounded; the dashboard scopes itself to the one real metric.
- **No baselines recorded.** Because nothing is scrapable, no
  `baseline`/`target`/`tolerance`/`last_verified` values have been measured for
  the one emitted metric; they are marked ASSUMED/UNVERIFIED in the canonical
  YAML.

## Review Checklist

- [x] Every listed metric has a real emission call site (file:line)
- [x] Label value sets are enumerated from source, not invented
- [x] Wanted-but-unemitted metrics are listed as gaps, not as existing metrics
