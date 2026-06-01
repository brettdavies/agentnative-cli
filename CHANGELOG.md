# Changelog

All notable changes to this project will be documented in this file.

## [0.5.0] - 2026-06-01

### Added

- Add `p2-raw-flag`, `p2-more-formats`, `p3-examples-subcommand`, `p6-color-flag`, `p7-verbose`, `p7-limit`, and `p7-cursor-pagination` behavioral checks. The two list-style checks (`p7-limit`, `p7-cursor-pagination`) vacuously skip when the target CLI has no list-style subcommand; the other five always run and produce Pass / Warn on flag presence in top-level `--help`. by @brettdavies in [#55](https://github.com/brettdavies/agentnative-cli/pull/55)
- `VersionCheck` now probes the short-alias family (`-V`, `-v`, `-version`) alongside `--version`. Pass when both work, Warn when only `--version` works (MUST satisfied, SHOULD missed), Fail when neither works.
- `AgentsMdCheck` now declares it covers `p8-should-bundle-exists`. The check already verified a subset of what P8 demands; this exposes the link to the coverage matrix.
- Add `p1-defaults-in-help` behavioral check. Scans `--help` for `[default: …]` / `(default: …)` / `default:` annotations. SHOULD-tier, universal. by @brettdavies in [#56](https://github.com/brettdavies/agentnative-cli/pull/56)
- Add `p1-rich-tui` behavioral check. Detects rich-TUI surface area via `--tui` / `--interactive` / `--ui` flags or spinner/progress/tui/ncurses/indicatif mentions in help text. MAY-tier, universal.
- Add `p3-about-long-about` behavioral check. Probes both `-h` and `--help` directly and Warns when the two outputs are byte-identical (no `long_about` defined). SHOULD-tier, universal.
- Add `p6-stdin-input` behavioral check. Gated on input-accepting subcommand verbs (process/parse/convert/transform/analyze/validate/format/lint/check); Warns when help text does not mention stdin or `-` as a path placeholder. SHOULD-tier, conditional.
- Add `p6-consistent-naming` behavioral check. Classifies subcommands against a common-verb list; Warns when verb-first and noun-first patterns mix. SHOULD-tier, conditional on 2+ user-defined subcommands.
- Add `p7-timeout-behavioral` behavioral check. Gated on long-running subcommand verbs (serve/daemon/watch/tail/monitor/follow/run/start/stream); Warns when no `--timeout` / `--deadline` / `--max-time` flag advertised. SHOULD-tier, conditional. Distinct from the source-layer `p6-must-timeout-network` which gates on network-library usage.
- 11 behavioral checks closing the remaining behavioral orphan coverage: structured exit codes, JSON error envelopes, consistent envelope shape, actionable error messages, JSON error output, subcommand examples, paired text+JSON examples, subcommand-shaped operations, force/yes on destructive subcommands, read/write surface distinction, and TTY-aware verbosity. by @brettdavies in [#57](https://github.com/brettdavies/agentnative-cli/pull/57)
- Top-level `--verbose` / `-v` flag (env `AGENTNATIVE_VERBOSE`) for diagnostic escalation, mutually exclusive with `--quiet`. by @brettdavies in [#58](https://github.com/brettdavies/agentnative-cli/pull/58)
- Top-level `--examples` flag prints a curated invocation block and exits. Pairs with `--output json` (or `--json`) so structured-output consumers can fetch the block without parsing full `--help`.
- Top-level `--color auto|always|never` (env `AGENTNATIVE_COLOR`) wraps text-mode status prefixes in ANSI styling, honoring NO_COLOR and TTY detection.
- Top-level `--raw` emits one `id<TAB>status` line per check with no headers, summary, or badge hint. Pipe-friendly for `grep`/`awk` workflows.
- Per-subcommand `Examples:` blocks in `--help` for `audit`, `emit`, `skill`.
- `anc skill install --all` and `anc skill update [host|--all]` iterate every known host in one invocation. Update guards against operating on non-bundle directories via a `SKILL.md` marker file.
- `audit` accepted as a community-standard verb in the p6-standard-names check (alongside `npm audit`, `cargo audit`, etc.).
- `scripts/sync-spec.sh` now accepts `--ref <git-ref>` (or `SPEC_REF` env var) to vendor `agentnative-spec` from an explicit branch, tag, or commit SHA. Default behavior unchanged: resolves the latest `v*` tag. by @brettdavies in [#59](https://github.com/brettdavies/agentnative-cli/pull/59)
- The resolved short SHA prints every run regardless of ref type, so consumer PRs can record the exact pin in their body.
- `scripts/hooks/pre-push` now runs `shellcheck --severity=warning` against every tracked `*.sh` plus everything under `scripts/hooks/` (so the hook itself is linted). Uses `git ls-files` so vendored / `.gitignore`d scripts are excluded. Skips with a one-line note if `shellcheck` is not installed, matching the existing `cargo deny` pattern. by @brettdavies in [#60](https://github.com/brettdavies/agentnative-cli/pull/60)
- New `opt_out` and `n_a` scorecard statuses surface in `anc audit --output json` (`status` field on each row and matching counters in `summary`). `opt_out` marks deliberate non-adoption (tool ships no `--output` flag, no `AGENTS.md` bundle); `n_a` marks a conditional requirement whose antecedent is unmet. Pre-0.6 consumers treat both as unknown and feature-detect. by @brettdavies in [#62](https://github.com/brettdavies/agentnative-cli/pull/62)
- Each entry in `results[]` now carries a `tier` field (`must` / `should` / `may`, or `null` for rows not in the registry) and a `check_id` field naming the probe that produced the row.
- `anc emit schema` returns the schema 0.6 contract (`$id: https://anc.dev/scorecard-v0.6.schema.json`) with the new status enum values, summary counters, and per-row fields.
- `RELEASES.md` gains a `### Dev-direct exception` subsection under Daily development, mirroring the spec's same insertion point and content. Names the two path categories (engineering docs and prose-check stack) that may be committed directly to `dev` without a feature branch. by @brettdavies in [#69](https://github.com/brettdavies/agentnative-cli/pull/69)
- `RELEASES-PREFLIGHT.md` gains a "Release mechanics sanity" item that runs the same guarded-path leak check `RELEASES.md` step 4 runs, so operators catch leaks before the release PR opens. Points at the new `RELEASES.md` § Cherry-pick conflicts on guarded paths subsection for resolution.

### Changed

- Vendored spec bump: `src/principles/spec/principles/p3-progressive-help-discovery.md` carries the two new `p3-must-version` and `p3-should-version-short` requirements (universal applicability). `REQUIREMENTS.len()` 57 -> 59; MUST count 27 -> 28; SHOULD count 20 -> 21; MAY count unchanged at 10. by @brettdavies in [#55](https://github.com/brettdavies/agentnative-cli/pull/55)
- Coverage matrix regenerated: `docs/coverage-matrix.md` and `coverage/matrix.json` reflect 59 requirement rows and the new verifier links.
- Coverage matrix regenerated: `docs/coverage-matrix.md` and `coverage/matrix.json` reflect 6 new verifier links across P1, P3, P6, and P7 requirement rows. by @brettdavies in [#56](https://github.com/brettdavies/agentnative-cli/pull/56)
- BREAKING: `anc check` renamed to `anc audit`; `anc generate` renamed to `anc emit`; `anc schema` removed and folded under `anc emit schema`. The implicit-default-subcommand injection now writes `audit`. Update scripts and CI invocations. by @brettdavies in [#58](https://github.com/brettdavies/agentnative-cli/pull/58)
- Top-level `--help` now renders an extended description (`long_about`) with paired text + JSON example invocations; `-h` still shows the concise summary.
- `STANDARD_VERBS` in p6-standard-names is now alphabetized within and across subgroups for review-friendliness; doc-comment notes the invariant.
- `src/project.rs` stderr warnings now reference `anc audit src/` instead of `anc check src/` when the file-walk hits depth or count limits.
- `scripts/sync-spec.sh` transport refactored from `git clone` to `gh api`. Reduces the shallow-vs-full clone distinction, makes ref resolution uniform across tag / branch / SHA, and removes the need for a local clone except as an offline fallback. by @brettdavies in [#59](https://github.com/brettdavies/agentnative-cli/pull/59)
- `scripts/SYNCS.md` documents the new flag, the `gh api` transport, the local-checkout fallback, and the convention of recording the resolved SHA in any consumer PR body.
- `anc audit --output json` emits one result entry per requirement-row instead of one per `check_id`. A probe like `p3-version` (covers `p3-must-version` and `p3-should-version-short`) now produces two distinct entries, each tier-stamped, so downstream scoring layers no longer need a coverage-matrix join to attribute a probe's outcome to a specific RFC 2119 level. by @brettdavies in [#62](https://github.com/brettdavies/agentnative-cli/pull/62)
- Conditional requirement rows whose antecedent collapses to `opt_out` or `n_a` are propagated to `n_a` in `results[]`; rows whose antecedent is `skip` or `error` inherit the indeterminacy. The propagated evidence string names the antecedent check id so the chain is legible from the JSON alone.
- The badge denominator excludes `opt_out` (transitional) and excludes `n_a` from both sides, matching the plan's posture that no formula is provably fair until the input shape is disambiguated.
- `anc audit` text mode renders `OPT` and `N/A` status badges alongside the existing five, and the summary line reports all seven counters.
- `p8-bundle-exists` emits `opt_out` when no top-level `AGENTS.md` or `SKILL.md` is found (a malformed bundle still emits `warn`); `p2-json-output` emits `opt_out` when no `--output` or `--format` flag is detected at top level or in any subcommand.
- The vendored `agentnative-spec` tree updates to `dev` commit `b4f4d02` (PR brettdavies/agentnative#34). Five rows in `p2` and `p8` migrate to the new `applicability.kind: conditional` / `antecedent.check_id` shape; the remaining 18 legacy `applicability.if: <prose>` rows stay as-is until each prerequisite grows a machine-readable check id.
- `anc audit` (text mode) now reports the same requirement rows as `--output json`: requirement-row ids (`p2-must-schema-print`) instead of probe ids, with the requirement tier (`must`/`should`/`may`) shown on each row. by @brettdavies in [#63](https://github.com/brettdavies/agentnative-cli/pull/63)
- The process exit code now reflects the per-row result set in both output modes. A requirement that collapses to `n_a` because its conditional prerequisite is `opt_out` no longer forces a non-zero exit; only a live `Fail`/`Error` exits `2` and a live `Warn` exits `1`.
- The leaderboard score now reflects shipped-binary behavior only: source- and project-layer checks no longer affect `score_pct` or badge eligibility. by @brettdavies in [#64](https://github.com/brettdavies/agentnative-cli/pull/64)
- `score_pct` is now credit-weighted: `warn` earns half credit and `opt_out` counts against the score, replacing the prior pass-only ratio.
- Lower the agent-native badge eligibility floor from 80% to 70%.
- Rename the `check_id` field to `audit_id` in `anc audit --output json` scorecards and in `coverage/matrix.json`. Scorecard `schema_version` goes 0.6 → 0.7 and its JSON Schema `$id` becomes `scorecard-v0.7`; the coverage matrix keeps `schema_version` 1.0 (unreleased). Consumers pinning `check_id` must read `audit_id`. by @brettdavies in [#65](https://github.com/brettdavies/agentnative-cli/pull/65)
- CLI help and output now describe the work as "audits" (for example, "Run only behavioral audits") to match the `anc audit` verb.
- Scorecard `spec_version` now reports `"0.5.0"` (vendored agentnative-spec bumped 0.4.0 → 0.5.0). by @brettdavies in [#66](https://github.com/brettdavies/agentnative-cli/pull/66)
- Tightened prose across all 11 tracked markdown docs in the repo plus historical sections of `CHANGELOG.md`. Each occurrence of a recurring punctuation pattern was replaced with the alternative best matching the local context (colon, semicolon, period, comma, or a small rewording). by @brettdavies in [#68](https://github.com/brettdavies/agentnative-cli/pull/68)
- `anc emit coverage-matrix` writes the trailer as `<!-- Generated by `anc emit coverage-matrix`; do not edit by hand -->`; the `--check` failure message in `src/main.rs` uses the same form.
- `styles/brand/README.md` regenerated against tightened `styles/brand/*.yml` rule sources and updated `scripts/generate-pack-readme.mjs` trailer so the drift check stays clean on future regenerations.
- `.github/workflows/guard-main-docs.yml`: pass `extra_paths: 'styles/,.vale.ini,scripts/prose-check.sh'` to the reusable guard workflow. Future PRs to `main` that add or modify those paths fail the check. Mirrors the path values used by `agentnative-spec`. by @brettdavies in [#69](https://github.com/brettdavies/agentnative-cli/pull/69)
- `BRAND.md`: refreshed verbatim from `agentnative-spec` to pick up the "narrative is authoritative for both the why and the what" reframing and the `(dev-only)` annotation on the Vale-rule-pack column of the Channel-artifacts table. As a vendored mirror, BRAND.md should match the spec word for word.
- `PRODUCT.md`: Inheritance and Register sections retune to "authoritative voice contract" framing; explicitly names the rule pack as dev-only tooling that does not ship to `main`. Removes the dead `styles/brand/README.md` / `styles/brand/*.yml` / `styles/config/vocabularies/cli/` links.
- `AGENTS.md`: rewrites the "Voice and prose rules" section to drop the `scripts/prose-check.sh` invocation example and the rule-pack vendoring prose; mirrors the spec's AGENTS.md voice-contract paragraph.
- `scripts/SYNCS.md`: drops the `sync-prose-tooling.sh` row from the upstream sync table, the matching mermaid arrow, and the Reference section entry.
- `RELEASES.md`: prose-scrubbing intro no longer promises a future vendoring; explicitly names the spec checkout as the Vale config source.
- `CLAUDE.md`: § "Scorecard v0.5 Fields" renamed to § "Scorecard JSON fields"; the cross-link in `RELEASES-PREFLIGHT.md` updates to match.
- `coverage/matrix.json` `schema_version` is now `"0.1"` (was `"1.0"`). The shape evolves additively until first public release locks it; consumers should feature-detect new fields rather than pin to an exact value. by @brettdavies in [#71](https://github.com/brettdavies/agentnative-cli/pull/71)
- `p6-should-consistent-naming` heuristic now recognizes hierarchical noun-verb subcommand patterns. Top-level verbs combined with noun-grouped verbs (the standard `git`, `gh`, `kubectl`, `docker`, `npm`, `cargo`, `anc` shape) Pass; only genuine inconsistency at the second level (a non-verb subcommand whose children include both verbs and non-verbs) Warns. The audit probes one level deeper per non-verb top-level subcommand via the existing cached `BinaryRunner` infrastructure. by @brettdavies in [#73](https://github.com/brettdavies/agentnative-cli/pull/73)
- `emit` added to the audit's `COMMON_VERBS` list so it is classified directly as a top-level verb action.
- Warn evidence now names every offending subcommand by name so the operator can target the fix.

### Fixed

- Error output under `--output json` / `--json` now emits a JSON envelope to stderr with `error`, `kind`, and `message` fields instead of clap's plain-text rendering, so agents pinned to JSON can parse failures with one shape. by @brettdavies in [#58](https://github.com/brettdavies/agentnative-cli/pull/58)
- `--help --output json` and `--version --output json` emit JSON envelopes so structured-output consumers can probe help and version without a separate text parser.
- `--examples` is no longer `exclusive = true`, so `anc --examples --output json` now produces the JSON envelope it always claimed to support (was failing with `argument-conflict`).
- `EXAMPLES_BLOCK` content (printed by `anc --examples`) updated to use current subcommand names (was still showing `anc check`, `anc generate`, `anc schema`).
- p2-schema-print check now walks one level into top-level subcommand help to discover `schema` exposed as a nested verb (e.g., `anc emit schema`), matching how an agent walks `--help` chains. Benefits every CLI checked by anc, not just anc itself.
- `scorecard::audience::tests::duplicate_signal_in_results_trips_debug_assert` is gated on `#[cfg(debug_assertions)]` so it runs only where `debug_assert!` actually fires (previously failed silently under `cargo test --release`).
- Embedded `schema/scorecard.schema.json` description fields updated to reference `anc audit` rather than `anc check`.
- Removed dead `spec_ref` variable in `scripts/sync-spec.sh` (SC2034). Declared during the `--ref` refactor in #59 but never read; the actual code uses the `SPEC_REF` env var directly. by @brettdavies in [#60](https://github.com/brettdavies/agentnative-cli/pull/60)
- The spec parser now rejects three malformed inputs that previously fell through silently: `antecedent.check_id` containing only whitespace, an applicability block carrying both legacy `if:` and new `kind:` (the legacy branch only fired for single-key maps, so the prose was being dropped on the floor), and any key inside `antecedent` other than `check_id` (compound antecedents are deferred to v2 of the schema per the plan's Sub-decision 2b). by @brettdavies in [#62](https://github.com/brettdavies/agentnative-cli/pull/62)
- Text mode now renders `[N/A ]` (with antecedent evidence) for conditional requirements whose prerequisite was opted out, instead of showing a misleading `[FAIL]` on the probe id. Text row count and badge score now match `--output json`. by @brettdavies in [#63](https://github.com/brettdavies/agentnative-cli/pull/63)
- `scripts/generate-changelog.sh` now refuses to prepend a duplicate `[X.Y.Z]` section when one already exists in `CHANGELOG.md`. Re-running the script with an already-published tag previously emitted a second copy of the same section and an empty `vX.Y.Z...vX.Y.Z` compare link. by @brettdavies in [#68](https://github.com/brettdavies/agentnative-cli/pull/68)

### Documentation

- Update `tests/build_parser.rs` and `src/principles/registry.rs` test counters to match the new 59-requirement registry. by @brettdavies in [#55](https://github.com/brettdavies/agentnative-cli/pull/55)
- `schema/scorecard.schema.json` regenerates against the 0.6 contract: new enum values, new required counters, `tier` and `check_id` on `CheckResultView`, and a refreshed `examples[0]` block. by @brettdavies in [#62](https://github.com/brettdavies/agentnative-cli/pull/62)
- `coverage/matrix.json` and `docs/coverage-matrix.md` regenerate against the new conditional applicability shape. Conditional rows surface with `applicability.antecedent.check_id` populated; legacy rows continue to emit `applicability.condition: "<prose>"`.
- `RELEASES.md` gains a "Cherry-pick conflicts on guarded paths" subsection documenting the `git update-index --remove` plus `gio trash` resolution pattern for modify/delete and rename/delete conflicts on `docs/plans/`, `docs/brainstorms/`, and `docs/ideation/` paths during release cherry-picks. Standard `git rm` is denied by repo policy; the plumbing form is the supported alternative. by @brettdavies in [#68](https://github.com/brettdavies/agentnative-cli/pull/68)
- `CLAUDE.md` § Coverage Matrix Artifact Lifecycle: updates the documented `schema_version` value and notes the pre-release-shape convention. by @brettdavies in [#71](https://github.com/brettdavies/agentnative-cli/pull/71)

**Full Changelog**: [v0.4.0...v0.5.0](https://github.com/brettdavies/agentnative-cli/compare/v0.4.0...v0.5.0)

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
