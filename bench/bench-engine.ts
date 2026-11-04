// Times runWebAudit in-process (no Bun startup) for one target.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { SPEC_VERSION } from '../site/src/worker/spec-version.gen';
import { runWebAudit } from '../site/src/worker/audit-web/engine';
import { ALWAYS_ADMIT_BUDGET } from '../site/src/worker/audit-web/follow-requests';
import type { WebAuditRegistry } from '../site/src/worker/audit-web/registry';

const registry = JSON.parse(
  readFileSync(join(import.meta.dir, '..', 'site', 'dist', '_internal', 'web-audit-registry.json'), 'utf8'),
) as WebAuditRegistry;
const url = process.argv[2] ?? 'https://anc.dev/';
const t0 = performance.now();
const trace = process.env.ANC_WEB_TRACE !== undefined;
const tracedFetch: typeof fetch = async (input, init) => {
  const s = performance.now();
  const method = init?.method ?? 'GET';
  const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
  try {
    const res = await fetch(input, init);
    console.error(`TRACE\t${((s - t0) / 1000).toFixed(3)}\t${((performance.now() - s) / 1000).toFixed(3)}\t1\t${method}\t${url}\t${res.status}`);
    return res;
  } catch (err) {
    console.error(`TRACE\t${((s - t0) / 1000).toFixed(3)}\t${((performance.now() - s) / 1000).toFixed(3)}\t1\t${method}\t${url}\terr`);
    throw err;
  }
};
const audit = runWebAudit({ url, registry, siteType: null, specVersion: SPEC_VERSION, followDeclarations: true, domainBudget: ALWAYS_ADMIT_BUDGET, fetchOptions: trace ? { fetchImpl: tracedFetch } : {} });
let score = -1;
for (let ev = await audit.next(); !ev.done; ev = await audit.next()) {
  if (ev.value.type === 'complete') score = ev.value.scorecard.score_pct;
}
console.log(`${((performance.now() - t0) / 1000).toFixed(3)}\t${score}`);
