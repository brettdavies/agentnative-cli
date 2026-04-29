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

`anc check <target> --output json` emits a `schema_version: "0.4"` scorecard. The schema is at `0.x` while `anc` is
pre-launch — shape may evolve before first public release, when it locks at `1.0`. During `0.x`, additive fields are the
norm; consumers should feature-detect new keys rather than pinning to an exact value. The current shape includes the
following scorecard-level fields beyond the base `results` / `summary`:

- `audience` — `"agent-optimized"` / `"mixed"` / `"human-primary"` / `null`. Derived from 4 signal behavioral checks
  (`p1-non-interactive`, `p2-json-output`, `p7-quiet`, `p6-no-color-behavioral`). Informational only; never gates totals
  or exit codes.
- `audience_reason` — present only when `audience` is `null`. Values: `"suppressed"` (signal check masked by
  `--audit-profile`) or `"insufficient_signal"` (signal check never produced). Tells an agent *why* there's no label.
- `audit_profile` — echoes the applied `--audit-profile <category>` flag value. `null` when no profile is set.
- `coverage_summary.{must,should,may}.verified` — requirements verified by a check that actually ran. Checks suppressed
  by `--audit-profile` do not count as verified; suppression means verification was intentionally skipped.
- `spec_version` — the `agentnative-spec` version this CLI was built against. Sourced at build time from
  `src/principles/spec/VERSION` by `build.rs`; reads `"unknown"` if that file was missing at build time. Pin against
  this to know which spec contract the scorecard's requirement IDs reference.
- `tool` — `{ name, binary, version }`. Identifies what was scored. `version` is best-effort (manifest field for project
  mode, `<bin> --version` / `-V` for binary/command mode); `null` when probing fails or is declined by the self-spawn
  guard. Schema `0.4` addition.
- `anc` — `{ version, commit }`. Identifies the `anc` build that produced the scorecard. `commit` is `null` for builds
  outside a Git checkout. Informational, not signed provenance. Schema `0.4` addition.
- `run` — `{ invocation, started_at, duration_ms, platform: { os, arch } }`. `invocation` reflects what the user typed
  (captured pre-injection). `started_at` is RFC 3339 UTC. Schema `0.4` addition.
- `target` — `{ kind, path, command }`. `kind` is `"project"` / `"binary"` / `"command"`. The unused field is always
  `null`, never missing. Schema `0.4` addition.

`--audit-profile` accepts exactly 4 values: `human-tui`, `file-traversal`, `posix-utility`, `diagnostic-only`. Unknown
values exit 2 with a structured error. The full per-category mapping of suppressed check IDs is committed to
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

Exit 2 is overloaded. To distinguish "ran but found problems" from "called incorrectly", parse stderr — usage errors
include `Usage:` text; check failures don't.

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

The canonical specification of the 7 agent-readiness principles lives in
[`brettdavies/agentnative`](https://github.com/brettdavies/agentnative), one file per principle under `principles/`. A
snapshot is **vendored** into this crate at `src/principles/spec/`, and `build.rs` parses its frontmatter at build time
to generate the `REQUIREMENTS` slice — IDs in the spec frontmatter are the contract this CLI checks against. There is no
manual sync of requirement IDs; only the `Check::covers()` declarations are hand-maintained.

The `anc` checks in `src/checks/` themselves are derived **manually** from each principle's prose. When a principle's
spec adds, removes, or reworks a requirement, propagate to the relevant check(s) deliberately.

**Resync cadence:** rerun `scripts/sync-spec.sh` after every new `agentnative-spec` tag. The script queries the remote
for the latest `v*` tag automatically and falls back to a local checkout (`$HOME/dev/agentnative-spec` by default) if
the remote is unreachable. The companion `repository_dispatch` from the spec's publish workflow is the canonical
trigger; if a future GitHub Action opens a resync PR automatically, this script becomes that action's body.

For iteration workflow, pressure-test protocol, and per-file structure of the spec itself, see
[`agentnative:principles/AGENTS.md`](https://github.com/brettdavies/agentnative/blob/main/principles/AGENTS.md). Read
before proposing a new check that stretches the existing `P<n>` coverage.

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

When an extract names concrete linter-rule candidates, walk its **"Linter rule coverage audit"** or equivalent section
against existing checks in `src/checks/` before opening a new check.
