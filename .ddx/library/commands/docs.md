---
description: Create and maintain project documentation following best practices
---

# Documentation Creation and Maintenance

Help create and maintain project documentation following best practices.

## Core Principles

1. **Search Before Creating** - Never create new docs without searching for existing content
2. **Never Archive** - Update in place, use version control for history
3. **Prefer Small, Focused Files** - One concept per file, 100-300 lines target

## Naming Conventions

- **Files**: lowercase with hyphens (`api-authentication.md`)
- **Folders**: lowercase, singular form (`architecture`)
- **Titles**: Title Case for main headings, sentence case for subheadings

## Recommended Structure

```
docs/
├── business/        # Strategy, requirements, stakeholders
├── product/         # Roadmaps, features, user stories
├── architecture/    # ADRs, diagrams, components
├── implementation/  # Setup, configuration, APIs
├── usage/           # Getting started, tutorials, how-to
├── development/     # Contributing, standards, testing
└── references/      # External references, research
```

## Document Template

```markdown
# Document Title

> **Last Updated**: YYYY-MM-DD
> **Status**: Draft | Review | Approved
> **Owner**: Team/Person Name

## Overview
Brief description of what this document covers.

## Context
Why this document exists and when to use it.

## Content
[Main content sections]

## Related Documents
- [[related-doc-1]]
- [[related-doc-2]]
```

## Tools
- **Markdown**: All text documentation (CommonMark + GFM)
- **Mermaid**: Flow charts, sequence diagrams, state machines
- **Excalidraw**: Architecture diagrams

What documentation would you like to create or improve?
