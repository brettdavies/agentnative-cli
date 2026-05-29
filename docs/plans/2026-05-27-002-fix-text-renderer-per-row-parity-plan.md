---
title: "fix: Route the text renderer through the per-row + propagation pipeline so text and JSON agree"
type: fix
status: completed
date: 2026-05-27
origin: U2 (PR #62 / commit 3839696, scorecard 7-status taxonomy + schema 0.6) shipped the per-row pipeline on the JSON path only; the `--output text` path was never migrated. Repro by auditing `bat`.
---

# fix: Route the text renderer through the per-row + propagation pipeline so text and JSON agree

## Summary

U2 introduced the requirement-row emission model (schema 0.6): each raw probe `AuditResult` is fanned out into one
result per requirement row it `covers()`, then conditional rows have their status rewritten by antecedent propagation
(Decision-2a). That pipeline runs **only** inside `build_scorecard()` on the JSON path. The default human view
(`--output text`) and the text-mode badge are computed from the **raw** probe results, so the terminal shows probe ids
instead of requirement-row ids, can never render `[N/A ]` (only propagation produces `n_a`), and its badge score and row
counts can diverge from the JSON the site/scorer consume.

This plan extracts the per-row + propagation step into one reusable function that both the text path and
`build_scorecard()` call, so a single source of truth backs both surfaces. No new renderer arms are added — the `OptOut`
and `NotApplicable` match arms already exist in `format_text` / `format_text_raw`; the per-row+propagated data simply
never reaches them today.

## Problem Frame

### text vs JSON divergence

`anc audit` has two terminal surfaces over one result set:

- **JSON** (`--output json`, consumed by `agentnative-site` and the scorer). `main::run` builds `RunMetadata` and calls
  `format_json(&results, &all_audits, …)` → `build_scorecard()`, which runs `fan_out_per_row(raw, catalog)` then
  `propagate_antecedents(&mut rows, raw)` before constructing the JSON view, `summary`, `score_pct`, and `badge`
  (`src/scorecard/mod.rs:744-786`).
- **Text** (`--output text`, the default human view). `main::run` calls `compute_badge(&results, …)` and
  `format_text(&results, quiet, Some(&badge), opts)` — both fed the **raw** `Vec<AuditResult>` with no fan-out and no
  propagation (`src/main.rs:284-293`).

`format_text` groups by `AuditGroup` and prints each raw audit's `.id` and `.label` (`src/scorecard/mod.rs:519-565`).
That `.id` is the probe id (`p2-schema-print`), never the requirement-row id (`p2-must-schema-print`). Because `n_a` is
produced **only** by `propagate_antecedents` — which the text path never calls — the `NotApplicable` arm in
`format_text` (lines 549-554) and `format_text_raw` (line 614) is dead on the text path. The arm exists; the data never
arrives.

### The `bat` repro

`bat` ships no structured-output flag, so `p2-json-output` resolves to `opt_out`, and the conditional
`p2-must-schema-print` row (antecedent `p2-json-output`) collapses to `n_a` via Decision-2a.

- `anc audit --command bat --output json` → correct: `opt_out` on `p2-must-output-flag`, `n_a` on
  `p2-must-schema-print`. ~43 rows.
- `anc audit --command bat` (text) → prints `[FAIL] … (p2-schema-print)` — the raw probe id, the wrong status, no `[N/A
  ]` badge anywhere. ~41 badges. Badge score computed from raw can differ from the JSON badge score.

### User-facing harm

A human reading the terminal concludes `bat` **failed** a MUST (`p2-must-schema-print`) it is actually **exempt** from.
The JSON path is unaffected, so this is text-only — but text is the default view, so it is the surface most humans see.
The maintainer wants this fixed before the U2 release; it is not part of U3 (scoring-formula work).

## Scope Boundaries

**In scope:**

- Extract the per-row + propagation step (`fan_out_per_row` → `propagate_antecedents`) into one reusable function called
  by both `build_scorecard()` and the text path, so text and JSON share one source of truth.
- Rewire the text path in `main::run` so `format_text` and `compute_badge` consume the per-row + propagated results
  (carrying `audit_id` provenance for display) rather than the raw probe results.
- Render the requirement-row id + tier + propagation evidence in the terminal (e.g. `[N/A ] p2-must-schema-print (must)
  — antecedent p2-json-output is opt_out`).
- Decide and document whether `exit_code` moves from raw to per-row results (see Key Technical Decisions — the trickiest
  call).
- Regression test reproducing the `bat` case: a probe emitting `opt_out` + a conditional consequent → text shows `n_a`
  (not `fail`) on the consequent row; text row count == JSON row count; text badge score == JSON badge score.
- Update existing text-render unit tests (`format_text_*`, `format_text_raw_emits_id_tab_status_per_line`,
  `format_text_color_wraps_status_prefix`) and any integration/dogfood text-mode assertions affected by per-row ids.

**Out of scope:**

- U3 scoring-formula work (whether `opt_out` re-enters the denominator and at what weight). `score_pct` semantics are
  untouched; this plan only changes which result set feeds it on the text path so it matches the JSON path.
- Removal of the implicit-default-subcommand (`inject_default_subcommand`) — a separate plan.
- Any change to the JSON shape or schema version. Schema stays `0.6`; the JSON path already produces correct output.
- Changes to `fan_out_per_row` / `propagate_antecedents` behavior. They are correct; this plan reuses them.

## Context & Research

### The pipeline lives in `build_scorecard` only

`src/scorecard/mod.rs:737-788` — `build_scorecard()`:

```rust
let mut row_results = fan_out_per_row(raw_results, ran_audits);   // 744
propagate_antecedents(&mut row_results, raw_results);             // 745
…
let per_row_only: Vec<AuditResult> = row_results.iter().map(|(r, _)| r.clone()).collect();  // 767
let badge = compute_badge(&per_row_only, &tool.name);             // 768
…
summary: build_summary(&per_row_only),                            // 776
results: row_results.iter().map(|(r, audit_id)| AuditResultView::from_row(r, audit_id)).collect(),  // 772-775
```

`row_results: Vec<(AuditResult, String)>` — the `String` is the originating probe `audit_id`. The JSON view threads it
into `AuditResultView::from_row` for provenance, then the pairing is discarded. **No caller outside `build_scorecard`
ever sees `row_results`.**

`fan_out_per_row` (lines 633-653): one entry per `covers()` row, status/label/group/layer/confidence copied from the
probe, `id` replaced with the row id, `audit_id` preserved. Audits with empty `covers()` pass through keyed by their own
id.

`propagate_antecedents` (lines 672-704): for each row whose registry entry is `Applicability::Conditional { antecedent:
Some(ante), .. }`, looks up the antecedent probe's raw status and rewrites the row: `pass`/`warn`/`fail` → unchanged;
`opt_out`/`n_a` → `NotApplicable` with a reason citing the antecedent id and its status; `skip` → `Skip`; `error` →
`Error`. **This is the only producer of `n_a` in the system.**

### The text path feeds raw results to both renderer and badge

`src/main.rs:284-293` — the `OutputFormat::Text` arm:

```rust
OutputFormat::Text => {
    let tool_name = derive_tool_name(command_name.as_deref(), &project);
    let badge = compute_badge(&results, &tool_name);     // raw results
    let opts = TextOptions { raw: cli.raw, color: color::should_color(cli.color) };
    format_text(&results, quiet, Some(&badge), opts)     // raw results
}
```

`src/main.rs:327` — `Ok(exit_code(&results))` — also raw, shared by both output modes.

`format_text` (`src/scorecard/mod.rs:507-600`) groups by `AuditGroup`, prints `[<PREFIX>] {r.label} ({r.id})`, then a
summary line, then the badge hint. The status-prefix match (lines 534-562) **already has `OptOut("OPT ")` and
`NotApplicable("N/A ")` arms** and a quiet-skip for both. The evidence block (lines 566-579) already prints the
`OptOut`/`NotApplicable` reason. `format_text_raw` (lines 606-621) **already maps `OptOut → "OPT_OUT"` and
`NotApplicable → "N_A"`.** Confirmed: renderer arms are present; only the data flow is missing.

### Existing tests that pin current (raw) text behavior

`src/scorecard/mod.rs` — `format_text_appends_hint_when_badge_eligible` (1638),
`format_text_omits_hint_when_below_floor` (1653), `format_text_without_badge_arg_is_unchanged` (1667),
`format_text_raw_emits_id_tab_status_per_line` (1677), `format_text_color_wraps_status_prefix` (1694). These build raw
`AuditResult`s with synthetic ids (`c1`…`c4`) and call `format_text`/`format_text_raw` directly — they exercise the
*formatter*, not the *pipeline*. They keep working unchanged **iff** the formatter signature still accepts a result set;
if the formatter switches to a per-row+provenance input, these tests update to pass the new shape.

`tests/integration.rs:148-155` asserts quiet text omits `[PASS]`/`[SKIP]`. `tests/integration.rs:130-145` asserts quiet
< normal byte length. `tests/dogfood.rs` asserts over **JSON** `results[]` (unaffected). No integration/dogfood test
currently asserts a specific text-mode requirement-row id, so the blast radius on text assertions is small.

### The shared-pipeline extraction design

Extract the two-line pipeline into a reusable function so both call sites are one source of truth:

```rust
/// Fan raw probe results out to per-requirement rows and apply antecedent
/// propagation. The sole producer of the per-row result set consumed by
/// every output surface (text + JSON) and the exit code.
pub fn build_row_results(
    raw: &[AuditResult],
    catalog: &[Box<dyn Audit>],
) -> Vec<(AuditResult, String)> {
    let mut rows = fan_out_per_row(raw, catalog);
    propagate_antecedents(&mut rows, raw);
    rows
}
```

`build_scorecard` calls it instead of inlining the two lines (no behavior change — pure refactor of two existing
statements). The text path in `main::run` calls it too, then maps to the shape `format_text`/`compute_badge` need.

For the text renderer, the cleanest input is `&[(AuditResult, String)]` (row + audit_id provenance) so the terminal can
optionally surface the probe id and so the row id (`r.id`) is the requirement-row id. `compute_badge` and `exit_code`
take `&[AuditResult]`; feed them the projected `Vec<AuditResult>` (drop the provenance string, same projection
`build_scorecard` already does at line 767).

## Key Technical Decisions

1. **One per-row pipeline, two consumers.** Add `build_row_results(raw, catalog) -> Vec<(AuditResult, String)>` in
   `src/scorecard/mod.rs`. `build_scorecard` and the text path both call it. This is the fix's spine: text and JSON can
   no longer disagree on the row set because they derive from the same function. `fan_out_per_row` and
   `propagate_antecedents` stay `pub` (already are) but the canonical entry point becomes `build_row_results`.

2. **The text renderer consumes per-row + provenance.** Change `format_text` (and `format_text_raw`) to take the per-row
   results so `r.id` is the requirement-row id and the probe `audit_id` is available for display. Grouping still keys on
   `AuditGroup` (each row carries the probe's group). The badge and summary are computed from the projected
   `Vec<AuditResult>`, identical to `build_scorecard`'s `per_row_only`.

3. **Terminal rendering of row id + tier + propagation evidence.** Print the requirement-row id, and for conditional
   rows that propagated to `n_a`, show the antecedent evidence the propagation already wrote into the reason string
   (e.g. `[N/A ] p2-must-schema-print (must) — antecedent p2-json-output is opt_out`). The tier (`must`/`should`/`may`)
   comes from `registry::find(&r.id)` (the same lookup `AuditResultView::from_row` uses at lines 405-409); render `null`
   tier defensively (omit the `(tier)` suffix) so an unregistered row id never panics. The propagation reason is already
   in `r.status`'s `NotApplicable(reason)` payload — the existing evidence block prints it; only the header line needs
   the tier suffix.

4. **exit_code: raw vs per-row — the load-bearing decision.** Today `exit_code(&results)` (`src/main.rs:327`) uses
   **raw** probe results; the JSON `summary` and `score_pct` already reflect **per-row** results. So a tool whose raw
   probe `Fail`s an audit whose requirement row propagates to `n_a` exits `2` in **both** text and JSON today (exit code
   is shared, computed once from raw), yet the per-row truth is "not applicable." Concretely: `bat`'s `p2-schema-print`
   probe raw-Fails → `exit_code(raw)` = 2 → `anc audit --command bat` exits 2 even though the requirement is `n_a`.

   **Decision: move `exit_code` to the per-row + propagated result set.** Rationale: the per-row set is the post-U2
   source of truth — it is what `summary`, `score_pct`, and `badge` already key on. Leaving `exit_code` on raw keeps a
   third, contradictory view of the same run (text shows `n_a`, JSON summary counts `n_a`, but the process exits `2` as
   if it failed). Moving it to per-row makes a propagated-to-`n_a` row stop lifting the exit code — exactly the
   "prerequisite absent, requirement does not apply" semantics Decision-2a encodes. `NotApplicable` and `OptOut` are
   already excluded from `score_pct`'s denominator (lines 220-223); `exit_code` should mirror that: only `Fail`/`Error`
   on a **live** row drives exit 2, only `Warn` drives exit 1.

   **Tension to document:** this changes observable exit behavior for any tool with a raw-Fail probe under an unmet
   conditional antecedent (today exit 2 → after fix exit 0/1). For `bat` that is the *correct* change (it is exempt, not
   failing). It does **not** affect the `--audit-profile` suppression path: suppressed audits emit `Skip` with the
   sentinel and never had a row that fan-out would Fail, so the R4 masking test
   (`exit_code_drops_when_audit_profile_suppresses_a_would_have_failed_audit`, lines 1283-1321) still holds — verify it
   does. The change is "exit code now agrees with the per-row truth the JSON already reports," not a new masking
   surface. The alternative (keep `exit_code` on raw) is rejected: it would leave text/JSON/exit-code as three
   inconsistent views and re-open the exact class of bug this plan closes.

## Implementation Units

- [ ] **Unit 1: Extract `build_row_results` (pure refactor, no behavior change)**

**Goal:** Single source of truth for the per-row + propagation pipeline.

**Files:**

- Modify: `src/scorecard/mod.rs` — add `pub fn build_row_results(raw, catalog) -> Vec<(AuditResult, String)>`; change
  `build_scorecard` to call it in place of the inlined `fan_out_per_row` + `propagate_antecedents` (lines 744-745).

**Approach:** Mechanical extraction. `build_scorecard`'s output is byte-identical before/after.

**Test scenarios:**

- New unit `build_row_results_fans_out_then_propagates`: probe `p2-json-output = opt_out` + consequent → row
  `p2-must-schema-print` is `NotApplicable`, matching the existing
  `propagation_collapses_consequent_when_antecedent_is_opt_out`.
- Existing `fan_out_*` and `propagation_*` tests still pass (they call the primitives directly — unchanged).

**Verification:** `cargo test scorecard` green; `anc audit . --output json` byte-identical to pre-change.

---

- [ ] **Unit 2: Route the text path through `build_row_results`**

**Goal:** `format_text` and the text-mode badge consume per-row + propagated results, not raw.

**Files:**

- Modify: `src/main.rs` (text arm, lines 284-293) — call `build_row_results(&results, &all_audits)`, project to
  `Vec<AuditResult>` for `compute_badge`, pass the per-row data to `format_text`.
- Modify: `src/scorecard/mod.rs` — change `format_text` / `format_text_raw` to accept the per-row result set (with
  `audit_id` provenance available); render the requirement-row id and, for conditional rows, the tier suffix.

**Approach:** The text arm mirrors `build_scorecard`'s projection (`row_results.iter().map(|(r,_)| r.clone())`). Decide
the renderer signature: simplest is `format_text(rows: &[(AuditResult, String)], …)` and project internally for the
summary/badge. Tier via `registry::find(&r.id)`, defensively `None`-tolerant.

**Test scenarios:**

- Per-row ids appear in text output (`p2-must-schema-print`, not `p2-schema-print`).
- `[N/A ]` badge renders with antecedent evidence for a propagated conditional row.
- `--raw` emits the per-row id + `N_A` token line (`format_text_raw`).
- `--quiet` still skips `PASS`/`SKIP`/`OPT`/`N/A` rows (existing quiet arms cover the new statuses).
- `--color` still wraps the status prefix in ANSI.

**Verification:** `anc audit --command bat` text shows `[N/A ] p2-must-schema-print` and no `[FAIL] …
(p2-schema-print)`.

---

- [ ] **Unit 3: Move `exit_code` to the per-row result set**

**Goal:** Process exit code reflects the per-row truth, matching `summary` / `score_pct` / `badge`.

**Files:**

- Modify: `src/main.rs:327` — compute the per-row projection once (shared with Unit 2's text arm and reusable for both
  output modes) and pass it to `exit_code`.

**Approach:** Build `row_results` once near where `results` is finalized (after the `--principle` retain), project to
`Vec<AuditResult>`, use it for `exit_code` in both arms. Avoid recomputing the pipeline twice in the text arm.

**Test scenarios:**

- A raw-Fail probe under an unmet conditional antecedent → per-row `n_a` → `exit_code` 0 (not 2). New unit pinning this.
- The R4 audit-profile masking test (`exit_code_drops_when_audit_profile_suppresses_a_would_have_failed_audit`) still
  passes unchanged.
- A genuine live `Fail` row still exits 2; a live `Warn` still exits 1.

**Verification:** `anc audit --command bat` exits 0; `echo $?` agrees between text and JSON modes.

---

- [ ] **Unit 4: Regression test — text/JSON parity on the `bat` shape + update affected tests**

**Goal:** Lock the fix against re-divergence; bring existing tests onto the per-row contract.

**Files:**

- Modify: `src/scorecard/mod.rs` tests — update `format_text_*` and `format_text_raw_emits_id_tab_status_per_line` /
  `format_text_color_wraps_status_prefix` for the per-row input shape.
- Add: a parity regression test (unit or `tests/integration.rs`) building the `bat`-like fixture — a probe emitting
  `opt_out` (`p2-json-output`) + a conditional consequent (`p2-must-schema-print`) — and asserting: (a) text output
  shows `n_a` (not `fail`) on the consequent row; (b) text row count == JSON `results.len()`; (c) text badge score ==
  JSON `badge.score_pct`.
- Audit `tests/integration.rs` quiet/byte-length text assertions still hold under per-row counts.

**Approach:** The parity test renders the same fixture through both `format_text`/`compute_badge` and `format_json`,
then diffs the per-row status set and the badge score. This is the `$100 Rule` guard for the data-flow gap.

**Verification:** `cargo test` green; the parity test fails if the text path ever stops calling `build_row_results`.

## System-Wide Impact

- **Interaction graph:** `main::run` → `build_row_results(raw, catalog)` (new) → projected per-row `Vec<AuditResult>` →
  `format_text` + `compute_badge` + `exit_code` (text arm) and `format_json` → `build_scorecard` → same
  `build_row_results` (JSON arm). One pipeline, two output surfaces, one exit code — all derived from the same per-row
  set.
- **Parity restored:** text and JSON now agree on the **row set** (requirement-row ids, not probe ids), **counts** (text
  badge count == JSON `results.len()`), **badge score** (text hint == JSON `badge.score_pct`), and **exit code**
  (per-row truth in both modes).
- **JSON path unchanged:** `build_scorecard` produces byte-identical output; schema stays `0.6`.
- **`--audit-profile` path:** unchanged. Suppression emits `Skip` with the sentinel before fan-out; propagation does not
  touch those rows; the R4 masking semantics for `exit_code` are preserved (verify the existing R4 test).
- **Unchanged invariants:** `fan_out_per_row` / `propagate_antecedents` behavior, `score_pct` formula,
  `coverage_summary`, `audience` classification (all key on raw results, as before).

## Risk Analysis

| Risk                                                                                                                                            | Severity                            | Mitigation                                                                                                                                                                                                                                                          |
| ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **exit-code semantics change** — a tool that raw-Fails an audit whose row propagates to `n_a` exits 2 in text today; after the fix it exits 0/1. | Medium (observable behavior change) | This IS the fix: the per-row truth is "not applicable," so exit 2 was wrong. Documented in Key Technical Decisions §4; pinned by a new unit test and the parity regression test. The `--audit-profile` R4 masking path is independent and its test must still pass. |
| Renderer signature change ripples into existing `format_text_*` unit tests.                                                                     | Low                                 | Tests updated in Unit 4; they exercise the formatter, and the formatter still accepts a result set (now per-row + provenance).                                                                                                                                      |
| Double-computing the pipeline (text arm + exit_code) wastes work or, worse, computes two different sets.                                        | Low                                 | Compute `build_row_results` **once** per run, share the projection across `format_text`/`compute_badge`/`exit_code`.                                                                                                                                                |
| A requirement-row id missing from the registry yields `None` tier and could panic on a `(tier)` render.                                         | Low                                 | Defensive: omit the `(tier)` suffix when `registry::find` returns `None` (same tolerance `AuditResultView::from_row` already has).                                                                                                                                  |
| Quiet/raw/color modes regress under the new statuses.                                                                                           | Low                                 | Existing `OptOut`/`NotApplicable` arms already handle quiet-skip and raw tokens; Unit 2 test scenarios cover all three.                                                                                                                                             |

## Verification

- `anc audit --command bat` (text) and `anc audit --command bat --output json` show the **same status per row**:
  `opt_out` on `p2-must-output-flag`, `n_a` on `p2-must-schema-print`, no `[FAIL] … (p2-schema-print)`.
- Text row count == JSON `results.len()`; text badge score == JSON `badge.score_pct`.
- `anc audit --command bat`; `echo $?` exits 0 (per-row truth), and the exit code agrees between text and JSON modes.
- anc-on-anc: `anc audit .` text and JSON agree; dogfood JSON guards (`tests/dogfood.rs`) remain green and anc audits
  itself at the same pass profile (100% on wired requirements).
- `cargo fmt`, `cargo clippy -Dwarnings`, `cargo test`, `cargo-deny`, Windows compat — the full pre-push gate.
- `anc emit coverage-matrix --check` still passes (no registry change in this plan).

## Sources & References

- U2 pipeline: `src/scorecard/mod.rs` — `fan_out_per_row` (633), `propagate_antecedents` (672), `build_scorecard` (737),
  `compute_badge` (166), `score_pct` (208), `exit_code` (870), `AuditResultView::from_row` (376).
- Text path: `src/main.rs:284-293` (text arm), `src/main.rs:327` (`exit_code`), `format_text` / `format_text_raw`
  (`src/scorecard/mod.rs:507`, `606`).
- Propagation table / antecedent registry: `src/principles/registry.rs` (`Applicability::Conditional`, antecedent
  resolution test at 478).
- Existing text tests: `src/scorecard/mod.rs:1638-1706`; integration text assertions: `tests/integration.rs:130-155`.
