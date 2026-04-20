---
title: "Handoff 1 of 5: v0.1.1 agentnative implementation"
type: handoff
order: 1
phase: v0.1.1
status: in-progress
depends_on: []
blocks: [2, 3]
---

# Handoff 1: v0.1.1 agentnative implementation

**Written for**: the session that picks up the Rust implementation of v0.1.1 after the doctrine review closed on
2026-04-20. This is the first and largest handoff.

## Sibling handoffs

| # | Phase  | Repo               | Doc                                                                              |
|---|--------|--------------------|----------------------------------------------------------------------------------|
| 1 | v0.1.1 | `agentnative`      | `docs/plans/2026-04-20-v011-handoff-1-agentnative-impl.md` *(this doc)*          |
| 2 | v0.1.1 | `agentnative-site` | `docs/plans/2026-04-20-v011-handoff-2-site-spec-coverage.md` (+ session brief)   |
| 3 | v0.1.1 | `agentnative-site` | `docs/plans/2026-04-20-v011-handoff-3-scorecard-regen.md`                        |
| 4 | v0.1.2 | `agentnative`      | `docs/plans/2026-04-20-v012-handoff-4-behavioral-checks.md`                      |
| 5 | v0.1.3 | `agentnative-site` | `docs/plans/2026-04-20-v013-handoff-5-audience-leaderboard.md`                   |

## The job, in one sentence

Build the `PrincipleRegistry` + matrix generator + Check trait `covers()` addition + miscategorized-check renames + P1
applicability fix, all in `agentnative` (Rust), on a new feature branch.

## Read these first (authoritative sources)

1. `~/.gstack/projects/brettdavies-agentnative/ceo-plans/2026-04-20-p1-doctrine-spec-coverage.md` — the plan. Read the
   "Eng Review Amendments" section at the bottom FIRST; it corrects several claims in the main body.
2. `~/.gstack/projects/brettdavies-agentnative/brett-dev-eng-review-test-plan-20260420-132817.md` — test plan.
3. `docs/plans/2026-04-17-p1-non-interactive-check-gap.md` — context on why this work exists.

Do not re-read the CEO-review transcripts or the pre-doctrine spike; everything actionable is in the two files above.

## Scope (what ships in this PR)

1. **New module `src/principles/`**:

- `registry.rs` — flat `&'static [Requirement]` array covering MUSTs + SHOULDs + MAYs (~46 entries). Types:
  `Requirement`, `Level { Must, Should, May }`, `Applicability { Universal, Conditional(&'static str) }`,
  `ExceptionCategory`.
- `matrix.rs` — generator emitting `docs/coverage-matrix.md` + `coverage/matrix.json`.
- `mod.rs` — public API surface.

1. **`Check` trait gains one method**: `fn covers(&self) -> &'static [&'static str]` (requirement IDs, empty by
   default).
2. **Miscategorized check renames** (identify during implementation; at minimum):

- `p6-tty-detection` → `p1-tty-detection-source` (verifies P1 SHOULD)
- `p6-env-flags` → `p1-env-flags-source` (verifies P1 MUST)
- Audit every existing check ID; rename any whose `group()` contradicts what the check actually verifies.

1. **New CLI subcommand** `anc generate coverage-matrix` with `--out`, `--json-out`, `--check` (drift check for CI).
2. **P1 applicability gate fix**: update `src/checks/behavioral/non_interactive.rs` to pass when any of:
   help-on-bare-invocation, agentic flag present, stdin-as-primary-input. Blocks the dogfood break.
3. **Scorecard JSON v1.1 fields**: add `coverage_summary { must, should, may }`, `audience`, `audit_profile` to the
   top-level scorecard output. Do NOT touch the existing `layer` field — it already exists.
4. **Tests**: unit + golden-file per the test plan. Registry validation tests (every covers() ID resolves; every MUST
   has a check or exception; IDs unique).

## Out of scope (do NOT touch in this PR)

- New behavioral checks (`p1-flag-existence`, `p1-env-hints`, `p6-no-pager-behavioral`) — that's handoff 4, v0.1.2.
- Audience classifier logic beyond the JSON field stub — that's handoff 5, v0.1.3.
- Spec text edits in `agentnative-site` — that's handoff 2.
- Scorecard regeneration for the 10 existing tools — that's handoff 3.
- Python source coverage expansion — explicitly de-scoped.

## Branch + workflow

- Branch off `dev`: `feat/v011-principle-registry-and-coverage`.
- User's global rule: never commit to `dev`/`main` directly; always via PR. This is a hard rule.
- Pre-push hook mirrors CI (fmt, clippy -Dwarnings, test, cargo-deny, Windows compat). Run `git config core.hooksPath
  scripts/hooks` if not already set.
- PR target: `dev`. After merge, tag `v0.1.1` only when handoff 2 (spec text) has also merged.

## Definition of done

- [x] All existing tests still pass
- [x] New unit tests per test plan (+17 unit, +2 integration vs. 304-test baseline)
- [x] `cargo run -- check .` on the agentnative repo itself passes all checks (dogfood: 26 pass / 2 warn / 0 fail / 2
  skip)
- [x] `cargo run -- generate coverage-matrix --check` passes (no drift — exit 0)
- [x] `docs/coverage-matrix.md` committed
- [x] `coverage/matrix.json` committed (`schema_version: "1.0"`, 46 rows, 19 covered / 27 uncovered)
- [x] Scorecard JSON shape matches v1.1 (`coverage_summary` populated, `audience` + `audit_profile` null until v0.1.3)
- [x] CLAUDE.md updated with registry + `covers()` + matrix-lifecycle + scorecard-v1.1 conventions (commit `1509331`)

## Known gotchas

- `Check` trait `layer()`, `CheckLayer` enum, and scorecard `layer` field **already exist**. Do NOT add them — the CEO
  plan mistakenly claimed they were new.
- Rust toolchain pinned via `rust-toolchain.toml`. Do NOT `rustup update` during this work.
- `ast-grep-core` and `ast-grep-language` pinned to `=0.42.0`. Do NOT bump.
- Pre-1.0 dependency pins are load-bearing; respect them.
- Check ID renames break the 10 existing scorecards in `agentnative-site/scorecards/*.json`. That's handoff 3's problem;
  document the rename map in this PR's description for handoff 3 to consume.

## Progress

Scope complete; PR #21 open against `dev` with CI green. All DoD items checked against commit `1509331` (debug
build smoke test — see todo 011 for record). Final pre-merge validation (release build + re-run after any subsequent
commits) still pending before merge.

Flip this plan's `status` to `complete` when PR #21 merges.

## After this PR merges

Handoff 2 (site spec text + `/coverage` page) can begin. Handoff 3 (scorecard regeneration) waits for both handoff 1 and
handoff 2 to merge.
