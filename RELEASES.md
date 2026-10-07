# Releasing `agentnative`

Operational runbook. Rationale lives in [`RELEASES-RATIONALE.md`](./RELEASES-RATIONALE.md). Pre-cut go/no-go checklist
lives in [`RELEASES-PREFLIGHT.md`](./RELEASES-PREFLIGHT.md); post-tag verification lives in
[`RELEASES-POSTFLIGHT.md`](./RELEASES-POSTFLIGHT.md).

```text
feature branch → PR to dev (squash merge)
              → overlay dev's tree onto a release/* branch cut from main
              → PR to main (squash merge)
              → tag push triggers crates.io publish + GitHub Release + Homebrew dispatch
```

Direct commits to `dev` or `main` are not permitted: every change has a PR number in its squash commit message.

## Branches

| Branch                                 | Role                                    | Lifetime                                    | Protection                           |
| -------------------------------------- | --------------------------------------- | ------------------------------------------- | ------------------------------------ |
| `main`                                 | Production. Only release commits.       | Forever.                                    | `.github/rulesets/protect-main.json` |
| `dev`                                  | Integration. All feature PRs land here. | Forever. Never delete.                      | `.github/rulesets/protect-dev.json`  |
| `feat/*`, `fix/*`, `chore/*`, `docs/*` | Feature work.                           | One PR's worth. Auto-deleted on merge.      | None. Squash into dev freely.        |
| `release/*`                            | Head of a dev → main PR.                | One release's worth. Auto-deleted on merge. | None.                                |

→ Rationale: [`RELEASES-RATIONALE.md` § Branching model](./RELEASES-RATIONALE.md#branching-model).

## Daily development (feature → dev)

```bash
git checkout dev && git pull
git checkout -b feat/short-description
# ... work ...
git push -u origin feat/short-description
gh pr create --base dev --title "feat(scope): what changed"
# CI passes → squash-merge (PR_BODY becomes the dev commit message)
```

- **Commit style**: [Conventional Commits](https://www.conventionalcommits.org/).
- **PR body**: follow `.github/pull_request_template.md`. See [§ PR body](#pr-body).
- **PR body prose scrub**: see [§ Prose scrubbing](#prose-scrubbing).

### Dev-direct exception

Paths that live only on `dev` and never ship to `main` can be committed directly to `dev` without a feature branch or
PR. The `guard-main-docs` workflow blocks them from `main` PRs regardless. The exception applies to:

- Engineering docs: `docs/brainstorms/`, `docs/ideation/`, `docs/plans/`, `docs/research/`, `docs/reviews/`,
  `docs/solutions/`, and anything under `.context/`.
- Prose-check stack: `styles/`, `.vale.ini`, `scripts/prose-check.sh`.

The standard feature → PR → squash-merge flow remains required for everything else, including consumer-facing markdown
(README, AGENTS, CONTRIBUTING, CHANGELOG, in-repo runbooks).

## PR body

Every PR (feature, fix, docs, release) uses `.github/pull_request_template.md` verbatim. Six required sections, in
template order, no inventions: `## Summary`, `## Changelog`, `## Type of Change`, `## Related Issues/Stories`,
`## Testing`, `## Files Modified`. The template's other sections (`## Key Features`, `## Benefits`,
`## Breaking Changes`, `## Deployment Notes`, `## Screenshots/Recordings`, `## Checklist`, `## Additional Context`) are
optional: fill `## Breaking Changes` on a major, delete the rest when they do not apply.

- **No explainer prose anywhere in the body.** User-facing substance only.
- **Summary describes the net diff only**: what merged `main` looks like vs the base branch. Not commit history,
  intermediate state, or cherry-pick mechanics.
- **Zero verification artifacts in the body.** No triple-diff stats, leak-check output ("`guard-main-docs` runs clean"),
  patch-id cherry-check counts, pre-push gate results, CI status, or prose-scrub findings. Anomalies get fixed before
  push, not audit-trailed.
- **Changelog** subsections (`### Added` / `### Changed` / `### Fixed` / `### Documentation` from the template, plus
  `### Breaking changes` or `### Deprecated` when a change needs one; the generator orders all six): 1-5 bullets each,
  delete empty subsections, each bullet starts with a verb. A `## Changelog` heading left standing with no bullets under
  it says the PR ships nothing user-facing, and `generate-changelog.py` adds nothing for it, whatever the PR title says.
- **Type of Change**: one checkbox. Prefer `feat`/`fix` over `chore` for any user-observable change.
- **Related Issues/Stories**: four labels (`Story:` / `Issue:` / `Architecture:` / `Related PRs:`). All four required
  even when empty (`- None.` / `n/a`).
- **Files Modified**: four sub-headers (`Modified` / `Created` / `Renamed` / `Deleted`). All four required even when
  empty.
- **No AI attribution** in commits or PR bodies.
- **No hard line wraps**: one logical line per paragraph or bullet.

→ Rationale: [`RELEASES-RATIONALE.md` § PR body conventions](./RELEASES-RATIONALE.md#pr-body-conventions).

## Releasing dev to main

Before cutting a release branch, walk [`RELEASES-PREFLIGHT.md`](./RELEASES-PREFLIGHT.md) end-to-end. Any unchecked item
holds the release.

Engineering docs (`docs/plans/`, `docs/solutions/`, `docs/brainstorms/`, `docs/reviews/`) live on `dev` only.
`guard-main-docs.yml` blocks them from reaching `main`, and `guard-release-branch.yml` rejects any PR to main whose head
isn't `release/*`.

**Branch naming**: `release/v<version>` or `release/v<version>-<slug>` (e.g. `release/v0.1.0`,
`release/v0.2.0-python-checks`). The `v<version>` prefix is required: `scripts/generate-changelog.py` extracts the
version from the branch name.

`main` and `dev` share only an ancient merge-base: every release squash-merges into `main`, so the two branches diverge
in history even as their content converges. Reconciling that with a merge, or a branch cut from `dev`, produces a pile
of rename/delete and lockfile conflicts that are artifacts of the lineage, not of the content shipping. The release
branch is therefore built as a **clean descendant of `main`** with `dev`'s tree overlaid on top, asserting the desired
end-state directly. `scripts/release/cut-release-branch.sh` builds it:

```bash
# 1. Build the branch: drift gate, branch from main, overlay dev's tree, strip the
#    guarded paths, generate CHANGELOG.md, and run checks A, B, and D. Stops
#    before committing; --dry-run prints the plan and touches nothing.
scripts/release/cut-release-branch.sh 0.2.0

# 2. Bump the version carriers and refresh the generated artifacts, in the order
#    § Project specifics lists.

# 3. Commit the overlay as one commit sitting directly on top of main, then run the
#    preflight gates against it.
git add -A
git commit
cargo build --release
scripts/release/preflight.sh all

# 4. Push and open the PR. Scrub the body in /tmp/ first.
git push -u origin release/v0.2.0
gh pr create --base main --head release/v0.2.0 --title "release: v0.2.0" --body-file /tmp/body.md
```

The script asserts `dev`'s tree onto the `main` base with `git read-tree -u --reset`, one operation that carries the
deletions too, so a file `main` carries and `dev` deleted or moved cannot ship. It strips the paths `guard-main-docs`
forbids, resolved from the workflow by `scripts/release/guarded-paths.sh` rather than from any restated copy, and writes
`CHANGELOG.md` with `scripts/generate-changelog.py --from-dev-prs --tag v<version>`. The overlay commit carries no
per-PR history, so the section is built from the PRs merged into `dev` since the previous release; scrub it per § Prose
scrubbing, fixing findings on the upstream PR bodies and regenerating, never by hand. Then it runs three checks:

| Check                                    | Asserts                                                                                                                                                                           | On failure                                                                                                     |
| ---------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| A: staged tree vs `origin/dev`           | nothing differs but the version carriers (`Cargo.toml`, `Cargo.lock`, `CHANGELOG.md`) and the stripped guarded paths                                                              | fails, naming each unexpected path                                                                             |
| B: guarded paths vs `origin/main`        | no guarded path is added or modified (`--diff-filter=ACMR`, so removing a guarded doc `main` still carries reads as cleanup, not a leak)                                          | fails, naming each leak                                                                                        |
| D: unguarded docs added to `origin/main` | every added `docs/` entry or markdown file is meant to ship (`--no-renames`, so a doc moved from one `main` carries lists as added rather than as a rename the filter would drop) | reports, never fails: each needs a reason to ship, or registering in the workflow's `extra_paths` and removing |

The worktree must be clean before it runs, because the overlay resets the index and working tree. Only exit 0 leads to
step 2:

| Exit | Meaning                                                                                    | Next                                                                                          |
| ---- | ------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------- |
| 0    | The branch is staged and every check passed; the script prints the commit steps.           | Step 2.                                                                                       |
| 1    | The drift gate, the changelog step, or a check failed.                                     | Never commit the branch. A drift failure stops before branching; any other recovers as below. |
| 2    | Setup error: a dirty worktree, an unknown ref, a missing tool, or no guarded-path pattern. | Fix what it names. Only the guarded-path error stops after branching; it recovers as below.   |

A cut that stopped after branching, at a failed check or at any failing step, leaves the overlay staged on the release
branch and prints the way back instead of the commit steps. The worktree was clean when the cut started, so everything
staged is the script's own output, and discarding it loses nothing:

```bash
git checkout -f dev
git branch -D release/v0.2.0
```

Then fix the cause and re-run; the script refuses the dirty worktree a failed cut leaves behind.

The result is a single commit whose diff against `main` is the release, with `main` as an ancestor, so the PR merges
with zero conflicts. The merge publishes nothing: no workflow in this repo triggers on a push to `main`, and every
artifact comes from the annotated tag (see [§ Tagging and publishing](#tagging-and-publishing)). Merge and tag in one
sitting, so `main` never documents a version that has no release to install. Auto-delete removes `release/v0.2.0` from
the remote on merge. `dev` is untouched.

→ Rationale (why overlay, not merge; why cut from `main`):
[`RELEASES-RATIONALE.md` § Branching model](./RELEASES-RATIONALE.md#branching-model). CHANGELOG mechanics:
[`RELEASES-RATIONALE.md` § CHANGELOG generation](./RELEASES-RATIONALE.md#changelog-generation).

### Exception: cherry-pick

The overlay is the release construction for every repo on this flow. Cherry-picking the dev squash-commits onto the
`origin/main` base is the exception, kept for a repo that has a stated reason it cannot overlay (record it under
[Project specifics](#project-specifics)); the per-PR changelog is not such a reason, since `--from-dev-prs` builds it
from `dev` either way. When cherry-picking, run the triple-diff verification:

```bash
# 1. Nothing on main that dev never received, then branch from main, NOT dev.
scripts/release/drift.sh
git fetch origin
git checkout -B release/v0.2.0 origin/main

# 2. List the dev commits not yet on main.
git log --oneline dev --not origin/main

# 3. Cherry-pick the ones to ship. Docs commits stay on dev.
git cherry-pick <sha1> <sha2> ...

# 4. Triple-diff verification.
GUARDED="$(scripts/release/guarded-paths.sh)"

git diff origin/main..HEAD --stat                                              # A: ship surface
git diff HEAD..origin/dev --name-only | grep -Ev "$GUARDED" || echo "(none)"   # B: no missed picks
git diff origin/dev..origin/main --stat | tail -5                              # C: phantom-commits sanity

# Re-confirm no guarded paths leaked. --diff-filter=ACMR for the same reason as the
# overlay's check B: a deletion of a guarded path main still carries is cleanup, not a
# leak, and an unfiltered grep aborts a correct release over it.
git diff origin/main..HEAD --diff-filter=ACMR --name-only \
  | grep -E "$GUARDED" \
  && echo "LEAKED: reset and redo" || echo "(clean)"

# D: what this release ADDS to main (see the overlay's check D for why).
# `--no-renames` lists a doc moved from one main carries as added; rename detection
# would report it as R, and the A filter would drop it.
git diff --no-renames origin/main..HEAD --diff-filter=A --name-only | grep -E '(^docs/|\.md$)' | grep -Ev "$GUARDED" || echo "(none unguarded)"

# Patch-id cherry check (noisy in squash-merge workflow; triage per-line).
git cherry HEAD origin/dev | grep '^+' || echo "(none)"
```

Cherry-picks of PRs that touched guarded paths hit modify/delete or rename/delete conflicts, since those paths live on
`dev` but are blocked from `main`; resolve them per the next section. Then generate the changelog the way the cut script
does (`scripts/generate-changelog.py --from-dev-prs --tag v0.2.0`), and finish with steps 2 to 4 of the overlay
procedure.

→ Triple-diff false-positive triage:
[`RELEASES-RATIONALE.md` § Triple-diff verification](./RELEASES-RATIONALE.md#triple-diff-verification).

### Cherry-pick conflicts on guarded paths

Cherry-picks of feature PRs that touched `docs/plans/` / `docs/brainstorms/` / `docs/ideation/` / `docs/reviews/` /
`docs/solutions/` / `.context/` files will hit modify/delete conflicts on the release branch. Those paths exist on `dev`
but are blocked from `main` by `guard-main-docs.yml`, so the cherry-pick sees them as "deleted in HEAD, modified in
`<commit>`". A PR that renames such a file (e.g., a repo-wide noun rename) also produces rename/delete conflicts on the
same paths.

Resolution (the standard `git rm` is denied by repo policy; use the plumbing form):

```bash
# 1. Mark every unmerged guarded path as deleted in the index.
git update-index --remove $(git diff --name-only --diff-filter=U)

# 2. Trash the orphan worktree files left by the rename target side.
gio trash docs/plans/<leftover-paths>.md

# 3. Continue the cherry-pick.
git cherry-pick --continue --no-edit
```

Repeat per conflicting commit. After all picks land, run `git ls-files docs/plans/ docs/brainstorms/`. If anything
remains, drop it with the same two-step pattern and commit as
`chore(release): drop stray plan spikes from cherry-pick rename detection` before the leak check. Rename detection
occasionally re-adds a path under the rename target's new name; the post-pick `ls-files` check catches that.

## Tagging and publishing

After the `release/v<version> → main` PR merges, tag and push:

```bash
git checkout main && git pull
git tag -a -m "Release v0.2.0" v0.2.0
git push origin main --tags
```

Always use annotated tags (`-a -m`). The tag push triggers `.github/workflows/release.yml`, which calls the reusable
`brettdavies/.github/.github/workflows/rust-release.yml@main` and runs:

| Step                  | What                                                                                                                                                                                                                                                                                            |
| --------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `check-version`       | Verify the tag matches `Cargo.toml` version (gate).                                                                                                                                                                                                                                             |
| `audit`               | `cargo deny check` (license + advisory + ban).                                                                                                                                                                                                                                                  |
| `build`               | Cross-compile binaries for 7 targets: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`. Each archive includes binary, completions, README, licenses. |
| `sbom`                | Generate a CycloneDX SBOM of `anc` from the tagged lockfile, with a read-only token.                                                                                                                                                                                                            |
| `attest`              | Sign what `build` uploaded, before anything is published: build provenance for all 7 archives and `sha256sum.txt`, and the SBOM against the archives. A failure here or in `sbom` publishes nothing (gate).                                                                                     |
| `publish-crate`       | `cargo publish` to crates.io via Trusted Publishing (OIDC, no static token after first publish).                                                                                                                                                                                                |
| `release`             | Create a **non-draft** GitHub Release with `make_latest: false`. Includes all 7 archives + `sha256sum.txt`.                                                                                                                                                                                     |
| `verify-attestations` | Download every published file and verify it against its attestation with `gh attestation verify --signer-workflow`. A failure withholds the Homebrew dispatch (gate).                                                                                                                           |
| `homebrew`            | Dispatch `update-formula` to `brettdavies/homebrew-tap` (formula name: `agentnative`, installs `anc`).                                                                                                                                                                                          |

The tap's formula installs this release's archives. Its `update-formula` workflow downloads the four it names (the two
`apple-darwin` and the two `linux-musl` archives), verifies each against the attestation `attest` made, and pins its
checksum; an archive with no attestation stops the bump, so `attest: true` in `release.yml` is what lets a release reach
Homebrew. The tap then builds bottles from those archives, signs them in its own `publish.yml`, and uploads them to this
repo's release assets.

After the homebrew-tap workflow uploads bottles to this repo's release assets, it dispatches `finalize-release` back to
this repo, which idempotently flips `make_latest: true`.

→ Rationale (`make_latest` flow, musl hard-block, annotated-tag gotcha):
[`RELEASES-RATIONALE.md` § Release pipeline](./RELEASES-RATIONALE.md#release-pipeline).

### After publish: sync `dev` with the release

Once `finalize-release.yml` has flipped the GitHub Release to `published`, bring the release bookkeeping (`Cargo.toml`
version, `Cargo.lock`, `CHANGELOG.md`) and every other release-only edit back to `dev` so the integration branch starts
from the released baseline and `anc audit`'s embedded badge URL points at the released slug:

```bash
scripts/sync-dev-after-release.sh v0.2.0 --dry-run   # preview: creates no branch, leaves the tree clean
scripts/sync-dev-after-release.sh v0.2.0
```

The script writes the released version into `Cargo.toml`, refreshes the workspace entries in `Cargo.lock` from the
synced manifests with `cargo update --workspace --offline`, and copies `CHANGELOG.md` verbatim from `origin/main`. Every
other path `main` and `dev` disagree about, guarded paths excepted, is classified against the previous release tag:

- **release-prep**: `dev`'s copy still matches the previous tag, so only the release changed it. Adopted.
- **contested**: both branches changed it since the previous tag. Listed and left out; `--include-contested` adopts
  every contested path.

`--only PATH` (repeatable) adopts exactly the discovered paths it names, release-prep or contested, and no other
discovered path; the version carriers and changelog are synced either way. Resolve anything left out by hand.

The offline lock refresh needs every crate in the local registry cache. When the lock does not resolve, the script stops
with exit 70 before committing and leaves `dev` as it found it; run `cargo fetch` and re-run. Without `cargo` on `PATH`
it stops with exit 69. Guarded paths never enter discovery, so the sync cannot remove `dev`-only content. After
committing, when the sync carried `CHANGELOG.md` and `git-cliff` is installed, the script runs
`scripts/generate-changelog.py --dry-run` and warns with the generator's own reason if the regenerated changelog would
differ (a PR body edited after the release, or a difference in line wrapping only). The warning does not block the
backport.

The script opens a PR against `dev`; merge it once CI is green. The postflight backport gate looks for that merged PR.
Never merge `main` into `dev` or push to `dev` directly: the squash-merged histories share no recent ancestry, so the
merge conflicts on every file both sides touched, and a direct push bypasses `dev`'s required checks.

The backport is idempotent: re-running on a `dev` already in sync exits 0 and leaves no branch or PR behind.

After it merges, confirm the branches actually converged. This is what catches a contested path nobody resolved:

```bash
git fetch origin
git diff --name-only origin/dev origin/main | grep -Ev "$(scripts/release/guarded-paths.sh)" || echo "(converged)"
```

→ Rationale: [`RELEASES-RATIONALE.md` § Release pipeline](./RELEASES-RATIONALE.md#release-pipeline).

## Rollback

A bad release is rolled back at the registry and formula surfaces first, then repaired in git. Rollback re-points what
users get; it does not revert history. After rolling back, land a `fix/*` or `revert` through the normal `dev` to
`release/*` to `main` flow so `main` matches what is live. Knowing the last-good identifier before the release goes out
is a [`RELEASES-POSTFLIGHT.md`](./RELEASES-POSTFLIGHT.md) gate.

The commands live under [Project specifics § Rollback commands](#rollback-commands).

→ Rationale: [`RELEASES-RATIONALE.md` § Rollback](./RELEASES-RATIONALE.md#rollback).

## Prose scrubbing

Three release-flow artifacts live outside any automated prose check and need a manual scrub before they ship:

- PR bodies (`gh pr create` / `gh pr edit` send body text directly to GitHub).
- `CHANGELOG.md` (a generated artifact built from upstream PR bodies).
- Release-PR bodies (composed after `CHANGELOG.md` has been generated).

The canonical Vale + LanguageTool rule packs and orchestrator behavior live in the spec repo at
[`~/dev/agentnative-spec/docs/architecture/voice-enforcement.md`](../agentnative-spec/docs/architecture/voice-enforcement.md).
This repo's `main` branch does not ship a local copy; point Vale at the spec checkout via `--config` (see the example
below).

```bash
# 1. Save the artifact to /tmp/.
gh pr view <num> --json body --jq .body > /tmp/body.md         # for PR body edits
# cp CHANGELOG.md /tmp/body.md                                 # for changelog scrub

# 2. Vale (against the spec's rule packs).
vale --no-global --config ~/dev/agentnative-spec/.vale.ini --output=line --minAlertLevel=error /tmp/body.md

# 3. LanguageTool grammar check via lt_check (~/dotfiles/config/shell/languagetool.sh).
#    Skips cleanly if LT is unreachable. Inspect: `lt_rules`, `lt_info`. See
#    ~/dev/agentnative-spec/CONTRIBUTING.md § Voice enforcement for the
#    install-vs-required nuance.
lt_check /tmp/body.md

# 4. unslop (em-dash density and AI-unique structural patterns).
~/.claude/skills/unslop/scripts/score.py /tmp/body.md

# 5. Apply fixes per finding. Re-run until 0 blocking and unslop score is 0.

# 6. Apply the cleaned version.
gh pr edit <num> --body-file /tmp/body.md     # for PR body edits
# ./scripts/generate-changelog.py             # for CHANGELOG.md (re-runs the PR-body fetch from GitHub)
```

For a `CHANGELOG.md` finding, fix the upstream PR body and regenerate. Hand-editing `CHANGELOG.md` directly produces
drift the next regeneration overwrites.

→ Rationale + which artifacts need this:
[`RELEASES-RATIONALE.md` § Prose scrubbing scope](./RELEASES-RATIONALE.md#prose-scrubbing-scope).

## Branch protection

Two rulesets are committed under `.github/rulesets/` and applied to the repo via the GitHub API:

- `protect-main.json` (required signatures, linear history, squash-only merges via PR, required status checks
  (`ci / Fmt, clippy, test`, `ci / Package check`, `ci / Security audit (bans licenses sources)`, `ci / Changelog`,
  `guard-docs / check-forbidden-docs`, `guard-provenance / check-provenance`,
  `guard-release / check-release-branch-name`), creation/deletion blocked, non-fast-forward blocked).
- `protect-dev.json` (required signatures, deletion blocked, non-fast-forward blocked). PR-only norm is convention +
  `guard-release-branch` on the main side.

### Applying changes

```bash
# First apply (creating a ruleset):
gh api -X POST repos/brettdavies/agentnative-cli/rulesets --input .github/rulesets/protect-dev.json

# Subsequent updates (replace by ID — find via `gh api repos/brettdavies/agentnative-cli/rulesets`):
gh api -X PUT repos/brettdavies/agentnative-cli/rulesets/<id> --input .github/rulesets/protect-main.json
```

→ Status-check context strings (inline vs reusable):
[`RELEASES-RATIONALE.md` § Status-check context strings](./RELEASES-RATIONALE.md#status-check-context-strings).

## Project specifics

Operational ship-channel details for `agentnative`: version carriers, secrets, channels, targets, bootstrap, rollback.
Not rationale, not pre-cut checks.

### Version carriers

`cut-release-branch.sh` leaves the version carriers alone and writes only `CHANGELOG.md`. On the staged branch, before
the commit, in this order:

```bash
# 1. Bump the crate to the version the cut was given, and refresh its lockfile entry.
sed -i 's/^version = ".*"/version = "0.2.0"/' Cargo.toml
cargo update -p agentnative

# 2. Regenerate the completions, which catches any subcommand or flag change missed
#    during dev.
./scripts/generate-completions.sh

# 3. Refresh the skill.json fixture from upstream and review the diff.
bash scripts/sync-skill-fixture.sh && git diff src/skill_install/skill.json
```

A difference from steps 2 or 3 is content `dev` did not carry; preflight's diff-B lists it for review, and the backport
brings it to `dev` after the release. `scripts/release/preflight.sh mechanics` checks the `Cargo.toml` version against
`anc --version` and the top section of `CHANGELOG.md`.

### Required secrets

| Secret                 | Purpose                                                                                                           | Lifecycle                                      |
| ---------------------- | ----------------------------------------------------------------------------------------------------------------- | ---------------------------------------------- |
| `CI_RELEASE_TOKEN`     | Fine-grained PAT, Contents R+W, Pull requests R+W. Used by `release.yml` to dispatch the Homebrew formula update. | Rotated annually.                              |
| `CARGO_REGISTRY_TOKEN` | crates.io API token. Required only for the first publish.                                                         | Removed after Trusted Publishing was enforced. |

`GITHUB_TOKEN` is automatic; CI (`ci.yml`) needs `contents: read` and `pull-requests: read`, the latter for the
changelog check that reads a PR's files, and uses no extra secrets.

### Distribution channels

| Channel        | Identifier                              | Populated by                                            |
| -------------- | --------------------------------------- | ------------------------------------------------------- |
| crates.io      | `agentnative` (installs `anc`)          | `release.yml` `publish-crate` (OIDC Trusted Publishing) |
| Homebrew       | `brettdavies/tap/agentnative`           | `brettdavies/homebrew-tap` `update-formula` dispatch    |
| GitHub Release | `v<version>` assets + `sha256sum.txt`   | `release.yml` `release`, then `finalize-release.yml`    |
| cargo-binstall | resolves from the GitHub Release assets | `[package.metadata.binstall]` in `Cargo.toml`           |

### Cross-compile target matrix

Seven targets, listed in the `build` row of [§ Tagging and publishing](#tagging-and-publishing). The two musl rows are
hard-blocking (`linux_musl_required: true`) and the x86_64-musl binary is exec-verified inside `alpine:latest`
(`linux_musl_verify_alpine: true`). `release-matrix-check.yml` builds the same seven rows on every push to a `release/*`
branch, and on a PR that changes `Cargo.toml`, `Cargo.lock`, or `rust-toolchain.toml`, so a broken row surfaces before
the tag.

Four of the archives are also what Homebrew installs. `Formula/agentnative.rb` in `brettdavies/homebrew-tap` names
`agentnative-aarch64-apple-darwin.tar.gz`, `agentnative-x86_64-apple-darwin.tar.gz`,
`agentnative-aarch64-unknown-linux-musl.tar.gz`, and `agentnative-x86_64-unknown-linux-musl.tar.gz`, and installs the
`anc` at the top of each archive's single directory. The musl builds are the Linux ones because they are static and run
against any glibc, Homebrew's included. An archive name, the place of `anc` inside it, and those four targets are
therefore a contract with the formula: change one and the tap's bump for the next release fails, so the formula changes
in the same step.

### First-time publish (one-time)

The initial crate publish requires a regular crates.io API token (Trusted Publishing needs the crate to exist first).
`agentnative` completed this step; its first crates.io version is `0.1.0-alpha.1`. The steps it ran:

1. Verify your email on crates.io (`https://crates.io/settings/profile`).
2. `cargo publish` locally with `CARGO_REGISTRY_TOKEN` set.
3. Configure Trusted Publishing on crates.io: `https://crates.io/settings/tokens/trusted-publishing` → add
   `brettdavies/agentnative-cli`, workflow `release.yml`.
4. Enable "Enforce Trusted Publishing" to block token-based publishes.
5. Remove the `CARGO_REGISTRY_TOKEN` repository secret.

Subsequent releases use the OIDC flow built into `release.yml`: no static token in CI.

### Rollback commands

Record the previous tag before pushing the new one; every command below needs it.

```bash
PREV=v0.1.0      # last-good tag
BAD=v0.2.0       # the release being rolled back

# crates.io: yank the bad version. Existing lockfiles keep resolving it; new resolutions
# (cargo install, cargo binstall, which follows the crates.io index) skip it.
cargo yank --version "${BAD#v}" agentnative

# GitHub Release: re-point /releases/latest at the last-good tag.
gh release edit "$PREV" --latest
gh release edit "$BAD" --prerelease

# Homebrew: revert the formula bump on the tap so `brew install` resolves the last-good bottle.
gh api repos/brettdavies/homebrew-tap/commits --jq '.[0:5][] | .sha[0:7] + " " + .commit.message'
# then revert the `agentnative: add <version> bottle.` and formula-bump commits via a PR to the tap's main.
```

Un-yank with `cargo yank --undo --version <version> agentnative` if the yank was wrong. A yanked crate version cannot be
re-published; the fix ships as the next patch version through the normal flow.

## Related docs

- [`RELEASES-PREFLIGHT.md`](./RELEASES-PREFLIGHT.md) (pre-cut go/no-go checklist gating release-branch creation)
- [`RELEASES-POSTFLIGHT.md`](./RELEASES-POSTFLIGHT.md) (post-tag verification of the publish pipeline)
- [`RELEASES-RATIONALE.md`](./RELEASES-RATIONALE.md) (release flow rationale, CHANGELOG pipeline, branch-protection
  pitfalls)
- [`.github/pull_request_template.md`](.github/pull_request_template.md) (PR body structure with changelog sections)
- [`AGENTS.md`](AGENTS.md) (running `anc`, project structure, adding new audits)
- [`README.md`](README.md) (install channels, principles, CLI reference)
