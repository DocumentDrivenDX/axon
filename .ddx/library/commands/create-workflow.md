---
description: Create a new DDx workflow interactively
---

# Create New Workflow

I'll help you create a new workflow for DDx (Document-Driven eXperience). DDx workflows are structured processes that guide users through complex tasks using phases, artifacts, templates, and AI assistance.

## Workflow Creation Process

### Step 1: Understand the Domain

**What domain or process will this workflow address?**
- Software development
- Research and analysis
- Content creation
- Business process
- Quality assurance
- Operations and maintenance
- Other (please specify)

### Step 2: Define Core Elements

Once we understand the domain, I'll gather:

1. **Workflow Identity**
   - Name, description, author, version

2. **Key Phases**
   - Phase name and description
   - Primary objectives
   - Key activities
   - Entry/exit criteria

3. **Essential Artifacts**
   - Name and description
   - Which phase it belongs to
   - Required or optional
   - Template needs

### Step 3: Configuration Details

1. **Quality Gates** - Criteria to advance between phases
2. **Variables** - Customizable aspects per project
3. **Integration Points** - Git, external tools, automation

### Step 4: Generate Structure

I'll create:
```
workflows/[workflow-name]/
├── README.md
├── GUIDE.md
├── workflow.yml
├── phases/
│   ├── 01-[first-phase]/
│   ├── 02-[second-phase]/
│   └── ...
└── [artifact-directories]/
    ├── README.md
    ├── template.md
    ├── prompt.md
    └── examples/
```

Let's start! **What domain or process will this workflow address?**
