---
title: UTF-8 Safe Evidence Previews - Plan
type: fix
date: 2026-09-16
status: completed
topic: utf8-safe-evidence-previews
artifact_contract: ce-unified-plan/v1
artifact_readiness: implementation-ready
product_contract_source: ce-plan-bootstrap
execution: code
---

# UTF-8 Safe Evidence Previews - Plan

## Goal Capsule

- **Objective:** A user auditing any source tree gets a scorecard. Today a project whose code contains a multi-byte
  character in the wrong column gets no result at all: the process aborts with a Rust panic message and exit 101, and
  the audited project learns nothing about itself.
- **Means:** Build the evidence preview from characters through one shared helper instead of slicing the string by byte
  offset in the audit file (KTD1, KTD2).
- **Authority hierarchy:** Requirements (R-IDs) win on behavior. Key Technical Decisions (KTD-IDs) win on mechanism
  within their cited R constraints. Units carry only local deltas.
- **Execution profile:** U1 is the fix and ships alone as PR #92, a `fix/…` branch cut from `dev`; it closes the crash
  and needs nothing from U2. U2 adds the recurrence guard as PR #93, a `fix/…` branch stacked on #92's and rebased onto
  `dev` once #92 merged.
- **Stop conditions:** Stop and report if the regression test passes against the unfixed code, since that means the
  fixture either short-circuits before reaching the formatter or is aligned so byte 80 lands on a char boundary (KTD4).
  Stop if the scoped lint in U2 reports no finding when the guarded helper is degraded, since that means the guard is
  vacuous.
- **Tail ownership:** The implementer owns the branch, the failing-first proof for each new test, and the PR. Brett owns
  the merge.

---

## Product Contract

### Summary

Replace the byte-indexed truncation behind the output-clamping audit's evidence preview with a character-based helper
that lives in the shared source-audit module. Add a regression test written at the symptom level, and a guard scoped to
the modules that produce evidence text: the shared helper's module and the source-audit subtree.

### Problem Frame

`anc audit <path> --source` aborts on any Rust file where a matched line carries a multi-byte character straddling byte
80 of the matched text. The preview builder slices the matched line by raw byte offset, which panics when that offset
falls inside a UTF-8 sequence. It reproduces on the first real third-party target it met: a source tree with a braille
spinner array inside a loop body.

Two properties make this worse than an ordinary crash. The release profile sets `panic = "abort"`, so nothing can catch
it: no per-file recovery, no partial scorecard, no structured error. And `anc` ships source audits that flag panic-prone
code, unhandled exits, and unstructured errors in other people's CLIs, so the tool fails a standard it administers.

The knowledge to prevent it has been in the solutions corpus for five months.
`docs/solutions/best-practices/rust-utf8-safe-string-truncation-2026-04-20.md` names this antipattern, supplies a
boundary-safe implementation, and ships a hunt command. The byte slice predates that doc: it has been in
`output_clamping.rs` since the v0.1 commit of 2026-04-01, nineteen days before the doc was filed, and the doc's hunt
command still finds it today, so the doc never reached this tree. That is why this plan carries an enforced guard and
not only a fix.

### Requirements

**Preview behavior**

- R1. The evidence preview never panics, for any input, at any budget. Source:
  `src/audits/source/rust/output_clamping.rs:142-153`.
- R2. The preview is bounded by characters, not bytes, and the ellipsis is appended only when truncation occurred.
- R3. A preview that truncates mid-word still renders as valid UTF-8 text on every surface that prints evidence. Source:
  `src/scorecard/mod.rs:642-647`.

**Ownership**

- R4. One helper owns evidence-preview truncation for source audits, and it lives in the shared source module rather
  than in an individual audit file. Source: `CLAUDE.md` § Source Audit Convention.

**Recurrence**

- R5. A regression test fails against the unfixed implementation and passes after it, asserting the audit returns a
  warning rather than aborting.
- R6. A mechanism outside prose prevents byte-indexed truncation from reappearing in the module that owns previews or in
  any source-audit module, current or future.

### Success Criteria

- `anc audit` on a tree containing braille, CJK, and emoji inside matched lines exits with a scorecard rather than exit
  101.
- The repo's own dogfood run is unchanged: `anc audit` against this crate reports the same scorecard it does today.
- A reader of the changed module can tell why byte slicing is refused there without reading this plan.

### Scope Boundaries

- The other nine source audits emit unbounded evidence with no truncation at all. They do not panic; they print long
  lines. Adopting the shared helper across them changes evidence output for every one, so it is deferred rather than
  folded in here.
- No change to what the output-clamping audit detects, to `CLAMP_PATTERNS`, or to `CLAMP_STRINGS`. This plan changes how
  evidence is formatted, never which files warn.
- No error-isolation redesign. Per-file panic recovery is not implementable while the release profile aborts (KTD5).

#### Deferred to Follow-Up Work

- An `anc` source audit for string slicing by a non-boundary-derived index. This is the guard that would actually cover
  the class, and `anc` is the right tool for it: its ast-grep-based source analysis can distinguish an index derived
  from `find()` from one derived from a budget, which a text-level lint cannot. It also generalizes to every CLI `anc`
  audits, and it has a sibling precedent in `src/audits/source/rust/unwrap.rs`. Out of this plan because it is a product
  feature, not a fix.
- Adopting the shared preview helper across the nine audits that currently emit unbounded evidence, closing `anc`'s own
  output-clamping gap against itself.
- A shared non-ASCII fixture set. The corpus has no convention for where adversarial text fixtures live, and this is the
  second UTF-8 incident in five months.

### Sources

- `docs/solutions/best-practices/rust-utf8-safe-string-truncation-2026-04-20.md`: the antipattern, the boundary-safe
  implementation, and the hunt command. Its own hunt regex finds one offender here; the clippy lint finds thirty
  string-slice sites, so the regex under-reports the class it was written to catch.
- `docs/solutions/best-practices/audit-scripts-as-documentation-immune-system-2026-04-20.md`: filed the same day as the
  truncation doc; argues prose without enforcement decays into fiction. This plan is the case study.
- `docs/solutions/logic-errors/regex-diff-heuristic-cannot-gate-public-api-breakage.md`: a regex that structurally
  cannot match the gated property is not a gate; the recorded resolution replaced it with a real analyzer.
- `docs/solutions/best-practices/ci-eprintln-audit-script-2026-04-20.md` and
  `docs/solutions/best-practices/reliable-static-analysis-compliance-checkers-20260327.md`: the in-family precedent for
  a small CI grep gate, and the recorded failure of that shape in this product line, where a checker silently reported
  zero for every check.
- `docs/solutions/best-practices/generalize-incident-prevention-into-symptom-level-regression-check.md`: shapes what R5
  asserts.
- `docs/solutions/best-practices/prove-a-freshly-authored-guards-tests-are-non-vacuous-by-temporarily-degrading-the-guard.md`:
  the acceptance method for U2.
- `Cargo.toml:96-100`: `[profile.release] panic = "abort"`.

---

## Planning Contract

### Key Technical Decisions

- KTD1. **Truncate by characters, not by a byte budget.** The 80 is a one-line display budget with no external byte
  constraint: evidence prints with a fixed indent and no width clamp. Character truncation is both display-correct and
  simpler than finding a safe byte boundary. Instantiates R2.
- KTD2. **The helper moves to `src/source.rs`.** `CLAUDE.md` § Source Audit Convention already forbids private
  per-language helpers in individual audit files for cross-language concerns, and a file-local helper is exactly what
  shipped this bug. An evidence preview is language-agnostic. Instantiates R4.
- KTD3. **The recurrence guard is `clippy::string_slice` denied at the module that owns previews and at the source-audit
  parent module, not workspace-wide.** Measured on this tree, the lint flags thirty sites and twenty-nine are safe by
  construction, deriving their index from `find()` or an ASCII byte scan. A workspace deny would need twenty-nine
  `#[allow]` annotations, and a wall of allow annotations is the rubber-stamp failure the guard exists to prevent.
  Restriction lints are allow-by-default, so `-Dwarnings` in CI and the pre-push hook do not reach this class without an
  explicit opt-in. The deny lives in two places: `src/source.rs`, where the shared helper does budget-based truncation,
  and `src/audits/source/mod.rs`, from which it propagates to every current and future audit file, since a file-local
  helper inside an audit file is the shape that shipped this bug (KTD2). Seven pre-existing slice sites under the Rust
  audits derive their index from `find()` or an ASCII scan and carry `#[expect(clippy::string_slice, reason = "…")]`; an
  expectation fails the build if the lint stops firing at that site, so it is a per-site justification, not the allow
  wall a workspace deny would need. Rejected: a workspace lint (noise ratio above); a CI grep gate (a regex cannot see a
  slice whose bound is a parameter, and that shape has a recorded silent-zero failure in this product line); the
  regression test alone (the documented hunt command went unrun against this tree for five months). The class-level
  guard is deferred to the `anc` audit rule in Scope Boundaries. Instantiates R6.
- KTD4. **The regression test asserts at symptom level.** It asserts that the audit returns a warning for source
  containing multi-byte characters in matched text, across several scripts, rather than pinning to the braille glyph
  from the original report. A test scoped to the incident's exact mechanism catches that mechanism and not the class it
  revealed.
- KTD5. **No panic-recovery design.** `Cargo.toml:100` sets `panic = "abort"` for release, so `catch_unwind` works under
  `cargo test` and dies in a shipped binary. The audit engine already converts an `Err` return into an error status per
  audit without aborting the run, so a non-panicking helper is the whole fix. Instantiates R1.

### Risks & Dependencies

- **The fixture can silently stop testing.** `CLAMP_STRINGS` is `--limit`, `--max`, `limit`, `max_results`, and
  `page_size`, matched with `contains` against the whole file, and `CLAMP_PATTERNS` matches any `.take(…)` or
  `.clamp(…)` call. A fixture containing any of these anywhere short-circuits the audit to `Pass` before the evidence
  formatter runs, and the test then passes against the unfixed code. The stop condition in the Goal Capsule exists for
  this. Source: `src/audits/source/rust/output_clamping.rs:22-25`.
- **The glyph must straddle byte 80 of the matched text.** A spinner array hoisted to a module-level constant does not
  reproduce, because the matched node text stays ASCII. Nor does a glyph run that happens to align: the unfixed slice
  panics only when the collapsed matched-node text exceeds 80 bytes and byte 80 falls inside a multi-byte sequence. For
  a run of w-byte glyphs starting at byte offset o of that text, that holds when (80 − o) is not a multiple of w.
  Braille and CJK glyphs are 3 bytes and emoji are 4, so each script's fixture must be positioned against this rule
  independently; a fixture that truncates cleanly at a boundary is green against the unfixed code without any clamp
  trigger. The multi-byte characters must stay inside the loop body or the collect chain.
- **The scoped deny is narrower than the class.** Every source-audit file is covered, but modules outside
  `src/source.rs` and the source-audit subtree (the behavioral audits, the help probe) are not, and a byte slice written
  there is unguarded. R4 and code review carry that residual, and the deferred `anc` audit rule is the real remedy.

### Sequencing

U1 is independent and ships first: it removes the crash and depends on nothing in U2. U2 follows because its non-vacuity
proof degrades the helper U1 creates.

---

## Implementation Units

### U1. Character-bounded preview helper and regression test

- **Goal:** `anc audit --source` completes on a tree with multi-byte characters in matched lines, and the evidence
  preview reads correctly.
- **Requirements:** R1, R2, R3, R4, R5; KTD1, KTD2, KTD4, KTD5.
- **Dependencies:** none.
- **Files:** `src/source.rs`, `src/audits/source/rust/output_clamping.rs`, `tests/integration.rs`,
  `tests/fixtures/hostile-utf8-evidence/Cargo.toml` (new), `tests/fixtures/hostile-utf8-evidence/src/main.rs` (new).
- **Approach:**
  1. Add a character-bounded preview helper to `src/source.rs`, alongside the existing shared source-audit helpers. It
     collapses newlines to spaces as the current code does, bounds the result by character count, and appends the
     ellipsis only when it truncated.
  2. Delete the file-local helper in `output_clamping.rs` and call the shared one at the single existing call site.
  3. Add the inline regression tests to the existing `#[cfg(test)] mod tests` in `output_clamping.rs`.
  4. Add a fixture crate under `tests/fixtures/hostile-utf8-evidence/`, shaped like the existing source-fixture crates
     (a `Cargo.toml` plus `src/*.rs`, never built), whose source reproduces the abort against the unfixed binary.
  5. Add an integration test to `tests/integration.rs` that runs the built binary against the fixture with `--source`
     and asserts a scorecard rather than exit 101, so the fixture has a permanent consumer.
- **Execution note:** Observe the panic first. Run the new test against the unfixed helper and quote the abort, then
  fix. A green first run has two causes to check before editing the fixture: a clamp trigger (a `CLAMP_STRINGS`
  substring or a `.take`/`.clamp` call) or byte 80 of the collapsed matched text landing on a char boundary.
- **Patterns to follow:** `docs/solutions/best-practices/rust-utf8-safe-string-truncation-2026-04-20.md` for the
  boundary rule; `tests/fixtures/source-only` for fixture shape and `test_source_only_fixture` in `tests/integration.rs`
  for the binary-level assertion (the `hostile-*` fixtures are behavioral probe scripts, not source crates); `CLAUDE.md`
  § Source Audit Convention for test style, which calls `audit_output_clamping()` and matches on `AuditStatus` directly.
- **Test scenarios:**
  - Happy path: source with a list pattern and only ASCII in the matched text returns a warning whose preview is
    unchanged from today.
  - Edge: a matched line carrying braille spinner glyphs across the budget returns a warning rather than aborting.
  - Edge: the same shape with CJK characters, and again with an emoji, each returns a warning. This is the symptom-level
    assertion KTD4 requires.
  - Edge: a matched line shorter than the budget gets no ellipsis; one longer gets exactly one.
  - Edge: a matched line whose characters are all multi-byte truncates to the character budget, not to a shorter
    byte-derived count.
  - Error: the fixture crate, audited through the binary with `--source`, exits with a scorecard rather than exit 101.
- **Verification:** The output-clamping unit tests pass, including the new cases. Auditing the new fixture with
  `--source` produces a scorecard. The dogfood run against this crate reports the same scorecard as before the change.

### U2. Scoped deny on byte-indexed string slicing

- **Goal:** A byte-indexed truncation cannot reappear in the module that owns evidence previews or in any source-audit
  module without failing the build.
- **Requirements:** R6; KTD3.
- **Dependencies:** U1.
- **Files:** `src/source.rs`, `src/audits/source/mod.rs`, `src/audits/source/rust/unwrap.rs`,
  `src/audits/source/rust/headless_auth.rs`, `src/audits/source/rust/error_types.rs`,
  `src/audits/source/rust/exit_codes.rs`.
- **Approach:**
  1. Add an inner `#![deny(clippy::string_slice)]` attribute to `src/source.rs` and to `src/audits/source/mod.rs`, each
     with a comment naming the invariant it protects rather than the incident that prompted it.
  2. Annotate the seven pre-existing slice sites under `src/audits/source/rust/` (three in `unwrap.rs`, two in
     `headless_auth.rs`, one each in `error_types.rs` and `exit_codes.rs`) with `#[expect(clippy::string_slice, reason =
     "…")]`, each reason naming the boundary the index is derived from.
  3. Confirm the rest of the tree is unaffected, since the attributes are module-scoped and the lint is allow-by-default
     elsewhere.
- **Execution note:** Prove the guard is not vacuous before declaring it done. Temporarily replace the helper's body
  with a byte-indexed slice and confirm clippy reddens naming that line and that lint; then temporarily add a byte slice
  to one audit file and confirm the same; then restore both. A guard whose test can only pass is not a guard.
- **Patterns to follow:**
  `docs/solutions/best-practices/prove-a-freshly-authored-guards-tests-are-non-vacuous-by-temporarily-degrading-the-guard.md`.
- **Test scenarios:**
  - Integration: with the helper degraded to a byte slice, `cargo clippy` fails and its output names
    `clippy::string_slice` and the helper's line.
  - Integration: with a byte slice added to any audit file under `src/audits/source/`, `cargo clippy` fails naming that
    file.
  - Integration: with the helper and audit files intact, `cargo clippy --all-targets` is clean under `-Dwarnings`, no
    other file gains a warning, and no `unfulfilled_lint_expectations` warning appears at the seven annotated sites.
- **Verification:** Clippy is clean on the untouched tree and red on the degraded one, with the degradation reverted
  before the PR opens.

---

## Verification Contract

| Gate                 | Command                                                   | Proves                                                                  |
| -------------------- | --------------------------------------------------------- | ----------------------------------------------------------------------- |
| Format               | `cargo fmt --check`                                       | Formatting matches the repo.                                            |
| Lint                 | `RUSTFLAGS=-Dwarnings cargo clippy --all-targets`         | No warnings, and U2's scoped deny holds.                                |
| Unit and integration | `cargo test`                                              | The new output-clamping cases pass and the existing suite is unchanged. |
| Fixture audit        | `anc audit tests/fixtures/hostile-utf8-evidence --source` | Exits with a scorecard rather than exit 101.                            |
| Dogfood              | `anc audit .`                                             | This crate's own scorecard is unchanged by the fix.                     |
| Supply chain         | `cargo deny check`                                        | Unchanged dependency posture.                                           |

Activate the local mirrors of the CI gates with `git config core.hooksPath scripts/hooks`.

---

## Definition of Done

Global:

- Each new test was observed failing against the unfixed code, and the failure output is quoted in the PR.
- U2's guard was observed failing against a degraded helper and against a degraded audit file, and both degradations are
  reverted.
- No comment in the diff narrates the change. A comment naming the character-boundary invariant is correct; one
  describing what the code used to do is not.
- No abandoned-approach code remains in the diff.
- The PR fills `.github/pull_request_template.md`, with the user-visible fix under the Changelog block's `### Fixed`.

| U-ID | Done when                                                                                                                                                                                                                                                                    |
| ---- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| U1   | Multi-byte characters in matched text produce a warning instead of an abort, across braille, CJK, and emoji; the preview helper lives in `src/source.rs` and the audit file holds no local copy; the fixture audits to a scorecard.                                          |
| U2   | `src/source.rs` and `src/audits/source/mod.rs` deny `clippy::string_slice`; the seven pre-existing audit slice sites carry `#[expect]` with reasons; a degraded helper and a degraded audit file each redden clippy naming that lint; the rest of the tree gains no warning. |
