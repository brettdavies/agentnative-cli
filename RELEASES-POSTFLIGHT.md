<!-- Canonical version: 2026.11.03 (generic) -->
# Post-release verification: `agentnative`

Operational post-flight checklist. Runs **after** the `release/v<version> → main` PR merges and you push the tag
(`git push origin vX.Y.Z`) per [`RELEASES.md` § Tagging and publishing](./RELEASES.md#tagging-and-publishing). Verifies
that the tag-triggered pipeline landed cleanly across `release.yml` → `homebrew-tap` → `finalize-release.yml`, and that
the published artifacts resolve on crates.io, the Homebrew tap, and the GitHub Release.

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

| Sub-command   | What it checks                                                                                                                             | Source of truth                           |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------- |
| `release`     | the `release.yml` run titled `Release vX.Y.Z`, which the tag's push dispatches onto `main`, concluded `"success"`                          | `gh run view`                             |
| `tap`         | `brettdavies/homebrew-tap`'s `update-formula` run titled `Update agentnative to vX.Y.Z` ran SUCCESS; it lands the bump on the tap's `main` | `gh run list -R brettdavies/homebrew-tap` |
| `finalize`    | `finalize-release.yml` callback ran in this repo (cross-repo dispatch loop closed)                                                         | `gh run list -e repository_dispatch`      |
| `make-latest` | GitHub Release `vX.Y.Z` is non-draft, non-prerelease, and `releases/latest` resolves to it                                                 | `gh api /releases/latest`                 |
| `crates`      | `crates.io` shows `agentnative vX.Y.Z` published                                                                                           | `crates.io` index API                     |
| `tags`        | Each workspace library's tag sits on the binary tag's commit; SKIPs for anc, which publishes no library                                    | `git rev-parse`                           |
| `backport`    | the subject `chore(release): sync dev after vX.Y.Z` on `dev`'s first-parent history or a merged PR's title                                 | `git log`, `gh pr list --base dev`        |
| `all`         | every above                                                                                                                                | all of the above                          |

Flags:

- `--repo OWNER/REPO`: override the auto-detected nameWithOwner
- `--tap-repo OWNER/REPO`: override the homebrew-tap repo (default: `brettdavies/homebrew-tap`)
- `--tag vX.Y.Z`: override auto-detection (default: `v` plus the `Cargo.toml` version, falling back to the newest
  `v[0-9]*` tag)
- `--crate NAME`: override the crate name for the `crates` gate (default: `Cargo.toml` `[package].name`)

## Checklist

Run once the tag is pushed.

- [ ] **`release.yml` green end-to-end.** The run is the one titled `Release vX.Y.Z`, which `gh run list --workflow
  release.yml --event workflow_dispatch --limit 3` lists. `gh run watch <id> --exit-status` then verify with
  `gh run view <id> --json conclusion --jq .conclusion` because the watcher exit code alone is not authoritative (a
  completed watcher is not a green watcher). Builds the seven cross-compile targets, publishes to crates.io via OIDC
  Trusted Publishing, and dispatches `update-formula` into the homebrew-tap. Run `scripts/release/postflight.sh release`
  for the automated check.
- [ ] **Homebrew-tap dispatch landed.** `gh run list -R brettdavies/homebrew-tap --limit 5` should show a recent
  `Update agentnative to v<version>` run SUCCESS. Its `land` job pushes the formula bump to tap `main` as one
  `chore(agentnative): bump to v<version>` commit, with no bottle:
  the formula installs this release's archives. Run `scripts/release/postflight.sh tap` for the automated check.
- [ ] **`finalize-release.yml` callback ran.** After the formula lands, the tap dispatches back to this repo and the
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
- [ ] **`brew update && brew install brettdavies/tap/agentnative`** on a fresh prefix downloads this release's archive
  for the platform and `anc --version` reports the new tag. Drive on a throwaway prefix
  (`HOMEBREW_PREFIX=/tmp/brew-postflight-X brew ...`). Confirms the homebrew-tap end of the cross-repo dispatch chain
  landed cleanly and the archive matches the checksum the formula pins. The install compiles nothing and pours no
  bottle; the tap verified each archive against this release's attestation before it pinned the checksum.
- [ ] **`cargo binstall agentnative`** (without `--version`) resolves to the new tag and installs the matching prebuilt
  binary. Confirms the GitHub Release asset layout (binary + completions + licenses, expected archive naming) matches
  binstall's asset-resolution rules and the `[package.metadata.binstall]` overrides in `Cargo.toml`. Drive on a clean
  container.
- [ ] **A release archive verifies against its attestation.** `release.yml` verifies every published file in its
  `verify-attestations` job and withholds the Homebrew dispatch when one fails; this is the same check from outside CI.
  Download one archive of the release and run the command below. The reusable workflow is the signer, so `--repo` alone
  fails; the `--signer-workflow` flag names it. Expect `Verification succeeded`. A failure with a green `release.yml`
  means the attestation jobs were skipped: check that `release.yml` still passes `attest: true` and grants
  `attestations: write`.

  ```bash
  gh release download v<X.Y.Z> --repo brettdavies/agentnative-cli --pattern 'agentnative-x86_64-unknown-linux-gnu.tar.gz'
  gh attestation verify agentnative-x86_64-unknown-linux-gnu.tar.gz --repo brettdavies/agentnative-cli \
    --signer-workflow brettdavies/.github/.github/workflows/rust-release.yml
  ```

- [ ] **Live site renders the new scorecard.** Click the `badge_url` and `scorecard_url` from a real emitted scorecard
  against the live site. First-time renders for a new spec version can 404 even when the JSON looks correct.
- [ ] **Last-good identifier recorded.** Before the release goes live, note the previous tag
  (`git tag --sort=-version:refname | sed -n 2p`), the previous crates.io version, and the tap's formula commit for it
  somewhere reachable under incident pressure, so a rollback is a single command. Commands live in
  [`RELEASES.md` § Rollback commands](./RELEASES.md#rollback-commands).
- [ ] **Rollback path confirmed.** If this release is bad, roll back at the surface first (`cargo yank`,
  `gh release edit --latest`, formula revert on the tap), then land a `fix` or `revert` through the normal `dev` to
  `release/*` to `main` flow so `main` reconverges with what is live.
- [ ] **Backport `main` → `dev`** via the PR `scripts/sync-dev-after-release.sh` opens, merged to `dev`.
  `scripts/sync-dev-after-release.sh vX.Y.Z` cuts `chore/sync-dev-after-vX.Y.Z`, writes the released version into
  `Cargo.toml`, refreshes the workspace entries in `Cargo.lock` offline, copies `CHANGELOG.md` from `main`, and opens
  the PR. Other release-only edits (generator config, README polish, `RELEASES.md` meta-edits) are discovered, not added
  by hand: a path `main` changed since the previous release tag while `dev` left it alone is release-prep and adopted. A
  path both branches changed since that tag is contested: the script lists it and leaves it out. `--include-contested`
  adopts every contested path, and `--only PATH` (repeatable) adopts exactly the discovered paths it names. Preview the
  set with `--dry-run`, which creates no branch and leaves the tree clean. Keeps the next release's PREFLIGHT `diff-B`
  step quiet so a real missed change stands out instead of hiding in expected divergence noise.

  The gate (`scripts/release/postflight.sh backport`) is signal-agnostic about which files moved: it looks for the
  subject the script gives the backport, `chore(release): sync dev after <tag>` (its PR title and squash commit) or
  `chore(release): backport <tag> artifacts to dev` (its commit), first on `dev`'s first-parent history, so a backport
  committed without a PR counts, then among merged PRs to `dev`. A feature PR that only mentions the version does not
  count. A release that withholds `anc web` takes the hand backport in
  [`RELEASES.md` § After publish](./RELEASES.md#after-publish-sync-dev-with-the-release) under the same title, so the
  gate finds it the same way. Its SKIP and FAIL name the sync command to run.

## Related docs

- [`RELEASES-PREFLIGHT.md`](./RELEASES-PREFLIGHT.md): pre-cut go/no-go checklist (runs BEFORE this one).
- [`RELEASES.md`](./RELEASES.md): operational runbook for the full release lifecycle.
- [`RELEASES-RATIONALE.md`](./RELEASES-RATIONALE.md): release-flow rationale.
