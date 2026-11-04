#!/usr/bin/env bash
# Interleaved wall-clock benchmark: each round visits every target once per engine, rotating engine order.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1
OUT=bench/out
mkdir -p "$OUT"
ROUNDS=${ROUNDS:-5}
TARGETS=(https://anc.dev/ https://developers.cloudflare.com/ https://supabase.com/ https://docs.stripe.com/ https://vercel.com/ https://modelcontextprotocol.io/ https://mcp.linear.app/ https://github.com/ https://en.wikipedia.org/ https://huggingface.co/)
ENGINES=(port bun)
[ -x target-pool/release/anc ] && ENGINES+=(pool)
EMBED=$(ls bench/js-embed/target/release/anc-embed 2>/dev/null || true)
[ -n "$EMBED" ] && ENGINES+=(embed)
now() { date +%s.%N; }
printf 'engine\ttarget\tround\twall_s\tengine_s\tscore\trss_kb\n' >"$OUT/results.tsv"
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
        port) timeout 120 target/release/anc web "$t" --output json >"$f.json" 2>"$f.err" ;;
        pool) timeout 120 target-pool/release/anc web "$t" --output json >"$f.json" 2>"$f.err" ;;
        bun) timeout 120 bun run bench/bench-engine.ts "$t" >"$f.json" 2>"$f.err" ;;
        embed) timeout 120 "$EMBED" "$t" >"$f.json" 2>"$f.err" ;;
      esac
      w=$(echo "$(now) - $s" | bc)
      rss=""
      if [ $e = bun ]; then
        es=$(cut -f1 "$f.json" | tail -1)
        sc=$(cut -f2 "$f.json" | tail -1)
      else
        es=""
        sc=$(jq -r '.score_pct // empty' "$f.json" 2>/dev/null)
      fi
      printf '%s\t%s\t%s\t%.3f\t%s\t%s\t%s\n' "$e" "$h" "$round" "$w" "$es" "$sc" "$rss" >>"$OUT/results.tsv"
    done
  done
done
python3 - "$OUT/results.tsv" <<'PY' | tee "$OUT/summary.md" >>"${GITHUB_STEP_SUMMARY:-/dev/null}"
import csv, statistics as st, sys
rows = list(csv.DictReader(open(sys.argv[1]), delimiter='\t'))
engines = sorted({r['engine'] for r in rows})
targets = list(dict.fromkeys(r['target'] for r in rows))
med = lambda xs: st.median(xs) if xs else float('nan')
print('| target | ' + ' | '.join(f'{e} wall s' for e in engines) + ' | bun engine-only s | ' + ' | '.join(f'{e} score' for e in engines) + ' |')
print('|' + '---|' * (2 * len(engines) + 2))
tot = {e: 0.0 for e in engines}
for t in targets:
    w = {e: med([float(r['wall_s']) for r in rows if r['engine'] == e and r['target'] == t]) for e in engines}
    for e in engines: tot[e] += w[e]
    eo = med([float(r['engine_s']) for r in rows if r['engine'] == 'bun' and r['target'] == t and r['engine_s']])
    sc = {e: sorted({r['score'] for r in rows if r['engine'] == e and r['target'] == t}) for e in engines}
    print(f'| {t} | ' + ' | '.join(f'{w[e]:.2f}' for e in engines) + f' | {eo:.2f} | ' + ' | '.join(','.join(sc[e]) for e in engines) + ' |')
print('| TOTAL of medians | ' + ' | '.join(f'{tot[e]:.2f}' for e in engines) + ' | | |')
PY
