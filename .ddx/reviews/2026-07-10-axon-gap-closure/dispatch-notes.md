# Harness Dispatch Notes

- Round 1 Gemini auto-routing failed because the harness requires an explicit
  model; the explicit `gemini-2.5-pro` retry was unsupported and advertised no
  supported models. No Gemini verdict was counted.
- Round 2 Fiz failed first on a refused local model endpoint, then on missing
  OpenRouter credentials. No round-2 Fiz verdict was counted.
- The first round-3 Codex attempt could not read `target-v2.md` because `/tmp`
  had exhausted its inode pool. Its reviewability BLOCK was infrastructure-only
  and was not counted as a content verdict.
- `/tmp` contained abandoned DDx temp homes from the prior day with no open
  file handles. Two small stale homes were removed, restoring approximately
  10,000 inodes for the final self-contained review.
