#!/usr/bin/env bash
# Vendor the prose-check tooling shipped on agentnative-spec.
#
# Parallel sync vehicle to scripts/sync-spec.sh, decoupled because prose
# tooling and the principles contract ship together but the CLI doesn't
# have to re-sync prose every time the contract changes. sync-spec.sh
# covers the contract anc lints against (principles, VERSION, CHANGELOG);
# this script covers the shared prose-check tooling (BRAND.md, Vale rule
# packs, vocabulary, orchestrator, generator, harness).
#
# Resolves the HEAD of agentnative-spec's `main` branch, preferring the
# remote repository, and falls back to a local checkout if the remote is
# unreachable. Extracts files via `git show <sha>:<path>` so neither
# checkout's working tree is perturbed. Mirrors the site repo's pattern
# (agentnative-site/scripts/sync-prose-tooling.sh) so the two consumers
# stay legible side-by-side.
#
# Tracks `main` HEAD by design — prose tooling is shared infrastructure
# with a faster cadence than the principle contract, and consumers do not
# pin to versions of it. Tag-pinning is for the principle contract via
# scripts/sync-spec.sh, where consumers pin to a released spec version.
#
# Vendored manifest (paths from spec main, mirrored verbatim into this
# repo at the same paths):
#
#   BRAND.md                                            (universal voice SoT)
#   styles/brand/                                       (universal rule pack + README)
#   styles/config/                                      (vocab: brand accept/reject lists)
#   scripts/prose-check.sh                              (orchestrator)
#   scripts/test-prose-check.mjs                        (test harness)
#   scripts/generate-pack-readme.mjs                    (generator)
#
# Skipped on purpose:
#
#   .vale.ini           Per-consumer vale config; upstream's references
#                       the `spec` rule pack which is wrong for CLI prose.
#                       Site repo follows the same pattern (vendors brand
#                       content + orchestrator only; authors its own
#                       `.vale.ini`).
#   styles/spec/        RFC 2119 register, wrong for CLI prose. Vendoring
#                       would systematic-false-positive every README install
#                       instruction.
#   styles/proselint/   Downloaded by vale at runtime via .vale.ini's
#   styles/write-good/  `Packages = ...` line; gitignored upstream. Mirror
#                       the same gitignore here so vale auto-fetches on
#                       first invocation.
#
# Usage:
#   scripts/sync-prose-tooling.sh
#   scripts/sync-prose-tooling.sh --check    drift detection (CI mode)
#   SPEC_ROOT=/path/to/agentnative-spec scripts/sync-prose-tooling.sh
#   SPEC_REMOTE_URL=git@github.com:brettdavies/agentnative.git scripts/sync-prose-tooling.sh
#
# Env vars (shared with sync-spec.sh):
#   SPEC_REMOTE_URL  Remote URL to query first.
#                    Default: https://github.com/brettdavies/agentnative.git
#   SPEC_ROOT        Local checkout to fall back to when the remote is
#                    unreachable. Default: $HOME/dev/agentnative-spec
#
# Resync cadence: rerun after any spec `main` push that touches any path
# in the manifest above. Faster cadence than spec tags; this script tracks
# `main` HEAD by design — tag-pinning is for the principle contract via
# scripts/sync-spec.sh. Idempotent at a fixed spec sha: re-running at the
# same HEAD produces no `git diff`. CI workflow (prose-tooling-drift.yml,
# deferred follow-up) runs `--check` on every PR and on a weekly schedule.
#
# CLI-LOCAL DIVERGENCE: scripts/prose-check.sh carries CLI-specific path
# exclusions (src/principles/spec/, docs/ideation/, tests/fixtures/) and
# is therefore EXCLUDED from --check byte-equivalence verification. The
# inline edits cannot be reversibly bracketed because the upstream `find`
# invocation and the upstream `grep -v -E` regex are single multi-line
# expressions; representing both upstream and CLI variants inline would
# require either duplicate executions or post-strip code restructuring,
# neither of which earns its complexity. Re-running this script overwrites
# the orchestrator and forces re-applying the divergence; `git diff
# scripts/prose-check.sh` post-sync surfaces what got reset. Until
# upstream lands the `--exclude PATTERN` flag tracked at
# agentnative-spec/.context/compound-engineering/todos/010-pending-p0-prose-check-consumer-exclusion-config.md,
# this divergence is expected.

set -euo pipefail

SPEC_REMOTE_URL="${SPEC_REMOTE_URL:-https://github.com/brettdavies/agentnative.git}"
SPEC_ROOT="${SPEC_ROOT:-$HOME/dev/agentnative-spec}"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

CHECK_MODE=0
while (( $# )); do
    case "$1" in
        --check)  CHECK_MODE=1 ;;
        -h|--help)
            sed -n '3,55p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "sync-prose-tooling: unknown flag '$1'" >&2
            exit 2
            ;;
    esac
    shift
done

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
spec_ref=""

echo "querying $SPEC_REMOTE_URL for main HEAD..."
remote_sha="$(git ls-remote "$SPEC_REMOTE_URL" 'refs/heads/main' 2>/dev/null \
    | awk '{print $1}' \
    | head -n 1 || true)"

if [[ -n "$remote_sha" ]]; then
    tmp_root="$(mktemp -d -t agentnative-prose-XXXXXX)"
    if git clone --depth 1 --branch main --quiet \
            "$SPEC_REMOTE_URL" "$tmp_root" 2>/dev/null; then
        spec_source="$tmp_root"
        spec_ref="main"
        resolved_sha="$(git -C "$spec_source" rev-parse --short=7 main)"
        echo "resolved main ($resolved_sha) from remote $SPEC_REMOTE_URL"
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
    if ! git -C "$spec_source" rev-parse --verify main >/dev/null 2>&1; then
        echo "error: local $SPEC_ROOT has no 'main' branch" >&2
        echo "       try \`git -C $SPEC_ROOT fetch origin main\` to pick up upstream HEAD" >&2
        exit 1
    fi
    spec_ref="main"
    resolved_sha="$(git -C "$spec_source" rev-parse --short=7 main)"
    echo "resolved main ($resolved_sha) from local $spec_source"
fi

# === Verify expected paths exist at main HEAD ==========================
required_paths=(
    "BRAND.md"
    "styles/brand"
    "styles/config"
    "scripts/prose-check.sh"
    "scripts/test-prose-check.mjs"
    "scripts/generate-pack-readme.mjs"
)
for path in "${required_paths[@]}"; do
    if ! git -C "$spec_source" cat-file -e "$spec_ref:$path" 2>/dev/null; then
        echo "error: $spec_ref ($resolved_sha) is missing required path: $path" >&2
        echo "       (the prose-check stack may not be present on main)" >&2
        exit 1
    fi
done

# Files vendored at top level (path-relative to repo root in both source and dest).
# Format: "<upstream-path>"
top_level_files=(
    "BRAND.md"
    "scripts/prose-check.sh"
    "scripts/test-prose-check.mjs"
    "scripts/generate-pack-readme.mjs"
)

# Directories vendored verbatim (all immediate file children, recursively where
# needed). Each entry walks `git ls-tree -r` at the resolved spec sha.
tree_dirs=(
    "styles/brand"
    "styles/config"
)

# === Mode dispatch =====================================================
if (( CHECK_MODE )); then
    drift=0

    check_blob() {
        # $1 = upstream path; compare against repo-local file at same path.
        # Uses temp files (not $(git show)) so empty blobs and trailing-newline
        # whitespace stay byte-faithful — command substitution strips trailing
        # newlines, which would false-positive on blank vocab files.
        local upstream_path="$1"
        local local_path="$REPO_ROOT/$upstream_path"
        # scripts/prose-check.sh carries a CLI-LOCAL DIVERGENCE block (see
        # script header). It is intentionally excluded from byte-equivalence
        # checking until upstream todo #010 lands. Re-syncing overwrites
        # the divergence; operators re-apply by hand post-sync.
        if [[ "$upstream_path" == "scripts/prose-check.sh" ]]; then
            return
        fi
        if [[ ! -f "$local_path" ]]; then
            echo "drift: missing locally: $upstream_path" >&2
            drift=1
            return
        fi
        local upstream_tmp
        upstream_tmp="$(mktemp -t anc-prose-check-XXXXXX)"
        git -C "$spec_source" show "$spec_ref:$upstream_path" >"$upstream_tmp"
        if ! cmp -s "$upstream_tmp" "$local_path"; then
            echo "drift: $upstream_path" >&2
            drift=1
        fi
        rm -f "$upstream_tmp"
    }

    for path in "${top_level_files[@]}"; do
        check_blob "$path"
    done

    for dir in "${tree_dirs[@]}"; do
        while IFS= read -r path; do
            [[ -n "$path" ]] || continue
            check_blob "$path"
        done < <(git -C "$spec_source" ls-tree -r --name-only "$spec_ref" "$dir")
    done

    if (( drift )); then
        echo "sync-prose-tooling: drift detected; rerun without --check to resolve" >&2
        exit 1
    fi
    echo "sync-prose-tooling: --check OK (all files byte-equal upstream main @ $resolved_sha)"
    exit 0
fi

# === Vendor mode =======================================================
extracted=0

extract_blob() {
    local upstream_path="$1"
    local dest="$REPO_ROOT/$upstream_path"
    mkdir -p "$(dirname "$dest")"
    git -C "$spec_source" show "$spec_ref:$upstream_path" >"$dest"
    extracted=$((extracted + 1))
}

for path in "${top_level_files[@]}"; do
    extract_blob "$path"
done

for dir in "${tree_dirs[@]}"; do
    while IFS= read -r path; do
        [[ -n "$path" ]] || continue
        extract_blob "$path"
    done < <(git -C "$spec_source" ls-tree -r --name-only "$spec_ref" "$dir")
done

# git show drops the executable bit; restore it for the orchestrator.
chmod +x "$REPO_ROOT/scripts/prose-check.sh"

echo "wrote $extracted file(s) from main @ $resolved_sha"
echo
echo "next: review \`git diff\` for unexpected changes; reapply CLI-LOCAL"
echo "      DIVERGENCE block in scripts/prose-check.sh if sync overwrote it;"
echo "      then commit."
