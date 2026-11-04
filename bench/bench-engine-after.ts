// Times runWebAudit in-process (no Bun startup) for one target.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { SPEC_VERSION } from '../site-after/src/worker/spec-version.gen';
import { runWebAudit } from '../site-after/src/worker/audit-web/engine';
import { ALWAYS_ADMIT_BUDGET } from '../site-after/src/worker/audit-web/follow-requests';
import type { WebAuditRegistry } from '../site-after/src/worker/audit-web/registry';

const registry = JSON.parse(
  readFileSync(join(import.meta.dir, '..', 'site-after', 'dist', '_internal', 'web-audit-registry.json'), 'utf8'),
) as WebAuditRegistry;
const url = process.argv[2] ?? 'https://anc.dev/';
const t0 = performance.now();
const audit = runWebAudit({ url, registry, siteType: null, specVersion: SPEC_VERSION, followDeclarations: true, domainBudget: ALWAYS_ADMIT_BUDGET });
let score = -1;
let links = '';
for (let ev = await audit.next(); !ev.done; ev = await audit.next()) {
  if (ev.value.type === 'complete') {
    score = ev.value.scorecard.score_pct;
    const row = ev.value.scorecard.results.find((r) => r.id === 'llms-txt-links');
    if (row !== undefined) links = `${row.status} ${row.evidence}`.replace(/\s+/g, ' ');
  }
}
console.log(`${((performance.now() - t0) / 1000).toFixed(3)}\t${score}\t${links}`);
