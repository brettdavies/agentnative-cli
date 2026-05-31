# Pre-release verification: `agentnative`

Operational pre-flight checklist. Runs **before** step 1 of
[`RELEASES.md` § Releasing dev to main](./RELEASES.md#releasing-dev-to-main) — gates the cut of the `release/v<version>`
branch, not the daily dev integration. Each box is an explicit go/no-go. If any item is unchecked or red, hold the
release.

CI (fmt, clippy, test, cargo-deny, skill-fixture-drift, Windows-compat) catches mechanical regressions inside this repo.
This checklist covers what CI structurally can't:

- Breaking changes to the scorecard JSON that downstream consumers must adapt to.
- Real-world behavior against external CLIs (CI only dogfoods `anc` against itself).
- Distribution paths that only exercise on real artifacts (cross-compile binaries, `git clone` to a real skill-bundle
  destination, `cargo install` from a clean machine).
- Cross-repo sequencing where releasing here before `agentnative-site` / `agentnative-spec` is ready breaks downstreams.

## Establish the surface

Everything below assumes you know what's changing. Run this first.

```bash
LAST_TAG=$(git tag --sort=-version:refname | head -n 1)
git log "$LAST_TAG..dev" --oneline                              # commits going out
git diff "$LAST_TAG..dev" --stat                                # file-level scope
git diff "$LAST_TAG..dev" -- src/scorecard/                     # JSON-shape surface
git log "$LAST_TAG..dev" --grep '^[a-z]\+!:' --oneline          # Conventional-Commits breaking markers
```

Every `!:` commit drives the major-version decision and gets a row in the release's `### Breaking changes` section.

## Checklist

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

Self-dogfood exercises one CLI shape; manual probes cover the rest. Pick fresh targets each release.

- [ ] `anc audit <python-CLI>` (e.g. `ruff`, `uv`) runs to completion, scorecard is non-empty.
- [ ] `anc audit <go-CLI>` (e.g. `gh`) runs to completion, scorecard is non-empty.
- [ ] `anc audit <posix-shaped-CLI>` (e.g. `jq`) under `--audit-profile posix-utility` — no panic, suppression doesn't
  crater scoring.
- [ ] `anc audit <CLI-with-no-version-flag>` produces a real `fail` on `p3-must-version` (regression guard for the
  universal MUST landed in #55).
- [ ] One run each with `--audit-profile {human-tui, file-traversal, posix-utility, diagnostic-only}` — no panic, JSON
  remains schema-valid.

### Distribution and install paths

The release builds cross-compiled binaries and the homebrew tap dispatches downstream — none of this runs in `cargo
test`.

- [ ] Last green run of `release.yml` (on this branch or a sibling) cross-compiled all seven targets listed in
  `RELEASES.md` § Tagging and publishing. If the workflow has changed since, dry-run with `cargo build --release
  --target <target>` for each.
- [ ] In a clean container or fresh machine: download a prior release archive, run `anc --version` and `anc audit
  <some-repo>`. Confirms the archive layout (binary + completions + README + licenses) still works without the project's
  toolchain.
- [ ] `anc skill install <host>` for each host slug in `src/skill_install/skill.json`, against a clean per-host
  destination directory. Confirms the hardened `git clone` reaches the live skill-bundle repo, not just the test
  fixture.

### Release mechanics sanity

These items duplicate steps in `RELEASES.md` deliberately — easy to skip, expensive to recover from. Confirm explicitly.

- [ ] `Cargo.toml` `version` bumped to the new tag value (`check-version` in `release.yml` enforces this; catch early).
- [ ] `Cargo.lock` regenerated via `cargo update -p agentnative`, committed.
- [ ] Rebuild locally, confirm `anc --version` prints the new tag value.
- [ ] Every PR merged since `$LAST_TAG` has a non-empty `## Changelog` section. Spot-check via `gh pr list --base dev
  --state merged --search "merged:>$(git log -1 --format=%aI $LAST_TAG)"` then `gh pr view <num> --json body`.
- [ ] `anc emit coverage-matrix --check` exits 0; `git status` shows `docs/coverage-matrix.md` and
  `coverage/matrix.json` pristine.
- [ ] `rust-toolchain.toml` last bumped ≥7 days ago (supply-chain quarantine). If a bump landed inside the window, hold
  or revert it before tagging.
- [ ] No unmerged dependency advisories from `cargo deny check advisories`.

### Post-tag verification

Run immediately after the tag push triggers `release.yml`.

- [ ] `release.yml` green end-to-end. `gh run watch <id> --exit-status` then verify with `gh run view <id> --json
  conclusion` — the watcher exit code alone is not authoritative.
- [ ] Homebrew-tap `update-formula` dispatch completed, then `finalize-release.yml` ran back here and flipped the GitHub
  Release `make_latest: true`.
- [ ] `crates.io` shows the new version published. `cargo install agentnative --version <new>` from a clean environment
  resolves and runs.
- [ ] Click the `badge_url` and `scorecard_url` from a real emitted scorecard against the live site. First-time renders
  for a new spec version can 404 even when the JSON looks correct.
- [ ] `./scripts/sync-dev-after-release.sh v<version> && git push origin dev` — `Cargo.toml` + `Cargo.lock` +
  `CHANGELOG.md` backported to `dev` per `RELEASES.md` § After publish.

## Related docs

- [`RELEASES.md`](./RELEASES.md) — operational runbook this checklist gates.
- [`RELEASES-RATIONALE.md`](./RELEASES-RATIONALE.md) — release-flow rationale.
- [`CLAUDE.md`](./CLAUDE.md) § Scorecard v0.5 Fields — consumer-facing JSON contract reference.
