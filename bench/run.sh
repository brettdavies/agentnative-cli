#!/usr/bin/env bash
# Interleaved wall-clock benchmark: each round visits every target once per engine, rotating engine order.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1
OUT=bench/out
mkdir -p "$OUT"
ROUNDS=${ROUNDS:-9}
TARGETS=(https://anc.dev/ https://developers.cloudflare.com/ https://supabase.com/ https://docs.stripe.com/ https://vercel.com/ https://modelcontextprotocol.io/ https://mcp.linear.app/ https://github.com/ https://en.wikipedia.org/ https://huggingface.co/)
ENGINES=(port-after port-before bun-after bun-before)
now() { date +%s.%N; }
printf 'engine\ttarget\tround\twall_s\tengine_s\tscore\tlinks\n' >"$OUT/results.tsv"
for round in $(seq "$ROUNDS"); do
  for t in "${TARGETS[@]}"; do
    h=${t#https://}
    h=${h%/}
    n=${#ENGINES[@]}
    off=$(((round - 1) % n))
    for i in $(seq 0 $((n - 1))); do
      e=${ENGINES[$(((i + off) % n))]}
      f="$OUT/$e-$h-$round"
      s=$(now)
      case $e in
        port-after) timeout 120 target/release/anc web "$t" --output json >"$f.json" 2>"$f.err" ;;
        port-before) timeout 120 target-before/release/anc web "$t" --output json >"$f.json" 2>"$f.err" ;;
        bun-after) SITE_DIR=site-after timeout 120 bun run bench/bench-engine-after.ts "$t" >"$f.json" 2>"$f.err" ;;
        bun-before) SITE_DIR=site-before timeout 120 bun run bench/bench-engine-before.ts "$t" >"$f.json" 2>"$f.err" ;;
      esac
      w=$(echo "$(now) - $s" | bc)
      case $e in
        bun-*)
          es=$(cut -f1 "$f.json" | tail -1)
          sc=$(cut -f2 "$f.json" | tail -1)
          li=$(cut -f3 "$f.json" | tail -1)
          ;;
        *)
          es=""
          sc=$(jq -r '.score_pct // empty' "$f.json" 2>/dev/null)
          li=$(jq -r '[.results[] | select(.id == "llms-txt-links") | "\(.status) \(.evidence)"] | first // ""' "$f.json" 2>/dev/null)
          ;;
      esac
      printf '%s\t%s\t%s\t%.3f\t%s\t%s\t%s\n' "$e" "$h" "$round" "$w" "$es" "$sc" "$li" >>"$OUT/results.tsv"
    done
  done
done
python3 bench/summary.py "$OUT/results.tsv" | tee "$OUT/summary.md" >>"${GITHUB_STEP_SUMMARY:-/dev/null}"
