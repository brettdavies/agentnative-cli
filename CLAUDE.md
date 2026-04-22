# agentnative

The agent-native CLI linter. Checks whether CLI tools follow 7 agent-readiness principles.

## Architecture

Two-layer check system:

- **Behavioral checks** — run the compiled binary, language-agnostic (any CLI)
- **Source checks** — ast-grep pattern matching via bundled `ast-grep-core` crate (Rust, Python at launch)
- **Project checks** — file existence, manifest inspection

Design doc: `~/.gstack/projects/brettdavies-agentnative/brett-main-design-20260327-214808.md`

## Skill Routing

When the user's request matches an available skill, ALWAYS invoke it using the Skill
tool as your FIRST action. Do NOT answer directly, do NOT use other tools first.

**gstack skills (ideation, planning, shipping, ops):**

- Product ideas, "is this worth building", brainstorming → invoke office-hours
- Plan review, scope challenge, "think bigger" → invoke autoplan (or plan-ceo-review, plan-eng-review)
- Ship, deploy, push, create PR → invoke ship
- Bugs, errors, "why is this broken" → invoke investigate
- What did we learn, persist learnings → invoke learn
- Weekly retro → invoke retro
- Security audit → invoke cso
- Second opinion → invoke codex

**compound-engineering skills (code loop):**

- Implementation plan from repo code → invoke ce-plan
- Write code following a plan → invoke ce-work
- Code review before PR → invoke ce-review
- Document solution in docs/solutions/ → invoke ce-compound

For the full routing table, see `~/.claude/skills/docs/workflow-routing.md`.

## Documented Solutions

`docs/solutions/` (symlink to `~/dev/solutions-docs/`) — searchable archive of past
solutions and best practices, organized by category with YAML frontmatter (`module`, `tags`, `problem_type`). Search
with `qmd query "<topic>" --collection solutions`. Relevant when implementing or debugging in documented areas.

## gstack Project History

This project was designed in the `brettdavies/agent-skills` repo, then moved here. gstack project data (design doc, eng
review, naming rationale, review history) has been copied to `~/.gstack/projects/brettdavies-agentnative/`.

Key decisions already made:

- Name: `agentnative` with `anc` alias (see naming rationale)
- Approach B: bundled ast-grep hybrid (behavioral + source checks)
- ast-grep-core v0.42.0 validated via spike (3 PoC checks, 18 tests pass)
- Eng review: CLEARED, 10 issues resolved, 1 critical gap addressed
- Codex review: 12 findings, 3 actioned

## Conventions

- `ast-grep-core` and `ast-grep-language` pinned to exact version (`=0.42.0`) — pre-1.0 API
- `Position` uses `.line()` / `.column(&node)` methods, not tuple access
- Pre-build `Pattern` objects for `find_all()` — `&str` rebuilds on every node
- Feature flag is `tree-sitter-rust`, not `language-rust`
- Edition 2024, dual MIT/Apache-2.0 license

## Source Check Convention

Most source checks follow this structure (a few legacy helpers in `output_module.rs` and `error_types.rs` use
different helper shapes but still satisfy the core contract that `run()` is the sole `CheckResult` constructor):

- **Struct** implements `Check` trait with `id()`, `group()`, `layer()`, `applicable()`, `run()`
- **`check_x()` helper** takes `(source: &str)` (or `(source: &str, file: &str)` when evidence needs file location
  context) and returns `CheckStatus` (not `CheckResult`) — this is the unit-testable core
- **`run()` is the sole `CheckResult` constructor** — uses `self.id()`, `self.group()`, `self.layer()` to build the
  result. Never hardcode ID/group/layer string literals in `check_x()` or anywhere outside `run()`
- **Tests call `check_x()`** and match on `CheckStatus` directly, not `result.status`

This prevents ID triplication (the same string literal in `id()`, `run()`, and `check_x()`) and ensures the `Check`
trait is the single source of truth for check metadata.

For cross-language pattern helpers, use `source::has_pattern_in()` / `source::find_pattern_matches_in()` /
`source::has_string_literal_in()` with a `Language` parameter — do not write private per-language helpers in individual
check files.

## Principle Registry

`src/principles/registry.rs` is the single source of truth linking spec requirements (MUSTs, SHOULDs, MAYs across P1–P7)
to the checks that verify them. IDs follow `p{N}-{level}-{key}` and are stable once published — scorecards and the
coverage matrix pin against them.

- Add requirements by appending to the `REQUIREMENTS` static slice, grouped by principle then level (MUST → SHOULD →
  MAY).
- Bumping `registry_size_matches_spec` or `level_counts_match_spec` is a deliberate act — the tests exist to flag
  unintentional growth. Update both counter tests plus the summary prose in `docs/coverage-matrix.md` when the registry
  grows.
- `Applicability::Universal` means every CLI; `Applicability::Conditional(reason)` names the gate in prose so the matrix
  and the site `/coverage` page can render it.
- `ExceptionCategory` drives `--audit-profile` suppression. The `SUPPRESSION_TABLE` maps each variant to the check IDs
  it suppresses; drift tests fail the build if a category has no entry or a listed check ID isn't in the catalog. Adding
  a fifth category requires a plan revision — the four v0.1.3 categories (`human-tui`, `file-traversal`,
  `posix-utility`, `diagnostic-only`) are the committed surface.

## covers() Declaration

Each `Check` declares which requirements it evidences via `fn covers(&self) -> &'static [&'static str]`. The default
returns `&[]` — checks opt in explicitly. Return a static slice; never allocate. For a check that verifies multiple
requirements, list them all:

```rust
fn covers(&self) -> &'static [&'static str] {
    &["p1-must-no-interactive", "p1-should-tty-detection"]
}
```

The drift detector (`dangling_cover_ids` in `src/principles/matrix.rs`) fails the build if any ID returned by `covers()`
is missing from the registry — typos surface at test time, not at render time.

## Coverage Matrix Artifact Lifecycle

`anc generate coverage-matrix` emits two committed artifacts:

- `docs/coverage-matrix.md` — human-readable table, grouped by principle.
- `coverage/matrix.json` — machine-readable (`schema_version: "1.0"`), consumed by the `agentnative-site` `/coverage`
  page.

Both files are tracked in git, not `.gitignore`d. `anc generate coverage-matrix --check` exits non-zero when the
committed artifacts disagree with the current registry + `covers()` declarations. The integration test
`test_generate_coverage_matrix_drift_check_passes_on_committed_artifacts` mirrors this behavior so CI catches drift from
either source.

Regenerate whenever you add a requirement, change a check's `covers()`, or rename a check ID. The regeneration is a
deliberate commit, not a build-time artifact — the matrix is citable from outside this repo.

## Scorecard v1.1 Fields

`src/scorecard/mod.rs` emits `schema_version: "1.1"` with three additions over the v1.0 shape:

- `coverage_summary` — three-way `{must, should, may} × {total, verified}` counts, computed from the checks that
  actually ran. Populated every run.
- `audience` — `Option<String>`, derived by `src/scorecard/audience.rs::classify()` from the 4 signal behavioral checks.
  Emits `"agent_optimized"`, `"mixed"`, `"human_primary"`, or `null` when any signal check is missing from results
  (including when suppressed by `--audit-profile`). The classifier is read-only over results and never gates totals or
  exit codes — per CEO review Finding #3, label mismatches are fixed via registry, not classifier logic.
- `audit_profile` — `Option<String>`, echoes the applied `--audit-profile` flag value (`"human-tui"`,
  `"file-traversal"`, `"posix-utility"`, `"diagnostic-only"`). `null` when no profile is set.

Consumers (notably the site's `/score/<tool>` page) must feature-detect the new fields — pre-v1.1 scorecards lack
them. v0.1.2 scorecards carry `audience: null` and `audit_profile: null`; v0.1.3+ populates both.

## Dogfooding Safety

Behavioral checks spawn the target binary as a child process. When dogfooding (`anc check .`), the target IS
agentnative. Two rules prevent recursive fork bombs:

1. **Bare invocation prints help** (`cli.rs`): `arg_required_else_help = true` means children spawned with no args get
   instant help output instead of running `check .`. This is also correct CLI behavior (P1 principle).
2. **Safe probing only** (`json_output.rs`): Subcommands are probed with `--help`/`--version` suffixes only, never bare.
   Bare `subcmd --output json` is unsafe for any CLI with side-effecting subcommands.

**Rules for new behavioral checks:**

- NEVER probe subcommands without `--help`/`--version` suffixes
- NEVER remove `arg_required_else_help` from `Cli` — it prevents recursive self-invocation

## CI and Quality

**Toolchain pin:** `rust-toolchain.toml` pins the channel to a specific `X.Y.Z` version with a trailing comment naming
the rustc commit SHA. Rustup reads this file on every `cargo` invocation — both local and CI snap to identical bits.
Rustup verifies component SHA256s from the distribution manifest, so the version pin is effectively a SHA pin (the
manifest is the toolchain's "lockfile"). Bumping the toolchain is a reviewed PR that updates `rust-toolchain.toml`; no
runtime `rustup update` anywhere. Policy: bump only after a new stable has aged ≥7 days (supply-chain quarantine).

**Pre-push hook:** `scripts/hooks/pre-push` mirrors CI exactly: fmt, clippy with `-Dwarnings`, test, cargo-deny, and a
Windows compatibility check. Tracked in git and activated via `core.hooksPath`. After cloning, run: `git config
core.hooksPath scripts/hooks`

**Windows compatibility:** Only `libc` belongs in `[target.'cfg(unix)'.dependencies]`. All SIGPIPE/signal code must be
inside `#[cfg(unix)]` blocks. The pre-push hook checks this statically.

**After pushing:** Check CI status in the background with `gh run watch --exit-status` (use `run_in_background: true` so
it doesn't block). Report failures when notified.
