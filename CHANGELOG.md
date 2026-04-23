# Changelog

All notable changes to this project will be documented in this file.

## [0.1.3] - 2026-04-23

### Added

- `audience` field on scorecard JSON now emits a kebab-case label (`agent-optimized` / `mixed` / `human-primary`) when all four signal behavioral checks ran, or `null` when any are missing. by @brettdavies in [#26](https://github.com/brettdavies/agentnative-cli/pull/26)
- `--audit-profile <category>` flag on `anc check` accepts `human-tui`, `file-traversal`, `posix-utility`, or `diagnostic-only`. The applied value echoes as the top-level `audit_profile` field on scorecard JSON, and suppressed checks drop out of `coverage_summary.{must,should,may}.verified` so site leaderboards don't overstate per-tool coverage under audit profiles.
- `audience_reason` field on scorecard JSON — populated only when `audience` is `null`, with `"suppressed"` (signal check masked by `--audit-profile`) or `"insufficient_signal"` (signal check never produced) so consumers can see why the classifier withheld a label. by @brettdavies in [#27](https://github.com/brettdavies/agentnative-cli/pull/27)
- `audit_profiles` array in `coverage/matrix.json` — each entry carries `{name, description, suppresses[]}`, letting agents and site renderers enumerate the four `--audit-profile` categories and what each one suppresses without scraping `--help`.

### Changed

- `p1-env-hints` now recognizes bash-style env-var references (`$FOO` / `TOOL_FOO`) near flag definitions in addition to clap `[env: FOO]` annotations. Tools like `ripgrep` and `aider` that document env bindings in free prose now Pass instead of Warn. `$PAGER` and uppercase section headers like `DOCKER_CONFIG:` are excluded so tools like `git` / `gh` / `man` and pages with structured help output don't produce false positives. by @brettdavies in [#26](https://github.com/brettdavies/agentnative-cli/pull/26)
- Suppressed and errored `results[].label` values now show the check's human-readable label (e.g., "Respects NO_COLOR") instead of falling back to the check id. by @brettdavies in [#27](https://github.com/brettdavies/agentnative-cli/pull/27)

### Documentation

- README.md, AGENTS.md, and CLAUDE.md updated to describe the shipped v0.1.3 scorecard surface: `audience` + `audience_reason` + `audit_profile` field semantics, the `--audit-profile` flag with examples, and the `audit_profiles` section of `coverage/matrix.json` as the programmatic source for category enumeration. by @brettdavies in [#27](https://github.com/brettdavies/agentnative-cli/pull/27)

**Full Changelog**: [v0.1.2...v0.1.3](https://github.com/brettdavies/agentnative-cli/compare/v0.1.2...v0.1.3)

## [0.1.2] - 2026-04-21

### Added

- Add `p1-flag-existence` behavioral check — passes when `--help` advertises a non-interactive gate flag (`--no-interactive`, `--batch`, `--headless`, `-y`, `--yes`, `-p`, `--print`, `--no-input`, `--assume-yes`). Skips when the target already satisfies P1 via help-on-bare-invocation or stdin-primary. by @brettdavies in [#24](https://github.com/brettdavies/agentnative-cli/pull/24)
- Add `p1-env-hints` behavioral check — passes when `--help` exposes clap-style `[env: FOO]` bindings for flags. Emits medium confidence; the heuristic covers the canonical but not the only env-binding format.
- Add `p6-no-pager-behavioral` behavioral check — passes when `--no-pager` is advertised in `--help`. Skips when no pager signal (`less` / `more` / `$PAGER` / `--pager`) appears. Emits medium confidence.
- Add `confidence` field to every scorecard result (`high` / `medium` / `low`). Additive; v1.1 consumers feature-detect.
- Add `dual_layer` count to the coverage matrix summary so the headline prose surfaces how many covered requirements have verifiers in two layers.

### Changed

- Raise required approving review count on `main` branch from 0 to 1. by @brettdavies in [#24](https://github.com/brettdavies/agentnative-cli/pull/24)

### Documentation

- Document the \`covers()\` trait method and the coverage-matrix regeneration step in the \"Adding a New Check\" guide. by @brettdavies in [#23](https://github.com/brettdavies/agentnative-cli/pull/23)
- Refresh README sample output to match v0.1.1 dogfood behaviour.
- Regenerate `docs/coverage-matrix.md` + `coverage/matrix.json` to pick up the three new behavioral verifiers. by @brettdavies in [#24](https://github.com/brettdavies/agentnative-cli/pull/24)

**Full Changelog**: [v0.1.1...v0.1.2](https://github.com/brettdavies/agentnative-cli/compare/v0.1.1...v0.1.2)

## [0.1.1] - 2026-04-20

### Added

- `anc generate coverage-matrix` subcommand (`--out`, `--json-out`, `--check` drift guard). Emits `docs/coverage-matrix.md` + `coverage/matrix.json` from the spec registry + each check's declared `covers()`. by @brettdavies in [#21](https://github.com/brettdavies/agentnative-cli/pull/21)
- Scorecard JSON v1.1 fields: `schema_version: "1.1"`, `coverage_summary` (`must`/`should`/`may` × `total`/`verified`), `audience` (reserved, null until v0.1.3), `audit_profile` (reserved, null until v0.1.3).
- GitHub issue templates for structured reporting: false-positive, scoring-bug, feature-request, grade-a-cli, pressure-test, spec-question (+ chooser `config.yml`).

### Changed

- Renamed `p6-tty-detection` → `p1-tty-detection-source` (verifies the P1 SHOULD for TTY detection, not a P6 concern). by @brettdavies in [#21](https://github.com/brettdavies/agentnative-cli/pull/21)
- Renamed `p6-env-flags` → `p1-env-flags-source` (verifies the P1 MUST that every flag be settable via env var — lives in P1, not P6).
- Repo URL references swept to `brettdavies/agentnative-cli` (renamed from `brettdavies/agentnative`). `Cargo.toml` `homepage` now points at `https://anc.dev`.

### Fixed

- P1 applicability gate (`src/checks/behavioral/non_interactive.rs`) now passes when any of help-on-bare-invocation, agentic-flag-present, or stdin-as-primary-input is observed. Previously `anc` risked warning itself once `p1-flag-existence` lands in v0.1.2. by @brettdavies in [#21](https://github.com/brettdavies/agentnative-cli/pull/21)

**Full Changelog**: [v0.1.0...v0.1.1](https://github.com/brettdavies/agentnative-cli/compare/v0.1.0...v0.1.1)

## [0.1.0] - 2026-04-16

### Added

- Add Check trait, Project struct with automatic language detection, and BinaryRunner with timeout and caching by @brettdavies in [#1](https://github.com/brettdavies/agentnative/pull/1)
- Add 8 behavioral checks: help text, version flag, JSON output, bad-args handling, quiet mode, SIGPIPE, non-interactive mode, no-color
- Add 3 Rust source checks via ast-grep: unwrap usage, no-color support, global flags
- Add CLI with `check` and `completions` subcommands, text and JSON scorecard output
- Add 30-check agent-readiness scorecard across behavioral, source, and project layers by @brettdavies in [#2](https://github.com/brettdavies/agentnative/pull/2)
- Add 13 Rust source checks and 6 project checks
- Add complete README with principles table, examples, and CLI reference
- `--command <name>` flag on `check` resolves a binary from PATH and runs behavioral checks against it. Mutually exclusive with the positional path. by @brettdavies in [#12](https://github.com/brettdavies/agentnative/pull/12)
- `value_hint = ValueHint::CommandName` on `--command` so zsh, fish, and elvish completions suggest PATH commands instead of file paths. Bash is patched post-generation in `scripts/generate-completions.sh`. by @brettdavies in [#13](https://github.com/brettdavies/agentnative/pull/13)
- `after_help` text on `Cli` documenting the implicit default subcommand and the bare-invocation contract directly in `anc --help` output.
- Mutual exclusion: `--command` and `--source` now error at parse time instead of silently producing an empty result.
- Add `code-bare-except` Python source check — detects bare `except:` clauses without exception types by @brettdavies in [#15](https://github.com/brettdavies/agentnative/pull/15)
- Add `p4-sys-exit` Python source check — detects `sys.exit()` calls outside `if __name__ == "__main__":` guards and `__main__.py` files
- Add `p6-no-color` Python source check — detects NO_COLOR env var handling (Warn, not Fail — behavioral check is the primary gate)
- Add language-parameterized source helpers `has_pattern_in()`, `find_pattern_matches_in()`, and `has_string_literal_in()` supporting Python and Rust

### Changed

- Change `--quiet`/`-q` to a global flag so it appears in top-level `--help` for agent discoverability by @brettdavies in [#6](https://github.com/brettdavies/agentnative/pull/6)
- The installed binary is now `anc`. The crate is still `agentnative`. Homebrew users will get both `anc` and an `agentnative` symlink (formula lands in Plan 002). by @brettdavies in [#11](https://github.com/brettdavies/agentnative/pull/11)
- `check` is now the default subcommand: `anc .`, `anc -q .`, and `anc --command ripgrep` all work without typing `check` explicitly. Bare `anc` (no arguments) still prints help and exits 2. by @brettdavies in [#12](https://github.com/brettdavies/agentnative/pull/12)
- `anc -q` / `anc --quiet` (top-level flag without subcommand) now prints help and exits 2 instead of panicking via `unreachable!()` (pre-existing bug). by @brettdavies in [#13](https://github.com/brettdavies/agentnative/pull/13)
- `anc help` and `anc help check` now work — clap's auto-generated `help` subcommand was missing from our known-subcommand set and got misclassified as a path.
- `anc --command <NAME>` where NAME collides with a subcommand name (e.g. `anc --command check`) now resolves NAME as a binary on PATH instead of producing a confusing clap error.
- `anc --command rg` and `anc --output json --source` (no positional argument) now work — the pre-parser detects subcommand-scoped flags and injects `check` accordingly.
- `anc -- .` (POSIX double-dash separator) now runs check against `.` instead of producing undefined behavior.

### Fixed

- Fix recursive fork bomb when dogfooding `agentnative check .` against itself by @brettdavies in [#7](https://github.com/brettdavies/agentnative/pull/7)
- Fix false positive: `sys.exit()` in `__main__.py` (Python entry point) no longer flagged by @brettdavies in [#15](https://github.com/brettdavies/agentnative/pull/15)
- Fix `is_main_guard`: now handles inline comments, parenthesized guards, no-space operators, and reversed operand order (e.g. `if "__main__" == __name__:`)
- Fix `is_bare_except`: restrict parsing to first line of node text (prevents false negatives on error-recovery nodes)
- Fix `__main__.py` skip to check filename component, not path suffix (prevents false skips on files like `my__main__.py`)
- Fix TOCTOU gap in `parsed_files` lazy initialization (replaced `RefCell` with `OnceLock`)
- Remove dead `except*` branch from bare-except detection (PEP 654 makes bare `except*:` a syntax error)

### Documentation

- Add `RELEASES.md` documenting the dev/main/release/* workflow and the Rust release pipeline (crates.io, GitHub Releases, Homebrew dispatch). by @brettdavies in [#11](https://github.com/brettdavies/agentnative/pull/11)
- README install section now lists all five distribution channels (Homebrew, cargo install, cargo binstall, GitHub Releases, from source) and all five shell completions with canonical auto-loaded paths.
- README and AGENTS.md updated to lead with the new ergonomics and document the `[PATH]` / `--command` mutual exclusion. by @brettdavies in [#12](https://github.com/brettdavies/agentnative/pull/12)
- README and AGENTS.md exit-code tables clarify that exit 2 is overloaded (failures, errors, and usage errors all share it). Suggest parsing stderr (`Usage:` text) to distinguish. by @brettdavies in [#13](https://github.com/brettdavies/agentnative/pull/13)
