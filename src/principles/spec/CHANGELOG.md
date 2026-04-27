# Changelog

All notable changes to the agent-native CLI standard are documented here.

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
