---
title: "Handoff 3 of 5: v0.1.1 scorecard regeneration"
type: handoff
order: 3
phase: v0.1.1
depends_on: [1, 2]
blocks: []
---

# Handoff 3: v0.1.1 scorecard regeneration

**Written for**: the session that cleans up the 10 committed scorecards in `agentnative-site` after handoffs 1 and 2
have merged. Small, mechanical, time-boxed to ~30 minutes.

## The job, in one sentence

Re-run `anc check` on every tool with a committed scorecard, commit the regenerated JSON, delete any stale IDs.

## Why this exists

Handoff 1's check-ID renames (`p6-tty-detection` → `p1-tty-detection-source`, etc.) change `results[].id` strings in
scorecard JSON. The 10 existing committed scorecards reference the old IDs and must be regenerated.

Handoff 1's PR description should document the exact rename map; consult it before running. If the rename map isn't in
the PR description, read `agentnative/src/principles/registry.rs` (the source of truth) and diff current check IDs
against the principal/layer columns.

## Read these first

1. Handoff 1 PR description (rename map + new check IDs).
2. `/home/brett/dev/agentnative-site/scorecards/*.json` — current scorecards (10 files).

That's it.

## Scope

- Reinstall `anc` from the newly-tagged `v0.1.1` (via Homebrew tap or `cargo install agentnative`).
- For each tool in `registry.yaml` that has a `version:` + `scored_at:` field, run `anc check <binary> --output json >
  scorecards/{name}-v{version}.json`.
- Update each tool's `scored_at:` in `registry.yaml` to today's date.
- Verify the output JSON includes the new v1.1 fields (`coverage_summary`, `audience`, `audit_profile`, existing
  `layer`).
- Spot-check `nvidia-smi` if it's been given a `version:` by the time this runs (it wasn't scored at handoff creation
  time).

## Out of scope

- Adding new tools to the registry. Registry growth is its own track.
- Tuning audit_profile assignments beyond what's obvious (e.g., `lazygit` → `human-tui`).
- Any code changes in `agentnative` or `agentnative-site`.

## Branch + workflow

- Branch off `dev` in `agentnative-site`: `chore/v011-regenerate-scorecards`.
- PR target: `dev`.
- This is a data-only PR. Diff should be 10 JSON files + `registry.yaml` `scored_at:` bumps.

## Definition of done

- [ ] All 10 committed scorecards regenerated against `anc v0.1.1`
- [ ] Each scorecard includes `coverage_summary`, `audience`, `audit_profile`
- [ ] `registry.yaml` `scored_at:` dates updated
- [ ] Site build passes; `/score/<tool>` pages render with new fields visible
- [ ] Leaderboard numbers not wildly different from pre-regen (sanity check — if every tool's score dropped 30%,
  something is off; investigate before merging)

## Known gotchas

- Some tools may classify differently under the new audience classifier. That's correct — the classifier is new. Don't
  panic at `lazygit` showing "human-primary"; that's the point.
- `anc check` requires the target binary to be installed. Homebrew-installed tools are already present on the scoring
  machine. Double-check version strings match `registry.yaml` before scoring.
- Do NOT hand-edit the JSON to fix scores. If a score changed for a reason you don't understand, investigate the check
  itself (maybe a false-positive in a new check); don't paper over the output.

## After this PR merges

v0.1.1 is fully landed on both repos. Begin handoff 4 (v0.1.2 behavioral checks) whenever ready.
