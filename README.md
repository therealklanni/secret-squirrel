# Secret Squirrel (ssq)

A command-line tool for detecting sensitive information in your code before it gets leaked. Think of it as your vigilant guardian against accidentally committing secrets, API keys, credentials, and PII.

## Features

- [x] 🚀 Written in Rust for maximum performance
- [x] 🔍 Scans repositories for potential sensitive information
- [x] ⚙️ Configurable pattern matching and ignores
- [ ] 🕒 Digs through Git history to find previously committed secrets
- [ ] 🎯 Can focus on staged files only (perfect for git hooks)

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
  github_token:
    description: GitHub personal access token pattern
    regex: '[A-Za-z0-9]{40}'
    severity: critical
  password:
    description: Generic password in configuration
    regex: '(?i)password\s*=\s*.+'
    severity: high
  email:
    description: Email addresses that might contain PII
    regex: '[a-zA-Z0-9._-]+@[a-zA-Z0-9._-]+\.[a-zA-Z0-9_-]+'
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
