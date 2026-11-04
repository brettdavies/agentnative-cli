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
const audit = runWebAudit({ url, registry, siteType: null, specVersion: SPEC_VERSION, followDeclarations: true, domainBudget: ALWAYS_ADMIT_BUDGET, fetchOptions: {} });
let score = -1;
for (let ev = await audit.next(); !ev.done; ev = await audit.next()) {
  if (ev.value.type === 'complete') score = ev.value.scorecard.score_pct;
}
console.log(`${((performance.now() - t0) / 1000).toFixed(3)}\t${score}`);
