---
ddx:
  id: RN-0.4.x
  depends_on:
    - helix.prd
    - helix.feature-registry
    - helix.implementation-plan
    - DEPLOY-CHECKLIST-001
    - RUNBOOK-001
    - helix.decision-2026-07-06-release-readiness
---

# Release Notes - Axon 0.4.x Pilot Target

## Release Scope

- Release identifier or version: Axon 0.4.x pilot target
- Release date: 2026-07-06 (target disposition; not a published binary release)
- Rollout window or environment: documentation and release-authority alignment
- Release owner: Erik LaBianca (operator/product owner)
- Source commit or build: `origin/master` at `ede4ade306ccd7ac0070d0cc959551dc91659d02`; evidence ceiling is `Cargo.toml` `version = "0.4.0"` and Git tag `v0.4.0`

## Audience and Channels

| Audience | Why they care | Delivery channel |
|----------|---------------|------------------|
| Developers and maintainers | Need the confirmed pilot target and evidence ceiling for planning and docs alignment | HELIX docs, release workflow notes |
| Operators and support | Need to know which release target is current and which note is historical | Deployment checklist, runbook, support routing |
| Internal stakeholders | Need a single release-authority note that matches the repo's actual version/tag state | HELIX release notes and decision record |

## Highlights

- The active release target is now documented as Axon 0.4.x pilot, matching the repository's evidence ceiling.
- The revoked 0.7.1 note remains available as a historical record instead of the active release authority.
- The version/tag reconciliation is explicit: `Cargo.toml` is `0.4.0`, and the newest tag is `v0.4.0`.

## Required Actions Summary

- Users: no runtime action required.
- Operators: do not present `0.7.1` as the current release target; keep `0.4.x` as the pilot target until a real `v0.4.x` publish exists.
- Support: route release-target mismatch questions to the release workflow and the decision record, not to product support.

## Changes and Fixes

### New or Improved

| Area | What changed | Who is affected |
|------|--------------|------------------|
| Release authority | Added an active 0.4.x release note so the current pilot target has its own canonical doc | Product, release, support |
| Historical retention | Kept `release-notes-0.7.1.md` as the revoked 0.7.1 historical note | Auditors, maintainers |
| Evidence ceiling | Recorded the fetched `origin/master` SHA, `Cargo.toml` version, and newest tag state in one place | Reviewers and operators |

### Fixes

| Issue or symptom | Resolution | User or operator impact |
|------------------|------------|-------------------------|
| Release authority was incomplete because the only release note was historical | Added the active `0.4.x` note | Clarifies which release target is current |
| `0.7.1` was easy to misread as the live target | Kept the old note historical and added a separate active note | Reduces accidental drift in release communication |

## Breaking Changes and Required Actions

There are no runtime breaking changes and no required migrations in this doc repair.

| Change | Impact | Required action | Deadline or trigger |
|--------|--------|-----------------|---------------------|
| Release target line corrected from revoked `0.7.1` to confirmed `0.4.x` pilot | Release and documentation stewards | Use `0.4.x` for current planning and release communication | Until an actual `v0.4.x` artifact is published |

## Migration or Rollback Guidance

### Upgrade or Migration

1. Verify `Cargo.toml` still declares `0.4.0`.
2. Verify the newest Git tag is still `v0.4.0`.
3. Re-run `ddx doc validate` after any future release-target or release-note update.

### Rollback or Hold Guidance

- Pause external release communication if a doc or artifact claims `0.7.1` as the current binary release.
- Roll back by reverting the doc alignment commit that introduced this active release note and any dependent reference updates.
- Ask for help in the release workflow tracker or the decision record.

## Known Issues and Support

| Issue | Who is affected | Workaround or next step |
|------|------------------|-------------------------|
| A real `v0.4.x` GitHub release/tag/package has not been published yet | Operators and consumers | Treat this file as pilot-target authority, not publication proof |
| The revoked `0.7.1` release note still exists as a historical record | Readers searching the doc set | Use the note title and status callout to distinguish historical from active authority |

## References

- Decision record: [../06-iterate/DECISION-2026-07-06-release-and-readiness-dispositions.md](../06-iterate/DECISION-2026-07-06-release-and-readiness-dispositions.md)
- PRD: [../01-frame/prd.md](../01-frame/prd.md)
- Feature registry: [../01-frame/feature-registry.md](../01-frame/feature-registry.md)
- Build plan: [../04-build/implementation-plan.md](../04-build/implementation-plan.md)
- Deployment checklist: [deployment-checklist.md](deployment-checklist.md)
- Runbook: [runbook.md](runbook.md)
- Historical release note: [release-notes-0.7.1.md](release-notes-0.7.1.md)
