# Execute Bead

You are running inside DDx's isolated execution worktree for this bead.
Your job is to make a best-effort attempt at the work described in the bead's Goals and Description, then commit the result. Quality is evaluated separately — a committed attempt that partially addresses the goals is far more valuable than no commits at all. Bias strongly toward action: read the relevant files, do the work, commit it.

## Bead
- ID: `axon-3009fe32`
- Title: Implement snapshot_entities handler method and GET /snapshot HTTP endpoint (FEAT-004 US-080)
- Labels: feat, p1, sync
- Base revision: `ad29baa21f98e41baba359f513e0e96957959b08`
- Execution bundle: `.ddx/executions/20260413T202544-6fd6467c`

## Description
Add to axon-api/src/request.rs: SnapshotRequest { collection_id, limit: Option<u32>, cursor: Option<String>, filter: Option<QueryFilter> }. Add to axon-api/src/response.rs: SnapshotResponse { entities: Vec<Entity>, next_page_cursor: Option<String>, audit_cursor: u64 }. Add handler method in axon-api/src/handler.rs: snapshot_entities(req: SnapshotRequest) -> Result<SnapshotResponse>. Implementation: read entities and current max audit_id from storage. For the in-memory backend, this is the current state at call time — no concurrent writers in single-threaded tests. V1 CAVEAT (must be documented in code comments and tests): multi-page snapshot consistency across pages is only guaranteed for in-memory single-threaded use. MemoryStorageAdapter has no storage-level snapshot isolation; concurrent writes between pages can cause a multi-page snapshot to reflect mixed state. This is acceptable for V1 (tests are single-threaded); production multi-page consistency requires storage-level snapshot support (deferred). Add HTTP route GET /collections/{name}/snapshot in axon-server parsing limit/cursor/filter query params.

## Acceptance Criteria
GET /snapshot returns { entities, next_page_cursor, audit_cursor }; create N entities, snapshot, create one more entity, tail from audit_cursor — new entity appears in tail but not in snapshot entities; paginated snapshot: all pages share the same audit_cursor (snapshot point); empty collection returns entities=[], next_page_cursor=null, valid audit_cursor; V1 caveat is present in a code comment on snapshot_entities and a note in the test file that multi-page consistency under concurrent writes is not guaranteed by this implementation

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
