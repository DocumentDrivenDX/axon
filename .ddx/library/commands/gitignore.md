---
description: Evaluate and create a comprehensive .gitignore file
---

# .gitignore Evaluation and Creation

Evaluate and enhance the .gitignore file following best practices.

## Evaluation Checklist

### 1. Operating System Files
- **macOS**: `.DS_Store`, `.AppleDouble`, `._*`
- **Windows**: `Thumbs.db`, `Desktop.ini`, `$RECYCLE.BIN/`
- **Linux**: `*~`, `.directory`, `.Trash-*`

### 2. IDE and Editor Files
- **JetBrains**: `.idea/`, `*.iml`
- **VS Code**: `.vscode/`, `*.code-workspace`
- **Vim/Emacs**: `*.swp`, `*~`, `\#*\#`

### 3. Language/Framework Specific
Based on detected languages, ensure appropriate patterns for:
- Build artifacts and binaries
- Dependencies (node_modules, vendor, venv)
- Test outputs and coverage
- Type checking caches

### 4. Security and Credentials (Critical)
Must never be committed:
- `.env`, `.env.*` (but allow `.env.example`)
- `*.pem`, `*.key`, `*.cert`
- `secrets.yml`, `credentials.json`
- SSH keys, API tokens

### 5. Logs and Temporary Files
- `*.log`, `logs/`
- `*.tmp`, `tmp/`, `cache/`
- `*.bak`, `*.backup`

## Actions

1. Analyze the repository for languages and tools
2. Check existing .gitignore for gaps
3. Identify any committed files that should be ignored
4. Generate comprehensive .gitignore with comments
5. Provide cleanup commands for already-tracked files

Let me analyze your repository now.
