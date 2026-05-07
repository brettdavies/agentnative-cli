# Changelog

All notable changes to this repository are documented here: governance, validator, release infrastructure, README, decision records.

Changes to the standard itself (principle MUST/SHOULD/MAY tier moves, requirement IDs added/removed/renamed, applicability shifts) are tracked per-principle in `principles/p*-*.md` via the `last-revised:` calver frontmatter field and the `## Pressure test notes` section appended to each file.

## [0.4.0] - 2026-05-07

### Added

- P1 MUST `p1-must-secret-non-leaky-path` (conditional on CLI accepting secret material): sensitive inputs are readable via stdin or a `--*-file` flag; flag-value and env-var inputs MAY exist for convenience but MUST NOT be the only path. by @brettdavies in [#25](https://github.com/brettdavies/agentnative/pull/25)
- P2 MUST `p2-must-schema-print` (conditional on structured output): expose the output schema via a `schema` subcommand or `--schema` flag, runtime-discoverable, with a documented format identifier (canonical recommendation: JSON Schema 2020-12).
- P2 SHOULD `p2-should-schema-file` (conditional on structured output): also export the schema to a stable file path so CI and static-analysis consumers can pin without invoking the tool.
- P2 SHOULD `p2-should-json-aliases`: accept `--json` and `--jsonl` as aliases for `--output json` and `--output jsonl`.
- P4 SHOULD `p4-should-enumerate-valid-set` (conditional on closed-set rejection): when rejecting input against an enum or fixed-allowed-values set, the error message includes the valid set.
- P6 MUST `p6-must-sigterm` (conditional on long-running operations): flush or roll back partial writes, release locks, exit non-zero within a bounded shutdown window. Next invocation succeeds without manual cleanup.
- P6 MAY `p6-may-standard-names` (conditional on subcommands): follow community-standard verbs (`get` / `list` / `create` / `update` / `delete`) and flag spellings (`--force`, `--yes`, `--limit`, `--quiet`, `--verbose`).
- New principle **P8 Discoverable Through Agent Skill Bundles** (four requirements: `p8-must-bundle-install`, `p8-should-bundle-exists`, `p8-may-install-all`, `p8-may-bundle-update`). CLIs ship a top-level skill bundle (`AGENTS.md`, `SKILL.md`, or equivalent) and provide an install path that registers the bundle with installed agent runtimes (canonical form: `tool skill install [<host>]`).

### Changed

- `VERSION`: 0.3.1 → 0.4.0 (MINOR per `principles/AGENTS.md`'s versioning rules; new MUSTs added). by @brettdavies in [#25](https://github.com/brettdavies/agentnative/pull/25)
- `.impeccable.md`: new spec-channel anti-pattern "No false canonicalization". When a bullet names an outcome the implementer can satisfy any way, prose uses indefinite articles and avoids language that canonicalizes one shape; when a bullet names a citable single-shape pattern, prose uses definite articles and cites the source.

**Full Changelog**: [v0.4.0...v0.4.0](https://github.com/brettdavies/agentnative/compare/v0.4.0...v0.4.0)

## [0.4.0] - 2026-05-07

### Added

- P1 MUST `p1-must-secret-non-leaky-path` (conditional on CLI accepting secret material): sensitive inputs are readable via stdin or a `--*-file` flag; flag-value and env-var inputs MAY exist for convenience but MUST NOT be the only path. by @brettdavies in [#25](https://github.com/brettdavies/agentnative/pull/25)
- P2 MUST `p2-must-schema-print` (conditional on structured output): expose the output schema via a `schema` subcommand or `--schema` flag, runtime-discoverable, with a documented format identifier (canonical recommendation: JSON Schema 2020-12).
- P2 SHOULD `p2-should-schema-file` (conditional on structured output): also export the schema to a stable file path so CI and static-analysis consumers can pin without invoking the tool.
- P2 SHOULD `p2-should-json-aliases`: accept `--json` and `--jsonl` as aliases for `--output json` and `--output jsonl`.
- P4 SHOULD `p4-should-enumerate-valid-set` (conditional on closed-set rejection): when rejecting input against an enum or fixed-allowed-values set, the error message includes the valid set.
- P6 MUST `p6-must-sigterm` (conditional on long-running operations): flush or roll back partial writes, release locks, exit non-zero within a bounded shutdown window. Next invocation succeeds without manual cleanup.
- P6 MAY `p6-may-standard-names` (conditional on subcommands): follow community-standard verbs (`get` / `list` / `create` / `update` / `delete`) and flag spellings (`--force`, `--yes`, `--limit`, `--quiet`, `--verbose`).
- New principle **P8 Discoverable Through Agent Skill Bundles** (four requirements: `p8-must-bundle-install`, `p8-should-bundle-exists`, `p8-may-install-all`, `p8-may-bundle-update`). CLIs ship a top-level skill bundle (`AGENTS.md`, `SKILL.md`, or equivalent) and provide an install path that registers the bundle with installed agent runtimes (canonical form: `tool skill install [<host>]`).

### Changed

- `VERSION`: 0.3.1 → 0.4.0 (MINOR per `principles/AGENTS.md`'s versioning rules; new MUSTs added). by @brettdavies in [#25](https://github.com/brettdavies/agentnative/pull/25)
- `.impeccable.md`: new spec-channel anti-pattern "No false canonicalization". When a bullet names an outcome the implementer can satisfy any way, prose uses indefinite articles and avoids language that canonicalizes one shape; when a bullet names a citable single-shape pattern, prose uses definite articles and cites the source.

**Full Changelog**: [v0.3.1...v0.4.0](https://github.com/brettdavies/agentnative/compare/v0.3.1...v0.4.0)

## [0.3.1] - 2026-05-07

### Added

- Badge claim convention (`docs/badge.md`) defines eligibility floor (≥80% pass-rate), embed shape, score-text format, color thresholds, and version-pinning posture for tool authors who self-host the agent-native badge linked to a live scorecard. by @brettdavies in [#20](https://github.com/brettdavies/agentnative/pull/20)
- README and CONTRIBUTING pointers to the badge convention so HN visitors and tool authors land on the convention from the two top-level entry points.
- Add `BRAND.md` at the repo root. Universal voice and identity SoT shared across the spec, site, linter, and skill bundle channels. Each channel inherits the shared identity and adds register and artifacts in its own `.impeccable.md`. by @brettdavies in [#22](https://github.com/brettdavies/agentnative/pull/22)
- Add spec-channel `.impeccable.md`: RFC 2119 register rules, third-person standards voice, no-implementation-leakage anti-patterns. Narrative identity layer; literal phrase enforcement lives in the `spec` Vale rule pack.
- Add `## Acknowledgements` to README. Names foundational CLI doctrine (12-factor, POSIX, clig.dev, NO_COLOR, XDG), parallel agent-CLI synthesis sources, the spec's proximate ancestors, and the anc.dev ecosystem's mechanism contribution.
- Add deterministic pre-push voice enforcement: Vale rule packs (`styles/brand/`, `styles/spec/`), LanguageTool grammar checks over the Tailnet (graceful skip when unreachable), and pack-README drift detection. One-time setup per contributor: `brew install vale jaq bun && vale sync` after activating `core.hooksPath scripts/hooks`. The layered SoT, orchestrator behavior, contributor flow, and deferred follow-ups live in the `dev`-only architecture docs.

### Changed

- Rename README "trifecta" to "four artifacts"; add `agentnative-skill` as a first-class artifact alongside the spec, the linter, and the leaderboard. by @brettdavies in [#22](https://github.com/brettdavies/agentnative/pull/22)
- Drop `docs/architecture/voice-enforcement.md` references from main-shipped files (`AGENTS.md`, `CONTRIBUTING.md`, `principles/AGENTS.md`, `.gitignore` comment). Replace the pointers with inline narrative that names the rule packs and the LT graceful-skip behavior. The architecture docs stay on `dev` as contributor-side reference and are not shipped to `main`. by @brettdavies in [#24](https://github.com/brettdavies/agentnative/pull/24)
- Update the `RELEASES.md` Prose scrubbing procedure to scrub-before-submit. Step 1 covers three entry points (scratch authoring for `gh pr create`, fetch-then-clean for `gh pr edit`, `cp CHANGELOG.md` for changelog scrub); step 6 submits the cleaned version once via `--body-file`.

**Full Changelog**: [v0.3.0...v0.3.1](https://github.com/brettdavies/agentnative/compare/v0.3.0...v0.3.1)

## [0.3.0] - 2026-04-28

### Added

- `active` status value for principles, joining `draft | under-review | locked` as the default state for shipped principles. by @brettdavies in [#12](https://github.com/brettdavies/agentnative/pull/12)
- README `## Status` section and HN-scroller hook explaining the active-with-pressure-tests-welcome posture.

### Changed

- All 7 principles flipped from `status: draft` to `status: active` for the v0.3.0 release. by @brettdavies in [#12](https://github.com/brettdavies/agentnative/pull/12)
- `principles/AGENTS.md` pressure-test protocol updated for the new status lifecycle (`active` is the default; `under-review` is reserved for substantive critique cycles).
- README restructured for HN-visitor flow: new `## The trifecta` callout (spec + linter + leaderboard as equals); `## Quick start` lead with `brew install brettdavies/tap/agentnative`; `## Live leaderboard` preview table; admin sections (Versioning, Decision records, Related, Contributing, License) reordered below spec content. License section tightened from 4 paragraphs to 3 bullets; Contributing tightened from 4-bullet list to 1 paragraph. by @brettdavies in [#14](https://github.com/brettdavies/agentnative/pull/14)
- README leaderboard URLs corrected from bare `anc.dev` to `anc.dev/scorecards`.
- AGENTS.md adds `brettdavies/agentnative-skill` as a documented downstream consumer (introductory list + cross-repo context table); replaces the prior `~/.claude/skills/agent-native-cli/` row with the public `~/dev/agentnative-skill` row.

### Documentation

- G11 red-team pass on all 7 principle files via `compound-engineering:ce-adversarial-document-reviewer`. 25 findings: 11 prose edits applied (P1 TUI parenthetical, P2 sysexits acknowledgment, P4 dependency-gating cleanup, P5 `--dry-run` write-gate + retry hedge, P6 SIGPIPE language-neutral + global-flags behavioral lead, P7 LLM-vs-non-LLM cost generalization), 10 `[later]` notes appended for v0.4.0 follow-up, 2 `[wontfix]` notes, 2 skipped. No requirement IDs added/removed/renamed; no level/applicability changes; no `last-revised:` bumps; no VERSION bump triggered. by @brettdavies in [#13](https://github.com/brettdavies/agentnative/pull/13)
- Three summary-text tightenings (P4 `gating-before-network`, P6 `sigpipe`, P6 `global-flags`) introduce mild registry-readable drift documented in pressure-test notes for v0.4.0 follow-up.

**Full Changelog**: [v0.2.0...v0.3.0](https://github.com/brettdavies/agentnative/compare/v0.2.0...v0.3.0)

## [0.2.0] - 2026-04-23

### Added

- Per-principle `requirements[]` frontmatter contract: 46 stable requirement IDs (`p1-must-env-var` … `p7-may-auto-verbosity`) with `level`, `applicability`, and `summary`. by @brettdavies in [#3](https://github.com/brettdavies/agentnative/pull/3)
- `status: draft | under-review | locked` field on every principle.
- `principles/AGENTS.md` authoring conventions and pressure-test protocol.
- `docs/decisions/` named records: P1 behavioral-MUST doctrine, naming rationale.
- `scripts/generate-changelog.sh` — two-stage release-note generator that runs `git-cliff` for the skeleton and a Python post-processor to fetch PR bodies from the GitHub API and expand each entry with `### Added / Changed / Fixed / Removed / Security` subsections. Ported from `brettdavies/agentnative`. by @brettdavies in [#9](https://github.com/brettdavies/agentnative/pull/9)

### Changed

- Requirement IDs are now sourced from this repo; `agentnative-cli` will vendor the spec and drift-check against it (previously the CLI embedded the list in `src/principles/registry.rs`). by @brettdavies in [#3](https://github.com/brettdavies/agentnative/pull/3)
- `CONTRIBUTING.md`: versioning rule now covers frontmatter-shape changes as MINOR.
- `cliff.toml` switched from fragile commit-body-header parsing (which broke when markdown headers got stripped during cherry-picks) to subject-line-with-PR-link rendering. The PR body is now the source of truth for release notes. by @brettdavies in [#9](https://github.com/brettdavies/agentnative/pull/9)

**Full Changelog**: [v0.1.1...v0.2.0](https://github.com/brettdavies/agentnative/compare/v0.1.1...v0.2.0)

## [0.1.1] - 2026-04-20

### Added

- Seven agent-native principles (P1–P7) published with `last-revised: 2026-04-20` per-principle calver.
- Governance model: three-repo architecture (spec / CLI / site), AI disclosure on all contributions, human co-sign on
  principle edits and PRs, coupled-release protocol between spec and checker.

### Changed

- P1 "Non-Interactive by Default" — applicability gates added (help-on-bare-invocation, agentic flag,
  stdin-as-primary-input).
