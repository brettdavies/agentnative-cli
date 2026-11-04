#!/usr/bin/env bash
# Request-level traces: each engine logs one TRACE line per network request (offset, duration to headers, attempt).
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1
OUT=bench/out
TARGETS=(https://vercel.com/ https://supabase.com/ https://docs.stripe.com/ https://modelcontextprotocol.io/)
for round in 1 2; do
  for t in "${TARGETS[@]}"; do
    h=${t#https://}
    h=${h%/}
    ANC_WEB_TRACE=1 timeout 120 target/release/anc web "$t" --output json 2>&1 >/dev/null | grep '^TRACE' >"$OUT/trace-port-$h-$round.tsv"
    [ -x target-pool/release/anc ] && ANC_WEB_TRACE=1 timeout 120 target-pool/release/anc web "$t" --output json 2>&1 >/dev/null | grep '^TRACE' >"$OUT/trace-pool-$h-$round.tsv"
    ANC_WEB_TRACE=1 timeout 120 bun run bench/bench-engine.ts "$t" 2>&1 >/dev/null | grep '^TRACE' >"$OUT/trace-bun-$h-$round.tsv"
  done
done
python3 - "$OUT" <<'PY' | tee "$OUT/trace-summary.md" >>"${GITHUB_STEP_SUMMARY:-/dev/null}"
import glob, os, statistics as st, sys
out = sys.argv[1]
rows = {}
for f in sorted(glob.glob(os.path.join(out, 'trace-*-*.tsv'))):
    name = os.path.basename(f)[len('trace-'):-len('.tsv')]
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
