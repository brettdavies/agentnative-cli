---
title: "feat: Show HN launch readiness — agentnative-cli"
type: feat
status: active
date: 2026-04-28
parent: ~/.gstack/projects/brettdavies-agentnative/brett-dev-design-show-hn-launch-inversion-20260427-144756.md
---

> **Parent:** `~/.gstack/projects/brettdavies-agentnative/brett-dev-design-show-hn-launch-inversion-20260427-144756.md`
> (the central Show HN launch tracker — single source of truth for gates, scope, approach across spec/CLI/site/skill).
> This per-repo plan inherits gates from the parent and is authoritative for repo-internal execution detail only.

# feat: Show HN launch readiness — agentnative-cli

## Overview

The CLI repo owes the launch one thing: **`brew install anc` works end-to-end on a cold device on launch morning**, with
the spec-vendor work merged on `dev` cut into a tagged release the night before. Everything else this repo controls
(naming alignment, plan-checkbox drift) is already shipped or trivial sweep work. This plan stages the Gate 7 release,
verifies the install path from a cold device, and audits CLI-side plans for the Gate 4 status sweep. It explicitly does
**not** execute anything — the plan is the deliverable for this session.

The substantive launch-blocking work is owned by the site repo (Gate 8 anc100 leaderboard, Gate 12 cold-device
reachability) and the spec repo (Gates 1–5, 9, 11). This plan exists primarily to serialize the CLI's release-train
choreography so the night-before cherry-pick to `release/launch` does not surprise anyone.

---

## Problem Frame

The CLI has accumulated three releases worth of merged-but-unreleased work on `dev` since `v0.1.3` (latest tag):

- `feat(spec-vendor)` (#29) — build-time `REQUIREMENTS` from vendored `principles/*.md` frontmatter, adds `spec_version`
  field to the JSON scorecard.

That single feature commit is the entirety of the unreleased CLI surface. The launch presents three coupled obligations:

1. Ensure that feature lands in a published release — Show HN readers who `brew install anc` should get the spec-vendor
   build (with `spec_version` in their scorecards), not `v0.1.3` from before the contract was tightened.
2. Verify the published binary actually installs and runs cleanly from a fresh device with no developer toolchain
   present — the central tracker calls this out explicitly under Gate 7.
3. Reconcile this repo's `docs/plans/*.md` checkbox drift before launch (Gate 4 sweep).

Naming-alignment work that touched this repo (Gate 5 — local rename + in-place plan drift fix) is already verified done
per the spec-side close-out in
[`agentnative-spec/docs/plans/2026-04-27-001-refactor-three-repo-naming-alignment-plan.md`](https://github.com/brettdavies/agentnative-spec/blob/dev/docs/plans/2026-04-27-001-refactor-three-repo-naming-alignment-plan.md).
No CLI execution remains for that gate.

---

## Requirements Trace

Inherited from the central tracker. CLI-owned gates only:

- **Gate 7 — CLI spec-vendor work merged + release tagged.** `brew install anc` must work end-to-end on a cold device.
  (PRIMARY)
- **Gate 4 — All plan checkboxes reflect reality.** CLI sweep of `docs/plans/*.md`. (HOUSEKEEPING)
- **Gate 5 — Naming alignment U2-U5/U8-U9.** CLI portion already shipped per spec-side close-out. (DONE; tracked here
  for completeness, no CLI work remaining)

Gates **not** owned by this repo: 1, 2, 3, 6, 8, 9, 10, 11, 12. Listed in Cross-references for awareness.

---

## Scope Boundaries

- This plan is the deliverable for this session. **Do not execute the gates** — execution begins in the next session per
  the handoff's Step 6.
- This plan does not subsume `2026-04-17-001-feat-multi-language-source-checks-plan.md` (Go/Ruby/TypeScript starter
  checks). That plan stays `active` and ships post-launch — multi-language source coverage is **invisible to Show HN
  readers** at launch (the post does not name-drop language coverage as a hook).
- This plan does not include TODO 016 (lib + bin split for internal test access —
  `.context/compound-engineering/todos/016-pending-p1-lib-bin-split-for-internal-test-access.md`). Rationale in **Open
  Questions → Deferred to Implementation → Q-CLI3** below.
- Cosmetic status-string normalization across plans (`completed` vs `complete` vs `done` vs `shipped` vs `implemented`)
  is out of scope for the Gate 4 sweep — only checkbox drift and stale `active`/`in-progress` markers matter for launch
  credibility.

### Deferred to Follow-Up Work

- TODO 016 (lib + bin split): post-launch cleanup, unblocks future drift tests. Not Show HN-visible.
- Multi-language source checks (Go/Ruby/TypeScript): per its own plan, post-launch.
- Status-string normalization across `docs/plans/*.md`: post-launch chore, not credibility-load-bearing.

---

## Context & Research

### Unreleased work on `dev` (the cherry-pick scope)

```text
2989e21 docs(plans): mark spec-vendor plan completed (#29 merged)
9a264f9 feat(spec-vendor): build-time REQUIREMENTS from vendored frontmatter (#29)
a918225 docs(plans): add spec vendoring plan for build-time REQUIREMENTS generation
```

Three commits. One feature (`#29`), one plan-add, one plan-close. All ship in the night-before pre-launch release PR to
`release/launch` → `main`. Tag triggers the existing reusable workflow at
`brettdavies/.github/.github/workflows/rust-release.yml`.

### Coordinating in-flight plans (do NOT dual-file)

- `docs/plans/2026-04-23-001-feat-spec-vendor-plan.md` — `status: completed`. Shipped via `#29`. Referenced here as the
  upstream of the only feature in the cherry-pick scope.
- `docs/plans/2026-04-17-001-feat-multi-language-source-checks-plan.md` — `status: active`. Spike done, units unstarted.
  **Stays active through launch.** Not in scope here.
- Cross-repo: spec-side
  [`2026-04-27-001-refactor-three-repo-naming-alignment-plan.md`](https://github.com/brettdavies/agentnative-spec/blob/dev/docs/plans/2026-04-27-001-refactor-three-repo-naming-alignment-plan.md)
  (`status: shipped`) is the authoritative record for naming-alignment U2/U3 (CLI side).

### Release pipeline (existing infrastructure — already in use)

- `.github/workflows/release.yml` — tag-driven (`v[0-9]+.[0-9]+.[0-9]+`). Calls reusable `rust-release.yml` which runs:
  check-version → build (5 targets) → crates.io publish → draft GitHub release → homebrew tap dispatch.
- Pre-push hooks at `scripts/hooks/pre-push` mirror CI exactly (fmt, clippy `-Dwarnings`, test, cargo-deny, Windows
  compat). Activated via `git config core.hooksPath scripts/hooks` after clone.
- `homebrew-tap/Formula/agentnative.rb` is updated automatically by the release pipeline's homebrew dispatch (per
  `2026-04-02-002-feat-release-infrastructure-plan.md`, `status: complete`).

### Institutional learnings (`docs/solutions/`)

Searched via `qmd query "homebrew tap formula update"` and `qmd query "rust release pipeline"`. The release pipeline is
documented and has been exercised through three releases (v0.1.1, v0.1.2, v0.1.3). No new learnings need to be captured
pre-launch. A post-launch retro can compound any lessons from the cold-device verification.

### External references

None applicable — this is repo-internal release choreography.

---

## Key Technical Decisions

- **Cherry-pick scope is the full `v0.1.3..dev` window.** Three commits, tightly coupled (the two doc commits frame the
  feature commit). No cherry-pick selection needed; the release branch resets to `dev` HEAD at branch-cut time.
- **Recommended next version: `v0.2.0` (MINOR).** The spec-vendor change is additive at the JSON scorecard level
  (`spec_version` is a new field consumers may key on), and it changes the `REQUIREMENTS` provenance from
  hand-maintained to build-time-derived. Schema is `0.3` (already pre-launch additive), so the JSON contract bump is
  cosmetic — but the user-facing capability is meaningful enough to warrant MINOR per the project's working pattern.
  Final call deferred to Q-CLI1 below.
- **Cold-device verification target: macOS (Apple Silicon).** Single primary verification path matches Show HN reader's
  most-likely posture. Linux verification optional; Windows is best-effort and explicitly not in the install-path
  narrative.
- **Do not cut the release branch until the spec repo's pre-launch PR is staged.** Because the spec-vendor feature pulls
  from a pinned `agentnative-spec` ref, a spec-side `v0.2.0` release timed for launch should land first so the CLI's
  published `spec_version` matches what readers see at `anc.dev`. Sequencing detail in the Pre-launch release PR
  checklist below.

---

## Open Questions

### Resolved during planning

- **Should TODO 016 be included?** No — defer post-launch. Internal API refactor, not Show HN-visible, and including it
  expands the cherry-pick scope at exactly the moment the launch needs the diff to be tight. The plan's scope-boundaries
  section codifies the deferral.
- **Are there other CLI-side gates beyond 4/5/7?** No — Gates 1, 2, 3, 6, 8, 9, 10, 11, 12 are owned by spec, site,
  vault, or no-repo per the central tracker.

### Deferred to implementation

- **Q-CLI1: PATCH (`v0.1.4`) or MINOR (`v0.2.0`) for the next release?** Recommendation: MINOR. Decide at branch-cut
  time. If the spec repo also publishes `v0.2.0` for the same launch window, version-aligning the two repos is a small
  but real readability win for anyone scanning both release pages. If spec stays at `v0.1.x`, CLI MINOR is still
  defensible on the additive-field grounds.
- **Q-CLI2: Which day to cut the release branch — Tuesday night, Wednesday night, or Thursday night?** Inherits from
  parent tracker's Q4 (post-day decision). Recommendation: cut release branch the evening **before** the post day, so
  the published binary has 8–12 hours of bake time before the post lands. If post day is Thursday, branch-cut Wednesday
  night.
- **Q-CLI3: Should the lib/bin split (TODO 016) ride this release?** Recommendation: no, defer post-launch. Even though
  the refactor is mechanical and ~1 hour, it would land an additional `[lib]` Cargo target in a `MINOR` release — a
  consumer-facing surface change distinct from the spec-vendor feature. Cleaner story is "v0.2.0 = spec-vendor", and
  TODO 016 lands in v0.2.1 or v0.3.0 alongside whatever tests it unblocks.
- **Q-CLI4: Cold-device verification — borrow a fresh Mac, or `brew uninstall` and reinstall on the dev machine?**
  Borrow if at all possible. `brew uninstall + brew install` is **not** equivalent to a cold-device test because the
  Homebrew bottle cache, prior tap registration, and any `~/.cargo` artifacts can mask install-path bugs. If a fresh Mac
  is unavailable, the fallback is `brew uninstall agentnative && brew untap brettdavies/tap && rm -rf
  ~/Library/Caches/Homebrew/downloads/*agentnative*` before reinstall — explicit, documented, called out as a known
  limitation.

---

## Implementation Units

- U1. **Pre-launch CHANGELOG + version-bump prep**

**Goal:** Ensure CHANGELOG.md and `Cargo.toml` are ready to ship the next release without surprises at branch-cut time.

**Status:** `not-started`

**Requirements:** Gate 7 (release tagged with the spec-vendor work), Q-CLI1.

**Dependencies:** None. Can run any time before the release branch is cut.

**Files:**

- Modify: `CHANGELOG.md` — add the new version's section per `cliff.toml` conventions, drawing from the `## Changelog`
  blocks in #29's PR body.
- Modify: `Cargo.toml` — bump `version = "0.1.0"` (current dev value — see note below) to the resolved version per
  Q-CLI1.
- Modify: `Cargo.lock` — regenerated by `cargo build` after the version bump.

**Approach:**

- Resolve Q-CLI1 first (MINOR vs PATCH). Recommendation MINOR (`v0.2.0`).
- Run `scripts/generate-changelog.sh` if the project uses it for changelog generation, otherwise hand-edit per the
  existing CHANGELOG voice.
- The version-string-on-`dev` note: `Cargo.toml` shows `version = "0.1.0"` even though tags `v0.1.1`/`v0.1.2`/`v0.1.3`
  exist. This is consistent with prior release branches doing the bump on the `release/*` branch rather than `dev`.
  Confirm this pattern at branch-cut time (look at `git log --oneline release/* -- Cargo.toml` from prior releases) and
  follow the established convention. If the convention is "bump on release branch", do not bump on `dev` here — only
  prep the CHANGELOG. The version field gets bumped in the release-branch commit, not before.

**Patterns to follow:** `2026-04-02-002-feat-release-infrastructure-plan.md` (release infrastructure plan); prior
release commits in `git log --grep='feat(v0\.1' --oneline`.

**Test scenarios:**

- Happy path: `cargo build` after version bump succeeds; `Cargo.lock` updates cleanly.
- Edge case: `cliff.toml` extracts the right commits when the release branch is cut. Verify by running `git cliff --tag
  v0.2.0 --unreleased` (dry-run, no changes) before tagging.
- Test expectation: no new test code — this is release-prep, not feature work.

**Verification:**

- `CHANGELOG.md` head section names the resolved version with non-empty user-facing bullets.
- `Cargo.toml` version matches the intended tag (whether bumped here or deferred to release-branch commit per
  established convention).

---

- U2. **Cut `release/launch` branch + open pre-launch PR**

**Goal:** Stage the night-before release PR from `dev` → `release/launch` → `main` per Brett's standing pattern.

**Status:** `not-started`

**Requirements:** Gate 7. Inherits Q-CLI2 (which night?).

**Dependencies:** U1 (CHANGELOG ready). Spec repo's pre-launch release PR ideally staged first (see Key Technical
Decisions § sequencing).

**Files:**

- Branch: `release/launch` cut from `dev` HEAD at branch-cut time.
- Modify (on release branch): `Cargo.toml` version bump if not done in U1.
- Modify (on release branch): `CHANGELOG.md` final polish if any commits land between U1 and branch-cut.

**Approach:**

- `git switch -c release/launch dev` from a clean dev head.
- If the established convention is "bump version on the release branch": commit the `Cargo.toml` + `Cargo.lock` bump as
  `chore(release): vX.Y.Z` per existing pattern.
- Open PR `release/launch` → `main` titled per Conventional Commits (`chore(release): vX.Y.Z`). Body uses
  `.github/pull_request_template.md` cascade per global CLAUDE.md.
- Ensure pre-push hook passes (`scripts/hooks/pre-push` runs fmt, clippy, test, cargo-deny, Windows compat).

**Patterns to follow:** Look at the merge-commit + branch-graph for `v0.1.3` (#27) and `v0.1.2` (#24) to mirror the
exact branch-naming and PR-body shape used previously.

**Test scenarios:**

- Pre-push hook passes on the release branch (fmt, clippy, test, cargo-deny, Windows compat).
- CI on the release-branch PR shows green: build, test, audit. Watch via `gh pr checks <pr> --watch`.
- Test expectation: no new tests — this is release plumbing.

**Verification:**

- PR open, CI green, ready for merge approval. Do **not** merge yet — that's U3.

---

- U3. **Tag the release + monitor pipeline**

**Goal:** Trigger the release pipeline (`release.yml` → reusable `rust-release.yml`) and watch it land cleanly to
crates.io, GitHub Releases, and the Homebrew tap.

**Status:** `not-started`

**Requirements:** Gate 7.

**Dependencies:** U2 (release PR merged to `main`).

**Files:**

- Tag (annotated): `vX.Y.Z` on the merge commit on `main`.
- Triggered by tag (no manual edits):
- GitHub Release artifacts (5 build targets per release pipeline).
- crates.io publish of `agentnative` v X.Y.Z.
- Homebrew tap `Formula/agentnative.rb` PR/dispatch updating `url`, `sha256`, and version line.

**Approach:**

- After `release/launch` merges to `main`, `git tag -a vX.Y.Z -m "vX.Y.Z"` on the merge commit; `git push origin
  vX.Y.Z`.
- Per global CLAUDE.md "CI monitoring is automated": after `git push --tags`, the CI-watch hook fires and the agent
  spawns `gh run watch <id> --exit-status` background processes for each active run. After watchers complete, re-run `gh
  run list --branch main` to catch the chained Homebrew dispatch (`finalize-release.yml` may chain further).
- If anything is red, **do not proceed to U4**. Diagnose (most likely candidates: a target build failure on Windows, a
  homebrew dispatch token issue, a crates.io rate-limit). Use `/investigate` if the failure mode isn't immediately
  obvious.

**Patterns to follow:** Prior tag-and-watch sequences for `v0.1.1`/`v0.1.2`/`v0.1.3`.

**Test scenarios:**

- Happy path: all 5 build-target jobs succeed; crates.io publish exits 0; GitHub Release draft auto-created with
  artifacts; Homebrew tap formula PR auto-opens (or auto-merges, per the existing dispatch flow).
- Error path: a single target fails — re-run that job once before escalating to investigation.
- Test expectation: no new tests — this is pipeline observation.

**Verification:**

- `agentnative vX.Y.Z` page exists on crates.io.
- GitHub Release for `vX.Y.Z` published (not draft) with all artifacts attached.
- `homebrew-tap/Formula/agentnative.rb` on `main` references `vX.Y.Z` and the correct sha256.

---

- U4. **Cold-device `brew install anc` verification**

**Goal:** Prove the install path readers will use on launch morning actually works end-to-end.

**Status:** `not-started`

**Requirements:** Gate 7 (the second half — release tagged is necessary but not sufficient; the install must work from a
cold device).

**Dependencies:** U3 (release published, tap formula updated).

**Files:** None modified — this is pure verification.

**Approach:**

- Per Q-CLI4: borrow a fresh macOS device (Apple Silicon preferred) with no prior Homebrew agentnative footprint. If
  unavailable, run the documented uninstall+untap+cache-clear sequence on the dev machine and proceed with the known
  limitation.
- Run, in order, capturing stdout/stderr to a verification log:

1. `brew install brettdavies/tap/agentnative` (or follow whatever tap-prefixed form the README documents at launch time)
2. `which anc` — confirm binary on `PATH`
3. `anc --version` — confirm version matches the published tag
4. `anc --help` — confirm help renders without errors
5. `mkdir -p /tmp/anc-cold && cd /tmp/anc-cold && cargo init --bin && anc .` — confirm a real check run works on a fresh
   trivial Rust project
6. `anc . --output json | jq .scorecard.spec_version` — confirm the spec-vendor field is populated (this is the
   post-launch receipt that the tagged release contains the spec-vendor work)

- If anything fails, **do not post**. Open an incident issue and diagnose.

**Patterns to follow:** None — this is novel verification work for this launch, but follows the broader pattern of
"verify install paths from a clean room" used by `homebrew-tap-publish` skill flows.

**Test scenarios:**

- Happy path: every command above exits 0; `--version` matches the tag; `spec_version` field is non-null and matches the
  spec repo's published version.
- Edge case: Homebrew bottle for the platform isn't built yet (race condition with the tap dispatch). Mitigation: wait
  10–15 minutes after tap formula PR merges, then retry. If still failing, manual `brew install --build-from-source` is
  the documented fallback.
- Error path: `anc --version` shows an older version. This means the tap formula update lagged or didn't run. Re-trigger
  the homebrew dispatch from `release.yml` workflow re-run UI.
- Test expectation: verification log committed (or pasted into the launch tracker chore commit) so the launch retro has
  a receipt of what was tested.

**Verification:**

- All 6 commands above produce expected output. Verification log saved to
  `~/.gstack/projects/brettdavies-agentnative/cold-device-verification-{date}.md` (private, not committed to this repo).

---

- U5. **Gate 4 plan-checkbox sweep (CLI side)**

> **Close-out (2026-04-27).** Sweep landed in `be767e4`. Actual scope diverged from the planning text in two
> instructive ways: (1) the three "Likely modify" handoff docs were **moved** to `.context/handoffs/` (local-only,
> gitignored) rather than status-flipped in place, and the relocation expanded to all 6 handoff-shaped docs across
> `docs/plans/` and `docs/plans/spikes/`; (2) the multi-language source-checks plan kept `status: active` (CE plan
> status enum is binary `active`/`completed` — no `deferred` exists) with a post-launch deferral admonition added at
> the top of the file. Net public surface: `docs/plans/` dropped from 12 plans to 9 + 2 spike artifacts. No
> status-string normalization, as planned.

**Goal:** Reconcile `docs/plans/*.md` checkboxes and statuses so a reader landing in this repo from the Show HN post
sees plans that match reality.

**Status:** `done`

**Requirements:** Gate 4 (CLI-repo subset — spec repo owns the rest of the sweep).

**Dependencies:** None. Can run any time before launch morning.

**Files:**

- Audit: every file under `docs/plans/*.md` (12 plans currently).
- Likely modify (suspect drift from initial scan):
- `docs/plans/2026-04-20-v011-handoff-1-agentnative-impl.md` — currently `status: in-progress` but v0.1.1 shipped on
  2026-04-21. Almost certainly stale; flip to `status: completed` (or whichever terminal status matches the file's
  voice) and check the box state inside.
- `docs/plans/2026-04-20-v012-handoff-4-behavioral-checks.md` — no status frontmatter at all. Verify: did v0.1.2 ship
  the behavioral-checks expansion? `git log --grep='v0.1.2'` says yes (`f969f8c`, #24). Add `status: completed`
  frontmatter or convert to a clearly-labeled handoff-archive doc.
- `docs/plans/2026-04-21-v012-h4-eng-agent-handoff.md` — same shape as above, no status. Same treatment.
- Possibly leave unchanged:
- `docs/plans/2026-04-17-001-feat-multi-language-source-checks-plan.md` — `status: active`. Spike done, units unstarted.
  Either keep `active` or downgrade to `deferred` (acknowledging the post-launch ship-window). Either is defensible —
  pick whichever the spec-side sweep ends up using as canonical so the two repos are coherent.
- Out of scope (not modified): cosmetic status-string normalization (`completed` vs `complete` vs `done` vs `shipped` vs
  `implemented` — five terms in current use across 12 plans).

**Approach:**

- For each plan with `status: active` or `status: in-progress`: open it, read the closing/last-updated state, decide
  whether the underlying work shipped, update status + checkboxes accordingly. Do **not** invent ship-evidence — if a
  plan looks abandoned but unrelated to launch, leave it `active` and note in the launch retro.
- Plans with no `status:` frontmatter: add one only if doing so doesn't change reader interpretation. Handoff-archive
  docs may legitimately not have `status:` — those are fine as-is if the filename or H1 already signals "archive."
- This unit lands as a single docs commit `chore(plans): reconcile checkboxes and statuses for launch readiness` direct
  to `dev` per the global CLAUDE.md branch-discipline carve-out for `docs/plans/**`. No feature branch needed.

**Patterns to follow:** Spec-side naming-alignment plan's close-out block (the `> **Close-out (2026-04-27).** All 9
implementation units shipped...` admonition at the top) is a good pattern for plans that need a terminal "this actually
shipped, here's the evidence" summary on top of a stale body.

**Test scenarios:**

- After commit: `for f in docs/plans/*.md; do grep -m1 '^status:' "$f" || echo "NO_STATUS -- $f"; done` produces a list
  where every `status:` line matches reality and every `NO_STATUS` entry is intentional.
- Test expectation: none — this is a docs reconciliation chore.

**Verification:**

- A skim of `docs/plans/` shows no plan claiming `active` or `in-progress` for work that has actually shipped.
- The commit message names the change in user-facing terms: "reconcile statuses to match shipped reality."

---

- U6. **(Coordination only) — Gate 5 status confirmation**

**Goal:** Confirm and document, in this plan, that Gate 5's CLI-side scope is fully done; no execution remains here.

**Status:** `done`

**Requirements:** Gate 5.

**Dependencies:** None.

**Files:** None.

**Approach:**

- Per the spec-side close-out block in
  [`agentnative-spec/docs/plans/2026-04-27-001-refactor-three-repo-naming-alignment-plan.md`](https://github.com/brettdavies/agentnative-spec/blob/dev/docs/plans/2026-04-27-001-refactor-three-repo-naming-alignment-plan.md):
- U2 (filesystem rename `~/dev/agentnative` → `~/dev/agentnative-cli`): non-commit, complete.
- U3 (in-place CLI repo drift fix on a then-uncommitted plan file): healed in place; the host file was committed later
  under the spec-vendor plan.
- Cross-check: `rg 'brettdavies/agentnative-spec' .` from the repo root returns 0 hits. **Confirmed at plan-write
  time.**
- This unit is a status-acknowledgement only. No work remains.

**Verification:**

- `rg 'brettdavies/agentnative-spec' .` returns no hits. (Verified during plan authoring: zero hits.)

---

## System-Wide Impact

- **Interaction graph:** The release pipeline (`release.yml` → `rust-release.yml` reusable) interacts with three
  external systems (crates.io, Homebrew tap repo, GitHub Releases). Failure in any one halts the launch's install story.
  U4 catches the integrated-failure mode.
- **Error propagation:** A red CI run on U2 or U3 propagates to "we don't post" — there is no graceful degrade.
- **State lifecycle risks:** None novel. Three releases of prior art with the same pipeline.
- **API surface parity:** The `spec_version` field added by spec-vendor is a JSON-scorecard surface change. Consumers
  must feature-detect (per the AGENTS.md `0.x` schema policy). Not a breaking change.
- **Integration coverage:** U4's cold-device test is the integration coverage that matters. Nothing else proves the
  install path end-to-end.
- **Unchanged invariants:** Exit codes (0/1/2 — see AGENTS.md), the bare-`anc`-prints-help fork-bomb guard, and the
  three-layer check architecture are unchanged. The release does not modify `cli.rs`'s `arg_required_else_help`
  property; U3 of the dogfooding-safety plan (`docs/plans/2026-04-02-001-fix-fork-bomb-dogfood-safety-plan.md`) protects
  this invariant explicitly.

---

## Risks & Dependencies

| Risk                                                                                           | Mitigation                                                                                                                                                                                              |
| ---------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Homebrew bottle build fails for one platform on launch night                                   | Existing pipeline allows per-target re-runs. Document the manual `brew install --build-from-source` fallback in the launch tracker. Decide ship/no-ship per the central tracker's "ship without" rule.  |
| crates.io publish fails (rate limit, auth)                                                     | crates.io publish is idempotent on tag. Re-run pipeline after token refresh. If unrecoverable in launch window, ship without crates.io and call it out in the post — `brew install` is the primary CTA. |
| Cold-device `brew install` exposes a packaging bug not caught by CI                            | This is exactly what U4 catches. Buffer at least 4–8 hours between U3 (tag) and the post being submitted, so a packaging bug surfaces with time to fix.                                                 |
| Spec repo and CLI repo publish version-misaligned releases (e.g., spec `v0.2.0`, CLI `v0.1.4`) | Sequence spec release first; align CLI version to MINOR if spec also bumps MINOR. Q-CLI1 captures this dependency.                                                                                      |
| Plan-checkbox sweep accidentally flips a still-active plan to `completed`                      | U5's approach explicitly says: do not invent ship-evidence. When in doubt, leave `active`.                                                                                                              |
| TODO 016 (lib/bin split) gets pulled into scope under launch-week pressure                     | Scope-Boundaries section codifies the deferral. Re-read this plan if anyone proposes adding 016 mid-week.                                                                                               |

---

## Pre-launch release PR checklist (the night-before cherry-pick to `release/launch`)

This is the explicit checklist the handoff requires. Run in order on the chosen pre-launch night (per Q-CLI2):

1. ☐ Confirm `dev` is green: `gh run list --branch dev --limit 5` shows recent successes; no in-flight runs.
2. ☐ Resolve Q-CLI1 (PATCH or MINOR — recommendation MINOR `v0.2.0`).
3. ☐ Confirm spec repo's pre-launch release PR is staged or merged. If spec is also publishing `v0.2.0` for launch
   alignment, ensure it lands first so the CLI's published `spec_version` matches.
4. ☐ Pre-flight on `dev`: U1's CHANGELOG prep landed (or will land on the release branch — confirm convention).
5. ☐ Cut `release/launch` from `dev` head (U2).
6. ☐ Bump `Cargo.toml` version on the release branch if that's the established convention; otherwise verify it was
   bumped on `dev`.
7. ☐ Open PR `release/launch` → `main`. Pre-push hook must pass.
8. ☐ CI green on the release PR (`gh pr checks <pr> --watch`).
9. ☐ Merge release PR to `main`.
10. ☐ Tag `vX.Y.Z` on the `main` merge commit. Push tag (U3).
11. ☐ Watch all release-pipeline runs: build (5 targets), crates.io, GitHub Release draft, Homebrew tap dispatch. Re-run
    individual jobs on transient failures.
12. ☐ Re-run `gh run list --branch main --limit 5` to catch chained `finalize-release.yml` and homebrew dispatch runs.
13. ☐ Verify the GitHub Release for `vX.Y.Z` is published (not draft) with all 5 artifacts attached.
14. ☐ Verify `homebrew-tap/Formula/agentnative.rb` on `main` references the new tag and correct sha256.
15. ☐ Run U4 cold-device verification. Save log to
    `~/.gstack/projects/brettdavies-agentnative/cold-device-verification-{date}.md`.
16. ☐ Update the central launch tracker: flip Gate 7 from `on-dev-pending` to `done`. Commit as `chore: update launch
    tracker — CLI release shipped`.

If any step fails irrecoverably within the launch window, escalate to "ship without" or "push launch by 24h" per the
central tracker's pre-launch night rules. **No "we'll fix it after."**

---

## Documentation / Operational Notes

- The Homebrew formula (`homebrew-tap/Formula/agentnative.rb`) is auto-managed by the release pipeline. Do not hand-edit
  during this launch.
- The README's install commands (`brew install brettdavies/tap/agentnative`) must match what the published tap formula
  serves. Verify in U4.
- Post-launch: run `/retro` within 48 hours per the central tracker's Distribution Plan; capture any cold-device install
  bugs into `docs/solutions/`.

---

## Sources & References

- **Parent (central tracker, source of truth):**
  `~/.gstack/projects/brettdavies-agentnative/brett-dev-design-show-hn-launch-inversion-20260427-144756.md`
- **Origin handoff:** `~/dev/agentnative-spec/.context/handoffs/2026-04-27-001-show-hn-launch-readiness-handoff.md`
- **Spec-side companion (when filed):**
  `agentnative-spec/docs/plans/2026-04-28-001-feat-show-hn-launch-readiness-plan.md`
- **Site-side companion (when filed):**
  `agentnative-site/docs/plans/2026-04-28-001-feat-show-hn-launch-readiness-plan.md`
- **Naming-alignment close-out (Gate 5 evidence):**
  [`agentnative-spec/docs/plans/2026-04-27-001-refactor-three-repo-naming-alignment-plan.md`](https://github.com/brettdavies/agentnative-spec/blob/dev/docs/plans/2026-04-27-001-refactor-three-repo-naming-alignment-plan.md)
- **Spec-vendor plan (the feature shipping in this release):** `docs/plans/2026-04-23-001-feat-spec-vendor-plan.md`
- **Release infrastructure (existing pipeline):** `docs/plans/2026-04-02-002-feat-release-infrastructure-plan.md`
- **Multi-language source checks (deferred post-launch):**
  `docs/plans/2026-04-17-001-feat-multi-language-source-checks-plan.md`
- **TODO 016 (deferred — internal lib/bin split):**
  `.context/compound-engineering/todos/016-pending-p1-lib-bin-split-for-internal-test-access.md`
