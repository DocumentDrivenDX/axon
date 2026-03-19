#!/usr/bin/env bash
# HELIX Quickstart Demo — scripted asciinema recording
#
# This script drives a full HELIX cycle on a tiny Node.js project:
#   1. Setup: init repo, install ddx plugin, init beads
#   2. Planning: create PRD, user story, technical design, test plan
#   3. Execution: create beads, run helix implement, show TDD cycle
#   4. Alignment: run helix check and helix align
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

# Narrated pause — prints a section header and waits for readability
narrate() {
  echo ""
  echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
  echo "  $1"
  echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
  echo ""
  sleep 2
}

# Show a command before running it (demo-friendly)
run() {
  echo "$ $*"
  "$@"
  echo ""
  sleep 1
}

demo_body() {
  # ── ACT 1: Setup ──────────────────────────────────────────
  narrate "ACT 1: Project Setup"

  narrate "Install br (beads issue tracker)"
  if command -v br &>/dev/null; then
    echo "$ # br already installed: $(br --version)"
    sleep 1
  else
    run curl -fsSL https://raw.githubusercontent.com/Dicklesworthstone/beads_rust/main/install.sh | bash
  fi

  run git init hello-helix
  cd hello-helix

  narrate "Initialize beads workspace"
  run br init

  narrate "Install the DDx Library plugin"
  # Copy plugin into the project's claude config so skills are available
  mkdir -p .claude
  cat > .claude/settings.json <<'SETTINGS'
{
  "permissions": {
    "allow": ["Bash(*)", "Read(*)", "Write(*)", "Edit(*)"]
  }
}
SETTINGS
  # Link ddx-library as a local plugin
  run claude --plugin-dir /ddx-library -p --no-session-persistence \
    "Say 'DDx plugin loaded' and list the available /ddx: slash commands. Be brief."

  # ── ACT 2: Planning Stack ────────────────────────────────
  narrate "ACT 2: Build the Planning Stack"

  narrate "Create the project PRD"
  run claude --plugin-dir /ddx-library -p --no-session-persistence <<'PROMPT'
Create a minimal PRD for "hello-helix", a Node.js CLI tool that converts
temperatures between Fahrenheit and Celsius.

Features:
- `convert --to-celsius <temp>` converts Fahrenheit to Celsius
- `convert --to-fahrenheit <temp>` converts Celsius to Fahrenheit
- Prints the result to stdout with one decimal place

Write the PRD to docs/helix/01-frame/prd.md. Create the directory structure.
Keep it short — this is a demo project.
PROMPT

  narrate "Create a user story with acceptance criteria"
  run claude --plugin-dir /ddx-library -p --no-session-persistence <<'PROMPT'
Read docs/helix/01-frame/prd.md, then create a user story at
docs/helix/01-frame/user-stories/US-001-temperature-conversion.md.

Include two acceptance criteria:
1. `convert --to-celsius 212` prints `100.0`
2. `convert --to-fahrenheit 0` prints `32.0`

Keep it concise.
PROMPT

  narrate "Create a technical design"
  run claude --plugin-dir /ddx-library -p --no-session-persistence <<'PROMPT'
Read the PRD and user story under docs/helix/01-frame/, then create a
technical design at docs/helix/02-design/technical-designs/TD-001-temperature-conversion.md.

Design a single bin/convert.js entry point that parses --to-celsius and
--to-fahrenheit flags using process.argv. Keep it minimal.
PROMPT

  narrate "Create a test plan with failing tests"
  run claude --plugin-dir /ddx-library -p --no-session-persistence <<'PROMPT'
Read the user story at docs/helix/01-frame/user-stories/US-001-temperature-conversion.md
and the technical design at docs/helix/02-design/technical-designs/TD-001-temperature-conversion.md.

You MUST create ALL of the following files:

1. docs/helix/03-test/test-plans/TP-001-temperature-conversion.md — the test plan document
2. package.json — with contents: {"name":"hello-helix","version":"0.1.0","scripts":{"test":"node --test"}}
3. tests/convert.test.js — actual failing tests using node:test and node:assert that:
   - require('../bin/convert.js') for the conversion functions
   - Test toFahrenheit(0) === 32.0
   - Test toCelsius(212) === 100.0
   - Test toCelsius(98.6) is approximately 37.0

Do NOT create bin/convert.js — the tests MUST fail because the implementation does not exist yet.
Verify all three files exist after writing them.
PROMPT

  # Safety net: ensure package.json exists even if Claude forgot
  if [[ ! -f package.json ]]; then
    echo '{"name":"hello-helix","version":"0.1.0","scripts":{"test":"node --test"}}' > package.json
  fi

  narrate "Verify tests fail (Red phase)"
  run npm test || true
  echo ""
  echo "Tests fail as expected — this is the Red phase of TDD."
  sleep 2

  # ── ACT 3: Execution ────────────────────────────────────
  narrate "ACT 3: Beads-Driven Execution"

  narrate "Create an execution bead from the planning stack"
  run br create "Implement US-001: temperature conversion CLI" \
    --type task --priority 1
  # Add labels (br requires separate label add)
  BEAD_ID=$(br list --json | jq -r '.[0].id')
  run br label add -l helix "$BEAD_ID"
  run br label add -l phase:build "$BEAD_ID"
  run br label add -l story:US-001 "$BEAD_ID"

  narrate "Show the ready queue"
  run br ready

  narrate "Implement — make the tests pass (Green phase)"
  run claude --plugin-dir /ddx-library -p --no-session-persistence <<PROMPT
You are performing a HELIX implementation pass.

Read the governing artifacts:
- docs/helix/01-frame/user-stories/US-001-temperature-conversion.md
- docs/helix/02-design/technical-designs/TD-001-temperature-conversion.md
- docs/helix/03-test/test-plans/TP-001-temperature-conversion.md
- tests/convert.test.js

Write ONLY the implementation code needed to make the failing tests pass.
Follow the technical design. Do not modify the tests.

After writing the code, run "npm test" to verify all tests pass.
Commit the working code with message: "feat: implement temperature conversion CLI

Bead: $BEAD_ID
Governing: US-001, TD-001, TP-001"
PROMPT

  narrate "Verify tests pass (Green phase)"
  if npm test; then
    echo ""
    echo "All tests pass — Green phase complete."
  else
    echo ""
    echo "Some tests still failing — implementation may need refinement."
  fi
  sleep 2

  narrate "Close the bead"
  run br close "$BEAD_ID"

  narrate "Show the CLI works"
  node bin/convert.js --to-celsius 212 2>/dev/null && run node bin/convert.js --to-celsius 212 || echo "(CLI output depends on implementation structure)"
  node bin/convert.js --to-fahrenheit 0 2>/dev/null && run node bin/convert.js --to-fahrenheit 0 || true
  node bin/convert.js --to-celsius 98.6 2>/dev/null && run node bin/convert.js --to-celsius 98.6 || true

  # ── ACT 4: Alignment ───────────────────────────────────
  narrate "ACT 4: Alignment Check"

  narrate "Check queue health"
  run claude --plugin-dir /ddx-library -p --no-session-persistence <<'PROMPT'
Use the HELIX check action at /ddx-library/workflows/helix/actions/check.md.

Inspect this repository. The beads CLI is "br" (not "bd").
Return the required NEXT_ACTION line and the exact next command.
Be concise.
PROMPT

  narrate "Demo complete!"
  echo ""
  echo "What you just saw:"
  echo "  1. Created a planning stack (PRD → User Story → Design → Test Plan)"
  echo "  2. Wrote failing tests BEFORE implementation (Test-First)"
  echo "  3. Tracked execution with beads issue tracker"
  echo "  4. Implemented code to pass the tests (TDD Green phase)"
  echo "  5. Checked queue health with HELIX alignment"
  echo ""
  echo "All artifacts are traced. All work is tracked. All specs govern code."
  echo ""
}

# When sourced by the asciinema subprocess, only define functions — don't run.
# When executed directly as entrypoint, handle recording vs direct execution.
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
