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

# Suppress inapplicable MUSTs for a categorical exception
anc . --audit-profile human-tui
```

Bare `anc` (no arguments) prints help and exits 2. This is a non-negotiable fork-bomb guard: when agentnative dogfoods
itself, children spawned without arguments must not recurse into `check .`.

## Agent-facing JSON surface

`anc check <target> --output json` emits a `schema_version: "1.1"` scorecard. Four fields are additive to v1.1 and v1.1
consumers feature-detect them:

- `audience` — `"agent-optimized"` / `"mixed"` / `"human-primary"` / `null`. Derived from 4 signal behavioral checks
  (`p1-non-interactive`, `p2-json-output`, `p7-quiet`, `p6-no-color-behavioral`). Informational only; never gates totals
  or exit codes.
- `audience_reason` — present only when `audience` is `null`. Values: `"suppressed"` (signal check masked by
  `--audit-profile`) or `"insufficient_signal"` (signal check never produced). Tells an agent *why* there's no label.
- `audit_profile` — echoes the applied `--audit-profile <category>` flag value. `null` when no profile is set.
- `coverage_summary.{must,should,may}.verified` — requirements verified by a check that actually ran. Checks suppressed
  by `--audit-profile` do not count as verified; suppression means verification was intentionally skipped.

`--audit-profile` accepts exactly 4 values: `human-tui`, `file-traversal`, `posix-utility`, `diagnostic-only`.
Unknown values exit 2 with a structured error. The full per-category mapping of suppressed check IDs is committed to
`coverage/matrix.json` under the `audit_profiles` section — agents should read that file rather than scraping `--help`:

```bash
jaq '.audit_profiles' coverage/matrix.json
```

Suppressed checks appear in `results[]` as `status: "skip"` with evidence starting with `"suppressed by audit_profile:
"` (the shared prefix is pinned in `src/principles/registry.rs` as `SUPPRESSION_EVIDENCE_PREFIX`).

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
- `src/principles/registry.rs` — single source of truth linking spec requirements (P1–P7 MUSTs/SHOULDs/MAYs) to the
  checks that verify them
- `src/principles/matrix.rs` — coverage-matrix generator + drift detector

## Adding a New Check

1. Create a file in the appropriate `src/checks/` subdirectory
2. Implement the `Check` trait: `id()`, `group()`, `layer()`, `applicable()`, `run()`, and `covers()` if the check
   verifies requirements in `src/principles/registry.rs` (return a `&'static [&'static str]` of requirement IDs)
3. Register in the layer's `mod.rs` (e.g., `all_rust_checks()`)
4. Add inline `#[cfg(test)]` tests
5. Regenerate the coverage matrix: `cargo run -- generate coverage-matrix` (produces `docs/coverage-matrix.md` +
   `coverage/matrix.json`, both tracked in git)

See `CLAUDE.md` §"Principle Registry" and §"`covers()` Declaration" for the registry conventions and drift-detector
behavior.

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
