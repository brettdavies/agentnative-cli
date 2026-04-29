#!/usr/bin/env bash
# Vendor agentnative-spec into src/principles/spec/.
#
# Extracts files at a pinned git ref via `git show <ref>:<path>` so the user's
# spec working tree is not perturbed. The vendored tree is the build-time input
# for build.rs, which generates the REQUIREMENTS slice.
#
# Usage:
#   scripts/sync-spec.sh
#   SPEC_REF=v0.2.1 scripts/sync-spec.sh
#   SPEC_ROOT=/path/to/agentnative-spec scripts/sync-spec.sh
#
# Env vars:
#   SPEC_ROOT  Path to agentnative-spec checkout. Default: $HOME/dev/agentnative-spec
#   SPEC_REF   Git ref (tag, branch, or SHA) to vendor. Default: v0.3.0
#
# Resync cadence: rerun after every new agentnative-spec tag. Stale orphan
# files in src/principles/spec/principles/ (e.g., from a spec rename) are
# accepted; `git status` surfaces them at commit time.

set -euo pipefail

SPEC_ROOT="${SPEC_ROOT:-$HOME/dev/agentnative-spec}"
SPEC_REF="${SPEC_REF:-v0.3.0}"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST_DIR="$REPO_ROOT/src/principles/spec"
DEST_PRINCIPLES="$DEST_DIR/principles"

if [[ ! -d "$SPEC_ROOT/.git" ]]; then
    echo "error: SPEC_ROOT is not a git repository: $SPEC_ROOT" >&2
    echo "       set SPEC_ROOT to your agentnative-spec checkout, or clone it to the default" >&2
    echo "       location: \$HOME/dev/agentnative-spec" >&2
    exit 1
fi

if ! git -C "$SPEC_ROOT" rev-parse --verify --quiet "$SPEC_REF^{commit}" >/dev/null; then
    echo "error: SPEC_REF does not resolve to a commit in $SPEC_ROOT: $SPEC_REF" >&2
    echo "       try \`git -C $SPEC_ROOT fetch --tags\` or check the ref name" >&2
    exit 1
fi

resolved_sha="$(git -C "$SPEC_ROOT" rev-parse --short=7 "$SPEC_REF^{commit}")"
echo "vendoring $SPEC_REF ($resolved_sha) from $SPEC_ROOT"

# Verify the principles/ tree exists at the ref.
if ! git -C "$SPEC_ROOT" cat-file -e "$SPEC_REF:principles" 2>/dev/null; then
    echo "error: $SPEC_REF has no principles/ directory in $SPEC_ROOT" >&2
    exit 1
fi

mkdir -p "$DEST_PRINCIPLES"

# VERSION and CHANGELOG.md are top-level in the spec repo.
git -C "$SPEC_ROOT" show "$SPEC_REF:VERSION" >"$DEST_DIR/VERSION"
git -C "$SPEC_ROOT" show "$SPEC_REF:CHANGELOG.md" >"$DEST_DIR/CHANGELOG.md"

# Enumerate principle files at the ref and extract each one.
copied=0
while IFS= read -r path; do
    case "$path" in
        principles/p*-*.md)
            dest_name="${path#principles/}"
            git -C "$SPEC_ROOT" show "$SPEC_REF:$path" >"$DEST_PRINCIPLES/$dest_name"
            copied=$((copied + 1))
            ;;
    esac
done < <(git -C "$SPEC_ROOT" ls-tree --name-only "$SPEC_REF" principles/)

if [[ "$copied" -eq 0 ]]; then
    echo "error: no principles/p*-*.md files found at $SPEC_REF" >&2
    exit 1
fi

echo "wrote $copied principle file(s) to $DEST_PRINCIPLES"
echo "wrote VERSION + CHANGELOG.md to $DEST_DIR"
echo
echo "next: review \`git diff\` for unexpected changes, then commit."
