# Execute Bead

You are running inside DDx's isolated execution worktree for this bead.
Your job is to make a best-effort attempt at the work described in the bead's Goals and Description, then commit the result. Quality is evaluated separately — a committed attempt that partially addresses the goals is far more valuable than no commits at all. Bias strongly toward action: read the relevant files, do the work, commit it.

## Bead
- ID: `axon-f63ac16f`
- Title: Clarify V1 isolation model in PRD: snapshot isolation vs full serializability
- Parent: `axon-b5a5cc01`
- Labels: chore, p1, planning
- Base revision: `f72d768c18c79ffd33d56d6afde18a55e502336f`
- Execution bundle: `.ddx/executions/20260413T200839-679ca1d9`

## Description
PRD §5 isolation table says 'Serializable is the default in V1, implemented via OCC with conflict detection at commit'. This is inaccurate: OCC with write-set conflict detection provides Snapshot Isolation, not Serializability. Write skew (two transactions each read disjoint entities and write to each other's read set) is not prevented. FILES TO UPDATE: (1) docs/helix/01-frame/prd.md §5 isolation table: change 'Serializable' row Axon Support from 'Default' to 'P1 — requires read-set tracking not yet implemented'. Change 'Snapshot Isolation' row to 'Default in V1 — write-set OCC provides snapshot isolation'. Add explicit note: 'V1 known gap: write skew is not prevented.' (2) docs/helix/01-frame/features/FEAT-008-acid-transactions.md: US-022 acceptance criteria currently states 'Write skew is prevented: if T1 reads A and writes B while T2 reads B and writes A, at most one commits' — this is FALSE for V1. Update US-022 to: keep the basic isolation criterion, add a note that write-skew prevention is P1 post-V1, and mark the write-skew criterion as deferred. (3) No other documents may claim serializable isolation as V1 default.

## Acceptance Criteria
PRD §5 isolation table accurately describes V1 as snapshot isolation (not serializable); write skew gap explicitly documented in PRD; FEAT-008.md US-022 acceptance criteria does NOT claim write-skew prevention as V1 behavior; a grep for 'serializable.*default' or 'serializable.*V1' across docs/ returns zero false claims; no regression to other isolation documentation

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
