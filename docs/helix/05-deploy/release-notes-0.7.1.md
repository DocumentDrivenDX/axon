---
ddx:
  id: RN-0.7.1
  depends_on:
    - helix.prd
    - helix.feature-registry
    - helix.implementation-plan
  review:
    self_hash: eeefac876b3c6461ead8ef6b0d604b150309443dc655abf70d11c7f97dc2e04c
    deps:
      helix.feature-registry: f8c97e4076e20b2f8bfe5d7706689fbad61c1ae22b38cec9920562c974a7412e
      helix.implementation-plan: 0510f3fcb3473db02d42a19eb66a9e528946a57e5aee5d03b2eecf080914329d
      helix.prd: 6703170c71275bba7d108c4f9c329d32e4104f9c965278db888ad43cdc3ca367
    reviewed_at: "2026-07-11T05:09:15Z"
---

# Release Notes - Axon 0.7.1

> **Superseded 2026-07-06 (decision owner: Erik LaBianca, operator/product
> owner).** The `0.7.1` release target recorded below is **revoked**; the
> confirmed release target disposition is **0.4.x**, backed by `Cargo.toml`
> (`version = "0.4.0"`) and Git tags reaching `v0.4.0`. See
> `docs/helix/06-iterate/DECISION-2026-07-06-release-and-readiness-dispositions.md`
> §1 for the full disposition and evidence. This file is retained as a
> historical record of the prior target; `axon-72b6f0b4` aligns the rest of
> the release/readiness doc set to the confirmed `0.4.x` target.

## Release Scope

- Release identifier or version: Axon 0.7.1
- Release date: 2026-06-14
- Rollout window or environment: documentation and website release target
- Release owner: Product Owner
- Source commit or build: pending published tag; local and `origin` evidence
  during alignment showed tags ending at `v0.2.8` while the HELIX stack is
  aligned to the operator-requested `0.7.1` target.

## Audience and Channels

| Audience | Why they care | Delivery channel |
|----------|---------------|------------------|
| Application developers | Need an accurate map of governed data-layer capabilities and examples | Website docs, sample projects, demo reels |
| Operators | Need caveats about release/tag state and operational scope | HELIX docs, release workflow notes |
| Internal stakeholders | Need a planning baseline for website and demo coverage | HELIX alignment report |

## Highlights

- HELIX PRD, feature registry, build plan, and release notes now use Axon 0.7.1
  as the planning target.
- The website expansion is governed by HELIX rather than hand-maintained
  marketing copy: feature, story, and scenario coverage must be generated from
  the canonical docs.
- The 2026-06-14 generated microsite coverage pass recorded 31 feature specs,
  140 user-story files, 17 named SCN scenarios, and 10 use-case domains mapped
  to sample projects and demo reels. This superseded note is not current website
  projection freshness evidence; use `scripts/generate_website_coverage.py
  --check` for that.
- The release notes explicitly separate the `0.7.1` documentation target from
  the currently observed package/tag state.

## Required Actions Summary

- Users: no runtime action from this documentation alignment alone.
- Operators: create or verify the `v0.7.1` release/tag/package artifacts before
  communicating `0.7.1` as a published binary release.
- Support: route install or artifact mismatch reports to release workflow work,
  not feature implementation.

## Changes and Fixes

### New or Improved

| Area | What changed | Who is affected |
|------|--------------|-----------------|
| HELIX planning | PRD and build plan version metadata aligned to 0.7.1 | Product and implementation stewards |
| Release communication | This release note records the 0.7.1 target and tag/package caveat | Users, operators, support |
| Website governance | Microsite expansion now has a generated release-bound coverage catalog | Documentation and website contributors |
| Examples and reels | Seven worked sample projects and seven scripted demo reels cover every HELIX entry in the catalog | Developers evaluating Axon use cases |

### Fixes

| Issue or symptom | Resolution | User or operator impact |
|------------------|------------|-------------------------|
| PRD still identified itself as 0.4.0 | Updated to 0.7.1 release-aligned draft | Avoids stale release framing in downstream docs |
| Build plan and backlog referenced PRD v0.4.0 | Updated references to PRD v0.7.1 | Keeps planning references internally consistent |
| Website had one quickstart cast and no sample project suite | Generated coverage, examples, and demo reel pages from HELIX sources during the 2026-06-14 alignment | Records the historical mapping pass; current website freshness must be checked separately |

## Breaking Changes and Required Actions

No runtime breaking changes are introduced by this documentation alignment.
Publishing an actual `v0.7.1` release may require separate migration or install
notes once release artifacts exist.

## Migration or Rollback Guidance

### Upgrade or Migration

1. Verify whether `v0.7.1` exists as a GitHub release/tag and package version.
2. If not, treat these notes as planning docs and do not present binary install
   commands as `0.7.1`-specific.
3. Re-run HELIX document validation after any release artifact update.

### Rollback or Hold Guidance

- Pause external `0.7.1` release communication if the repository tag, Cargo
  version, Docker image, or release assets are absent.
- Roll back by reverting this release-alignment documentation commit before
  public publication.
- Ask for help in the release workflow tracker.

## Known Issues and Support

| Issue | Who is affected | Workaround or next step |
|------|------------------|-------------------------|
| During the 2026-06-14 alignment, local and remote tags ended at `v0.2.8`; no `v0.7.1` release exists in the current refresh | Installers and release consumers | Do not use this superseded note for binary release messaging; use the active 0.4.x release note |
| During the 2026-06-14 alignment, `Cargo.toml` declared workspace version `0.2.8`; current repo evidence has since moved to `0.4.0` | Package consumers | Use current package metadata and the active 0.4.x release note for release communication |

## References

- PRD: [../01-frame/prd.md](../01-frame/prd.md)
- Feature registry: [../01-frame/feature-registry.md](../01-frame/feature-registry.md)
- Build plan: [../04-build/implementation-plan.md](../04-build/implementation-plan.md)
- Alignment report: [../06-iterate/alignment-reviews/AR-2026-06-14-release-0.7.1-website.md](../06-iterate/alignment-reviews/AR-2026-06-14-release-0.7.1-website.md)
- Website coverage source: `scripts/generate_website_coverage.py`
