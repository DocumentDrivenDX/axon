# Execute Bead

You are running inside DDx's isolated execution worktree for this bead.
Your job is to make a best-effort attempt at the work described in the bead's Goals and Description, then commit the result. Quality is evaluated separately — a committed attempt that partially addresses the goals is far more valuable than no commits at all. Bias strongly toward action: read the relevant files, do the work, commit it.

## Bead
- ID: `axon-0f9c54cf`
- Title: Add transition_lifecycle method to AxonHandler (prereq for FEAT-015 lifecycle mutations)
- Labels: feat, p1, lifecycle, testability
- Base revision: `92e575dfd1aeb1a7e5a5c35f0f5dc30784698969`
- Execution bundle: `.ddx/executions/20260413T203751-85717fd1`

## Description
Prerequisites: axon-eec93920 (LifecycleDef in CollectionSchema) must be complete before this task. axon-api/src/handler.rs: no transition_lifecycle() method exists. Implement handler method only — no GraphQL wiring. Signature: transition_lifecycle(req: TransitionLifecycleRequest) -> Result<Entity>. TransitionLifecycleRequest { collection_id: CollectionId, entity_id: EntityId, lifecycle_name: String, target_state: String, expected_version: u64 }. Logic: (1) load collection schema; (2) look up schema.lifecycles[lifecycle_name] — if not found, return LifecycleNotFound error; (3) read current entity; (4) get current value of schema.lifecycles[lifecycle_name].field from entity.data (e.g., entity.data['status']); (5) look up allowed transitions for current state from LifecycleDef.transitions; (6) if target_state not in allowed set, return InvalidTransition { lifecycle_name, current_state, valid_transitions }; (7) if valid: update entity.data[lifecycle_field] = target_state via update_entity (standard OCC using expected_version).

## Acceptance Criteria
transition_lifecycle with valid target state updates the correct data field and returns updated entity; invalid target state returns InvalidTransition error with lifecycle_name, current_state, and valid_transitions list; LifecycleNotFound returned for unknown lifecycle_name; wrong expected_version returns version conflict; entity at final state (e.g., 'done' with transitions=[]) has empty valid_transitions in InvalidTransition error; tests use a collection schema with lifecycles populated via CollectionSchema.lifecycles (from axon-eec93920)

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
