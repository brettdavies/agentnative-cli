# Pre-release verification: `agentnative`

Operational pre-flight checklist. Walk it before step 1 of
[`RELEASES.md` § Releasing dev to main](./RELEASES.md#releasing-dev-to-main), the cut: it gates the `release/v<version>`
branch, not the daily dev integration. The automated gates (`scripts/release/preflight.sh all`) run at step 3, against
the committed release branch, because the mechanics checks read the bumped version and the release tree; the cut runs
the drift gate itself before it branches. Each box is an explicit go/no-go. If any item is unchecked or red, hold the
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
1Password reads, `shred -u` tempdir cleanup, subcommand dispatch, surface, changelog-sections, semver and mechanics
gates) is vendored from the `github-repo-setup` skill's skeleton, and the project-specific smoke body lives in a sibling
script (`scripts/release/smoke.sh`) that the roster delegates to, as it delegates drift to the vendored
`scripts/release/drift.sh`. `all` runs the drift gate first, since nothing else matters while `main` holds changes `dev`
never received. The recipes in the sections below document what each gate verifies and serve as the manual fallback when
running by hand.

Sub-commands let you re-run one section in isolation:

| Sub-command          | What it checks                                                                                                              | Source of truth                                                |
| -------------------- | --------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| `drift`              | Commits on `main` that `dev` lacks, `.github/` parity, lockfile packages `main` has newer                                   | `scripts/release/drift.sh`                                     |
| `surface`            | Commits since the last release reached `dev`, files changed since the last tag, breaking markers                            | `git log`, `git diff`                                          |
| `smoke`              | Schema parity, multi-target audit runs, scorecard validation, negative control                                              | `scripts/release/smoke.sh`                                     |
| `mechanics`          | Version, lockfile, toolchain age, advisories, leak check, unguarded docs, diff-B                                            | `Cargo.toml`, `CHANGELOG.md`, `cargo deny`, `guarded-paths.sh` |
| `changelog-sections` | No PR merged into `dev` since the last release leaves its changelog entry to its title for want of a `## Changelog` section | `generate-changelog.py --audit-sections`, `gh`                 |
| `semver`             | cargo-semver-checks against the release type the version bump claims over the last `v` tag                                  | `cargo semver-checks`                                          |
| `all`                | Every above sequentially, drift first                                                                                       |                                                                |

Flags:

- `--smoke-home PATH`: reuse an existing seeded `$SMOKE_HOME` instead of creating + seeding
- `--no-cleanup`: keep `$SMOKE_HOME` after exit (default: shred on exit)
- `--tag TAG`: override `LAST_TAG` resolution (default: the newest `v[0-9]*` tag)

`anc` is a single-binary CLI with no deployed HTTP surface, so `scripts/release/surface-smoke.sh` is not vendored and
the `surface-smoke` sub-command SKIPs.

After `git push origin vX.Y.Z` triggers the release pipeline, run
[`scripts/release/postflight.sh all`](./RELEASES-POSTFLIGHT.md) to verify the downstream chain.

## Establish the surface

Everything below assumes you know what's changing. Run this first.

Driven by `scripts/release/preflight.sh surface`.

```bash
LAST_TAG=$(git tag --list 'v[0-9]*' --sort=-version:refname | head -n 1)
# The commit that synced the tag back into dev opens dev's window.
SINCE=$(git log origin/dev --format='%H %s' \
  | grep -E -m1 "^[0-9a-f]+ chore\(release\): (sync dev after|backport) ${LAST_TAG//./\\.}( |$)" | cut -d' ' -f1)
git log "$SINCE..origin/dev" --oneline                          # commits going out
git diff "$LAST_TAG" origin/dev --name-only \
  | grep -Ev "$(scripts/release/guarded-paths.sh)"              # file-level scope: what ships
git diff "$LAST_TAG" origin/dev -- src/scorecard/               # JSON-shape surface
git log "$SINCE..origin/dev" --grep '^[a-z]\+\(([^)]*)\)\?!:' --oneline   # Conventional-Commits breaking markers, scoped or not
```

Every release squash-merges into `main`, so the tag shares no recent history with `dev`, and a log from the tag counts
`dev`'s whole past. The window opens at the commit that synced the tag back into `dev`, whose subject reads
`chore(release): sync dev after vX.Y.Z`: the boundary `generate-changelog.py` uses. The file list compares trees, so it
reads from the tag directly, minus the guarded set `dev` carries but never ships. With no commit syncing the tag back,
`preflight.sh surface` SKIPs, and the surface is `origin/main..origin/dev`.

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
- [ ] Nothing under `.github/` on `main` is missing from `dev` (gate 2). `.github/` reaches `main` through the release,
  so config that has reached `dev` and not `main` is what this release delivers, and the gate counts it. The gate fails
  on the other direction, where `main` holds workflow or ruleset config `dev` never received, and names each path as
  `missing` or `differs`.
- [ ] No `Cargo.lock` package resolves newer on `main` than on `dev` (gate 3). The one benign case is a version still
  inside a release-age window when the advisory is already patched at `dev`'s version.
- [ ] `dev`-newer packages are the routine updates this release ships; the gate counts them and does not list them.

### Dependabot preflight

Run before the version is bumped and before the release branch is cut. A release commit that re-resolves `Cargo.lock`
triggers Dependabot's out-of-cycle re-evaluation, so an update still pending at the cut arrives as a PR the moment the
release lands, after the tag it needed to make. Surface what is pending now, so each update merges on `dev` or is
declined first.

- [ ] Trigger the workflow: Actions → "Dependabot Preflight" → "Run workflow" (head = `dev`). The caller is
  `.github/workflows/dependabot-preflight.yml`, a thin caller of the `brettdavies/.github` reusable.
- [ ] Review the `cargo` job's `cargo outdated --workspace --depth 1` report in the run summary; the job runs red while
  any direct dependency has a newer compatible version. For each one, decide: merge an update PR on `dev` now, accept
  the stale version this release, or rule the update out with a `Cargo.toml` constraint.
- [ ] Review the `github-actions` job's pin-drift table. For every drifted action, bump the pinned SHA on `dev` and
  update the trailing `# <version>` comment.
- [ ] (Optional) Have Dependabot open PRs for whatever the preflight surfaced: Insights → Dependency graph → Dependabot
  → "Check for updates". Merge anything that passes CI on `dev`.

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

Driven by `scripts/release/preflight.sh smoke`, which delegates to `scripts/release/smoke.sh`. Self-dogfood exercises
one CLI shape; this gate covers the rest by spawning third-party binaries the auditor has never seen.

The gate asserts, over a default matrix of six real CLIs spanning every documented `--audit-profile`:

- [ ] `anc emit schema` matches committed `schema/scorecard.schema.json`, the copy consumers validate against.
- [ ] Each target grades to completion: exit code is a verdict (0/1/2) rather than a panic (101) or a signal (128+N),
  the JSON parses with a non-empty result set, and the scorecard echoes back the profile it was asked for.
- [ ] Every emitted scorecard validates against the committed schema (needs `uvx` for `check-jsonschema`; SKIPs without
  it).
- [ ] `tests/fixtures/no-version-flag/noversion` still produces a real `fail` on `p3-must-version` (#55). This is the
  negative control: every other target can come back green whether the auditor works or has stopped auditing, and this
  one cannot.

Pass `--targets "name[:profile] ..."` to swap in fresh targets for a given release; a target absent from `PATH` SKIPs
rather than fails, so the gate reports the same way on a dev box and in CI.

Still manual:

- [ ] Shell completions generated by `scripts/generate-completions.sh --check` are current for every supported shell.

### Distribution and install paths

The release builds cross-compiled binaries and the homebrew tap dispatches downstream. None of this runs in
`cargo test`.

- [ ] Last green run of `release.yml` (on this branch or a sibling) cross-compiled all seven targets listed in
  `RELEASES.md` § Tagging and publishing. If the workflow has changed since, dry-run with
  `cargo build --release --target <target>` for each.
- [ ] In a clean container or fresh machine: download a **prior** release archive, run `anc --version` and
  `anc audit <some-repo>`. Confirms the archive layout (binary + completions + README + licenses) still works without
  the project's toolchain. Install of the **newly** published artifact happens post-tag in
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
- [ ] No PR this release carries leaves its changelog entry to its title. Swept, not sampled, by
  `scripts/release/preflight.sh changelog-sections`, which runs `generate-changelog.py --audit-sections`. It reads the
  PRs from `dev`'s history since the previous release, stacked PRs included, and names every one that never offered the
  `## Changelog` section under a title the fallback would print. A section left empty on purpose passes: the generator
  reads it as nothing user-facing.
- [ ] `anc emit coverage-matrix --check` exits 0; `git status` shows `docs/coverage-matrix.md` and
  `coverage/matrix.json` pristine.
- [ ] `rust-toolchain.toml` last bumped ≥7 days ago (supply-chain quarantine). If a bump landed inside the window, hold
  or revert it before tagging.
- [ ] No unmerged dependency advisories from `cargo deny check advisories`. The full local pre-push check
  (`scripts/hooks/pre-push`) mirrors CI; run it explicitly before pushing the release branch.
- [ ] `scripts/release/cut-release-branch.sh` exited 0, so its check A held: the staged tree equals `origin/dev`'s apart
  from the version carriers and the guarded paths. A cherry-pick release runs the triple diff in `RELEASES.md` §
  Exception: cherry-pick instead, with `HEAD..origin/dev` filtered by the guarded set (not all of `docs/`, since a
  directory that ships to `main` would hide a missed pick).
- [ ] **Leak check before pushing the release branch.** No guarded path may be added or modified in the diff vs
  `origin/main`. The cut's check B screens the staged tree, and `preflight.sh mechanics` screens the committed branch;
  both resolve the set from `.github/workflows/guard-main-docs.yml` via `scripts/release/guarded-paths.sh`, so never
  restate the pattern inline. `--diff-filter=ACMR`, because a release that removes a guarded doc `main` still carries
  lists the removal too, and that is cleanup, not a leak. If cherry-picks pulled in guarded paths via rename detection,
  resolve per `RELEASES.md` § Cherry-pick conflicts on guarded paths.

  ```bash
  GUARDED="$(scripts/release/guarded-paths.sh)"
  git diff origin/main..HEAD --diff-filter=ACMR --name-only | grep -E "$GUARDED" && echo "LEAKED: reset and redo" || echo "(clean)"
  ```

- [ ] **Every doc this release adds to `main` is meant to ship.** The leak check is blind to a category nobody
  registered. The cut's check D and `preflight.sh mechanics` list the unguarded additions, as does the command below;
  each one needs a reason to ship, or it gets registered in the workflow's `extra_paths` and removed from the branch.
  `--no-renames` lists a doc moved from one `main` carries as added, where rename detection would report it as a rename
  and the `A` filter would drop it.

  ```bash
  git diff --no-renames origin/main..HEAD --diff-filter=A --name-only | grep -E '(^docs/|\.md$)' | grep -Ev "$GUARDED"
  ```

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
