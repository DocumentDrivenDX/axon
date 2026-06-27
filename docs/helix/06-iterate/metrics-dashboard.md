---
ddx:
  id: metrics-dashboard
  depends_on:
    - metric-definition.axon-auth-rejections-total
    - metric-definitions
---

# Metrics Dashboard: Emitted-Metric Baseline Survey

**Review Window**: 2026-06-27 (point-in-time code survey)
**Baseline**: none recorded — this is the first survey; no prior measured run exists
**Status**: draft

## Decision

No improvement-or-regression judgment can be made this iteration. The codebase
emits exactly one metric (`axon_auth_rejections_total`) and there is **no
recorder or exporter installed**, so no value has ever been measured. The
actionable decision is to wire a metrics exporter to the reserved `/metrics`
path before any dashboard can compare values against a baseline.

## Summary

A code survey of `crates/` found a single emitted metric, the
`axon_auth_rejections_total` counter at `crates/axon-server/src/auth_pipeline.rs:412`.
The `metrics` facade records it, but no exporter (Prometheus or otherwise) is
installed and no handler serves `/metrics`, so the counter is dropped into a
no-op recorder at runtime. There is therefore no current value, no baseline, and
nothing to interpret as improvement or regression. This dashboard records that
state honestly rather than inventing readings.

## Metrics Table

| Metric | Baseline | Current | Direction | Result | Source |
|--------|----------|---------|-----------|--------|--------|
| `axon_auth_rejections_total` | none (unmeasured) | unobservable (no recorder installed) | lower | n/a — not measurable | `docs/helix/06-iterate/metrics/axon-auth-rejections-total.yaml`; emitted at `crates/axon-server/src/auth_pipeline.rs:412` |

## Interpretation Rules

- A metric with no installed recorder is **not measurable**; its result is
  recorded as `n/a`, never as pass/fail/noise, per the FEAT-016 phantom-claim
  guard.
- Once an exporter exists: a value within tolerance of the recorded baseline is
  noise; a sustained increase in `axon_auth_rejections_total` (lower-is-better)
  beyond tolerance is a regression and creates a follow-up.
- The `error_code` breakdown matters more than the absolute total: a rise
  concentrated in authorization-class codes (`not_a_tenant_member`,
  `database_not_granted`, `op_not_granted`, `grants_exceed_*`,
  `credential_wrong_tenant`) is a higher-severity signal than a rise in
  authentication-class codes and warrants its own follow-up.

## Trend Notes

- No trend data exists; this is the first survey and nothing is scrapable.
- The reserved `/metrics` path (`crates/axon-server/src/path_router.rs:26`) is
  routed away from the data plane but has no handler bound to it — the intended
  scrape surface is stubbed, not implemented.
- No latency, throughput, error-rate, saturation, storage, or audit metrics are
  emitted anywhere in `crates/`, so RED/USE coverage is currently zero outside
  the one auth counter.

## Follow-Up

- Install a metrics recorder/exporter (assumed Prometheus, from the reserved
  `/metrics` path) and bind a handler to `/metrics`, then record the first
  `axon_auth_rejections_total` baseline. This is the blocking prerequisite for a
  real dashboard.
- After the exporter lands, file an improvement-backlog item to add RED metrics
  (request rate / errors / duration) for the data plane so the dashboard covers
  more than auth rejections. See `docs/helix/06-iterate/improvement-backlog.md`.
- Re-run this survey and replace the `n/a` row with a measured baseline.

## Review Checklist

- [x] Baseline is explicit (explicitly "none recorded" with the reason)
- [x] Each metric cites a source
- [x] The summary states the decision implication
