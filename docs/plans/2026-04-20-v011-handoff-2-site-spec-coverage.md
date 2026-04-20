---
title: "Handoff 2 of 5: v0.1.1 agentnative-site spec text + /coverage page"
type: handoff
order: 2
phase: v0.1.1
depends_on: [1]
blocks: [3]
---

# Handoff 2: v0.1.1 site spec text + `/coverage` page

**Written for**: the session landing the `agentnative-site` half of v0.1.1. Can start in parallel with handoff 1 for the
spec-text portion. The `/coverage` page consumption cannot finalize until handoff 1's matrix artifact format is stable.

## The job, in one sentence

Rewrite P1's MUST + scope note, add applicability-gate text to P1/P5/P6/P7, and add a new `/coverage` page that renders
the committed matrix artifact from `agentnative`.

## Read these first

1. `/home/brett/dev/agentnative/docs/plans/2026-04-20-p1-doctrine-call-handoff.md` — why this exists.
2. `~/.gstack/projects/brettdavies-agentnative/ceo-plans/2026-04-20-p1-doctrine-spec-coverage.md` — authoritative plan.
   Skip to "Doctrine Decision" and "Accepted Scope (v0.1.1)".
3. Existing principle docs in `/home/brett/dev/agentnative-site/content/principles/` — the files you'll edit.

Do NOT read the original spike or CEO-review transcripts.

## Scope (what ships in this PR — agentnative-site repo)

- **P1 MUST rewording (Option ε)**. Replace the current `--no-interactive` MUST bullet with behavioral wording that
  covers prompt library calls AND TUI session initialization without enumerating specific libraries. Exact target
  wording is in the CEO plan's "Doctrine Decision" section — copy verbatim, review in PR.
- **P1 scope note**. Add a "Scope" section per the CEO plan. Honest about verification gap for PTY-driving agents.
- **Applicability gates named explicitly** in P1, P5, P6, P7 principle text. Universal vs conditional MUSTs called out
  where they currently read as universal but aren't.
- **Methodology callout** added to `/scorecards` page: "compliance is self-reported via `--help`; behavioral
  verification is bounded to timeout probes."
- **New page `/coverage`**: reads the committed `coverage/matrix.json` artifact published by handoff 1. Renders
  MUSTs/SHOULDs/MAYs tables with per-requirement "verified by: [check IDs]" and named exceptions. Links each requirement
  back to its principle page anchor.
- **New scorecard renderers in `src/build/scorecards.mjs`**:
- `renderCoverageSummary(scorecard.coverage_summary)` — three-way MUST/SHOULD/MAY counts.
- `renderAudienceBanner(scorecard.audience, scorecard.audit_profile)` — "this tool is optimized for humans, not agents"
    style banner when applicable. Informational, not punitive.
- **Update `/score/<tool>` detail page** to use the new renderers. Old scorecards (pre-v1.1) render fine because new
  fields are additive; degrade gracefully if fields missing.

## Out of scope (do NOT touch in this PR)

- Any Rust code in `agentnative` — that's handoff 1.
- Scorecard regeneration — that's handoff 3.
- ANC 100 leaderboard UX beyond what the existing page already does — that's handoff 5 (v0.1.3).
- Audience-detector logic — it's computed in Rust (handoff 1) and emitted in scorecard JSON; this handoff only renders
  it.
- New principle doc files. Only edit existing P1/P5/P6/P7 files.

## Branch + workflow

- Branch off `dev` in `/home/brett/dev/agentnative-site`: `feat/v011-spec-doctrine-and-coverage-page`.
- User's global rule applies here too: PR, not direct commits to `dev`/`main`.
- Site-repo dev branch currently has in-progress scorecard-file-rename work (see `git status` there). Coordinate with
  that work; do not revert unrelated changes.
- Matrix artifact consumption: start by reading a placeholder JSON file. Swap to the real artifact once handoff 1's PR
  lands and the format is fixed. If in doubt, ask handoff 1's session for the current artifact schema before wiring the
  `/coverage` page.

## Definition of done

- [ ] Principle docs P1/P5/P6/P7 edited; lint + link-check pass
- [ ] `/coverage` page renders from the committed matrix artifact
- [ ] `/score/<tool>` detail page renders new `coverage_summary` + `audience` banner when present
- [ ] Old v1.0 scorecard JSONs (still present until handoff 3 regenerates them) render without error
- [ ] Site build passes (`bun run build` or equivalent); tests pass

## Known gotchas

- The registry `audit_profile` field is set per-tool in `registry.yaml`. That file is in this repo. No schema change is
  needed to add the field; just start setting it on tools that need a categorical exception. Don't set it speculatively;
  wait for a tool to actually need suppression.
- `nvidia-smi` was added to `registry.yaml` as part of the doctrine session with a placeholder `repo:` field
  (proprietary tool, not GitHub-hosted). Do not fix the placeholder in this PR unless asked.

## After this PR merges

Handoff 3 (scorecard regeneration) can proceed. Once both this PR and handoff 1's PR merge, tag v0.1.1 in
`agentnative`.
