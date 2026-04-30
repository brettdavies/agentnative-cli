#!/usr/bin/env bash
# Sync tests/fixtures/skill.json from agentnative-site/src/data/skill.json.
#
# The fixture is the drift anchor between this binary's hardcoded host map
# (src/skill_install.rs) and the canonical site contract. Drift is caught at
# two layers: the cargo-level companion test 12 (host_map_matches_site_skill_json)
# fires on `cargo test`, and `--check` mode here fires on every PR.
#
# Modes:
#   scripts/sync-skill-fixture.sh           Update the fixture in place.
#   scripts/sync-skill-fixture.sh --check   Verify the fixture is current;
#                                           exit non-zero on drift.
#
# Env vars (mirroring scripts/sync-spec.sh shape):
#   SKILL_SITE_REMOTE_URL  Remote URL to query first.
#                          Default: https://github.com/brettdavies/agentnative-site.git
#   SKILL_SITE_REF         Ref to extract from. Default: dev.
#                          (agentnative-site uses a dev/main forever-branch
#                          flow — dev is the working trunk; main is older.)
#   SKILL_SITE_ROOT        Local checkout to fall back to when the remote is
#                          unreachable. Default: $HOME/dev/agentnative-site
#
# Resync cadence: rerun whenever agentnative-site changes
# src/data/skill.json. Pre-release checklist captures this in RELEASES.md.

set -euo pipefail

SKILL_SITE_REMOTE_URL="${SKILL_SITE_REMOTE_URL:-https://github.com/brettdavies/agentnative-site.git}"
SKILL_SITE_REF="${SKILL_SITE_REF:-dev}"
SKILL_SITE_ROOT="${SKILL_SITE_ROOT:-$HOME/dev/agentnative-site}"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST_FILE="$REPO_ROOT/tests/fixtures/skill.json"
SOURCE_PATH="src/data/skill.json"

mode="update"
if [[ ${1:-} == "--check" ]]; then
    mode="check"
elif [[ -n ${1:-} ]]; then
    echo "error: unknown argument: $1" >&2
    echo "usage: $0 [--check]" >&2
    exit 2
fi

# Always-allocated workspace dir for the upstream copy. The remote path
# also clones into a child of this dir; the local path uses it just for
# the staged blob.
tmp_root="$(mktemp -d -t agentnative-site-sync-XXXXXX)"
cleanup() {
    if [[ -n "$tmp_root" && -d "$tmp_root" ]]; then
        rm -rf "$tmp_root"
    fi
}
trap cleanup EXIT

# === Remote-first resolution ===========================================
site_source=""
resolved_sha=""

echo "querying $SKILL_SITE_REMOTE_URL for $SKILL_SITE_REF..."
remote_clone="$tmp_root/clone"
if git clone --depth 1 --branch "$SKILL_SITE_REF" --quiet \
        "$SKILL_SITE_REMOTE_URL" "$remote_clone" 2>/dev/null; then
    site_source="$remote_clone"
    resolved_sha="$(git -C "$site_source" rev-parse --short=7 HEAD)"
    echo "extracting $SKILL_SITE_REF ($resolved_sha) from remote $SKILL_SITE_REMOTE_URL"
fi

# === Local fallback ====================================================
if [[ -z "$site_source" ]]; then
    if [[ ! -d "$SKILL_SITE_ROOT/.git" ]]; then
        echo "error: remote unreachable and SKILL_SITE_ROOT is not a git repository: $SKILL_SITE_ROOT" >&2
        echo "       remote: $SKILL_SITE_REMOTE_URL" >&2
        echo "       set SKILL_SITE_ROOT to your agentnative-site checkout, or check network access." >&2
        exit 1
    fi
    echo "warning: remote query failed; falling back to local $SKILL_SITE_ROOT" >&2
    site_source="$SKILL_SITE_ROOT"
    resolved_sha="$(git -C "$site_source" rev-parse --short=7 "$SKILL_SITE_REF" 2>/dev/null || echo "unknown")"
    echo "extracting $SKILL_SITE_REF ($resolved_sha) from local $site_source"
fi

# === Extract via git show (works identically for remote and local) =====
if ! git -C "$site_source" cat-file -e "$SKILL_SITE_REF:$SOURCE_PATH" 2>/dev/null; then
    echo "error: $SKILL_SITE_REF has no $SOURCE_PATH in $site_source" >&2
    exit 1
fi

# Stream the upstream blob to a temp file. Variable capture (`$(...)`) would
# strip trailing newlines, breaking byte-for-byte parity with the committed
# fixture. File-based comparison preserves exact bytes including the EOF
# newline boundary.
upstream_tmp="$tmp_root/skill.json"
git -C "$site_source" show "$SKILL_SITE_REF:$SOURCE_PATH" >"$upstream_tmp"

# === Mode-specific behavior ============================================
case "$mode" in
    update)
        mkdir -p "$(dirname "$DEST_FILE")"
        cp "$upstream_tmp" "$DEST_FILE"
        echo "wrote $DEST_FILE"
        echo
        echo "next: review \`git diff $DEST_FILE\` for unexpected changes, then commit."
        ;;
    check)
        if cmp -s "$DEST_FILE" "$upstream_tmp"; then
            echo "ok: $DEST_FILE matches $SKILL_SITE_REF:$SOURCE_PATH ($resolved_sha)"
            exit 0
        fi
        echo "error: $DEST_FILE drifted from $SKILL_SITE_REF:$SOURCE_PATH ($resolved_sha)" >&2
        echo "       run \`scripts/sync-skill-fixture.sh\` to refresh, then commit." >&2
        echo >&2
        diff -u "$DEST_FILE" "$upstream_tmp" || true
        exit 1
        ;;
esac
