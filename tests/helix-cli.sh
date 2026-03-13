#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
test_count=0

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

assert_eq() {
  local expected="$1"
  local actual="$2"
  local message="$3"
  if [[ "$expected" != "$actual" ]]; then
    printf 'expected:\n%s\nactual:\n%s\n' "$expected" "$actual" >&2
    fail "$message"
  fi
}

assert_contains() {
  local haystack="$1"
  local needle="$2"
  local message="$3"
  if [[ "$haystack" != *"$needle"* ]]; then
    printf 'missing substring: %s\nin:\n%s\n' "$needle" "$haystack" >&2
    fail "$message"
  fi
}

assert_file_exists() {
  local path="$1"
  local message="$2"
  [[ -f "$path" ]] || fail "$message"
}

assert_fails() {
  local message="$1"
  shift
  if "$@"; then
    fail "$message"
  fi
}

make_mock_bin() {
  local root="$1"
  mkdir -p "$root/bin" "$root/state"

  cat >"$root/bin/bd" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

state_root="${MOCK_STATE_ROOT:?}"
sandbox=0

while [[ "${1:-}" == "--sandbox" ]]; do
  sandbox=1
  shift
done

if [[ "${MOCK_REQUIRE_SANDBOX:-0}" == "1" && "$sandbox" -ne 1 ]]; then
  echo "mock bd expected --sandbox" >&2
  exit 1
fi

command="${1:-}"
shift || true

pop_first_line() {
  local file="$1"
  if [[ ! -s "$file" ]]; then
    return 1
  fi
  head -n1 "$file"
  tail -n +2 "$file" > "$file.tmp" || true
  mv "$file.tmp" "$file"
}

emit_ready_json() {
  local count="$1"
  printf '['
  local i
  for ((i = 0; i < count; i++)); do
    [[ "$i" -gt 0 ]] && printf ','
    printf '{"id":"bd-mock-%d"}' "$i"
  done
  printf ']\n'
}

case "$command" in
  init)
    mkdir -p .beads
    ;;
  status)
    if [[ "${MOCK_BD_STATUS:-ok}" == "fail" ]]; then
      echo "mock bd status failure" >&2
      exit 1
    fi
    cat <<'JSON'
{"summary":{"total_issues":0,"ready_issues":0}}
JSON
    ;;
  ready)
    count="$(pop_first_line "$state_root/ready-seq" || echo 0)"
    emit_ready_json "$count"
    ;;
  *)
    echo "unsupported mock bd command: $command" >&2
    exit 1
    ;;
esac
EOF

  cat >"$root/bin/codex" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

state_root="${MOCK_STATE_ROOT:?}"

if [[ "$*" != *"--dangerously-bypass-approvals-and-sandbox"* ]]; then
  echo "mock codex expected --dangerously-bypass-approvals-and-sandbox" >&2
  exit 1
fi

if [[ "$*" != *"--progress-cursor"* ]]; then
  echo "mock codex expected --progress-cursor" >&2
  exit 1
fi

if [[ "${MOCK_EXPECT_BEADS_DIRECT:-0}" == "1" && "${BEADS_DOLT_SERVER_MODE:-}" != "0" ]]; then
  echo "mock codex expected BEADS_DOLT_SERVER_MODE=0" >&2
  exit 1
fi

payload="$*"
mode="${MOCK_BACKFILL_MODE:-complete}"

record() {
  printf '%s\n' "$1" >> "$state_root/calls.log"
}

next_action() {
  local file="$state_root/next-actions"
  if [[ ! -s "$file" ]]; then
    echo STOP
    return
  fi
  head -n1 "$file"
  tail -n +2 "$file" > "$file.tmp" || true
  mv "$file.tmp" "$file"
}

case "$payload" in
  *"implementation action"*)
    record implement
    echo "implementation complete"
    ;;
  *"check action"*)
    record check
    action="$(next_action)"
    printf 'NEXT_ACTION: %s\n' "$action"
    echo "Recommended Command: mock"
    ;;
  *"alignment action"*)
    if [[ "${MOCK_ALIGN_FAIL:-0}" == "1" ]]; then
      echo "mock alignment failure" >&2
      exit 1
    fi
    record align
    echo "alignment complete"
    ;;
  *"backfill action"*)
    record backfill
    case "$mode" in
      complete)
        mkdir -p docs/helix/06-iterate/backfill-reports
        report="docs/helix/06-iterate/backfill-reports/BF-2099-01-01-repo.md"
        printf '# mock backfill report\n' > "$report"
        echo "Backfill Metadata"
        echo "BACKFILL_STATUS: COMPLETE"
        echo "BACKFILL_REPORT: $report"
        echo "RESEARCH_EPIC: bd-mock-backfill"
        ;;
      guidance)
        mkdir -p docs/helix/06-iterate/backfill-reports
        report="docs/helix/06-iterate/backfill-reports/BF-2099-01-01-guidance.md"
        printf '# mock guidance report\n' > "$report"
        echo "Backfill Metadata"
        echo "BACKFILL_STATUS: GUIDANCE_NEEDED"
        echo "BACKFILL_REPORT: $report"
        echo "RESEARCH_EPIC: bd-mock-backfill"
        ;;
      missing-report)
        echo "Backfill Metadata"
        echo "BACKFILL_STATUS: COMPLETE"
        echo "RESEARCH_EPIC: bd-mock-backfill"
        ;;
      blocked)
        mkdir -p docs/helix/06-iterate/backfill-reports
        report="docs/helix/06-iterate/backfill-reports/BF-2099-01-01-blocked.md"
        printf '# mock blocked report\n' > "$report"
        echo "Backfill Metadata"
        echo "BACKFILL_STATUS: BLOCKED"
        echo "BACKFILL_REPORT: $report"
        echo "RESEARCH_EPIC: bd-mock-backfill"
        ;;
      *)
        echo "unsupported mock backfill mode: $mode" >&2
        exit 1
        ;;
    esac
    ;;
  *)
    record other
    echo "mock codex"
    ;;
esac
EOF

  cat >"$root/bin/claude" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'claude mock %s\n' "$*"
EOF

  chmod +x "$root/bin/bd" "$root/bin/codex" "$root/bin/claude"
}

make_workspace() {
  local root
  root="$(mktemp -d)"
  mkdir -p "$root/work"
  make_mock_bin "$root"
  (
    cd "$root/work"
    git init -q
    mkdir -p .beads
  )
  echo "$root"
}

run_helix() {
  local root="$1"
  shift
  (
    cd "$root/work"
    HOME="$root/home" \
    PATH="$root/bin:$PATH" \
    MOCK_STATE_ROOT="$root/state" \
    HELIX_LIBRARY_ROOT="$repo_root/workflows/helix" \
    bash "$repo_root/scripts/helix" "$@"
  )
}

test_help() {
  local root
  root="$(make_workspace)"
  local output
  output="$(run_helix "$root" help)"
  assert_contains "$output" "helix run" "help should list run command"
  assert_contains "$output" "--review-every" "help should list review option"
  rm -rf "$root"
}

test_check_dry_run() {
  local root
  root="$(make_workspace)"
  local output
  output="$(run_helix "$root" check --dry-run repo)"
  assert_contains "$output" "codex --dangerously-bypass-approvals-and-sandbox exec --progress-cursor -C" "dry-run should print codex command"
  assert_contains "$output" "actions/check.md" "dry-run should reference check action"
  rm -rf "$root"
}

test_check_dry_run_uses_beads_direct_mode() {
  local root
  root="$(make_workspace)"
  local output
  output="$(BEADS_DOLT_SERVER_MODE=0 run_helix "$root" check --dry-run repo)"
  assert_contains "$output" "env BEADS_DOLT_SERVER_MODE=0 codex --dangerously-bypass-approvals-and-sandbox exec --progress-cursor -C" "dry-run should propagate Beads direct mode to Codex"
  assert_contains "$output" "This session must use Beads direct mode." "dry-run should tell the agent to stay off localhost Dolt server access"
  rm -rf "$root"
}

test_backfill_dry_run() {
  local root
  root="$(make_workspace)"
  local output
  output="$(run_helix "$root" backfill --dry-run repo)"
  assert_contains "$output" "actions/backfill-helix-docs.md" "backfill dry-run should reference backfill action"
  assert_contains "$output" "This is a writable live session" "backfill dry-run should assert writable live execution"
  assert_contains "$output" "BACKFILL_STATUS" "backfill dry-run should require machine-readable trailer"
  rm -rf "$root"
}

test_run_fails_without_beads_workspace() {
  local root
  root="$(make_workspace)"
  rm -rf "$root/work/.beads"

  local output
  if output="$(run_helix "$root" run 2>&1)"; then
    fail "run should fail when Beads is not initialized"
  fi

  assert_contains "$output" "Beads is not initialized" "run should report missing Beads workspace"
  assert_contains "$output" "bd init" "run should tell the operator to initialize Beads manually"
  [[ ! -f "$root/state/calls.log" ]] || fail "run should not invoke the agent when Beads is missing"
  rm -rf "$root"
}

test_implement_fails_when_beads_is_unhealthy() {
  local root
  root="$(make_workspace)"

  local output
  if output="$(MOCK_BD_STATUS=fail run_helix "$root" implement repo 2>&1)"; then
    fail "implement should fail when live Beads access is broken"
  fi

  assert_contains "$output" "failed to access live Beads tracker" "implement should report live Beads failure"
  assert_contains "$output" "mock bd status failure" "implement should surface the bd status error"
  assert_contains "$output" "refusing to auto-initialize or inspect backup/exported tracker data" "implement should refuse fallback behavior"
  [[ ! -f "$root/state/calls.log" ]] || fail "implement should not invoke the agent when Beads is unhealthy"
  rm -rf "$root"
}

test_run_stops_after_queue_drains() {
  local root
  root="$(make_workspace)"
  printf '1\n1\n0\n' > "$root/state/ready-seq"
  printf 'STOP\n' > "$root/state/next-actions"

  local output
  output="$(run_helix "$root" run 2>&1)"

  local calls
  calls="$(cat "$root/state/calls.log")"
  assert_eq $'implement\nimplement\ncheck' "$calls" "run should implement until drained, then check once"
  assert_contains "$output" "helix: stopping after check returned STOP" "run should report why it stopped after the queue drained"
  rm -rf "$root"
}

test_run_periodic_alignment() {
  local root
  root="$(make_workspace)"
  printf '1\n1\n0\n' > "$root/state/ready-seq"
  printf 'STOP\n' > "$root/state/next-actions"

  run_helix "$root" run --review-every 2 >/dev/null

  local calls
  calls="$(cat "$root/state/calls.log")"
  assert_eq $'implement\nimplement\nalign\ncheck' "$calls" "periodic alignment should run after configured cycles"
  rm -rf "$root"
}

test_run_auto_aligns_once() {
  local root
  root="$(make_workspace)"
  printf '0\n0\n' > "$root/state/ready-seq"
  printf 'ALIGN\nSTOP\n' > "$root/state/next-actions"

  run_helix "$root" run >/dev/null

  local calls
  calls="$(cat "$root/state/calls.log")"
  assert_eq $'check\nalign\ncheck' "$calls" "run should auto-align once when check returns ALIGN"
  rm -rf "$root"
}

test_run_reports_periodic_alignment_failure() {
  local root
  root="$(make_workspace)"
  printf '1\n' > "$root/state/ready-seq"

  local output
  if output="$(MOCK_ALIGN_FAIL=1 run_helix "$root" run --review-every 1 2>&1)"; then
    fail "run should fail when periodic alignment fails"
  fi

  assert_contains "$output" "mock alignment failure" "run should surface the alignment failure"
  assert_contains "$output" "helix: periodic alignment failed after 1 cycles" "run should report why the loop exited on periodic alignment failure"
  rm -rf "$root"
}

test_run_uses_beads_direct_mode_for_wrapper_and_agent() {
  local root
  root="$(make_workspace)"
  printf '0\n' > "$root/state/ready-seq"
  printf 'STOP\n' > "$root/state/next-actions"

  BEADS_DOLT_SERVER_MODE=0 \
  MOCK_REQUIRE_SANDBOX=1 \
  MOCK_EXPECT_BEADS_DIRECT=1 \
    run_helix "$root" run >/dev/null

  local calls
  calls="$(cat "$root/state/calls.log")"
  assert_eq $'check' "$calls" "run should keep Beads direct mode on for wrapper bd calls and the spawned agent"
  rm -rf "$root"
}

run_backfill_missing_report() {
  local root="$1"
  MOCK_BACKFILL_MODE=missing-report run_helix "$root" backfill repo >/dev/null
}

test_backfill_requires_report_marker() {
  local root
  root="$(make_workspace)"
  assert_fails "backfill should fail when the trailer omits BACKFILL_REPORT" \
    run_backfill_missing_report "$root"
  rm -rf "$root"
}

test_backfill_creates_report() {
  local root
  root="$(make_workspace)"
  local output
  output="$(run_helix "$root" backfill repo)"
  assert_contains "$output" "BACKFILL_STATUS: COMPLETE" "backfill should report completion"
  assert_file_exists "$root/work/docs/helix/06-iterate/backfill-reports/BF-2099-01-01-repo.md" "backfill should create the declared report"
  local calls
  calls="$(cat "$root/state/calls.log")"
  assert_eq $'backfill' "$calls" "backfill should invoke the backfill action once"
  rm -rf "$root"
}

test_installer_creates_launcher() {
  local root
  root="$(make_workspace)"
  (
    cd "$repo_root"
    HOME="$root/home" \
    CODEX_HOME="$root/codex-home" \
    CLAUDE_HOME="$root/claude-home" \
    bash scripts/install-local-skills.sh >/dev/null
  )

  [[ -x "$root/home/.local/bin/helix" ]] || fail "installer should create helix launcher"
  local launcher
  launcher="$(cat "$root/home/.local/bin/helix")"
  assert_contains "$launcher" 'exec bash "'$repo_root'/scripts/helix"' "launcher should invoke repo helix script through bash"
  rm -rf "$root"
}

run_test() {
  local name="$1"
  shift
  "$@"
  test_count=$((test_count + 1))
  echo "ok - $name"
}

run_test "help" test_help
run_test "check dry-run" test_check_dry_run
run_test "check dry-run beads direct mode" test_check_dry_run_uses_beads_direct_mode
run_test "backfill dry-run" test_backfill_dry_run
run_test "run requires beads workspace" test_run_fails_without_beads_workspace
run_test "implement fails on unhealthy beads" test_implement_fails_when_beads_is_unhealthy
run_test "run stops after drain" test_run_stops_after_queue_drains
run_test "periodic alignment" test_run_periodic_alignment
run_test "auto-align" test_run_auto_aligns_once
run_test "periodic alignment failure reason" test_run_reports_periodic_alignment_failure
run_test "run beads direct mode" test_run_uses_beads_direct_mode_for_wrapper_and_agent
run_test "backfill requires report marker" test_backfill_requires_report_marker
run_test "backfill creates report" test_backfill_creates_report
run_test "installer launcher" test_installer_creates_launcher

echo "PASS: ${test_count} helix wrapper tests"
