---
title: "Handoff 4 of 5: v0.1.2 new behavioral checks + HelpOutput cache"
type: handoff
order: 4
phase: v0.1.2
depends_on: [1, 2, 3]
blocks: [5]
---

# Handoff 4: v0.1.2 new behavioral checks

**Written for**: the session building the three new behavioral checks that land after v0.1.1 is fully shipped and
stable. This is net-new verification code; the registry + coverage infrastructure already exists.

## Sibling handoffs

| # | Phase  | Repo               | Doc                                                                              |
|---|--------|--------------------|----------------------------------------------------------------------------------|
| 1 | v0.1.1 | `agentnative`      | `docs/plans/2026-04-20-v011-handoff-1-agentnative-impl.md`                       |
| 2 | v0.1.1 | `agentnative-site` | `docs/plans/2026-04-20-v011-handoff-2-site-spec-coverage.md` (+ session brief)   |
| 3 | v0.1.1 | `agentnative-site` | `docs/plans/2026-04-20-v011-handoff-3-scorecard-regen.md`                        |
| 4 | v0.1.2 | `agentnative`      | `docs/plans/2026-04-20-v012-handoff-4-behavioral-checks.md` *(this doc)*         |
| 5 | v0.1.3 | `agentnative-site` | `docs/plans/2026-04-20-v013-handoff-5-audience-leaderboard.md`                   |

## The job, in one sentence

Build `p1-flag-existence`, `p1-env-hints`, `p6-no-pager-behavioral` as behavioral checks, and introduce a shared
`HelpOutput` cache so all three share a single `<binary> --help` probe per tool.

## Read these first

1. `~/.gstack/projects/brettdavies-agentnative/ceo-plans/2026-04-20-p1-doctrine-spec-coverage.md` — "Accepted Scope
   (v0.1.2)" section and the "Eng Review Amendments" section (for why this set shrunk to 3 checks).
2. `src/principles/registry.rs` (will exist after handoff 1) — the MUSTs these checks verify.
3. `src/checks/behavioral/non_interactive.rs` — example of the existing behavioral check pattern.

Do NOT re-read doctrine or review transcripts.

## Scope

- **`src/runner/help_probe.rs` (new)** — `HelpOutput` struct that spawns `<binary> --help` once and exposes lazy cached
  parse views: `flags()`, `env_hints()`, `subcommands()`. Runner passes a shared `Arc<HelpOutput>` into each behavioral
  check that needs it.
- **`src/checks/behavioral/flag_existence.rs`** — new check `p1-flag-existence`. Scans parsed flags for any of:
  `--no-interactive`, `-p`, `--print`, `--no-input`, `--batch`, `--headless`, `-y`, `--yes`, `--assume-yes`. Pass if at
  least one exists. Skip (applicability false) if the target satisfies P1's alternative gates (stdin-primary,
  help-on-bare). Warn otherwise with documented false-positive/negative conditions.
- **`src/checks/behavioral/env_hints.rs`** — new check `p1-env-hints`. Scans `--help` for clap-style `[env: FOO]` hints
  OR bash-style `$FOO` / `TOOL_FOO` mentions near flag definitions. Pass if present; Warn if flags exist but no env
  hints; Skip if no flags exist.
- **`src/checks/behavioral/no_pager_behavioral.rs`** — new check `p6-no-pager-behavioral`. Pass if `--no-pager` flag
  detected in `--help`. Skip if no `pager` / `less` / `$PAGER` mentions. Warn if pager is mentioned but no `--no-pager`
  escape hatch.
- **Confidence field**: each check emits `confidence: "high" | "medium" | "low"` in its `CheckResult`. Regex-based
  probes on short flag lists = high; heuristic mentions = medium; inferences = low.
- **Register all three checks in the registry** (add entries to `src/principles/registry.rs` linking the requirement IDs
  they cover).
- **Update `docs/coverage-matrix.md` via `anc generate coverage-matrix`** — it should reflect the new coverage (these
  requirements move from "source-only" to "verified at both layers" or "newly verified behaviorally").
- **Tests per the test plan artifact** at
  `~/.gstack/projects/brettdavies-agentnative/brett-dev-eng-review-test-plan-20260420-132817.md`. Happy path,
  Skip-applicability, Warn-missing, and one non-English-help exception test per check.

## Out of scope (explicitly cut from v0.1.2)

- `p1-headless-auth-behavioral` — cut. Source-layer `p1-headless-auth` is authoritative; binary-only targets get
  "source-only verification" disclaimer.
- `p5-dry-run-behavioral` — cut. Write-verb heuristic is too fragile.
- `p6-timeout-behavioral` — cut. Network-touching classification too fragile.
- Audience classifier / banner — that's handoff 5 (v0.1.3).

If a future PR revisits any of these cuts, do it as its own plan, not here.

## Branch + workflow

- Branch off `dev` in `/home/brett/dev/agentnative`: `feat/v012-behavioral-check-expansion`.
- PR target: `dev`. Tag `v0.1.2` after merge.
- Pre-push hook runs CI-equivalent; respect its output.

## Definition of done

- [ ] All three checks have full unit-test coverage (happy + Skip + Warn + exception)
- [ ] `HelpOutput` has its own unit tests for each lazy parser
- [ ] `anc check <binary>` on the existing validation targets (`ripgrep`, `bird`, `xurl-rs`) produces sensible verdicts
  for the new checks
- [ ] Coverage matrix regenerated; committed diff shows the new checks picking up their requirements
- [ ] Dogfood: `anc check .` on the agentnative repo itself passes all new checks
- [ ] Regenerate the 10 committed scorecards in `agentnative-site` (small follow-on PR there — or bundle into this
  release's landing sequence)

## Known gotchas

- `HelpOutput` must be cached per-tool-per-invocation, NOT globally. State leak between different target tools would
  produce wrong verdicts.
- Heuristic false-positive patterns documented in `docs/coverage-matrix.md` exceptions section. If a new false-positive
  surfaces during validation, add it to the doc in this PR — don't leave for later.
- Non-English `--help` output: regexes are English-only. This is a named exception in the coverage matrix. Do NOT try to
  handle localized help in this PR.

## After this PR merges

v0.1.2 is done. Handoff 5 (v0.1.3 audience detector + leaderboard) can begin once the 100-tool registry baseline
prerequisite is met.
