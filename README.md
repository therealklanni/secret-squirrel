# Secret Squirrel (ssq)

A command-line tool for detecting sensitive information in your code before it gets leaked. Think of it as your vigilant guardian against accidentally committing secrets, API keys, credentials, and PII.

## Features

- 🔍 Scans repositories for potential sensitive information
- 🕒 Digs through Git history to find previously committed secrets
- 🎯 Can focus on staged files only (perfect for git hooks)
- ⚙️ Configurable pattern matching and ignores
- 🚀 Written in Rust for maximum performance

## Installation

```bash
cargo install secret-squirrel
```

## Usage

Basic repository scan:
```bash
# Run from the root of your repository
ssq

# Or specify the path
ssq /path/to/repository
```

Scan only staged files:
```bash
ssq --staged
```

Scan Git history:
```bash
ssq --history
```

## Configuration

Create a `.ssq.yaml` in your project root:

```yaml
# Ignore specific patterns
ignore_patterns:
  - 'TEST_API_KEY=.*'
  - 'localhost:.*'
  - '^dummy_password=.*'

# Ignore specific files or directories
ignore_paths:
  - 'tests/fixtures/*'
  - '*.test.js'
  - 'docs/**/*'

# Custom severity levels for different patterns
patterns:
  - pattern: '[A-Za-z0-9]{40}'  # GitHub token pattern
    severity: critical
  - pattern: '(?i)password\s*=\s*.+'
    severity: high
  - pattern: '\b[\w\.-]+@[\w\.-]+\.\w+\b'  # Email pattern
    severity: medium
```

## Git Hook Setup

Add to `.git/hooks/pre-commit`:

```bash
#!/bin/sh
ssq --staged
```

Make it executable:
```bash
chmod +x .git/hooks/pre-commit
```

## License

MIT © [Your Name]
