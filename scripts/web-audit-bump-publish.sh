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
# and a symlink anywhere in the directory, or a missing part, stops the run before
# this job's token reads or sends any of it.
#
# dev requires verified signatures and a runner holds no key. GitHub signs what
# createCommitOnBranch creates; it leaves a REST `POST git/commits` unsigned. The
# commit is built on a scratch ref and the branch moves onto it in one forced
# update: resetting the branch to dev first would empty an open PR, and GitHub
# closes a PR whose head reaches its base.
#
# GitHub answers createCommitOnBranch with a 504 past about ten seconds, and that commit
# does not land. Measured: 279 files in a 9.6 MiB request took 6.7 s on one try and timed
# out on another, while 80 files in 2.8 MiB took 3.4 s. A new check moves every golden, so
# a bump lands as several commits under those sizes. The PR squash-merges into one.
#
# GH_TOKEN must be a PAT. Events GITHUB_TOKEN causes start no workflows, so a PR
# opened with it would never run its required checks.

set -euo pipefail

if ((BASH_VERSINFO[0] < 4 || (BASH_VERSINFO[0] == 4 && BASH_VERSINFO[1] < 4))); then
  echo "error: bash >= 4.4 required, but this is bash ${BASH_VERSION:-unknown}. Install a newer bash: brew install bash" >&2
  exit 1
fi

invoked_from="$PWD"
cd "$(dirname "$0")/.."

BRANCH="bot/web-audit-bump"
MAX_FILES_PER_COMMIT=100
MAX_BYTES_PER_COMMIT=$((3 * 1024 * 1024))
# In the body of every commit this script makes.
COMMIT_MARK="Web-Audit-Bump: part"
LEDGER="tests/fixtures/web-audit-divergences.toml"

mapfile -t paths < <(bash scripts/sync-web-audit.sh --list)
pin_file="${paths[-1]}"
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

if [[ -n $(find "$prepared" -type l -print -quit) ]]; then
  echo "error: the prepared directory holds a symlink; refusing to read it" >&2
  exit 1
fi
# A tree without the pin would read as a bump that deletes every vendored path.
if [[ ! -f $prepared/title || ! -f $prepared/body.md || ! -f $prepared/tree/$pin_file ]]; then
  echo "error: $prepared lacks title, body.md, or tree/$pin_file; refusing to publish from it" >&2
  exit 1
fi
read -r title <"$prepared/title"
read -r new_pin <"$prepared/tree/$pin_file" || [[ -n $new_pin ]]
body="$prepared/body.md"
branch="$BRANCH"

BASE="dev"
repo="${GITHUB_REPOSITORY:-brettdavies/agentnative-cli}"

# The first job runs for most of an hour. A merge inside that window leaves this pin on
# dev, and a PR opened for it would never close on its own.
dev_pin="$(gh api "repos/$repo/contents/$pin_file?ref=$BASE" \
  --header 'Accept: application/vnd.github.raw+json' 2>/dev/null)" || dev_pin=""
if [[ ${dev_pin%$'\n'} == "$new_pin" ]]; then
  echo "$BASE already holds pin ${new_pin:0:7}; nothing to publish"
  exit 0
fi

for path in "${paths[@]}"; do
  rm -rf "$path"
  if [[ -e "$prepared/tree/$path" ]]; then
    mkdir -p "$(dirname "$path")"
    cp -R "$prepared/tree/$path" "$path"
  fi
done

scratch="$branch-next"
base_sha="$(git rev-parse HEAD)"

# A base holding commits dev lacks would carry them into the bump PR, and the next run
# would read them as port work and stand down for good.
past_dev="$(gh api "repos/$repo/compare/$BASE...$base_sha" --jq .ahead_by)"
if [[ $past_dev -ne 0 ]]; then
  echo "error: $base_sha holds $past_dev commits $BASE lacks; run a bump from $BASE" >&2
  exit 1
fi

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
git diff --cached --name-status --no-renames -z "$base_sha" >"$work/changes"
unset GIT_INDEX_FILE

pr="$(gh api "repos/$repo/pulls?head=${repo%%/*}:$branch&base=$BASE&state=open" --jq '.[0].number // empty')"
head="$(gh api "repos/$repo/git/ref/heads/$branch" --jq .object.sha 2>/dev/null)" || head=""

# A commit past the bump's own is someone's port work, which a forced update would
# destroy whether or not a PR is open; it is theirs to carry to the next pin. The bump's
# own are the ones GitHub committed: a local amend keeps the mark and loses that, and
# GitHub's "Update branch" merge is the only merge that is not a person's hand. A branch
# a merged PR left behind holds nothing a forced update could lose.
if [[ -n $head ]]; then
  port="$(gh api "repos/$repo/compare/$BASE...$head" \
    --jq "[.commits[] | select((.commit.committer.name != \"GitHub\") or ((.parents | length == 1) and (.commit.message | test(\"\\n$COMMIT_MARK [0-9]+\\\\s*\$\") | not)))] | length")"
  if [[ $port -gt 0 ]]; then
    merged="$(gh api "repos/$repo/pulls?head=${repo%%/*}:$branch&base=$BASE&state=closed" \
      --jq "[.[] | select(.merged_at != null and .head.sha == \"$head\")] | length")"
    if [[ $merged -eq 0 ]]; then
      echo "::warning::standing down: $branch has $port commits past $BASE that are not the bump's own, so a forced update would drop port work; delete the branch to start over"
      exit 0
    fi
  fi
fi

# GraphQL can answer HTTP 200 with an errors array, so the exit code alone does not
# prove the mutation landed.
graphql() {
  gh api graphql --input "$1" >"$work/response.json"
  if jq -e '.errors' "$work/response.json" >/dev/null; then
    jq -r '.errors[].message' "$work/response.json" >&2
    exit 1
  fi
}

# Lands what the batch files hold as one verified commit on the scratch ref, on top of
# $tip, and leaves them empty for the next batch.
commit_batch() {
  part=$((part + 1))
  jq -s . "$work/additions.jsonl" >"$work/additions.json"
  jq -s . "$work/deletions.jsonl" >"$work/deletions.json"
  jq -n \
    --arg repo "$repo" --arg branch "$scratch" --arg head "$tip" --arg msg "$title" --arg body "$COMMIT_MARK $part" \
    --slurpfile additions "$work/additions.json" --slurpfile deletions "$work/deletions.json" \
    '{query: "mutation($input: CreateCommitOnBranchInput!) { createCommitOnBranch(input: $input) { commit { oid } } }",
      variables: {input: {
        branch: {repositoryNameWithOwner: $repo, branchName: $branch},
        expectedHeadOid: $head,
        message: {headline: $msg, body: $body},
        fileChanges: {additions: $additions[0], deletions: $deletions[0]}}}}' >"$work/payload.json"
  graphql "$work/payload.json"
  tip="$(jq -r '.data.createCommitOnBranch.commit.oid' "$work/response.json")"
  local verified
  verified="$(gh api "repos/$repo/commits/$tip" --jq .commit.verification.verified)"
  if [[ $verified != "true" ]]; then
    echo "error: $tip is not verified, so dev's signature rule would refuse the PR" >&2
    exit 1
  fi
  : >"$work/additions.jsonl"
  : >"$work/deletions.jsonl"
  batch_files=0
  batch_bytes=0
}

commit_changes() {
  local status path size
  : >"$work/additions.jsonl"
  : >"$work/deletions.jsonl"
  batch_files=0
  batch_bytes=0
  part=0
  tip="$base_sha"
  gh api -X DELETE "repos/$repo/git/refs/heads/$scratch" >/dev/null 2>&1 || true
  gh api -X POST "repos/$repo/git/refs" -f ref="refs/heads/$scratch" -f sha="$base_sha" >/dev/null
  scratch_made="yes"
  # -z, because git quotes a path holding a non-ASCII byte, and base64 would not find it.
  while IFS= read -r -d '' status && IFS= read -r -d '' path; do
    size=0
    if [[ $status != "D" ]]; then
      # Contents go through files: one argument caps at 128 KB on Linux.
      base64 <"$path" | tr -d '\n' >"$work/contents.b64"
      size="$(wc -c <"$work/contents.b64")"
    fi
    if ((batch_files > 0 && (batch_files >= MAX_FILES_PER_COMMIT || batch_bytes + size > MAX_BYTES_PER_COMMIT))); then
      commit_batch
    fi
    if [[ $status == "D" ]]; then
      jq -n --arg p "$path" '{path: $p}' >>"$work/deletions.jsonl"
    else
      jq -n --arg p "$path" --rawfile c "$work/contents.b64" '{path: $p, contents: $c}' >>"$work/additions.jsonl"
    fi
    batch_files=$((batch_files + 1))
    batch_bytes=$((batch_bytes + size))
  done <"$work/changes"
  commit_batch
  if [[ -n $head ]]; then
    # beforeOid makes the forced move conditional: a port commit pushed since the read
    # above refuses it, where a REST forced update would drop that commit.
    jq -n --arg id "$(gh api "repos/$repo" --jq .node_id)" --arg name "refs/heads/$branch" \
      --arg before "$head" --arg after "$tip" \
      '{query: "mutation($input: UpdateRefsInput!) { updateRefs(input: $input) { clientMutationId } }",
        variables: {input: {repositoryId: $id, refUpdates: [{name: $name, beforeOid: $before, afterOid: $after, force: true}]}}}' \
      >"$work/move.json"
    graphql "$work/move.json"
  else
    gh api -X POST "repos/$repo/git/refs" -f ref="refs/heads/$branch" -f sha="$tip" >/dev/null
  fi
  echo "moved $branch to $tip in $part commits"
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
