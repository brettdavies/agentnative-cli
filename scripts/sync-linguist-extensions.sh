#!/usr/bin/env bash
# Refresh src/web_audit/linguist-extensions.txt from GitHub Linguist's languages.yml,
# the corpus `anc <name>` reads to tell a filename from a host. Each line is the last
# label of an extension, lowercased, because routing reads a name by its last label.
# The file's first line names the commit it came from.
set -euo pipefail

readonly REPO="https://github.com/github-linguist/linguist"
readonly RAW="https://raw.githubusercontent.com/github-linguist/linguist"
readonly SOURCE="lib/linguist/languages.yml"
readonly DEST="src/web_audit/linguist-extensions.txt"
# Linguist has listed well over a thousand of these for years, so a shorter result
# is a truncated download, a captive portal's page, or a change to the file's shape.
readonly MIN_NAMES=1000

cd "$(git rev-parse --show-toplevel)"

sha=$(git ls-remote "$REPO" HEAD | cut -f1)
if ! printf '%s\n' "$sha" | grep -Eq '^[0-9a-f]{40}$'; then
  printf 'error: %s named no commit for HEAD; %s left as it was\n' "$REPO" "$DEST" >&2
  exit 1
fi

yml=$(mktemp)
tmp=$(mktemp)
trap 'rm -f "$yml" "$tmp"' EXIT

curl --fail --silent --show-error --location --proto '=https' --max-time 30 \
  "$RAW/$sha/$SOURCE" -o "$yml"

printf '# github-linguist/linguist@%s %s (MIT)\n' "$sha" "$SOURCE" >"$tmp"
awk '/^  extensions:/ { listed = 1; next } listed && /^  - / { print; next } { listed = 0 }' "$yml" \
  | sed -E 's/^  - "?//; s/"$//; s/.*\.//' \
  | tr '[:upper:]' '[:lower:]' \
  | { grep -E '^[a-z0-9-]+$' || true; } \
  | LC_ALL=C sort -u >>"$tmp"

names=$(sed 1d "$tmp" | wc -l | tr -d ' ')
if ((names < MIN_NAMES)); then
  printf 'error: %s holds %s extensions, fewer than %s; %s left as it was\n' "$SOURCE" "$names" "$MIN_NAMES" "$DEST" >&2
  exit 1
fi

cp "$tmp" "$DEST"
printf '%s: %s extensions, %s\n' "$DEST" "$names" "$(head -n 1 "$DEST")"
