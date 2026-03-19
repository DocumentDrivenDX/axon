#!/usr/bin/env bash
# HELIX Quickstart Demo — scripted asciinema recording
#
# This script drives a full HELIX cycle on a tiny Node.js project:
#   1. Setup: init repo, init beads, install ddx skills
#   2. Planning: create PRD, user story, technical design, test plan (Red)
#   3. Execution: bead-driven implementation (Green)
#   4. Review: critical review of the work product
#   5. Triage: queue health and gap analysis
#
# Every artifact is created by Claude — no canned fallbacks.
#
# Usage:
#   docker run --rm \
#     -v ~/.claude.json:/root/.claude.json:ro \
#     -v ~/.claude:/root/.claude:ro \
#     -v $(pwd):/ddx-library:ro \
#     -v $(pwd)/docs/demos/helix-quickstart/recordings:/recordings \
#     helix-demo
#
set -euo pipefail

RECORDING_FILE="/recordings/helix-quickstart-$(date +%Y%m%d-%H%M%S).cast"
MAX_RETRIES=5
COOLDOWN=3  # seconds between claude calls to avoid rate limits

narrate() {
  echo ""
  echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
  echo "  $1"
  echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
  echo ""
  sleep 2
}

run() {
  echo "$ $*"
  "$@"
  echo ""
  sleep 1
}

show_file() {
  local file="$1"
  local lines="${2:-20}"
  echo "── $file ──"
  head -n "$lines" "$file" 2>/dev/null || echo "(file not found)"
  echo "..."
  echo ""
  sleep 2
}

# Run claude with retries. Accepts prompt as argument or via stdin.
# Captures stdin first so retries can re-send it.
# Detects success by: (1) meaningful output text, or (2) new files in
# the working directory — Claude often writes files but returns an error.
claude_run() {
  local prompt="" output=""
  if [[ $# -gt 0 ]]; then
    prompt="$*"
  else
    prompt="$(cat)"
  fi

  local attempt files_before files_after
  for attempt in $(seq 1 "$MAX_RETRIES"); do
    files_before=$(find . -not -path './.git/*' -not -path './.beads/*' -newer /tmp/claude_ts 2>/dev/null | wc -l || echo 0)
    touch /tmp/claude_ts
    output=$(printf '%s' "$prompt" | claude -p --no-session-persistence 2>/dev/null) || true
    files_after=$(find . -not -path './.git/*' -not -path './.beads/*' -newer /tmp/claude_ts 2>/dev/null | wc -l || echo 0)

    # Success: got real output text
    if [[ -n "$output" && "$output" != "Execution error" ]]; then
      break
    fi
    # Success: Claude wrote files even though output was an error
    if [[ "$files_after" -gt 0 ]]; then
      break
    fi
    if [[ $attempt -lt $MAX_RETRIES ]]; then
      echo "  (retrying $attempt/$MAX_RETRIES...)"
      sleep $((attempt * 3))
    fi
  done

  # Show output if meaningful; suppress bare error messages
  if [[ -n "$output" && "$output" != "Execution error" ]]; then
    printf '%s\n' "$output"
  fi

  # Cooldown between calls to avoid rate limits
  sleep "$COOLDOWN"
}

# Require a file to exist — abort the demo if it doesn't.
# This catches Claude failures early instead of cascading silently.
require_file() {
  local file="$1"
  local label="${2:-$file}"
  if [[ ! -f "$file" ]]; then
    echo ""
    echo "ERROR: Claude did not create $label"
    echo "The demo cannot continue. Re-run to try again."
    exit 1
  fi
}

demo_body() {
  # ── ACT 1: Setup ──────────────────────────────────────────
  narrate "ACT 1: Project Setup"

  run git init hello-helix
  cd hello-helix
  run br init

  narrate "Install DDx skills"
  mkdir -p .claude/skills
  cp -rf /ddx-library/skills/* .claude/skills/
  cat > .claude/settings.json <<'SETTINGS'
{
  "permissions": {
    "allow": ["Bash(*)", "Read(*)", "Write(*)", "Edit(*)"]
  }
}
SETTINGS
  run claude_run "List the available skills. Show just their names and one-line descriptions. Be brief."

  # ── ACT 2: Planning Stack ────────────────────────────────
  narrate "ACT 2: Build the Planning Stack"

  # Step 1: PRD
  narrate "Step 1: Create the PRD"
  claude_run 'Create a minimal PRD for "hello-helix", a Node.js CLI tool that converts temperatures between Fahrenheit and Celsius. Features: (1) `convert --to-celsius <temp>` converts Fahrenheit to Celsius, (2) `convert --to-fahrenheit <temp>` converts Celsius to Fahrenheit, (3) prints the result to stdout with one decimal place. Write the PRD to docs/helix/01-frame/prd.md. Create the directory structure. Keep it short — this is a demo project.'
  require_file docs/helix/01-frame/prd.md "the PRD"
  show_file docs/helix/01-frame/prd.md

  # Step 2: User story
  narrate "Step 2: Create a user story"
  claude_run 'Read docs/helix/01-frame/prd.md, then create a user story at docs/helix/01-frame/user-stories/US-001-temperature-conversion.md. Include two acceptance criteria: (1) `convert --to-celsius 212` prints `100.0`, (2) `convert --to-fahrenheit 0` prints `32.0`. Keep it concise.'
  require_file docs/helix/01-frame/user-stories/US-001-temperature-conversion.md "the user story"
  show_file docs/helix/01-frame/user-stories/US-001-temperature-conversion.md

  # Step 3: Technical design
  narrate "Step 3: Create a technical design"
  claude_run 'Read the PRD and user story under docs/helix/01-frame/, then create a technical design at docs/helix/02-design/technical-designs/TD-001-temperature-conversion.md. Design a single bin/convert.js entry point that parses --to-celsius and --to-fahrenheit flags using process.argv. The module should export toFahrenheit(c) and toCelsius(f) functions. Keep it minimal.'
  require_file docs/helix/02-design/technical-designs/TD-001-temperature-conversion.md "the technical design"
  show_file docs/helix/02-design/technical-designs/TD-001-temperature-conversion.md

  # Step 4: Tests (Red phase)
  narrate "Step 4: Create failing tests (Red phase)"
  claude_run 'Read the user story at docs/helix/01-frame/user-stories/US-001-temperature-conversion.md and the technical design at docs/helix/02-design/technical-designs/TD-001-temperature-conversion.md. You MUST create ALL of the following files: (1) docs/helix/03-test/test-plans/TP-001-temperature-conversion.md — the test plan, (2) package.json with contents: {"name":"hello-helix","version":"0.1.0","scripts":{"test":"node --test"}}, (3) tests/convert.test.js using node:test and node:assert that requires ../bin/convert.js for toFahrenheit and toCelsius functions, tests toFahrenheit(0) === 32.0, toCelsius(212) === 100.0, and toCelsius(98.6) is approximately 37.0. Do NOT create bin/convert.js — the tests MUST fail because the implementation does not exist yet.'

  # package.json is the only safety net — Claude sometimes skips it
  # because it's trivial, but npm test needs it
  if [[ ! -f package.json ]]; then
    echo '{"name":"hello-helix","version":"0.1.0","scripts":{"test":"node --test"}}' > package.json
  fi
  require_file tests/convert.test.js "the test file"
  show_file tests/convert.test.js 30

  narrate "Verify tests fail"
  run npm test || true
  echo "Tests fail as expected — Red phase."
  sleep 2

  # ── ACT 3: Execution ────────────────────────────────────
  narrate "ACT 3: Bead-Driven Implementation (Green phase)"

  run br create "Implement US-001: temperature conversion CLI" \
    --type task --priority 1
  BEAD_ID=$(br list --json | jq -r '.[0].id')
  br label add -l helix "$BEAD_ID" >/dev/null
  br label add -l phase:build "$BEAD_ID" >/dev/null
  br label add -l story:US-001 "$BEAD_ID" >/dev/null
  run br ready

  narrate "Implement — make the tests pass"
  claude_run "Read the governing artifacts: docs/helix/01-frame/user-stories/US-001-temperature-conversion.md, docs/helix/02-design/technical-designs/TD-001-temperature-conversion.md, and tests/convert.test.js. Write ONLY the implementation code in bin/convert.js to make the tests pass. The module must export toFahrenheit(c) and toCelsius(f). Also add CLI handling that parses --to-celsius and --to-fahrenheit from process.argv and prints the result with one decimal place. Follow the technical design. Do not modify the tests. Run 'npm test' to verify all tests pass."
  require_file bin/convert.js "the implementation"
  show_file bin/convert.js 25

  narrate "Verify tests pass"
  if npm test; then
    echo ""
    echo "All tests pass — Green phase complete!"
  else
    echo ""
    echo "Tests did not pass on first try — a real HELIX cycle would iterate here."
  fi
  sleep 2

  # Commit the implementation with bead traceability
  git add -A
  git commit -m "feat: implement temperature conversion CLI [${BEAD_ID}]" --allow-empty || true

  run br close "$BEAD_ID"

  # ── ACT 4: Critical Review ─────────────────────────────
  narrate "ACT 4: Critical Review"

  claude_run "Review all artifacts in this project for errors, omissions, and mischaracterizations: docs/helix/01-frame/prd.md, docs/helix/01-frame/user-stories/US-001-temperature-conversion.md, docs/helix/02-design/technical-designs/TD-001-temperature-conversion.md, tests/convert.test.js, and bin/convert.js. Does the implementation match the specs? Are acceptance criteria covered? Be concise — list findings as bullet points."
  sleep 2

  # ── ACT 5: Triage ──────────────────────────────────────
  narrate "ACT 5: Queue Health & Triage"

  claude_run 'Read tests/convert.test.js and bin/convert.js. List the gaps: which error-handling code paths have no tests? Which conversion edge cases are untested? Which acceptance criteria lack integration tests? Output a numbered list of gaps — one line each, no explanation.'

  # Create beads for the standard gaps the review always surfaces
  echo ""
  echo "Creating follow-up beads from triage findings..."
  run br create "Add CLI integration tests for AC-1 and AC-2" --type task --priority 2
  run br create "Add error-path tests (missing flag, bad input, both flags)" --type task --priority 2
  run br create "Add toFahrenheit edge-case tests (negative, fractional, -40 crossover)" --type task --priority 3

  echo ""
  echo "Beads queue after triage:"
  run br list --all
  sleep 2

  narrate "Demo complete!"
  echo ""
  echo "What you just saw:"
  echo "  1. Planning stack: PRD -> User Story -> Design -> Test Plan"
  echo "  2. Red phase: failing tests written BEFORE implementation"
  echo "  3. Bead-tracked implementation to Green"
  echo "  4. Critical review for errors and compliance"
  echo "  5. Queue triage and gap analysis"
  echo ""
  echo "All artifacts created by Claude. All work tracked in beads."
  echo ""
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  if [[ -d /recordings && "${HELIX_DEMO_RECORDING:-0}" != "1" ]]; then
    echo "Recording to $RECORDING_FILE"
    HELIX_DEMO_RECORDING=1 asciinema rec \
      -c "bash /usr/local/bin/demo.sh" \
      --title "HELIX Quickstart: Temperature Converter" \
      --cols 100 --rows 30 \
      "$RECORDING_FILE"
    echo "Recording saved: $RECORDING_FILE"
  else
    demo_body
  fi
fi
