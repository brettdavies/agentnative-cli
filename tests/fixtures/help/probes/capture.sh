#!/usr/bin/env bash
# Take every capture ../index.json attributes to one image, inside that image.
#
# Usage: capture.sh <image> <out-dir>
#
#   <image>    A key of the index's `images` table: `scorer` or `probes`.
#   <out-dir>  Receives one file per fixture and `_exit.tsv` (file, exit code).
#
# The image is addressed by the ID the index records and is never pulled. The
# `probes` image is built from the Dockerfile beside this script:
#
#   docker build --provenance=false --tag anc-help-probes tests/fixtures/help/probes
#   docker image inspect --format '{{.Id}}' anc-help-probes
#
# Each capture runs as the scorer runs a target: the index's `probe_env` over the
# image's own environment, without a TTY, on the default network. The network
# stays on because `terraform stacks --help` prints the help of a plugin
# terraform downloads when the subcommand runs. A fixture the index marks
# `trimmed` comes out whole; its `kept` field says which lines the fixture keeps.
#
# Requires: docker, jaq (or jq)

set -euo pipefail

if [ $# -ne 2 ]; then
  echo "usage: capture.sh <image> <out-dir>" >&2
  exit 2
fi

image=$1
out=$2
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
index=$here/../index.json
jq=$(command -v jaq || command -v jq) || {
  echo "capture.sh: needs jaq or jq on PATH" >&2
  exit 2
}

id=$("$jq" -r --arg image "$image" '.images[$image].id // empty' "$index")
if [ -z "$id" ]; then
  echo "capture.sh: the index has no image named '$image'" >&2
  exit 2
fi

mapfile -t env_flags < <("$jq" -r '.probe_env | to_entries[] | "--env", "\(.key)=\(.value)"' "$index")

mkdir -p "$out"
"$jq" -r --arg image "$image" \
  '.fixtures[] | select(.image == $image) | [.file] + .argv | @tsv' "$index" \
  | docker run --rm --interactive --pull never "${env_flags[@]}" \
    --volume "$here/capture-inside.sh:/capture-inside.sh:ro" \
    --entrypoint /bin/bash "$id" /capture-inside.sh \
  | tar -xf - -C "$out"
