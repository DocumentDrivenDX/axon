---
ddx:
  id: RN-0.7.1
  depends_on:
    - helix.prd
    - helix.feature-registry
    - helix.implementation-plan
  review:
    self_hash: 8459009047e01155712f5fd30227bb67ce2400d3ca9bce7bcf1a9577f75436b1
    deps:
      helix.feature-registry: e274f95d36a550a5e82c16900412a48e567aed8937e50c3dbe29c59ebfdb531f
      helix.implementation-plan: c00ab6585798f23953b7f0a7a496bdd4e6d4c8668cdb0557c40dc2ac40b55c03
      helix.prd: dff98156a6cc934f406611b78b513892d85cee1bd7b4c011f045146fcdfd23e1
    reviewed_at: "2026-06-14T04:25:45Z"
---

# Release Notes - Axon 0.7.1

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
| Website governance | Microsite expansion now has a release-bound coverage target | Documentation and website contributors |

### Fixes

| Issue or symptom | Resolution | User or operator impact |
|------------------|------------|-------------------------|
| PRD still identified itself as 0.4.0 | Updated to 0.7.1 release-aligned draft | Avoids stale release framing in downstream docs |
| Build plan and backlog referenced PRD v0.4.0 | Updated references to PRD v0.7.1 | Keeps planning references internally consistent |

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
| Local and remote tags observed during alignment end at `v0.2.8` | Installers and release consumers | Create/verify the real `v0.7.1` tag and artifacts before binary release messaging |
| `Cargo.toml` still declares workspace version `0.2.8` | Package consumers | Update package metadata in a release workflow bead if `0.7.1` should be a shipped crate/binary version |

## References

- PRD: [../01-frame/prd.md](../01-frame/prd.md)
- Feature registry: [../01-frame/feature-registry.md](../01-frame/feature-registry.md)
- Build plan: [../04-build/implementation-plan.md](../04-build/implementation-plan.md)
- Alignment report: [../06-iterate/alignment-reviews/AR-2026-06-14-release-0.7.1-website.md](../06-iterate/alignment-reviews/AR-2026-06-14-release-0.7.1-website.md)
