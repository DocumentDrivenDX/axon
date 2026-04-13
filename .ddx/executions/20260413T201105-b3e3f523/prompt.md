# Execute Bead

You are running inside DDx's isolated execution worktree for this bead.
Your job is to make a best-effort attempt at the work described in the bead's Goals and Description, then commit the result. Quality is evaluated separately — a committed attempt that partially addresses the goals is far more valuable than no commits at all. Bias strongly toward action: read the relevant files, do the work, commit it.

## Bead
- ID: `axon-be2a06cc`
- Title: Implement KafkaCdcSink with rdkafka producer — replace in-memory stub (FEAT-021)
- Parent: `axon-b5a5cc01`
- Labels: feat, p1, cdc
- Base revision: `9ea6a6b8befc64bcab60476126242f705b37b26a`
- Execution bundle: `.ddx/executions/20260413T201105-b3e3f523`

## Description
File: axon-audit/src/cdc.rs. KafkaCdcSink at line 228 buffers events in memory with a comment 'actual rdkafka integration is deferred'. Changes: (1) Add rdkafka = { version = '...', optional = true } to axon-audit/Cargo.toml with a 'kafka' feature flag (default = []). (2) Add 'kafka' to the CI test command — check the project's CI config (likely .github/workflows/*.yml or a Makefile/justfile) and add '--features kafka' to the axon-audit test invocation. If no CI config exists yet, add a note in a comment at the top of cdc.rs documenting that 'cargo test --features kafka' is required to test Kafka integration. (3) Replace the Vec<(topic, partition_key, json)> buffer with rdkafka::producer::FutureProducer behind #[cfg(feature='kafka')]. (4) Implement produce(): serialize CdcEvent to Debezium JSON envelope, call producer.send() with topic from KafkaConfig, entity_id as message key. (5) Error handling: producer errors return CdcError::ProducerError — do not panic. (6) Tests: use a MockProducer struct (small internal trait with sent: Arc<Mutex<Vec<...>>>) rather than a real Kafka broker.

## Acceptance Criteria
KafkaCdcSink with mock producer: entity create event produces message on correct topic with entity_id as key and valid Debezium envelope (op=c); update event produces op=u; delete event produces op=d; producer error returns CdcError not a panic; 'cargo test --features kafka' runs the Kafka tests; 'cargo test' without the feature still compiles and passes (existing tests unaffected); CI config updated (or comment added documenting the feature flag)

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
