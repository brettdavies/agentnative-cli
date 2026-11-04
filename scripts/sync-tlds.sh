#!/usr/bin/env bash
# Refresh src/web_audit/tlds-alpha-by-domain.txt from IANA's root-zone list, the
# allowlist `anc <name>` routes a bare dotted token against. The file's first
# line is IANA's own version stamp.
#
# A stale copy fails safe: a new top-level domain stays on the offline audit
# path until the refresh, and `anc web <name>` reaches it regardless.
set -euo pipefail

readonly URL="https://data.iana.org/TLD/tlds-alpha-by-domain.txt"
readonly DEST="src/web_audit/tlds-alpha-by-domain.txt"
# The root zone has held well over a thousand names since 2015, so a shorter
# file is a truncated download or a captive portal's page.
readonly MIN_NAMES=1000

cd "$(git rev-parse --show-toplevel)"

tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

curl --fail --silent --show-error --location --proto '=https' --max-time 30 "$URL" -o "$tmp"

if ! head -n 1 "$tmp" | grep -Eq '^# Version [0-9]+'; then
  printf 'error: %s has no version stamp on its first line; %s left as it was\n' "$URL" "$DEST" >&2
  exit 1
fi
# Not `grep -q`: it stops at the first match, which under pipefail kills sed and
# reads as no match once the file outgrows the pipe buffer.
if sed 1d "$tmp" | grep -Ev '^[A-Z0-9-]+$' >/dev/null; then
  printf 'error: %s holds a line that is not a top-level domain; %s left as it was\n' "$URL" "$DEST" >&2
  exit 1
fi
names=$(sed 1d "$tmp" | wc -l | tr -d ' ')
if ((names < MIN_NAMES)); then
  printf 'error: %s holds %s names, fewer than %s; %s left as it was\n' "$URL" "$names" "$MIN_NAMES" "$DEST" >&2
  exit 1
fi

cp "$tmp" "$DEST"
printf '%s: %s names, %s\n' "$DEST" "$names" "$(head -n 1 "$DEST")"
