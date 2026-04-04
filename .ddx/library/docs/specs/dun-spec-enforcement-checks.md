# Dun Spec Enforcement Check Types

## Overview

This specification defines new check types for dun to enforce spec-driven development. These checks address three core problems:
1. Agents forgetting to update specs
2. Spec-code drift
3. Isolated implementation without cohesion

---

## Check Type: spec-binding

**Purpose**: Verify bidirectional references between specifications and code.

### Configuration Schema

```yaml
- id: string                    # Unique check identifier
  type: spec-binding            # Check type
  phase: frame|design|test|build|deploy|iterate
  description: string           # Human-readable description

  bindings:
    specs:                      # Specification file patterns
      - pattern: string         # Glob pattern for spec files
        implementation_section: string  # Section containing impl refs
        id_pattern: string      # Regex for spec IDs (e.g., "FEAT-\\d+")

    code:                       # Code file patterns
      - pattern: string         # Glob pattern for code files
        spec_comment: string    # Comment pattern linking to specs
        # e.g., "// Implements: FEAT-" or "# Spec: US-"

  rules:
    - type: bidirectional-coverage
      min_coverage: float       # 0.0-1.0, required coverage ratio

    - type: no-orphan-code
      warn_only: boolean        # If true, warn but don't fail

    - type: no-orphan-specs
      warn_only: boolean
```

### Example Configuration

```yaml
- id: feature-spec-binding
  type: spec-binding
  phase: build
  description: "Verify feature specs have implementations"

  bindings:
    specs:
      - pattern: "docs/helix/01-frame/features/FEAT-*.md"
        implementation_section: "## Implementation"
        id_pattern: "FEAT-\\d+"

    code:
      - pattern: "internal/**/*.go"
        spec_comment: "// Implements: FEAT-"
      - pattern: "cmd/**/*.go"
        spec_comment: "// Implements: FEAT-"

  rules:
    - type: bidirectional-coverage
      min_coverage: 0.8
    - type: no-orphan-specs
      warn_only: false
```

### Implementation Requirements

1. **Spec Parser**
   - Load all files matching spec patterns
   - Extract IDs using `id_pattern` regex
   - Parse `implementation_section` for code references

2. **Code Parser**
   - Load all files matching code patterns
   - Extract spec references from `spec_comment` patterns
   - Build map: `code_file -> [spec_ids]`

3. **Bidirectional Mapping**
   ```
   specs_to_code: Map<spec_id, code_file[]>
   code_to_specs: Map<code_file, spec_id[]>
   ```

4. **Coverage Calculation**
   ```
   coverage = specs_with_code / total_specs
   ```

5. **Output**
   ```json
   {
     "status": "pass|warn|fail",
     "signal": "Spec coverage: 85%",
     "detail": "42/50 specs have implementations",
     "issues": [
       {"type": "orphan-spec", "path": "FEAT-023.md", "reason": "No implementation found"},
       {"type": "orphan-code", "path": "internal/legacy.go", "reason": "No spec reference"}
     ]
   }
   ```

### Go Implementation Skeleton

```go
// internal/dun/spec_binding.go

type SpecBindingConfig struct {
    Bindings struct {
        Specs []struct {
            Pattern              string `yaml:"pattern"`
            ImplementationSection string `yaml:"implementation_section"`
            IDPattern            string `yaml:"id_pattern"`
        } `yaml:"specs"`
        Code []struct {
            Pattern     string `yaml:"pattern"`
            SpecComment string `yaml:"spec_comment"`
        } `yaml:"code"`
    } `yaml:"bindings"`
    Rules []struct {
        Type        string  `yaml:"type"`
        MinCoverage float64 `yaml:"min_coverage"`
        WarnOnly    bool    `yaml:"warn_only"`
    } `yaml:"rules"`
}

func runSpecBindingCheck(root string, check Check) (CheckResult, error) {
    var config SpecBindingConfig
    // Parse check.Rules into config

    // 1. Find all spec files
    specFiles := findFiles(root, config.Bindings.Specs[0].Pattern)

    // 2. Extract spec IDs and implementation refs
    specMap := make(map[string][]string) // spec_id -> code_refs
    for _, sf := range specFiles {
        ids, refs := parseSpec(sf, config.Bindings.Specs[0])
        for _, id := range ids {
            specMap[id] = refs
        }
    }

    // 3. Find all code files
    codeFiles := findFiles(root, config.Bindings.Code[0].Pattern)

    // 4. Extract spec references from code
    codeMap := make(map[string][]string) // code_file -> spec_ids
    for _, cf := range codeFiles {
        refs := parseCodeRefs(cf, config.Bindings.Code[0].SpecComment)
        codeMap[cf] = refs
    }

    // 5. Calculate coverage
    // 6. Build issues list
    // 7. Return result
}
```

---

## Check Type: change-cascade

**Purpose**: Detect when upstream changes require downstream updates.

### Configuration Schema

```yaml
- id: string
  type: change-cascade
  phase: iterate
  description: string

  cascade_rules:
    - upstream: string          # File or glob pattern
      downstreams:
        - path: string          # File or glob pattern
          sections: string[]    # Optional: specific sections
          required: boolean     # If false, warn only

  trigger: git-diff|always      # When to check
  baseline: string              # Git ref for comparison (default: HEAD~1)
```

### Example Configuration

```yaml
- id: prd-cascade
  type: change-cascade
  phase: iterate
  description: "PRD changes cascade to design and features"

  cascade_rules:
    - upstream: "docs/helix/01-frame/prd.md"
      downstreams:
        - path: "docs/helix/02-design/architecture.md"
          sections: ["## Requirements Mapping", "## Components"]
          required: true
        - path: "docs/helix/01-frame/features/*.md"
          required: true

    - upstream: "docs/helix/01-frame/features/*.md"
      downstreams:
        - path: "internal/**/*.go"
          required: false
        - path: "docs/helix/03-test/test-plan.md"
          required: true

  trigger: git-diff
  baseline: HEAD~1
```

### Implementation Requirements

1. **Change Detection**
   - Run `git diff --name-only $baseline HEAD`
   - Filter to files matching upstream patterns

2. **Cascade Analysis**
   - For each changed upstream file:
     - Find all downstream patterns
     - Check if downstream files were also modified
     - If sections specified, check section content hashes

3. **Staleness Detection**
   ```
   upstream_mtime > downstream_mtime → stale
   ```

4. **Output**
   ```json
   {
     "status": "fail",
     "signal": "3 downstream files need updates",
     "issues": [
       {
         "type": "stale-downstream",
         "upstream": "docs/helix/01-frame/prd.md",
         "downstream": "docs/helix/02-design/architecture.md",
         "sections": ["## Requirements Mapping"]
       }
     ],
     "next": "Update downstream files to reflect upstream changes"
   }
   ```

---

## Check Type: integration-contract

**Purpose**: Verify components define and satisfy integration interfaces.

### Configuration Schema

```yaml
- id: string
  type: integration-contract
  phase: build
  description: string

  contracts:
    map: string                 # Path to integration-map.yaml
    definitions: string         # Glob for interface definitions

  rules:
    - type: all-providers-implemented
    - type: all-consumers-satisfied
    - type: no-circular-dependencies
    - type: interface-compatibility
```

### Integration Map Schema

```yaml
# contracts/integration-map.yaml
components:
  auth-service:
    provides:
      - name: AuthProvider
        definition: contracts/auth.ts
    consumes:
      - name: UserRepository
        from: user-service

  user-service:
    provides:
      - name: UserRepository
        definition: contracts/user.ts
    consumes: []
```

### Example Configuration

```yaml
- id: component-contracts
  type: integration-contract
  phase: build
  description: "Verify all component contracts are satisfied"

  contracts:
    map: "contracts/integration-map.yaml"
    definitions: "contracts/*.ts"

  rules:
    - type: all-providers-implemented
    - type: all-consumers-satisfied
    - type: no-circular-dependencies
```

### Implementation Requirements

1. **Parse Integration Map**
   - Load YAML file
   - Build component graph

2. **Verify Providers**
   - For each component.provides:
     - Check implementation exists in codebase
     - Verify signature matches definition

3. **Verify Consumers**
   - For each component.consumes:
     - Verify provider exists
     - Check interface compatibility

4. **Cycle Detection**
   - Build dependency graph
   - Run cycle detection algorithm

5. **Output**
   ```json
   {
     "status": "fail",
     "signal": "1 provider not implemented, 1 consumer unsatisfied",
     "issues": [
       {"type": "missing-provider", "component": "auth-service", "interface": "AuthProvider"},
       {"type": "unsatisfied-consumer", "component": "payment-service", "needs": "BillingProvider"}
     ]
   }
   ```

---

## Check Type: conflict-detection

**Purpose**: Detect overlapping work before commit.

### Configuration Schema

```yaml
- id: string
  type: conflict-detection
  phase: iterate
  description: string

  tracking:
    manifest: string            # Path to WIP manifest
    claim_pattern: string       # Pattern in code marking claimed sections

  rules:
    - type: no-overlap
      scope: file|function|line
    - type: claim-before-edit
      required: boolean
```

### WIP Manifest Schema

```yaml
# .dun/work-in-progress.yaml
claims:
  - agent: "agent-123"
    files:
      - path: "internal/auth/handler.go"
        scope: file
        claimed_at: "2026-01-31T10:00:00Z"

  - agent: "agent-456"
    files:
      - path: "internal/auth/handler.go"
        scope: function
        function: "HandleLogin"
        claimed_at: "2026-01-31T10:05:00Z"
```

### Example Configuration

```yaml
- id: wip-conflicts
  type: conflict-detection
  phase: iterate
  description: "Detect overlapping agent work"

  tracking:
    manifest: ".dun/work-in-progress.yaml"
    claim_pattern: "// WIP: {agent}"

  rules:
    - type: no-overlap
      scope: function
    - type: claim-before-edit
      required: true
```

### Implementation Requirements

1. **Parse WIP Manifest**
   - Load claims from YAML
   - Build file→claims mapping

2. **Detect Overlaps**
   - Check for same file claimed by multiple agents
   - If scope=function, check for same function
   - Report conflicts

3. **Validate Claims**
   - If claim-before-edit required:
     - Check git diff for modified files
     - Verify all modified files have claims

4. **Output**
   ```json
   {
     "status": "fail",
     "signal": "2 overlapping claims detected",
     "issues": [
       {
         "type": "overlap",
         "file": "internal/auth/handler.go",
         "agents": ["agent-123", "agent-456"],
         "scope": "function",
         "function": "HandleLogin"
       }
     ],
     "next": "Resolve claim conflicts before proceeding"
   }
   ```

---

## Check Type: agent-rule-injection

**Purpose**: Dynamically inject rules into agent prompts.

### Configuration Schema

```yaml
- id: string
  type: agent-rule-injection
  phase: build
  description: string

  base_prompt: string           # Path to base prompt template

  inject_rules:
    - source: string            # File path or "from_registry"
      section: string           # Where to inject in prompt

  enforce_rules:
    - id: string
      pattern: string           # Regex to verify in output
      required: boolean
```

### Example Configuration

```yaml
- id: coding-with-rules
  type: agent-rule-injection
  phase: build
  description: "Inject coding standards into agent prompt"

  base_prompt: "prompts/implement-feature.md"

  inject_rules:
    - source: ".dun/rules/coding-standards.yaml"
      section: "## Rules to Follow"
    - source: "docs/helix/01-frame/principles.md"
      section: "## Guiding Principles"
    - source: from_registry
      section: "## Governing Specs"

  enforce_rules:
    - id: must-reference-spec
      pattern: "Implements: FEAT-\\d+"
      required: true
    - id: must-have-tests
      pattern: "_test\\.go$"
      required: true
```

### Implementation Requirements

1. **Load Base Prompt**
   - Read template file
   - Identify injection points

2. **Load Rules**
   - For each inject_rules entry:
     - Load source file content
     - Or query spec-registry for file-specific rules

3. **Build Enhanced Prompt**
   - Insert rules at specified sections
   - Return as PromptEnvelope

4. **Validate Response**
   - When agent responds:
     - Check output against enforce_rules patterns
     - Fail if required patterns not present

---

## Plugin Manifest Updates

Add these check types to the helix plugin:

```yaml
# internal/plugins/builtin/helix/plugin.yaml
id: helix
version: "2.0.0"
description: "HELIX workflow with spec enforcement"

checks:
  # Existing checks...

  - id: helix-spec-binding
    type: spec-binding
    phase: build
    description: "Verify feature specs have implementations"
    bindings:
      specs:
        - pattern: "docs/helix/01-frame/features/FEAT-*.md"
          implementation_section: "## Implementation"
          id_pattern: "FEAT-\\d+"
      code:
        - pattern: "internal/**/*.go"
          spec_comment: "// Implements: FEAT-"
    rules:
      - type: bidirectional-coverage
        min_coverage: 0.8

  - id: helix-change-cascade
    type: change-cascade
    phase: iterate
    description: "Verify downstream updates when specs change"
    cascade_rules:
      - upstream: "docs/helix/01-frame/prd.md"
        downstreams:
          - path: "docs/helix/02-design/architecture.md"
            required: true
          - path: "docs/helix/01-frame/features/*.md"
            required: true
    trigger: git-diff

  - id: helix-integration-contracts
    type: integration-contract
    phase: build
    description: "Verify component contracts are satisfied"
    contracts:
      map: "contracts/integration-map.yaml"
      definitions: "contracts/*.ts"
    rules:
      - type: all-providers-implemented
      - type: all-consumers-satisfied

  - id: helix-conflict-detection
    type: conflict-detection
    phase: iterate
    description: "Detect overlapping agent work"
    tracking:
      manifest: ".dun/work-in-progress.yaml"
      claim_pattern: "// WIP: {agent}"
    rules:
      - type: no-overlap
        scope: function
```

---

## Testing Requirements

Each check type needs:
1. Unit tests for configuration parsing
2. Unit tests for core logic
3. Integration tests with sample repos
4. Edge case tests (empty files, missing patterns, etc.)

Example test structure:
```
internal/dun/
  spec_binding.go
  spec_binding_test.go
  change_cascade.go
  change_cascade_test.go
  integration_contract.go
  integration_contract_test.go
  conflict_detection.go
  conflict_detection_test.go
```
