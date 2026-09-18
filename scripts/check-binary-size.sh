#!/usr/bin/env bash
# Assert the release binary stays under its recorded ceiling.
#
# The local web audit brought a TLS stack, an HTTP client and a compiled
# regex engine into a binary that had none of them reachable, and that cost
# is the one thing a Rust-native port can silently lose. The ceiling is
# committed here so a dependency bump that eats the remaining headroom is a
# reviewed diff on this line rather than a surprise in a release artifact.
#
# Numbers below are bytes, measured with `cargo build --release` on
# x86_64-unknown-linux-gnu. Update them deliberately, in a PR that says
# what grew and why.
set -euo pipefail

# The `dev` binary immediately before the web audit landed (78b0a8e).
readonly BASELINE_BYTES=3517008
# The measured artifact with `anc web` reachable, plus headroom for a
# dependency bump: exceeding this is a decision, not an accident.
readonly CEILING_BYTES=10485760

BINARY="${1:-target/release/anc}"

if [[ ! -f "$BINARY" ]]; then
  printf 'error: %s not found; run "cargo build --release" first\n' "$BINARY" >&2
  exit 2
fi

size=$(wc -c <"$BINARY" | tr -d ' ')
delta=$((size - BASELINE_BYTES))
headroom=$((CEILING_BYTES - size))

# Integer math rather than bc: the script depends on coreutils alone, so it
# runs the same on a hosted runner and on a maintainer's machine.
mib() {
  printf '%d.%02d MiB' "$(($1 / 1048576))" "$((($1 % 1048576) * 100 / 1048576))"
}

printf 'release binary: %s bytes (%s)\n' "$size" "$(mib "$size")"
printf '  pre-web baseline: %s bytes\n' "$BASELINE_BYTES"
printf '  delta over baseline: %s bytes (%s)\n' "$delta" "$(mib "$delta")"
printf '  ceiling: %s bytes (%s); headroom: %s bytes\n' \
  "$CEILING_BYTES" "$(mib "$CEILING_BYTES")" "$headroom"

if ((size > CEILING_BYTES)); then
  printf 'error: the release binary is %s bytes over its ceiling\n' "$((size - CEILING_BYTES))" >&2
  printf '       read scripts/check-binary-size.sh before raising it\n' >&2
  exit 1
fi
