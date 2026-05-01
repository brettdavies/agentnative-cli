#!/usr/bin/env bash
# Vendor agentnative-spec into src/principles/spec/.
#
# Resolves the latest v* tag of agentnative-spec, preferring the remote
# repository, and falls back to a local checkout if the remote is
# unreachable. Extracts files via `git show <tag>:<path>` so neither
# checkout's working tree is perturbed. The vendored tree is the
# build-time input for build.rs, which generates the REQUIREMENTS slice.
#
# Usage:
#   scripts/sync-spec.sh
#   SPEC_ROOT=/path/to/agentnative-spec scripts/sync-spec.sh
#   SPEC_REMOTE_URL=git@github.com:brettdavies/agentnative.git scripts/sync-spec.sh
#
# Env vars:
#   SPEC_REMOTE_URL  Remote URL to query first.
#                    Default: https://github.com/brettdavies/agentnative.git
#   SPEC_ROOT        Local checkout to fall back to when the remote is
#                    unreachable. Default: $HOME/dev/agentnative-spec
#
# Resync cadence: rerun after every new agentnative-spec tag. The remote
# query picks up new tags automatically; a local fallback only sees what
# the local checkout already has fetched.
#
# Stale orphan files in src/principles/spec/principles/ (e.g., from a
# spec rename) are accepted; `git status` surfaces them at commit time.

set -euo pipefail

SPEC_REMOTE_URL="${SPEC_REMOTE_URL:-https://github.com/brettdavies/agentnative.git}"
SPEC_ROOT="${SPEC_ROOT:-$HOME/dev/agentnative-spec}"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST_DIR="$REPO_ROOT/src/principles/spec"
DEST_PRINCIPLES="$DEST_DIR/principles"

# Cleanup hook for the temp clone (set only after mktemp succeeds).
tmp_root=""
cleanup() {
    if [[ -n "$tmp_root" && -d "$tmp_root" ]]; then
        rm -rf "$tmp_root"
    fi
}
trap cleanup EXIT

# === Remote-first resolution ===========================================
spec_source=""
spec_tag=""

echo "querying $SPEC_REMOTE_URL for latest v* tag..."
remote_tag="$(git ls-remote --tags --sort='-version:refname' \
    "$SPEC_REMOTE_URL" 'refs/tags/v*' 2>/dev/null \
    | awk '{print $2}' \
    | sed 's|refs/tags/||' \
    | grep -v '\^{}$' \
    | head -n 1 || true)"

if [[ -n "$remote_tag" ]]; then
    tmp_root="$(mktemp -d -t agentnative-spec-XXXXXX)"
    if git clone --depth 1 --branch "$remote_tag" --quiet \
            "$SPEC_REMOTE_URL" "$tmp_root" 2>/dev/null; then
        spec_source="$tmp_root"
        spec_tag="$remote_tag"
        resolved_sha="$(git -C "$spec_source" rev-parse --short=7 "$spec_tag^{commit}")"
        echo "vendoring $spec_tag ($resolved_sha) from remote $SPEC_REMOTE_URL"
    fi
fi

# === Local fallback ====================================================
if [[ -z "$spec_source" ]]; then
    if [[ ! -d "$SPEC_ROOT/.git" ]]; then
        echo "error: remote unreachable and SPEC_ROOT is not a git repository: $SPEC_ROOT" >&2
        echo "       remote: $SPEC_REMOTE_URL" >&2
        echo "       set SPEC_ROOT to your agentnative-spec checkout, or check network access." >&2
        exit 1
    fi
    echo "warning: remote query failed; falling back to local $SPEC_ROOT" >&2

    spec_source="$SPEC_ROOT"
    spec_tag="$(git -C "$spec_source" tag --list 'v*' --sort='-version:refname' | head -n 1)"
    if [[ -z "$spec_tag" ]]; then
        echo "error: no v* tags found in $SPEC_ROOT" >&2
        echo "       try \`git -C $SPEC_ROOT fetch --tags\` to pick up upstream tags" >&2
        exit 1
    fi
    resolved_sha="$(git -C "$spec_source" rev-parse --short=7 "$spec_tag^{commit}")"
    echo "vendoring $spec_tag ($resolved_sha) from local $spec_source"
fi

# === Verify + extract (works identically for remote and local sources) =
if ! git -C "$spec_source" cat-file -e "$spec_tag:principles" 2>/dev/null; then
    echo "error: $spec_tag has no principles/ directory in $spec_source" >&2
    exit 1
fi

mkdir -p "$DEST_PRINCIPLES"

# VERSION and CHANGELOG.md are top-level in the spec repo.
git -C "$spec_source" show "$spec_tag:VERSION" >"$DEST_DIR/VERSION"
git -C "$spec_source" show "$spec_tag:CHANGELOG.md" >"$DEST_DIR/CHANGELOG.md"

# Enumerate principle files at the tag and extract each one.
copied=0
while IFS= read -r path; do
    case "$path" in
        principles/p*-*.md)
            dest_name="${path#principles/}"
            git -C "$spec_source" show "$spec_tag:$path" >"$DEST_PRINCIPLES/$dest_name"
            copied=$((copied + 1))
            ;;
    esac
done < <(git -C "$spec_source" ls-tree --name-only "$spec_tag" principles/)

if [[ "$copied" -eq 0 ]]; then
    echo "error: no principles/p*-*.md files found at $spec_tag" >&2
    exit 1
fi

echo "wrote $copied principle file(s) to $DEST_PRINCIPLES"
echo "wrote VERSION + CHANGELOG.md to $DEST_DIR"
echo
echo "next: review \`git diff\` for unexpected changes, then commit."
