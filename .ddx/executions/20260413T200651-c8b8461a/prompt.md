# Execute Bead

You are running inside DDx's isolated execution worktree for this bead.
Your job is to make a best-effort attempt at the work described in the bead's Goals and Description, then commit the result. Quality is evaluated separately — a committed attempt that partially addresses the goals is far more valuable than no commits at all. Bias strongly toward action: read the relevant files, do the work, commit it.

## Bead
- ID: `axon-6f69ef62`
- Title: Author ADR for Agent Guardrails rate limiting and scope constraints (FEAT-022)
- Parent: `axon-b5a5cc01`
- Labels: chore, p1, planning, agent-guardrails
- Base revision: `658bdd20acc4582714f76dd5a201eba03fc0d31e`
- Execution bundle: `.ddx/executions/20260413T200651-c8b8461a`

## Description
Write ADR-016-agent-guardrails.md in docs/helix/02-design/adr/. The ADR must resolve: (1) Rate limiting strategy: token bucket per agent identity (identity = CallerIdentity.actor from axon-e0817efb). Bucket configuration: mutations_per_second and mutations_per_minute, burst_allowance. In-process implementation (no Redis) using a HashMap<actor, TokenBucket> protected by Mutex. Rate limit exceeded produces a retryable error with retry_after_ms. (2) Scope constraints: an agent credential can be annotated with an entity_filter (e.g., 'assignee = agent-id'). Any mutation targeting an entity not matching the filter is rejected with ScopeViolation error. Scope annotation lives in the auth layer (FEAT-012) — for V1 this means it's a field on CallerIdentity that the guardrails layer reads. (3) Interaction with FEAT-012 auth: guardrails layer reads from CallerIdentity (already threaded by axon-e0817efb). RBAC and guardrails are separate — guardrails are per-agent preventive controls, not per-role access grants. (4) Audit: all guardrail rejections produce audit entries with operation=guardrail_rejection and the rejection reason. (5) Out of scope: semantic validation hooks (deferred per FEAT-022 spec). Configuring limits via API (V1 is config-file only).

## Acceptance Criteria
ADR-016 exists at docs/helix/02-design/adr/ADR-016-agent-guardrails.md; rate limiting strategy and data structure are decided; scope constraint model is defined; audit trail for rejections is specified; no open questions remain that would block implementation (axon-0fc26e99)

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
