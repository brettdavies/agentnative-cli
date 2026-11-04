#!/usr/bin/env bash
# Land the working tree's changes to the given paths as one signed commit on top
# of HEAD, move the bump branch onto it, and open or update that branch's PR to
# dev.
#
# usage: scripts/web-audit-bump-publish.sh <prepared-dir>
#        scripts/web-audit-bump-publish.sh --branch   Print the bump branch.
#        scripts/web-audit-bump-publish.sh --paths    Print the paths a bump may commit.
#
# <prepared-dir> is what `web-audit-bump.sh --prepare` wrote: title, body.md, and tree/,
# a copy of the vendored paths. It comes from a job that ran test code, so it is data:
# this checkout decides the branch and the paths, anything else in tree/ is ignored,
# and a symlink anywhere in it stops the run.
#
# dev requires verified signatures and a runner holds no key. GitHub signs what
# createCommitOnBranch creates; it leaves a REST `POST git/commits` unsigned. The
# commit is built on a scratch ref and the branch moves onto it in one forced
# update: resetting the branch to dev first would empty an open PR, and GitHub
# closes a PR whose head reaches its base.
#
# GH_TOKEN must be a PAT. Events GITHUB_TOKEN causes start no workflows, so a PR
# opened with it would never run its required checks.

set -euo pipefail

invoked_from="$PWD"
cd "$(dirname "$0")/.."

BRANCH="bot/web-audit-bump"
LEDGER="tests/fixtures/web-audit-divergences.toml"

mapfile -t paths < <(bash scripts/sync-web-audit.sh --list)
paths+=("$LEDGER")

case "${1:-}" in
  --branch)
    echo "$BRANCH"
    exit 0
    ;;
  --paths)
    printf '%s\n' "${paths[@]}"
    exit 0
    ;;
esac

[[ $# -eq 1 ]] || {
  echo "usage: $0 <prepared-dir> | --branch | --paths" >&2
  exit 2
}
prepared="$(cd "$invoked_from" && cd "$1" && pwd)"
read -r title <"$prepared/title"
body="$prepared/body.md"
branch="$BRANCH"

if [[ -n $(find "$prepared/tree" -type l -print -quit) ]]; then
  echo "error: the prepared tree holds a symlink; refusing to commit from it" >&2
  exit 1
fi
for path in "${paths[@]}"; do
  rm -rf "$path"
  if [[ -e "$prepared/tree/$path" ]]; then
    mkdir -p "$(dirname "$path")"
    cp -R "$prepared/tree/$path" "$path"
  fi
done

BASE="dev"
repo="${GITHUB_REPOSITORY:-brettdavies/agentnative-cli}"
scratch="$branch-next"
base_sha="$(git rev-parse HEAD)"

work="$(mktemp -d -t agentnative-web-audit-publish-XXXXXX)"
scratch_made=""
cleanup() {
  if [[ -n $scratch_made ]]; then
    gh api -X DELETE "repos/$repo/git/refs/heads/$scratch" >/dev/null 2>&1 || true
  fi
  rm -rf "$work"
}
trap cleanup EXIT

# A private index keeps the commit's content to the named paths whatever else the
# checkout holds, and its tree id is the one GitHub would compute.
export GIT_INDEX_FILE="$work/index"
git read-tree "$base_sha"
git add -A -- "${paths[@]}"
tree="$(git write-tree)"
git diff --cached --name-status --no-renames "$base_sha" >"$work/changes"
unset GIT_INDEX_FILE

pr="$(gh api "repos/$repo/pulls?head=${repo%%/*}:$branch&base=$BASE&state=open" --jq '.[0].number // empty')"
head="$(gh api "repos/$repo/git/ref/heads/$branch" --jq .object.sha 2>/dev/null)" || head=""

# A commit past the bump's own is someone's port work, which a forced update would
# destroy; it is theirs to carry to the next pin.
if [[ -n $pr && -n $head ]]; then
  ahead="$(gh api "repos/$repo/compare/$BASE...$head" --jq .ahead_by)"
  if [[ $ahead -gt 1 ]]; then
    echo "standing down: PR #$pr has $ahead commits on $branch, so a forced update would drop port work"
    exit 0
  fi
fi

commit_changes() {
  local status path
  : >"$work/additions.jsonl"
  : >"$work/deletions.jsonl"
  while IFS=$'\t' read -r status path; do
    if [[ $status == "D" ]]; then
      jq -n --arg p "$path" '{path: $p}' >>"$work/deletions.jsonl"
    else
      # Contents go through files: one argument caps at 128 KB on Linux.
      base64 <"$path" | tr -d '\n' >"$work/contents.b64"
      jq -n --arg p "$path" --rawfile c "$work/contents.b64" '{path: $p, contents: $c}' >>"$work/additions.jsonl"
    fi
  done <"$work/changes"
  jq -s . "$work/additions.jsonl" >"$work/additions.json"
  jq -s . "$work/deletions.jsonl" >"$work/deletions.json"
  jq -n \
    --arg repo "$repo" --arg branch "$scratch" --arg head "$base_sha" --arg msg "$title" \
    --slurpfile additions "$work/additions.json" --slurpfile deletions "$work/deletions.json" \
    '{query: "mutation($input: CreateCommitOnBranchInput!) { createCommitOnBranch(input: $input) { commit { oid } } }",
      variables: {input: {
        branch: {repositoryNameWithOwner: $repo, branchName: $branch},
        expectedHeadOid: $head,
        message: {headline: $msg},
        fileChanges: {additions: $additions[0], deletions: $deletions[0]}}}}' >"$work/payload.json"

  gh api -X DELETE "repos/$repo/git/refs/heads/$scratch" >/dev/null 2>&1 || true
  gh api -X POST "repos/$repo/git/refs" -f ref="refs/heads/$scratch" -f sha="$base_sha" >/dev/null
  scratch_made="yes"
  # GraphQL can answer HTTP 200 with an errors array, so the exit code alone
  # does not prove the commit exists.
  gh api graphql --input "$work/payload.json" >"$work/response.json"
  if jq -e '.errors' "$work/response.json" >/dev/null; then
    jq -r '.errors[].message' "$work/response.json" >&2
    exit 1
  fi
  local oid verified
  oid="$(jq -r '.data.createCommitOnBranch.commit.oid' "$work/response.json")"
  verified="$(gh api "repos/$repo/commits/$oid" --jq .commit.verification.verified)"
  if [[ $verified != "true" ]]; then
    echo "error: $oid is not verified, so dev's signature rule would refuse the PR" >&2
    exit 1
  fi
  if [[ -n $head ]]; then
    gh api -X PATCH "repos/$repo/git/refs/heads/$branch" -f sha="$oid" -F force=true >/dev/null
  else
    gh api -X POST "repos/$repo/git/refs" -f ref="refs/heads/$branch" -f sha="$oid" >/dev/null
  fi
  echo "moved $branch to $oid"
}

# The same tree on the same branch means nothing moved: pushing it again would
# only re-run the PR's CI.
if [[ -n $head && $(gh api "repos/$repo/git/commits/$head" --jq .tree.sha) == "$tree" ]]; then
  echo "$branch already holds this tree"
else
  commit_changes
fi

if [[ -n $pr ]]; then
  gh api -X PATCH "repos/$repo/pulls/$pr" -f title="$title" -F body=@"$body" --jq .html_url
else
  gh api -X POST "repos/$repo/pulls" -f title="$title" -f head="$branch" -f base="$BASE" -F body=@"$body" --jq .html_url
fi
