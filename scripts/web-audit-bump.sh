#!/usr/bin/env bash
# Move the vendored web-audit inputs to agentnative-site's dev head and open a
# PR for the move, when that head changes any of them.
#
# A bump that diverges from the site still lands green: the divergence ledger is
# regenerated, so each divergence becomes an entry owned by the site PR behind it,
# and a check this build cannot run is listed as port work owed. Port PRs clear
# both, and scripts/release/smoke.sh refuses a release while either remains. A
# failure neither absorbs still opens the PR, under a title that says so.
#
# Modes:
#   scripts/web-audit-bump.sh            Bump, test, and open or update the PR.
#   scripts/web-audit-bump.sh --dry-run  Bump and test; print the title and body,
#                                        open nothing.
#   scripts/web-audit-bump.sh --prepare <dir>
#                                        Bump and test; leave the tree changed and
#                                        write what web-audit-bump-publish.sh takes
#                                        into <dir>, so the suites run in a job the
#                                        token never reaches.
#   scripts/web-audit-bump.sh --decide [--on-dev-push]
#                                        Print bump=yes or bump=no and stop, with
#                                        no Rust toolchain. --on-dev-push also
#                                        answers no when the bump branch already
#                                        carries the site head and dev has touched
#                                        nothing the suites read since its base.
#
# "Nothing to bump" means the pin is on site dev and dev's head changes no vendored
# file. A pin off dev, such as a site PR's head, always moves onto dev, even when the
# bytes match, because that commit can vanish from the site's history.
#
# Every mode but --prepare leaves the vendored paths as it found them, and every mode
# refuses to start when one is dirty. sync-web-audit.sh owns what is vendored and how
# it is read; the only vendored-adjacent path named here is the divergence ledger.
#
# Env vars:
#   WEB_AUDIT_SITE_REPO        Local agentnative-site repository whose `dev` branch
#                              to read instead of cloning.
#   WEB_AUDIT_SITE_REMOTE_URL  Remote to clone.
#                              Default: https://github.com/brettdavies/agentnative-site.git
#   GITHUB_OUTPUT              --decide appends its answer here too, and --prepare
#                              appends ready=yes and the commit it prepared against.
#   GITHUB_REPOSITORY          Repository that holds the bump branch.
#                              Default: brettdavies/agentnative-cli
#   GH_TOKEN                   The token publishing runs with; see
#                              web-audit-bump-publish.sh.

set -euo pipefail

cd "$(dirname "$0")/.."
# shellcheck source=scripts/hardened-git.sh disable=SC1091
. scripts/hardened-git.sh

BOT_BRANCH="$(bash scripts/web-audit-bump-publish.sh --branch)"
SITE_REMOTE_URL="${WEB_AUDIT_SITE_REMOTE_URL:-https://github.com/brettdavies/agentnative-site.git}"
REPO="${GITHUB_REPOSITORY:-brettdavies/agentnative-cli}"
# Every suite that replays or reads the vendored inputs: the PR's own CI runs them
# too, so a narrower set here would title a red PR as a clean one.
WEB_SUITES=(--lib --test 'web_audit_*' --test build_registry --test sync_web_audit)

fail() {
  echo "error: $*" >&2
  exit 1
}

usage() {
  echo "usage: $0 [--dry-run | --prepare <dir> | --decide [--on-dev-push]]" >&2
  exit 2
}

mode="bump"
on_dev_push=""
prepared=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) mode="dry-run" ;;
    --decide) mode="decide" ;;
    --prepare)
      [[ $# -ge 2 ]] || usage
      mode="prepare"
      prepared="$2"
      shift
      ;;
    --on-dev-push) on_dev_push="yes" ;;
    *)
      echo "error: unknown argument: $1" >&2
      usage
      ;;
  esac
  shift
done
[[ -z $on_dev_push || $mode == "decide" ]] || usage

report_decision() {
  echo "bump=$1"
  if [[ -n ${GITHUB_OUTPUT:-} ]]; then
    echo "bump=$1" >>"$GITHUB_OUTPUT"
  fi
}

nothing_to_bump() {
  echo "nothing to bump: $1"
  if [[ $mode == "decide" ]]; then
    report_decision no
  fi
  exit 0
}

# When GitHub cannot say what changed, the answer that re-runs the suites is the safe one.
dev_touched_what_the_suites_read() {
  local branch_head branch_base touched
  branch_head="$(gh api "repos/$REPO/git/ref/heads/$BOT_BRANCH" --jq .object.sha)" || return 0
  branch_base="$(gh api "repos/$REPO/git/commits/$branch_head" --jq '.parents[0].sha')" || return 0
  touched="$(gh api "repos/$REPO/compare/$branch_base...$(git rev-parse HEAD)" --jq '.files[].filename')" || return 0
  grep -Eq '^(src/|tests/|scripts/|build\.rs$|Cargo\.)' <<<"$touched"
}

mapfile -t vendored < <(bash scripts/sync-web-audit.sh --list)
[[ ${#vendored[@]} -gt 1 ]] || fail "sync-web-audit.sh --list named no vendored path"
pin_file="${vendored[-1]}"
unset 'vendored[-1]'
mapfile -t tracked < <(bash scripts/web-audit-bump-publish.sh --paths)

if [[ -n $(git status --porcelain --untracked-files=all -- "${tracked[@]}") ]]; then
  fail "commit or discard the changes to the vendored paths first: a bump compares them with HEAD"
fi
read -r old <"$pin_file"

work="$(mktemp -d -t agentnative-web-audit-bump-XXXXXX)"
restore() {
  if [[ $mode != "prepare" ]]; then
    git restore --source=HEAD --staged --worktree -- "${tracked[@]}"
    git clean -fdq -- "${tracked[@]}"
  fi
  rm -rf "$work"
}
trap restore EXIT

# Whole history, single branch: the ledger generator walks old..new, and the
# clone costs seconds and 13 MB.
if [[ -z ${WEB_AUDIT_SITE_REPO:-} ]]; then
  WEB_AUDIT_SITE_REPO="$work/site"
  hardened_git clone --quiet --single-branch --branch dev --no-tags --no-checkout \
    "$SITE_REMOTE_URL" "$WEB_AUDIT_SITE_REPO"
fi
export WEB_AUDIT_SITE_REPO
new="$(hardened_git -C "$WEB_AUDIT_SITE_REPO" rev-parse --verify refs/heads/dev)" \
  || fail "no dev branch in $WEB_AUDIT_SITE_REPO"

if [[ $old == "$new" ]]; then
  nothing_to_bump "the pin already names site dev ${new:0:7}"
fi

on_dev=""
if hardened_git -C "$WEB_AUDIT_SITE_REPO" merge-base --is-ancestor "$old" "$new" 2>/dev/null; then
  on_dev="yes"
elif [[ $WEB_AUDIT_SITE_REPO == "$work/site" ]]; then
  # A single-branch clone lacks a PR head; the ledger generator walks from it.
  hardened_git -C "$WEB_AUDIT_SITE_REPO" fetch --quiet origin "$old" || true
fi

bash scripts/sync-web-audit.sh --pin "$new" >/dev/null
mapfile -t changed < <(git status --porcelain --no-renames --untracked-files=all -- "${vendored[@]}" | cut -c4-)
if [[ ${#changed[@]} -eq 0 && -n $on_dev ]]; then
  nothing_to_bump "site dev ${new:0:7} changes no vendored file since ${old:0:7}"
fi

if [[ $mode == "decide" ]]; then
  # Every dev merge would otherwise rebuild and restart the PR's CI for a head the PR
  # already has. Only a merge the suites can see is worth that; the daily run still
  # moves the PR onto dev's tip.
  if [[ -n $on_dev_push ]]; then
    carried="$(gh api "repos/$REPO/contents/$pin_file?ref=$BOT_BRANCH" \
      --header 'Accept: application/vnd.github.raw+json' 2>/dev/null)" || carried=""
    if [[ ${carried%$'\n'} == "$new" ]] && ! dev_touched_what_the_suites_read; then
      nothing_to_bump "$BOT_BRANCH already carries site dev ${new:0:7}, and dev touched nothing the suites read"
    fi
  fi
  report_decision yes
  exit 0
fi

ledger_status=0
WEB_AUDIT_UNPORTED_OUT="$work/unported.json" python3 -B scripts/web-audit-divergence-ledger.py --since "$old" \
  >"$work/ledger.log" 2>&1 || ledger_status=$?
suite_status=0
cargo test --features web --no-fail-fast "${WEB_SUITES[@]}" >"$work/suite.log" 2>&1 || suite_status=$?

title="feat(web): re-pin to agentnative-site dev ${new:0:7}"
if [[ $ledger_status -ne 0 || $suite_status -ne 0 ]]; then
  title+=" (web suites fail, port needed)"
fi
: >"$work/changed.txt"
if [[ ${#changed[@]} -gt 0 ]]; then
  printf '%s\n' "${changed[@]}" >"$work/changed.txt"
fi
git ls-files --deleted -- "${vendored[@]}" >"$work/deleted.txt"
python3 -B scripts/web-audit-bump-body.py \
  --old "$old" --new "$new" --changed "$work/changed.txt" --deleted "$work/deleted.txt" --ledger "${tracked[-1]}" \
  --unported "$work/unported.json" \
  --ledger-status "$ledger_status" --ledger-log "$work/ledger.log" \
  --suite-status "$suite_status" --suite-log "$work/suite.log" >"$work/body.md"

if [[ $mode == "dry-run" ]]; then
  printf 'title: %s\n\n' "$title"
  cat "$work/body.md"
  exit 0
fi

if [[ $mode == "bump" ]]; then
  prepared="$work/prepared"
fi
mkdir -p "$prepared/tree"
printf '%s\n' "$title" >"$prepared/title"
cp "$work/body.md" "$prepared/body.md"
for path in "${tracked[@]}"; do
  if [[ -e $path ]]; then
    mkdir -p "$prepared/tree/$(dirname "$path")"
    cp -R "$path" "$prepared/tree/$path"
  fi
done
if [[ $mode == "prepare" ]]; then
  echo "prepared: $title"
  if [[ -n ${GITHUB_OUTPUT:-} ]]; then
    {
      echo "ready=yes"
      echo "base=$(git rev-parse HEAD)"
    } >>"$GITHUB_OUTPUT"
  fi
  exit 0
fi
bash scripts/web-audit-bump-publish.sh "$prepared"
