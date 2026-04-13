# Execute Bead

You are running inside DDx's isolated execution worktree for this bead.
Your job is to make a best-effort attempt at the work described in the bead's Goals and Description, then commit the result. Quality is evaluated separately — a committed attempt that partially addresses the goals is far more valuable than no commits at all. Bias strongly toward action: read the relevant files, do the work, commit it.

## Bead
- ID: `axon-118f54c6`
- Title: Add schema_version field to Entity for per-entity schema tracking (FEAT-017)
- Parent: `axon-b5a5cc01`
- Labels: feat, p1, schema-evolution
- Base revision: `5603349f20c8c3e90bc55fec50b7a556ef45a405`
- Execution bundle: `.ddx/executions/20260413T200452-f307acfa`

## Description
Add schema_version: u32 to the Entity struct in axon-core/src/types.rs. Set at create time to the collection's current schema version (retrieved from storage.get_schema_version). On update/patch, do NOT change schema_version — it only changes when the entity is explicitly revalidated via the revalidate_collection handler. Surface in all entity API responses (automatic since responses include the full Entity struct). The revalidate handler must update schema_version to current version for each re-validated entity it writes back. SERDE COMPATIBILITY: Add #[serde(default)] to schema_version on the Entity struct so entities persisted before this field was added (when the field is absent in the stored JSON) deserialize successfully with schema_version=0 instead of returning a deserialization error.

## Acceptance Criteria
Entity created with schema at version N has schema_version=N; updating an entity does not change schema_version; after revalidate_collection, each entity's schema_version equals current schema version; schema_version present in all API response types; BACKWARD COMPATIBILITY TEST: manually construct a JSON entity blob WITHOUT schema_version field; deserialize into Entity — must succeed with schema_version=0 (no error); this simulates entities stored before this migration

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
