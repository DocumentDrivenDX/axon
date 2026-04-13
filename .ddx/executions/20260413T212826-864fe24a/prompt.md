# Execute Bead

You are running inside DDx's isolated execution worktree for this bead.
Your job is to make a best-effort attempt at the work described in the bead's Goals and Description, then commit the result. Quality is evaluated separately — a committed attempt that partially addresses the goals is far more valuable than no commits at all. Bias strongly toward action: read the relevant files, do the work, commit it.

## Bead
- ID: `axon-d6acb336`
- Title: Enforce lifecycle initial state and improve invalid-state error in transition_lifecycle
- Labels: feat, p2, lifecycle, schema
- Base revision: `e608498efc82abc6cab56dfe77e66c9bbefb5b1b`
- Execution bundle: `.ddx/executions/20260413T212826-864fe24a`

## Description
Two gaps identified in the lifecycle implementation from axon-0f9c54cf:

(1) LifecycleDef.initial is parsed but never enforced. create_entity does not auto-populate the lifecycle field to initial, nor does it validate that an explicitly provided value is a known state. Add to create_entity: if the collection schema has a lifecycle definition for a field, and the entity data includes that field, validate that the value is a known state (key in transitions or matches initial). If the field is absent, auto-populate with LifecycleDef.initial. Document this behavior in a code comment.

(2) transition_lifecycle silently coerces a missing or non-string current state to empty string, then returns InvalidTransition { current_state: '', valid_transitions: [] } — confusing for callers. Add a new error variant AxonError::LifecycleStateInvalid { field: String, actual: Value } (or LifecycleFieldMissing { field: String } if the field is entirely absent). Return this error before the transition lookup when entity.data[lifecycle.field] is missing or not a string.

File: crates/axon-core/src/error.rs (new variants), crates/axon-api/src/handler.rs (create_entity + transition_lifecycle), crates/axon-server/src/gateway.rs (map new errors to 422), crates/axon-mcp/src/handlers.rs (map new errors in to_tool_error).

## Acceptance Criteria
Creating an entity without a lifecycle field auto-populates it with LifecycleDef.initial; creating an entity with a lifecycle field set to an unknown state returns a validation error; transition_lifecycle on an entity with a missing lifecycle field returns LifecycleFieldMissing (not InvalidTransition with empty string); transition_lifecycle on an entity with a non-string lifecycle field returns LifecycleStateInvalid; all new error variants map to 422 on HTTP; tests cover all four new cases

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
