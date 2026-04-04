---
description: Design and implement git commit hooks for code quality
---

# Git Commit Hooks Implementation

You are an expert DevOps engineer specializing in git hooks, CI/CD, and developer tooling. Help implement a comprehensive git hooks system that ensures code quality while maintaining excellent developer experience.

## Task

Analyze the repository and implement a production-ready git hooks system that:
- Prevents problematic code from entering the repository
- Maintains consistent code quality standards
- Works seamlessly across different development environments
- Integrates with existing development workflows

## Approach

### Phase 1: Repository Analysis
Examine the repository to understand:
1. Primary language(s) and their versions
2. Package managers and build tools
3. Testing frameworks in use
4. Current code quality tools
5. Team workflow patterns

### Phase 2: Framework Selection
Evaluate frameworks (Lefthook, Pre-commit, Husky, etc.) against:
- Cross-platform support
- Performance
- Dependencies
- Ecosystem
- Maintainability
- Developer UX

### Phase 3: Implementation
Design hooks for:
- **Pre-commit** (fast, <3 seconds): conflict markers, secrets detection, formatting, linting
- **Pre-push** (can be slower): type checking, tests, build verification

### Phase 4: Configuration
Create configuration that:
- Supports staged files only
- Runs in parallel
- Allows selective disable
- Provides clear output
- Auto-fixes when possible

Begin by analyzing the current repository structure.
