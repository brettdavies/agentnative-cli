---
title: "fix: PR #79 follow-up — close the three worst bypasses in p2-must-json-errors role-based validator"
type: fix
status: proposed
priority: P1
date: 2026-06-03
origin: "Adversarial review of merged PR #79 (commit 34b94175) surfaced 9 findings, including a self-acknowledged
  gaming case in the PR body itself. This plan ships the three highest-impact tactical patches while the larger
  redesign (semantic-roles vs literal-names vs structural-stability) lives in plan
  2026-06-03-001-refactor-audit-philosophy-structure-over-vocabulary-plan.md"
---

# fix: PR #79 follow-up — close the three worst bypasses in p2-must-json-errors role-based validator

## Summary

PR #79 reframed `p2-must-json-errors` from "envelope must contain literal keys `error`, `kind`, `message`" to "envelope
must carry three semantic roles: a discriminant, a type identifier, and a human-readable message." The reframe is
defensible in principle but landed without the corresponding `agentnative-spec` PR — the vendored spec text still says
"at least `error`, `kind`, and `message` fields" — and the predicates are loose enough that several envelopes pass the
validator that no agent could reliably dispatch on, including:

1. Numeric "discriminants" on info-coded values (`{"level":0,"kind":"info","message":"..."}` passes today).
2. Success-coded synonyms outside the 8-string closed set (`{"status":"OKAY",...}`, `{"outcome":"successful",...}`).
3. Success envelopes carrying nested debug data (`{"ok":true,"data":{"status":"error","code":"X","message":"..."}}`).

The PR body itself acknowledged a remaining gaming case (`{error:"ok", kind:"<prose>", message:"<token>"}`) and deferred
the fix as "a larger refactor." This plan closes the three worst bypasses above while the larger redesign (role-based vs
literal-name vs structural-stability vs move-to-P8) lives in plan #001.

The deeper concern — that PR #79's predicates were calibrated to match `anc`'s own `InstallEnvelope` shape (`{status,
reason, exit_code, message}`) — is parked. Plan #001 owns whether the audit should be reverted to literal-keys, kept
with full name-role coupling, or redesigned around contract-stability (run multiple failure modes, verify shape
consistency).

PR reference: <https://github.com/brettdavies/agentnative-cli/pull/79> (merged 2026-06-03).

## Findings inventory

The adversarial review surfaced 9 findings against this PR.

| #   | Severity | Title                                                                                           | Disposition                                          |
| --- | -------- | ----------------------------------------------------------------------------------------------- | ---------------------------------------------------- |
| 1   | P1       | Discriminant accepts ANY number on a discriminant-named field — `level:0` / `severity:1` passes | **Fixed here (R1)**                                  |
| 2   | P1       | SUCCESS-coded guard is closed set of 8 strings — `OKAY`, `successful`, `ok-result` bypass       | **Fixed here (R2)**                                  |
| 3   | P1       | Nested-object recursion permits success envelopes with debug data to pass                       | **Fixed here (R3)**                                  |
| 4   | P1       | Type-identifier predicate too permissive (single chars, dictionary words)                       | Parked → plan #001                                   |
| 5   | P1       | Spec text is literal (`error/kind/message`); code is reframed (roles) — unauthorized divergence | Parked → plan #001                                   |
| 6   | P2       | Anc-shape calibration — every predicate threshold matches anc's emitted InstallEnvelope         | Parked → plan #001                                   |
| 7   | P2       | Happy-path test bias — 11 of 12 tests; only 1 adversarial test                                  | **Fixed here (R4)**                                  |
| 8   | P2       | `headless_auth.rs` unrelated scope expansion in same PR (defensible on its own)                 | No-op — flagged for process notes; defensible change |
| 9   | P2       | Order-dependence — first field that matches a role wins                                         | Deferred                                             |

## Scope

### In scope

- Tighten `is_discriminant` for numeric values: a numeric discriminant requires a field name in a small subset of
  "error-coded" names (e.g., `error_code`, `exit_code`, `code`) AND a non-zero value.
- Expand the SUCCESS guard beyond an 8-string closed list. Match against a documented family of patterns
  (case-insensitive prefixes / known synonyms) or extend the array with the most common variants. Net effect: `OKAY`,
  `successful`, `ok-result`, `succeeded`, `passed`, `done` all trigger the guard.
- Reject the nested-recursion path when the outer envelope contains any field with a success-coded value at depth 0. An
  outer envelope that signals success (`{"ok":true,...}`, `{"status":"success",...}`) is never an error envelope — the
  validator should not descend into its `data` to harvest roles.
- Add 6 adversarial unit tests covering the three patched bypasses plus three regression-pins (existing passes must
  continue to pass).

### Out of scope

- Spec PR in `agentnative-spec` — plan #001 owns this.
- Name-role coupling refactor (the "follow-up" the PR body promised) — plan #001 owns this.
- Revert to literal-keys validator — plan #001 evaluates this option.
- Reverting `headless_auth.rs` changes from PR #79 — the change itself is defensible (adds `pub fn`, `pub(crate) fn`,
  `async fn` patterns to ast-grep). The process complaint is that it was bundled with a json_errors reframe in the same
  PR; the code change stays.
- Type-identifier predicate tightening (e.g., minimum length, required hyphen/underscore) — plan #001.
- Order-dependence in field-role assignment — plan #001.

## Requirements

- **R1**: `is_discriminant` for `Value::Number(n)` returns `true` only when the field name is in a new constant
  `ERROR_DISCRIMINANT_NAMES: &[&str] = &["error_code", "exit_code", "code"]` AND `n.as_i64() != Some(0)` (zero is
  success-coded for integer-style exit codes). Source: `src/audits/behavioral/json_errors.rs:148-173`.
- Concrete test: `{"level":0,"kind":"info","message":"completed."}` produces `Warn` (not `Pass`).

- **R2**: `SUCCESS_VALUE_STRINGS` expands to recognize a documented family of success synonyms. Either:
- (a) extend the array to include `okay`, `successful`, `noop`, `no-op`, `n/a`, `noop-success`, and any other common
    phrasings, OR
- (b) replace the equality check with a small set of prefix/substring rules (`val.starts_with("ok")` plus the explicit
    phrases) coupled with case-insensitive comparison.

  Either approach is acceptable; prefer (a) for simplicity unless the value set grows past 20. Source:
  `src/audits/behavioral/json_errors.rs:61-70, 163-168`.
- Concrete tests: `{"status":"OKAY",...}`, `{"outcome":"successful",...}`, `{"result":"success",...}` all produce
    `Warn`.

- **R3**: Add a function `outer_envelope_is_success_coded(obj: &Map<String, Value>) -> bool` that returns `true` if any
  top-level field has a value matching the (expanded) success guard. `classify_envelope` checks this at depth 0 before
  recursing into nested objects. If true, return `RoleSet::default()` (no roles claimed) and the audit emits `Warn`.
  Source: new check at `src/audits/behavioral/json_errors.rs::classify_object` entry.
- Concrete test: `{"ok":true,"data":{"status":"error","code":"X","message":"debug."}}` produces `Warn`.
- Negative pin: `{"ok":false,"status":"error","code":"X","message":"..."}` continues to `Pass` (the `ok:false` value is
    not success-coded; `false` does not match any string in the success-guard set).

- **R4**: Six new unit tests:
- `fail_numeric_discriminant_on_info_coded_value` — `{"level":0,"kind":"info","message":"..."}` → `Warn`.
- `fail_success_synonym_okay` — `{"status":"OKAY","reason":"all-good","message":"we are fine."}` → `Warn`.
- `fail_success_synonym_successful` — `{"outcome":"successful","kind":"foo","message":"all good."}` → `Warn`.
- `fail_outer_success_with_nested_error_data` — `{"ok":true,"data":{"status":"error","code":"X","message":"..."}}` →
    `Warn`.
- `pass_ok_false_canonical_error_envelope_regression` — `{"ok":false,"kind":"auth-required","message":"login please."}`
    continues to `Pass`. Pin that the R3 guard doesn't over-fire on `ok:false`.
- `pass_anc_install_envelope_regression` — anc's own `{"status":"error","reason":"destination-not-empty","exit_code":1,
    "message":"..."}` continues to `Pass`. Pin that the R1/R2/R3 changes don't break the audit's primary dogfood.

- **R5**: The existing 12 tests continue to pass (no regressions in either direction).

## Implementation Units

### U1. Add `ERROR_DISCRIMINANT_NAMES` constant and tighten numeric-discriminant check

Add the constant near the existing `DISCRIMINANT_FIELD_NAMES`. Modify `is_discriminant` for the `Value::Number` branch
to require `field_name ∈ ERROR_DISCRIMINANT_NAMES && n.as_i64() != Some(0)`. Numeric values on other discriminant-named
fields no longer satisfy the discriminant role.

### U2. Expand SUCCESS guard

Extend `SUCCESS_VALUE_STRINGS` with the synonyms named in R2 option (a). Comparison stays case-insensitive (already
true). If the array grows past 20 entries during implementation, switch to option (b) (prefix/phrase rules).

### U3. Add `outer_envelope_is_success_coded` and gate nested recursion

Add the helper near `is_discriminant`. Modify `classify_object` (or `classify_envelope`, whichever is the depth-0 entry
point) to check this before any nested-object descent. When true, return `RoleSet::default()`. Document the intent
inline: "outer envelope signals success; never descend into nested data to harvest error roles."

### U4. Add the 6 new tests

Place tests adjacent to the existing 12 in `src/audits/behavioral/json_errors.rs`. Each test constructs the JSON
explicitly (not via macros) so the bypass shape is readable. Name tests descriptively (R4 names above).

### U5. Verify with dogfood and downstream consumers

- `anc audit .` on the anc source: `p2-must-json-errors` Passes (regression pin via test, also exercised by dogfood).
- `anc audit /home/brett/dev/xurl-rs`: `p2-must-json-errors` Passes (xurl-rs's envelope is the canonical shape; the
  patches don't tighten in ways that affect it).
- `anc audit /home/brett/dev/bird` (or wherever the bird CLI lives): same check.

These are verification steps, not in-scope code changes.

## Open questions

- **`code` vs `exit_code` in ERROR_DISCRIMINANT_NAMES.** A field named `code` is ambiguous — sometimes it's an
  error-code string identifier, sometimes a status-code integer. The R1 logic restricts numeric `code` to non-zero,
  which handles the case well. Leaving `code` in the set keeps backward compatibility with envelopes that use it as the
  numeric exit code; revisit in plan #001 if a real CLI emits `code:0` to indicate error.

- **Should the SUCCESS-guard expansion include `success: true` (boolean)?** Currently the guard scans for string-shaped
  success values. A field `{"success":true,"message":"..."}` would NOT trigger the outer-success guard today and could
  pass via numeric discriminant elsewhere. Plan #001 territory — depends on the eventual approach to discriminant
  semantics. Default for this plan: keep the guard string-shaped only; `success:true` is parked.

- **xurl-rs envelope re-verification.** The PR-#79 motivation was xurl-rs's adoption of the canonical shape. Verify
  after these patches that no xurl-rs envelope shape regresses. The user's `feedback_pr_body_discipline.md` memory noted
  PRs touching xurl-rs need lockstep follow-ups; coordinate via xurl-rs maintainer.

- **Order-dependence (finding #9).** When a string-valued field is discriminant-named (e.g., `reason:"not-found"`), it
  satisfies both the discriminant role (by name) and the type_id role (by shape). The current first-field-wins logic
  assigns it to discriminant. Adding a second discriminant-named field changes the assignment non-obviously. Deferred to
  plan #001 (the larger refactor); this plan does not change role-assignment ordering.

- **Documentation pointer.** Should the Warn evidence string point to a docs page describing the role contract? Plan
  #001 will likely revise that contract; for this plan, keep the Warn evidence string as-is (verbose but not pointing to
  a doc that will change).

## Acceptance

- All 6 new tests pass.
- The existing 12 tests pass unchanged.
- `cargo test` total at the post-PR-#79 baseline (833+ passing, 2 ignored).
- `cargo clippy --all-targets -- -Dwarnings` clean.
- `cargo fmt --check` clean.
- Dogfood: `anc audit .` produces `pass` on `p2-must-json-errors`.
- xurl-rs and bird (or other consumer CLIs) envelope shapes continue to `Pass`.
- The PR-#79-acknowledged gaming case (`{error:"ok", kind:"<prose>", message:"<token>"}`) is now caught — `error:"ok"`
  is in the (expanded) success-guard set; R3's outer-envelope check returns `RoleSet::default()` and the audit Warns.
  Add this as a regression test (`fail_known_gaming_case_from_pr79_body`).

## Notes for the implementer

- Conventional commit shape: `fix(p2-json-errors): tighten numeric-discriminant, success guard, nested recursion`.
- Diff is small (~30-50 lines of audit logic plus tests). Most of the work is test cases.
- Do not modify `src/principles/spec/principles/p2-structured-parseable-output.md` in this PR. The spec divergence
  (literal `error/kind/message` vs role-based) stays unresolved — plan #001 owns the resolution.
- Do not touch `src/audits/source/rust/headless_auth.rs` (unrelated to this fix).
- Do not bump scorecard `schema_version` — the changes are internal validator logic, not schema-visible.
- Open the PR against `dev`. Before merging: run `anc emit coverage-matrix --check`, `cargo deny check advisories`, and
  a manual dogfood against at least one external CLI (xurl-rs is the canonical target).
