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

Create a `.ssq.yaml` in your project root. For IDE support (autocomplete and validation), add the schema reference:

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/therealklanni/secret-squirrel/main/schema/ssq.schema.json

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

### Schema

The configuration schema supports:

- `severity`: Global minimum severity level (`LOW`, `MEDIUM`, `HIGH`, `CRITICAL`)
- `ignore_patterns`: Array of regex patterns to ignore
- `ignore_paths`: Array of glob patterns for ignored paths
- `patterns`: Object containing detection patterns
  - Each pattern requires:
    - `description`: Human-readable description
    - `regex`: Regular expression pattern
    - `severity`: Pattern-specific severity level

## License

MIT © Kevin Lanni
