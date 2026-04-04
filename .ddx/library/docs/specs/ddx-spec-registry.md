# DDX Spec Registry Enhancement

## Overview

This specification defines enhancements to DDX and claude-flow for maintaining a spec registry that enables spec-driven development enforcement.

---

## Spec Registry Memory Namespace

### Purpose

Store bidirectional mappings between code files and their governing specifications.

### Schema

```typescript
interface SpecRegistryEntry {
  // The specification file path
  spec: string;

  // Specific sections within the spec that govern this code
  sections: string[];

  // When the binding was last verified
  last_synced: string; // ISO-8601

  // Hash of spec content for drift detection
  spec_hash: string;

  // Optional metadata
  metadata?: {
    feature_id?: string;
    priority?: "P0" | "P1" | "P2";
    owner?: string;
  };
}

// Memory namespace structure
interface SpecRegistry {
  namespace: "spec-registry";
  entries: Record<string, SpecRegistryEntry>; // key is code file path
}
```

### Example Entries

```json
{
  "namespace": "spec-registry",
  "entries": {
    "internal/auth/handler.go": {
      "spec": "docs/helix/01-frame/features/FEAT-001-auth.md",
      "sections": ["## Implementation", "## API Endpoints"],
      "last_synced": "2026-01-31T10:00:00Z",
      "spec_hash": "sha256:abc123...",
      "metadata": {
        "feature_id": "FEAT-001",
        "priority": "P0"
      }
    },
    "internal/auth/middleware.go": {
      "spec": "docs/helix/01-frame/features/FEAT-001-auth.md",
      "sections": ["## Middleware"],
      "last_synced": "2026-01-31T10:00:00Z",
      "spec_hash": "sha256:abc123..."
    },
    "internal/user/repository.go": {
      "spec": "docs/helix/01-frame/features/FEAT-002-users.md",
      "sections": ["## Data Model", "## Repository"],
      "last_synced": "2026-01-31T09:00:00Z",
      "spec_hash": "sha256:def456..."
    }
  }
}
```

---

## CLI Commands

### ddx spec register

Register a code-to-spec binding.

```bash
ddx spec register \
  --code "internal/auth/handler.go" \
  --spec "docs/helix/01-frame/features/FEAT-001-auth.md" \
  --sections "## Implementation" "## API Endpoints" \
  --feature-id "FEAT-001"
```

**Implementation**:
```bash
# Translates to claude-flow memory store
npx @claude-flow/cli@latest memory store \
  --namespace spec-registry \
  --key "internal/auth/handler.go" \
  --value '{"spec":"docs/helix/01-frame/features/FEAT-001-auth.md","sections":["## Implementation","## API Endpoints"],"last_synced":"2026-01-31T10:00:00Z","spec_hash":"...","metadata":{"feature_id":"FEAT-001"}}'
```

### ddx spec status

Check the spec status for a code file.

```bash
ddx spec status --code "internal/auth/handler.go"

# Output:
# 📋 Spec: docs/helix/01-frame/features/FEAT-001-auth.md
# 📑 Sections: ## Implementation, ## API Endpoints
# ✅ In sync (last checked: 2026-01-31T10:00:00Z)
```

```bash
ddx spec status --code "internal/auth/handler.go" --json

# Output:
{
  "code": "internal/auth/handler.go",
  "spec": "docs/helix/01-frame/features/FEAT-001-auth.md",
  "sections": ["## Implementation", "## API Endpoints"],
  "status": "synced",
  "last_synced": "2026-01-31T10:00:00Z"
}
```

**Implementation**:
```bash
npx @claude-flow/cli@latest memory retrieve \
  --namespace spec-registry \
  --key "internal/auth/handler.go"
```

### ddx spec find

Find all code files governed by a spec.

```bash
ddx spec find --spec "docs/helix/01-frame/features/FEAT-001-auth.md"

# Output:
# Code files governed by FEAT-001-auth.md:
#   - internal/auth/handler.go (## Implementation, ## API Endpoints)
#   - internal/auth/middleware.go (## Middleware)
#   - internal/auth/types.go (## Data Types)
```

**Implementation**:
```bash
# Search memory for entries with matching spec
npx @claude-flow/cli@latest memory search \
  --namespace spec-registry \
  --query "FEAT-001-auth.md"
```

### ddx spec drift

Detect drift between specs and code.

```bash
ddx spec drift --since HEAD~5

# Output:
# 🔍 Checking spec-code drift since HEAD~5...
#
# ⚠️ DRIFT DETECTED:
#   internal/auth/handler.go
#     - Code changed: 2026-01-31T11:00:00Z
#     - Spec unchanged since: 2026-01-30T09:00:00Z
#     - Governing spec: docs/helix/01-frame/features/FEAT-001-auth.md
#
# ✅ In sync: 15 files
# ⚠️ Drifted: 1 file
```

**Implementation**:
1. Get list of files changed since baseline: `git diff --name-only HEAD~5`
2. For each changed file:
   - Retrieve spec binding from registry
   - Check if spec was also modified
   - If code changed but spec didn't → drift

### ddx spec sync

Update spec hash after verification.

```bash
ddx spec sync --code "internal/auth/handler.go"

# Output:
# ✅ Updated sync status for internal/auth/handler.go
# 📋 Spec: docs/helix/01-frame/features/FEAT-001-auth.md
# 🔄 New hash: sha256:xyz789...
```

### ddx spec list

List all registered bindings.

```bash
ddx spec list

# Output:
# Registered spec bindings:
#
# FEAT-001-auth.md (3 files)
#   - internal/auth/handler.go
#   - internal/auth/middleware.go
#   - internal/auth/types.go
#
# FEAT-002-users.md (2 files)
#   - internal/user/repository.go
#   - internal/user/service.go
```

### ddx spec unregister

Remove a binding.

```bash
ddx spec unregister --code "internal/auth/legacy.go"

# Output:
# ✅ Removed spec binding for internal/auth/legacy.go
```

---

## Hook Integrations

### Pre-Edit Hook

Inject spec context before any file edit.

```bash
# .claude-flow/hooks.yaml
pre-edit:
  enabled: true
  script: |
    #!/bin/bash
    FILE="$TOOL_INPUT_file_path"

    # Check if file has a governing spec
    ENTRY=$(npx @claude-flow/cli@latest memory retrieve \
      --namespace spec-registry \
      --key "$FILE" 2>/dev/null)

    if [ -n "$ENTRY" ] && [ "$ENTRY" != "null" ]; then
      SPEC=$(echo "$ENTRY" | jq -r '.spec')
      SECTIONS=$(echo "$ENTRY" | jq -r '.sections | join(", ")')

      echo ""
      echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
      echo "📋 GOVERNING SPEC: $SPEC"
      echo "📑 SECTIONS: $SECTIONS"
      echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
      echo ""
      echo "⚠️  You MUST update the spec if changing this file's behavior."
      echo "    Use 'ddx spec sync' after updating both files."
      echo ""

      # Optionally show spec content
      if [ -f "$SPEC" ]; then
        echo "--- Spec Content Preview ---"
        head -30 "$SPEC"
        echo "..."
        echo "----------------------------"
      fi
    fi
```

### Post-Edit Hook

Verify spec was updated when code changes.

```bash
# .claude-flow/hooks.yaml
post-edit:
  enabled: true
  script: |
    #!/bin/bash
    FILE="$TOOL_INPUT_file_path"
    SUCCESS="$TOOL_SUCCESS"

    if [ "$SUCCESS" != "true" ]; then
      exit 0
    fi

    # Check if file has a governing spec
    ENTRY=$(npx @claude-flow/cli@latest memory retrieve \
      --namespace spec-registry \
      --key "$FILE" 2>/dev/null)

    if [ -n "$ENTRY" ] && [ "$ENTRY" != "null" ]; then
      SPEC=$(echo "$ENTRY" | jq -r '.spec')

      # Check if spec is in git staging area
      SPEC_STAGED=$(git diff --cached --name-only | grep -F "$SPEC")

      if [ -z "$SPEC_STAGED" ]; then
        echo ""
        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        echo "⚠️  WARNING: Code changed but spec not updated"
        echo "📋 Governing spec: $SPEC"
        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        echo ""
        echo "Consider updating the spec before committing."
        echo "Run 'ddx spec sync --code $FILE' after updating."
        echo ""
      fi
    fi

    # Train neural patterns on successful edit
    npx @claude-flow/cli@latest hooks post-edit \
      --file "$FILE" \
      --success true \
      --train-neural true
```

### Pre-Commit Hook (Lefthook)

Gate commits on spec compliance.

```yaml
# lefthook.yml
pre-commit:
  commands:
    spec-drift-check:
      run: |
        #!/bin/bash

        # Get staged files
        STAGED=$(git diff --cached --name-only)

        DRIFT_FOUND=0

        for FILE in $STAGED; do
          # Skip non-code files
          if [[ ! "$FILE" =~ \.(go|ts|py|js)$ ]]; then
            continue
          fi

          # Check if file has governing spec
          ENTRY=$(npx @claude-flow/cli@latest memory retrieve \
            --namespace spec-registry \
            --key "$FILE" 2>/dev/null)

          if [ -n "$ENTRY" ] && [ "$ENTRY" != "null" ]; then
            SPEC=$(echo "$ENTRY" | jq -r '.spec')

            # Check if spec is also staged
            SPEC_STAGED=$(echo "$STAGED" | grep -F "$SPEC")

            if [ -z "$SPEC_STAGED" ]; then
              echo "❌ SPEC DRIFT: $FILE changed but $SPEC not updated"
              DRIFT_FOUND=1
            fi
          fi
        done

        if [ $DRIFT_FOUND -eq 1 ]; then
          echo ""
          echo "Commit blocked: Update governing specs or use --no-verify to skip"
          exit 1
        fi

        exit 0

      fail_text: "Spec-code drift detected. Update specs before committing."
```

---

## Integrator Agent Definition

### Agent Manifest

```yaml
# agents/integrator.yaml
name: integrator
description: Assembles components into cohesive whole, verifies contracts

capabilities:
  - Verify all components satisfy integration contracts
  - Resolve conflicts between agent implementations
  - Maintain contracts/integration-map.yaml
  - Run assembly validation tests
  - Coordinate handoffs between implementation agents

triggers:
  - event: multiple_agents_complete
    condition: related_work
  - event: component_ready
    condition: needs_integration
  - event: contract_changed
    condition: always

inputs:
  - contracts/integration-map.yaml
  - contracts/*.ts
  - .dun/work-in-progress.yaml

outputs:
  - integration-report.md
  - updated integration-map.yaml

prompts:
  system: |
    You are the Integration Agent. Your role is to ensure all components
    work together cohesively. You are the gatekeeper for integration.

    ## Core Responsibilities

    1. **Contract Verification**
       - All interfaces must be defined BEFORE implementation
       - No component proceeds without a contract
       - Contracts are immutable once implementation begins

    2. **Conflict Resolution**
       - Detect overlapping work between agents
       - Mediate interface disagreements
       - Propose unified solutions

    3. **Assembly Validation**
       - Run integration tests after each component
       - Verify no regressions introduced
       - Ensure components compose correctly

    ## Process

    Before approving any integration:
    1. Check contracts/integration-map.yaml for completeness
    2. Verify all providers have implementations
    3. Verify all consumers can resolve dependencies
    4. Run integration test suite
    5. Update documentation

    ## Output Format

    Always provide structured integration reports:
    ```markdown
    ## Integration Report

    ### Components Integrated
    - [component name]: [status]

    ### Contract Status
    - [interface name]: [provider] → [consumers]

    ### Test Results
    - [test suite]: [pass/fail]

    ### Issues Found
    - [issue description]

    ### Next Steps
    - [action items]
    ```

  pre_work: |
    Before starting integration work:

    1. Load contracts/integration-map.yaml
    2. Check .dun/work-in-progress.yaml for active claims
    3. Review recent changes via git log
    4. Identify components ready for integration

    Report what you find before proceeding.

  post_work: |
    After completing integration:

    1. Update integration-map.yaml with new bindings
    2. Clear resolved claims from work-in-progress.yaml
    3. Store integration patterns in memory:
       npx @claude-flow/cli@latest memory store \
         --namespace integration-patterns \
         --key "[pattern-name]" \
         --value "[what worked]"
    4. Notify dependent agents via handoff protocol
```

### Agent Spawning

```bash
# Spawn integrator agent
npx @claude-flow/cli@latest agent spawn \
  --type integrator \
  --name integration-coordinator \
  --config agents/integrator.yaml
```

Or via Task tool:
```javascript
Task({
  prompt: "Verify all components satisfy contracts and integrate correctly",
  subagent_type: "integrator",
  description: "Integration verification"
})
```

---

## Beads Integration

### Upstream Beads Mapping

Use upstream Beads fields rather than a custom bead YAML schema:

```bash
bd create "Implement authentication handler" \
  --type task \
  --labels helix,phase:build,kind:build,area:auth \
  --spec-id docs/helix/01-frame/features/FEAT-001-auth.md \
  --description "Governing artifacts: FEAT-001-auth, TD-auth, TP-auth" \
  --design "Provides: AuthProvider. Consumes: UserRepository from user-service." \
  --acceptance "tests/integration/auth_test.go passes and permissions integration remains green."

# User service must complete first
bd dep add auth-abc123 user-def456

# Permissions service depends on this bead
bd dep auth-abc123 --blocks perms-ghi789
```

Recommended mapping:

- spec binding: native `spec-id`
- governing artifact chain: `description`
- contracts: `design` or structured metadata
- integration tests: `acceptance`
- blocking links: `bd dep add`, `bd dep --blocks`, or `--deps`
- parent-child structure: native `parent`
- discovered work: native `discovered-from` dependency type where useful

### Beads Plugin Checks

```yaml
# .dun/plugins/beads/plugin.yaml
id: beads
version: "2.0.0"
description: "Enhanced beads with spec and contract enforcement"

checks:
  - id: bead-spec-binding
    type: rule-set
    phase: frame
    description: "Verify bead has governing spec_id"
    rules:
      - type: bead-field-required
        field: spec_id
      - type: path-exists
        path: "{{ .bead.spec_id }}"
        message: "Bead spec file does not exist"

  - id: bead-contracts-defined
    type: integration-contract
    phase: design
    description: "Verify bead contracts are in integration map"
    contracts:
      map: "contracts/integration-map.yaml"
    rules:
      - type: bead-design-provides-registered
      - type: bead-design-consumes-available

  - id: bead-integration-tests
    type: rule-set
    phase: test
    description: "Verify integration tests exist"
    rules:
      - type: bead-acceptance-references-existing-tests

  - id: bead-ready-check
    type: agent
    phase: build
    description: "Verify bead is ready for implementation"
    prompt: prompts/bead-readiness.md
    inputs:
      - "{{ .bead.spec_id }}"
      - "contracts/integration-map.yaml"
```

---

## Memory Patterns

### Store successful integration patterns

```bash
# After successful integration
npx @claude-flow/cli@latest memory store \
  --namespace integration-patterns \
  --key "auth-user-integration" \
  --value '{
    "components": ["auth-service", "user-service"],
    "approach": "Repository pattern with interface injection",
    "tests": ["TestAuthWithMockUser", "TestAuthIntegration"],
    "issues_resolved": ["circular dependency via interface"],
    "success": true
  }'
```

### Search for relevant patterns before integration

```bash
# Before starting new integration
npx @claude-flow/cli@latest memory search \
  --namespace integration-patterns \
  --query "service integration repository"
```

---

## Summary

This specification enables:

1. **Spec Registry** - Bidirectional code-spec mappings stored in claude-flow memory
2. **CLI Commands** - `ddx spec` commands for managing bindings
3. **Hook Integration** - Pre/post edit hooks inject and verify spec compliance
4. **Commit Gates** - Pre-commit checks block drift
5. **Integrator Agent** - Dedicated agent for assembly coordination
6. **Beads Enhancement** - Beads include spec and contract requirements
7. **Memory Patterns** - Learn from successful integrations
