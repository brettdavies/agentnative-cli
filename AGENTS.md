# AGENTS.md

## Running anc

The crate is `agentnative`. The installed binary is `anc`.

```bash
# Check current project — `check` is implicit when the first non-flag arg is a path
anc .

# Resolve a command on PATH and run behavioral checks against it
anc --command ripgrep

# JSON output for parsing
anc . --output json

# Quiet mode (warnings and failures only)
anc . -q

# Filter by principle (1-7)
anc . --principle 4

# Behavioral checks only (no source analysis)
anc . --binary

# Source checks only (no binary execution)
anc . --source
```

Bare `anc` (no arguments) prints help and exits 2. This is a non-negotiable fork-bomb guard: when agentnative dogfoods
itself, children spawned without arguments must not recurse into `check .`.

## Exit Codes

- `0` — all checks passed
- `1` — warnings present, no failures
- `2` — failures, errors, or usage errors (bare `anc`, unknown flag, mutually exclusive flags, command not found on
  PATH)

Exit 2 is overloaded. To distinguish "ran but found problems" from "called
incorrectly", parse stderr — usage errors include `Usage:` text; check failures don't.

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

## Spec source (principles)

The canonical specification of the 7 agent-readiness principles lives in the vault, one file per principle. The `anc`
checks in `src/checks/` are derived **manually** from these files — there is no build-time import, no live link. When a
principle's spec changes, propagate to the relevant check(s) deliberately.

- `~/obsidian-vault/Projects/brettdavies-agentnative/principles/index.md` — table of P1-P7 with status (draft /
  under-review / locked).
- `~/obsidian-vault/Projects/brettdavies-agentnative/principles/AGENTS.md` — iteration workflow, pressure-test protocol,
  per-file structure. Read before proposing a new check that stretches the existing P<n> coverage.

When a check is added or revised, its code or doc comment should name the principle code (`P<n>`) it implements for
traceability. Do not embed the principle text in the check source.

## External signal / research

Curated external signal that informs principle iteration, check rules, and positioning lives in the sibling research
folder:

- `~/obsidian-vault/Projects/brettdavies-agentnative/research/index.md` — top of the research tree. Lists every extract
  with date, topic, and which principles it maps to. Read this before adding new checks driven by external patterns or
  competitor behavior.
- `extracts/` — curated, topic-scoped files (verbatim quotes, principle mapping, recommended uses).
- `raw/` — full-text captures.

When an extract names concrete linter-rule candidates, walk its **"Linter rule coverage audit"** or equivalent
section against existing checks in `src/checks/` before opening a new check.
