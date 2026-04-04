# Enforcement Architecture for Agent-Driven Development

## Executive Summary

This document defines an architecture to enforce three critical constraints in agent-driven development:

1. **Memory Problem**: Agents forget to update specs and follow rules
2. **Drift Problem**: Specs don't impact actual code (spec-code drift)
3. **Assembly Problem**: Agents implement in isolation, not cohesively

The architecture provides tooling-level enforcement through pre-commit hooks, continuous verification, assembly coordination, and persistent context injection.

---

## System Diagram (Conceptual)

```
+-----------------------------------------------------------------------------------+
|                           ENFORCEMENT ARCHITECTURE                                  |
+-----------------------------------------------------------------------------------+
|                                                                                   |
|  +------------------+     +-------------------+     +----------------------+       |
|  |   AGENT          |     |   ENFORCEMENT     |     |   SHARED STATE       |       |
|  |   SESSION        |     |   LAYER           |     |                      |       |
|  |                  |     |                   |     |  +----------------+  |       |
|  | Claude Code      |---->| Pre-Edit Hooks    |---->|  | Spec Registry  |  |       |
|  | Agent Tasks      |     | Pre-Commit Gates  |     |  | (code<->spec)  |  |       |
|  | Bash Commands    |     | Pre-Task Checks   |     |  +----------------+  |       |
|  |                  |     |                   |     |                      |       |
|  +------------------+     +-------------------+     |  +----------------+  |       |
|          |                        |                 |  | Assembly Graph |  |       |
|          v                        v                 |  | (dependencies) |  |       |
|  +------------------+     +-------------------+     |  +----------------+  |       |
|  |   CODE CHANGES   |     |   CONTINUOUS      |     |                      |       |
|  |                  |     |   VERIFICATION    |     |  +----------------+  |       |
|  | src/**           |<--->| Drift Detector    |     |  | Rule Memory    |  |       |
|  | tests/**         |     | Pattern Analyzer  |     |  | (persisted)    |  |       |
|  | docs/helix/**    |     | Traceability      |     |  +----------------+  |       |
|  +------------------+     +-------------------+     |                      |       |
|          |                        |                 |  +----------------+  |       |
|          v                        v                 |  | Context Cache  |  |       |
|  +------------------+     +-------------------+     |  | (session data) |  |       |
|  |   GIT COMMIT     |     |   ASSEMBLY        |     |  +----------------+  |       |
|  |                  |     |   COORDINATOR     |     |                      |       |
|  | Lefthook Gates   |<--->| Dependency Check  |     +----------------------+       |
|  | dun check        |     | Conflict Detect   |               ^                    |
|  | Spec Validation  |     | Foundation Verify |               |                    |
|  +------------------+     +-------------------+               |                    |
|          |                        |                           |                    |
|          +------------------------+---------------------------+                    |
|                                   |                                                |
|                                   v                                                |
|                        +-------------------+                                       |
|                        |   CONTEXT         |                                       |
|                        |   INJECTION       |                                       |
|                        |                   |                                       |
|                        | Session Start     |                                       |
|                        | Pre-Edit Remind   |                                       |
|                        | Pre-Task Brief    |                                       |
|                        +-------------------+                                       |
|                                                                                   |
+-----------------------------------------------------------------------------------+
```

---

## Component Responsibilities

### 1. Enforcement Layer

**Purpose**: Gate all agent actions with validation checks.

| Component | Responsibility | Hook Point |
|-----------|---------------|------------|
| Pre-Edit Validator | Block edits without spec traceability | `.claude/settings.json` PreToolUse |
| Pre-Commit Gate | Block commits failing spec-code sync | `lefthook.yml` pre-commit |
| Pre-Task Checker | Inject rules before complex tasks | Pre-task hook |
| Phase Guard | Enforce HELIX phase boundaries | Pre-edit, Pre-commit |

### 2. Continuous Verification

**Purpose**: Detect drift and divergence proactively.

| Component | Responsibility | Trigger |
|-----------|---------------|---------|
| Drift Detector | Compare spec content to code behavior | On file save, scheduled |
| Pattern Analyzer | Detect architectural divergence | On multiple file changes |
| Traceability Checker | Verify artifact chains complete | On phase advancement |
| Conflict Scanner | Find duplicate/conflicting implementations | On src/** changes |

### 3. Assembly Coordinator

**Purpose**: Prevent isolated implementations.

| Component | Responsibility | Trigger |
|-----------|---------------|---------|
| Dependency Graph | Track what depends on what | Build-time |
| Foundation Verifier | Ensure shared code used properly | Pre-commit |
| Integration Mapper | Detect missing integration points | Cross-agent work |
| Duplication Detector | Find parallel implementations | PR/commit |

### 4. Shared State (Spec Registry)

**Purpose**: Single source of truth for spec-code mappings.

```yaml
# .helix/spec-registry.yml
version: 1
mappings:
  - spec: docs/helix/02-design/contracts/auth-api.yaml
    code:
      - src/api/auth/routes.ts
      - src/api/auth/handlers.ts
    tests:
      - tests/api/auth.test.ts
      - tests/contracts/auth.contract.ts
    last_verified: 2026-01-31T10:00:00Z
    drift_status: clean

  - spec: docs/helix/01-frame/user-stories/US-042.md
    code:
      - src/commands/workflow.ts
    tests:
      - tests/commands/workflow.test.ts
    phase: build
    story_artifacts:
      - US-042
      - TD-042
      - TP-042
      - IP-042
```

### 5. Context Injection

**Purpose**: Remind agents of rules at the right time.

| Injection Point | Context Provided |
|-----------------|------------------|
| Session Start | Active stories, phase constraints, recent violations |
| Pre-Edit | Relevant spec for file, required updates |
| Pre-Task | Rules for task type, patterns from memory |
| Pre-Commit | Spec-code sync requirements, checklist |

---

## Tool Specifications

### New `dun check` Types

#### 1. `dun check spec-sync`

**Purpose**: Verify code changes have corresponding spec updates.

```yaml
# In GATE.yaml or lefthook.yml
- check: spec-sync
  description: "Code changes require spec updates"
  config:
    # Files that require spec updates when changed
    watched_paths:
      - "src/api/**/*.ts"
      - "src/core/**/*.ts"
    # Spec locations to check for updates
    spec_paths:
      - "docs/helix/02-design/contracts/**/*.yaml"
      - "docs/helix/02-design/architecture.md"
    # How to determine if specs are updated
    sync_rules:
      - pattern: "src/api/{module}/**"
        requires: "docs/helix/02-design/contracts/{module}.yaml"
      - pattern: "src/core/{feature}/**"
        requires: "docs/helix/01-frame/features/FEAT-*-{feature}.md"
    # Tolerance for minor changes
    tolerance:
      minor_refactor: true    # Allow internal refactoring
      comment_only: true      # Allow comment changes
      test_only: true         # Allow test-only changes
```

#### 2. `dun check traceability`

**Purpose**: Verify complete artifact chains exist.

```yaml
- check: traceability
  description: "Story artifacts must form complete chains"
  config:
    story_pattern: "US-{id}"
    required_chain:
      - "docs/helix/01-frame/user-stories/US-{id}-*.md"
      - "docs/helix/02-design/technical-designs/TD-{id}-*.md"
    optional_chain:
      - "docs/helix/03-test/test-plans/TP-{id}-*.md"
      - "docs/helix/04-build/implementation-plans/IP-{id}-*.md"
    cross_references:
      - from: "TD-{id}"
        must_contain: "[[US-{id}]]"
      - from: "TP-{id}"
        must_contain: "[[TD-{id}]]"
```

#### 3. `dun check drift`

**Purpose**: Detect spec-code semantic drift.

```yaml
- check: drift
  description: "Code behavior must match spec intent"
  config:
    detection_methods:
      - type: api-contract
        spec: "docs/helix/02-design/contracts/*.yaml"
        code: "src/api/**/*.ts"
        compare: openapi-schema
      - type: test-coverage
        spec: "docs/helix/01-frame/user-stories/*.md"
        tests: "tests/**/*.test.ts"
        compare: acceptance-criteria
      - type: architecture
        spec: "docs/helix/02-design/architecture.md"
        code: "src/**/*.ts"
        compare: component-boundaries
    severity:
      missing_endpoint: error
      extra_endpoint: warning
      schema_mismatch: error
      missing_test: warning
```

#### 4. `dun check assembly`

**Purpose**: Detect isolated implementations.

```yaml
- check: assembly
  description: "Implementations must use shared foundations"
  config:
    shared_foundations:
      - path: "src/core/**"
        description: "Core utilities and types"
      - path: "src/shared/**"
        description: "Shared components"
    isolation_patterns:
      - pattern: "Duplicated logic"
        detect: "Similar functions in different modules"
        threshold: 0.8  # 80% similarity
      - pattern: "Missing integration"
        detect: "Components not connected to shared services"
      - pattern: "Conflicting implementations"
        detect: "Same interface, different behavior"
    required_imports:
      - "src/api/**": ["@/core/types", "@/core/errors"]
      - "src/commands/**": ["@/core/config", "@/core/utils"]
```

#### 5. `dun check phase-guard`

**Purpose**: Enforce HELIX phase boundaries.

```yaml
- check: phase-guard
  description: "Actions must align with current phase"
  config:
    phase_detection:
      strategy: artifact-based
      state_file: ".helix-state.yml"
    phase_rules:
      frame:
        allowed:
          - "docs/helix/01-frame/**"
        blocked:
          - "src/**"
          - "tests/**"
        message: "Frame phase: Define requirements, not code"
      design:
        allowed:
          - "docs/helix/02-design/**"
          - "docs/helix/01-frame/**"  # Updates allowed
        blocked:
          - "src/**"
          - "tests/**"
        message: "Design phase: Architecture only, no implementation"
      test:
        allowed:
          - "tests/**"
          - "docs/helix/03-test/**"
        blocked:
          - "src/**"
        message: "Test phase: Write failing tests, no implementation"
      build:
        allowed:
          - "src/**"
          - "tests/**"
          - "docs/**"
        required_together:
          - code_change: "src/**"
            with: "tests/**"
        message: "Build phase: Implement to pass tests"
```

---

### New Hooks

#### 1. `pre-edit-spec-check`

**Purpose**: Before editing code, verify spec awareness.

```typescript
// Hook implementation concept
interface PreEditSpecCheck {
  trigger: "Write|Edit|MultiEdit";
  checks: {
    // Find relevant spec for the file being edited
    findSpec(filePath: string): SpecMapping | null;

    // Verify agent has read the spec recently
    verifySpecAwareness(spec: SpecMapping): boolean;

    // Inject spec content as context
    injectSpecContext(spec: SpecMapping): string;
  };
  response: {
    // Block if no spec exists
    blockWithoutSpec: boolean;

    // Require spec acknowledgment
    requireAcknowledgment: boolean;

    // Auto-inject spec as context
    autoInjectContext: boolean;
  };
}
```

**Integration**:
```json
// .claude/settings.json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "^(Write|Edit|MultiEdit)$",
        "hooks": [
          {
            "type": "command",
            "command": "ddx enforce pre-edit-spec --file \"$TOOL_INPUT_file_path\"",
            "timeout": 5000,
            "blocking": true
          }
        ]
      }
    ]
  }
}
```

#### 2. `post-edit-spec-sync`

**Purpose**: After editing code, remind to update specs.

```typescript
interface PostEditSpecSync {
  trigger: "Write|Edit|MultiEdit";
  checks: {
    // Determine if edit requires spec update
    requiresSpecUpdate(filePath: string, changes: Diff): boolean;

    // Generate spec update suggestions
    suggestSpecUpdates(spec: SpecMapping, changes: Diff): string[];
  };
  response: {
    // Warn if spec update needed
    warnRequired: boolean;

    // Block commit without spec update
    blockCommitWithoutUpdate: boolean;

    // Auto-generate spec update PR
    autoGenerateUpdate: boolean;
  };
}
```

#### 3. `pre-commit-assembly-check`

**Purpose**: Before commit, verify no isolation.

```typescript
interface PreCommitAssemblyCheck {
  checks: {
    // Check for duplicate implementations
    findDuplicates(changedFiles: string[]): Duplicate[];

    // Verify shared foundation usage
    verifyFoundationUsage(changedFiles: string[]): FoundationViolation[];

    // Check integration completeness
    verifyIntegration(changedFiles: string[]): MissingIntegration[];
  };
  response: {
    // Block commit on critical violations
    blockOnCritical: boolean;

    // Warn on minor violations
    warnOnMinor: boolean;

    // Suggest fixes
    suggestFixes: boolean;
  };
}
```

#### 4. `session-start-context-load`

**Purpose**: Load relevant context at session start.

```typescript
interface SessionStartContextLoad {
  loads: {
    // Active stories and their phase
    activeStories(): StoryState[];

    // Recent violations and learnings
    recentViolations(): Violation[];

    // Phase-specific rules
    phaseRules(currentPhase: Phase): Rule[];

    // Pending spec updates
    pendingSpecUpdates(): SpecUpdate[];
  };
  injects: {
    // System prompt additions
    systemPromptAdditions: string;

    // Initial context message
    initialContext: string;
  };
}
```

---

### New `ddx` Commands

#### 1. `ddx enforce status`

**Purpose**: Show current enforcement state.

```bash
$ ddx enforce status

ENFORCEMENT STATUS
==================

Spec-Code Sync:
  - Clean: 12 mappings
  - Drift: 2 mappings
    - src/api/auth/routes.ts <-> contracts/auth.yaml (schema mismatch)
    - src/commands/init.ts <-> US-001 (missing acceptance criteria test)

Traceability:
  - Complete chains: 8 stories
  - Incomplete: 3 stories
    - US-042: Missing TD-042 (needs Design phase artifact)
    - US-043: Missing TP-043 (needs Test phase artifact)
    - US-044: TD-044 exists but no [[US-044]] reference

Assembly:
  - Shared foundations: 3 modules
  - Isolation warnings: 1
    - src/utils/format.ts duplicates src/core/format.ts

Phase:
  - Current: build
  - Active stories: 5
  - Phase violations today: 2
```

#### 2. `ddx enforce sync`

**Purpose**: Repair spec-code mappings.

```bash
$ ddx enforce sync

SYNC OPERATION
==============

Analyzing changes since last sync...

Updates required:
  1. docs/helix/02-design/contracts/auth.yaml
     - Add endpoint: POST /api/auth/refresh
     - Update schema: LoginResponse (added refreshToken field)

  2. docs/helix/01-frame/user-stories/US-042.md
     - Update acceptance criteria (5 new tests added)

Options:
  [a] Auto-generate spec updates
  [m] Manual update (open files)
  [s] Skip (mark as intentional drift)
  [c] Cancel

Choice: a

Generating spec updates...
  - Updated auth.yaml (+12 lines)
  - Updated US-042.md (+5 acceptance criteria)

Commit these changes? [y/n]: y
```

#### 3. `ddx enforce trace`

**Purpose**: Show artifact lineage.

```bash
$ ddx enforce trace US-042

STORY TRACEABILITY: US-042
==========================

Lineage:
  FEAT-001-workflow-commands
    |
    +-> US-042-list-mcp-servers (Frame)
         |
         +-> TD-042-list-mcp-servers (Design)
              |
              +-> TP-042-list-mcp-servers (Test)
                   |
                   +-> IP-042-list-mcp-servers (Build) <- CURRENT

Code Mappings:
  - src/commands/mcp.ts (primary implementation)
  - src/api/mcp/handlers.ts (API layer)

Test Mappings:
  - tests/commands/mcp.test.ts (unit tests)
  - tests/api/mcp.integration.ts (integration tests)
  - tests/contracts/mcp.contract.ts (contract tests)

Cross-References:
  - TD-042 references [[US-042]] ✓
  - TP-042 references [[TD-042]] ✓
  - IP-042 references [[TP-042]] ✓
  - Code contains @story US-042 ✓

Missing:
  - DP-042 (Deploy) - not yet created
  - IR-042 (Iterate) - not yet created
```

#### 4. `ddx enforce check`

**Purpose**: Run all enforcement checks.

```bash
$ ddx enforce check

RUNNING ENFORCEMENT CHECKS
==========================

[1/5] spec-sync ............ PASS (15 mappings clean)
[2/5] traceability ......... WARN (2 incomplete chains)
[3/5] drift ................ FAIL (1 schema mismatch)
[4/5] assembly ............. PASS (no isolation detected)
[5/5] phase-guard .......... PASS (all changes phase-appropriate)

Summary:
  - 3 PASS
  - 1 WARN
  - 1 FAIL

Details for failures:

FAIL: drift
  File: src/api/users/routes.ts
  Spec: docs/helix/02-design/contracts/users.yaml
  Issue: Endpoint POST /users/bulk not in spec
  Fix: Add endpoint to spec or remove from code

Run `ddx enforce sync` to repair.
```

#### 5. `ddx enforce remind`

**Purpose**: Generate context reminders for agents.

```bash
$ ddx enforce remind --for-task "implement user authentication"

CONTEXT REMINDER
================

For task: "implement user authentication"

Relevant Specs:
  - docs/helix/01-frame/user-stories/US-015-user-auth.md
  - docs/helix/02-design/technical-designs/TD-015-user-auth.md
  - docs/helix/02-design/contracts/auth.yaml

Current Phase: build

Phase Rules:
  1. All tests in TP-015 must pass
  2. Implementation must match TD-015 architecture
  3. API must conform to auth.yaml contract
  4. Use shared auth utilities from src/core/auth

Previous Learnings:
  - JWT refresh token flow documented in ADR-007
  - Rate limiting pattern from src/core/middleware
  - Error handling via @/core/errors

Required Updates After Implementation:
  - [ ] Update auth.yaml if new endpoints added
  - [ ] Update TD-015 if architecture changes
  - [ ] Add to secure-coding-checklist.md
```

---

## Integration with Existing Systems

### Lefthook Integration

```yaml
# lefthook.yml
pre-commit:
  commands:
    # Existing plugin validation
    validate-plugin:
      run: claude plugin validate .
      glob: "{.claude-plugin/**/*,commands/**/*,skills/**/*,agents/**/*}"

    # NEW: Spec-code sync check
    spec-sync:
      run: ddx enforce check spec-sync --staged
      glob: "src/**/*"
      fail_text: "Code changes require spec updates"

    # NEW: Traceability check
    traceability:
      run: ddx enforce check traceability --staged
      glob: "docs/helix/**/*"
      fail_text: "Incomplete artifact chains"

    # NEW: Assembly check
    assembly:
      run: ddx enforce check assembly --staged
      glob: "src/**/*"
      fail_text: "Isolated implementation detected"

    # NEW: Phase guard
    phase-guard:
      run: ddx enforce check phase-guard --staged
      fail_text: "Changes violate current phase boundaries"
```

### Claude Settings Integration

```json
// .claude/settings.json additions
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "^(Write|Edit|MultiEdit)$",
        "hooks": [
          {
            "type": "command",
            "command": "ddx enforce pre-edit --file \"$TOOL_INPUT_file_path\" --inject-context",
            "timeout": 5000,
            "blocking": true
          }
        ]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "^(Write|Edit|MultiEdit)$",
        "hooks": [
          {
            "type": "command",
            "command": "ddx enforce post-edit --file \"$TOOL_INPUT_file_path\" --check-spec-sync",
            "timeout": 5000
          }
        ]
      }
    ],
    "SessionStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "ddx enforce session-start --load-context --inject-rules",
            "timeout": 10000
          }
        ]
      }
    ]
  },
  "enforcement": {
    "enabled": true,
    "strictMode": false,
    "specSyncRequired": true,
    "traceabilityRequired": true,
    "assemblyCheckEnabled": true,
    "phaseGuardEnabled": true,
    "contextInjection": {
      "onSessionStart": true,
      "onPreEdit": true,
      "onPreTask": true
    }
  }
}
```

### HELIX Gate Integration

```yaml
# workflows/helix/phases/04-build/GATE.yaml additions
exit_requirements:
  automated:
    # Existing checks...

    # NEW: Spec-code sync verification
    - check: spec-sync
      description: "All code changes have corresponding spec updates"
      config:
        watched_paths: ["src/**/*.ts"]
        spec_paths: ["docs/helix/02-design/**/*"]

    # NEW: Traceability verification
    - check: traceability
      description: "All stories have complete artifact chains"
      config:
        required_chain: ["US", "TD", "TP", "IP"]

    # NEW: Assembly verification
    - check: assembly
      description: "No isolated implementations"
      config:
        shared_foundations: ["src/core/**", "src/shared/**"]
```

---

## Implementation Priority

### Phase 1: Foundation (Week 1-2)

**Goal**: Basic enforcement infrastructure.

| Item | Priority | Effort | Impact |
|------|----------|--------|--------|
| Spec Registry schema | P0 | 2 days | Enables all checks |
| `dun check spec-sync` | P0 | 3 days | Core drift prevention |
| `ddx enforce status` | P0 | 2 days | Visibility into state |
| Lefthook integration | P0 | 1 day | Immediate enforcement |

### Phase 2: Verification (Week 3-4)

**Goal**: Continuous verification capabilities.

| Item | Priority | Effort | Impact |
|------|----------|--------|--------|
| `dun check traceability` | P0 | 3 days | Chain completeness |
| `dun check drift` | P1 | 4 days | Semantic drift detection |
| `ddx enforce trace` | P1 | 2 days | Developer visibility |
| Pre-edit context injection | P1 | 2 days | Agent awareness |

### Phase 3: Assembly (Week 5-6)

**Goal**: Prevent isolated implementations.

| Item | Priority | Effort | Impact |
|------|----------|--------|--------|
| `dun check assembly` | P1 | 4 days | Cohesion enforcement |
| Dependency graph builder | P1 | 3 days | Integration mapping |
| Duplication detector | P2 | 3 days | Reduce redundancy |
| Foundation usage checker | P1 | 2 days | Shared code adoption |

### Phase 4: Context (Week 7-8)

**Goal**: Persistent memory and reminders.

| Item | Priority | Effort | Impact |
|------|----------|--------|--------|
| Session context loader | P0 | 3 days | Rule persistence |
| `ddx enforce remind` | P1 | 2 days | Just-in-time guidance |
| Violation history | P2 | 2 days | Learning from mistakes |
| Pattern memory integration | P2 | 3 days | Cross-session learning |

---

## Success Metrics

### Enforcement Effectiveness

| Metric | Target | Measurement |
|--------|--------|-------------|
| Spec-code drift rate | < 5% | Drift checks per commit |
| Incomplete chains | 0 at deploy | Traceability checks |
| Isolated implementations | 0 new | Assembly checks per PR |
| Phase violations | < 1/day | Phase guard blocks |

### Agent Behavior

| Metric | Target | Measurement |
|--------|--------|-------------|
| Spec updates with code | > 90% | Co-committed changes |
| Correct phase actions | > 95% | Phase guard pass rate |
| Shared foundation usage | > 80% | Import analysis |
| Cross-reference accuracy | 100% | Link validation |

### Developer Experience

| Metric | Target | Measurement |
|--------|--------|-------------|
| False positive rate | < 5% | Override usage |
| Check execution time | < 5s | Performance monitoring |
| Context relevance | > 80% | Agent feedback |
| Fix suggestion accuracy | > 70% | Accepted suggestions |

---

## Risk Mitigation

### Risk: Over-Enforcement

**Symptoms**: Agents blocked constantly, reduced velocity.

**Mitigation**:
- Start with warnings, escalate to blocks
- Provide clear override mechanism
- Track false positive rate
- Tune thresholds based on data

### Risk: Performance Impact

**Symptoms**: Slow pre-commit hooks, session start delays.

**Mitigation**:
- Cache spec mappings
- Incremental change detection
- Async background checks where possible
- Timeout with warnings vs blocks

### Risk: Agent Circumvention

**Symptoms**: Agents find workarounds, enforcement bypassed.

**Mitigation**:
- Multiple enforcement points (pre-edit, pre-commit, CI)
- Log all overrides for review
- Periodic audit of bypassed checks
- Continuous improvement of detection

---

## Appendix: Configuration Files

### `.helix/enforcement.yml`

```yaml
version: 1
enabled: true

spec_sync:
  enabled: true
  mode: warn  # warn | block
  watched_paths:
    - "src/**/*.ts"
    - "src/**/*.tsx"
  spec_paths:
    - "docs/helix/02-design/contracts/**"
    - "docs/helix/02-design/architecture.md"
  exclude:
    - "src/**/*.test.ts"
    - "src/**/*.spec.ts"

traceability:
  enabled: true
  mode: block
  required_chain:
    - US
    - TD
  optional_chain:
    - TP
    - IP
    - DP
    - IR
  cross_references:
    enforce: true
    format: "wikilink"  # wikilink | markdown

drift:
  enabled: true
  mode: warn
  detection:
    api_contracts: true
    architecture_boundaries: true
    test_coverage: true
  schedule:
    on_commit: true
    on_pr: true
    daily: true

assembly:
  enabled: true
  mode: warn
  foundations:
    - path: "src/core/**"
      required_by: ["src/api/**", "src/commands/**"]
    - path: "src/shared/**"
      required_by: ["src/**"]
  duplication:
    threshold: 0.8
    min_lines: 10

phase_guard:
  enabled: true
  mode: block
  detection: artifact-based
  override_allowed: false

context_injection:
  session_start: true
  pre_edit: true
  pre_task: true
  content:
    active_stories: true
    phase_rules: true
    recent_violations: true
    relevant_specs: true
```

### `.helix/spec-registry.yml`

```yaml
version: 1
last_updated: 2026-01-31T10:00:00Z

mappings:
  - id: auth-api
    spec: docs/helix/02-design/contracts/auth.yaml
    code:
      - src/api/auth/routes.ts
      - src/api/auth/handlers.ts
      - src/api/auth/middleware.ts
    tests:
      - tests/api/auth.test.ts
      - tests/contracts/auth.contract.ts
    stories:
      - US-015
      - US-016
    last_verified: 2026-01-31T10:00:00Z
    status: clean

  - id: workflow-commands
    spec: docs/helix/01-frame/features/FEAT-001-workflow-commands.md
    code:
      - src/commands/workflow.ts
      - src/commands/init.ts
    tests:
      - tests/commands/workflow.test.ts
    stories:
      - US-042
      - US-043
      - US-044
    last_verified: 2026-01-30T15:00:00Z
    status: drift
    drift_details:
      - file: src/commands/workflow.ts
        issue: "New subcommand 'validate' not in spec"
```

---

*This architecture provides comprehensive enforcement at the tooling level, ensuring agents remember rules, specs remain synchronized with code, and implementations build cohesively on shared foundations.*
