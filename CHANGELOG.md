# Changelog

All notable changes to this project will be documented in this file.

## [0.4.0] - 2026-05-21

### Added

- Add P1 secret-handling check (`p1-must-secret-non-leaky-path`): scans target CLIs' `--help` for secret-bearing flag
  families (`--token`, `--password`, `--api-key`, `--secret`, `--auth`, `--credential`) and verifies each has either a
  `--*-file` companion or stdin path advertised. Vacuous Pass when no secret-bearing flag is detected. by @brettdavies
  in [#50](https://github.com/brettdavies/agentnative-cli/pull/50)
- Add P2 schema trio (`p2-must-schema-print`, `p2-should-schema-file`, `p2-should-json-aliases`): runtime-discoverable
  output schema via `schema` subcommand or `--schema` flag, file-export of schemas (`schema/*.json`, `*.schema.json` at
  repo root), and `--json` / `--jsonl` short aliases for `--output`.
- Add P4 closed-set rejection check (`p4-should-enumerate-valid-set`, Rust + Python): detects clap `ValueEnum`,
  `PossibleValuesParser`, `value_parser!`, and Python `argparse.choices=` / `click.Choice()`.
- Add P6 lifecycle and naming checks (`p6-must-sigterm`, Rust + Python; `p6-may-standard-names`): SIGTERM-handler
  detection across `signal_hook`, `tokio::signal::unix`, `signal.signal`, and `loop.add_signal_handler`;
  community-standard-verb allow-list applied to top-level subcommands.
- Add P8 skill-bundle suite (`p8-should-bundle-exists`, `p8-must-bundle-install`, `p8-may-install-all`,
  `p8-may-bundle-update`): repo-root detection of `AGENTS.md` / `SKILL.md` with YAML frontmatter, plus help-surface
  probes for `skill install`, `skill install --all`, and `skill update` / `skill upgrade`. Brand-new principle in the
  registry.
- New `PRODUCT.md` at repo root codifies linter-channel voice: second-person imperative register, three-part error shape
  (what failed, why, what to do), no marketing voice in CLI surface. Inherits universal rules from vendored `BRAND.md`.
  by @brettdavies in [#52](https://github.com/brettdavies/agentnative-cli/pull/52)
- New `CONTRIBUTING.md` documents the three-tier intake (signal / proposal / code), routes principle-level discussion to
  the spec repo, and names the dev-setup gates.
- New `add-tool-to-registry` issue template for proposing CLI tools to the anc100 registry.
- Add `anc schema` top-level subcommand. Prints the embedded JSON Schema (draft 2020-12) describing the shape of `anc
  check --output json` scorecards. Closes the `p2-must-schema-print` FAIL surfaced by self-check. by @brettdavies in
  [#54](https://github.com/brettdavies/agentnative-cli/pull/54)
- Add `schema/scorecard.schema.json` committed at the repo root and embedded into the binary via `include_str!`.
  Hand-written coverage of the 0.5 shape (Scorecard plus ToolInfo, AncInfo, RunInfo, PlatformInfo, TargetInfo,
  BadgeInfo, LevelCounts, CoverageSummary, Summary, CheckResultView). Closes the `p2-should-schema-file` WARN.
- Add YAML frontmatter to `AGENTS.md` naming the tool, binary, description, homepage, and repository so agent runtimes
  can index the bundle. Closes the `p8-should-bundle-exists` WARN.

### Changed

- Bump CLI from 0.3.1 to 0.4.0 (MINOR; meaningful coverage growth across five principles, including a brand-new
  principle). by @brettdavies in [#50](https://github.com/brettdavies/agentnative-cli/pull/50)
- Binary discovery in `src/project.rs::discover_rust_binaries` now picks the newer of `target/release/<bin>` and
  `target/debug/<bin>` by mtime when both exist. Ties and metadata failures fall back to debug (matches cargo's dev-flow
  default). CI scenarios where only one profile is built are unchanged. by @brettdavies in
  [#51](https://github.com/brettdavies/agentnative-cli/pull/51)
- `RELEASES.md` slims to operational runbook (95 lines); rationale moves to companion `RELEASES-RATIONALE.md` (243
  lines). Each runbook section ends with a section-pointer at the rationale. by @brettdavies in
  [#52](https://github.com/brettdavies/agentnative-cli/pull/52)
- Issue-template config adds `agentnative-skill` as a fourth cross-repo destination; renames "CLI grading" to "grading
  findings" to match spec-repo terminology.
- `derive_tool_name` now follows the four-tier fallback chain `command_name -> binary basename -> manifest package name
  -> directory basename`. The old shape returned the project directory basename, producing 404-bound badge URLs for any
  tool whose registry slug differed from its directory name. `anc check .` now emits `badge_url:
  https://anc.dev/badge/anc.svg` (HTTP 200, matches the site's `registry.yaml`). by @brettdavies in
  [#54](https://github.com/brettdavies/agentnative-cli/pull/54)
- `matches_principle` gains the `(CheckGroup::P8, 8)` arm. Pre-fix, `--principle 8` silently filtered out every P8 check
  because the match table predated the new principle.
- README refreshed for v0.4.0: principle count 7 -> 8 with a P8 row, "Example Output" rewritten against the current
  44-check self-check (P2 schema, P6 standard-names + SIGTERM, P8 bundle results), "Three Check Layers" lists Python
  alongside Rust under Source, JSON sample dropped the stale `anc.commit` field. README em-dash density scrubbed from
  20.5/1000 to 0/1000.
- README "Reporting issues" section folded into "Contributing" as a three-tier intake (signal / proposal / code) that
  points at the shipped `CONTRIBUTING.md` for the full breakdown. Cross-repo routing preserved.

### Documentation

- Document prose-scrubbing runbook in `RELEASES.md` for release-flow artifacts (PR bodies, `CHANGELOG.md`, release-PR
  bodies) using Vale + LanguageTool + unslop. by @brettdavies in
  [#50](https://github.com/brettdavies/agentnative-cli/pull/50)
- Add `## PR body` section to `RELEASES.md` codifying what belongs in PR bodies (NEW user-facing substance, six required
  template sections) and what does not (workflow recap, triple-diff output, pre-push gate results, CI status, AI
  attribution).
- Add Dogfooding Safety rule 3 to `CLAUDE.md` describing the mtime-based selection, with a `NEVER` directive against
  reverting to the always-prefer-release shape. by @brettdavies in
  [#51](https://github.com/brettdavies/agentnative-cli/pull/51)
- `AGENTS.md` gains a "Voice and prose rules" section pointing at `PRODUCT.md` for the linter-channel register and
  `scripts/prose-check.sh` for the local gate. by @brettdavies in
  [#52](https://github.com/brettdavies/agentnative-cli/pull/52)
- `scripts/SYNCS.md` documents the new `sync-prose-tooling.sh` row and the consumer-owned status of
  `scripts/prose-check.sh`.
- Update `--principle <PRINCIPLE>` doc from `(1-7)` to `(1-8)` in both README and `src/cli.rs`. by @brettdavies in
  [#54](https://github.com/brettdavies/agentnative-cli/pull/54)

**Full Changelog**: [v0.3.1...v0.4.0](https://github.com/brettdavies/agentnative-cli/compare/v0.3.1...v0.4.0)

## [0.3.1] - 2026-05-04

### Added

- Ship `x86_64-` and `aarch64-unknown-linux-musl` static binaries on every release. Statically linked against musl libc,
  so they run on Alpine and other musl-libc-host distros without glibc, and on every glibc distro too. by @brettdavies
  in [#48](https://github.com/brettdavies/agentnative-cli/pull/48)

### Documentation

- Document the `cliff.toml` chore-skip footgun and the "CHANGELOG is generated, never hand-written" rule in
  `RELEASES.md` under `Releasing dev to main`. Adds a new review step (renumbered to 9) and tightens the existing "PRs
  and changelog generation" section. by @brettdavies in [#48](https://github.com/brettdavies/agentnative-cli/pull/48)

**Full Changelog**: [v0.3.0...v0.3.1](https://github.com/brettdavies/agentnative-cli/compare/v0.3.0...v0.3.1)

## [0.3.0] - 2026-05-01

### Added

- Add four scorecard metadata blocks (`tool`, `anc`, `run`, `target`) to `--output json`: identifies the scored
  tool/version, the `anc` build that produced the scorecard, the user-typed invocation with timestamp and duration, and
  the resolved target (project / binary / command). by @brettdavies in
  [#34](https://github.com/brettdavies/agentnative-cli/pull/34)
- Add `time = "=0.3.47"` dependency for RFC 3339 timestamps in `run.started_at`.
- Add `anc skill install <host>` subcommand to install the
  [`agentnative-skill`](https://github.com/brettdavies/agentnative-skill) bundle into a host's canonical skills
  directory. Six hosts: `claude_code`, `codex`, `cursor`, `factory`, `kiro`, `opencode`. by @brettdavies in
  [#35](https://github.com/brettdavies/agentnative-cli/pull/35)
- Add `--dry-run` flag (P5): prints the resolved `git clone` command without spawning. Captures cleanly via `eval $(anc
  skill install --dry-run <host>)`.
- Add `--output {text,json}` flag (P2): JSON envelope is uniform across success and error and across dry-run / live
  install. Typed `reason` on error (`destination-not-empty`, `destination-is-file`, `home-not-set`, `git-not-found`,
  `git-clone-failed`).
- `--output text` now appends an agent-native badge embed hint after the summary line when the tool clears the 80%
  eligibility floor. Below the floor, nothing badge-related is printed (the convention's "do not nag" rule). by
  @brettdavies in [#36](https://github.com/brettdavies/agentnative-cli/pull/36)
- `--output json` scorecard now includes a `badge` block (`eligible`, `score_pct`, `embed_markdown`, `scorecard_url`,
  `badge_url`, `convention_url`). `embed_markdown` is `null` below the floor; `scorecard_url` / `badge_url` are
  populated whenever a tool slug exists, since the site renders an SVG for every scored tool.
- `scripts/sync-dev-after-release.sh`: backports `Cargo.toml` `[package].version`, `Cargo.lock`, and `CHANGELOG.md` from
  `main` to `dev` after a release tag publishes. Surgical (preserves dev's other Cargo.toml lines), idempotent (re-runs
  are a no-op when dev is already in sync), and signed via the operator's normal commit signing, which satisfies
  `protect-dev`'s `required_signatures` ruleset without needing a CI bot identity. by @brettdavies in
  [#37](https://github.com/brettdavies/agentnative-cli/pull/37)

### Changed

- Vendoring now always tracks the latest published spec tag; `SPEC_REF` env override removed. Run `bash
  scripts/sync-spec.sh` to refresh; no environment configuration required. by @brettdavies in
  [#33](https://github.com/brettdavies/agentnative-cli/pull/33)
- Bump scorecard `schema_version` from `"0.3"` to `"0.4"` (additive within the documented `0.x` policy; older consumers
  feature-detect). by @brettdavies in [#34](https://github.com/brettdavies/agentnative-cli/pull/34)
- Bump `rust-version` from `1.87` to `1.88` (let-chain stabilization).
- Bumped scorecard `schema_version` from `"0.4"` to `"0.5"`. Pre-`0.5` consumers feature-detect the new `badge` key and
  continue to work. by @brettdavies in [#36](https://github.com/brettdavies/agentnative-cli/pull/36)
- `p7-naked-println` source check now exempts `build.rs` at any crate root. Cargo build scripts use
  `println!("cargo:…")` directives by protocol; flagging them produces noise without an alternative API. Misnamed
  `src/build.rs` or `tests/build.rs` files stay flagged. by @brettdavies in
  [#38](https://github.com/brettdavies/agentnative-cli/pull/38)
- README refreshed for current state: schema 0.5 with `badge` block, `--audit-profile <CATEGORY>` documented under CLI
  Reference, `target.path` documented as basename-only (PII-safe), refreshed Example Output to match the live 33-check
  dogfood and the post-summary badge embed hint. by @brettdavies in
  [#40](https://github.com/brettdavies/agentnative-cli/pull/40)
- `--output json` scorecard `anc` block no longer includes a `commit` field. `anc.version` (the crate version pin)
  remains as the build identity. Removed because the per-build Git SHA capture made cached builds fragile (stale SHAs
  across local commits) without solving any consumer-facing problem; `anc.version` already identifies the released
  binary unambiguously, and pre-launch no public consumer of `anc.commit` exists. by @brettdavies in
  [#47](https://github.com/brettdavies/agentnative-cli/pull/47)

### Fixed

- Eliminated four `.unwrap()` calls on infallible operations across `src/skill_install.rs` and `build.rs`. Replaced with
  `.expect("…")` naming the upstream contract that guarantees `Some`/`Ok`. No behavior change: these were already
  infallible; the `expect` messages document why. by @brettdavies in
  [#38](https://github.com/brettdavies/agentnative-cli/pull/38)
- `target.path` in `anc check --output json` now emits the basename of the resolved target instead of the canonicalized
  absolute path, eliminating a home-directory / username PII leak that flowed into committed scorecards, badge URLs, and
  agent-posted artifacts. Project mode emits the directory name (e.g. `"agentnative-cli"`); binary mode emits the file
  name (e.g. `"anc"`); command mode unchanged at `null`. No schema bump: value semantics changed, schema shape did not.
  by @brettdavies in [#39](https://github.com/brettdavies/agentnative-cli/pull/39)
- Corrected cross-repo URLs in `.github/ISSUE_TEMPLATE/` so contact links and agent-filing instructions point at the
  right repos. Spec repo references switched from `agentnative-cli` to `agentnative`; site repo references switched from
  `agentnative-cli-site` to `agentnative-site`; the `agentnative-cli-cli` double-suffix typo in agent gh-search guidance
  was corrected to `agentnative-cli`. Affects `config.yml`, `false-positive.yml`, `feature-request.yml`,
  `scoring-bug.yml`. by @brettdavies in [#42](https://github.com/brettdavies/agentnative-cli/pull/42)

### Documentation

- `AGENTS.md` and `src/principles/spec/README.md` updated to reflect the simpler vendor flow. by @brettdavies in
  [#33](https://github.com/brettdavies/agentnative-cli/pull/33)
- Document the four new top-level objects in README.md, AGENTS.md, and CLAUDE.md, including the publishing-PII review
  reminder for `run.invocation` and `target.path`. by @brettdavies in
  [#34](https://github.com/brettdavies/agentnative-cli/pull/34)
- Add `## Install the skill` section to README with one-line examples per host and the manual `git clone` fallback for
  hosts not yet in the binary's map. by @brettdavies in [#35](https://github.com/brettdavies/agentnative-cli/pull/35)
- `RELEASES.md` § "After publish: sync ``dev`` with the release" documents the backport step, supersedes the prior
  "never back-merged" rule for these three specific files, and points operators at the script. by @brettdavies in
  [#37](https://github.com/brettdavies/agentnative-cli/pull/37)
- Add the `[![agent-native](https://anc.dev/badge/anc.svg)](https://anc.dev/score/anc)` badge plus crates.io and license
  shields at the top of `README.md`. by @brettdavies in [#40](https://github.com/brettdavies/agentnative-cli/pull/40)
- Trim `.github/ISSUE_TEMPLATE/` to `false-positive`, `feature-request`, `scoring-bug`, plus a new `00-blank.yml` that
  lets a Blank issue option sit first in the chooser ahead of the structured forms. Spec-side templates
  (`pressure-test`, `grade-a-cli`, `spec-question`) were duplicates of the spec repo's set from before the rename; they
  belong on `brettdavies/agentnative` only, and `config.yml` already redirects there.
- Add `scripts/SYNCS.md`: cross-repo sync map covering every spec/skill/coverage/release data flow with mechanism,
  payload, trigger, and drift check per edge. Includes a flowchart of inbound/outbound edges, a release-pipeline
  sequence diagram, and a cadence summary table reducing the system to "automatic vs manual" per sync point. by
  @brettdavies in [#41](https://github.com/brettdavies/agentnative-cli/pull/41)
- `RELEASES.md` § "Releasing dev to main" step 4 replaced with a triple-diff verification block (A: main→release, B:
  release→dev, C: dev→main) plus a `git cherry HEAD origin/dev` patch-id check. The new flow catches both directions of
  drift before the release tag goes out: guarded paths leaking IN (the original concern) and missed cherry-picks that
  should have shipped (the new concern). Discovered during v0.3.0 prep when an ad-hoc triple-diff caught 4
  `.github/ISSUE_TEMPLATE/*.yml` files that had drifted on `main` since the v0.1.1 squash. by @brettdavies in
  [#45](https://github.com/brettdavies/agentnative-cli/pull/45)
- `RELEASES.md` § "Releasing dev to main" step 4: expanded the `git cherry` patch-id check comment with squash-merge
  triage guidance (three expected noise sources, what a real miss looks like, and a two-command triage recipe).
  Discovered during v0.3.0 prep when the check produced 55 noisy `+` lines that all turned out to be expected; the
  original comment didn't explain that this is normal in a squash-merge workflow. by @brettdavies in
  [#46](https://github.com/brettdavies/agentnative-cli/pull/46)

**Full Changelog**: [v0.2.0...v0.3.0](https://github.com/brettdavies/agentnative-cli/compare/v0.2.0...v0.3.0)

## [0.2.0] - 2026-04-29

### Added

- Vendored `agentnative-spec` snapshot under `src/principles/spec/` with `scripts/sync-spec.sh` for pinned-tag resync
  (extracts via `git show <ref>` so the spec checkout's working tree is not perturbed). by @brettdavies in
  [#29](https://github.com/brettdavies/agentnative-cli/pull/29)
- `spec_version` field in `anc check --output json` scorecard, sourced at build time from vendored
  `src/principles/spec/VERSION`. Pin against this to know which spec contract a scorecard's requirement IDs reference.

### Changed

- `REQUIREMENTS` is now generated at build time from vendored frontmatter; no hand-maintained duplicate. No scoring
  behavior change: pre/post diff verified byte-identical across all 33 check results, summaries, and coverage totals. by
  @brettdavies in [#29](https://github.com/brettdavies/agentnative-cli/pull/29)
- Scorecard `schema_version` reset `1.2` → `0.3`. Pre-launch correction; the schema is at `0.x` while `anc` is
  pre-launch and will lock at `1.0` on first public release. No public consumers exist today.
- All 7 principles flipped from `status: draft` to `status: active` via re-vendor against `agentnative-spec` `v0.3.0`.
  Scorecard `spec_version` now reports `"0.3.0"`. Three SHOULD/MUST requirement summaries reworded for clarity
  (`p4-should-gating-before-network`, `p6-must-sigpipe`, `p6-must-global-flags`); no requirement-ID changes, no count
  changes.

### Documentation

- AGENTS.md "Spec source" section rewritten to describe build-time vendoring and resync cadence (`SPEC_REF` env var
  bumps the vendored tag). by @brettdavies in [#29](https://github.com/brettdavies/agentnative-cli/pull/29)

**Full Changelog**: [v0.1.3...v0.2.0](https://github.com/brettdavies/agentnative-cli/compare/v0.1.3...v0.2.0)

## [0.1.3] - 2026-04-23

### Added

- `audience` field on scorecard JSON now emits a kebab-case label (`agent-optimized` / `mixed` / `human-primary`) when
  all four signal behavioral checks ran, or `null` when any are missing. by @brettdavies in
  [#26](https://github.com/brettdavies/agentnative-cli/pull/26)
- `--audit-profile <category>` flag on `anc check` accepts `human-tui`, `file-traversal`, `posix-utility`, or
  `diagnostic-only`. The applied value echoes as the top-level `audit_profile` field on scorecard JSON, and suppressed
  checks drop out of `coverage_summary.{must,should,may}.verified` so site leaderboards don't overstate per-tool
  coverage under audit profiles.
- `audience_reason` field on scorecard JSON: populated only when `audience` is `null`, with `"suppressed"` (signal check
  masked by `--audit-profile`) or `"insufficient_signal"` (signal check never produced) so consumers can see why the
  classifier withheld a label. by @brettdavies in [#27](https://github.com/brettdavies/agentnative-cli/pull/27)
- `audit_profiles` array in `coverage/matrix.json`: each entry carries `{name, description, suppresses[]}`, letting
  agents and site renderers enumerate the four `--audit-profile` categories and what each one suppresses without
  scraping `--help`.

### Changed

- `p1-env-hints` now recognizes bash-style env-var references (`$FOO` / `TOOL_FOO`) near flag definitions in addition to
  clap `[env: FOO]` annotations. Tools like `ripgrep` and `aider` that document env bindings in free prose now Pass
  instead of Warn. `$PAGER` and uppercase section headers like `DOCKER_CONFIG:` are excluded so tools like `git` / `gh`
  / `man` and pages with structured help output don't produce false positives. by @brettdavies in
  [#26](https://github.com/brettdavies/agentnative-cli/pull/26)
- Suppressed and errored `results[].label` values now show the check's human-readable label (e.g., "Respects NO_COLOR")
  instead of falling back to the check id. by @brettdavies in
  [#27](https://github.com/brettdavies/agentnative-cli/pull/27)

### Documentation

- README.md, AGENTS.md, and CLAUDE.md updated to describe the shipped v0.1.3 scorecard surface: `audience` +
  `audience_reason` + `audit_profile` field semantics, the `--audit-profile` flag with examples, and the
  `audit_profiles` section of `coverage/matrix.json` as the programmatic source for category enumeration. by
  @brettdavies in [#27](https://github.com/brettdavies/agentnative-cli/pull/27)

**Full Changelog**: [v0.1.2...v0.1.3](https://github.com/brettdavies/agentnative-cli/compare/v0.1.2...v0.1.3)

## [0.1.2] - 2026-04-21

### Added

- Add `p1-flag-existence` behavioral check: passes when `--help` advertises a non-interactive gate flag
  (`--no-interactive`, `--batch`, `--headless`, `-y`, `--yes`, `-p`, `--print`, `--no-input`, `--assume-yes`). Skips
  when the target already satisfies P1 via help-on-bare-invocation or stdin-primary. by @brettdavies in
  [#24](https://github.com/brettdavies/agentnative-cli/pull/24)
- Add `p1-env-hints` behavioral check: passes when `--help` exposes clap-style `[env: FOO]` bindings for flags. Emits
  medium confidence; the heuristic covers the canonical but not the only env-binding format.
- Add `p6-no-pager-behavioral` behavioral check: passes when `--no-pager` is advertised in `--help`. Skips when no pager
  signal (`less` / `more` / `$PAGER` / `--pager`) appears. Emits medium confidence.
- Add `confidence` field to every scorecard result (`high` / `medium` / `low`). Additive; v1.1 consumers feature-detect.
- Add `dual_layer` count to the coverage matrix summary so the headline prose surfaces how many covered requirements
  have verifiers in two layers.

### Changed

- Raise required approving review count on `main` branch from 0 to 1. by @brettdavies in
  [#24](https://github.com/brettdavies/agentnative-cli/pull/24)

### Documentation

- Document the \`covers()\` trait method and the coverage-matrix regeneration step in the \"Adding a New Check\" guide.
  by @brettdavies in [#23](https://github.com/brettdavies/agentnative-cli/pull/23)
- Refresh README sample output to match v0.1.1 dogfood behaviour.
- Regenerate `docs/coverage-matrix.md` + `coverage/matrix.json` to pick up the three new behavioral verifiers. by
  @brettdavies in [#24](https://github.com/brettdavies/agentnative-cli/pull/24)

**Full Changelog**: [v0.1.1...v0.1.2](https://github.com/brettdavies/agentnative-cli/compare/v0.1.1...v0.1.2)

## [0.1.1] - 2026-04-20

### Added

- `anc generate coverage-matrix` subcommand (`--out`, `--json-out`, `--check` drift guard). Emits
  `docs/coverage-matrix.md` + `coverage/matrix.json` from the spec registry + each check's declared `covers()`. by
  @brettdavies in [#21](https://github.com/brettdavies/agentnative-cli/pull/21)
- Scorecard JSON v1.1 fields: `schema_version: "1.1"`, `coverage_summary` (`must`/`should`/`may` × `total`/`verified`),
  `audience` (reserved, null until v0.1.3), `audit_profile` (reserved, null until v0.1.3).
- GitHub issue templates for structured reporting: false-positive, scoring-bug, feature-request, grade-a-cli,
  pressure-test, spec-question (+ chooser `config.yml`).

### Changed

- Renamed `p6-tty-detection` → `p1-tty-detection-source` (verifies the P1 SHOULD for TTY detection, not a P6 concern).
  by @brettdavies in [#21](https://github.com/brettdavies/agentnative-cli/pull/21)
- Renamed `p6-env-flags` → `p1-env-flags-source` (verifies the P1 MUST that every flag be settable via env var; lives in
  P1, not P6).
- Repo URL references swept to `brettdavies/agentnative-cli` (renamed from `brettdavies/agentnative`). `Cargo.toml`
  `homepage` now points at `https://anc.dev`.

### Fixed

- P1 applicability gate (`src/checks/behavioral/non_interactive.rs`) now passes when any of help-on-bare-invocation,
  agentic-flag-present, or stdin-as-primary-input is observed. Previously `anc` risked warning itself once
  `p1-flag-existence` lands in v0.1.2. by @brettdavies in [#21](https://github.com/brettdavies/agentnative-cli/pull/21)

**Full Changelog**: [v0.1.0...v0.1.1](https://github.com/brettdavies/agentnative-cli/compare/v0.1.0...v0.1.1)

## [0.1.0] - 2026-04-16

### Added

- Add Check trait, Project struct with automatic language detection, and BinaryRunner with timeout and caching by
  @brettdavies in [#1](https://github.com/brettdavies/agentnative/pull/1)
- Add 8 behavioral checks: help text, version flag, JSON output, bad-args handling, quiet mode, SIGPIPE, non-interactive
  mode, no-color
- Add 3 Rust source checks via ast-grep: unwrap usage, no-color support, global flags
- Add CLI with `check` and `completions` subcommands, text and JSON scorecard output
- Add 30-check agent-readiness scorecard across behavioral, source, and project layers by @brettdavies in
  [#2](https://github.com/brettdavies/agentnative/pull/2)
- Add 13 Rust source checks and 6 project checks
- Add complete README with principles table, examples, and CLI reference
- `--command <name>` flag on `check` resolves a binary from PATH and runs behavioral checks against it. Mutually
  exclusive with the positional path. by @brettdavies in [#12](https://github.com/brettdavies/agentnative/pull/12)
- `value_hint = ValueHint::CommandName` on `--command` so zsh, fish, and elvish completions suggest PATH commands
  instead of file paths. Bash is patched post-generation in `scripts/generate-completions.sh`. by @brettdavies in
  [#13](https://github.com/brettdavies/agentnative/pull/13)
- `after_help` text on `Cli` documenting the implicit default subcommand and the bare-invocation contract directly in
  `anc --help` output.
- Mutual exclusion: `--command` and `--source` now error at parse time instead of silently producing an empty result.
- Add `code-bare-except` Python source check: detects bare `except:` clauses without exception types by @brettdavies in
  [#15](https://github.com/brettdavies/agentnative/pull/15)
- Add `p4-sys-exit` Python source check: detects `sys.exit()` calls outside `if __name__ == "__main__":` guards and
  `__main__.py` files
- Add `p6-no-color` Python source check: detects NO_COLOR env var handling (Warn, not Fail; the behavioral check is the
  primary gate)
- Add language-parameterized source helpers `has_pattern_in()`, `find_pattern_matches_in()`, and
  `has_string_literal_in()` supporting Python and Rust

### Changed

- Change `--quiet`/`-q` to a global flag so it appears in top-level `--help` for agent discoverability by @brettdavies
  in [#6](https://github.com/brettdavies/agentnative/pull/6)
- The installed binary is now `anc`. The crate is still `agentnative`. Homebrew users will get both `anc` and an
  `agentnative` symlink (formula lands in Plan 002). by @brettdavies in
  [#11](https://github.com/brettdavies/agentnative/pull/11)
- `check` is now the default subcommand: `anc .`, `anc -q .`, and `anc --command ripgrep` all work without typing
  `check` explicitly. Bare `anc` (no arguments) still prints help and exits 2. by @brettdavies in
  [#12](https://github.com/brettdavies/agentnative/pull/12)
- `anc -q` / `anc --quiet` (top-level flag without subcommand) now prints help and exits 2 instead of panicking via
  `unreachable!()` (pre-existing bug). by @brettdavies in [#13](https://github.com/brettdavies/agentnative/pull/13)
- `anc help` and `anc help check` now work. Clap's auto-generated `help` subcommand was missing from our
  known-subcommand set and got misclassified as a path.
- `anc --command <NAME>` where NAME collides with a subcommand name (e.g. `anc --command check`) now resolves NAME as a
  binary on PATH instead of producing a confusing clap error.
- `anc --command rg` and `anc --output json --source` (no positional argument) now work. The pre-parser detects
  subcommand-scoped flags and injects `check` accordingly.
- `anc -- .` (POSIX double-dash separator) now runs check against `.` instead of producing undefined behavior.

### Fixed

- Fix recursive fork bomb when dogfooding `agentnative check .` against itself by @brettdavies in
  [#7](https://github.com/brettdavies/agentnative/pull/7)
- Fix false positive: `sys.exit()` in `__main__.py` (Python entry point) no longer flagged by @brettdavies in
  [#15](https://github.com/brettdavies/agentnative/pull/15)
- Fix `is_main_guard`: now handles inline comments, parenthesized guards, no-space operators, and reversed operand order
  (e.g. `if "__main__" == __name__:`)
- Fix `is_bare_except`: restrict parsing to first line of node text (prevents false negatives on error-recovery nodes)
- Fix `__main__.py` skip to check filename component, not path suffix (prevents false skips on files like
  `my__main__.py`)
- Fix TOCTOU gap in `parsed_files` lazy initialization (replaced `RefCell` with `OnceLock`)
- Remove dead `except*` branch from bare-except detection (PEP 654 makes bare `except*:` a syntax error)

### Documentation

- Add `RELEASES.md` documenting the dev/main/release/* workflow and the Rust release pipeline (crates.io, GitHub
  Releases, Homebrew dispatch). by @brettdavies in [#11](https://github.com/brettdavies/agentnative/pull/11)
- README install section now lists all five distribution channels (Homebrew, cargo install, cargo binstall, GitHub
  Releases, from source) and all five shell completions with canonical auto-loaded paths.
- README and AGENTS.md updated to lead with the new ergonomics and document the `[PATH]` / `--command` mutual exclusion.
  by @brettdavies in [#12](https://github.com/brettdavies/agentnative/pull/12)
- README and AGENTS.md exit-code tables clarify that exit 2 is overloaded (failures, errors, and usage errors all share
  it). Suggest parsing stderr (`Usage:` text) to distinguish. by @brettdavies in
  [#13](https://github.com/brettdavies/agentnative/pull/13)
