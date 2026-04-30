# agentnative

The agent-native CLI linter. Checks whether CLI tools follow 7 agent-readiness principles.

## Architecture

Two-layer check system:

- **Behavioral checks** — run the compiled binary, language-agnostic (any CLI)
- **Source checks** — ast-grep pattern matching via bundled `ast-grep-core` crate (Rust, Python at launch)
- **Project checks** — file existence, manifest inspection

Design doc: `~/.gstack/projects/brettdavies-agentnative/brett-main-design-20260327-214808.md`

## Skill Routing

When the user's request matches an available skill, ALWAYS invoke it using the Skill tool as your FIRST action. Do NOT
answer directly, do NOT use other tools first.

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

`docs/solutions/` (symlink to `~/dev/solutions-docs/`) — searchable archive of past solutions and best practices,
organized by category with YAML frontmatter (`module`, `tags`, `problem_type`). Search with `qmd query "<topic>"
--collection solutions`. Relevant when implementing or debugging in documented areas.

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

Most source checks follow this structure (a few legacy helpers in `output_module.rs` and `error_types.rs` use different
helper shapes but still satisfy the core contract that `run()` is the sole `CheckResult` constructor):

- **Struct** implements `Check` trait with `id()`, `label()`, `group()`, `layer()`, `applicable()`, `run()`
- **`check_x()` helper** takes `(source: &str)` (or `(source: &str, file: &str)` when evidence needs file location
  context) and returns `CheckStatus` (not `CheckResult`) — this is the unit-testable core
- **No `Check` impl constructs `CheckResult` outside its own `run()`.** `run()` is the sole place each check assembles
  its own result — never hardcode ID/group/layer/label string literals in `check_x()` or anywhere outside `run()`. The
  runtime layer (`main::run`'s error and `--audit-profile` suppression branches) legitimately constructs `CheckResult`
  as a *second* site — it's the runner, not a `Check` impl, and it uses `check.id()`, `check.label()`, `check.group()`,
  `check.layer()` from the trait (never string literals). Test doubles (`FakeCheck` in `src/principles/matrix.rs` and
  `src/scorecard/mod.rs`) similarly sidestep the rule by design.
- **`label()` returns `&'static str`** and feeds the `label` field in `run()`'s `CheckResult`. Having the label on the
  trait also means the suppression and error branches can show the human label instead of falling back to the opaque
  `id`. See `src/check.rs`.
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

## Scorecard v0.4 Fields

`src/scorecard/mod.rs` emits `schema_version: "0.4"`. The schema evolves additively during the `0.x` pre-launch window —
consumers feature-detect each addition rather than pinning exact shape. Cumulative history:

- `0.2` — `coverage_summary` (three-way `{must, should, may} × {total, verified}` counts), `audience`, `audit_profile`.
- `0.3` — `spec_version` (vendored agentnative-spec version, sourced by `build.rs` from `src/principles/spec/VERSION`).
- `0.4` — four top-level objects making the scorecard self-describing: `tool`, `anc`, `run`, `target`.

Existing field semantics:

- `coverage_summary` — populated every run. Checks suppressed by `--audit-profile` do not count toward `verified`.
- `audience` — `Option<String>`, derived by `src/scorecard/audience.rs::classify()` from the 4 signal behavioral checks.
  Emits `"agent-optimized"`, `"mixed"`, `"human-primary"`, or `null` when any signal check is missing (including
  `--audit-profile` suppression). Read-only over results; never gates totals or exit codes — per CEO review Finding #3,
  label mismatches are fixed via registry, not classifier logic.
- `audit_profile` — `Option<String>`, echoes the applied `--audit-profile` flag value (`"human-tui"`,
  `"file-traversal"`, `"posix-utility"`, `"diagnostic-only"`). `null` when no profile is set.
- `spec_version` — `&'static str` — the vendored spec version this `anc` build was compiled against.

`0.4` additions (defined as serde-derived sub-structs in `src/scorecard/mod.rs`):

- `tool` — `ToolInfo { name: String, binary: Option<String>, version: Option<String> }`. Built in `main.rs`'s
  `build_tool_info`. Project mode prefers the manifest version (`Cargo.toml`/`pyproject.toml`); command/binary mode
  probes `<bin> --version` then `-V` via a fresh `BinaryRunner` with a 2-second timeout. Self-spawn guard compares the
  resolved binary path against `std::env::current_exe()` — recursion declined → `tool.version: null`.
- `anc` — `AncInfo { version: &'static str, commit: Option<&'static str> }`. Both fields are build-time constants
  emitted by `build.rs` into `$OUT_DIR/build_info.rs` (re-exported from `src/build_info.rs`). `commit` is `None` for
  builds outside a Git checkout. `build.rs` declares `cargo:rerun-if-changed` directives for `.git/HEAD`,
  `.git/refs/heads/<branch>`, and `.git/packed-refs` so cached builds don't embed a stale SHA across local commits.
- `run` — `RunInfo { invocation, started_at, duration_ms, platform: { os, arch } }`. `invocation` is captured **before**
  `inject_default_subcommand` rewrites argv (so `anc .` records as `"anc ."`, not `"anc check ."`). `started_at` is RFC
  3339 UTC via the `time` crate (pinned `=0.3.45`). `duration_ms` uses `Instant` for monotonic measurement.
  `platform.{os,arch}` come from `std::env::consts`.
- `target` — `TargetInfo { kind: String, path: Option<String>, command: Option<String> }`. `kind` is one of `"project"`,
  `"binary"`, `"command"`. The unused field is always `null`, never missing.

Always-present null contract: `tool.version`, `tool.binary`, `target.path`, `target.command` serialize as JSON `null`
when not applicable, never as missing keys. Consumers can access these paths unconditionally. The exception is
`audience_reason`, which uses `skip_serializing_if = "Option::is_none"` — its absence carries information (audience has
a label).

Consumers (notably the site's `/score/<tool>` page) must feature-detect the new fields — pre-`0.4` scorecards lack the
four metadata blocks. The site's `agentnative-site/registry.yaml` will eventually drop its parallel `version` /
`scored_at` fields once consumers read those facts from the scorecard's `tool.version` / `run.started_at`. That
follow-up lives in the `agentnative-site` repo, not here.

## Skill Install Verb

`anc skill install <host>` ships the `agentnative-skill` bundle into a host's canonical skills directory. The host map
is **hardcoded** in `src/skill_install.rs` (no `skill.json` parsing in production, no HTTPS fetch, no allowlist
validator). The freshness model is: re-vendor `tests/fixtures/skill.json`, update the Rust map to match, cut a patch
release. CI fails on drift between the fixture and the canonical site copy at every layer:

- `tests/fixtures/skill.json` is a verbatim copy of `agentnative-site/src/data/skill.json`. Test 12
  (`host_map_matches_site_skill_json`) loads the fixture and asserts each Rust-map `(url, dest_template)` reconstructs
  the fixture's `install.<host>` command verbatim — fails fast in `cargo test` with no network access.
- `scripts/sync-skill-fixture.sh --check` (CI workflow `skill-fixture-drift.yml`) clones the upstream site at
  `SKILL_SITE_REF` (default `dev`, since the site uses a dev/main forever-branch flow) and `cmp`s the live blob against
  the committed fixture. Runs on every PR and on push to main/dev.

The `git clone` invocation runs with named-const hardening that defeats ambient git-config and env subversion. The full
surface lives in `src/skill_install.rs`:

- `GIT_HARDEN_FLAGS: &[&str]` — five `-c key=value` pairs (`credential.helper=`, `core.askPass=`,
  `protocol.allow=https-only`, `http.followRedirects=false`, `url.<repo>.insteadOf=`). Applied via `Command::args`
  *before* the `clone` subcommand — git's required position for top-level `-c` options.
- `GIT_HARDEN_ENV_REMOVE: &[&str]` — seven env vars stripped via `env_remove` (`GIT_CONFIG_{GLOBAL,SYSTEM}`,
  `GIT_SSH{,_COMMAND}`, `GIT_PROXY_COMMAND`, `GIT_ASKPASS`, `GIT_EXEC_PATH`).
- `GIT_TERMINAL_PROMPT=0` is **set** (not removed) so git never prompts when credentials are missing — its
  default-when-unset is to prompt, which is the wrong default for a non-interactive subcommand.

**Rules for changes touching skill install:**

- NEVER call `Command::env_clear()` — it strips PATH and breaks git's helper resolution. Use `env_remove` per var.
- NEVER use `sh -c` or any shell-mediated invocation. Tokens go directly to `git` via `Command::args`.
- NEVER reintroduce `skill.json` parsing in production code. The fixture is a CI drift anchor, not a runtime resource.
- When adding a new host, update both `SkillHost` (with the `rename_all = "snake_case"` clap value) AND `KNOWN_HOSTS`
  AND `resolve_host`. Test 11 enforces lockstep on the first two; the registry-style guard is intentional.

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
