#!/usr/bin/env node
// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Regression tests for the Vale rule packs.
//
// Spawns `vale` against each fixture under scripts/__fixtures__/prose-check/
// and asserts that the expected rule (e.g., Brand.MarketingRegister) fired
// at least once. Collateral rule firings are allowed at v1 — fixtures
// optimize for "this rule must catch its target", not "no other rule
// touches this fixture". Strict one-rule-per-fixture cleanup is a
// follow-up.
//
// Invoked manually by developers; not wired into pre-push (see U7 of the
// prose-check stack plan for the rationale — the orchestrator already
// runs once per push, running fixtures via the orchestrator would double
// the work).
//
// Exits 0 on all-pass, 1 on any case failure.

import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const FIXTURES = path.join(HERE, "__fixtures__/prose-check");

const CASES = [
  { name: "marketing-register", expect: /brand\.MarketingRegister/ },
  { name: "hedge-words", expect: /brand\.HedgeWords/ },
  { name: "filler-adjectives", expect: /brand\.FillerAdjectives/ },
  { name: "rfc-keywords", expect: /spec\.RFCKeywords/ },
  { name: "first-person-plural", expect: /spec\.FirstPersonPlural/ },
  { name: "second-person-imperative", expect: /spec\.SecondPersonImperative/ },
];

let failed = 0;
for (const c of CASES) {
  const fixture = path.join(FIXTURES, c.name, "case.md");
  const res = spawnSync(
    "vale",
    ["--no-global", "--output=line", "--minAlertLevel=warning", fixture],
    { encoding: "utf8" },
  );
  const combined = (res.stdout ?? "") + (res.stderr ?? "");
  const ok = c.expect.test(combined);
  if (ok) {
    console.log(`  pass  ${c.name}`);
  } else {
    failed += 1;
    console.error(`  FAIL  ${c.name}`);
    console.error(`        expected substring ${c.expect}`);
    console.error(`        vale exit=${res.status}`);
    console.error(`        output:\n${combined.split("\n").map((l) => `          ${l}`).join("\n")}`);
  }
}

if (failed > 0) {
  console.error(`\ntest-prose-check: ${failed}/${CASES.length} case(s) failed`);
  process.exit(1);
}
console.log(`\ntest-prose-check: ${CASES.length}/${CASES.length} OK`);
