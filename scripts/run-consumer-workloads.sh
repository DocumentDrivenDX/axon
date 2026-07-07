#!/usr/bin/env bash
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
DEFAULT_NEXIQ_WORKLOAD_PATH="$(cd "${REPO_ROOT}/.." && pwd)/nexiq"
DEFAULT_DDX_WORKLOAD_PATH="$(cd "${REPO_ROOT}/.." && pwd)/ddx"
DEFAULT_CAYCE_WORKLOAD_PATH="$(cd "${REPO_ROOT}/.." && pwd)/cayce"

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
CONSUMER_PATH=""
CONSUMER_SHA=""
CONSUMER_DIRTY=0
ENDPOINT="${AXON_ENDPOINT:-${NEXIQ_AXON_ENDPOINT:-http://127.0.0.1:0}}"
TENANT="${AXON_TENANT:-${NEXIQ_AXON_TENANT:-consumer-workload}}"
DATABASE="${AXON_DATABASE:-${NEXIQ_AXON_DATABASE:-default}}"
SCHEMA_HASH="${AXON_SCHEMA_HASH:-${NEXIQ_AXON_SCHEMA_HASH:-}}"
NEXIQ_WORKLOAD_PATH="${NEXIQ_WORKLOAD_PATH:-${AXON_NEXIQ_PATH:-${DEFAULT_NEXIQ_WORKLOAD_PATH}}}"
DDX_WORKLOAD_PATH="${DDX_WORKLOAD_PATH:-${AXON_DDX_PATH:-${DEFAULT_DDX_WORKLOAD_PATH}}}"
DDX_REAL_AXON_WORKLOAD_COMMAND="${DDX_REAL_AXON_WORKLOAD_COMMAND:-}"
CAYCE_WORKLOAD_PATH="${CAYCE_WORKLOAD_PATH:-${AXON_CAYCE_PATH:-${DEFAULT_CAYCE_WORKLOAD_PATH}}}"
CAYCE_WORKLOAD_COMMAND="${CAYCE_WORKLOAD_COMMAND:-}"

STATUS="unknown"
CLASSIFICATION="unknown"
FAILURE_MESSAGE=""
FORCE_RESULT_AFTER_PLAN=0
FORCED_STATUS=""
FORCED_CLASSIFICATION=""
FORCED_FAILURE_MESSAGE=""

declare -a CHILD_PIDS=()
declare -a COMMAND_NAMES=()
declare -a COMMAND_CWDS=()
declare -a COMMAND_SHELLS=()

usage() {
  cat <<'USAGE'
Usage: scripts/run-consumer-workloads.sh [options]

Options:
  --consumer NAME   Consumer workload to run. Currently supports: fake, nexiq, ddx, cayce.
  --backend NAME    Backend under test, such as sqlite or postgres.
  --mode MODE       Gate/workload mode: pr, nightly, release, contract, or e2e. Defaults to pr.
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
    pr|nightly|release|contract|e2e) ;;
    *) die "--mode must be pr, nightly, release, contract, or e2e" ;;
  esac

  case "$BACKEND" in
    sqlite|postgres) ;;
    *) die "--backend must be sqlite or postgres" ;;
  esac

  if [[ "$SELF_TEST" -eq 1 ]]; then
    CONSUMER="fake"
  fi
}

command_env_keys() {
  case "$CONSUMER" in
    nexiq)
      printf '%s' 'AXON_ENDPOINT,AXON_TENANT,AXON_DATABASE,AXON_SCHEMA_HASH,AXON_BACKEND,NEXIQ_AXON_ENDPOINT,NEXIQ_AXON_TENANT,NEXIQ_AXON_DATABASE,NEXIQ_AXON_SCHEMA_HASH'
      ;;
    ddx)
      printf '%s' 'AXON_ENDPOINT,AXON_TENANT,AXON_DATABASE,AXON_SCHEMA_HASH,AXON_BACKEND,DDX_AXON_ENDPOINT,DDX_AXON_TENANT,DDX_AXON_DATABASE,DDX_AXON_SCHEMA_HASH'
      ;;
    cayce)
      printf '%s' 'AXON_ENDPOINT,AXON_TENANT,AXON_DATABASE,AXON_SCHEMA_HASH,AXON_BACKEND,CAYCE_AXON_ENDPOINT,CAYCE_AXON_TENANT,CAYCE_AXON_DATABASE,CAYCE_AXON_SCHEMA_HASH'
      ;;
    *)
      printf '%s' 'AXON_ENDPOINT,AXON_TENANT,AXON_DATABASE,AXON_BACKEND'
      ;;
  esac
}

consumer_checkout_path() {
  case "$CONSUMER" in
    nexiq) printf '%s' "$NEXIQ_WORKLOAD_PATH" ;;
    ddx) printf '%s' "$DDX_WORKLOAD_PATH" ;;
    cayce) printf '%s' "$CAYCE_WORKLOAD_PATH" ;;
    *) printf '' ;;
  esac
}

resolve_consumer_git_state() {
  CONSUMER_PATH=""
  CONSUMER_SHA=""
  CONSUMER_DIRTY=0

  local path
  path="$(consumer_checkout_path)"
  if [[ -z "$path" || ! -d "$path" ]]; then
    return 0
  fi

  CONSUMER_PATH="$path"

  if ! git -C "$path" rev-parse --git-dir >/dev/null 2>&1; then
    return 0
  fi

  CONSUMER_SHA="$(git -C "$path" rev-parse HEAD 2>/dev/null || printf '')"
  if [[ -n "$(git -C "$path" status --porcelain 2>/dev/null)" ]]; then
    CONSUMER_DIRTY=1
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
  local test_counts_source="${13}"

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
  COMMAND_TEST_COUNTS_SOURCE="$test_counts_source" \
  COMMAND_ENV_KEYS="$(command_env_keys)" \
  AXON_ENDPOINT="$ENDPOINT" \
  AXON_TENANT="$TENANT" \
  AXON_DATABASE="$DATABASE" \
  AXON_SCHEMA_HASH="$SCHEMA_HASH" \
  AXON_BACKEND="$BACKEND" \
  NEXIQ_AXON_ENDPOINT="$ENDPOINT" \
  NEXIQ_AXON_TENANT="$TENANT" \
  NEXIQ_AXON_DATABASE="$DATABASE" \
  NEXIQ_AXON_SCHEMA_HASH="$SCHEMA_HASH" \
  DDX_AXON_ENDPOINT="$ENDPOINT" \
  DDX_AXON_TENANT="$TENANT" \
  DDX_AXON_DATABASE="$DATABASE" \
  DDX_AXON_SCHEMA_HASH="$SCHEMA_HASH" \
  CAYCE_AXON_ENDPOINT="$ENDPOINT" \
  CAYCE_AXON_TENANT="$TENANT" \
  CAYCE_AXON_DATABASE="$DATABASE" \
  CAYCE_AXON_SCHEMA_HASH="$SCHEMA_HASH" \
  python3 - <<'PY'
import json
import os


def optional_int(value: str):
    if value in {"", "null"}:
        return None
    return int(value)


def optional_string(value: str):
    return value if value else None


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
    "test_counts_source": optional_string(os.environ["COMMAND_TEST_COUNTS_SOURCE"]),
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
  CONSUMER_PATH="$CONSUMER_PATH" \
  CONSUMER_SHA="$CONSUMER_SHA" \
  CONSUMER_DIRTY="$CONSUMER_DIRTY" \
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
    "consumer_path": optional_string(os.environ["CONSUMER_PATH"]),
    "consumer_sha": optional_string(os.environ["CONSUMER_SHA"]),
    "consumer_dirty": os.environ["CONSUMER_DIRTY"] == "1",
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

build_nexiq_workload() {
  if [[ ! -d "$NEXIQ_WORKLOAD_PATH" && "$DRY_RUN" -ne 1 ]]; then
    STATUS="missing"
    CLASSIFICATION="missing_workload"
    FAILURE_MESSAGE="Nexiq workload checkout is missing at ${NEXIQ_WORKLOAD_PATH}"
    return 1
  fi

  case "$MODE" in
    pr|contract)
      COMMAND_NAMES=("nexiq-contract")
      COMMAND_CWDS=("$NEXIQ_WORKLOAD_PATH")
      COMMAND_SHELLS=("RUN_INTEGRATION=1 bun test tests/contract/axon-contract.spec.ts")
      ;;
    nightly|release|e2e)
      COMMAND_NAMES=("nexiq-e2e")
      COMMAND_CWDS=("$NEXIQ_WORKLOAD_PATH")
      COMMAND_SHELLS=("bun run scripts/run-e2e-real-axon.ts")
      ;;
  esac
}

build_ddx_workload() {
  if [[ -n "$DDX_REAL_AXON_WORKLOAD_COMMAND" ]]; then
    if [[ ! -d "$DDX_WORKLOAD_PATH" && "$DRY_RUN" -ne 1 ]]; then
      STATUS="missing"
      CLASSIFICATION="missing_workload"
      FAILURE_MESSAGE="DDx workload checkout is missing at ${DDX_WORKLOAD_PATH}"
      return 1
    fi

    COMMAND_NAMES=("ddx-real-axon-contract")
    COMMAND_CWDS=("$DDX_WORKLOAD_PATH")
    COMMAND_SHELLS=("$DDX_REAL_AXON_WORKLOAD_COMMAND")
    return 0
  fi

  STATUS="blocked"
  CLASSIFICATION="contract_gap"
  FAILURE_MESSAGE="DDx real-Axon workload contract is not configured; existing DDx Axon paths are experimental and fake transports, in-process emulation, or JSONL-shaped writes are not passing real workloads"

  if [[ "$DRY_RUN" -eq 1 ]]; then
    FORCE_RESULT_AFTER_PLAN=1
    FORCED_STATUS="$STATUS"
    FORCED_CLASSIFICATION="$CLASSIFICATION"
    FORCED_FAILURE_MESSAGE="$FAILURE_MESSAGE"
    COMMAND_NAMES=("ddx-future-real-axon-proof")
    COMMAND_CWDS=("$DDX_WORKLOAD_PATH")
    COMMAND_SHELLS=("printf '%s\n' 'future proof: configure DDX_REAL_AXON_WORKLOAD_COMMAND with a DDx command that targets the injected AXON_ENDPOINT' 'future proof: prove real Axon wire calls with real_axon_wire_calls=1 or equivalent captured request-log evidence' 'future proof: require nonzero executed tests or an explicit postcondition query' 'contract_gap: fake GraphQL transports, in-process emulation, JSONL-shaped writes, and skipped backend tests do not count as success'")
    return 0
  fi

  return 1
}

build_cayce_workload() {
  if [[ ! -e "$CAYCE_WORKLOAD_PATH" ]]; then
    STATUS="missing"
    CLASSIFICATION="missing_workload"
    FAILURE_MESSAGE="Cayce workload source or export is missing at ${CAYCE_WORKLOAD_PATH}"
    return 1
  fi

  if [[ -z "$CAYCE_WORKLOAD_COMMAND" ]]; then
    STATUS="blocked"
    CLASSIFICATION="contract_gap"
    FAILURE_MESSAGE="Cayce workload source exists at ${CAYCE_WORKLOAD_PATH}, but no real workload command is configured; refusing to synthesize fixtures"
    return 1
  fi

  COMMAND_NAMES=("cayce-contract")
  COMMAND_CWDS=("$CAYCE_WORKLOAD_PATH")
  COMMAND_SHELLS=("$CAYCE_WORKLOAD_COMMAND")
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
    nexiq)
      build_nexiq_workload
      return "$?"
      ;;
    ddx)
      build_ddx_workload
      return "$?"
      ;;
    cayce)
      build_cayce_workload
      return "$?"
      ;;
    *)
      STATUS="missing"
      CLASSIFICATION="missing_workload"
      FAILURE_MESSAGE="consumer workload '${CONSUMER}' is not available in this runner core"
      return 1
      ;;
  esac
}

detect_test_counts() {
  local stdout_log="$1"
  local stderr_log="$2"
  local mode="${3:-$MODE}"
  local value

  value="$(python3 - "$mode" "$stdout_log" "$stderr_log" <<'PY'
import json
import re
import sys
from pathlib import Path

mode = sys.argv[1]
text = ""
for raw_path in sys.argv[2:]:
    path = Path(raw_path)
    if path.exists():
        text += path.read_text(encoding="utf-8", errors="replace") + "\n"


def emit(source: str, executed: str = "", skipped: str = "") -> None:
    print(f"{source}|{executed}|{skipped}")


for raw_line in text.splitlines():
    line = raw_line.strip()
    if not line or not line.startswith("{") or not line.endswith("}"):
        continue
    try:
        payload = json.loads(line)
    except json.JSONDecodeError:
        continue
    if not isinstance(payload, dict):
        continue
    if "executed_tests" not in payload or "skipped_tests" not in payload:
        continue
    try:
        executed = int(payload["executed_tests"])
        skipped = int(payload["skipped_tests"])
    except (TypeError, ValueError):
        continue
    emit("native", str(executed), str(skipped))
    raise SystemExit

lower = text.lower()
executed = None
skipped = None

match = re.search(r"executed_tests=([0-9]+)", text)
if match:
    executed = int(match.group(1))
elif re.search(r"\b(no tests? (?:found|ran|run)|0 tests? (?:found|ran|run)|ran\s+0\s+tests?)\b", lower):
    executed = 0
else:
    pass_counts = [int(match.group(1)) for match in re.finditer(r"(?m)^\s*(\d+)\s+pass(?:ed)?\b", lower)]
    if pass_counts:
        executed = sum(pass_counts)
    else:
        match = re.search(r"\b(\d+)\s+tests?\s+passed\b", lower)
        if match:
            executed = int(match.group(1))
        else:
            match = re.search(r"\bpassed\s+(\d+)\s+tests?\b", lower)
        if match:
            executed = int(match.group(1))

match = re.search(r"skipped_tests=([0-9]+)", text)
if match:
    skipped = int(match.group(1))
else:
    skip_counts = [int(match.group(1)) for match in re.finditer(r"(?m)^\s*(\d+)\s+skip(?:ped)?\b", lower)]
    skip_counts.extend(
        int(match.group(1))
        for match in re.finditer(r"\b(\d+)\s+tests?\s+skipped\b", lower)
    )
    if skip_counts:
        skipped = sum(skip_counts)
    elif re.search(r"\bskip(?:ped|s|ping)?\b", lower):
        skipped = 1

if executed is None and skipped is None:
    emit("none")
else:
    emit(
        "heuristic",
        "" if executed is None else str(executed),
        "" if skipped is None else str(skipped),
    )
PY
)"
  printf '%s' "$value"
}

logs_report_no_tests() {
  local stdout_log="$1"
  local stderr_log="$2"

  python3 - "$stdout_log" "$stderr_log" <<'PY'
import re
import sys
from pathlib import Path

text = ""
for raw_path in sys.argv[1:]:
    path = Path(raw_path)
    if path.exists():
        text += path.read_text(encoding="utf-8", errors="replace") + "\n"

lower = text.lower()
if re.search(r"\b(no tests? (?:found|ran|run)|0 tests? (?:found|ran|run)|ran\s+0\s+tests?)\b", lower):
    print("1")
else:
    print("0")
PY
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
  elif [[ "$combined" == *"contract_gap"* || "$combined" == *"fake transport"* || "$combined" == *"skipped"* || "$combined" == *"no tests"* || "$combined" == *"0 tests"* ]]; then
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
    missing:missing_workload:release|missing:missing_workload:contract|missing:missing_workload:e2e) printf '1' ;;
    missing:missing_workload:*) printf '0' ;;
    blocked:contract_gap:pr) printf '0' ;;
    blocked:contract_gap:contract) [[ "$DRY_RUN" -eq 1 ]] && printf '0' || printf '1' ;;
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
    printf 'env AXON_SCHEMA_HASH=%s\n' "$SCHEMA_HASH"
    printf 'env AXON_BACKEND=%s\n' "$BACKEND"
    if [[ "$CONSUMER" == "nexiq" ]]; then
      printf 'env NEXIQ_AXON_ENDPOINT=%s\n' "$ENDPOINT"
      printf 'env NEXIQ_AXON_TENANT=%s\n' "$TENANT"
      printf 'env NEXIQ_AXON_DATABASE=%s\n' "$DATABASE"
      printf 'env NEXIQ_AXON_SCHEMA_HASH=%s\n' "$SCHEMA_HASH"
    elif [[ "$CONSUMER" == "ddx" ]]; then
      printf 'env DDX_AXON_ENDPOINT=%s\n' "$ENDPOINT"
      printf 'env DDX_AXON_TENANT=%s\n' "$TENANT"
      printf 'env DDX_AXON_DATABASE=%s\n' "$DATABASE"
      printf 'env DDX_AXON_SCHEMA_HASH=%s\n' "$SCHEMA_HASH"
      if [[ "$FORCE_RESULT_AFTER_PLAN" -eq 1 ]]; then
        printf 'classification=%s\n' "$FORCED_CLASSIFICATION"
        printf 'status=%s\n' "$FORCED_STATUS"
        printf 'note=%s\n' "$FORCED_FAILURE_MESSAGE"
      fi
    elif [[ "$CONSUMER" == "cayce" ]]; then
      printf 'env CAYCE_AXON_ENDPOINT=%s\n' "$ENDPOINT"
      printf 'env CAYCE_AXON_TENANT=%s\n' "$TENANT"
      printf 'env CAYCE_AXON_DATABASE=%s\n' "$DATABASE"
      printf 'env CAYCE_AXON_SCHEMA_HASH=%s\n' "$SCHEMA_HASH"
    fi
    printf 'command=%s\n' "$shell_cmd"
  } > "$stdout_log"
  : > "$stderr_log"

  cat "$stdout_log"
  append_command_json "$name" "$cwd" "$shell_cmd" "planned" "$now" "$now" "null" "0" "$stdout_log" "$stderr_log" "" "" ""
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
    AXON_SCHEMA_HASH="$SCHEMA_HASH" \
    AXON_BACKEND="$BACKEND" \
    NEXIQ_AXON_ENDPOINT="$ENDPOINT" \
    NEXIQ_AXON_TENANT="$TENANT" \
    NEXIQ_AXON_DATABASE="$DATABASE" \
    NEXIQ_AXON_SCHEMA_HASH="$SCHEMA_HASH" \
    DDX_AXON_ENDPOINT="$ENDPOINT" \
    DDX_AXON_TENANT="$TENANT" \
    DDX_AXON_DATABASE="$DATABASE" \
    DDX_AXON_SCHEMA_HASH="$SCHEMA_HASH" \
    CAYCE_AXON_ENDPOINT="$ENDPOINT" \
    CAYCE_AXON_TENANT="$TENANT" \
    CAYCE_AXON_DATABASE="$DATABASE" \
    CAYCE_AXON_SCHEMA_HASH="$SCHEMA_HASH" \
    AXON_CONSUMER_NAME="$CONSUMER" \
    bash -c "$shell_cmd"
  ) >"$stdout_log" 2>"$stderr_log" &

  local pid="$!"
  CHILD_PIDS+=("$pid")

  local command_exit=0
  wait "$pid" || command_exit="$?"
  forget_child "$pid"

  command_finished_at="$(rfc3339_now)"
  end_epoch="$(date +%s)"
  duration_ms="$(( (end_epoch - start_epoch) * 1000 ))"

  local test_counts
  local test_counts_source
  local executed_tests
  local skipped_tests
  test_counts="$(detect_test_counts "$stdout_log" "$stderr_log")"
  IFS='|' read -r test_counts_source executed_tests skipped_tests <<< "$test_counts"

  append_command_json "$name" "$cwd" "$shell_cmd" "completed" "$command_started_at" "$command_finished_at" "$command_exit" "$duration_ms" "$stdout_log" "$stderr_log" "$executed_tests" "$skipped_tests" "$test_counts_source"
  return "$command_exit"
}

release_traffic_exception_applies() {
  local exception_consumer="${AXON_RELEASE_TRAFFIC_PROOF_EXCEPTION:-}"
  local exception_reason="${AXON_RELEASE_TRAFFIC_PROOF_EXCEPTION_REASON:-}"

  [[ -n "$exception_consumer" && "$exception_consumer" == "$CONSUMER" && -n "$exception_reason" ]]
}

validate_release_traffic_proof() {
  local name="$1"
  local stdout_log="$2"
  local stderr_log="$3"

  if [[ "$MODE" != "release" ]]; then
    return 0
  fi

  if release_traffic_exception_applies; then
    return 0
  fi

  local combined
  combined="$(cat "$stdout_log" "$stderr_log" 2>/dev/null | tr '[:upper:]' '[:lower:]')"

  if [[ "$combined" == *"real_axon_wire_calls=1"* || "$combined" == *"axon_request_log=1"* || "$combined" == *"axon_postcondition_query=1"* ]]; then
    return 0
  fi

  STATUS="failed"
  CLASSIFICATION="contract_gap"
  FAILURE_MESSAGE="command '${name}' did not provide release-mode proof of real Axon traffic (captured request log, postcondition query, or an explicit Phase-0 exception); a command that ignores AXON_ENDPOINT cannot pass release qualification"
  return 1
}

validate_successful_command() {
  local idx="$1"
  local name="$2"
  local shell_cmd="$3"
  local stdout_log="$4"
  local stderr_log="$5"

  local test_counts
  local test_counts_source
  local executed_tests
  local skipped_tests
  test_counts="$(detect_test_counts "$stdout_log" "$stderr_log")"
  IFS='|' read -r test_counts_source executed_tests skipped_tests <<< "$test_counts"

  if [[ "$CONSUMER" == "nexiq" ]]; then
    case "$MODE" in
      pr|contract) ;;
      *) return 0 ;;
    esac

    if [[ "$shell_cmd" != *"RUN_INTEGRATION=1"* ]]; then
      STATUS="failed"
      CLASSIFICATION="contract_gap"
      FAILURE_MESSAGE="command '${name}' did not force RUN_INTEGRATION=1"
      return 1
    fi
  elif [[ "$CONSUMER" == "ddx" ]]; then
    local combined
    combined="$(cat "$stdout_log" "$stderr_log" 2>/dev/null | tr '[:upper:]' '[:lower:]')"
    if [[ "$combined" == *"fake transport"* || "$combined" == *"fake graphql"* || "$combined" == *"in-process emulation"* || "$combined" == *"jsonl-shaped"* ]]; then
      STATUS="failed"
      CLASSIFICATION="contract_gap"
      FAILURE_MESSAGE="command '${name}' used fake transport or local emulation evidence"
      return 1
    fi

    if [[ "$combined" != *"real_axon_wire_calls=1"* && "$combined" != *"axon_request_log=1"* ]]; then
      STATUS="failed"
      CLASSIFICATION="contract_gap"
      FAILURE_MESSAGE="command '${name}' did not provide evidence of real Axon wire calls"
      return 1
    fi
  fi

  # Release mode only trusts native machine-readable counts, not heuristic stdout markers.
  if [[ "$MODE" == "release" && "$test_counts_source" != "native" ]]; then
    STATUS="failed"
    CLASSIFICATION="contract_gap"
    FAILURE_MESSAGE="command '${name}' did not provide native machine-readable test counts in release mode"
    return 1
  fi

  if ! validate_release_traffic_proof "$name" "$stdout_log" "$stderr_log"; then
    return 1
  fi

  if [[ "$(logs_report_no_tests "$stdout_log" "$stderr_log")" == "1" && "$test_counts_source" != "native" ]]; then
    STATUS="failed"
    CLASSIFICATION="contract_gap"
    FAILURE_MESSAGE="command '${name}' reported that no integration tests ran"
    return 1
  fi

  if [[ -n "$skipped_tests" && "$skipped_tests" -gt 0 ]]; then
    STATUS="failed"
    CLASSIFICATION="contract_gap"
    FAILURE_MESSAGE="command '${name}' reported skipped integration tests"
    return 1
  fi

  if [[ -z "$executed_tests" || "$executed_tests" -eq 0 ]]; then
    STATUS="failed"
    CLASSIFICATION="contract_gap"
    FAILURE_MESSAGE="command '${name}' did not provide evidence of executed integration tests"
    return 1
  fi

  printf '%s\n' "validated ${name}: executed_tests=${executed_tests} skipped_tests=${skipped_tests:-0}" >&2
  return 0
}

run_commands() {
  local first_failure_classification="none"
  local idx
  local stdout_log
  local stderr_log

  for idx in "${!COMMAND_NAMES[@]}"; do
    if [[ "$DRY_RUN" -eq 1 ]]; then
      record_dry_run_plan "$idx" "${COMMAND_NAMES[$idx]}" "${COMMAND_CWDS[$idx]}" "${COMMAND_SHELLS[$idx]}"
      continue
    fi

    run_one_command "$idx" "${COMMAND_NAMES[$idx]}" "${COMMAND_CWDS[$idx]}" "${COMMAND_SHELLS[$idx]}"
    local command_exit="$?"
    stdout_log="${LOG_DIR}/${idx}-$(safe_name "${COMMAND_NAMES[$idx]}").stdout.log"
    stderr_log="${LOG_DIR}/${idx}-$(safe_name "${COMMAND_NAMES[$idx]}").stderr.log"
    if [[ "$command_exit" -ne 0 ]]; then
      first_failure_classification="$(classify_command_result "$command_exit" "$stdout_log" "$stderr_log")"
      CLASSIFICATION="$first_failure_classification"
      STATUS="$(status_for_classification "$CLASSIFICATION")"
      FAILURE_MESSAGE="command '${COMMAND_NAMES[$idx]}' failed"
      return 1
    fi

    if ! validate_successful_command "$idx" "${COMMAND_NAMES[$idx]}" "${COMMAND_SHELLS[$idx]}" "$stdout_log" "$stderr_log"; then
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
  resolve_consumer_git_state

  if [[ "$SELF_TEST" -eq 1 ]]; then
    run_classifier_self_test
  fi

  if ! build_workload; then
    local missing_exit
    missing_exit="$(gate_exit_code)"
    write_summary "$missing_exit"
    exit "$missing_exit"
  fi

  if [[ "$MODE" == "release" && "$CONSUMER_DIRTY" -eq 1 ]]; then
    STATUS="failed"
    CLASSIFICATION="consumer_dirty"
    FAILURE_MESSAGE="consumer checkout at ${CONSUMER_PATH} is dirty; release qualification requires a clean, reproducible checkout"
    local dirty_exit
    dirty_exit="$(gate_exit_code)"
    write_summary "$dirty_exit"
    exit "$dirty_exit"
  fi

  run_commands || true

  if [[ "$FORCE_RESULT_AFTER_PLAN" -eq 1 ]]; then
    STATUS="$FORCED_STATUS"
    CLASSIFICATION="$FORCED_CLASSIFICATION"
    FAILURE_MESSAGE="$FORCED_FAILURE_MESSAGE"
  fi

  local final_exit
  final_exit="$(gate_exit_code)"
  write_summary "$final_exit"
  exit "$final_exit"
}

main "$@"
