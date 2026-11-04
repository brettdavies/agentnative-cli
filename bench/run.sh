#!/usr/bin/env bash
# Interleaved wall clock: each round visits every target once per engine, rotating the engine order.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1
OUT=bench/out
mkdir -p "$OUT"
ROUNDS=${ROUNDS:-5}
TARGETS=(https://anc.dev/ https://developers.cloudflare.com/ https://supabase.com/ https://docs.stripe.com/ https://vercel.com/ https://modelcontextprotocol.io/ https://mcp.linear.app/ https://github.com/ https://en.wikipedia.org/ https://huggingface.co/)
mapfile -t ENGINES < <(awk 'NF {print $1}' bench/variants)
[ -d site ] && ENGINES+=(bun)
now() { date +%s.%N; }
printf 'engine\ttarget\tround\twall_s\tscore\trss_kb\n' >"$OUT/results.tsv"
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
      if [ "$e" = bun ]; then
        /usr/bin/time -f %M -o "$f.rss" timeout 120 bun run bench/bench-engine.ts "$t" >"$f.json" 2>"$f.err"
        sc=$(cut -f2 "$f.json" | tail -1)
      else
        /usr/bin/time -f %M -o "$f.rss" timeout 120 "bin/$e" web "$t" --output json >"$f.json" 2>"$f.err"
        sc=$(jq -r '.score_pct // empty' "$f.json" 2>/dev/null)
      fi
      w=$(echo "$(now) - $s" | bc)
      printf '%s\t%s\t%s\t%.3f\t%s\t%s\n' "$e" "$h" "$round" "$w" "$sc" "$(tail -1 "$f.rss")" >>"$OUT/results.tsv"
    done
  done
done
python3 - "$OUT/results.tsv" <<'PY' | tee "$OUT/summary.md" >>"${GITHUB_STEP_SUMMARY:-/dev/null}"
import csv, statistics as st, sys
rows = list(csv.DictReader(open(sys.argv[1]), delimiter='\t'))
engines = list(dict.fromkeys(r['engine'] for r in rows))
targets = list(dict.fromkeys(r['target'] for r in rows))
med = lambda xs: st.median(xs) if xs else float('nan')
print('| target | ' + ' | '.join(f'{e} s' for e in engines) + ' | scores |')
print('|' + '---|' * (len(engines) + 2))
tot = {e: 0.0 for e in engines}
for t in targets:
    w = {e: med([float(r['wall_s']) for r in rows if r['engine'] == e and r['target'] == t]) for e in engines}
    for e in engines:
        tot[e] += w[e]
    sc = {e: sorted({r['score'] for r in rows if r['engine'] == e and r['target'] == t}) for e in engines}
    print(f'| {t} | ' + ' | '.join(f'{w[e]:.2f}' for e in engines) + ' | ' + ' '.join(','.join(sc[e]) for e in engines) + ' |')
print('| TOTAL of medians | ' + ' | '.join(f'{tot[e]:.2f}' for e in engines) + ' | |')
rss = {e: med([int(r['rss_kb']) for r in rows if r['engine'] == e and r['rss_kb'].isdigit()]) for e in engines}
print('| median peak RSS KB | ' + ' | '.join(f'{rss[e]:.0f}' for e in engines) + ' | |')
PY
