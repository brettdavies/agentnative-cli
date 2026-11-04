import csv
import statistics as st
import sys

rows = list(csv.DictReader(open(sys.argv[1]), delimiter='\t'))
engines = ['port-before', 'port-after', 'bun-before', 'bun-after']
targets = list(dict.fromkeys(r['target'] for r in rows))
med = lambda xs: st.median(xs) if xs else float('nan')
walls = lambda e, t: [float(r['wall_s']) for r in rows if r['engine'] == e and r['target'] == t]
print('| target | ' + ' | '.join(f'{e} s' for e in engines) + ' | port after/before | port-after/bun-after | ' + ' | '.join(f'{e} score' for e in engines) + ' |')
print('|' + '---|' * (2 * len(engines) + 3))
tot = {e: 0.0 for e in engines}
for t in targets:
    w = {e: med(walls(e, t)) for e in engines}
    for e in engines:
        tot[e] += w[e]
    sc = {e: sorted({r['score'] for r in rows if r['engine'] == e and r['target'] == t}) for e in engines}
    print(f'| {t} | ' + ' | '.join(f'{w[e]:.2f}' for e in engines) + f" | {w['port-after'] / w['port-before']:.2f} | {w['port-after'] / w['bun-after']:.2f} | " + ' | '.join(','.join(sc[e]) for e in engines) + ' |')
print('| TOTAL of medians | ' + ' | '.join(f'{tot[e]:.2f}' for e in engines) + f" | {tot['port-after'] / tot['port-before']:.2f} | {tot['port-after'] / tot['bun-after']:.2f} | | | | |")
print()
print(f"KD18 speed gate, port-after / bun-after sum of medians: {tot['port-after'] / tot['bun-after']:.3f} (must be <= 1.05)")
print()
print('Links row stalls: rounds whose llms-txt-links evidence reads a timeout or an exhausted budget, per target with any.')
print()
print('| target | ' + ' | '.join(engines) + ' |')
print('|' + '---|' * (len(engines) + 1))
stalled = lambda r: 'budget exhausted' in r['links'] or 'TimeoutError' in r['links']
for t in targets:
    counts = {e: (sum(1 for r in rows if r['engine'] == e and r['target'] == t and stalled(r)), len(walls(e, t))) for e in engines}
    if any(c[0] for c in counts.values()):
        print(f'| {t} | ' + ' | '.join(f'{counts[e][0]}/{counts[e][1]}' for e in engines) + ' |')
print()
print('Each round per engine, in seconds, for the targets the medians turn on.')
for t in ('vercel.com', 'anc.dev'):
    print()
    print(f'**{t}**')
    print()
    for e in engines:
        print(f"- {e}: {' '.join(f'{x:.2f}' for x in walls(e, t))}")
