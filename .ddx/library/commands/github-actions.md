---
description: Design and implement GitHub Actions CI/CD workflows
---

# GitHub Actions CI/CD Implementation

Design and implement comprehensive GitHub Actions workflows for CI/CD.

## Workflow Strategy

### Core Workflows

1. **ci.yml** - Main CI pipeline
   - Triggers: push (main/develop), pull_request
   - Jobs: lint, test, build, security
   - Strategy: parallel execution

2. **release.yml** - Release automation
   - Triggers: tag push (v*)
   - Jobs: test, build-matrix, create-release
   - Artifacts: binaries for all platforms

3. **security.yml** - Security scanning
   - Triggers: schedule (daily), manual
   - Jobs: dependency-check, SAST, secrets-scan

### Platform Build Matrix
```yaml
strategy:
  matrix:
    include:
      - os: ubuntu-latest, arch: amd64, target: linux-amd64
      - os: ubuntu-latest, arch: arm64, target: linux-arm64
      - os: windows-latest, arch: amd64, target: windows-amd64
      - os: macos-latest, arch: arm64, target: darwin-arm64
```

### Pre-commit to CI Mapping
If using a commit-hook tool (Lefthook, pre-commit, Husky), I'll leverage it in CI for consistency:
- Local hooks run on staged files
- CI runs same checks on entire codebase

### Security Best Practices
- Minimal permissions by default
- Pin action versions with SHA
- Use OIDC for cloud providers
- Generate SBOM/provenance for releases

## Analysis

Let me analyze your repository to understand:
- Current CI/CD setup
- Build commands and dependencies
- Test suites and coverage tools
- Existing pre-commit checks to mirror
- Platform-specific requirements

Starting analysis now.
