#!/usr/bin/env bash
# Sync the web-audit inputs from agentnative-site at the pinned commit.
#
# Build inputs (build.rs compiles them into $OUT_DIR/generated_web_registry.rs):
#   src/data/web-audit/registry.yaml            -> src/web_audit/vendored/registry.yaml
#   src/data/web-audit/remediation.yaml         -> src/web_audit/vendored/remediation.yaml
#   src/shared/user-agents.ts                   -> src/web_audit/vendored/user-agents.ts
#   src/shared/site-url.ts                      -> src/web_audit/vendored/site-url.ts
#   src/shared/audit-routes.ts                  -> src/web_audit/vendored/audit-routes.ts
# Test inputs (Cargo.toml excludes tests/ from the crates.io package):
#   tests/fixtures/web-audit-score-parity.json  -> tests/fixtures/web-audit-score-parity.json
#   tests/fixtures/web-audit-conformance/       -> tests/fixtures/web-audit-conformance/
#
# The pin is src/web_audit/vendored/SITE_SHA, a full 40-hex commit on the
# site's dev branch. Fetching is by commit, never by branch: `git fetch
# --depth 1 origin <sha>` yields exactly that commit or fails naming it.
# CI's web-audit-drift workflow runs `--check` on every PR.
#
# Modes:
#   scripts/sync-web-audit.sh              Refresh every vendored copy at the pin.
#   scripts/sync-web-audit.sh --pin <sha>  Move the pin to <sha>, then refresh.
#   scripts/sync-web-audit.sh --check      Verify every copy matches the pin;
#                                          exit 1 on drift, listing each path.
#
# Env vars:
#   WEB_AUDIT_SITE_SHA         Commit to use for this run instead of SITE_SHA
#                              (the file is rewritten only by --pin).
#   WEB_AUDIT_SITE_REMOTE_URL  Remote to fetch from.
#                              Default: https://github.com/brettdavies/agentnative-site.git
#   WEB_AUDIT_SITE_REPO        Local agentnative-site repository to read the
#                              commit from instead of fetching.
#   WEB_AUDIT_DEST_ROOT        Repository root to write into or check.
#                              Default: the repository this script lives in.
#
# Every git call runs with the same hardening src/skill_install.rs applies to
# `anc skill install`: user and system git config disabled, credential
# prompts off, SSH/proxy/askpass/exec-path overrides stripped, and the five
# `-c` flags below. tests/sync_web_audit.rs pins that surface.

set -euo pipefail

WEB_AUDIT_SITE_REMOTE_URL="${WEB_AUDIT_SITE_REMOTE_URL:-https://github.com/brettdavies/agentnative-site.git}"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST_ROOT="${WEB_AUDIT_DEST_ROOT:-$REPO_ROOT}"
PIN_FILE="src/web_audit/vendored/SITE_SHA"

# "<site path>=<destination path>", relative to each repository root.
SYNC_FILES=(
  "src/data/web-audit/registry.yaml=src/web_audit/vendored/registry.yaml"
  "src/data/web-audit/remediation.yaml=src/web_audit/vendored/remediation.yaml"
  "src/shared/user-agents.ts=src/web_audit/vendored/user-agents.ts"
  "src/shared/site-url.ts=src/web_audit/vendored/site-url.ts"
  "src/shared/audit-routes.ts=src/web_audit/vendored/audit-routes.ts"
  "tests/fixtures/web-audit-score-parity.json=tests/fixtures/web-audit-score-parity.json"
)
SYNC_DIRS=(
  "tests/fixtures/web-audit-conformance=tests/fixtures/web-audit-conformance"
)

GIT_HARDEN_FLAGS=(
  -c credential.helper=
  -c core.askPass=
  -c protocol.allow=never
  -c protocol.https.allow=always
  -c http.followRedirects=false
)

hardened_git() {
  env -u GIT_SSH -u GIT_SSH_COMMAND -u GIT_PROXY_COMMAND -u GIT_ASKPASS -u GIT_EXEC_PATH \
    GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GIT_TERMINAL_PROMPT=0 \
    git "${GIT_HARDEN_FLAGS[@]}" "$@"
}

fail() {
  echo "error: $*" >&2
  exit 1
}

usage() {
  echo "usage: $0 [--check | --pin <sha>]" >&2
  exit 2
}

is_full_sha() {
  [[ $1 =~ ^[0-9a-f]{40}$ ]]
}

# === Arguments =========================================================
mode="update"
pin_arg=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --check)
      mode="check"
      shift
      ;;
    --pin)
      [[ $# -ge 2 ]] || usage
      pin_arg="$2"
      is_full_sha "$pin_arg" || fail "--pin wants a full 40-hex commit SHA (got ${pin_arg@Q})"
      shift 2
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      usage
      ;;
  esac
done
[[ $mode == "check" && -n $pin_arg ]] && usage

# === Resolve the pin ===================================================
sha=""
if [[ -n $pin_arg ]]; then
  sha="$pin_arg"
elif [[ -n ${WEB_AUDIT_SITE_SHA:-} ]]; then
  sha="$WEB_AUDIT_SITE_SHA"
elif [[ -f "$DEST_ROOT/$PIN_FILE" ]]; then
  read -r sha <"$DEST_ROOT/$PIN_FILE" || true
else
  fail "no pin: $DEST_ROOT/$PIN_FILE is missing; run with --pin <sha> or set WEB_AUDIT_SITE_SHA"
fi
is_full_sha "$sha" || fail "pin must be a full 40-hex commit SHA (got ${sha@Q})"

tmp_root="$(mktemp -d -t agentnative-web-audit-sync-XXXXXX)"
cleanup() {
  if [[ -n "$tmp_root" && -d "$tmp_root" ]]; then
    rm -rf "$tmp_root"
  fi
}
trap cleanup EXIT

# === Locate the commit: fetch by SHA, or read a local repository =======
src_repo=""
rev=""
if [[ -n ${WEB_AUDIT_SITE_REPO:-} ]]; then
  src_repo="$WEB_AUDIT_SITE_REPO"
  hardened_git -C "$src_repo" cat-file -e "$sha^{commit}" 2>/dev/null \
    || fail "commit $sha is not in $src_repo"
  rev="$sha"
  echo "reading $sha from local $src_repo"
else
  src_repo="$tmp_root/site"
  echo "fetching $sha from $WEB_AUDIT_SITE_REMOTE_URL..."
  hardened_git init --quiet "$src_repo"
  hardened_git -C "$src_repo" remote add origin "$WEB_AUDIT_SITE_REMOTE_URL"
  hardened_git -C "$src_repo" fetch --depth 1 --quiet origin "$sha" \
    || fail "cannot fetch commit $sha from $WEB_AUDIT_SITE_REMOTE_URL; the pin must name a commit on the site's dev branch"
  rev="FETCH_HEAD"
  fetched="$(hardened_git -C "$src_repo" rev-parse FETCH_HEAD)"
  [[ $fetched == "$sha" ]] || fail "fetched $fetched, expected $sha"
fi

# === Stage every vendored copy under $tmp_root/stage ===================
# File-based extraction throughout: variable capture would strip trailing
# newlines and break byte parity with the committed copies.
stage="$tmp_root/stage"
mkdir -p "$stage"
for entry in "${SYNC_FILES[@]}"; do
  src="${entry%%=*}"
  dest="${entry#*=}"
  mkdir -p "$stage/$(dirname "$dest")"
  hardened_git -C "$src_repo" show "$rev:$src" >"$stage/$dest" 2>/dev/null \
    || fail "commit $sha has no $src"
done
for entry in "${SYNC_DIRS[@]}"; do
  src="${entry%%=*}"
  dest="${entry#*=}"
  extract="$tmp_root/extract-$(basename "$dest")"
  mkdir -p "$extract" "$stage/$(dirname "$dest")"
  hardened_git -C "$src_repo" archive "$rev" -- "$src" 2>/dev/null | tar -x -C "$extract" \
    || fail "commit $sha has no $src/"
  [[ -d "$extract/$src" ]] || fail "commit $sha has no $src/"
  mv "$extract/$src" "$stage/$dest"
done
printf '%s\n' "$sha" >"$stage/$PIN_FILE"

# === Mode-specific behavior ============================================
case "$mode" in
  update)
    for entry in "${SYNC_FILES[@]}"; do
      dest="${entry#*=}"
      mkdir -p "$DEST_ROOT/$(dirname "$dest")"
      cp "$stage/$dest" "$DEST_ROOT/$dest"
      echo "wrote $dest"
    done
    for entry in "${SYNC_DIRS[@]}"; do
      dest="${entry#*=}"
      mkdir -p "$DEST_ROOT/$(dirname "$dest")"
      rm -rf "${DEST_ROOT:?}/$dest"
      cp -R "$stage/$dest" "$DEST_ROOT/$dest"
      echo "wrote $dest/"
    done
    if [[ -n $pin_arg || ! -f "$DEST_ROOT/$PIN_FILE" ]]; then
      cp "$stage/$PIN_FILE" "$DEST_ROOT/$PIN_FILE"
      echo "wrote $PIN_FILE ($sha)"
    fi
    echo
    echo "next: review \`git diff\` for unexpected changes, then commit."
    ;;
  check)
    drifted=()
    for entry in "${SYNC_FILES[@]}"; do
      dest="${entry#*=}"
      if ! cmp -s "$DEST_ROOT/$dest" "$stage/$dest"; then
        drifted+=("$dest")
      fi
    done
    for entry in "${SYNC_DIRS[@]}"; do
      dest="${entry#*=}"
      if ! diff -rq "$DEST_ROOT/$dest" "$stage/$dest" >/dev/null 2>&1; then
        drifted+=("$dest/")
      fi
    done
    if ! cmp -s "$DEST_ROOT/$PIN_FILE" "$stage/$PIN_FILE"; then
      drifted+=("$PIN_FILE")
    fi
    if [[ ${#drifted[@]} -eq 0 ]]; then
      echo "ok: every vendored web-audit copy matches agentnative-site $sha"
      exit 0
    fi
    echo "error: vendored web-audit copies drifted from agentnative-site $sha:" >&2
    for dest in "${drifted[@]}"; do
      echo "       $dest" >&2
    done
    echo "       run \`scripts/sync-web-audit.sh\` to refresh, then commit." >&2
    echo >&2
    for dest in "${drifted[@]}"; do
      diff -ru "$DEST_ROOT/${dest%/}" "$stage/${dest%/}" || true
    done
    exit 1
    ;;
esac
