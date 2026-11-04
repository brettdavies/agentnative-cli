#!/usr/bin/env bash
# Counts that do not depend on the runner's load, one audit per variant per target under strace,
# then per-request time to headers from each engine's TRACE lines, two rounds.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1
OUT=bench/out
TARGETS=(https://anc.dev/ https://vercel.com/ https://supabase.com/ https://docs.stripe.com/ https://github.com/ https://modelcontextprotocol.io/)
printf 'engine\ttarget\tscore\ttcp_connects\tresolver_sends\tthreads\n' >"$OUT/trace.tsv"
mapfile -t ENGINES < <(awk 'NF {print $1}' bench/variants)
for t in "${TARGETS[@]}"; do
  for e in "${ENGINES[@]}"; do
    f=$(mktemp)
    timeout 120 strace -f -qq -e trace=connect,sendmmsg,sendto,clone3,clone -o "$f" "bin/$e" web "$t" --output json >"$f.json" 2>/dev/null
    tcp=$(grep -cE 'connect\(.*sin6?_port=htons\((443|80)\)' "$f")
    mmsg=$(grep -c 'sendmmsg(' "$f")
    spawned=$(grep -cE 'clone3?\(' "$f")
    printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$e" "$t" "$(jq -r '.score_pct // empty' "$f.json")" "$tcp" "$mmsg" "$spawned" >>"$OUT/trace.tsv"
  done
  if [ -d site ]; then
    f=$(mktemp)
    timeout 120 strace -f -qq -e trace=connect,sendmmsg,sendto,clone3,clone -o "$f" bun run bench/bench-engine.ts "$t" >"$f.json" 2>/dev/null
    tcp=$(grep -cE 'connect\(.*sin6?_port=htons\((443|80)\)' "$f")
    mmsg=$(grep -c 'sendmmsg(' "$f")
    spawned=$(grep -cE 'clone3?\(' "$f")
    printf 'bun\t%s\t%s\t%s\t%s\t%s\n' "$t" "$(cut -f2 "$f.json" | tail -1)" "$tcp" "$mmsg" "$spawned" >>"$OUT/trace.tsv"
  fi
done
cat "$OUT/trace.tsv"

for round in 1 2; do
  for t in "${TARGETS[@]}"; do
    h=${t#https://}
    h=${h%/}
    for e in "${ENGINES[@]}"; do
      ANC_WEB_TRACE=1 timeout 120 "bin/$e" web "$t" --output json 2>&1 >/dev/null | grep '^TRACE' >"$OUT/req-$e-$h-$round.tsv"
    done
    if [ -d site ]; then
      ANC_WEB_TRACE=1 timeout 120 bun run bench/bench-engine.ts "$t" 2>&1 >/dev/null | grep '^TRACE' >"$OUT/req-bun-$h-$round.tsv"
    fi
  done
done
python3 - "$OUT" <<'PY' | tee "$OUT/trace-summary.md" >>"${GITHUB_STEP_SUMMARY:-/dev/null}"
import glob, os, statistics as st, sys
out = sys.argv[1]
rows = {}
for f in sorted(glob.glob(os.path.join(out, 'req-*-*.tsv'))):
    name = os.path.basename(f)[len('req-'):-len('.tsv')]
    engine, rest = name.split('-', 1)
    host, rnd = rest.rsplit('-', 1)
    reqs = [l.rstrip('\n').split('\t') for l in open(f) if l.startswith('TRACE')]
    if not reqs:
        continue
    durs = [float(r[2]) for r in reqs]
    end = max(float(r[1]) + float(r[2]) for r in reqs)
    retries = sum(1 for r in reqs if r[3] != '1')
    rows.setdefault((host, engine), []).append((len(reqs), st.median(durs), sum(durs), end, retries))
print('\n| target | engine | requests | median s per request | summed request s | last header at s | retried |')
print('|---|---|---|---|---|---|---|')
for (host, engine), v in sorted(rows.items()):
    print(f"| {host} | {engine} | {st.median(x[0] for x in v):.0f} | {st.median(x[1] for x in v):.3f} | {st.median(x[2] for x in v):.2f} | {st.median(x[3] for x in v):.2f} | {sum(x[4] for x in v)} |")
PY
