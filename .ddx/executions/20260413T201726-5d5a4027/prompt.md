# Execute Bead

You are running inside DDx's isolated execution worktree for this bead.
Your job is to make a best-effort attempt at the work described in the bead's Goals and Description, then commit the result. Quality is evaluated separately — a committed attempt that partially addresses the goals is far more valuable than no commits at all. Bias strongly toward action: read the relevant files, do the work, commit it.

## Bead
- ID: `axon-fadfcecd`
- Title: Extend audit query handler for multi-collection tail with unified cursor (FEAT-003 US-079)
- Labels: feat, p1, audit
- Base revision: `d517d4580a7ba734c73121d4275b234f1b14434e`
- Execution bundle: `.ddx/executions/20260413T201726-5d5a4027`

## Description
Changes:
(1) axon-api/src/request.rs: add collection_ids: Vec<CollectionId> to QueryAuditRequest (the existing type — there is no ListAuditEntriesRequest). The existing collection: Option<CollectionId> field stays for backward compatibility. When collection_ids is non-empty, filter to those collections; when collection_ids is empty AND collection is None, return entries from all collections. The handler should merge both fields: effective set = collection_ids union (collection as a single-element vec if Some).

(2) axon-api/src/handler.rs query_audit: update logic to derive an effective Vec<CollectionId> from both fields (see above), pass to storage, and return entries ordered globally by audit_id ascending — NOT grouped by collection. The global ordering guarantee is non-negotiable: if collection A produces entry id=5 and collection B produces entry id=3, the response is [id=3(B), id=5(A)].

(3) axon-storage/src/adapter.rs StorageAdapter trait: update list_audit_entries (or query_audit_entries — use the actual method name in the trait) signature to accept &[CollectionId] (empty slice = all collections). Update memory.rs and sqlite.rs implementations accordingly.

(4) HTTP route: add GET /audit/tail (or extend the existing /audit/query route) accepting query params ?collections=name1,name2&after_id=<id>. Parse collections as a comma-split Vec<CollectionId>.

## Acceptance Criteria
GET /audit/tail?collections=beads,tasks&after_id=0 returns entries from both collections ordered by audit_id ascending (NOT grouped). INTERLEAVING TEST: create task (id=1), create bead (id=2), update task (id=3); multi-collection tail returns [id=1, id=2, id=3] in that order — not [tasks: id=1,3, beads: id=2]. Omitting collections= returns entries from all collections. after_id cursor: second request with after_id=<last_seen_id> returns only newer entries. Unknown collection name returns 400. Single-collection path still works via existing collection field on QueryAuditRequest (existing tests pass). TYPE NAME: no type named ListAuditEntriesRequest should appear in the implementation — the existing QueryAuditRequest is extended.

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
