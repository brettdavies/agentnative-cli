#!/usr/bin/env bash
# Backport release artifacts from main to dev after a release tag publishes.
#
# Pulls three files from main and lands them as a single signed commit on dev:
#   - Cargo.toml — surgically updates ONLY the [package].version line. Other
#     Cargo.toml lines on dev (deps, rust-version, etc) may legitimately be
#     ahead of main; they are preserved.
#   - Cargo.lock — regenerated cleanly via `cargo build --release` after the
#     version bump. Never hand-patched.
#   - CHANGELOG.md — copied verbatim from origin/main. Main is fully
#     authoritative for CHANGELOG; dev never edits it directly.
#
# Run AFTER:
#   1. The release/v* → main PR has merged.
#   2. `git tag -a vX.Y.Z` has been pushed to origin.
#   3. `finalize-release.yml` has flipped the GitHub Release to `published`
#      (i.e. homebrew bottles uploaded, make_latest=true, full release done).
#
# Usage:
#   ./scripts/sync-dev-after-release.sh v0.2.0
#
# Idempotent: safe to re-run. If dev already matches main on these three
# files, the script exits 0 with no commit.

set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 vX.Y.Z" >&2
    exit 64
fi

VERSION="$1"
if [[ ! "$VERSION" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "error: version must match vMAJOR.MINOR.PATCH (got: $VERSION)" >&2
    exit 64
fi
VERSION_NO_V="${VERSION#v}"

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

if [[ -n "$(git status --porcelain)" ]]; then
    echo "error: working tree not clean — commit or stash first" >&2
    git status --short >&2
    exit 65
fi

git fetch origin --tags --quiet

# Verify the release tag exists on origin/main.
if ! git rev-parse --verify --quiet "refs/tags/$VERSION" >/dev/null; then
    echo "error: tag $VERSION not found locally — run 'git fetch origin --tags' or verify the release published" >&2
    exit 66
fi

# Verify main is at or past the tag (i.e. release actually merged).
TAG_SHA="$(git rev-parse "$VERSION")"
if ! git merge-base --is-ancestor "$TAG_SHA" origin/main; then
    echo "error: tag $VERSION is not reachable from origin/main — wait for release/v* to merge" >&2
    exit 66
fi

git switch dev
git pull --ff-only origin dev

# Surgical Cargo.toml version bump (only the first `version = "..."` line,
# which is the [package] section's by convention). Uses awk to avoid sed -i
# portability issues across Linux/macOS.
awk -v ver="$VERSION_NO_V" '
    !done && /^version = "[^"]*"$/ { print "version = \"" ver "\""; done=1; next }
    { print }
' Cargo.toml > Cargo.toml.tmp && mv Cargo.toml.tmp Cargo.toml

# CHANGELOG.md from main (authoritative).
git checkout origin/main -- CHANGELOG.md

# Regenerate Cargo.lock cleanly.
cargo build --release --quiet

if git diff --quiet Cargo.toml Cargo.lock CHANGELOG.md; then
    echo "no changes — dev already in sync with $VERSION"
    exit 0
fi

git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "chore(release): backport $VERSION artifacts to dev

Brings dev's release-bookkeeping current with the $VERSION release on main:
Cargo.toml [package].version, regenerated Cargo.lock, and CHANGELOG.md
copied from origin/main."

echo "committed; push with: git push origin dev"
