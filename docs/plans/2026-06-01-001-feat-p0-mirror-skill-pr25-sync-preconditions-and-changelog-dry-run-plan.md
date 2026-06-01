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
here.

Reference PR (read the exact diff before implementing): <https://github.com/brettdavies/agentnative-skill/pull/25>.

## Scope

### In scope (three items)

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

## Notes for the implementer

- Read the exact diffs in the skill repo's PR #25 before touching anything here — the precondition placement, exit
  codes, EXIT-trap mechanics, and warn-vs-fail split are load-bearing.
- The existing `sync-dev-after-release.sh` already does `git fetch origin --tags --quiet`; add the `gh release view`
  check after that fetch but before the tag-reachable-from-main check, so all release-state preconditions cluster.
- The script's existing exit codes are `64` (usage), `65` (dirty tree), `66` (tag missing or not reachable). Pick
  `67` for the new release-state precondition to keep them monotonic.
- Conventional Commit style: `feat(scripts): harden sync-dev-after-release.sh with two preconditions` (matches the
  skill repo's commit verb). Open the PR against `dev`, not `main`.
