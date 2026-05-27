#!/usr/bin/env bash
# Vendor agentnative-spec into src/principles/spec/.
#
# Default behavior: resolves the latest v* tag of agentnative-spec via the
# GitHub API and pulls VERSION, CHANGELOG.md, and principles/p*-*.md at
# that tag. The vendored tree is the build-time input for build.rs, which
# generates the REQUIREMENTS slice consumed by `anc audit`.
#
# Override behavior (--ref / SPEC_REF): vendors an explicit branch HEAD,
# tag, or commit SHA instead of the latest v* tag. Use for cross-repo
# coordination of in-flight spec work that hasn't released yet (e.g., a
# CLI/site change that depends on a spec PR landed on `dev` but not yet
# tagged). The resolved short SHA is always printed alongside the ref so
# the user knows exactly what landed; record that SHA in any consumer PR
# body so the vendoring is traceable post-merge.
#
# Transport: `gh api` against the GitHub REST contents endpoint. Pulls
# files individually (no clone, no tarball) so branches, tags, and SHAs
# take the same code path — `?ref=<X>` accepts all three. Requires `gh`
# authenticated against github.com. When the API path fails (network
# down, gh unauthenticated, repo unreachable), the script falls back to
# a local checkout for offline development.
#
# Usage:
#   scripts/sync-spec.sh                            # latest v* tag (default)
#   scripts/sync-spec.sh --ref dev                  # HEAD of dev branch
#   scripts/sync-spec.sh --ref v0.4.0               # explicit tag
#   scripts/sync-spec.sh --ref b4f4d02              # specific commit SHA
#   SPEC_REF=dev scripts/sync-spec.sh               # env-var form of --ref
#   SPEC_ROOT=/path/to/agentnative-spec scripts/sync-spec.sh
#   SPEC_REMOTE_URL=git@github.com:brettdavies/agentnative.git scripts/sync-spec.sh
#
# Flags:
#   --ref <git-ref>  Branch name, tag, or commit SHA to vendor. Wins over
#                    SPEC_REF env var. When unset, the script resolves the
#                    latest v* tag.
#
# Env vars:
#   SPEC_REF         Same as --ref but via env. CLI flag wins on conflict.
#   SPEC_REMOTE_URL  Remote URL identifying the repo. The script parses
#                    `<owner>/<repo>` out of it for the `gh api` calls and
#                    out of it for the local-fallback's remote-name lookup.
#                    Default: https://github.com/brettdavies/agentnative.git
#   SPEC_ROOT        Local checkout to fall back to when the API is
#                    unreachable. Default: $HOME/dev/agentnative-spec
#
# Resync cadence: rerun after every new agentnative-spec tag. The default
# API query picks up new tags automatically. Spec's
# `repository_dispatch:spec-release` event already fires to this repo on
# tag publish — a consumer-side handler that auto-PRs the resync is
# tracked as follow-up work.
#
# Stale orphan files in src/principles/spec/principles/ (e.g., from a spec
# rename) are accepted; `git status` surfaces them at commit time.

set -euo pipefail

SPEC_REMOTE_URL="${SPEC_REMOTE_URL:-https://github.com/brettdavies/agentnative.git}"
SPEC_ROOT="${SPEC_ROOT:-$HOME/dev/agentnative-spec}"
SPEC_REF="${SPEC_REF:-}"

# --- Argument parsing ---------------------------------------------------
# CLI --ref wins over SPEC_REF env. Other flags reserved for future use.
while [[ $# -gt 0 ]]; do
    case "$1" in
        --ref)
            if [[ $# -lt 2 || -z "$2" ]]; then
                echo "error: --ref requires a value (branch, tag, or SHA)" >&2
                exit 2
            fi
            SPEC_REF="$2"
            shift 2
            ;;
        --ref=*)
            SPEC_REF="${1#--ref=}"
            if [[ -z "$SPEC_REF" ]]; then
                echo "error: --ref= requires a value (branch, tag, or SHA)" >&2
                exit 2
            fi
            shift
            ;;
        -h|--help)
            sed -n '2,55p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "error: unknown argument: $1" >&2
            echo "       run \`$0 --help\` for usage" >&2
            exit 2
            ;;
    esac
done

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST_DIR="$REPO_ROOT/src/principles/spec"
DEST_PRINCIPLES="$DEST_DIR/principles"

# Parse `<owner>/<repo>` out of SPEC_REMOTE_URL for `gh api` calls.
# Handles both URL shapes:
#   https://github.com/<owner>/<repo>.git
#   git@github.com:<owner>/<repo>.git
spec_repo="${SPEC_REMOTE_URL%.git}"
spec_repo="${spec_repo#*github.com[/:]}"
spec_repo="${spec_repo%/}"
if [[ -z "$spec_repo" || "$spec_repo" == "$SPEC_REMOTE_URL" || "$spec_repo" != */* ]]; then
    echo "error: could not parse owner/repo from SPEC_REMOTE_URL: $SPEC_REMOTE_URL" >&2
    exit 1
fi

# === Resolution =========================================================
# spec_ref: the ref the user requested (or "" if auto-resolving latest tag)
# resolved_ref: what we actually vendor (e.g., "v0.4.0" or a SHA)
# resolved_sha: 7-char SHA for display (always set after resolution)
# source_label: human-readable origin string for the "vendoring" line
spec_ref=""
resolved_ref=""
resolved_sha=""
source_label=""

# Try the API path first. Captures both branches: explicit user ref OR
# auto-resolve latest v* tag.
api_ok=false

if [[ -n "$SPEC_REF" ]]; then
    # User-specified ref. gh api accepts branches, tags, and SHAs at the
    # same endpoint via `?ref=<X>`.
    if full_sha="$(gh api "repos/$spec_repo/commits/$SPEC_REF" --jq '.sha' 2>/dev/null)"; then
        resolved_ref="$SPEC_REF"
        resolved_sha="${full_sha:0:7}"
        source_label="github.com:$spec_repo via gh api"
        api_ok=true
    fi
else
    # Default: latest v* tag. Query all tags, filter to semver-shape v*,
    # sort -V (version sort, descending), take the first.
    latest_tag="$(gh api "repos/$spec_repo/tags?per_page=100" --jq '.[].name' 2>/dev/null \
        | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+' \
        | sort -V -r \
        | head -n 1 || true)"
    if [[ -n "$latest_tag" ]]; then
        if full_sha="$(gh api "repos/$spec_repo/commits/$latest_tag" --jq '.sha' 2>/dev/null)"; then
            resolved_ref="$latest_tag"
            resolved_sha="${full_sha:0:7}"
            source_label="github.com:$spec_repo via gh api"
            api_ok=true
        fi
    fi
fi

# === Local fallback =====================================================
# When the API path fails (offline, gh not authenticated, repo not
# reachable), use a local checkout instead. The fallback uses `git`
# against SPEC_ROOT — `gh` is not involved in this path so it works
# without network or auth.
if ! $api_ok; then
    if [[ ! -d "$SPEC_ROOT/.git" ]]; then
        echo "error: API path failed and SPEC_ROOT is not a git repository: $SPEC_ROOT" >&2
        echo "       remote: $SPEC_REMOTE_URL" >&2
        if [[ -n "$SPEC_REF" ]]; then
            echo "       requested ref: $SPEC_REF" >&2
        fi
        echo "       check \`gh auth status\`, network access, or point SPEC_ROOT at a local checkout." >&2
        exit 1
    fi
    echo "warning: gh api unreachable; falling back to local $SPEC_ROOT" >&2

    if [[ -n "$SPEC_REF" ]]; then
        # User-provided ref must be resolvable in the local checkout.
        if ! git -C "$SPEC_ROOT" rev-parse --verify --quiet "$SPEC_REF^{commit}" >/dev/null; then
            echo "error: ref \`$SPEC_REF\` not found in $SPEC_ROOT" >&2
            echo "       try \`git -C $SPEC_ROOT fetch --all --tags\` to pick up upstream refs," >&2
            echo "       or pass a SHA the local checkout already contains." >&2
            exit 1
        fi
        resolved_ref="$SPEC_REF"
    else
        # Latest v* tag in the local checkout.
        resolved_ref="$(git -C "$SPEC_ROOT" tag --list 'v*' --sort='-version:refname' | head -n 1)"
        if [[ -z "$resolved_ref" ]]; then
            echo "error: no v* tags found in $SPEC_ROOT" >&2
            echo "       try \`git -C $SPEC_ROOT fetch --tags\` to pick up upstream tags" >&2
            exit 1
        fi
    fi
    resolved_sha="$(git -C "$SPEC_ROOT" rev-parse --short=7 "$resolved_ref^{commit}")"
    source_label="local $SPEC_ROOT"
fi

echo "vendoring $resolved_ref ($resolved_sha) from $source_label"

# === Extract =============================================================
# Fetcher: API path uses `gh api` with raw accept header; local path uses
# `git show`. Both write a single file to $2 from path $1.
fetch_file() {
    local path="$1"
    local dest="$2"
    if $api_ok; then
        gh api -H "Accept: application/vnd.github.raw" \
            "repos/$spec_repo/contents/$path?ref=$resolved_ref" >"$dest"
    else
        git -C "$SPEC_ROOT" show "$resolved_ref:$path" >"$dest"
    fi
}

# Lister: returns names (not paths) of files in a directory at the ref.
# API path uses the contents endpoint; local path uses ls-tree.
list_dir() {
    local dir="$1"
    if $api_ok; then
        gh api "repos/$spec_repo/contents/$dir?ref=$resolved_ref" --jq '.[].name'
    else
        git -C "$SPEC_ROOT" ls-tree --name-only "$resolved_ref" "$dir/" \
            | sed "s|^$dir/||"
    fi
}

mkdir -p "$DEST_PRINCIPLES"

# VERSION and CHANGELOG.md are top-level in the spec repo.
fetch_file "VERSION" "$DEST_DIR/VERSION"
fetch_file "CHANGELOG.md" "$DEST_DIR/CHANGELOG.md"

# Enumerate principle files at the ref and extract each one. Filter to
# p*-*.md so principles/AGENTS.md (spec-side design context, not consumed
# by the site) is skipped.
copied=0
while IFS= read -r name; do
    [[ -z "$name" ]] && continue
    case "$name" in
        p[0-9]-*.md|p[0-9][0-9]-*.md)
            fetch_file "principles/$name" "$DEST_PRINCIPLES/$name"
            copied=$((copied + 1))
            ;;
    esac
done < <(list_dir "principles")

if [[ "$copied" -eq 0 ]]; then
    echo "error: no principles/p*-*.md files found at ref \`$resolved_ref\`" >&2
    exit 1
fi

echo "wrote $copied principle file(s) to $DEST_PRINCIPLES"
echo "wrote VERSION + CHANGELOG.md to $DEST_DIR"
echo
echo "next: review \`git diff\` for unexpected changes, then commit."
