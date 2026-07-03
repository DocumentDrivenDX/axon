#!/usr/bin/env bash
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

CONSUMER="fake"
BACKEND="sqlite"
MODE="${AXON_CONSUMER_WORKLOAD_MODE:-pr}"
DRY_RUN=0
SELF_TEST=0
RUN_DIR_OVERRIDE=""

RUN_ID=""
RUN_DIR=""
LOG_DIR=""
COMMANDS_JSONL=""
SUMMARY_PATH=""
STARTED_AT=""
FINISHED_AT=""
AXON_SHA=""
ENDPOINT="${AXON_ENDPOINT:-http://127.0.0.1:0}"
TENANT="${AXON_TENANT:-consumer-workload}"
DATABASE="${AXON_DATABASE:-default}"
SCHEMA_HASH="${AXON_SCHEMA_HASH:-}"

STATUS="unknown"
CLASSIFICATION="unknown"
FAILURE_MESSAGE=""

declare -a CHILD_PIDS=()
declare -a COMMAND_NAMES=()
declare -a COMMAND_CWDS=()
declare -a COMMAND_SHELLS=()

usage() {
  cat <<'USAGE'
Usage: scripts/run-consumer-workloads.sh [options]

Options:
  --consumer NAME   Consumer workload to run. Currently supports: fake.
  --backend NAME    Backend under test, such as sqlite or postgres.
  --mode MODE       Gate mode: pr, nightly, or release. Defaults to pr.
  --dry-run         Write and print the commands/env without executing them.
  --self-test       Run the fake workload and built-in classifier checks.
  --run-dir PATH    Evidence directory. Defaults under target/consumer-workloads.
  -h, --help        Show this help.
USAGE
}

log() {
  printf '%s\n' "$*" >&2
}

die() {
  log "error: $*"
  exit 2
}

rfc3339_now() {
  date -u +"%Y-%m-%dT%H:%M:%SZ"
}

safe_name() {
  printf '%s' "$1" | tr -c 'A-Za-z0-9_.-' '-'
}

cleanup_processes() {
  local pid
  for pid in "${CHILD_PIDS[@]:-}"; do
    if kill -0 "$pid" 2>/dev/null; then
      kill "$pid" 2>/dev/null || true
    fi
  done
  for pid in "${CHILD_PIDS[@]:-}"; do
    wait "$pid" 2>/dev/null || true
  done
}

forget_child() {
  local target="$1"
  local kept=()
  local pid
  for pid in "${CHILD_PIDS[@]:-}"; do
    if [[ "$pid" != "$target" ]]; then
      kept+=("$pid")
    fi
  done
  CHILD_PIDS=("${kept[@]}")
}

on_interrupt() {
  trap - INT TERM
  cleanup_processes
  exit 130
}

trap cleanup_processes EXIT
trap on_interrupt INT TERM

parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --consumer)
        [[ $# -ge 2 ]] || die "--consumer requires a value"
        CONSUMER="$2"
        shift 2
        ;;
      --backend)
        [[ $# -ge 2 ]] || die "--backend requires a value"
        BACKEND="$2"
        shift 2
        ;;
      --mode)
        [[ $# -ge 2 ]] || die "--mode requires a value"
        MODE="$2"
        shift 2
        ;;
      --dry-run)
        DRY_RUN=1
        shift
        ;;
      --self-test)
        SELF_TEST=1
        shift
        ;;
      --run-dir)
        [[ $# -ge 2 ]] || die "--run-dir requires a value"
        RUN_DIR_OVERRIDE="$2"
        shift 2
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      *)
        die "unknown option: $1"
        ;;
    esac
  done
}

validate_options() {
  case "$MODE" in
    pr|nightly|release) ;;
    *) die "--mode must be pr, nightly, or release" ;;
  esac

  case "$BACKEND" in
    sqlite|postgres) ;;
    *) die "--backend must be sqlite or postgres" ;;
  esac

  if [[ "$SELF_TEST" -eq 1 ]]; then
    CONSUMER="fake"
  fi
}

setup_run_dir() {
  RUN_ID="consumer-workloads-$(date -u +%Y%m%dT%H%M%SZ)-$$"
  if [[ -n "$RUN_DIR_OVERRIDE" ]]; then
    RUN_DIR="$RUN_DIR_OVERRIDE"
  else
    RUN_DIR="${REPO_ROOT}/target/consumer-workloads/${RUN_ID}"
  fi

  LOG_DIR="${RUN_DIR}/logs"
  COMMANDS_JSONL="${RUN_DIR}/commands.jsonl"
  SUMMARY_PATH="${RUN_DIR}/summary.json"
  STARTED_AT="$(rfc3339_now)"
  AXON_SHA="$(git -C "$REPO_ROOT" rev-parse HEAD 2>/dev/null || printf 'unknown')"

  mkdir -p "$LOG_DIR"
  : > "$COMMANDS_JSONL"
}

append_command_json() {
  local name="$1"
  local cwd="$2"
  local shell_cmd="$3"
  local state="$4"
  local command_started_at="$5"
  local command_finished_at="$6"
  local exit_code="$7"
  local duration_ms="$8"
  local stdout_log="$9"
  local stderr_log="${10}"
  local executed_tests="${11}"
  local skipped_tests="${12}"

  COMMANDS_JSONL="$COMMANDS_JSONL" \
  COMMAND_NAME="$name" \
  COMMAND_CWD="$cwd" \
  COMMAND_SHELL="$shell_cmd" \
  COMMAND_STATE="$state" \
  COMMAND_STARTED_AT="$command_started_at" \
  COMMAND_FINISHED_AT="$command_finished_at" \
  COMMAND_EXIT_CODE="$exit_code" \
  COMMAND_DURATION_MS="$duration_ms" \
  COMMAND_STDOUT="$stdout_log" \
  COMMAND_STDERR="$stderr_log" \
  COMMAND_EXECUTED_TESTS="$executed_tests" \
  COMMAND_SKIPPED_TESTS="$skipped_tests" \
  COMMAND_ENV_KEYS="AXON_ENDPOINT,AXON_TENANT,AXON_DATABASE,AXON_BACKEND" \
  AXON_ENDPOINT="$ENDPOINT" \
  AXON_TENANT="$TENANT" \
  AXON_DATABASE="$DATABASE" \
  AXON_BACKEND="$BACKEND" \
  python3 - <<'PY'
import json
import os


def optional_int(value: str):
    if value in {"", "null"}:
        return None
    return int(value)


env_keys = [key for key in os.environ["COMMAND_ENV_KEYS"].split(",") if key]
entry = {
    "name": os.environ["COMMAND_NAME"],
    "cwd": os.environ["COMMAND_CWD"],
    "env": {key: os.environ.get(key) for key in env_keys},
    "env_keys": env_keys,
    "shell": os.environ["COMMAND_SHELL"],
    "state": os.environ["COMMAND_STATE"],
    "started_at": os.environ["COMMAND_STARTED_AT"] or None,
    "finished_at": os.environ["COMMAND_FINISHED_AT"] or None,
    "exit_code": optional_int(os.environ["COMMAND_EXIT_CODE"]),
    "duration_ms": optional_int(os.environ["COMMAND_DURATION_MS"]),
    "stdout": os.environ["COMMAND_STDOUT"] or None,
    "stdout_log": os.environ["COMMAND_STDOUT"] or None,
    "stderr": os.environ["COMMAND_STDERR"] or None,
    "stderr_log": os.environ["COMMAND_STDERR"] or None,
    "executed_tests": optional_int(os.environ["COMMAND_EXECUTED_TESTS"]),
    "skipped_tests": optional_int(os.environ["COMMAND_SKIPPED_TESTS"]),
}

with open(os.environ["COMMANDS_JSONL"], "a", encoding="utf-8") as handle:
    handle.write(json.dumps(entry, sort_keys=True))
    handle.write("\n")
PY
}

write_summary() {
  local exit_code="$1"
  FINISHED_AT="$(rfc3339_now)"

  SUMMARY_PATH="$SUMMARY_PATH" \
  COMMANDS_JSONL="$COMMANDS_JSONL" \
  RUN_ID="$RUN_ID" \
  RUN_DIR="$RUN_DIR" \
  MODE="$MODE" \
  CONSUMER="$CONSUMER" \
  BACKEND="$BACKEND" \
  STATUS="$STATUS" \
  CLASSIFICATION="$CLASSIFICATION" \
  AXON_SHA="$AXON_SHA" \
  ENDPOINT="$ENDPOINT" \
  TENANT="$TENANT" \
  DATABASE="$DATABASE" \
  SCHEMA_HASH="$SCHEMA_HASH" \
  STARTED_AT="$STARTED_AT" \
  FINISHED_AT="$FINISHED_AT" \
  EXIT_CODE="$exit_code" \
  FAILURE_MESSAGE="$FAILURE_MESSAGE" \
  python3 - <<'PY'
import json
import os
from pathlib import Path


def optional_string(value: str):
    return value if value else None


commands = []
commands_path = Path(os.environ["COMMANDS_JSONL"])
if commands_path.exists():
    for line in commands_path.read_text(encoding="utf-8").splitlines():
        if line.strip():
            commands.append(json.loads(line))

failure_message = os.environ["FAILURE_MESSAGE"]
failure = {"message": failure_message} if failure_message else None
summary = {
    "run_id": os.environ["RUN_ID"],
    "mode": os.environ["MODE"],
    "consumer": os.environ["CONSUMER"],
    "backend": os.environ["BACKEND"],
    "status": os.environ["STATUS"],
    "classification": os.environ["CLASSIFICATION"],
    "axon_sha": os.environ["AXON_SHA"],
    "consumer_sha": None,
    "consumer_dirty": False,
    "endpoint": os.environ["ENDPOINT"],
    "tenant": os.environ["TENANT"],
    "database": os.environ["DATABASE"],
    "schema_hash": optional_string(os.environ["SCHEMA_HASH"]),
    "started_at": os.environ["STARTED_AT"],
    "finished_at": os.environ["FINISHED_AT"],
    "exit_code": int(os.environ["EXIT_CODE"]),
    "commands": commands,
    "artifacts": [
        {"name": "run_directory", "path": os.environ["RUN_DIR"]},
        {"name": "commands_jsonl", "path": os.environ["COMMANDS_JSONL"]},
    ],
    "failure": failure,
}

Path(os.environ["SUMMARY_PATH"]).write_text(
    json.dumps(summary, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)
PY

  printf 'summary_json=%s\n' "$SUMMARY_PATH"
}

build_fake_workload() {
  COMMAND_NAMES=("fake-consumer-contract")
  COMMAND_CWDS=("$REPO_ROOT")
  COMMAND_SHELLS=("printf '%s\n' 'fake consumer workload executed' \"consumer=\${AXON_CONSUMER_NAME:-fake}\" \"backend=\${AXON_BACKEND:-sqlite}\" \"endpoint=\${AXON_ENDPOINT}\" \"\${AXON_FAKE_WORKLOAD_MARKER:-executed_tests=1 skipped_tests=0}\"; exit \"\${AXON_FAKE_WORKLOAD_EXIT:-0}\"")
}

build_workload() {
  COMMAND_NAMES=()
  COMMAND_CWDS=()
  COMMAND_SHELLS=()

  case "$CONSUMER" in
    fake)
      build_fake_workload
      return 0
      ;;
    *)
      STATUS="missing"
      CLASSIFICATION="missing_workload"
      FAILURE_MESSAGE="consumer workload '${CONSUMER}' is not available in this runner core"
      return 1
      ;;
  esac
}

detect_test_count() {
  local pattern="$1"
  local stdout_log="$2"
  local stderr_log="$3"
  local value

  value="$(python3 - "$pattern" "$stdout_log" "$stderr_log" <<'PY'
import re
import sys
from pathlib import Path

pattern = re.compile(sys.argv[1])
text = ""
for raw_path in sys.argv[2:]:
    path = Path(raw_path)
    if path.exists():
        text += path.read_text(encoding="utf-8", errors="replace")
match = pattern.search(text)
print(match.group(1) if match else "")
PY
)"
  printf '%s' "$value"
}

classify_command_result() {
  local exit_code="$1"
  local stdout_log="$2"
  local stderr_log="$3"

  if [[ "$exit_code" -eq 0 ]]; then
    printf 'none'
    return 0
  fi

  local combined
  combined="$(cat "$stdout_log" "$stderr_log" 2>/dev/null | tr '[:upper:]' '[:lower:]')"
  if [[ "$combined" == *"missing_workload"* ]]; then
    printf 'missing_workload'
  elif [[ "$combined" == *"contract_gap"* || "$combined" == *"fake transport"* || "$combined" == *"skipped"* ]]; then
    printf 'contract_gap'
  elif [[ "$combined" == *"axon_defect"* || "$combined" == *"axon panic"* || "$combined" == *"axon error"* ]]; then
    printf 'axon_defect'
  elif [[ "$combined" == *"infra"* || "$combined" == *"port in use"* || "$combined" == *"docker"* ]]; then
    printf 'infra'
  elif [[ "$combined" == *"consumer_defect"* ]]; then
    printf 'consumer_defect'
  else
    printf 'consumer_defect'
  fi
}

status_for_classification() {
  case "$1" in
    none) printf 'passed' ;;
    missing_workload) printf 'missing' ;;
    unknown) printf 'unknown' ;;
    *) printf 'failed' ;;
  esac
}

gate_exit_code() {
  case "${STATUS}:${CLASSIFICATION}:${MODE}" in
    passed:none:*) printf '0' ;;
    missing:missing_workload:release) printf '1' ;;
    missing:missing_workload:*) printf '0' ;;
    blocked:contract_gap:pr) printf '0' ;;
    *) printf '1' ;;
  esac
}

run_classifier_self_test() {
  local stdout_log="${LOG_DIR}/classifier-contract-gap.stdout.log"
  local stderr_log="${LOG_DIR}/classifier-contract-gap.stderr.log"
  printf '%s\n' "CONTRACT_GAP: fake adapter intentionally cannot force endpoint" > "$stdout_log"
  : > "$stderr_log"

  local classified
  classified="$(classify_command_result 42 "$stdout_log" "$stderr_log")"
  if [[ "$classified" != "contract_gap" ]]; then
    STATUS="unknown"
    CLASSIFICATION="unknown"
    FAILURE_MESSAGE="classifier self-test expected contract_gap, got ${classified}"
    write_summary 1 >/dev/null
    exit 1
  fi
}

record_dry_run_plan() {
  local idx="$1"
  local name="$2"
  local cwd="$3"
  local shell_cmd="$4"
  local safe
  safe="$(safe_name "$name")"
  local stdout_log="${LOG_DIR}/${idx}-${safe}.dry-run.log"
  local stderr_log="${LOG_DIR}/${idx}-${safe}.dry-run.stderr.log"
  local now
  now="$(rfc3339_now)"

  {
    printf 'DRY RUN\n'
    printf 'cwd=%s\n' "$cwd"
    printf 'env AXON_ENDPOINT=%s\n' "$ENDPOINT"
    printf 'env AXON_TENANT=%s\n' "$TENANT"
    printf 'env AXON_DATABASE=%s\n' "$DATABASE"
    printf 'env AXON_BACKEND=%s\n' "$BACKEND"
    printf 'command=%s\n' "$shell_cmd"
  } > "$stdout_log"
  : > "$stderr_log"

  cat "$stdout_log"
  append_command_json "$name" "$cwd" "$shell_cmd" "planned" "$now" "$now" "null" "0" "$stdout_log" "$stderr_log" "" ""
}

run_one_command() {
  local idx="$1"
  local name="$2"
  local cwd="$3"
  local shell_cmd="$4"
  local safe
  safe="$(safe_name "$name")"
  local stdout_log="${LOG_DIR}/${idx}-${safe}.stdout.log"
  local stderr_log="${LOG_DIR}/${idx}-${safe}.stderr.log"
  local command_started_at
  local command_finished_at
  local start_epoch
  local end_epoch
  local duration_ms

  command_started_at="$(rfc3339_now)"
  start_epoch="$(date +%s)"

  (
    cd "$cwd" || exit 127
    AXON_ENDPOINT="$ENDPOINT" \
    AXON_TENANT="$TENANT" \
    AXON_DATABASE="$DATABASE" \
    AXON_BACKEND="$BACKEND" \
    AXON_CONSUMER_NAME="$CONSUMER" \
    bash -lc "$shell_cmd"
  ) >"$stdout_log" 2>"$stderr_log" &

  local pid="$!"
  CHILD_PIDS+=("$pid")

  local command_exit=0
  wait "$pid" || command_exit="$?"
  forget_child "$pid"

  command_finished_at="$(rfc3339_now)"
  end_epoch="$(date +%s)"
  duration_ms="$(( (end_epoch - start_epoch) * 1000 ))"

  local executed_tests
  local skipped_tests
  executed_tests="$(detect_test_count 'executed_tests=([0-9]+)' "$stdout_log" "$stderr_log")"
  skipped_tests="$(detect_test_count 'skipped_tests=([0-9]+)' "$stdout_log" "$stderr_log")"

  append_command_json "$name" "$cwd" "$shell_cmd" "completed" "$command_started_at" "$command_finished_at" "$command_exit" "$duration_ms" "$stdout_log" "$stderr_log" "$executed_tests" "$skipped_tests"
  return "$command_exit"
}

run_commands() {
  local first_failure_classification="none"
  local idx

  for idx in "${!COMMAND_NAMES[@]}"; do
    if [[ "$DRY_RUN" -eq 1 ]]; then
      record_dry_run_plan "$idx" "${COMMAND_NAMES[$idx]}" "${COMMAND_CWDS[$idx]}" "${COMMAND_SHELLS[$idx]}"
      continue
    fi

    run_one_command "$idx" "${COMMAND_NAMES[$idx]}" "${COMMAND_CWDS[$idx]}" "${COMMAND_SHELLS[$idx]}"
    local command_exit="$?"
    if [[ "$command_exit" -ne 0 ]]; then
      first_failure_classification="$(classify_command_result "$command_exit" "${LOG_DIR}/${idx}-$(safe_name "${COMMAND_NAMES[$idx]}").stdout.log" "${LOG_DIR}/${idx}-$(safe_name "${COMMAND_NAMES[$idx]}").stderr.log")"
      CLASSIFICATION="$first_failure_classification"
      STATUS="$(status_for_classification "$CLASSIFICATION")"
      FAILURE_MESSAGE="command '${COMMAND_NAMES[$idx]}' failed"
      return 1
    fi
  done

  STATUS="passed"
  CLASSIFICATION="none"
  FAILURE_MESSAGE=""
  return 0
}

main() {
  parse_args "$@"
  validate_options
  setup_run_dir

  if [[ "$SELF_TEST" -eq 1 ]]; then
    run_classifier_self_test
  fi

  if ! build_workload; then
    local missing_exit
    missing_exit="$(gate_exit_code)"
    write_summary "$missing_exit"
    exit "$missing_exit"
  fi

  run_commands || true

  local final_exit
  final_exit="$(gate_exit_code)"
  write_summary "$final_exit"
  exit "$final_exit"
}

main "$@"
