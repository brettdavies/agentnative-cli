---
title: "fix: PR #77 follow-up — cfg(not(test)) polarity bug in code-unwrap; missing negative-case tests"
type: fix
status: proposed
priority: P0
date: 2026-06-03
origin: "Adversarial review of merged PR #77 (commit 817d6fa7) surfaced a P0 polarity bug: `cfg(not(test))` is treated
  identically to `cfg(test)`, silently exempting production-only code from the code-unwrap audit. Confirmed by reading
  src/audits/source/rust/unwrap.rs:247-256 and by an injected adversarial test that panicked"
---

# fix: PR #77 follow-up — cfg(not(test)) polarity bug in code-unwrap; missing negative-case tests

## Summary

PR #77 replaced the flat ast-grep pattern for `code-unwrap` with a tree-sitter walk that exempts `.unwrap()` calls
inside `#[cfg(test)]`-gated items. The intent is correct (test code is allowed to panic) and the structural walk is the
right shape. The cfg-args parser, however, treats `not(...)` identically to `any(...)` and `all(...)`: it descends into
the inner argument list and returns `true` whenever the bare identifier `test` appears at any depth, with no polarity
tracking. The effect is that **`#[cfg(not(test))]` on production-only code now silently exempts that code from the
audit** — exactly the failure mode the operator's review was investigating.

This is a P0 correctness regression. Any Rust crate using `#[cfg(not(test))]` to gate production-only code paths (real
network calls, non-mock dependencies, real-time effects) will have `.unwrap()` calls inside those items silently pass
`code-unwrap`. The bug is small (one parameter through one recursive call site); the test coverage gap that allowed it
through review is the more important pattern to address.

PR reference: <https://github.com/brettdavies/agentnative-cli/pull/77> (merged 2026-06-02).

Locations:

- The buggy parser: `src/audits/source/rust/unwrap.rs:247-256` (the `any`/`all`/`not` branch).
- The walk consumer: `src/audits/source/rust/unwrap.rs:117-140` (the one-shot flag propagation).
- The missing negative tests: `src/audits/source/rust/unwrap.rs` test module (currently 13 tests, 0 covering polarity).

## Findings inventory

The adversarial review surfaced 3 findings against this PR.

| #   | Severity | Title                                                                   | Disposition            |
| --- | -------- | ----------------------------------------------------------------------- | ---------------------- |
| 1   | **P0**   | `cfg(not(test))` silently exempts production-only code                  | **Fixed here (R1-R4)** |
| 2   | P2       | `cfg_attr(test, …)` correctly NOT handled, but no test pins this        | **Fixed here (R5)**    |
| 3   | P2       | Inner-attribute (`#![cfg(test)]`) comment overclaims sticky propagation | **Fixed here (R6)**    |

## Scope

### In scope

- Thread a `negated: bool` parameter through `cfg_args_contain_test` (or equivalent polarity-tracking refactor).
- Correct the `not(...)` recursive call to flip polarity.
- Make the "found bare `test`" return value condition on the current polarity (true at even-parity, false at
  odd-parity).
- Add negative-case tests covering: `cfg(not(test))`, `cfg(any(not(test), unix))`, `cfg(all(not(test), feature = "x"))`,
  `cfg(not(any(test)))` (double-nested), `cfg_attr(test, allow(unused))`, and the bare `mod tests` (without cfg gate)
  case to pin existing correct behavior.
- Correct the comment block at lines 122-130 to describe one-shot reset semantics accurately.

### Out of scope

- Refactor of the broader cfg-attribute walking system.
- Performance characterization of the tree-sitter walk (PR #77 didn't add benchmarks; this plan doesn't add them
  either).
- Extending `code-unwrap` to other call patterns (e.g., `.expect()`, `unwrap_or_else(|_| panic!())`) — separate redesign
  per plan #001's source-audit direction.
- Changes to `--include-tests` plumbing or its semantics.

## Requirements

- **R1**: `#[cfg(not(test))] fn production_only() { foo().unwrap(); }` produces a `Fail` from the `code-unwrap` audit by
  default (i.e., when `--include-tests` is off). The unwrap is reported as evidence with its line number.

- **R2**: `#[cfg(any(not(test), unix))] fn x() { foo().unwrap(); }` produces a `Fail`. (The unwrap is gated only when
  NOT testing AND when on a non-unix platform; in test mode on unix the item compiles, so it is production code under
  the test compilation.) The conservative reading: not-test-pure means production; flag it.

- **R3**: `#[cfg(all(not(test), feature = "x"))] fn x() { foo().unwrap(); }` produces a `Fail`. The item is
  production-only under feature `x`; the unwrap is production code.

- **R4**: `#[cfg(any(test, not(feature = "real")))] fn x() { foo().unwrap(); }` continues to exempt the unwrap. The
  `test` predicate at even parity correctly triggers the test-gate. (Verifies the polarity fix did not break the
  positive case.)

- **R5**: `#[cfg_attr(test, allow(unused))] fn x() { foo().unwrap(); }` produces a `Fail`. `cfg_attr` applies the inner
  attributes conditionally; the item itself compiles unconditionally. The walk correctly does NOT treat `cfg_attr` as a
  gate (current behavior, now pinned by test).

- **R6**: The comment block at `src/audits/source/rust/unwrap.rs:122-130` accurately describes the one-shot reset
  semantics: the "next sibling is cfg-test-gated" flag consumes on the very next sibling and is then reset, regardless
  of whether that sibling is an item, use, or unrelated AST node. The comment is corrected; a test pins the semantics:
  `#![cfg(test)] use foo; fn production() { foo().unwrap(); }` produces a `Fail` (the `use` consumes the one-shot flag;
  the `fn` is not gated).

- **R7**: An additional pinning test: `mod tests { fn helper() { foo().unwrap(); } }` (no `#[cfg(test)]` gate on the
  module) produces a `Fail`. The module-name-only case must not be treated as test-gated; the PR-#77 walk gets this
  right today; the test pins it.

## Implementation Units

### U1. Add polarity parameter to `cfg_args_contain_test`

Change the signature from `fn cfg_args_contain_test(args: &str) -> bool` to `fn cfg_args_contain_test(args: &str,
negated: bool) -> bool`. The bare-test branch (currently at lines 240-246) returns `!negated` rather than `true`.

### U2. Flip polarity on `not(...)` recursion

At the recursive-call site (currently at line 251), branch on the identifier:

```rust
let recurse_negated = match ident {
    "not" => !negated,
    _ => negated, // "any", "all" preserve polarity
};
if cfg_args_contain_test(inner, recurse_negated) {
    return true;
}
```

The `any` and `all` cases preserve polarity. Only `not` flips it. Nested `not(not(test))` correctly returns true at the
outer level (even parity restored).

### U3. Update call sites

The function is called from `attribute_text_is_cfg_test` (line 198, approximate). Update the call to pass `negated:
false` initially. There are no other callers in production code.

### U4. Add 8 negative-case tests

Tests for R1, R2, R3, R4, R5, R6, R7 plus a polarity-pinning test (`cfg(not(not(test)))` returns true; `cfg(not(any(
test)))` returns false). Place tests in the existing test module of `src/audits/source/rust/unwrap.rs`. Name them
descriptively: `cfg_not_test_does_not_exempt_production_unwrap`, `cfg_any_not_test_unix_does_not_exempt`,
`cfg_all_not_test_feature_does_not_exempt`, `cfg_any_test_or_not_feature_still_exempts`,
`cfg_attr_test_does_not_exempt`, `inner_attribute_one_shot_consumes_on_use_not_fn`, `mod_tests_without_gate_does_not_
exempt`, `cfg_not_not_test_does_exempt`.

### U5. Correct the inner-attribute comment

The comment block at `src/audits/source/rust/unwrap.rs:122-130` currently says (paraphrasing) that an inner attribute
gates all subsequent siblings. The actual behavior is one-shot: the flag consumes on the very next sibling. Correct the
comment to describe what the code does. If the operator wants sticky propagation later, that is a separate change with
its own test coverage; the comment must match present behavior.

### U6. Dogfood and regression check

Run `anc audit .` against the post-fix `anc` source. Search the codebase for any existing `#[cfg(not(test))]` items:

```bash
rg -n --type rust '#\[cfg\(not\(test\)\)\]' src/
```

If any production-only items contain `.unwrap()`, evaluate whether they should be addressed (legitimate fix needed) or
suppressed via `--include-tests` profile or `--audit-profile diagnostic-only`. This is verification, not in-scope
remediation.

## Open questions

- **Should the audit also flag `.expect()`?** Today, `code-unwrap` is narrowly scoped to `.unwrap()`. `.expect("msg")`
  is morally equivalent and panics the same way. Extending the audit is plan #001 territory (source-audit direction);
  this plan does not do it.

- **`cfg(target_test = ...)` or other future cfg keys.** The parser currently treats any `ident == "test"` as the
  bare-test predicate when followed by `,`, `)`, or end-of-args. If a future Rust release introduces `target_test` or
  similar, the audit would not gate. Default: strict bare-test only; revisit when/if such a cfg key ships.

- **xurl-rs verification.** The PR-#77 motivating use case was xurl-rs's inline test modules. After the polarity fix,
  verify that xurl-rs's `code-unwrap` still passes (i.e., no production unwraps are gated by `cfg(not(test))`). If a
  regression appears, it is xurl-rs's bug to address, not this plan's.

- **`Confidence` for the audit.** `code-unwrap` doesn't currently emit a `Confidence` marker. Whether failures flagged
  via this stricter parser deserve `Confidence::High` vs unmarked is a style question; default: unmarked, as today.

## Acceptance

- All 8 new tests pass.
- All 13 pre-existing tests continue to pass.
- `cargo test` total at the post-PR-#77 baseline (810+ passing, 2 ignored).
- `cargo clippy --all-targets -- -Dwarnings` clean.
- The polarity regression test (`cfg_not_test_does_not_exempt_production_unwrap`) is explicitly named in the suite so
  any future regression surfaces with a clear cause.
- `anc audit .` dogfood: any existing `#[cfg(not(test))]` items in `src/` are inspected; any flagged production unwraps
  either fixed or explicitly catalogued as expected.
- `anc audit /home/brett/dev/xurl-rs` `code-unwrap` continues to Pass.

## Notes for the implementer

- Conventional commit shape: `fix(code-unwrap): cfg(not(test)) polarity; add missing negative-case tests`.
- The fix is small (~5 lines plus the parameter signature change). The bulk of the diff is test cases.
- The `balanced_parens` helper at lines 269-305 is unchanged.
- Do not touch `src/audits/source/rust/unwrap.rs` outside the parser function, the comment, and the test module. Keep
  the diff surgical so the fix is easy to review and bisect.
- Open the PR against `dev`. Priority is P0 — land this ahead of plans #002, #003, #005, since it's a real correctness
  regression rather than a design-level patch.
- Before merging: `cargo deny check advisories` (must stay ok) and `anc emit coverage-matrix --check` (must stay green).
