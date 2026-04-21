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

## Status as of 2026-04-21

v0.1.1 shipped end-to-end (crates.io, GitHub Release marked Latest, Homebrew bottles clean). H2 and H3 have both landed
on `agentnative-site` `dev` (not yet promoted to `main`):

- **H2 (agentnative-site PR #24, merged 2026-04-21)** — P1/P5/P6/P7 spec text reworded and annotated with applicability
  gates; `/coverage` page renders the 46-requirement matrix from committed `src/data/coverage-matrix.json`;
  `renderCoverageSummary()` + `renderAudienceBanner()` wired into per-tool pages with graceful degradation when
  `coverage_summary`/`audience`/`audit_profile` are missing. No H4 dependencies — the site code feature-detects, so H4
  can ship without coordinating.
- **H3 (agentnative-site PR #25, merged 2026-04-21)** — All 10 committed scorecards regenerated with `anc` v0.1.1:
  `ripgrep`, `fd`, `jq`, `bat`, `dust`, `gh`, `claude-code`, `aider`, `llm`, `anc`. All now v1.1 schema
  (`schema_version`, `coverage_summary`, `audience: null`, `audit_profile: null`). Zero remaining `p6-tty-detection` or
  `p6-env-flags` references. Site `/score/<tool>` renderer verified against all 10.

**Net impact on H4:** no scope change, no unblocking needed. The scorecard-regeneration step in DoD (item 6) is a
proven follow-on workflow now — re-run `anc check` against the 10 tools, commit to `agentnative-site` via a single PR to
dev. See the "After this PR merges" section below for the exact steps.

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
- **Wire each check's `covers()` to existing registry IDs** — the registry already carries the MUSTs these checks
  verify. No registry additions, only `covers()` linkage:
- `p1-flag-existence` → `covers(&["p1-must-no-interactive"])` — second behavioral proof alongside the existing
  `p1-non-interactive` (which tests bare-invocation/help-on-bare/stdin-primary). Behaviorally distinct: flag existence
  vs. runtime behaviour.
- `p1-env-hints` → `covers(&["p1-must-env-var"])` — currently source-only via `p1-env-flags-source`. This adds the
  behavioral layer.
- `p6-no-pager-behavioral` → `covers(&["p6-must-no-pager"])` — currently source-only via `p6-no-pager`. This adds the
  behavioral layer.
- **Update `docs/coverage-matrix.md` + `coverage/matrix.json` via `anc generate coverage-matrix`** — the three
  requirements move from single-layer to dual-layer coverage. Both artifacts are committed; CI's drift check (`anc
  generate coverage-matrix --check`) enforces.
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

Post-merge coordination sequence (mirrors v0.1.1's release path):

1. **Tag v0.1.2** via the release-branch flow in `RELEASES.md` — cherry-pick non-docs commits onto
   `release/v0.1.2-<slug>` off `origin/main`, bump `Cargo.toml`, regenerate completions, run
   `./scripts/generate-changelog.sh`, PR to main, merge, annotate + push the tag. Tag push fires the usual crates.io
   publish → GitHub Release → Homebrew dispatch chain.
2. **Regenerate the 10 committed scorecards on `agentnative-site`** (mirrors H3's proven workflow). Install v0.1.2 via
   brew or `cargo install --version 0.1.2 agentnative` on the site box. For each of the 10 tools (`ripgrep`, `fd`, `jq`,
   `bat`, `dust`, `gh`, `claude-code`, `aider`, `llm`, `anc`), run `anc check <binary> --output json` and overwrite the
   corresponding file in `scorecards/`. Bump `scored_at: 2026-MM-DD` in `registry.yaml` for each tool. PR to
   `agentnative-site` `dev` in a single atomic commit — CI regression tests cover the rendering.
3. **Sync the coverage matrix to agentnative-site** — run `scripts/sync-coverage-matrix.sh` on agentnative-site dev; it
   copies `agentnative:coverage/matrix.json` into `src/data/coverage-matrix.json`. Commit on the same PR as step 2 (or
   separately — matrix sync is idempotent).
4. **Handoff 5 (v0.1.3 audience detector + leaderboard)** can begin once the 100-tool registry baseline prerequisite is
   met.
