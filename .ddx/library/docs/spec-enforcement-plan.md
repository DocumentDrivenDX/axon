# Consensus Plan: Spec-Driven Agent Development

## Executive Summary

This plan addresses three core problems in agent-driven development:
1. **Agents forget to update specs or follow rules**
2. **Specs don't impact actual code (drift)**
3. **Agents implement in isolation, not assembling cohesively**

Based on analysis from CODEX (claude-flow), GEMINI (dun), and research on multi-agent coordination patterns.

---

## Problem Analysis

### Problem 1: Agents Forget Rules

**Root Cause**: No mechanism to inject rules into agent context at execution time.

| Current State | Gap |
|--------------|-----|
| Claude-flow has 27 hooks (all disabled) | Hooks not configured to inject spec requirements |
| Dun has agent prompts with rules | Rules are advisory, not enforced at commit time |
| Memory systems exist | No spec registry mapping code→spec files |

### Problem 2: Spec-Code Drift

**Root Cause**: Specifications and code are checked independently with no bidirectional binding.

| Current State | Gap |
|--------------|-----|
| Dun has `helix-align-specs` check | Only compares docs, doesn't check code |
| Claude-flow has post-edit hooks | No verification that specs were updated |
| Pre-commit hooks exist | No spec-validation in commit gate |

### Problem 3: Isolated Implementation

**Root Cause**: No assembly coordination or integration contract enforcement.

| Current State | Gap |
|--------------|-----|
| Claude-flow has hierarchical topology | No integrator agent type |
| Dun has state rules for artifact progression | No conflict detection between agents |
| Multi-agent patterns documented | No structured handoff protocol in use |

---

## Solution Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                      SPEC ENFORCEMENT LAYER                          │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌────────────────┐    ┌────────────────┐    ┌────────────────┐     │
│  │  Spec Registry │◄───│  Drift Detector│◄───│ Change Cascade │     │
│  │   (Memory)     │    │   (Dun Worker) │    │   (Dun Check)  │     │
│  └────────────────┘    └────────────────┘    └────────────────┘     │
│          ▲                    │                      │              │
│          │                    ▼                      ▼              │
│  ┌────────────────┐    ┌────────────────┐    ┌────────────────┐     │
│  │  pre-edit hook │    │  post-edit hook│    │  pre-commit    │     │
│  │ (inject specs) │    │ (verify update)│    │  (gate commit) │     │
│  └────────────────┘    └────────────────┘    └────────────────┘     │
│                                                                      │
├─────────────────────────────────────────────────────────────────────┤
│                    INTEGRATION CONTRACT LAYER                        │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌────────────────┐    ┌────────────────┐    ┌────────────────┐     │
│  │   Contracts    │◄───│  Integration   │◄───│   Conflict     │     │
│  │   Registry     │    │   Validator    │    │   Detector     │     │
│  └────────────────┘    └────────────────┘    └────────────────┘     │
│          ▲                    │                      │              │
│          │                    ▼                      ▼              │
│  ┌────────────────┐    ┌────────────────┐    ┌────────────────┐     │
│  │   Integrator   │    │   Assembly     │    │   Handoff      │     │
│  │     Agent      │    │   Validator    │    │   Protocol     │     │
│  └────────────────┘    └────────────────┘    └────────────────┘     │
│                                                                      │
├─────────────────────────────────────────────────────────────────────┤
│                       EXISTING INFRASTRUCTURE                        │
│  ┌────────────────┐    ┌────────────────┐    ┌────────────────┐     │
│  │  Claude-flow   │    │     Dun        │    │   Gas Town     │     │
│  │  Swarm + Hooks │    │  Plugin Engine │    │  Beads/Rigs    │     │
│  └────────────────┘    └────────────────┘    └────────────────┘     │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Implementation Phases

### Phase 1: Spec Registry & Injection (Week 1)

**Objective**: Agents cannot edit code without knowing associated specs.

#### 1.1 Create Spec Registry

Store bidirectional mappings in claude-flow memory:

```bash
# Register code→spec mappings
npx @claude-flow/cli@latest memory store \
  --namespace spec-registry \
  --key "internal/auth/handler.go" \
  --value '{"spec": "docs/helix/01-frame/features/FEAT-001-auth.md", "sections": ["## Implementation"]}'
```

#### 1.2 Enable Pre-Edit Hook

Configure claude-flow to inject spec context before any file edit:

```yaml
# .claude-flow/hooks/pre-edit.yaml
enabled: true
action: |
  spec=$(npx @claude-flow/cli@latest memory retrieve --namespace spec-registry --key "$FILE")
  if [ -n "$spec" ]; then
    echo "⚠️ This file is governed by spec: $spec"
    echo "You MUST update the spec if changing this file's behavior."
  fi
```

#### 1.3 Dun: spec-injection Check Type

New check that injects specs into agent prompts:

```yaml
# In dun helix plugin
- id: spec-injection
  type: agent-rule-injection
  phase: build
  inject_rules:
    - source: "docs/helix/01-frame/principles.md"
    - from_registry: true  # Dynamically load from spec-registry
```

---

### Phase 2: Drift Detection & Enforcement (Week 2)

**Objective**: Changes to code require spec updates; changes to specs cascade downstream.

#### 2.1 Dun: spec-binding Check Type

Verify bidirectional references between specs and code:

```yaml
- id: spec-code-binding
  type: spec-binding
  phase: build
  bindings:
    specs:
      - pattern: "docs/helix/01-frame/features/FEAT-*.md"
        implementation_section: "## Implementation"
    code:
      - pattern: "internal/**/*.go"
        spec_comment: "// Implements: FEAT-"
  rules:
    - type: bidirectional-coverage
      min_coverage: 0.8
```

#### 2.2 Dun: change-cascade Check Type

When upstream files change, verify downstream updates:

```yaml
- id: change-cascade
  type: change-cascade
  phase: iterate
  cascade_rules:
    - upstream: "docs/helix/01-frame/prd.md"
      downstreams:
        - "docs/helix/02-design/architecture.md"
        - "docs/helix/01-frame/features/*.md"
    - upstream: "docs/helix/01-frame/features/*.md"
      downstreams:
        - "internal/**/*.go"
        - "docs/helix/03-test/test-plan.md"
  trigger: git-diff
```

#### 2.3 Pre-Commit Gate

Add to lefthook:

```yaml
# lefthook.yml
pre-commit:
  commands:
    spec-drift:
      run: dun check --filter spec-binding --fail-fast
      fail_text: "Spec-code binding violated. Update specs or add // Implements: comments."
```

---

### Phase 3: Integration Contracts & Assembly (Week 3)

**Objective**: Components define interfaces before implementation; agents verify integration.

#### 3.1 Contract Registry

Define contracts in a standard location:

```
contracts/
  auth-service.ts       # TypeScript interface definitions
  user-repository.ts
  integration-map.yaml  # Which components connect
```

```yaml
# contracts/integration-map.yaml
components:
  auth-service:
    provides:
      - AuthProvider
      - TokenValidator
    consumes:
      - UserRepository
  user-service:
    provides:
      - UserRepository
    consumes: []
```

#### 3.2 Dun: integration-contract Check Type

```yaml
- id: integration-contracts
  type: integration-contract
  phase: build
  contracts:
    map: "contracts/integration-map.yaml"
    definitions: "contracts/*.ts"
  rules:
    - type: all-providers-implemented
    - type: all-consumers-satisfied
    - type: no-circular-dependencies
```

#### 3.3 Dun: conflict-detection Check Type

Detect overlapping agent work:

```yaml
- id: conflict-detection
  type: conflict-detection
  phase: iterate
  tracking:
    manifest: ".dun/work-in-progress.yaml"
    claim_pattern: "// WIP: agent-{id}"
  rules:
    - type: no-overlap
      scope: function
```

#### 3.4 Integrator Agent

New agent type for claude-flow:

```yaml
# agents/integrator.yaml
name: integrator
description: Assembles components into cohesive whole
responsibilities:
  - Verify all components satisfy contracts
  - Resolve integration conflicts
  - Maintain integration-map.yaml
  - Run assembly validation tests
prompts:
  pre_work: |
    You are the Integration Agent. Your role is to ensure all components
    work together cohesively. Before any implementation proceeds:
    1. Verify contracts are defined for all interfaces
    2. Check for conflicts with existing components
    3. Validate integration tests exist
```

---

### Phase 4: Structured Handoffs (Week 4)

**Objective**: Agent transitions preserve context and enforce contracts.

#### 4.1 Handoff Protocol

Define standard handoff structure:

```typescript
interface AgentHandoff {
  from: string;          // Source agent ID
  to: string;            // Target agent ID
  completed: {
    objective: string;
    outcome: "success" | "partial" | "blocked";
    artifacts: string[];
    spec_updates: string[];
  };
  next_task: {
    objective: string;
    constraints: string[];
    must_integrate_with: string[];
    success_criteria: string[];
    contracts_to_satisfy: string[];
  };
  shared_state_updates: Record<string, unknown>;
}
```

#### 4.2 Handoff Validation

```yaml
# Dun check
- id: handoff-validation
  type: agent
  phase: iterate
  prompt: prompts/validate-handoff.md
  inputs:
    - ".dun/handoffs/*.json"
  rules:
    - all_fields_present
    - contracts_referenced
    - integration_points_listed
```

---

## Tool Specifications

### Specification 1: Dun Check Types

**File**: `docs/specs/dun-spec-enforcement-checks.md`

```markdown
# Dun Spec Enforcement Check Types

## spec-binding

**Purpose**: Verify bidirectional references between specifications and code.

**Configuration**:
```yaml
- id: example-spec-binding
  type: spec-binding
  phase: build
  bindings:
    specs:
      - pattern: "docs/specs/*.md"
        implementation_section: "## Implementation"
        id_pattern: "SPEC-\\d+"
    code:
      - pattern: "src/**/*.ts"
        spec_comment: "// Spec: SPEC-"
  rules:
    - type: bidirectional-coverage
      min_coverage: 0.8
    - type: no-orphan-code
      warn_only: true
```

**Implementation Requirements**:
1. Parse spec files for implementation sections
2. Parse code files for spec reference comments
3. Build bidirectional mapping
4. Report coverage percentage
5. List orphaned specs (no code) and orphaned code (no spec)

---

## change-cascade

**Purpose**: Detect when upstream changes require downstream updates.

**Configuration**:
```yaml
- id: example-cascade
  type: change-cascade
  phase: iterate
  cascade_rules:
    - upstream: "docs/prd.md"
      downstreams:
        - path: "docs/design/*.md"
          sections: ["## Requirements"]
        - path: "src/**/*.ts"
          required: false
  trigger: git-diff
  baseline: HEAD~1
```

**Implementation Requirements**:
1. Detect changed files via git diff
2. Match against upstream patterns
3. Check downstream modification timestamps
4. Report stale downstreams
5. Support section-level tracking (optional)

---

## integration-contract

**Purpose**: Verify components define and satisfy integration interfaces.

**Configuration**:
```yaml
- id: example-contracts
  type: integration-contract
  phase: build
  contracts:
    map: "contracts/integration-map.yaml"
    definitions: "contracts/*.ts"
  rules:
    - type: all-providers-implemented
    - type: all-consumers-satisfied
    - type: interface-compatibility
```

**Implementation Requirements**:
1. Parse integration map YAML
2. Parse TypeScript interface definitions
3. Scan codebase for implementations
4. Verify all providers have implementations
5. Verify all consumers can find their dependencies
6. Check interface signature compatibility

---

## conflict-detection

**Purpose**: Detect overlapping work before commit.

**Configuration**:
```yaml
- id: example-conflict
  type: conflict-detection
  phase: iterate
  tracking:
    manifest: ".dun/wip.yaml"
    claim_pattern: "// WIP: {agent}"
  rules:
    - type: no-overlap
      scope: function
    - type: claim-before-edit
```

**Implementation Requirements**:
1. Maintain WIP manifest with claimed files/functions
2. Parse claim markers in code
3. Detect overlapping claims
4. Report conflicts before they're committed
```

### Specification 2: DDX/Claude-Flow Enhancements

**File**: `docs/specs/ddx-spec-enforcement.md`

```markdown
# DDX Spec Enforcement Enhancements

## Memory Namespace: spec-registry

**Purpose**: Store bidirectional mappings between code files and specifications.

**Schema**:
```json
{
  "namespace": "spec-registry",
  "entries": {
    "<code-file-path>": {
      "spec": "<spec-file-path>",
      "sections": ["<section-name>"],
      "last_synced": "<ISO-8601>",
      "spec_hash": "<sha256>"
    }
  }
}
```

**CLI Commands**:
```bash
# Register a binding
ddx spec register --code "internal/auth.go" --spec "docs/FEAT-001.md"

# Check binding status
ddx spec status --code "internal/auth.go"

# Find all code for a spec
ddx spec find --spec "docs/FEAT-001.md"

# Detect drift
ddx spec drift --since HEAD~5
```

---

## New Agent Type: integrator

**Purpose**: Coordinate assembly of components into cohesive whole.

**Responsibilities**:
1. Maintain `contracts/integration-map.yaml`
2. Verify all interfaces are defined before implementation
3. Run integration tests after component completion
4. Resolve conflicts between agent implementations
5. Approve or reject component integrations

**Trigger Conditions**:
- Multiple agents complete related work
- New component is ready for integration
- Contract changes detected

**Prompts**:
```markdown
# Integrator Agent

You are responsible for ensuring components work together.

## Before Integration
1. Verify contracts exist for all interfaces
2. Check for conflicts with existing components
3. Ensure integration tests are defined

## During Integration
1. Run integration test suite
2. Verify no regressions
3. Update integration-map.yaml

## After Integration
1. Notify dependent agents
2. Update shared state
3. Document any deviations
```

---

## Hook Enhancements

### pre-edit Hook

**Enhanced Behavior**:
```bash
# Automatically inject spec context
SPEC=$(ddx spec status --code "$FILE" --json | jq -r '.spec')
if [ -n "$SPEC" ]; then
  echo "📋 Governing Spec: $SPEC"
  echo ""
  cat "$SPEC" | head -50
  echo ""
  echo "⚠️ You MUST update this spec if changing behavior."
fi
```

### post-edit Hook

**Enhanced Behavior**:
```bash
# Verify spec was updated if code changed
CODE_CHANGED=$(git diff --name-only "$FILE")
SPEC=$(ddx spec status --code "$FILE" --json | jq -r '.spec')

if [ -n "$CODE_CHANGED" ] && [ -n "$SPEC" ]; then
  SPEC_CHANGED=$(git diff --name-only "$SPEC")
  if [ -z "$SPEC_CHANGED" ]; then
    echo "⚠️ WARNING: Code changed but spec not updated"
    echo "Consider updating: $SPEC"
  fi
fi
```

---

## Beads Integration

**Upstream Beads Mapping**:
```bash
bd create "Implement auth handler" \
  --type task \
  --spec-id docs/FEAT-001.md \
  --labels helix,phase:build,kind:build,area:auth \
  --description "Governing specification: docs/FEAT-001.md" \
  --design "Provides: AuthProvider. Consumes: UserRepository." \
  --acceptance "tests/integration/auth_test.go passes before close."
```

- governing specification: native `spec-id`
- contracts: `design` or structured metadata
- integration tests: `acceptance`
- blockers: native dependency graph via `bd dep add`

**Beads Plugin Checks**:
```yaml
# dun beads plugin
checks:
  - id: bead-spec-binding
    type: spec-binding
    description: "Verify bead has governing spec_id"
    rules:
      - type: field-required
        field: spec_id
      - type: file-exists
        field: spec_id

  - id: bead-contracts
    type: integration-contract
    description: "Verify bead contracts are satisfied"
    rules:
      - type: design-contracts-defined
      - type: acceptance-tests-exist
```
```

---

## Success Metrics

| Metric | Baseline | Target | Measurement |
|--------|----------|--------|-------------|
| Spec coverage | ~20% | 80% | `dun check spec-binding` |
| Cascade compliance | Unknown | 95% | `dun check change-cascade` |
| Contract coverage | 0% | 100% | `dun check integration-contract` |
| Conflict rate | Unknown | <5% | `dun check conflict-detection` |
| Integration test pass rate | Unknown | 100% | CI pipeline |

---

## Implementation Order

1. **Immediate** (this week):
   - Enable claude-flow pre-edit hook
   - Create spec-registry namespace
   - Add `// Implements: SPEC-XXX` comments to existing code

2. **Short-term** (2 weeks):
   - Implement `spec-binding` check type in dun
   - Implement `change-cascade` check type in dun
   - Add pre-commit spec-drift gate

3. **Medium-term** (4 weeks):
   - Implement `integration-contract` check type
   - Implement `conflict-detection` check type
   - Create integrator agent definition

4. **Long-term** (6 weeks):
   - Full handoff protocol implementation
   - Beads integration with spec enforcement
   - Assembly validation automation

---

## References

- [Claude-flow V3 Documentation](https://github.com/ruvnet/claude-flow)
- [Dun Plugin Architecture](file:///home/erik/gt/dun/crew/fionn/internal/dun/engine.go)
- [Multi-Agent Coordination Patterns - Anthropic](https://www.anthropic.com/engineering/multi-agent-research-system)
- [Contract-Driven Development - InfoQ](https://www.infoq.com/articles/contract-driven-development/)
- [Multi-Agent Failure Taxonomy - arxiv](https://arxiv.org/pdf/2503.13657)
