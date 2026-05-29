---
title: "feat: p2-json-output bad-arg-VALUE probe + Rust tightening + Python source sibling"
type: feat
status: active
date: 2026-05-01
deepened: 2026-05-01
origin: docs/brainstorms/2026-05-01-p2-json-output-upgrade-requirements.md
---

# feat: p2-json-output bad-arg-VALUE probe + Rust tightening + Python source sibling

## Summary

Add a third evidence path to `p2-json-output` (behavioral) — a bad-arg-VALUE probe that injects a deliberately-invalid
value for the detected `--output`/`--format` flag and parses stderr for the parser's declared value enumeration; tighten
the existing Rust source audit `p2-structured-output` from "enum exists" to a tiered Strong / Medium / Weak detection
that walks from a clap-bound `OutputFormat::Json` variant to a reachable `serde_json` call site; and ship a Python
source sibling that mirrors the Rust shape via argparse `choices=` / click `Choice([...])` plus reachable
`json.dumps`/`json.dump`. Verdict shifts on already-scored tools land deliberately, anchored by a parser-family fixture
matrix that lands **before** the detection widens.

---

## Problem Frame

The dominant cause of `anc`'s self-cap (~97% project / ~89% binary) is `p2-json-output` emitting Warn whenever its
safe-suffix probes (`--help` / `--version`) cannot validate JSON — which is the dominant case across the leaderboard.
Two structural facts shape the remedy: (1) a third safe-probe shape (bad-arg-VALUE) is universally side-effect-safe
because the parser rejects the value before any subcommand handler runs, and (2) the Rust source audit already covers
the same requirement (`p2-must-output-flag`) but stops at "enum exists" — source layer can do strictly more than
behavioral via reachability and flow analysis. See origin:
`docs/brainstorms/2026-05-01-p2-json-output-upgrade-requirements.md`.

---

## Requirements

Carried verbatim from origin requirements doc. R-IDs stable.

- R1. Add a bad-arg-VALUE probe to `validate_json_output()` as a third evidence path inside the existing
  `p2-json-output` audit. Probe shape: inject a deliberately-invalid value for the detected `--output`/`--format` flag;
  parse stderr for the declared value enumeration.
- R2. Verdict semantics shift in-place (same audit ID, same `covers()`). Pass at `Confidence::Medium` when the
  value-enum echo confirms `json` is a declared accepted value (`p1-env-hints` v0.1.3 widening precedent — widening
  detection does not raise confidence).
- R3. When the bad-arg-VALUE probe cannot fire, fall back to the existing safe-suffix probes and emit the existing Warn
  evidence message. No regression on currently-passing tools.
- R4. Replace the binary "enum exists or not" detection in `p2-structured-output` (Rust) with a tiered model: Strong
  (Confidence::High) when the OutputFormat enum has a `Json` variant AND a clap-derive field references it AND a
  `serde_json` serialization call site is reachable from the match arm gating that variant; Medium (Confidence::Medium)
  when enum + clap reference + any `serde_json` call in the same crate; Weak / Warn when only the enum exists.
- R5. Tools previously passing at `Confidence::High` purely on enum existence shift to a tier consistent with their
  actual reachability. Shifts anchored by the fixture matrix (R12) and enumerated in the PR description.
- R6. Audit ID stays `p2-structured-output`; `covers()` stays `&["p2-must-output-flag"]`. No new requirement IDs added
  to `src/principles/registry.rs`.
- R7. Add a Python source-layer audit applicable when `Project::language` is Python. Detects argparse `add_argument(...,
  choices=[..., 'json', ...])` / click `Choice([..., 'json', ...])` declarations + a `json.dumps` / `json.dump` call
  site reachable from the dispatch on that argument's value.
- R8. Detection produces the same tiered Strong / Medium / Weak shape as Rust (R4), with the same Confidence levels and
  the same Warn message when only the choice declaration exists without a `json.dumps` call site.
- R9. The Python audit declares `covers() = &["p2-must-output-flag"]` and registers in
  `src/audits/source/python/mod.rs`. The `dangling_cover_ids` test passes after the addition.
- R10. `src/scorecard/audience.rs::SIGNAL_AUDIT_IDS` is unchanged. Verdict shifts on `p2-json-output` cause natural
  re-derivation of audience labels. Changelog and PR description enumerate which currently-scored tools shift labels and
  which way.
- R11. No scorecard schema bump. `schema_version` stays at `"0.5"`.
- R12. A parser-family fixture matrix anchors verdict shifts before merge: behavioral fixtures cover currently-warn-
  capped tools across clap, cobra, argparse, and click parser families; source fixtures cover Rust tools at each tier
  (Strong / Medium / Weak) and Python tools at each tier. Each fixture documents its expected verdict and Confidence
  before the detection lands.
- R13. `tests/dogfood.rs` updates to reflect the new verdict for `p2-json-output` against `anc` itself (Pass at
  `Confidence::Medium`) and the tier `p2-structured-output` resolves to. **The concrete Confidence value for
  `p2-structured-output` depends on the Strong-tier resolution from `Open Questions → Deferred from Document Review`** —
  `Confidence::High` if option (b) "any same-crate `serde_json`" is chosen, `Confidence::Medium` if option (a)
  "within-arm strict" is chosen (anc's match arm calls `format_json` in `scorecard/`, not `serde_json` directly).
- R14. `docs/coverage-matrix.md` and `coverage/matrix.json` are regenerated via `anc emit coverage-matrix` as part
  of the same PR. The integration test `test_generate_coverage_matrix_drift_check_passes_on_committed_artifacts` passes.

---

## Scope Boundaries

Carried from origin. Anything below is explicitly NOT in this plan.

- No new requirement IDs added to `agentnative-spec`. A+B' attest the existing `p2-must-output-flag` MUST more
  rigorously; no spec PR.
- No `agentnative-spec` version bump. Vendored spec text unchanged.
- No scorecard schema bump (stays `"0.5"`). Envelope contract unchanged.
- No self-declared manifest field opt-in (`[package.metadata.agentnative]` — ideation D). Deferred.
- No `<tool> agentnative-probe` spec convention (ideation F). Long-term ecosystem move.
- No spec MUST rewriting (ideation E, adversarial-rejected: doctrine downgrades prose to preference, not level of MUST).
- No Skip-with-evidence verdict change (ideation G). Audience-let-it-shift covers the ground.
- No change to `src/scorecard/audience.rs::SIGNAL_AUDIT_IDS`.
- No CommandFactory build-time shim (ideation F6.9). Cross-compilation cost too high.
- No `jc` external-wrapper fallback (ideation F2.7 / F6.11). Subject-tangential.
- No coupling at the requirements level with active output-envelope plan (`docs/plans/2026-04-30-001`). Bad-arg probe
  primitive may be shared at code-level but the two plans don't cross-depend.
- No third source language (Go, Bun, etc.) at this iteration. Rust + Python are the source-audit launch languages per
  `Cargo.toml` feature flags.

---

## Context & Research

### Relevant Code and Patterns

- `src/audits/behavioral/json_output.rs:148-202` — `validate_json_output()` is the primary extension site. The current
  function takes `(runner, prefix, has_output_flag, has_format_flag)` and tries safe-suffix probes; R1/R2/R3 add a third
  path before the existing fallthrough returns Warn.
- `src/audits/behavioral/bad_args.rs:35` — current bad-arg trigger using `--this-flag-does-not-exist-agentnative-probe`.
  The bad-arg-VALUE probe is the same pattern with a different argument shape (`--output
  __invalid_format_value_agentnative_probe__`).
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
- `src/runner/mod.rs::BinaryRunner::run` — `(args, env_overrides)` primitive U1 invokes for the bad-arg-VALUE probe.
  `NO_COLOR=1` is always set; results are cached by `(args, env_overrides)`.

### Institutional Learnings

- `docs/solutions/best-practices/cli-env-var-shape-heuristic-2026-04-21.md` — `p1-env-hints` v0.1.2 → v0.1.3 widening
  precedent. Two regressions surfaced post-merge — only fixture-driven leaderboard anchoring caught them. **The U4-
  before-U1/U2/U3 sequencing rule is direct guidance from this learning.**
- `docs/solutions/best-practices/reliable-static-analysis-compliance-auditors-20260327.md` — SRP-per-audit doctrine.
  Validates that A's third evidence path lands in-place rather than as a new audit ID — the property under test (flag
  honors JSON output) is unchanged; only the probe shape widens.
- `docs/solutions/best-practices/behavioral-vs-structural-must-when-authoring-spec-requirements-20260420.md` — when a
  behavioral audit can't safely attest, ADD a source-layer sibling. The Python sibling (U3) is direct compliance.
- `docs/solutions/architecture-patterns/aggregate-verdicts-are-informational-not-authoritative-20260420.md` —
  covers()-OR is at the requirement layer; per-audit verdicts stay independent. U2 tightening Rust does not aggregate
  with U1 behavioral; both verdicts surface honestly side-by-side on every run.
- `docs/solutions/best-practices/audit-scripts-as-documentation-immune-system-2026-04-20.md` — doctrine that downgrades
  PROSE to preference, not LEVEL of MUST. Validates rejecting ideation E (spec-side narrowing).

### External References

Not gathered — local patterns are direct, ast-grep pattern shapes are explicitly deferred to implementation per R4/R7.

---

## Key Technical Decisions

Carried from origin's locked decisions table; plan-time additions are flagged. **Do not relitigate the carried decisions
during implementation unless contradicting evidence surfaces.**

- **A's verdict lands in-place on `p2-json-output` (third evidence path), not as a new audit ID.** Rationale: the
  property under test is unchanged (flag honors JSON output); only the probe shape widens. SRP-per-audit is about
  properties, not probe shapes. (origin: Key Decisions §1)
- **Source-layer detection becomes asymmetrically more nuanced than behavioral via reachability.** Behavioral stays at
  `Confidence::Medium` per the widening doctrine; source can earn `Confidence::High` when reachability is provable.
  (origin: Key Decisions §2)
- **Audience-classifier shifts are accepted as correctness wins.** Tools previously under-read becoming
  `agent-optimized` is the right direction. (origin: Key Decisions §3)
- **No scorecard / spec version bump.** Verdict shifts on already-scored tools are normal release-note territory. The
  envelope contract and the requirement IDs are unchanged. (origin: Key Decisions §4)
- **Plan landing target is a NEW top-level plan**, not a fold-in to active output-envelope plan
  `docs/plans/2026-04-30-001`. A+B' don't add new spec SHOULDs. (origin: Key Decisions §5)
- **Plan-time:** **Fixture matrix layout uses `tests/fixtures/known-shapes/<parser-family>/<detection-shape>/`.** Each
  parser-family directory holds one fixture per detection-shape (e.g. `clap/strong/`, `clap/medium/`, `clap/weak/` for
  source-tier shapes; `clap/value-enum-echoes/`, `cobra/value-enum-echoes/`, `argparse/value-enum-echoes/`,
  `click/value-enum-echoes/` for behavioral parser-family shapes). Anchors per origin Outstanding Questions §5; mirrors
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

- **Concrete tool list for the fixture matrix** (origin Outstanding §1, R12). Resolve at the start of U4 by querying the
  latest leaderboard data and grouping by parser family. The R12 minimum is one tool per family covering the warn-capped
  case; expansion to additional tools is allowed if leaderboard inspection surfaces edge-case error-message shapes
  within a family. **Empty-family fallback** (per U4 Approach): if a parser family has no warn-capped leaderboard tool,
  the family fixture uses a synthetic minimal tool committed alongside the fixture (~20-line clap crate / ~10-line
  argparse script / equivalent), captured via the same `NO_COLOR=1 LANG=C COLUMNS=80` env, labeled `source:
  synthetic-minimal-tool` in the fixture's `README.md`, and called out in the PR description.
- **Concrete ast-grep pattern shape for Strong-tier reachability in Rust** (R4). The match-arm-to-call-site traversal is
  the load-bearing detection step. Expect iteration during U2 — the patterns are tuned against fixtures.
- **Concrete ast-grep patterns for argparse `choices=` and click `Choice([...])`** (R7). argparse exposes `choices` as a
  list literal; click wraps it in a `Choice` constructor. Both must be supported. Tuned against the U4 Python fixtures
  during U3.
- **Concrete enumeration of audience-label shifts on currently-scored tools** (R10). Resolve at the end of U5 by running
  `anc audit` against each leaderboard member with the new detection enabled and diffing the audience field; cite the
  diff in the PR description.
- **Whether any leaderboard tool's stderr enumeration uses a format the regex misses on first iteration** (R3 fallback
  shape). Implementation-time discovery; the fallback path is intentionally conservative.

### Deferred from Document Review (blocking-before-implementation)

These four items came out of the document-review pass on 2026-05-01. Each is a design-class decision that must be
resolved **before** the implementation unit it gates begins — they are NOT iteration-time tuning questions. Resolve each
via discussion, then either fold the resolution into the Key Technical Decisions section above and remove the entry
here, or carry the chosen path into the relevant unit's Approach.

> **The four decisions interact — resolve in order.** They are not independent. At least three pairs of options
> constrain each other:
>
> - **Strong-tier × Confidence demotion.** Picking Strong (a) "within-arm strict" forces `anc` itself to Medium → makes
>   Confidence demotion (a) "per-evidence-path Confidence" much more attractive (recovers a coherent High dogfood story
>   for the safe-suffix path). Picking Strong (b) "any same-crate `serde_json`" leaves `anc` at High, weakening the case
>   for Confidence (a).
> - **Strong-tier (c) × Tier-symmetry (b).** Strong (c) collapses to (Strong, Weak) — eliminates Medium. Tier-symmetry
>   (b) "Python Strong caps at Medium until flow analysis lands" becomes incoherent if Medium doesn't exist.
> - **Parser-safety (a) × Tier-symmetry (a).** Both rely on parser-family fingerprinting from `--help` output. Pick them
>   consistently or one direction's infrastructure goes unused.
>
> **Recommended resolution order:** Strong-tier definition → Confidence demotion doctrine → Parser-rejects-before-handler
> safety → Tier label cross-language symmetry. Each downstream decision sees a constrained option set under this order.

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
- **Parser-rejects-before-handler safety premise (gates U1).** The plan asserts the bad-arg-VALUE probe is universally
  side-effect-safe because parsers reject the value before any subcommand handler runs. Counter-examples: Go stdlib
  `flag` accepts any string value, argparse without `choices=` and `type=` callbacks coerce-but-don't-reject, post-parse
  `validate()` hooks run after side-effecting setup, subcommand-specific flags dispatch to handlers that do their own
  validation. The plan currently re-introduces a dogfooding-safety risk class without the rigor `arg_required_else_help`
  got. Pick one before U1 begins:
- **(a) Parser-family fingerprint gating.** Detect parser family from `--help` output (clap's `Usage:` shape, cobra's
  tree shape, argparse's `usage:` prefix); only fire the bad-arg-VALUE probe when a known-safe parser is detected.
- **(b) Pre-flight bad-arg sanity check.** Before firing the bad-arg-VALUE probe, run `--this-flag-does-not-exist` and
  confirm the binary exits non-zero. A tool that accepts unknown flags also won't reliably reject unknown values. (a)
- (b) is closest to the existing dogfooding-safety doctrine.
- **(c) Document the holes.** Add a Risks-table row enumerating Go-`flag` / argparse-no-choices / post-parse-validation
  cases as known holes; accept that R12 cannot prove safety on unknown shapes; consumers reading `anc`'s code can see
  the surface.
- **Confidence demotion doctrine (gates U1 + R10 enumeration scope).** U1 currently demotes the audit's top-level
  `Confidence` from `High` to `Medium` for **all** Pass paths, including the existing safe-suffix path (validated
  literal JSON parse) which evidences a stronger property than the new bad-arg-VALUE path (parser-introspection only).
  Consequence: tools currently passing via safe-suffix get a silent `confidence` field shift visible to downstream
  consumers (site `/score` page, badge color tiers, agents reading the JSON). R10's enumeration only catches audience
  label shifts; this Confidence-only class is missed. Pick one before U1 begins:
- **(a) Per-evidence-path Confidence.** Emit `Confidence::High` when the safe-suffix Pass actually fires, `Medium` when
  only the bad-arg-VALUE probe fires. The result-message already names the probe shape; consumers can reconcile. The
  widening doctrine that drove the demotion was calibrated on `p1-env-hints` v0.1.3, where both old and new paths shared
  the same evidence-property shape (env-var existence) — the analogy doesn't hold here because safe-suffix and
  bad-arg-VALUE evidence different properties.
- **(b) Single-Confidence-per-audit + extend R10 enumeration.** Keep the `Medium`-for-all-Pass-paths shift; extend R10's
  PR-description and changelog enumeration scope from "audience-label shifts" to "any change in `(verdict, confidence,
  audience)` tuple." CHANGELOG gets two sections: "Audience shifts (correctness wins)" and "Confidence demotions
  (doctrine fix)."
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

Five units. **U4 lands first** per the `p1-env-hints` v0.1.3 lesson — fixtures encode the expected verdict before the
detection widens, so silent regressions surface as test breaks rather than leaderboard surprises. U1, U2, U3 then run in
parallel against the fixture matrix. U5 closes the loop with dogfood guards and coverage-matrix regen.

```text
U4 (fixture matrix)
   ├── U1 (behavioral bad-arg-VALUE probe)
   ├── U2 (Rust source tiering)
   └── U3 (Python source sibling)
            └── U5 (dogfood + coverage regen + changelog)
```

---

- U1. **Bad-arg-VALUE probe in `validate_json_output()`**

**Goal:** Add a third evidence path to `p2-json-output` that invokes `<bin> [prefix...] <flag>
__invalid_format_value_agentnative_probe__` and parses stderr for the declared value enumeration. When the parser echoes
`json` as a declared value, return `AuditStatus::Pass` at `Confidence::Medium`. When the probe cannot extract a value
list, fall through to the existing safe-suffix Warn.

**Requirements:** R1, R2, R3.

**Dependencies:** U4 (fixture matrix anchors expected verdicts before this lands).

**Files:**

- Modify: `src/audits/behavioral/json_output.rs`
- Test: `src/audits/behavioral/json_output.rs` (existing `#[cfg(test)] mod tests`)
- Test fixtures (consumed by integration tests, created in U4): `tests/fixtures/known-shapes/clap/value-enum-echoes/`,
  `tests/fixtures/known-shapes/cobra/value-enum-echoes/`, `tests/fixtures/known-shapes/argparse/value-enum-echoes/`,
  `tests/fixtures/known-shapes/click/value-enum-echoes/`.

**Approach:**

- The probe runs **after** the existing `--help`/`--version` safe-suffix loop produces a non-Pass result and **before**
  the `Warn` fallthrough. This preserves R3 ordering: existing passing tools see no change, the new path only activates
  when the prior loop didn't already conclude `Pass`.
- The probe argv is built per detected flag (`--output` and/or `--format`): `[prefix..., flag,
  "__invalid_format_value_agentnative_probe__"]`. Sentinel value follows the existing `bad_args.rs` naming convention
  for grep-discoverability.
- After `runner.run`, parse the merged `(stdout, stderr)` lowercase output for a declared-value enumeration. Recognized
  shapes: `must be one of [...]` (clap), `valid choices: ...` (argparse), `must be one of: ...` (cobra/click). Extract
  the bracketed/comma-separated list; pass if `json` is enumerated.
- The result-message format names the probe shape and the declared values: `"--output flag declares JSON in value
  enumeration via bad-arg-VALUE probe (declared values: [text, json, yaml])"`. Confidence stays `Medium`.
- The `validate_json_output` function signature does not change. The new probe is private to the function (or a small
  private helper, e.g. `try_value_enum_probe(runner, prefix, flag) -> Option<AuditStatus>`).
- The audit's top-level `Confidence::High` field at line 62 changes to `Confidence::Medium` to reflect the new
  weakest-evidence-path of the union (per widening doctrine). Existing tools that pass via the safe-suffix path also
  shift to `Medium`; this is a deliberate honesty fix, not a regression.

**Patterns to follow:**

- `src/audits/behavioral/bad_args.rs:35` — naming convention for the sentinel argument value.
- `src/audits/behavioral/json_output.rs:205-229` — `try_json_probe` helper as the shape model for
  `try_value_enum_probe`.
- `try_value_enum_probe` returns `Option<AuditStatus>` (mirrors `try_json_probe`) so the existing per-flag/per-suffix
  loop stays uniform.

**Test scenarios:**

- *Happy path — clap-shape echo.* sh-script fixture emits clap's shape `error: invalid value
  '__invalid_format_value_agentnative_probe__' for '--output <FORMAT>': must be one of [text, json]` to stderr on the
  invalid-value invocation; audit returns `Pass` at `Confidence::Medium`. **Covers AE1.**
- *Happy path — argparse-shape echo.* sh-script fixture emits argparse's shape `error: argument --output: invalid
  choice: '__invalid__' (choose from 'text', 'json', 'yaml')`; audit returns `Pass`.
- *Happy path — click-shape echo.* sh-script fixture emits click's shape `Error: Invalid value for '--output': 'foo' is
  not one of 'text', 'json', 'yaml'.`; audit returns `Pass`.
- *Edge — bad-arg-VALUE response missing `json`.* Fixture echoes `must be of [text, yaml]` (no json); audit falls back
  to safe-suffix probes; emits the existing Warn. **Covers AE2.**
- *Edge — bad-arg-VALUE returns free-form error without enumeration.* Fixture emits `Error: unknown output format`; the
  value-extraction regex fails; falls back to existing Warn message. **Covers AE2 / R3 (no regression).**
- *Edge — flag exists but binary errors before parse.* Fixture exits non-zero with no stderr enumeration; falls back to
  existing Warn.
- *Regression — already-passing tool stays Pass.* Existing `json_output_pass_with_valid_json` test must still pass after
  the change (with the Confidence shift to Medium).
- *Regression — already-Warn tool stays Warn.* Existing `json_output_fail_with_invalid_json` test must still produce
  `Warn` (R3).
- *Regression — Skip behavior unchanged for `no flag detected`.* Existing `json_output_skip_no_flag` test must still
  produce `Skip`.

**Verification:**

- `cargo test -p anc --lib audits::behavioral::json_output` passes for all old + new scenarios.
- The `Confidence` field on the result is `Medium` for both bad-arg-VALUE and safe-suffix Pass paths.
- Existing integration tests in `tests/integration.rs` continue to pass.

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
- `tier_for_source(source: &str) -> Tier` — private to the rust audit file. Returns the resolved `Tier::Strong / Medium
  / Weak`. Used only inside the trait-impl `run()` to select per-tier `Confidence`.
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
- `analyze_source`, its own private `Tier`) — the only shared surface across U2/U3 is the `STRUCTURED_OUTPUT_WEAK_WARN:
  &str` constant.

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
  (`"p2-structured-output-python"`); the Rust audit's ID stays as-is. **Both audits declare `covers() =
  &["p2-must-output-flag"]`** — covers()-OR at the requirement layer credits the requirement when either passes (R9,
  validated by `dangling_cover_ids` and the existing matrix.rs OR-coverage logic).
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

**Goal:** Land the fixture matrix encoding expected verdicts for each parser family (behavioral) and each tier (source)
**before** U1 / U2 / U3 widen detection. Each fixture is the external source of truth that catches silent regressions
during U1-U3 implementation. This is the load-bearing failure-mode mitigation for the `p1-env-hints` v0.1.3 lesson.

**Requirements:** R12, plus implicit support for AE1-AE6.

**Dependencies:** None (this unit lands first).

**Files:**

- Create: `tests/fixtures/known-shapes/clap/value-enum-echoes/` — sh-script binary fixture emitting clap-style stderr.
- Create: `tests/fixtures/known-shapes/cobra/value-enum-echoes/` — sh-script binary fixture emitting cobra-style stderr.
- Create: `tests/fixtures/known-shapes/argparse/value-enum-echoes/` — sh-script binary fixture emitting argparse-style
  stderr.
- Create: `tests/fixtures/known-shapes/click/value-enum-echoes/` — sh-script binary fixture emitting click-style stderr.
- Create: `tests/fixtures/known-shapes/<family>/value-enum-echoes/expected-stderr.txt` for each behavioral fixture — the
  literal stderr captured from a real leaderboard tool of that family using the normalized capture environment
  (`NO_COLOR=1 LANG=C COLUMNS=80 <real-tool> --output __invalid_format_value_agentnative_probe__`).
- Create: `tests/fixtures/known-shapes/<family>/value-enum-echoes/capture.sh` for each behavioral fixture — the literal
  capture command + env, committed for reviewer reproducibility.
- Create: `tests/fixtures/known-shapes/clap/strong/`, `clap/medium/`, `clap/weak/` — Rust source fixtures (mini Cargo
  crates with declared expected tier).
- Create: `tests/fixtures/known-shapes/argparse/strong/`, `argparse/medium/`, `argparse/weak/` — Python source fixtures
  (mini pyproject.toml + `.py` files) with declared expected tier.
- Create: `tests/fixtures/known-shapes/click/strong/`, `click/medium/`, `click/weak/` — Python source fixtures using
  click idioms.
- Create: `tests/fixtures/known-shapes/README.md` — declares the matrix layout, expected verdicts per fixture, and the
  parser-family family classification rationale.
- Modify: `tests/integration.rs` (this unit) — add U4 smoke tests that run `anc audit` against every known-shape fixture
  and assert it does not panic against the pre-tightening detection (catches malformed fixtures, missing manifests, and
  shell-script syntax errors at U4 time rather than during U1/U2/U3 implementation). The verdict-asserting integration
  tests still land in U5 — they need U1/U2/U3 detection to be present.

**Approach:**

- **Behavioral fixtures** are sh-script binaries (the existing `tests/fixtures/binary-only/` and `hostile-*` archetype
  pattern). Each fixture's script reproduces its parser family's stderr shape on the bad-arg-VALUE invocation. This
  isolates the regex-extraction logic (U1) from real CLI behavior — the fixture's job is to encode the parser family's
  literal stderr format.
- **External validation step (load-bearing).** For each behavioral fixture, capture literal stderr from one real
  leaderboard tool of that parser family using a normalized capture environment: `NO_COLOR=1 LANG=C COLUMNS=80
  <real-tool> --output __invalid_format_value_agentnative_probe__ 2>expected-stderr.txt`. The normalized env strips ANSI
  color, locale-translated quote forms, and terminal-width-driven wrapping at capture time so the committed file is
  environment-stable. Commit a small `capture.sh` alongside `expected-stderr.txt` documenting the exact invocation;
  reviewers can re-run it when a real-tool version bumps. The sh-script reproduces the value-enumeration shape (the
  substring U1's regex depends on) — byte-for-byte equality is deliberately not the contract; the smoke test asserts
  substring containment, not byte parity, since drift on ANSI / version / `argv[0]` / locale doesn't affect U1's
  detection.
- **Empty-parser-family fallback rule (synthetic minimal tool).** If the leaderboard query reveals zero currently-warn-
  capped tools for a parser family, build a synthetic minimal tool of that family inside the fixture directory — a
  ~20-line clap-derive Cargo crate, a ~10-line argparse Python script, or the equivalent — and capture its
  `expected-stderr.txt` from that synthetic tool the same way (`NO_COLOR=1 LANG=C COLUMNS=80 …`). Label the fixture's
  `README.md` as `source: synthetic-minimal-tool` with the synthetic tool's source path, the parser library version
  used, and the capture command. The PR description must call out which fixtures are `synthetic-minimal-tool`-derived.
  This keeps the fallback in the same external-verification regime as the tool-derived path — doc drift cannot poison
  the fixture because the source of truth is a tool that compiles in this repo, not external documentation. The cost is
  bounded (~30 minutes per family at U4 time).
- **Source fixtures** are real Cargo crates / Python projects with the smallest viable code that exhibits each tier's
  signature (the existing `tests/fixtures/perfect-rust/`, `broken-rust/`, `broken-python/` archetype pattern).
- Each fixture's `README.md` declares the expected verdict, Confidence, the tier classification rationale, and (for
  behavioral fixtures) the source field (`tool-derived` with the leaderboard tool name + version, or
  `synthetic-minimal-tool` with the synthetic tool's source path + parser library version).
- The tests against these fixtures split: U4 ships **smoke tests** (does `anc audit <fixture>` panic? does the fixture's
  binary run? does the captured stderr file match the script output?), and U5 ships the **verdict-asserting integration
  tests** (which need U1/U2/U3 detection present). U4's smoke tests give U1/U2/U3 implementers immediate breakage signal
  during iteration without depending on detection that doesn't exist yet.
- Sequencing within U4: capture real-tool stderr first (any family with a leaderboard tool); then commit behavioral
  fixtures + smoke tests; then Rust source fixtures + smoke tests; then Python source fixtures + smoke tests. Each
  sub-step is a separate commit.

**Patterns to follow:**

- `tests/fixtures/perfect-rust/` — archetype-per-directory layout for source fixtures.
- `tests/fixtures/binary-only/`, `hostile-stdout-flood/` — sh-script binary fixture pattern.
- `tests/fixtures/broken-python/pyproject.toml` — minimal Python project fixture shape.

**Test scenarios:**

- *Documentation.* Each fixture's `README.md` documents the parser-family or tier represented, the exact `anc audit`
  invocation expected to verify it, the expected verdict (Pass / Warn / Skip) + Confidence + evidence-message excerpt,
  and (for behavioral fixtures) the `source` field (`tool-derived` with the leaderboard tool name + version, or
  `synthetic-minimal-tool` with the synthetic tool's source path + parser library version).
- *Smoke test — fixture loads.* For each fixture under `tests/fixtures/known-shapes/`, `anc audit <fixture-path>`
  completes without panicking against the current pre-tightening detection.
- *Smoke test — sh-script reproduces the value enumeration from captured stderr.* For each behavioral fixture with
  `expected-stderr.txt`, the test runs the sh-script with the bad-arg-VALUE argv and asserts that the script's stderr
  *contains* the value-enumeration substring extracted from the captured file (e.g., `must be one of [text, json]`).
  This is the property U1's regex actually depends on. Byte-for-byte equality is deliberately avoided — real-tool stderr
  drifts on ANSI color, version strings, locale, `argv[0]` echo, and trailing whitespace, none of which affect U1's
  detection.
- *Smoke test — source fixtures parse.* For each Rust source fixture, the test shells out to `cargo metadata --offline
  --manifest-path <fixture>/Cargo.toml --format-version=1` and asserts exit zero — `--offline` avoids network flake on
  CI; the shell-out posture matches `tests/dogfood.rs`'s existing approach for `anc audit` and avoids growing
  `[dev-dependencies]` with the `cargo_metadata` crate. For each Python source fixture, the test parses the `.py` files
  via the existing `Project::discover` path. Catches manifest-parseability problems (malformed `Cargo.toml`, missing
  `[package]`, unresolvable path-deps); does NOT cover detection-relevant issues — those land in U5's verdict-asserting
  tests.
- *Verdict-asserting tests deferred to U5.* The tests that assert each fixture resolves to its documented expected
  verdict require U1/U2/U3 detection to be present and land in U5.

**Verification:**

- `tests/fixtures/known-shapes/` directory exists with the documented layout.
- `tests/fixtures/known-shapes/README.md` enumerates every fixture, its expected verdict, and (for behavioral fixtures)
  its `source` field — `tool-derived` or `synthetic-minimal-tool`.
- Every behavioral fixture has an `expected-stderr.txt` captured under the normalized environment, plus a `capture.sh`
  documenting the capture invocation.
- `cargo test --test integration` passes — U4's new smoke tests (no-panic, value-enumeration substring, `cargo metadata
  --offline`) succeed against current detection.
- The PR description for U4 enumerates which behavioral fixtures are `synthetic-minimal-tool`-derived and which
  leaderboard tool + version was the source for each `tool-derived` fixture.

---

- U5. **Dogfood guards, coverage matrix regen, and changelog with audience-shift enumeration**

**Goal:** Close the loop. Land integration tests that exercise the U4 fixture matrix against the U1/U2/U3 detection;
strengthen `tests/dogfood.rs` to assert the new verdicts on `anc` itself; regenerate the coverage matrix artifacts;
write a changelog entry that explicitly enumerates audience-classifier label shifts on currently-scored tools.

**Requirements:** R10, R11, R13, R14.

**Dependencies:** U1, U2, U3 (detection must be implemented before the integration tests can assert the fixture
verdicts).

**Files:**

- Modify: `tests/integration.rs` — add fixture-matrix verdict-asserting tests for each U4 fixture (parser-family
  behavioral verdicts; Rust + Python source-tier verdicts). **Replaces** the U4 verdict-overlapping smoke tests (`anc
  audit <fixture>` no-panic + sh-script value-enumeration substring) — these are subsumed by the verdict-asserting
  tests, so they are deleted in this unit. The structural smoke tests stay (`cargo metadata --offline` parseability and
  the per-fixture `Project::discover` Python parse) — they catch a different class of drift the verdict tests don't
  surface.
- Modify: `tests/dogfood.rs` — add explicit assertions for `p2-json-output` Pass at Confidence::Medium and
  `p2-structured-output` Pass at Confidence::High against the agentnative-cli repo itself.
- Regenerate: `docs/coverage-matrix.md` (via `anc emit coverage-matrix`).
- Regenerate: `coverage/matrix.json` (via the same command).
- Modify: `CHANGELOG.md` — add the next-version entry (per project convention; not bumping VERSION here — `/ship`
  handles version bumps).

**Approach:**

- Integration tests run `anc audit <fixture-path> --output json` per fixture, parse the result, and assert the verdict
- Confidence + evidence-message excerpt match the fixture's documented expectations. Drift catches both regression
  (fixture passes but should fail) and rebound (fixture fails but should pass). Pattern follows
  `tests/dogfood.rs::collect_failed`.

- Dogfood guards extend the existing `tests/dogfood.rs` `dogfood_no_p2_fail_after_skill_subcommand` test with two
  additional assertions: `p2-json-output` resolves to Pass with `confidence == "medium"`, `p2-structured-output`
  resolves to Pass with `confidence == "high"`. R13 explicit.
- Coverage matrix regen is mechanical: `cargo run -- generate coverage-matrix` writes the artifacts; the existing
  `test_generate_coverage_matrix_drift_check_passes_on_committed_artifacts` integration test verifies parity. R14.
- The audience-classifier shift enumeration (R10) is a manual research step at the end of this unit: run `anc audit`
  against each currently-scored leaderboard member with the new detection, diff the `audience` field against the
  pre-merge baseline, list every shift in the changelog and PR description. The changelog entry names tools that shift
  `human-primary` → `mixed`, `mixed` → `agent-optimized`, etc. — each shift framed as a correctness win.
- `CHANGELOG.md` entry covers: behavioral upgrade (R1-R3), Rust tightening (R4-R6), Python sibling (R7-R9), and the
  enumerated audience shifts (R10). The "no schema bump" framing (R11) is mentioned for downstream consumers reading the
  entry.

**Execution note:** Coverage-matrix regen happens **after** all detection changes are merged into the branch but
**before** the PR is opened. CI's drift-check test would fail otherwise.

**Patterns to follow:**

- `tests/dogfood.rs::audit_repo_json` and `collect_failed` — JSON envelope parsing pattern for integration assertions.
- `docs/coverage-matrix.md` (current state) — the existing format, regenerated verbatim.

**Test scenarios:**

- *Integration — every U4 behavioral fixture resolves correctly.* For each fixture under
  `tests/fixtures/known-shapes/<family>/value-enum-echoes/`, `anc audit` reports the documented `p2-json-output` verdict
- Confidence.
- *Integration — every U4 source fixture resolves correctly.* For each fixture under
  `tests/fixtures/known-shapes/<family>/<tier>/`, `anc audit` reports the documented `p2-structured-output` (Rust) or
  `p2-structured-output-python` (Python) verdict + Confidence.
- *Dogfood — `anc` self-audit produces Pass + Medium for `p2-json-output`.* Assertion against the JSON envelope from
  `anc audit $CARGO_MANIFEST_DIR --output json`. **Covers R13.**
- *Dogfood — `anc` self-audit resolves `p2-structured-output` to its Strong-tier-resolution-dependent Confidence.* Same
  envelope. The asserted `confidence` value depends on the Strong-tier decision from `Open Questions → Deferred from
  Document Review`: `"high"` under option (b) "any same-crate `serde_json`", `"medium"` under option (a) "within-arm
  strict" (since anc's match arm calls `format_json` in `scorecard/`, not `serde_json` directly). The concrete value is
  set when that question is resolved; until then this scenario is parametric. **Covers R13.**
- *Coverage matrix drift — committed artifacts agree with registry + audits.*
  `test_generate_coverage_matrix_drift_check_passes_on_committed_artifacts` continues to pass after the regen. **Covers
  R14.**
- *Schema parity — scorecard JSON `schema_version` stays `"0.5"`.* `tests/scorecard_schema_v05.rs` continues to pass
  unchanged (R11).

**Verification:**

- `cargo test --test integration` passes for the new fixture-matrix assertions.
- `cargo test --test dogfood` passes for the strengthened guards.
- `cargo run -- generate coverage-matrix --check` exits 0.
- `cargo test --test scorecard_schema_v05` passes.
- The audience-shift list in `CHANGELOG.md` enumerates every currently-scored tool whose label changes — verifiable by
  cross-referencing against the pre-merge baseline.

---

## System-Wide Impact

- **Interaction graph:** The new bad-arg-VALUE probe in U1 invokes `BinaryRunner::run` against the target binary with a
  novel argv shape. The runner's existing cache (keyed by `(args, env)`) absorbs this transparently. No new caller of
  `BinaryRunner::run` outside the audit; no risk of fork-bomb regression because the probe argv is not bare.
- **Error propagation:** U1's probe failure modes (parser rejects with non-recognizable format, binary times out, binary
  crashes) all route through `runner.run`'s existing `RunStatus` taxonomy (`Crash`, `Timeout`, `Error`). Each case
  fall-throughs to U1's existing safe-suffix probes; if those also fail, the existing Warn message is emitted. No new
  error type, no new propagation surface.
- **State lifecycle risks:** None. Source audits and behavioral audits both operate on per-invocation state (parsed
  files / runner output) with no shared mutable state. Coverage-matrix regen is the only persistent-write step, and the
  drift-check test guards correctness.
- **API surface parity:** No change to the scorecard envelope shape, the CLI surface, or any exported type. The
  `Confidence` field on `p2-json-output` shifts from `High` (line 62 of json_output.rs) to `Medium` for **all** Pass
  paths — this is a deliberate honesty fix that downstream consumers need only feature-detect via the existing
  `confidence` JSON field. R11 stipulates schema_version stays `"0.5"`; consumers reading the field get accurate
  Confidence.
- **Integration coverage:** The U4 fixture matrix is the integration backbone for U1/U2/U3. Each fixture exercises the
  full audit pipeline (detection → AuditStatus → AuditResult → JSON envelope → consumer). Mocks alone would not catch
  the regex-extraction subtleties (U1) or the cross-file ast-grep aggregation (U2/U3) — the fixture matrix is
  load-bearing.
- **Unchanged invariants:**
- `src/scorecard/audience.rs::SIGNAL_AUDIT_IDS` is unchanged. Audience labels re-derive naturally from the new verdicts;
  the classifier rules are unchanged. R10 explicit.
- `schema_version` stays `"0.5"`. R11 explicit.
- `p2-json-output` and `p2-structured-output` retain their existing IDs and `covers()` declarations. No new requirement
  IDs added to `src/principles/registry.rs`. R6, R9 explicit.
- `arg_required_else_help = true` on `Cli` is preserved (CLAUDE.md "Dogfooding Safety"). The bad-arg-VALUE probe does
  not bare-invoke the target — the argv always contains an explicit flag value pair.
- Active output-envelope plan `docs/plans/2026-04-30-001` is not coupled to this plan at the requirements level. The
  bad-arg-VALUE probe primitive (U1) and that plan's U4 (`p2-should-json-envelope-on-error`) may share an implementation
  helper at code-time; if so, the coupling is documented in the U1 PR for future awareness.

---

## Risks & Dependencies

| Risk                                                                                                                                                    | Mitigation                                                                                                                                                                                                                            |
| ------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| U1's stderr-enumeration regex misses a parser family's idiomatic format → silent regression on currently-Pass tools.                                    | U4 lands first with one fixture per parser family encoding the literal stderr shape. U1 implementation is regex-iterated against the U4 fixtures; PR review audits fixture coverage against current leaderboard parser-family spread. |
| U2's ast-grep reachability misses a real Strong-tier wiring (false Medium) → reviewer misreads the verdict as a regression on a previously-Strong tool. | U2's evidence message names the detection gap explicitly. Confidence::Medium is honest under this doctrine. PR description enumerates every Rust-tool tier shift; reviewers can spot misclassifications.                              |
| U3's argparse / click pattern misses a Python idiom (e.g. `argparse.ArgumentParser` configured via class methods, click groups) → false Skip / Weak.    | U4 includes click + argparse fixtures at each tier. The Python sibling can be tightened iteratively without changing its public contract; first iteration covers the dominant idioms named in R7.                                     |
| Coverage-matrix regen runs before all detection changes land → committed artifacts disagree with detection → CI drift-check fails on the PR.            | U5 explicitly sequences regen as the **last** step before opening the PR. The pre-push hook (`scripts/hooks/pre-push`) runs the drift-check test and would catch the gap locally.                                                     |

---

## Documentation / Operational Notes

- **PR description must enumerate** (per R5, R10): every Rust tool whose `p2-structured-output` tier shifts, and every
  currently-scored tool whose `audience` label shifts, with the pre/post values. This is the reviewer's primary aid for
  validating the verdict shifts as deliberate.
- **`CHANGELOG.md` entry must include** the audience-shift enumeration (R10), the no-schema-bump framing (R11), and the
  cross-cutting "third evidence path" framing for the behavioral upgrade (R1-R3 in user-visible language). Pure internal
  refactoring (the shared helper module) does NOT belong in the changelog per CLAUDE.md "PR / Changelog source of
  truth."
- **Branch:** `feat/p2-json-output-bad-arg-and-source-tightening`, cut from `dev`, PR'd back to `dev`. Standard
  CLAUDE.md branch discipline.
- **Sequencing for `/ce-work`:** U4 → (U1, U2, U3 in parallel) → U5. U4 is the gate.
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
