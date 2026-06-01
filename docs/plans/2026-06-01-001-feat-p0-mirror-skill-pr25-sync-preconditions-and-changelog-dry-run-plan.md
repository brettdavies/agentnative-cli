---
title: "feat: P0 — mirror skill PR #25 release-flow hardening (sync preconditions + changelog --dry-run)"
type: feat
status: proposed
priority: P0
date: 2026-06-01
origin: "cross-repo mirror of brettdavies/agentnative-skill PR #25 (merged 2026-06-01); scoped to the three items not already shipped via CLI PR #68"
---

# feat: P0 — mirror skill PR #25 release-flow hardening (sync preconditions + changelog --dry-run)

## Summary

`agentnative-skill` PR #25 hardened its release flow with two changes to `scripts/sync-dev-after-release.sh` and two
changes to `scripts/generate-changelog.sh`. The duplicate-section guard from PR #25 already shipped in this repo via
[CLI PR #68](https://github.com/brettdavies/agentnative-cli/pull/68); the remaining three items still need to land
here. A follow-up skill PR ([#26](https://github.com/brettdavies/agentnative-skill/pull/26)) fixed a regression the
`--dry-run` work surfaced in the PR-number extraction regex; mirror that here too.

Reference PRs (read the exact diffs before implementing): <https://github.com/brettdavies/agentnative-skill/pull/25>
and <https://github.com/brettdavies/agentnative-skill/pull/26>.

## Scope

### In scope (five items)

1. **`scripts/sync-dev-after-release.sh` — GitHub Release published-state precondition.** Before the existing
   `tag reachable from origin/main` check, verify the GitHub Release artifact for `$VERSION` exists and is
   published-not-draft:

   ```bash
   gh release view "$VERSION" --json isDraft --jq .isDraft
   ```

   Exit `67` when the release is missing (the `gh` call fails) or when it returns `true` (draft). The tag being
   reachable from `main` is necessary but not sufficient — consumers discover the release via `gh release`, so the
   release artifact must actually exist before the dev-backport commit can claim it does.

2. **`scripts/sync-dev-after-release.sh` — post-sync regen-idempotency check.** After the backport commit is written
   to `dev`, run:

   ```bash
   scripts/generate-changelog.sh --dry-run --tag "$VERSION"
   ```

   Warn (do not fail) when PR bodies have drifted from `main`'s `CHANGELOG.md`. The backport itself is still correct
   against current `main`; drift is a signal that a follow-up release branch should regenerate cleanly, not a reason
   to block the sync.

3. **`scripts/generate-changelog.sh --dry-run` flag.** Stash `CHANGELOG.md`, run the normal generation flow in place,
   print a unified diff to stderr if the regenerated content differs from the stashed copy, restore the original via
   `trap … EXIT`, exit 0 when idempotent and exit 1 on drift. The `--tag` flag is the existing release-version
   selector. `--dry-run` is what item 2 above depends on, so land it in the same PR or land it first.

4. **`scripts/generate-changelog.sh` — PR-number extraction regex fix.** Mirror of
   [skill PR #26](https://github.com/brettdavies/agentnative-skill/pull/26). The current extraction uses
   `grep -oP '\(#\K\d+'`, which only matches the parenthesized `(#14)` form git-cliff emits on the initial prepend.
   The script's own Python expansion step rewrites those to markdown-link form `[#14](https://github.com/.../pull/14)`.
   A second run (e.g. `--dry-run` against an already-processed `CHANGELOG.md`, which is exactly the
   `sync-dev-after-release.sh` regen-idempotency check's mode of operation — item 2 above) extracts zero PR numbers;
   with `set -euo pipefail`, grep's exit-1-on-no-match aborts the script with empty output before
   `summarize_and_exit` can run. Change the regex to `[\(\[]#\K\d+` (accepts both forms) and append `|| true` so the
   downstream `[[ -z "$PR_NUMBERS" ]]` branch handles the empty case via `summarize_and_exit`. Land this together
   with item 3 — without it, item 2's `--dry-run` invocation will produce opaque empty-output failures the first time
   anyone runs the sync script after a release.

5. **`scripts/generate-changelog.sh --dry-run` — wrap-tolerant comparison.** Known follow-up that the skill repo did
   *not* ship in PR #26. The dry-run comparison uses byte-exact `cmp -s`. The on-disk `CHANGELOG.md` is line-wrapped
   by the repo's markdownlint / `md-wrap` hook; the script's direct writes are unwrapped. Even after item 4, the
   dry-run will false-positive "drift" on every release until the comparison is made wrap-tolerant. The
   regen-idempotency check in item 2 is "warn, do not fail", so this does not block the sync — but the warning will
   fire on every release until fixed, training maintainers to ignore it. Fix it in the same pass that ports item 3.
   Suggested approach: run both files through `fmt -w 9999` (or an equivalent paragraph-flatten) before diffing,
   then use `diff --ignore-all-space --ignore-blank-lines`.

### Already shipped here

- **Duplicate-section guard** — `scripts/generate-changelog.sh` already refuses to prepend a `## [X.Y.Z]` section when
  one exists for the current tag (CLI PR #68). No action needed.

## Acceptance

- `scripts/sync-dev-after-release.sh vX.Y.Z` exits `67` with a clear error message when the GitHub Release for
  `vX.Y.Z` is missing or `isDraft=true`. Existing exit codes (`64` usage, `65` dirty tree, `66` tag-not-reachable) are
  unchanged.
- After a successful backport commit, the script invokes `generate-changelog.sh --dry-run --tag $VERSION` and prints a
  warning (stderr, non-fatal) when the regen-idempotency check reports drift; exits 0 either way.
- `scripts/generate-changelog.sh --dry-run --tag vX.Y.Z` exits 0 on idempotent regeneration and exits 1 with a unified
  diff on stderr when PR bodies have drifted.
- `CHANGELOG.md` is restored byte-for-byte after every `--dry-run` run, including the failure path (verified by the
  EXIT trap).
- PR-number extraction handles both `(#X)` and `[#X]` forms, and the script no longer aborts via `set -euo pipefail`
  when an already-processed `CHANGELOG.md` yields zero matches — the empty case flows through `summarize_and_exit`
  cleanly.
- `--dry-run` does not false-positive drift on a `CHANGELOG.md` whose only difference from a fresh regeneration is the
  markdownlint / `md-wrap` line-wrapping applied by the pre-commit hook; the regen-idempotency warning in item 2 only
  fires on real drift (PR bodies edited after release).

## Notes for the implementer

- Read the exact diffs in the skill repo's PRs #25 and #26 before touching anything here — the precondition
  placement, exit codes, EXIT-trap mechanics, regex-with-`|| true` shape, and warn-vs-fail split are load-bearing.
- The existing `sync-dev-after-release.sh` already does `git fetch origin --tags --quiet`; add the `gh release view`
  check after that fetch but before the tag-reachable-from-main check, so all release-state preconditions cluster.
- The script's existing exit codes are `64` (usage), `65` (dirty tree), `66` (tag missing or not reachable). Pick
  `67` for the new release-state precondition to keep them monotonic.
- Conventional Commit style: `feat(scripts): harden sync-dev-after-release.sh with two preconditions` (matches the
  skill repo's commit verb). Open the PR against `dev`, not `main`.
