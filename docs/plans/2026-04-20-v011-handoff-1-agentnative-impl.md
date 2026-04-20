---
title: "Handoff 1 of 5: v0.1.1 agentnative implementation"
type: handoff
order: 1
phase: v0.1.1
depends_on: []
blocks: [2, 3]
---

# Handoff 1: v0.1.1 agentnative implementation

**Written for**: the session that picks up the Rust implementation of v0.1.1 after the doctrine review closed on
2026-04-20. This is the first and largest handoff.

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

- [ ] All existing tests still pass
- [ ] New unit tests per test plan (~30 tests across registry + matrix + renames + applicability)
- [ ] `cargo run -- check .` on the agentnative repo itself passes all checks (dogfood)
- [ ] `cargo run -- generate coverage-matrix --check` passes (no drift)
- [ ] `docs/coverage-matrix.md` committed
- [ ] `coverage/matrix.json` committed (or generated at build — decide during implementation)
- [ ] Scorecard JSON shape matches v1.1: test by running `anc check` and diffing against committed golden
- [ ] CLAUDE.md updated if new conventions emerge (registry editing, covers() declaration pattern)

## Known gotchas

- `Check` trait `layer()`, `CheckLayer` enum, and scorecard `layer` field **already exist**. Do NOT add them — the CEO
  plan mistakenly claimed they were new.
- Rust toolchain pinned via `rust-toolchain.toml`. Do NOT `rustup update` during this work.
- `ast-grep-core` and `ast-grep-language` pinned to `=0.42.0`. Do NOT bump.
- Pre-1.0 dependency pins are load-bearing; respect them.
- Check ID renames break the 10 existing scorecards in `agentnative-site/scorecards/*.json`. That's handoff 3's problem;
  document the rename map in this PR's description for handoff 3 to consume.

## After this PR merges

Handoff 2 (site spec text + `/coverage` page) can begin. Handoff 3 (scorecard regeneration) waits for both handoff 1
and handoff 2 to merge.
