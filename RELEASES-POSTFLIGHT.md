# Post-release verification: `agentnative`

Operational post-flight checklist. Runs **after** the `release/v<version> → main` PR merges and you push the tag (`git
push origin vX.Y.Z`) per [`RELEASES.md` § Tagging and publishing](./RELEASES.md#tagging-and-publishing). Verifies that
the tag-triggered pipeline landed cleanly across `release.yml` → `homebrew-tap` → `finalize-release.yml`, and that the
published artifacts resolve on crates.io, the Homebrew tap, and the GitHub Release.

Companion to [`RELEASES-PREFLIGHT.md`](./RELEASES-PREFLIGHT.md), which gates the release-branch cut. Both docs follow
the same go/no-go shape: every box is explicit, an unchecked or red item holds the next release (or motivates a hotfix).

## Quick start: run the automated gates

```bash
scripts/release/postflight.sh all
```

The script (`scripts/release/postflight.sh`) covers the automatable post-tag gates: `release.yml` end-to-end,
homebrew-tap dispatch, `finalize-release.yml` callback, GitHub Release `make_latest` flip, crates.io publish
verification, and the `main → dev` backport check. Install-on-fresh-machine smokes (`cargo install`, `brew install`,
`cargo binstall`) are documented but not driven from the script. Running them on the local dev machine pollutes its
toolchain and doesn't actually exercise the fresh-machine semantics. Drive those on a throwaway container or a sibling
machine.

`anc` is a single-env CLI, so `--env staging|prod` is irrelevant here: every gate behaves identically and the default
(`prod`) is correct. The `surface-smoke` gate SKIPs because `scripts/release/surface-smoke.sh` is not vendored (no
deployed HTTP surface).

Sub-commands let you re-run one verification in isolation:

| Sub-command   | What it checks                                                                                               | Source of truth                           |
| ------------- | ------------------------------------------------------------------------------------------------------------ | ----------------------------------------- |
| `release`     | `release.yml` on the tag push: `gh run view ... --json conclusion` is `"success"`                            | `gh run view`                             |
| `tap`         | `brettdavies/homebrew-tap` `update-formula` (repository_dispatch) + `Publish bottles` (workflow_run) SUCCESS | `gh run list -R brettdavies/homebrew-tap` |
| `finalize`    | `finalize-release.yml` callback ran in this repo (cross-repo dispatch loop closed)                           | `gh run list -e repository_dispatch`      |
| `make-latest` | GitHub Release `vX.Y.Z` is non-draft, non-prerelease, and `releases/latest` resolves to it                   | `gh api /releases/latest`                 |
| `crates`      | `crates.io` shows `agentnative vX.Y.Z` published                                                             | `crates.io` index API                     |
| `backport`    | a merged PR to `dev` with the version in its title                                                           | `gh pr list --base dev --state merged`    |
| `all`         | every above                                                                                                  | all of the above                          |

Flags:

- `--repo OWNER/REPO`: override the auto-detected nameWithOwner
- `--tap-repo OWNER/REPO`: override the homebrew-tap repo (default: `brettdavies/homebrew-tap`)
- `--tag vX.Y.Z`: override auto-detection (default: derived from `Cargo.toml` version, falls back to latest git tag)
- `--crate NAME`: override the crate name for the `crates` gate (default: `Cargo.toml` `[package].name`)

## Checklist

Run immediately after the tag push triggers `release.yml`.

- [ ] **`release.yml` green end-to-end.** `gh run watch <id> --exit-status` then verify with `gh run view <id> --json
  conclusion --jq .conclusion` because the watcher exit code alone is not authoritative (a completed watcher is not a
  green watcher). Builds the seven cross-compile targets, publishes to crates.io via OIDC Trusted Publishing, and
  dispatches `update-formula` into the homebrew-tap. Run `scripts/release/postflight.sh release` for the automated
  check.
- [ ] **Homebrew-tap dispatch landed.** `gh run list -R brettdavies/homebrew-tap --limit 5` should show a recent
  `update-formula` (event=repository_dispatch) and a `Publish bottles` (event=workflow_run) both SUCCESS. The bottles
  workflow auto-merges the formula bump PR and pushes an `agentnative: add <version> bottle.` commit to tap `main`. Run
  `scripts/release/postflight.sh tap` for the automated check.
- [ ] **`finalize-release.yml` callback ran.** After the bottles publish, the tap dispatches back to this repo and the
  callback flips the GitHub Release `make_latest: true`. Check `gh run list -e repository_dispatch --limit 3`; expect a
  `finalize-release` SUCCESS. Run `scripts/release/postflight.sh finalize` for the automated check.
- [ ] **GitHub Release marked latest.** `gh api repos/brettdavies/agentnative-cli/releases/latest --jq .tag_name`
  returns `vX.Y.Z`, not the previous tag. Confirms `finalize-release.yml` actually flipped the flag. Run
  `scripts/release/postflight.sh make-latest` for the automated check.
- [ ] **crates.io shows the new version published.** `cargo search agentnative` lists the new version. Run
  `scripts/release/postflight.sh crates` for the automated index check.
- [ ] **`cargo install agentnative --version <new>` on a clean environment** resolves and runs. Drive on a fresh
  container or a sibling machine so the local `~/.cargo/bin` isn't polluted. Confirms the publish landed all package
  data and the installer can reconstruct `anc` from source.
- [ ] **`brew update && brew install brettdavies/tap/agentnative`** on a fresh prefix resolves the new bottle and `anc
  --version` reports the new tag. Drive on a throwaway prefix (`HOMEBREW_PREFIX=/tmp/brew-postflight-X brew ...`).
  Confirms the homebrew-tap end of the cross-repo dispatch chain landed cleanly and the published bottle SHA matches the
  formula.
- [ ] **`cargo binstall agentnative`** (without `--version`) resolves to the new tag and installs the matching prebuilt
  binary. Confirms the GitHub Release asset layout (binary + completions + licenses, expected archive naming) matches
  binstall's asset-resolution rules and the `[package.metadata.binstall]` overrides in `Cargo.toml`. Drive on a clean
  container.
- [ ] **Live site renders the new scorecard.** Click the `badge_url` and `scorecard_url` from a real emitted scorecard
  against the live site. First-time renders for a new spec version can 404 even when the JSON looks correct.
- [ ] **Last-good identifier recorded.** Before the release goes live, note the previous tag (`git tag
  --sort=-version:refname | sed -n 2p`), the previous crates.io version, and the tap's formula commit for it somewhere
  reachable under incident pressure, so a rollback is a single command. Commands live in
  [`RELEASES.md` § Rollback commands](./RELEASES.md#rollback-commands).
- [ ] **Rollback path confirmed.** If this release is bad, roll back at the surface first (`cargo yank`, `gh release
  edit --latest`, formula revert on the tap), then land a `fix` or `revert` through the normal `dev` to `release/*` to
  `main` flow so `main` reconverges with what is live.
- [ ] **Backport `main` → `dev`** via a **merged PR to `dev` with the version in its title.**
  `scripts/sync-dev-after-release.sh vX.Y.Z` cuts `chore/sync-dev-after-vX.Y.Z`, writes the released version into
  `Cargo.toml` and `Cargo.lock`, copies `CHANGELOG.md` from `main`, and opens the PR. Add any other release-only edits
  (generator config, README polish, `RELEASES.md` meta-edits) to that branch before merging. Keeps the next release's
  PREFLIGHT `diff-B` step quiet so a real missed change stands out instead of hiding in expected divergence noise.

  The gate (`scripts/release/postflight.sh backport`) is signal-agnostic about which files moved: it looks for the
  merged PR alone, since "which files" varies release-to-release. The only requirement is the version string in the PR
  title.

## Related docs

- [`RELEASES-PREFLIGHT.md`](./RELEASES-PREFLIGHT.md): pre-cut go/no-go checklist (runs BEFORE this one).
- [`RELEASES.md`](./RELEASES.md): operational runbook for the full release lifecycle.
- [`RELEASES-RATIONALE.md`](./RELEASES-RATIONALE.md): release-flow rationale.
