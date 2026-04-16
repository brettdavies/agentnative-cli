# Releasing `agentnative`

Every change reaches production via this pipeline. Direct commits to `dev` or `main` are not permitted — every change
has a PR number in its squash commit message, which keeps the history scannable, attributable, and changelog-ready.

```text
feature branch → PR to dev (squash merge)
              → cherry-pick to release/* branch
              → PR to main (squash merge)
              → tag push triggers crates.io publish + GitHub Release + Homebrew dispatch
```

## Branches

| Branch | Role | Lifetime | Protection |
| ------ | ---- | -------- | ---------- |
| `main` | Production. Only release commits. | Forever. | `.github/rulesets/protect-main.json` |
| `dev` | Integration. All feature PRs land here. | Forever. Never delete. | `.github/rulesets/protect-dev.json` |
| `feat/*`, `fix/*`, `chore/*`, `docs/*` | Feature work. | One PR's worth. Auto-deleted on merge. | None — squash into dev freely. |
| `release/*` | Head of a dev → main PR. | One release's worth. Auto-deleted on merge. | None. |

`dev` is a **forever branch**. Never delete it locally or remotely, even after a `release/* → main` merge. The next
release cycle reuses the same `dev`. The repo's `deleteBranchOnMerge: true` setting doesn't touch `dev` as long as `dev`
is never the head of a PR — using a short-lived `release/*` head is what keeps the setting compatible with a forever
integration branch.

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
- **PR body**: follow `.github/pull_request_template.md`. The `## Changelog` section is the source of truth for
  user-facing release notes — `git-cliff` extracts these bullets verbatim into `CHANGELOG.md` during release prep.

## Releasing dev to main

Engineering docs (`docs/plans/`, `docs/solutions/`, `docs/brainstorms/`,
`docs/reviews/`) live on `dev` only. `guard-main-docs.yml` blocks them from reaching `main`, and
`guard-release-branch.yml` rejects any PR to main whose head isn't `release/*`. Use the release-branch cherry-pick
pattern:

**Branch naming**: `release/v<version>` or `release/v<version>-<slug>` (e.g. `release/v0.1.0`,
`release/v0.2.0-python-checks`). The `v<version>` prefix is required — `scripts/generate-changelog.sh` extracts the
version from the branch name.

```bash
# 1. Branch from main, NOT dev. Branching from dev causes add/add conflicts
#    when dev and main have divergent histories (the post-squash-merge norm).
git fetch origin
git checkout -b release/v0.2.0 origin/main

# 2. List the dev commits not yet on main:
git log --oneline dev --not origin/main

# 3. Cherry-pick the ones you want to ship. Docs commits stay on dev.
git cherry-pick <sha1> <sha2> ...

# 4. Verify no guarded paths leaked through:
git diff origin/main --stat
# If anything under docs/plans/, docs/solutions/, or docs/brainstorms/
# shows up, you cherry-picked a docs commit by mistake — reset and redo.

# 5. Bump version in Cargo.toml and commit:
#    sed -i 's/^version = ".*"/version = "0.2.0"/' Cargo.toml
#    cargo update -p agentnative   # refresh Cargo.lock
#    git add Cargo.toml Cargo.lock && git commit -m "chore: bump version to 0.2.0"

# 6. Regenerate completions (catches any subcommand/flag changes missed during dev):
./scripts/generate-completions.sh
git add completions/ && git commit -m "chore: regenerate shell completions" || true

# 7. Generate CHANGELOG.md (auto-detects version from branch name; CI enforces this):
./scripts/generate-changelog.sh
git add CHANGELOG.md && git commit -m "docs: update CHANGELOG.md for v0.2.0"

# 8. Push and open the PR:
git push -u origin release/v0.2.0
gh pr create --base main --head release/v0.2.0 --title "release: v0.2.0"
```

When the PR merges, the deploy / publish workflow picks up the push to `main`. Auto-delete removes `release/v0.2.0` from
the remote on merge. `dev` is untouched.

### Why branch from main, not dev

Branching from `dev` and then `gio trash`-ing the guarded paths seems simpler but produces `add/add` merge conflicts
whenever `dev` and `main` have diverged (which they always do after the first squash merge). The file appears as "added"
on both sides with different content. Always branch from `origin/main` and cherry-pick onto it.

## Tagging and publishing

After the `release/v<version> → main` PR merges, tag and push:

```bash
git checkout main && git pull
git tag -a -m "Release v0.2.0" v0.2.0
git push origin main --tags
```

> Always use annotated tags (`-a -m`). Bare `git tag <name>` silently fails with
> `fatal: no tag message?` on machines where `tag.gpgsign=true` is set globally
> (a brettdavies dotfile default). See
> [solutions: git tag fails with tag.gpgsign — use annotated tags](https://github.com/brettdavies/solutions-docs/blob/main/best-practices/git-tag-fails-with-tag-gpgsign-use-annotated-tags-2026-04-13.md).

The tag push triggers `.github/workflows/release.yml`, which calls the reusable
`brettdavies/.github/.github/workflows/rust-release.yml@main` and runs:

| Step | What |
| ---- | ---- |
| `check-version` | Verify the tag matches `Cargo.toml` version (gate). |
| `audit` | `cargo deny check` (license + advisory + ban). |
| `build` | Cross-compile binaries for 5 targets: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`. Each archive includes the `anc` binary, completions, README, and licenses. |
| `publish-crate` | `cargo publish` to crates.io via Trusted Publishing (OIDC, no static token after first publish). |
| `release` | Create a **non-draft** GitHub Release with `make_latest: false` — visible immediately (so `cargo-binstall` and `/releases/latest` don't 404 during the bottle-build window) but not yet promoted to "Latest". Includes all 5 archives + `sha256sum.txt`. |
| `homebrew` | Dispatch `update-formula` to `brettdavies/homebrew-tap` (formula name: `agentnative`, installs `anc`). |

After the homebrew-tap workflow uploads bottles to this repo's release assets, it dispatches `finalize-release` back to
this repo, which idempotently flips `make_latest: true`. End result: crate on crates.io, GitHub Release marked latest,
Homebrew formula updated with bottles, all atomically advertised.

### First-time publish (one-time)

The very first crate publish requires a regular crates.io API token (Trusted Publishing needs the crate to exist first).
Steps for `v0.1.0`:

1. Verify your email on crates.io (`https://crates.io/settings/profile`).
2. `cargo publish` locally with `CARGO_REGISTRY_TOKEN` set.
3. Configure Trusted Publishing on crates.io: `https://crates.io/settings/tokens/trusted-publishing` → add
   `brettdavies/agentnative`, workflow `release.yml`.
4. Enable "Enforce Trusted Publishing" to block token-based publishes.
5. Remove the `CARGO_REGISTRY_TOKEN` repository secret.

Subsequent releases use the OIDC flow built into `release.yml` — no static token in CI.

## PRs and changelog generation

Every PR **must** follow `.github/pull_request_template.md`. The template has a `## Changelog` section with these
subsections:

- `### Added` — new user-visible features or capabilities
- `### Changed` — changes to existing behavior
- `### Fixed` — bug fixes
- `### Removed` — removed features or APIs
- `### Security` — security-relevant changes

`scripts/generate-changelog.sh` (which wraps `git-cliff` per `cliff.toml`) reads the squash-merged commit bodies for
these sections and assembles `CHANGELOG.md` entries. A PR that lands with an empty or missing `## Changelog` section
silently drops its user-facing notes from the next release changelog.

## Branch protection

Two rulesets are committed under `.github/rulesets/` and applied to the repo via the GitHub API:

- `protect-main.json` — required signatures, linear history, squash-only merges via PR, required status checks (`ci /
  Fmt, clippy, test`, `ci / Package check`, `ci / Security audit (bans licenses sources)`, `ci / Changelog`, `guard-docs
  / check-forbidden-docs`, `guard-provenance / check-provenance`, `guard-release / check-release-branch-name`),
  creation/deletion blocked, non-fast-forward blocked.
- `protect-dev.json` — required signatures, deletion blocked, non-fast-forward blocked. No PR-requirement at the ruleset
  level; the PR-only norm is enforced by convention + `guard-release-branch` on the main side.

### Applying changes

Edit the JSON locally, then sync to the remote:

```bash
# First apply (creating a ruleset):
gh api -X POST repos/brettdavies/agentnative/rulesets --input .github/rulesets/protect-dev.json

# Subsequent updates (replace by ID — find via `gh api repos/brettdavies/agentnative/rulesets`):
gh api -X PUT repos/brettdavies/agentnative/rulesets/<id> --input .github/rulesets/protect-main.json
```

Committing the JSON alongside code means ruleset changes land via the same review process as workflow changes — a
`chore(ci): tighten protect-main` change goes through dev → release/* → main like anything else.

### Status-check context pitfall

The `required_status_checks[].context` strings in `protect-main.json` must match exactly what GitHub publishes for each
check:

- **Inline job** (with `name:` field): published as just `<job-name>` (no workflow-name prefix).
- **Reusable-workflow caller** (`uses: .../foo.yml@ref`): published as `<caller-job-id> / <reusable-job-id-or-name>`.

Mixing these produces a stuck-but-green PR: all actual checks report green, but the ruleset waits forever on a context
that will never appear. Confirm the real contexts after a first CI run with:

```bash
gh api repos/brettdavies/agentnative/commits/<sha>/check-runs --jq '.check_runs[].name'
```

## Required secrets

| Secret | Purpose | Lifecycle |
| ------ | ------- | --------- |
| `CI_RELEASE_TOKEN` | Fine-grained PAT, Contents R+W, Pull requests R+W. Used by `release.yml` to dispatch the Homebrew formula update. | Rotated annually. |
| `CARGO_REGISTRY_TOKEN` | crates.io API token. Required only for the first publish. | Remove after Trusted Publishing is configured. |

`GITHUB_TOKEN` is automatic; CI (`ci.yml`) only needs `contents: read` and uses no extra secrets.

## Related docs

- [`.github/pull_request_template.md`](.github/pull_request_template.md) — PR body structure with changelog sections
- [`AGENTS.md`](AGENTS.md) — running `anc`, project structure, adding new checks
- [`README.md`](README.md) — install channels, principles, CLI reference
