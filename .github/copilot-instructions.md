This is a CLI tool to scour repositories for potential sensitive information leaks, such as API keys, credentials, PII, etc. similar to trufflehog but geared towards catching sensitive information before it gets leaked. It can also dig through Git history to identify potential secrets that have been committed in the past. It can be used stand-alone or as a git hook.

It can be configured (per project) to ignore certain patterns.

It can also be configured to only scan files that have been staged for commit via a commandline flag for use with git hooks.

The command will be `ssq` (short for Secret Squirrel).

This is a Rust project. Follow clippy recommended patterns. Don't unnecessarily add parentheses to if statements. Use variables directly in `format!` strings. Use 2 spaces for indentation.

`config.yml` is the correct filename for the installed base config, but it is copied from the ssq.yml file under the config dir.
