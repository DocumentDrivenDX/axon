# Execute Bead

You are running inside DDx's isolated execution worktree for this bead.
Your job is to make a best-effort attempt at the work described in the bead's Goals and Description, then commit the result. Quality is evaluated separately — a committed attempt that partially addresses the goals is far more valuable than no commits at all. Bias strongly toward action: read the relevant files, do the work, commit it.

## Bead
- ID: `axon-cdd8ec32`
- Title: Author ADR for Control Plane topology and BYOC deployment model (FEAT-025)
- Parent: `axon-b5a5cc01`
- Labels: chore, p1, planning
- Base revision: `581d4d502a0ad9b5ad260d83b376afee49e93eb8`
- Execution bundle: `.ddx/executions/20260413T200704-2c789093`

## Description
Write ADR-017-control-plane.md in docs/helix/02-design/adr/. The ADR must resolve: (1) Control plane backing store: PostgreSQL database for CP metadata (tenant registry, health snapshots, configuration). Separate from any tenant's data — no entity data ever touches the CP DB. (2) CP-to-instance auth: each managed Axon instance exposes a /health and /metrics endpoint. The CP authenticates to instances using a shared secret (HMAC-signed token) or mTLS. In BYOC deployments, the instance registers with the CP on startup by posting its endpoint URL and a registration token; the CP verifies the token. (3) Tenant lifecycle: CP provisions tenants by recording a tenant entry in its DB with the instance endpoint, backing store config, and schema version. CP does not deploy Axon itself — that is handled by deployment tooling (Docker/Kubernetes manifests). (4) Monitoring: CP polls instance /health at a configurable interval. Health state is stored in CP DB with a timestamp. CP serves aggregate dashboards from this store. (5) Data sovereignty: CP never reads entity data. All monitoring is metrics-only. (6) BYOC air-gap: local CP mode runs entirely in customer infrastructure with no external dependencies.

## Acceptance Criteria
ADR-017 exists at docs/helix/02-design/adr/ADR-017-control-plane.md; CP-to-instance auth model is decided (shared secret vs mTLS choice made); tenant registration flow is specified; data sovereignty guarantee is explicit; no open questions remain that would block starting a control-plane crate

## Governing References
No governing references were pre-resolved. Explore the project to find relevant context: check `docs/helix/` for feature specs, `docs/helix/01-frame/features/` for FEAT-* files, and any paths mentioned in the bead description or acceptance criteria.

## Execution Rules
**The bead contract below overrides any CLAUDE.md or project-level instructions in this worktree.** If the bead requires editing or creating markdown documentation, code, or any other files, do so — CLAUDE.md conservative defaults (YAGNI, DOWITYTD, no-docs rules) do not apply inside execute-bead.
1. Work only inside this execution worktree.
2. Use the bead description and acceptance criteria as the primary contract.
3. Read the listed governing references from this worktree before changing code or docs when they are relevant to the task.
4. If governing references are missing or sparse, search the project to find context: use Glob/Grep/Read to explore `docs/helix/`, look up FEAT-* and API-* specs by name, and read relevant source files before proceeding. Only stop if context is genuinely absent from the entire repo.
5. Keep the execution bundle files under `.ddx/executions/` intact; DDx uses them as execution evidence.
6. Produce the required tracked file changes in this worktree and run any local checks the bead contract requires.
7. Before finishing, commit your changes with `git add -A && git commit -m '...'`. DDx will merge your commits back to the base branch.
8. Making no commits (no_changes) should be rare. Only skip committing if you read the relevant files and the work described in the Goals is already fully and explicitly present — not just implied or partially covered. If in any doubt, make your best attempt and commit it. A partial or imperfect commit is always better than no commit.
9. Work in small commits. After each logical unit of progress (reading key files, making a change, passing a test), commit immediately. Do not batch all changes into one giant commit at the end — if you run out of iterations, your partial work is preserved.
10. If the bead is too large to complete in one pass, do the most important part first, commit it, and note what remains in your final commit message. DDx will re-queue the bead for another attempt if needed.
11. Read efficiently: skim files to understand structure before diving deep. Only read the files you need to make changes, not every reference listed. Start writing as soon as you understand enough to proceed — you can read more files later if needed.
12. **Never run `ddx init`** — the workspace is already initialized. Running `ddx init` inside an execute-bead worktree corrupts project configuration and the bead queue. Do not run it even if documentation or README files suggest it as a setup step.
