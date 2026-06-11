---
ddx:
  id: helix.feature-story-e2e-traceability
  depends_on:
    - helix.prd
    - helix.technical-requirements
    - TP-001
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
