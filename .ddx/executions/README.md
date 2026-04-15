# `.ddx/executions/` — execute-bead run bundles

Each subdirectory is one `ddx agent execute-bead` attempt. The directory
name is `<timestamp>-<random-id>` (e.g. `20260415T115435-3ea6086f`) and
matches the `attempt_id` written into the manifest.

Bundles are DDx's durable record of what the automation did; they are
checked in so reviewers, future agents, and post-mortems can reconstruct
every attempt byte-for-byte. See DDx TD-010 for the design rationale.

## Layout

| File / dir                  | Purpose                                                                                               | Tracked |
| --------------------------- | ----------------------------------------------------------------------------------------------------- | ------- |
| `manifest.json`             | Attempt metadata: bead id, base rev, requested harness/model, governing refs, bead title/description | yes     |
| `prompt.md`                 | The exact prompt handed to the agent (post-template, post-context-load)                              | yes     |
| `result.json`               | Outcome: exit code, result rev, outcome enum, error, tool-call summary                               | yes     |
| `usage.json`                | Token usage, model, provider, cost (when the harness reports it)                                    | yes     |
| `checks.json`               | Optional post-run check output (cargo check / test / clippy / fmt)                                   | yes     |
| `no_changes_rationale.txt`  | Written by the agent when it decides the bead is already satisfied                                   | yes     |
| `embedded/agent-*.jsonl`    | Per-iteration agent trace: every `session.start`, `llm.request`, `llm.response`, `tool.call` event  | **no**  |

## Why `embedded/` is gitignored

The per-iteration trace is the forensic-grade record of the run — every
message, every tool call, every model response. Individual traces can
exceed 50 MB; a single busy day produces hundreds of MB. Committing them
blows up clone size indefinitely and blows past GitHub's 50 MB per-file
warning.

They stay **local** under `.ddx/executions/*/embedded/` for analysis and
debugging. A forthcoming archive/mirror mechanism (see the DDx bead
tracker) will push them to a configurable object store so they remain
queryable without bloating the repo.

## Harness coverage caveat

As of 2026-04-15, only the embedded `ddx-agent` harness writes
`embedded/agent-*.jsonl`. The `claude` harness currently drops its
trace into `.ddx/agent-logs/` (the runner-wide default) instead of the
per-run `embedded/` dir — see the DDx harness-parity bead. Expect
`embedded/` to be empty for `harness: claude` runs until that's fixed.

## Reading a bundle

```bash
# Which bead did this run address?
jq '.bead_id, .bead.title, .requested' .ddx/executions/<id>/manifest.json

# Did it succeed?
jq '{outcome, exit_code, result_rev}' .ddx/executions/<id>/result.json

# What did it actually do (local-only)?
ls .ddx/executions/<id>/embedded/
```
