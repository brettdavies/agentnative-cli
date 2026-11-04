#!/usr/bin/env bash
# Builds each line of bench/variants: `<name> <ref>[+<ref>...] [patch[,patch...]]`, the refs merged in order,
# the request trace patched in, and bench/patches/<patch>.py applied, into bin/<name>, sharing one
# target directory.
set -euo pipefail
root=$PWD
mkdir -p bin bench/out
git config user.name bench
git config user.email bench@invalid
printf 'variant\tbytes\n' >bench/out/sizes.tsv
while read -r name refs patch; do
  [ -n "$name" ] || continue
  dir=$RUNNER_TEMP/variant-$name
  first=${refs%%+*}
  git worktree add -q --detach "$dir" "origin/$first"
  if [ "$refs" != "$first" ]; then
    rest=${refs#*+}
    for ref in ${rest//+/ }; do git -C "$dir" merge -q --no-edit "origin/$ref"; done
  fi
  (cd "$dir" && python3 "$root/bench/patches/trace.py")
  for one in ${patch//,/ }; do
    (cd "$dir" && python3 "$root/bench/patches/$one.py")
  done
  (cd "$dir" && CARGO_TARGET_DIR=$root/target-shared cargo build --release --features web)
  cp target-shared/release/anc "bin/$name"
  printf '%s\t%s\n' "$name" "$(wc -c <"bin/$name")" >>bench/out/sizes.tsv
done <bench/variants
cat bench/out/sizes.tsv
