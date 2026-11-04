#!/usr/bin/env bash
# Refuse a release whose `anc web` differs from anc.dev. A divergence ledger entry is
# an element the engine does not reproduce at the vendored site pin, an unported check
# skips wherever it applies, a pin off site dev names a commit that can vanish, and a
# pin behind a vendored change on site dev is a bump that has not landed. A web-audit
# bump lands the first two on dev on purpose, so the refusal belongs at the release and
# not in every PR's CI.
#
# Each refusal leads with a stable code and ends with its fix:
#   web-audit-ledger    the divergence ledger has entries
#   web-audit-unported  a vendored check binds Unsupported
#   web-audit-pin       the vendored pin is not reachable from site dev
#   web-audit-stale     site dev moved a vendored input past the pin
# RELEASES.md's "Handling a bump PR" section explains each one.
#
# Callers: scripts/release/smoke.sh in the manual preflight, and
# .github/workflows/web-audit-release-gate.yml on PRs to main.
#
# usage: scripts/release/web-parity-gate.sh --bin PATH
#
# Prints one `<pass|fail|skip><TAB><name><TAB><detail>` line per condition.
#
# Env vars:
#   WEB_AUDIT_SITE_REPO        Local agentnative-site repository whose `dev` branch to
#                              read instead of cloning.
#   WEB_AUDIT_SITE_REMOTE_URL  Remote to clone.
#                              Default: https://github.com/brettdavies/agentnative-site.git
#
# Exit codes:
#   0 = no condition failed
#   1 = a condition failed
#   2 = usage error

set -euo pipefail

if ((BASH_VERSINFO[0] < 4 || (BASH_VERSINFO[0] == 4 && BASH_VERSINFO[1] < 4))); then
  echo "error: bash >= 4.4 required, but this is bash ${BASH_VERSION:-unknown}. Install a newer bash: brew install bash" >&2
  exit 1
fi

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
WEB_LEDGER="tests/fixtures/web-audit-divergences.toml"
SITE_REMOTE_URL="${WEB_AUDIT_SITE_REMOTE_URL:-https://github.com/brettdavies/agentnative-site.git}"
DOCS="RELEASES.md#handling-a-bump-pr"
BUMP_PRS="https://github.com/brettdavies/agentnative-cli/pulls?q=is%3Apr+is%3Aopen+head%3Abot%2Fweb-audit-bump"

report() {
  printf '%s\t%s\t%s\n' "$1" "$2" "${3:-}"
  if [[ $1 == "fail" ]]; then
    failed="yes"
  fi
}

bin=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --bin)
      [[ $# -ge 2 ]] || {
        echo "usage: $0 --bin PATH" >&2
        exit 2
      }
      bin="$2"
      shift 2
      ;;
    *)
      echo "usage: $0 --bin PATH" >&2
      exit 2
      ;;
  esac
done
[[ -n $bin ]] || {
  echo "usage: $0 --bin PATH" >&2
  exit 2
}

failed=""
if [[ ! -d "$REPO_ROOT/src/web_audit" ]]; then
  report skip "web audit parity" "this release tree strips anc web, so nothing it ships can differ"
  exit 0
fi

# shellcheck source=scripts/hardened-git.sh disable=SC1091
. "$REPO_ROOT/scripts/hardened-git.sh"
work="$(mktemp -d -t anc-web-parity-XXXXXX)"
trap 'rm -rf "$work"' EXIT

if [[ ! -f "$REPO_ROOT/$WEB_LEDGER" ]]; then
  report fail "web-audit-ledger: the divergence ledger is missing" \
    "$WEB_LEDGER is missing while src/web_audit/ ships. Fix: restore it from dev. Docs: $DOCS"
else
  count="$(grep -c '^\[\[divergence\]\]$' "$REPO_ROOT/$WEB_LEDGER" || true)"
  if [[ $count -eq 0 ]]; then
    report pass "the divergence ledger has no entries"
  else
    owners="$(sed -n 's/^owner = "\(#[0-9]*\)"$/\1/p' "$REPO_ROOT/$WEB_LEDGER" | sort | uniq -c | sort -rn \
      | awk '{ printf "%s%s (%s)", sep, $2, $1; sep = ", " }')"
    report fail "web-audit-ledger: the divergence ledger holds $count entries" \
      "anc web differs from anc.dev at them, by owning site PR: $owners. Fix: port those site PRs and delete the entries they close, or hold the release. Docs: $DOCS"
  fi
fi

# A check with no `ported` field reads as unported: a binary too old to say cannot
# pass this gate by saying nothing.
checks="$work/checks.json"
if ! "$bin" emit web-checks >"$checks" 2>/dev/null; then
  report fail "web-audit-unported: cannot read the checks" \
    "$bin has no working \`emit web-checks\`. Fix: cargo build --features web, and pass that binary to --bin. Docs: $DOCS"
elif unported="$(python3 -c '
import json, sys
print("; ".join(
    check["id"] + " (handler " + check["handler"] + ", eval " + str(check.get("eval")) + ")"
    for check in json.load(open(sys.argv[1]))["checks"]
    if check.get("ported") is not True
))' "$checks" 2>/dev/null)"; then
  if [[ -z $unported ]]; then
    report pass "every vendored check runs in this build"
  else
    report fail "web-audit-unported: vendored checks bind Unsupported" \
      "$unported. Fix: port each handler kind or eval rule into src/web_audit/handlers/, or hold the release. Docs: $DOCS"
  fi
else
  report fail "web-audit-unported: cannot read the checks" \
    "could not read what \`$bin emit web-checks\` printed. Fix: cargo build --features web, and pass that binary to --bin. Docs: $DOCS"
fi

pin_file="$(bash "$REPO_ROOT/scripts/sync-web-audit.sh" --list | tail -n 1)"
read -r pin <"$REPO_ROOT/$pin_file" || [[ -n $pin ]]
site="${WEB_AUDIT_SITE_REPO:-}"
if [[ -z $site ]] && hardened_git clone --quiet --single-branch --branch dev --no-tags --no-checkout \
  "$SITE_REMOTE_URL" "$work/site" 2>/dev/null; then
  site="$work/site"
fi
if [[ -z $site ]]; then
  report fail "web-audit-pin: cannot read site dev" \
    "could not clone $SITE_REMOTE_URL to check pin $pin. Fix: rerun with network access, or set WEB_AUDIT_SITE_REPO. Docs: $DOCS"
elif hardened_git -C "$site" merge-base --is-ancestor "$pin" refs/heads/dev 2>/dev/null; then
  report pass "the site pin is on site dev"
  mapfile -t site_paths < <(bash "$REPO_ROOT/scripts/sync-web-audit.sh" --site-paths)
  moved="$(hardened_git -C "$site" diff --name-only "$pin" refs/heads/dev -- "${site_paths[@]}")"
  if [[ -z $moved ]]; then
    report pass "the pin carries site dev's vendored inputs"
  else
    report fail "web-audit-stale: site dev has moved vendored inputs past the pin" \
      "$(wc -l <<<"$moved" | tr -d ' ') files since $pin, first $(head -n 1 <<<"$moved"). Fix: merge the open bump PR ($BUMP_PRS), or run the bump workflow: gh workflow run web-audit-bump.yml --ref dev. Docs: $DOCS"
  fi
else
  # Until site dev holds what the sync reads, a bump run waits, so running it alone fixes nothing.
  undelivered=""
  while IFS= read -r site_path; do
    hardened_git -C "$site" cat-file -e "refs/heads/dev:$site_path" 2>/dev/null || undelivered="$site_path"
  done < <(bash "$REPO_ROOT/scripts/sync-web-audit.sh" --site-paths)
  if [[ -n $undelivered ]]; then
    report fail "web-audit-pin: the site pin is not on site dev" \
      "$pin is not reachable from site dev, and site dev has no $undelivered yet. Fix: land the site PR that carries it, then gh workflow run web-audit-bump.yml --ref dev and merge the PR it opens. Docs: $DOCS"
  else
    report fail "web-audit-pin: the site pin is not on site dev" \
      "$pin is not reachable from site dev. Fix: gh workflow run web-audit-bump.yml --ref dev, then merge the PR it opens. Docs: $DOCS"
  fi
fi

if [[ -n $failed ]]; then
  exit 1
fi
