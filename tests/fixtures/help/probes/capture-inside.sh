#!/usr/bin/env bash
# Runs inside a capture image; capture.sh starts it.
#
# Reads `file<TAB>binary<TAB>arg...` rows on stdin and runs each row the way
# anc's BinaryRunner runs a target: the binary resolved on PATH to an absolute
# path, stdin closed, no TTY, stdout and stderr captured apart and written
# stdout first. Writes a tar of the captures plus `_exit.tsv` (file, exit code)
# to stdout. The probe environment arrives from `docker run --env`.

set -uo pipefail

work=$(mktemp -d)
out=$work/out
mkdir "$out"

while IFS=$'\t' read -r -a row; do
  file=${row[0]}
  if ! path=$(type -P "${row[1]}"); then
    echo "capture-inside.sh: ${row[1]} is not on PATH (wanted for $file)" >&2
    exit 1
  fi
  timeout 20 "$path" "${row[@]:2}" </dev/null >"$work/stdout" 2>"$work/stderr"
  printf '%s\t%s\n' "$file" "$?" >>"$out/_exit.tsv"
  cat "$work/stdout" "$work/stderr" >"$out/$file"
done

tar -cf - -C "$out" .
