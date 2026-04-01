# AGENTS.md

## Running agentnative

```bash
# Check current project
agentnative check .

# JSON output for parsing
agentnative check . --output json

# Quiet mode (warnings and failures only)
agentnative check . -q

# Filter by principle (1-7)
agentnative check . --principle 4

# Behavioral checks only (no source analysis)
agentnative check . --binary

# Source checks only (no binary execution)
agentnative check . --source
```

## Exit Codes

- `0` — all checks passed
- `1` — warnings present, no failures
- `2` — failures or errors detected

## Project Structure

- `src/check.rs` — Check trait definition
- `src/checks/behavioral/` — checks that run the compiled binary
- `src/checks/source/rust/` — ast-grep source analysis checks
- `src/checks/project/` — file and manifest inspection checks
- `src/runner.rs` — binary execution with timeout and caching
- `src/project.rs` — project discovery and source file walking
- `src/scorecard.rs` — output formatting (text and JSON)
- `src/types.rs` — CheckResult, CheckStatus, CheckGroup, CheckLayer

## Adding a New Check

1. Create a file in the appropriate `src/checks/` subdirectory
2. Implement the `Check` trait: `id()`, `group()`, `layer()`, `applicable()`, `run()`
3. Register in the layer's `mod.rs` (e.g., `all_rust_checks()`)
4. Add inline `#[cfg(test)]` tests

## Testing

```bash
cargo test                    # unit + integration tests
cargo test -- --ignored       # fixture tests (slower)
```
