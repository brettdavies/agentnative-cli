---
title: "feat: p2-structured-output Rust tiering + Python source sibling"
type: feat
status: proposed
date: 2026-05-01
deepened: 2026-05-01
origin: docs/brainstorms/2026-05-01-p2-json-output-upgrade-requirements.md
---

# feat: p2-structured-output Rust tiering + Python source sibling

## Summary

Tighten the Rust source audit `p2-structured-output` from "enum exists" to a tiered Strong / Medium / Weak detection
that walks from a clap-bound `OutputFormat::Json` variant to a reachable `serde_json` call site, and ship a Python
source sibling that mirrors the Rust shape via argparse `choices=` / click `Choice([...])` plus reachable
`json.dumps`/`json.dump`. Verdict shifts on already-scored tools land deliberately, anchored by a source fixture matrix
that lands **before** the detection widens. The behavioral half of the origin (R1 to R3) is answered outside this plan:
`.anc.toml [p2] json_probe` (#147) runs a declared probe, and a miss scores Skip (#178).

---

## Problem Frame

`p2-json-output` reaches JSON only through its safe-suffix probes (`--help` / `--version`), which most CLIs answer in
text whatever the output flag says. When no safe probe prints JSON, the row is Skip and unscored, and a `[p2]
json_probe` declaration in `.anc.toml` runs a declared probe in its place (#147, #178). A tool with no declaration gets
no behavioral evidence for its JSON mode, and the declaration answers that. The source layer is where this plan works:
the Rust source audit already covers the same requirement (`p2-must-output-flag`) but stops at "enum exists", and
source layer can do strictly more than behavioral via reachability and flow analysis. See origin:
`docs/brainstorms/2026-05-01-p2-json-output-upgrade-requirements.md`.

---

## Requirements

Carried verbatim from origin requirements doc. R-IDs stable. R1 to R3 are answered outside this plan, and no unit
builds them: #147 adds `.anc.toml [p2] json_probe`, which runs a declared probe when no safe suffix prints JSON, and #178
scores a miss as Skip. This plan builds R4 to R9, with R11, R12, R13 and R14 in support.

- R1. Add a bad-arg-VALUE probe to `validate_json_output()` as a third evidence path inside the existing
  `p2-json-output` audit. Probe shape: inject a deliberately-invalid value for the detected `--output`/`--format` flag;
  parse stderr for the declared value enumeration. Answered by #147 and #178; not built here.
- R2. Verdict semantics shift in-place (same audit ID, same `covers()`). Pass at `Confidence::Medium` when the
  value-enum echo confirms `json` is a declared accepted value (`p1-env-hints` v0.1.3 widening precedent — widening
  detection does not raise confidence). Answered by #147 and #178; not built here.
- R3. When the bad-arg-VALUE probe cannot fire, fall back to the existing safe-suffix probes and emit the existing Skip
  evidence message. No regression on currently-passing tools. Answered by #147 and #178; not built here.
- R4. Replace the binary "enum exists or not" detection in `p2-structured-output` (Rust) with a tiered model: Strong
  (Confidence::High) when the OutputFormat enum has a `Json` variant AND a clap-derive field references it AND a
  `serde_json` serialization call site is reachable from the match arm gating that variant; Medium (Confidence::Medium)
  when enum + clap reference + any `serde_json` call in the same crate; Weak / Warn when only the enum exists.
- R5. Tools previously passing at `Confidence::High` purely on enum existence shift to a tier consistent with their
  actual reachability. Shifts anchored by the fixture matrix (R12) and enumerated in the PR description.
- R6. Audit ID stays `p2-structured-output`; `covers()` stays `&["p2-must-output-flag"]`. No new requirement IDs added
  to `src/principles/registry.rs`.
- R7. Add a Python source-layer audit applicable when `Project::language` is Python. Detects argparse
  `add_argument(..., choices=[..., 'json', ...])` / click `Choice([..., 'json', ...])` declarations + a `json.dumps` /
  `json.dump` call site reachable from the dispatch on that argument's value.
- R8. Detection produces the same tiered Strong / Medium / Weak shape as Rust (R4), with the same Confidence levels and
  the same Warn message when only the choice declaration exists without a `json.dumps` call site.
- R9. The Python audit declares `covers() = &["p2-must-output-flag"]` and registers in
  `src/audits/source/python/mod.rs`. The `dangling_cover_ids` test passes after the addition.
- R10. `src/scorecard/audience.rs::SIGNAL_AUDIT_IDS` is unchanged. Neither source audit is a signal audit, so audience
  labels do not move.
- R11. No scorecard schema bump. `schema_version` stays at its current value.
- R12. A source fixture matrix anchors verdict shifts before merge: source fixtures cover Rust tools at each tier
  (Strong / Medium / Weak) and Python tools at each tier. Each fixture documents its expected verdict and
  Confidence before the detection lands.
- R13. `tests/dogfood.rs` updates to reflect the tier `p2-structured-output` resolves to against `anc` itself. **The
  concrete Confidence value for `p2-structured-output` depends on the Strong-tier resolution from `Open Questions →
  Deferred from Document Review`** — `Confidence::High` if option (b) "any same-crate `serde_json`" is chosen,
  `Confidence::Medium` if option (a) "within-arm strict" is chosen (anc's match arm calls `format_json` in `scorecard/`,
  not `serde_json` directly).
- R14. `docs/coverage-matrix.md` and `coverage/matrix.json` are regenerated via `anc emit coverage-matrix` as part of
  the same PR. The integration test `test_generate_coverage_matrix_drift_check_passes_on_committed_artifacts` passes.

---

## Scope Boundaries

Carried from origin. Anything below is explicitly NOT in this plan.

- No new requirement IDs added to `agentnative-spec`. A+B' attest the existing `p2-must-output-flag` MUST more
  rigorously; no spec PR.
- No `agentnative-spec` version bump. Vendored spec text unchanged.
- No scorecard schema bump. Envelope contract unchanged.
- No self-declared manifest field opt-in (`[package.metadata.agentnative]` — ideation D). Deferred.
- No `<tool> agentnative-probe` spec convention (ideation F). Long-term ecosystem move.
- No spec MUST rewriting (ideation E, adversarial-rejected: doctrine downgrades prose to preference, not level of MUST).
- No Skip-with-evidence verdict change (ideation G). #178 scores a `p2-json-output` miss as Skip.
- No change to `src/scorecard/audience.rs::SIGNAL_AUDIT_IDS`.
- No CommandFactory build-time shim (ideation F6.9). Cross-compilation cost too high.
- No `jc` external-wrapper fallback (ideation F2.7 / F6.11). Subject-tangential.
- No change to `p2-json-output`. R1 to R3 are answered by #147 and #178.
- No coupling with the output-envelope plan (`docs/plans/2026-04-30-001`).
- No third source language (Go, Bun, etc.) at this iteration. Rust + Python are the source-audit launch languages per
  `Cargo.toml` feature flags.

---

## Context & Research

### Relevant Code and Patterns

- `src/audits/source/rust/structured_output.rs` — current Rust source audit, will be tightened in U2. Helper
  `audit_structured_output(source: &str) -> AuditStatus` at line 87 is the unit-testable core; new tier helper functions
  follow the same shape per CLAUDE.md "Source Audit Convention".
- `src/audits/source/python/no_color.rs` — closest pattern for the new Python source sibling (U3). Uses
  `ast_grep_core::Pattern::try_new` + `Python.ast_grep(source).root().find(&pattern)`. Multi-pattern OR-fallback in
  `source_handles_no_color()` at line 82 is the shape U3 mirrors for argparse / click detection.
- `src/source.rs` — cross-language pattern helpers (`has_pattern_in`, `find_pattern_matches_in`,
  `has_string_literal_in`). The Python sibling reuses these rather than writing private per-language helpers.
- `src/principles/matrix.rs:96-129` — `build()` implements covers()-OR coverage logic at the requirement layer. This is
  shipped today; no change needed in U3 — the matrix already credits the requirement when either covering audit passes.

### Institutional Learnings

- `docs/solutions/best-practices/cli-env-var-shape-heuristic-2026-04-21.md` — `p1-env-hints` v0.1.2 → v0.1.3 widening
  precedent. Two regressions surfaced post-merge — only fixture-driven leaderboard anchoring caught them. **The U4-
  before-U2/U3 sequencing rule is direct guidance from this learning.**
- `docs/solutions/best-practices/reliable-static-analysis-compliance-auditors-20260327.md` — SRP-per-audit doctrine.
  Each language's source audit stays its own audit with its own verdict.
- `docs/solutions/best-practices/behavioral-vs-structural-must-when-authoring-spec-requirements-20260420.md` — when a
  behavioral audit can't safely attest, ADD a source-layer sibling. The Python sibling (U3) is direct compliance.
- `docs/solutions/architecture-patterns/aggregate-verdicts-are-informational-not-authoritative-20260420.md` —
  covers()-OR is at the requirement layer; per-audit verdicts stay independent. U2 tightening Rust does not aggregate
  with `p2-json-output`; both verdicts surface honestly side-by-side on every run.
- `docs/solutions/best-practices/audit-scripts-as-documentation-immune-system-2026-04-20.md` — doctrine that downgrades
  PROSE to preference, not LEVEL of MUST. Validates rejecting ideation E (spec-side narrowing).

### External References

Not gathered — local patterns are direct, ast-grep pattern shapes are explicitly deferred to implementation per R4/R7.

---

## Key Technical Decisions

Carried from origin's locked decisions table; plan-time additions are flagged. **Do not relitigate the carried decisions
during implementation unless contradicting evidence surfaces.**

- **`p2-json-output` is not changed here.** Its safe-suffix probes and the declared `[p2] json_probe` (#147) stand,
  and a miss scores Skip (#178).
- **Source-layer detection becomes asymmetrically more nuanced than behavioral via reachability.** Behavioral keeps its
  current `Confidence`; source can earn `Confidence::High` when reachability is provable.
  (origin: Key Decisions §2)
- **No scorecard / spec version bump.** Verdict shifts on already-scored tools are normal release-note territory. The
  envelope contract and the requirement IDs are unchanged. (origin: Key Decisions §4)
- **Plan landing target is a NEW top-level plan**, not a fold-in to the output-envelope plan
  `docs/plans/2026-04-30-001`. A+B' don't add new spec SHOULDs. (origin: Key Decisions §5)
- **Plan-time:** **Fixture matrix layout uses `tests/fixtures/known-shapes/<parser-family>/<detection-shape>/`.** Each
  parser-family directory holds one fixture per detection-shape (e.g. `clap/strong/`, `clap/medium/`, `clap/weak/` for
  source-tier shapes). Anchors per origin Outstanding Questions §5; mirrors
  the existing `tests/fixtures/perfect-rust/`, `broken-python/` archetype-per-directory pattern.
- **Plan-time:** **Tier resolution lives in each language's audit; only the Weak-tier Warn evidence message is shared.**
  A single `pub(crate) const STRUCTURED_OUTPUT_WEAK_WARN: &str` in `src/audits/source/mod.rs` is the shared surface;
  R8's "same tiered shape as Rust (R4)" is enforced by the shared message string. Each audit's `audit_x(source: &str)`
  returns `AuditStatus` per CLAUDE.md "Source Audit Convention", with the trait-impl `run()` constructing the per-tier
  `Confidence` directly. Introducing a new module + `Tier` enum + resolver function for one consumer per language is
  premature abstraction.
- **Plan-time:** **`tests/fixtures/perfect-rust/src/main.rs` already exhibits the within-arm Strong-tier signature.**
  `serde_json::to_string_pretty(&result)` is called inline in the `OutputFormat::Json` match arm at lines 68–73, and
  `serde_json` is declared in `Cargo.toml`. U2 verifies this against the new tier detection via the regression test
  scenario; no fixture mutation required.

---

## Open Questions

### Resolved During Planning

- **Fixture matrix layout** (origin Outstanding §5): chosen
  `tests/fixtures/known-shapes/<parser-family>/<detection-shape>/`. See Key Technical Decisions.
- **Whether to share tier resolution between Rust and Python source audits**: only the Weak-tier Warn evidence message
  is shared via `STRUCTURED_OUTPUT_WEAK_WARN: &str` in `src/audits/source/mod.rs`. Tier classification stays internal to
  each language's audit, which keeps `audit_x()` returning `AuditStatus` per CLAUDE.md "Source Audit Convention". See
  Key Technical Decisions.
- **Whether `tests/fixtures/perfect-rust/` needs an update**: no. The fixture's existing inline
  `serde_json::to_string_pretty(&result)` call in the `OutputFormat::Json` arm at lines 68–73 already satisfies the
  within-arm Strong-tier reachability rule. See Key Technical Decisions.

### Deferred to Implementation

- **Concrete ast-grep pattern shape for Strong-tier reachability in Rust** (R4). The match-arm-to-call-site traversal is
  the load-bearing detection step. Expect iteration during U2 — the patterns are tuned against fixtures.
- **Concrete ast-grep patterns for argparse `choices=` and click `Choice([...])`** (R7). argparse exposes `choices` as a
  list literal; click wraps it in a `Choice` constructor. Both must be supported. Tuned against the U4 Python fixtures
  during U3.

### Deferred from Document Review (blocking-before-implementation)

These two items came out of the document-review pass on 2026-05-01. Each is a design-class decision that must be
resolved **before** the implementation unit it gates begins — they are NOT iteration-time tuning questions. Resolve each
via discussion, then either fold the resolution into the Key Technical Decisions section above and remove the entry
here, or carry the chosen path into the relevant unit's Approach.

> **The two decisions interact: resolve Strong-tier first.** Strong (c) collapses to (Strong, Weak) and eliminates
> Medium, which makes Tier-symmetry (b) "Python Strong caps at Medium until flow analysis lands" incoherent.

- **Strong-tier definition (gates U2 + R13 + U5 dogfood).** U2 currently defines Strong two contradictory ways: (a)
  within-file local AST traversal of the `OutputFormat::Json => $$$BODY` arm for `serde_json::$METHOD`, and (b)
  cross-file aggregation across `cli.rs` / `main.rs` / `output.rs`. ast-grep-core's `Pattern` API is per-source-string —
  option (b) is not implementable as written. R13 + the U5 dogfood assertion require `anc` itself to resolve to
  Strong+High, but `anc`'s match arm calls `format_json` (in `scorecard/`), exactly the case U2 currently describes as
  Medium ("helper-module wirings, e.g., `anc`'s own `scorecard::format_json` indirection, without claiming Strong").
  Pick one before U2 begins:
- **(a) Strong = within-arm strict.** R13 changes to Medium; U5 dogfood asserts `confidence == "medium"`. The
  `tests/fixtures/perfect-rust/` fixture's existing inline call still qualifies as Strong.
- **(b) Strong = enum + clap-bound field + any `serde_json` call in the same crate.** Medium becomes "no `serde_json`
  call in the crate"; R13 + U5 dogfood stay at High. The U2 Medium-tier example needs rewriting.
- **(c) Tier collapses to (Strong, Weak).** Medium has no realizable test fixture under either definition; ship two
  tiers instead of three.
- **Tier label cross-language symmetry (gates U3).** Rust's match-arm AST is straightforward to ast-grep; Python's
  dispatch is structurally heterogeneous (`if`, `match`, dict-dispatch, `getattr`). When Rust resolves Strong via
  match-arm-to-call traversal, Python's Strong for the same label is doing strictly weaker reachability — for click
  handlers especially, where the option callback's body is reached via decorator wiring ast-grep cannot follow. The
  shared `Tier` label makes this look like a single concept; the underlying evidence-quality is unequal. Pick one before
  U3 begins:
- **(a) Per-language evidence-source metadata in the message.** Rust prints `"Strong (match-arm reachability)"`, Python
  prints `"Strong (declaration + dispatch text-match)"`; consumers can disambiguate while the tier label stays shared.
- **(b) Per-language Confidence cap.** Python Strong caps at `Confidence::Medium` until flow analysis lands; Rust Strong
  stays at High. Honors per-result `Confidence` and acknowledges Strong is language-conditional.

---

## Implementation Units

Four units. **U4 lands first** per the `p1-env-hints` v0.1.3 lesson — fixtures encode the expected verdict before the
detection widens, so silent regressions surface as test breaks rather than leaderboard surprises. U2 and U3 then run in
parallel against the fixture matrix. U5 closes the loop with dogfood guards and coverage-matrix regen. U1 is not built.

```text
U4 (fixture matrix)
   ├── U2 (Rust source tiering)
   └── U3 (Python source sibling)
            └── U5 (dogfood + coverage regen + changelog)
```

---

- U1. **Not in scope: a bad-arg-VALUE probe in `validate_json_output()`.** R1 to R3 are answered by `.anc.toml [p2]
  json_probe` (#147) and by scoring a miss as Skip (#178).

---

- U2. **Rust source tiering for `p2-structured-output`**

**Goal:** Replace the binary "enum exists or not" detection with a tiered model: Strong (`Confidence::High`) when the
OutputFormat enum has a `Json` variant AND a clap-derive field references it AND a `serde_json` serialization call site
is reachable from the match arm gating that variant; Medium (`Confidence::Medium`) when enum + clap reference + any
`serde_json` call in the same crate; Weak / Warn when only the enum exists with no `serde_json` call site.

**Requirements:** R4, R5, R6.

**Dependencies:** U4 (fixture matrix encodes Strong / Medium / Weak Rust fixtures).

**Files:**

- Modify: `src/audits/source/mod.rs` (add `pub(crate) const STRUCTURED_OUTPUT_WEAK_WARN: &str` shared with U3).
- Modify: `src/audits/source/rust/structured_output.rs` (replace `audit_structured_output` body with tier logic; return
  type stays `AuditStatus` per CLAUDE.md "Source Audit Convention").
- Test: `src/audits/source/rust/structured_output.rs` (existing `#[cfg(test)] mod tests`) — extended with per-tier
  scenarios.

**Approach:**

- **Helper-pair shape (per CLAUDE.md "Source Audit Convention").** Two functions in `structured_output.rs` share a
  single ast-grep parsing pass:
- `audit_structured_output(source: &str) -> AuditStatus` — the unit-testable contract per CLAUDE.md. Same shape as
  today, breaks no existing tests. Returns `Pass` for Strong/Medium tiers, `Warn` for Weak (with the shared
  `STRUCTURED_OUTPUT_WEAK_WARN` constant), `Skip` for non-clap codebases.
- `tier_for_source(source: &str) -> Tier` — private to the rust audit file. Returns the resolved
  `Tier::Strong / Medium / Weak`. Used only inside the trait-impl `run()` to select per-tier `Confidence`.
- Both helpers internally call a shared private `analyze_source(source: &str) -> SourceAnalysis` that performs the
  single ast-grep pass and returns the structured signals (enum present? clap field bound to it? `serde_json` call in
  arm? `serde_json` call anywhere in source?). `audit_structured_output` and `tier_for_source` each project from
  `SourceAnalysis` to their respective return types — no duplicate parsing.
- The trait-impl `run()` aggregates per-file results: it calls `tier_for_source` for each parsed file, picks the
  strongest tier across files (Strong dominates Medium dominates Weak), and constructs the `AuditResult` with the
  per-tier `Confidence` directly (Strong → `Confidence::High`, Medium → `Confidence::Medium`, Weak →
  `Confidence::Medium` with the shared Weak-tier Warn message). `audit_structured_output` is exported solely for unit
  tests; `run()` does not call it.
- `Tier` is a private enum inside `structured_output.rs`; not exported. U3 mirrors this shape (its own `tier_for_source`
- `analyze_source`, its own private `Tier`) — the only shared surface across U2/U3 is the
  `STRUCTURED_OUTPUT_WEAK_WARN: &str` constant.

- Strong-tier ast-grep approach (subject to U4 fixture-driven tuning): identify the OutputFormat-bound clap field, find
  the match arm `OutputFormat::Json => $$$BODY`, search `$$$BODY` for `serde_json::$METHOD(...)` calls. Use
  `find_pattern_matches_in` to get evidence locations.
- Medium-tier fallback: enum + clap field reference + any `serde_json::$METHOD($$$ARGS)` call anywhere in the parsed
  files. Looser reachability captures helper-module wirings (e.g., `anc`'s own `scorecard::format_json` indirection)
  without claiming Strong.
- Weak-tier: enum exists, no `serde_json` call site found in any parsed file. Emits the existing Warn message extended
  with "no `serde_json` call site detected — the enum may be declared but not wired to a serializer."
- The audit's `Confidence` field becomes per-tier rather than constant `High`. Plumbing: change `run()` to emit
  `confidence` based on the resolved tier rather than the constant on line 81.
- Preserve the existing "no clap detected" Skip path for non-clap codebases.

**Execution note:** The fixture matrix is the load-bearing harness. Verify the chosen ast-grep pattern shapes against
each U4 Rust fixture before integrating into `run()` — the pattern-shape iteration lives in dedicated unit tests inside
`src/audits/source/rust/structured_output.rs`'s test module, then graduates to the trait-level test.

**Patterns to follow:**

- `src/audits/source/rust/structured_output.rs:87` — `audit_structured_output(source: &str) -> AuditStatus` is the
  unit-testable core; the tiered logic replaces the body but keeps the signature.
- `src/source.rs::has_pattern_in` / `find_pattern_matches_in` — cross-language pattern primitives.
- `src/audits/source/rust/output_module.rs` — closest precedent for multi-pattern detection in a Rust source audit.

**Test scenarios:**

- *Happy path — Strong tier.* Source has `enum OutputFormat { Json, Text }`, clap field `output: OutputFormat`, match
  arm `OutputFormat::Json => { serde_json::to_string_pretty(&result)? }`. Tier resolves to Strong → Pass at
  Confidence::High. **Covers AE3.**
- *Happy path — Medium tier (helper module).* Source has the enum + clap field, but the `serde_json::to_string` call
  lives in a helper module that the match arm calls indirectly. Tier resolves to Medium → Pass at Confidence::Medium.
  **Covers AE4.**
- *Edge — Weak tier (enum only).* Source has `enum OutputFormat { Json }` with no `serde_json` call site anywhere. Tier
  resolves to Weak → Warn naming the gap. **Covers AE5.**
- *Edge — multi-file aggregation.* `OutputFormat` declared in `cli.rs`, match arm in `main.rs`, `serde_json` in
  `output.rs`. Resolver picks the strongest tier across files (per-file tiers Medium, Strong respectively → final
  Strong).
- *Edge — `Format` synonym.* Source uses `enum Format` instead of `enum OutputFormat`; both shapes are detected (R4
  requires both per the existing audit).
- *Edge — clap-detection false positive.* Source contains `clap` only in a string literal but no derive; tier audit
  routes through the existing "no clap detected" Skip.
- *Regression — perfect-rust fixture.* `cargo test --test integration` resolves `tests/fixtures/perfect-rust/` to Pass
  at Strong tier without modifying the fixture — the existing inline `serde_json::to_string_pretty(&result)` call in the
  `OutputFormat::Json` arm at `tests/fixtures/perfect-rust/src/main.rs:68-73` already satisfies the within-arm
  reachability rule.
- *Regression — broken-rust / source-only fixtures.* Existing fixtures continue to resolve to their documented verdicts
  (the tiered model upgrades, never downgrades, the broken-rust verdict).

**Verification:**

- `cargo test -p anc --lib audits::source::rust::structured_output` passes (existing tests + new per-tier scenarios).
- `anc audit tests/fixtures/perfect-rust` reports `p2-structured-output` as Pass at Confidence::High.
- `cargo test --test integration` passes (no regression on the structured_output integration assertions).

---

- U3. **Python source sibling audit**

**Goal:** Add a Python source-layer audit that mirrors the Rust shape (R4) via argparse `choices=` / click
`Choice([...])` plus `json.dumps`/`json.dump` reachability. Same tiered Strong / Medium / Weak verdict shape, same
Confidence levels, declares `covers() = &["p2-must-output-flag"]`.

**Requirements:** R7, R8, R9.

**Dependencies:** U4 (fixture matrix encodes Strong / Medium / Weak Python fixtures); U2's `STRUCTURED_OUTPUT_WEAK_WARN`
constant is the only shared surface — U3 can land before U2 if the constant is committed first, since the constant has
no behavior it just shares wording.

**Files:**

- Create: `src/audits/source/python/structured_output.rs` — new audit file.
- Modify: `src/audits/source/python/mod.rs` — register the new audit in `all_python_audits()`.
- Test: `src/audits/source/python/structured_output.rs` (`#[cfg(test)] mod tests`).

**Approach:**

- New struct `StructuredOutputPythonAudit` implements the `Audit` trait. `id()` is unique
  (`"p2-structured-output-python"`); the Rust audit's ID stays as-is. **Both audits declare
  `covers() = &["p2-must-output-flag"]`** — covers()-OR at the requirement layer credits the requirement when either
  passes (R9, validated by `dangling_cover_ids` and the existing matrix.rs OR-coverage logic).
- The audit's `applicable()` returns true iff `project.language == Some(Language::Python)`. The Rust audit stays
  applicable iff Rust. The two audits run independently against their respective fixtures; they never co-mingle on a
  single project.
- **Helper-pair shape (mirrors U2).** Two functions in `python/structured_output.rs` share a single ast-grep parsing
  pass — same shape as U2:
- `audit_structured_output_python(source: &str) -> AuditStatus` per CLAUDE.md "Source Audit Convention" — the
  unit-testable contract.
- `tier_for_source_python(source: &str) -> Tier` private to the file — returns the resolved tier; called only by `run()`
  to select `Confidence`.
- Both helpers internally call a shared private `analyze_source` that performs the single ast-grep pass and returns
  structured Python-specific signals (choice declaration present? `'json'` in choices? `json.dumps`/`json.dump` call in
  handler body? `json.dumps`/`json.dump` call anywhere in source?). Each top-level helper projects from this analysis to
  its return type.
- The trait-impl `run()` aggregates per-file `Tier` values, picks the strongest tier across files, and selects per-tier
  `Confidence` directly (Strong → High, Medium → Medium, Weak → Medium with the shared Weak-tier Warn). The Weak-tier
  `AuditStatus::Warn` evidence message uses the shared `STRUCTURED_OUTPUT_WEAK_WARN` constant from
  `src/audits/source/mod.rs`.
- Strong tier (subject to U4 fixture-driven tuning):
- argparse: `parser.add_argument($$$ARGS, choices=[$$$VALUES])` with `'json'` (or `"json"`) in `$$$VALUES`, AND a
  dispatch `if args.$NAME == 'json': $$$BODY` where `$$$BODY` contains `json.dumps(...)` / `json.dump(...)`.
- click: `@click.option(..., type=click.Choice([$$$VALUES]))` with `'json'` in values, AND a `json.dumps(...)`/
  `json.dump(...)` call in the option-handler body.
- Medium tier: choice declaration found + any `json.dumps`/`json.dump` call in any parsed file; reachability not
  provable.
- Weak tier: choice declaration found + no `json.dumps`/`json.dump` call site anywhere.
- Skip path: no choice declaration found in any parsed file (the Python equivalent of the Rust "no clap detected" Skip).

**Patterns to follow:**

- `src/audits/source/python/no_color.rs` — closest precedent: `Pattern::try_new`, `Python.ast_grep(source).root().find`,
  multi-pattern OR with `has_string_literal_in` fallback.
- `src/source.rs::has_pattern_in(source, pattern, Language::Python)` — Python-aware pattern dispatch.
- The shared `STRUCTURED_OUTPUT_WEAK_WARN: &str` constant from U2 (in `src/audits/source/mod.rs`) — used in the
  Weak-tier `AuditStatus::Warn` evidence message so both languages emit the same wording.

**Test scenarios:**

- *Happy path — Strong (argparse).* Python source with `parser.add_argument('--output', choices=['text', 'json'])` and
  dispatch `if args.output == 'json': print(json.dumps(result))`. Tier resolves to Strong → Pass at Confidence::High.
  **Covers AE6.**
- *Happy path — Strong (click).* Python source with `@click.option('--output', type=click.Choice(['text', 'json']))` and
  `json.dumps(result)` in the handler. Tier resolves to Strong.
- *Happy path — Medium (helper module).* Choice declaration in `cli.py`, dispatch + `json.dumps` in `output.py`. Tier
  resolves to Medium.
- *Edge — Weak (declaration only).* `parser.add_argument('--output', choices=['text', 'json'])` exists, no `json.dumps`
  anywhere in the parsed files. Tier resolves to Weak → Warn.
- *Edge — choices does not contain `'json'`.* `parser.add_argument('--output', choices=['text', 'yaml'])`; audit routes
  through the Skip path (no JSON support advertised) without false-positive.
- *Edge — `json.dumps` exists but no choice declaration.* No advertised `--output` flag; audit routes through Skip
  (mirrors the Rust "no clap detected" path).
- *Edge — both quote forms.* Variants with single-quoted (`'json'`) and double-quoted (`"json"`) literals both detected
  (matching `no_color.rs:107` precedent for quote-form coverage).
- *Drift guard — `dangling_cover_ids` test passes.* Adding the new audit with `covers() = &["p2-must-output-flag"]` must
  not break `src/principles/matrix.rs::live_catalog_has_no_dangling_cover_ids` (R9).
- *Drift guard — Python audits registered.* The existing `python_audits_registered` test in
  `src/audits/source/python/mod.rs` gains an assertion for the new audit ID.

**Verification:**

- `cargo test -p anc --lib audits::source::python::structured_output` passes.
- `cargo test -p anc --lib principles::matrix::tests::live_catalog_has_no_dangling_cover_ids` passes.
- `anc audit tests/fixtures/known-shapes/argparse/strong/` (or equivalent U4 fixture) reports
  `p2-structured-output-python` as Pass at Confidence::High.

---

- U4. **Parser-family fixture matrix**

**Goal:** Land the fixture matrix encoding expected verdicts for each source tier **before** U2 and U3 widen detection.
Each fixture is the external source of truth that catches silent regressions during U2 and U3 implementation. This is
the load-bearing failure-mode mitigation for the `p1-env-hints` v0.1.3 lesson.

**Requirements:** R12, plus implicit support for AE1-AE6.

**Dependencies:** None (this unit lands first).

**Files:**

- Create: `tests/fixtures/known-shapes/clap/strong/`, `clap/medium/`, `clap/weak/` — Rust source fixtures (mini Cargo
  crates with declared expected tier).
- Create: `tests/fixtures/known-shapes/argparse/strong/`, `argparse/medium/`, `argparse/weak/` — Python source fixtures
  (mini pyproject.toml + `.py` files) with declared expected tier.
- Create: `tests/fixtures/known-shapes/click/strong/`, `click/medium/`, `click/weak/` — Python source fixtures using
  click idioms.
- Create: `tests/fixtures/known-shapes/README.md` — declares the matrix layout, expected verdicts per fixture, and the
  parser-family family classification rationale.
- Modify: `tests/integration.rs` (this unit) — add U4 smoke tests that run `anc audit` against every known-shape fixture
  and assert it does not panic against the pre-tightening detection (catches malformed fixtures, and missing manifests
  at U4 time rather than during U2 and U3 implementation). The verdict-asserting integration tests still land in U5 —
  they need U2 and U3 detection to be present.

**Approach:**

- **Source fixtures** are real Cargo crates / Python projects with the smallest viable code that exhibits each tier's
  signature (the existing `tests/fixtures/perfect-rust/`, `broken-rust/`, `broken-python/` archetype pattern).
- Each fixture's `README.md` declares the expected verdict, Confidence, and the tier classification rationale.
- The tests against these fixtures split: U4 ships **smoke tests** (does `anc audit <fixture>` panic? does the manifest
  parse?), and U5 ships the **verdict-asserting integration tests** (which need U2 and U3 detection present). U4's smoke
  tests give U2 and U3 implementers immediate breakage signal during iteration without depending on detection that
  doesn't exist yet.
- Sequencing within U4: Rust source fixtures + smoke tests, then Python source fixtures + smoke tests. Each
  sub-step is a separate commit.

**Patterns to follow:**

- `tests/fixtures/perfect-rust/` — archetype-per-directory layout for source fixtures.
- `tests/fixtures/broken-python/pyproject.toml` — minimal Python project fixture shape.

**Test scenarios:**

- *Documentation.* Each fixture's `README.md` documents the tier represented, the exact `anc audit` invocation expected
  to verify it, and the expected verdict (Pass / Warn / Skip) + Confidence + evidence-message excerpt.
- *Smoke test — fixture loads.* For each fixture under `tests/fixtures/known-shapes/`, `anc audit <fixture-path>`
  completes without panicking against the current pre-tightening detection.
- *Smoke test — source fixtures parse.* For each Rust source fixture, the test shells out to
  `cargo metadata --offline --manifest-path <fixture>/Cargo.toml --format-version=1` and asserts exit zero — `--offline`
  avoids network flake on CI; the shell-out posture matches `tests/dogfood.rs`'s existing approach for `anc audit` and
  avoids growing `[dev-dependencies]` with the `cargo_metadata` crate. For each Python source fixture, the test parses
  the `.py` files via the existing `Project::discover` path. Catches manifest-parseability problems (malformed
  `Cargo.toml`, missing `[package]`, unresolvable path-deps); does NOT cover detection-relevant issues — those land in
  U5's verdict-asserting tests.
- *Verdict-asserting tests deferred to U5.* The tests that assert each fixture resolves to its documented expected
  verdict require U2 and U3 detection to be present and land in U5.

**Verification:**

- `tests/fixtures/known-shapes/` directory exists with the documented layout.
- `tests/fixtures/known-shapes/README.md` enumerates every fixture and its expected verdict.
- `cargo test --test integration` passes — U4's new smoke tests (no-panic, `cargo metadata --offline`) succeed
  against current detection.

---

- U5. **Dogfood guards, coverage matrix regen, and changelog**

**Goal:** Close the loop. Land integration tests that exercise the U4 fixture matrix against the U2 and U3 detection;
strengthen `tests/dogfood.rs` to assert the new verdicts on `anc` itself; regenerate the coverage matrix artifacts;
write a changelog entry that enumerates the tier shifts.

**Requirements:** R10, R11, R13, R14.

**Dependencies:** U2, U3 (detection must be implemented before the integration tests can assert the fixture
verdicts).

**Files:**

- Modify: `tests/integration.rs` — add fixture-matrix verdict-asserting tests for each U4 fixture (Rust + Python
  source-tier verdicts). **Replaces** the U4 verdict-overlapping smoke tests
  (`anc audit <fixture>` no-panic) — these are subsumed by the verdict-asserting
  tests, so they are deleted in this unit. The structural smoke tests stay (`cargo metadata --offline` parseability and
  the per-fixture `Project::discover` Python parse) — they catch a different class of drift the verdict tests don't
  surface.
- Modify: `tests/dogfood.rs` — add an explicit assertion for the tier `p2-structured-output` resolves to against the
  agentnative-cli repo itself.
- Regenerate: `docs/coverage-matrix.md` (via `anc emit coverage-matrix`).
- Regenerate: `coverage/matrix.json` (via the same command).
- Modify: `CHANGELOG.md` — add the next-version entry (per project convention; not bumping VERSION here — `/ship`
  handles version bumps).

**Approach:**

- Integration tests run `anc audit <fixture-path> --output json` per fixture, parse the result, and assert the verdict
- Confidence + evidence-message excerpt match the fixture's documented expectations. Drift catches both regression
  (fixture passes but should fail) and rebound (fixture fails but should pass). Pattern follows
  `tests/dogfood.rs::collect_failed`.

- Dogfood guards extend the existing `tests/dogfood.rs` `dogfood_no_p2_fail_after_skill_subcommand` test with one
  additional assertion: the tier and `confidence` `p2-structured-output` resolves to. R13 explicit.
- Coverage matrix regen is mechanical: `cargo run -- emit coverage-matrix` writes the artifacts; the existing
  `test_generate_coverage_matrix_drift_check_passes_on_committed_artifacts` integration test verifies parity. R14.
- `CHANGELOG.md` entry covers: Rust tightening (R4-R6) and the Python sibling (R7-R9). The "no schema bump" framing
  (R11) is mentioned for downstream consumers reading the entry.

**Execution note:** Coverage-matrix regen happens **after** all detection changes are merged into the branch but
**before** the PR is opened. CI's drift-check test would fail otherwise.

**Patterns to follow:**

- `tests/dogfood.rs::audit_repo_json` and `collect_failed` — JSON envelope parsing pattern for integration assertions.
- `docs/coverage-matrix.md` (current state) — the existing format, regenerated verbatim.

**Test scenarios:**

- *Integration — every U4 source fixture resolves correctly.* For each fixture under
  `tests/fixtures/known-shapes/<family>/<tier>/`, `anc audit` reports the documented `p2-structured-output` (Rust) or
  `p2-structured-output-python` (Python) verdict + Confidence.
- *Dogfood — `anc` self-audit resolves `p2-structured-output` to its Strong-tier-resolution-dependent Confidence.* Same
  envelope. The asserted `confidence` value depends on the Strong-tier decision from
  `Open Questions → Deferred from Document Review`: `"high"` under option (b) "any same-crate `serde_json`", `"medium"`
  under option (a) "within-arm strict" (since anc's match arm calls `format_json` in `scorecard/`, not `serde_json`
  directly). The concrete value is set when that question is resolved; until then this scenario is parametric. **Covers
  R13.**
- *Coverage matrix drift — committed artifacts agree with registry + audits.*
  `test_generate_coverage_matrix_drift_check_passes_on_committed_artifacts` continues to pass after the regen. **Covers
  R14.**
- *Schema parity — scorecard JSON `schema_version` stays unchanged.* `tests/scorecard_schema_v05.rs` continues to pass
  unchanged (R11).

**Verification:**

- `cargo test --test integration` passes for the new fixture-matrix assertions.
- `cargo test --test dogfood` passes for the strengthened guards.
- `cargo run -- emit coverage-matrix --check` exits 0.
- `cargo test --test scorecard_schema_v05` passes.

---

## System-Wide Impact

- **Interaction graph:** Both units are source audits over parsed files; neither spawns the target binary.
- **Error propagation:** Source audits return `AuditStatus` per the Source Audit Convention; no new error type, no new
  propagation surface.
- **State lifecycle risks:** None. Source audits and behavioral audits both operate on per-invocation state (parsed
  files / runner output) with no shared mutable state. Coverage-matrix regen is the only persistent-write step, and the
  drift-check test guards correctness.
- **API surface parity:** No change to the scorecard envelope shape, the CLI surface, or any exported type.
  `p2-json-output` keeps its `Confidence`. R11 stipulates schema_version stays unchanged.
- **Integration coverage:** The U4 fixture matrix is the integration backbone for U2 and U3. Each fixture exercises the
  full audit pipeline (detection → AuditStatus → AuditResult → JSON envelope → consumer). Mocks alone would not catch
  the cross-file ast-grep aggregation (U2/U3) — the fixture matrix is
  load-bearing.
- **Unchanged invariants:**
- `src/scorecard/audience.rs::SIGNAL_AUDIT_IDS` is unchanged. Neither source audit is a signal audit, so audience labels
  do not move. R10 explicit.
- `schema_version` stays unchanged. R11 explicit.
- `p2-json-output` and `p2-structured-output` retain their existing IDs and `covers()` declarations. No new requirement
  IDs added to `src/principles/registry.rs`. R6, R9 explicit.
- `arg_required_else_help = true` on `Cli` is preserved (CLAUDE.md "Dogfooding Safety"). Neither unit invokes the
  target.
- The output-envelope plan `docs/plans/2026-04-30-001` is not coupled to this plan.

---

## Risks & Dependencies

| Risk                                                                                                                                                    | Mitigation                                                                                                                                                                                                                            |
| ------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| U2's ast-grep reachability misses a real Strong-tier wiring (false Medium) → reviewer misreads the verdict as a regression on a previously-Strong tool. | U2's evidence message names the detection gap explicitly. Confidence::Medium is honest under this doctrine. PR description enumerates every Rust-tool tier shift; reviewers can spot misclassifications.                              |
| U3's argparse / click pattern misses a Python idiom (e.g. `argparse.ArgumentParser` configured via class methods, click groups) → false Skip / Weak.    | U4 includes click + argparse fixtures at each tier. The Python sibling can be tightened iteratively without changing its public contract; first iteration covers the dominant idioms named in R7.                                     |
| Coverage-matrix regen runs before all detection changes land → committed artifacts disagree with detection → CI drift-check fails on the PR.            | U5 explicitly sequences regen as the **last** step before opening the PR. The pre-push hook (`scripts/hooks/pre-push`) runs the drift-check test and would catch the gap locally.                                                     |

---

## Documentation / Operational Notes

- **PR description must enumerate** (per R5): every Rust tool whose `p2-structured-output` tier shifts, with the pre/post
  values. This is the reviewer's primary aid for
  validating the verdict shifts as deliberate.
- **`CHANGELOG.md` entry must include** the tier shifts (R5) and the no-schema-bump framing (R11). Pure internal
  refactoring (the shared helper module) does NOT belong in the changelog per CLAUDE.md "PR / Changelog source of
  truth."
- **Branch:** `feat/p2-json-output-bad-arg-and-source-tightening`, cut from `dev`, PR'd back to `dev`. Standard
  CLAUDE.md branch discipline.
- **Sequencing for `/ce-work`:** U4 → (U2, U3 in parallel) → U5. U4 is the gate.
- **Dogfooding the `/coverage` site renderer:** post-merge, the agentnative-site `/coverage` page consumes the
  regenerated `coverage/matrix.json`. The site repo will pick up the new artifact through the existing copy-on-release
  pipeline; no coordinating PR required.

---

## Sources & References

- **Origin requirements doc:** `docs/brainstorms/2026-05-01-p2-json-output-upgrade-requirements.md`
- **Origin ideation doc:** `docs/ideation/2026-04-30-p2-json-output-ideation.md`
- **Pre-plan handoff:** `.context/handoffs/2026-05-01-001-p2-json-output-pre-plan-handoff.md`
- **Active output-envelope plan (related, not coupled):**
  `docs/plans/2026-04-30-001-feat-spec-output-envelope-shoulds-plan.md`
- **Direct precedent for in-place widening:** `docs/solutions/best-practices/cli-env-var-shape-heuristic-2026-04-21.md`
- **SRP-per-audit doctrine:** `docs/solutions/best-practices/reliable-static-analysis-compliance-auditors-20260327.md`
- **Behavioral-vs-source split doctrine:**
  `docs/solutions/best-practices/behavioral-vs-structural-must-when-authoring-spec-requirements-20260420.md`
- **Aggregate-verdict doctrine:**
  `docs/solutions/architecture-patterns/aggregate-verdicts-are-informational-not-authoritative-20260420.md`
- **Behavioral audit entry point:** `src/audits/behavioral/json_output.rs`
- **Rust source audit entry point:** `src/audits/source/rust/structured_output.rs`
- **Python source audit directory:** `src/audits/source/python/`
- **Audience classifier:** `src/scorecard/audience.rs`
- **Coverage matrix logic:** `src/principles/matrix.rs`
- **Dogfood guards:** `tests/dogfood.rs`
