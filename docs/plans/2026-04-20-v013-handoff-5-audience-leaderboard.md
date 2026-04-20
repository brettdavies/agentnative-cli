---
title: "Handoff 5 of 5: v0.1.3 audience detector + ANC 100 leaderboard"
type: handoff
order: 5
phase: v0.1.3
depends_on: [1, 2, 3, 4]
prerequisites: ["100-tool registry baseline complete"]
blocks: []
---

# Handoff 5: v0.1.3 audience detector + leaderboard launch

**Written for**: the session that turns the pre-GA project into a public leaderboard. Happens only after v0.1.2 is
stable AND the 100-tool registry has baseline scores. This is the launch-facing release.

## The job, in one sentence

Implement the audience detector (derived scorecard verdict), activate categorical exceptions via `registry.yaml
audit_profile`, and launch the ANC 100 leaderboard on the site.

## Hard prerequisite: DO NOT launch without this

`registry.yaml` must have ≥100 tools AND every tool must have baseline scorecards committed. Current status at handoff
creation: ~80 entries, 10 with committed scorecards. **Launching before this is met makes the leaderboard look
punitive** (5 of 10 tools would drop under new checks, HN narrative becomes "new linter rates famous tools badly based
on rules written last week" — per the CEO review's outside voice Finding #7).

If the baseline isn't ready, do not start this handoff. Work on registry growth + initial scoring first. That's its own
track; not covered by this handoff.

## Read these first

1. `~/.gstack/projects/brettdavies-agentnative/ceo-plans/2026-04-20-p1-doctrine-spec-coverage.md` — "Accepted Scope
   (v0.1.3)" section.
2. `/home/brett/dev/agentnative-site/docs/brainstorms/anc-100-audit-requirements.md` — the original leaderboard spec
   (R1-R20). Skip requirements already met by v0.1.1 + v0.1.2.
3. `src/scorecard.rs` (exists after handoff 1) — where audience fields live in JSON.

Do NOT re-read doctrine transcripts.

## Scope

### In `agentnative` (code)

- **`src/scorecard/audience.rs`** — derived classifier. Input: scorecard results. Output: `Audience::AgentOptimized |
  Mixed | HumanPrimary`. Rule: count warns across exactly 4 signal checks (`p1-non-interactive`, `p2-json-output`,
  `p7-quiet`, `p6-no-color-behavioral`). 0-1 → AgentOptimized; 2 → Mixed; 3-4 → HumanPrimary. Emit as top-level
  `audience` field in scorecard JSON.
- **Registry `audit_profile` honoring** — when a tool's registry entry has `audit_profile: human-tui | file-traversal |
  posix-utility | diagnostic-only`, the scorecard suppresses the inapplicable MUSTs per the exception category rules in
  `src/principles/registry.rs`. Emit the applied profile as top-level `audit_profile` field.
- **Drift test** — unit test that fails if any of the 4 signal check IDs don't exist in the registry. Prevents silent
  classifier breakage if a check is renamed.

### In `agentnative-site` (site)

- **Audience banner on `/score/<tool>`** — render when `audience != AgentOptimized` OR `audit_profile` is present. Copy
  stance: informational, not shaming. Example: "This tool appears optimized for humans, not agents. P1/P2/P7 warnings
  below reflect that audience choice rather than defects."
- **Leaderboard filters** — "Agent-optimized only" toggle on `/scorecards`. TUI-by-design and file-traversal tools
  hidden by default; visible when toggled on.
- **Go-live on `/scorecards`** — whatever R10-R14 of the ANC 100 audit brainstorm hasn't shipped already:
  sortable/filterable leaderboard, tier filters, install-CTA per scorecard, llms-full.txt inclusion.
- **Methodology note** — explicit sentence about audience classification being aggregate and informational, not
  authoritative. The per-check evidence is the ground truth.

## Out of scope

- PTY probes — deferred to post-v0.1.x.
- `CheckStatus::Coverage` variant — still deferred. TODOS.md item.
- `--fix` track — parallel track, different release cycle.
- GitHub Action integration — strategic track, not this release.
- Any principle restructuring — deferred; revisit with leaderboard data.

## Branch + workflow

- Two branches (one per repo):
- `agentnative`: `feat/v013-audience-classifier`.
- `agentnative-site`: `feat/v013-leaderboard-launch`.
- PR target: `dev` in each.
- Tag `v0.1.3` in `agentnative` after both merge.
- Announcement coordination (Show HN, Tweet, etc.) is NOT part of this handoff. That's a product-launch track.

## Definition of done

- [ ] Audience classifier unit tested against known fixtures (perfect-rust → AgentOptimized; pure-human-tui →
  HumanPrimary; mixed cases → Mixed)
- [ ] Drift test passes; renaming a signal check fails the build loudly
- [ ] Registry audit_profile suppression produces correct scorecard output (tested on `lazygit`, `ripgrep`, `nvidia-smi`
  at minimum)
- [ ] Site leaderboard renders ≥100 tools; sorting/filtering works
- [ ] Per-tool `/score/<tool>` page shows layer callouts, coverage summary, audience banner (if applicable), and
  exception reason (if audit_profile set)
- [ ] Methodology page updated to describe audience classifier as informational-only
- [ ] Manual sanity check: `anc check gh` → AgentOptimized; `anc check lazygit` → HumanPrimary; `anc check ripgrep` →
  AgentOptimized

## Known gotchas

- The audience classifier is **informational, not authoritative**. It MUST NOT gate scorecard totals or override
  per-check verdicts. Per the CEO review's outside voice Finding #3: aggregate signal is strictly weaker than per- check
  evidence. The banner is a hint; the check list is the truth.
- `audit_profile` suppression does NOT delete check results from the scorecard JSON. It marks them as N/A-by- category.
  Readers get to see what was excluded and why.
- When classifier disagrees with intuition (e.g., a tool you consider agent-hostile gets AgentOptimized), do NOT patch
  the classifier. The patch goes in the registry (add an audit_profile) or in the MUST set (new check), not in the
  classifier rules.

## After this PR merges

The ANC 100 leaderboard is live. Future work:

- Community registry submissions (R19).
- PTY probe principle revision (separate doctrine call).
- `--fix` track starts (parallel to spec work).
- GitHub Action integration (strategic track).
- Post-launch tuning based on real user feedback on the scorecard.
