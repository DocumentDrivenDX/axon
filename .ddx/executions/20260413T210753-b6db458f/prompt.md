# Execute Bead

You are running inside DDx's isolated execution worktree for this bead.
Your job is to make a best-effort attempt at the work described in the bead's Goals and Description, then commit the result. Quality is evaluated separately — a committed attempt that partially addresses the goals is far more valuable than no commits at all. Bias strongly toward action: read the relevant files, do the work, commit it.

## Bead
- ID: `axon-4c08a548`
- Title: Complete PRD isolation fix — 7 residual false serializable claims after axon-f63ac16f
- Labels: bug, p0, docs, isolation
- Base revision: `77a05f64fefe3db6006272a2613c4b143468d3db`
- Execution bundle: `.ddx/executions/20260413T210753-b6db458f`

## Description
Bead axon-f63ac16f updated the PRD isolation table and FEAT-008 but left at least 7 residual false or contradictory claims. The bead's acceptance criterion ('no false serializable.*V1 claims remaining') is NOT met.

Files and locations requiring fixes:

(1) docs/helix/01-frame/prd.md:137 — WORST: prose directly contradicts the updated table two lines below. Still reads: 'The default and recommended level is serializable; weaker levels are available as explicit opt-ins'. Rewrite to say snapshot isolation is the V1 default with write skew as a known gap.

(2) docs/helix/01-frame/principles.md:65 — P4 row still reads 'Serializable isolation | Concurrent transactions produce results equivalent to serial execution'. Update to Snapshot Isolation with a P1 note for serializability.

(3) docs/helix/01-frame/technical-requirements.md:84 — Storage Adapter Trait: 'Transactions: begin, commit, abort with serializable isolation'. This is misleading at the product level next to the updated PRD. Add a clarifying note: storage backends may provide serializable at the transaction level, but product-level isolation is snapshot isolation for V1.

(4) docs/helix/01-frame/technical-requirements.md:343 — Correctness Invariants table: 'Serializable isolation | Cycle test...'. Relabel as Snapshot Isolation or add P1 note.

(5) docs/helix/03-test/test-plan.md:82-84 — INV-002: 'Serializable Isolation (Cycle Test)' with claim 'No write skew, no phantom reads, no dirty reads'. The 'no write skew' claim is false for V1. Reframe as SI invariant (cycle test is still valid under SI) and demote write-skew prevention to P1.

(6) docs/helix/01-frame/features/FEAT-008-acid-transactions.md:63 — Conflict Resolution section: 'Under serializable isolation, the first transaction to commit wins'. Update to snapshot isolation.

(7) docs/helix/04-compete/competitive-analysis.md — Axon column claims 'Full ACID, serializable isolation'. Add V1 footnote: 'V1 provides snapshot isolation; serializable is P1'.

Do NOT edit: alignment review docs (point-in-time artifacts), SPIKE-001 (describes third-party backends), ADR-003/ADR-004 storage-layer references (accurate at the storage-transaction level).

## Acceptance Criteria
grep -ri 'serializable.*default' docs/ returns zero results; grep -ri 'no write skew' docs/ returns zero V1 claims (test-plan INV-002 reframed or demoted); prd.md §5 prose and table are internally consistent; principles.md P4 updated; FEAT-008 conflict resolution section updated; competitive-analysis has V1 footnote; cargo check still passes (docs only)

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
