# Graph Benchmark Qualification Report

Bead: `axon-gap-closure-e2ffd6e3`

## Summary

- Added a release-qualification section to `docs/helix/03-test/ci-ratchets.md`.
- Added a matching L5 benchmark qualification contract to `docs/helix/03-test/test-plan.md`.
- Tightened `docs/helix/03-test/test-plans/STP-074-pattern-query-for-ready-blocked-queue.md` so the graph benchmark is explicitly tied to the dedicated reference host/runner and `TARGET_RELEASE`.

## Verification

- `python3 scripts/check_covers_traceability.py --format text`
- `ddx doc validate`
- `git diff --check -- docs/helix`
- `rg -n "dataset|hardware|backend|configuration|warmup|sample|percentile|p99|threshold" docs/helix/03-test docs/helix/04-build`
- `rg -n "dedicated|reference host|release.block|GitHub.hosted|functional" docs/helix/03-test`
- `rg -n "commit|environment|artifact|metadata|1,?000|10,?000" docs/helix/03-test/ci-ratchets.md docs/helix/03-test/test-plan.md`

## Notes

- `ddx doc validate` exited 0 and emitted an unrelated warning about `metrics-dashboard` depending on `metric-definition.axon-auth-rejections-total`.
- The benchmark contract treats GitHub-hosted functional runs as non-authoritative.
- Only the dedicated reference host/runner may decide pass/hold or clear `release.block` for `TARGET_RELEASE`.
