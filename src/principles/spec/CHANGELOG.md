# Changelog

All notable changes to this repository are documented here — governance, validator, release infrastructure, README, decision records.

Changes to the standard itself — principle MUST/SHOULD/MAY tier moves, requirement IDs added/removed/renamed, applicability shifts — are tracked per-principle in `principles/p*-*.md` via the `last-revised:` calver frontmatter field and the `## Pressure test notes` section appended to each file.

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
