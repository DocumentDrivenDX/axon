---
ddx:
  id: helix.feature-story-e2e-traceability
  depends_on:
    - helix.prd
    - helix.technical-requirements
    - TP-001
  review:
    self_hash: 4dd909c9651e43727883ea77c837e969af103c3c3484eca07cf9d32aff2faaf9
    deps:
      TP-001: b2fd65f5c9fee74cac32a456a2eb53e5f492374e51469bbfdfce158ade121821
      helix.prd: d87a9cbc61d7abb53d32d8c675cc74c63fd9502e953c0ebee44285efde51df1f
      helix.technical-requirements: b50c3f03df0814348846c9a6e6eb9bebbc4b7be7dcb3783fdd6d9b4104a56fca
    reviewed_at: "2026-06-14T03:52:45Z"
---
# Feature Story and E2E Traceability Review

**Date**: 2026-04-19 (superseded 2026-06-10)
**Status**: Superseded
**Scope**: FEAT-001 through FEAT-031

## Superseded

This review's role is replaced by two artifacts, effective 2026-06-10:

1. **Test Plan §3 — Acceptance Criteria Layer Allocation**
   (`docs/helix/03-test/test-plan.md`): allocates acceptance-criterion classes
   (`US-<n>-AC<m>`) to test layers, defines the `@covers US-<n>-AC<m>` citation
   rule, and the `UNCITED_COVERAGE` / `ASSERTED_UNBACKED` / `UNTESTED`
   classification vocabulary.
2. **Story test plans** (`docs/helix/03-test/test-plans/STP-*.md`): per-story
   AC↔test matrices keyed by stable AC IDs, carrying the test-file evidence
   that previously lived in this review's feature matrix, with honest per-AC
   coverage statuses.

Stories now live as one-file-per-story artifacts under
`docs/helix/01-frame/user-stories/US-*.md` (see the registry README there);
the feature-level story summaries this review keyed on are obsolete.

STPs currently exist for the guardrail slice (FEAT-029, FEAT-030, FEAT-031,
FEAT-009, including US-046/US-047 which moved to FEAT-029). **For features
without STPs yet, the per-feature coverage rows formerly in this document
remain available in git history** (`git log -- docs/helix/03-test/feature-story-e2e-traceability.md`,
last full revision before 2026-06-10); their migration into STPs is tracked as
deferred backlog.

Do not add new rows here. New coverage mapping goes into the owning STP; new
layer-allocation decisions go into Test Plan §3.
