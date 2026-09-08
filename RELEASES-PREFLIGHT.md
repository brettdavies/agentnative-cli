# Pre-release verification: `agentnative`

Operational pre-flight checklist. Runs **before** step 1 of
[`RELEASES.md` § Releasing dev to main](./RELEASES.md#releasing-dev-to-main). Gates the cut of the `release/v<version>`
branch, not the daily dev integration. Each box is an explicit go/no-go. If any item is unchecked or red, hold the
release.

CI (fmt, clippy, test, cargo-deny, skill-fixture-drift, Windows-compat) catches mechanical regressions inside this repo.
This checklist covers what CI structurally can't:

- Breaking changes to the scorecard JSON that downstream consumers must adapt to.
- Real-world behavior against external CLIs (CI only dogfoods `anc` against itself).
- Distribution paths that only exercise on real artifacts (cross-compile binaries, `git clone` to a real skill-bundle
  destination, `cargo install` from a clean machine).
- Cross-repo sequencing where releasing here before `agentnative-site` / `agentnative-spec` is ready breaks downstreams.

Post-tag verification (`release.yml` → homebrew-tap → `finalize-release.yml` → crates.io) lives in
[`RELEASES-POSTFLIGHT.md`](./RELEASES-POSTFLIGHT.md). The tag push happens AFTER the release-branch cut and the
PR-to-main merge, so verification of the tag-triggered pipeline is post-flight, not pre-flight.

## Quick start: run the automated gates

Most of this checklist can run from one script. Build the release binary first (`cargo build --release`), then:

```bash
scripts/release/preflight.sh all
```

The preflight script (`scripts/release/preflight.sh`) is **project-authored**: the shared scaffolding (gate helpers,
1Password reads, `shred -u` tempdir cleanup, subcommand dispatch, drift + surface + mechanics gates) is vendored from the
`github-repo-setup` skill's skeleton, and the `gate_smoke` body is this repo's to fill in with the multi-target audit
runs below. `all` runs the drift gate first, since nothing else matters while `main` holds changes `dev` never received.
The recipes in the sections below document what each gate verifies and serve as the manual fallback when running by
hand.

Sub-commands let you re-run one section in isolation:

| Sub-command | What it checks                                                                            | Source of truth                                                |
| ----------- | ----------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| `drift`     | Commits on `main` that `dev` lacks, `.github/` parity, lockfile packages `main` has newer | `scripts/release/drift.sh`                                     |
| `surface`   | Commits + diff vs last tag, breaking markers                                              | `git log`, `git diff`                                          |
| `smoke`     | Project-authored multi-target audit runs (`gate_smoke` body)                              | project body                                                   |
| `mechanics` | Version, lockfile, toolchain age, advisories, leak check, unguarded docs, diff-B          | `Cargo.toml`, `CHANGELOG.md`, `cargo deny`, `guarded-paths.sh` |
| `all`       | Every above sequentially, drift first                                                     |                                                                |

Flags:

- `--smoke-home PATH`: reuse an existing seeded `$SMOKE_HOME` instead of creating + seeding
- `--no-cleanup`: keep `$SMOKE_HOME` after exit (default: shred on exit)
- `--tag TAG`: override `LAST_TAG` resolution (default: `git tag --sort=-version:refname | head -n 1`)

`anc` is a single-binary CLI with no deployed HTTP surface, so `scripts/release/surface-smoke.sh` is not vendored and
the `surface-smoke` sub-command SKIPs.

After `git push origin vX.Y.Z` triggers the release pipeline, run
[`scripts/release/postflight.sh all`](./RELEASES-POSTFLIGHT.md) to verify the downstream chain.

## Establish the surface

Everything below assumes you know what's changing. Run this first.

Driven by `scripts/release/preflight.sh surface`.

```bash
LAST_TAG=$(git tag --sort=-version:refname | head -n 1)
git log "$LAST_TAG..dev" --oneline                              # commits going out
git diff "$LAST_TAG..dev" --stat                                # file-level scope
git diff "$LAST_TAG..dev" -- src/scorecard/                     # JSON-shape surface
git log "$LAST_TAG..dev" --grep '^[a-z]\+\(([^)]*)\)\?!:' --oneline   # Conventional-Commits breaking markers, scoped or not
```

On a repo with no tags yet, or whose lineage is squash-only so no tag is an ancestor of `dev`, the surface is
`origin/main..origin/dev` instead of `$LAST_TAG..dev`; `preflight.sh surface` SKIPs the tag counts in that case.

Every `!:` commit drives the major-version decision and gets a row in the release's `### Breaking changes` section.

## Checklist

### Branch drift (main ahead of dev)

Driven by `scripts/release/preflight.sh drift` (delegates to `scripts/release/drift.sh`).

Security PRs, hotfixes, and config edits land on `main` first. The release branch is cut from `main` and then takes
`dev`'s changes, so anything `main` holds that `dev` never received is reverted by the release or collides with it, and
Dependabot raises the same fix again.

- [ ] The previous release's bookkeeping (`Cargo.toml`, `Cargo.lock`, `CHANGELOG.md`) reached `dev` (gate 0 fails when
      it never did; run `scripts/sync-dev-after-release.sh v<version>` and merge its PR first).
- [ ] Every commit on `main` since the last release has its changes on `dev` (gate 1 lists the ones that do not, as
      `differs` or `missing`). Backport them by PR into `dev` first, merge, and rerun.
- [ ] `.github/` is identical on both branches (gate 2). A difference either way is a config change that only reached
      one branch.
- [ ] No `Cargo.lock` package resolves newer on `main` than on `dev` (gate 3). The one benign case is a version still
      inside a release-age window when the advisory is already patched at `dev`'s version.
- [ ] `dev`-newer packages are the routine updates this release ships; the gate counts them and does not list them.

### Pending dependency updates

Dependabot's weekly schedule is a floor, not the only trigger: the release commit bumps `Cargo.toml` and re-resolves
`Cargo.lock`, and any push that touches a watched manifest re-evaluates immediately. Surface what is pending before
the cut so it merges on `dev` (or gets rejected) instead of arriving as PRs the moment the release lands.

- [ ] Actions → "Dependabot Preflight" → "Run workflow" (`.github/workflows/dependabot-preflight.yml`). A red `cargo`
      job lists direct deps with a newer compatible version; the `github-actions` job's summary table lists SHA-pinned
      actions behind their latest tag. Merge or decline each on `dev` before continuing.

### Cross-repo blast radius

- [ ] Scorecard JSON diff: emit a scorecard on `$LAST_TAG` and on `dev` against the same target, `diff` the JSON. Every
  field renamed / added / removed / shape-changed becomes a row in the release's `### Breaking changes` (consumers
  feature-detect from this list).
- [ ] `agentnative-site` reads the new JSON shape correctly (`/score/<tool>` renders, no `undefined` fields, the new
  `schema_version` is recognized). If the site is not ready, hold the tag.
- [ ] `agentnative-spec` `VERSION` matches `src/principles/spec/VERSION`. If you bumped one without the other, the
  `spec_version` field in the scorecard lies.
- [ ] Every host URL in `src/skill_install/skill.json` resolves (the destination repo exists, the branch the install
  command targets exists).

### Real-world smoke (multi-target)

Driven by `scripts/release/preflight.sh smoke` (project-authored `gate_smoke` body; until it is filled in, the gate
SKIPs and these run by hand).

Self-dogfood exercises one CLI shape; manual probes cover the rest. Pick fresh targets each release.

- [ ] `anc audit <python-CLI>` (e.g. `ruff`, `uv`) runs to completion, scorecard is non-empty.
- [ ] `anc audit <go-CLI>` (e.g. `gh`) runs to completion, scorecard is non-empty.
- [ ] `anc audit <posix-shaped-CLI>` (e.g. `jq`) under `--audit-profile posix-utility`: no panic, suppression doesn't
  crater scoring.
- [ ] `anc audit <CLI-with-no-version-flag>` produces a real `fail` on `p3-must-version` (regression guard for the
  universal MUST landed in #55).
- [ ] One run each with `--audit-profile {human-tui, file-traversal, posix-utility, diagnostic-only}`: no panic, JSON
  remains schema-valid.
- [ ] Shell completions generated by `scripts/generate-completions.sh --check` are current for every supported shell.

### Distribution and install paths

The release builds cross-compiled binaries and the homebrew tap dispatches downstream. None of this runs in `cargo
test`.

- [ ] Last green run of `release.yml` (on this branch or a sibling) cross-compiled all seven targets listed in
  `RELEASES.md` § Tagging and publishing. If the workflow has changed since, dry-run with `cargo build --release
  --target <target>` for each.
- [ ] In a clean container or fresh machine: download a **prior** release archive, run `anc --version` and `anc audit
  <some-repo>`. Confirms the archive layout (binary + completions + README + licenses) still works without the project's
  toolchain. Install of the **newly** published artifact happens post-tag in
  [`RELEASES-POSTFLIGHT.md`](./RELEASES-POSTFLIGHT.md).
- [ ] `anc skill install <host>` for each host slug in `src/skill_install/skill.json`, against a clean per-host
  destination directory. Confirms the hardened `git clone` reaches the live skill-bundle repo, not just the test
  fixture.

### Release mechanics sanity

Driven by `scripts/release/preflight.sh mechanics`.

These items duplicate steps in `RELEASES.md` deliberately: easy to skip, expensive to recover from. Confirm explicitly.

- [ ] `Cargo.toml` `version` bumped to the new tag value (`check-version` in `release.yml` enforces this; catch early).
- [ ] `Cargo.lock` regenerated via `cargo update -p agentnative`, committed.
- [ ] Rebuild locally, confirm `anc --version` prints the new tag value.
- [ ] Every PR merged since `$LAST_TAG` has a non-empty `## Changelog` section. Spot-check via `gh pr list --base dev
  --state merged --search "merged:>$(git log -1 --format=%aI $LAST_TAG)"` then `gh pr view <num> --json body`.
- [ ] `anc emit coverage-matrix --check` exits 0; `git status` shows `docs/coverage-matrix.md` and
  `coverage/matrix.json` pristine.
- [ ] `rust-toolchain.toml` last bumped ≥7 days ago (supply-chain quarantine). If a bump landed inside the window, hold
  or revert it before tagging.
- [ ] No unmerged dependency advisories from `cargo deny check advisories`. The full local pre-push check
  (`scripts/hooks/pre-push`) mirrors CI; run it explicitly before pushing the release branch.
- [ ] Triple-diff verification before tag: `git diff origin/main..HEAD`, `git diff HEAD..origin/dev` filtered by the
  guarded set (not all of `docs/`, since a directory that ships to `main` would hide a missed pick),
  `git diff origin/dev..origin/main` (sanity): all three agree on intended scope.
- [ ] **Leak check before pushing the release branch.** No guarded path may surface in the diff vs `origin/main`. The
  set resolves from `.github/workflows/guard-main-docs.yml` via `scripts/release/guarded-paths.sh`; never restate the
  pattern inline. If cherry-picks pulled in guarded paths via rename detection, resolve per `RELEASES.md` § Cherry-pick
  conflicts on guarded paths.

  ```bash
  GUARDED="$(scripts/release/guarded-paths.sh)"
  git diff origin/main..HEAD --name-only | grep -E "$GUARDED" && echo "LEAKED: reset and redo" || echo "(clean)"
  ```

- [ ] **Every doc this release adds to `main` is meant to ship.** The leak check is blind to a category nobody
  registered. `git diff origin/main..HEAD --diff-filter=A --name-only | grep -E '(^docs/|\.md$)' | grep -Ev "$GUARDED"`
  lists the unguarded additions; each one needs a reason to ship, or it gets registered in the workflow's
  `extra_paths` and removed from the branch.
- [ ] `CHANGELOG.md` versioned section has no `[Unreleased]` placeholder and matches the bumped version.

### Post-tag verification

Moved to [`RELEASES-POSTFLIGHT.md`](./RELEASES-POSTFLIGHT.md) because tagging happens **after** the release-branch cut
and PR-to-main merge, so verification of the tag-triggered pipeline (`release.yml` → homebrew-tap →
`finalize-release.yml` → crates.io publish → fresh-machine install smokes) is post-flight, not pre-flight. Run
`scripts/release/postflight.sh all` immediately after `git push origin vX.Y.Z`.

## Related docs

- [`RELEASES-POSTFLIGHT.md`](./RELEASES-POSTFLIGHT.md): runs AFTER the tag push to verify the downstream pipeline.
- [`RELEASES.md`](./RELEASES.md): operational runbook this checklist gates.
- [`RELEASES-RATIONALE.md`](./RELEASES-RATIONALE.md): release-flow rationale.
- [`CLAUDE.md`](./CLAUDE.md) § Scorecard JSON fields: consumer-facing JSON contract reference.
