---
title: "feat: Python starter checks, fixture test coverage, and real-world validation"
type: feat
status: active
date: 2026-04-02
origin: ~/.gstack/projects/brettdavies-agentnative/brett-main-design-20260327-214808.md
---

# feat: Python starter checks, fixture test coverage, and real-world validation

## Overview

Three confidence gaps before GA: (1) Python source checks are empty — shipping 2-3 starter checks proves the
tree-sitter-python pipeline works; (2) three integration tests are `#[ignore]` and need to run; (3) the design doc's
success criteria require validation against 3+ real-world CLIs (bird, xurl-rs, and one external like ripgrep).

## Problem Frame

The design doc commits to "5-6 key Python checks" in v0.1. Zero shipped. Rather than the full set, we'll ship 2-3
starter checks that prove the ast-grep + tree-sitter-python pipeline works end-to-end, deferring the rest to v0.2. The
three `#[ignore]` integration tests exist but have never been verified in CI. And the tool has never been run against
real-world CLIs outside of self-dogfooding.

## Requirements Trace

- R1. At least 2-3 Python source checks implemented and tested
- R2. `tree-sitter-python` pipeline proven end-to-end (parse → pattern match → evidence)
- R3. All 3 ignored integration tests un-ignored and passing
- R4. Validation against bird, xurl-rs, and at least one external CLI (e.g., ripgrep)
- R5. Design doc updated to reflect revised Python check scope for v0.1

## Scope Boundaries

- Python checks target Click/Typer CLI patterns (the most common Python CLI frameworks)
- Full Python check set (5-6 checks from design doc) deferred to v0.2
- Real-world validation is manual (run the tool, review output, document results) — no automated CI job
- Fixture tests may need minor fixture updates to pass, but fixture architecture stays as-is

## Context & Research

### Relevant Code and Patterns

- `src/checks/source/python/mod.rs` — empty stub, returns `vec![]`
- `src/checks/source/rust/unwrap.rs` — canonical pattern for implementing a source check
- `src/checks/source/rust/no_color.rs` — pattern for conditional/trigger-based source checks
- `src/source.rs` — `has_pattern()` and `find_pattern_matches()` for ast-grep matching
- `src/project.rs` — `Language::Python` variant already exists in detection logic
- `Cargo.toml` — `tree-sitter-python` feature already enabled on `ast-grep-language`
- `tests/integration.rs` — 3 ignored tests: `test_broken_fixture`, `test_perfect_fixture`, `test_source_only_fixture`
- `tests/fixtures/` — 4 fixture directories: `binary-only/`, `broken-rust/`, `perfect-rust/`, `source-only/`

### Institutional Learnings

- No existing solutions for Python ast-grep checks — this is greenfield
- `src/source.rs` `find_pattern_matches()` already handles multi-language dispatch via `ast-grep-language`
- The `Position` type uses `.line()` / `.column(&node)` methods (CLAUDE.md convention)

### External References

- ast-grep pattern catalog for Python: `$F.unwrap()` style won't work — Python uses `try/except`, not `.unwrap()`
- Click/Typer common patterns: `@click.command()`, `@click.option()`, `typer.Option()`

## Key Technical Decisions

- **Which 2-3 Python checks to implement**: Focus on checks that (a) prove the pipeline, (b) are meaningful for
  Click/Typer CLIs, and (c) have clear ast-grep patterns:

1. **`code-bare-except`** — detects bare `except:` without exception type (Python anti-pattern, analogous to `.unwrap()`
     for Rust). Simple ast-grep pattern, high signal.
2. **`p4-sys-exit`** — detects `sys.exit()` outside `if __name__ == "__main__"` (analogous to Rust's `process::exit()`
     check). Validates conditional pattern matching.
3. **`p6-no-color-source`** — detects `os.environ.get("NO_COLOR")` or `os.getenv("NO_COLOR")` handling. Validates
     cross-language check parity (same concept as the Rust check).

- **Fixture tests — what's blocking them**: The `#[ignore]` comments say "requires parseable Rust source" or "requires
  cargo build." The fixtures need to be valid enough for ast-grep to parse. Verify each fixture passes before
  un-ignoring.
- **Real-world validation approach**: Run `agentnative check <path>` against bird, xurl-rs (local repos), and `ripgrep`
  (installed binary). Document results in a validation report (not committed — ephemeral).

## Open Questions

### Resolved During Planning

- **Can ast-grep parse Python with tree-sitter-python?**: Yes — the `tree-sitter-python` feature is already enabled in
  `Cargo.toml` and `ast-grep-language` supports it. The `source.rs` helpers dispatch to the correct language grammar.
- **Do we need a Python fixture project?**: Not for v0.1 starter checks. The Rust fixtures test the check
  infrastructure. Python checks get unit tests with inline source strings.

### Deferred to Implementation

- **Exact ast-grep pattern syntax for Python**: The Python tree-sitter grammar may represent `try/except` differently
  than expected. Implementation will need to test patterns against real Python ASTs.
- **Whether fixture tests need fixture updates**: The ignore comments suggest possible issues — investigate at
  implementation time.

## Implementation Units

- [ ] **Unit 1: Implement `code-bare-except` Python source check**

**Goal:** Detect bare `except:` clauses (without exception type) in Python source. Proves the tree-sitter-python
pipeline works end-to-end.

**Requirements:** R1, R2

**Dependencies:** None

**Files:**

- Create: `src/checks/source/python/bare_except.rs`
- Modify: `src/checks/source/python/mod.rs` — register the check
- Test: inline `#[cfg(test)]` module in `bare_except.rs`

**Approach:**

- Follow the exact pattern from `src/checks/source/rust/unwrap.rs`:
- Unit struct `BareExceptCheck`
- Implement `Check` trait
- `id()` returns `"code-bare-except"`
- `group()` returns `CheckGroup::CodeQuality`
- `layer()` returns `CheckLayer::Source`
- `applicable()` checks `project.language == Some(Language::Python)`
- `run()` calls `find_pattern_matches()` with the ast-grep pattern for bare except
- The ast-grep pattern for bare `except:` in Python: `except: $$$` (match except clause with no exception type)
- Iterate over `project.parsed_files()` using the Python language

**Patterns to follow:**

- `src/checks/source/rust/unwrap.rs` — identical structure, different language and pattern

**Test scenarios:**

- Happy path: Python source with `except Exception:` passes (no bare except)
- Happy path: Python source with `except:` (bare) fails with evidence showing file:line:column
- Edge case: Python source with `except: pass` fails (bare except, even with `pass`)
- Edge case: Python source with `except ValueError:` passes (specific exception type)
- Edge case: No Python files in project → Skip

**Verification:**

- Check returns Fail with evidence for bare `except:` clauses
- Check returns Pass for properly typed exception handlers
- Evidence format matches Rust checks: `file:line:column — violation text`

---

- [ ] **Unit 2: Implement `p4-sys-exit` Python source check**

**Goal:** Detect `sys.exit()` calls outside `if __name__ == "__main__"` blocks.

**Requirements:** R1, R2

**Dependencies:** Unit 1 (establishes the Python check pattern)

**Files:**

- Create: `src/checks/source/python/sys_exit.rs`
- Modify: `src/checks/source/python/mod.rs` — register the check
- Test: inline `#[cfg(test)]` module in `sys_exit.rs`

**Approach:**

- Pattern: detect `sys.exit($$$)` calls
- Conditional logic: if `sys.exit()` is found, check whether it's inside an `if __name__ == "__main__"` block
- If found outside the guard, return Fail
- This validates the conditional check pattern for Python (analogous to `process_exit.rs` for Rust)
- May need to use `find_pattern_matches()` and inspect the match context rather than a single ast-grep pattern

**Patterns to follow:**

- `src/checks/source/rust/process_exit.rs` — same concept, different language

**Test scenarios:**

- Happy path: `sys.exit(1)` inside `if __name__ == "__main__":` block passes
- Happy path: `sys.exit(0)` outside guard fails with evidence
- Edge case: `exit()` (builtin, not `sys.exit`) — skip (different function)
- Edge case: no `sys.exit()` anywhere → Pass (no violations)
- Edge case: `sys.exit()` in a function called from `__main__` guard — this is a false positive risk; accept Warn

**Verification:**

- Correctly distinguishes guarded vs unguarded `sys.exit()` calls
- Evidence includes file path and line number

---

- [ ] **Unit 3: Implement `p6-no-color-source` Python source check**

**Goal:** Detect `NO_COLOR` environment variable handling in Python source.

**Requirements:** R1, R2

**Dependencies:** Unit 1 (establishes the Python check pattern)

**Files:**

- Create: `src/checks/source/python/no_color.rs`
- Modify: `src/checks/source/python/mod.rs` — register the check
- Test: inline `#[cfg(test)]` module in `no_color.rs`

**Approach:**

- Pattern: search for `os.environ.get("NO_COLOR")` or `os.getenv("NO_COLOR")` or `os.environ["NO_COLOR"]`
- If found, Pass (project respects NO_COLOR)
- If not found, Warn (not an error — the behavioral check covers NO_COLOR too)
- This validates cross-language check parity (same check exists for Rust)

**Patterns to follow:**

- `src/checks/source/rust/no_color.rs` — same concept, different language and pattern

**Test scenarios:**

- Happy path: Python source with `os.environ.get("NO_COLOR")` passes
- Happy path: Python source with `os.getenv("NO_COLOR")` passes
- Happy path: Python source without any NO_COLOR reference warns
- Edge case: `NO_COLOR` mentioned in a comment — should not count as handling it (ast-grep ignores comments)
- Edge case: `NO_COLOR` in a string literal — should not count (ast-grep is AST-aware)

**Verification:**

- Correctly detects NO_COLOR handling via common Python env var access patterns
- Returns Warn (not Fail) when missing — behavioral check is the primary gate

---

- [ ] **Unit 4: Un-ignore and fix integration tests for fixtures**

**Goal:** Make the 3 `#[ignore]` integration tests pass and remove the `#[ignore]` attribute.

**Requirements:** R3

**Dependencies:** None (can be done in parallel with Python checks)

**Files:**

- Modify: `tests/integration.rs` — remove `#[ignore]` from 3 tests
- Possibly modify: `tests/fixtures/broken-rust/src/main.rs` — if fixture source isn't parseable
- Possibly modify: `tests/fixtures/perfect-rust/` — if fixture needs updates
- Possibly modify: `tests/fixtures/source-only/` — if fixture needs updates

**Approach:**

- Run each ignored test individually with `cargo test -- --ignored <test_name> --nocapture` to see what fails
- `test_source_only_fixture`: needs parseable Rust source in `tests/fixtures/source-only/`. Verify the fixture's
  `src/main.rs` is valid enough for ast-grep to parse.
- `test_broken_fixture`: needs parseable Rust source in `tests/fixtures/broken-rust/`. The fixture intentionally has
  violations — verify ast-grep can parse it (parse errors ≠ check failures).
- `test_perfect_fixture`: the comment says "requires cargo build" — if behavioral checks need a binary, either build the
  fixture in CI or scope the test to `--source` only. Prefer `--source` to avoid build dependency.
- After fixing, remove `#[ignore]` and verify all 15 tests pass

**Patterns to follow:**

- Existing non-ignored integration tests in `tests/integration.rs`

**Test scenarios:**

- Happy path: `test_source_only_fixture` runs source + project checks, no behavioral checks
- Happy path: `test_broken_fixture` detects violations (fail count > 0)
- Happy path: `test_perfect_fixture` has 0 failures and 0 errors (or scope to source-only)
- Edge case: `test_perfect_fixture` with `--source` flag avoids need for compiled binary

**Verification:**

- `cargo test --test integration` reports 15 passed, 0 ignored, 0 failed
- No test requires a pre-built binary in the fixture directory (fixtures are source-only or use `--source`)

---

- [ ] **Unit 5: Real-world validation against bird, xurl-rs, and ripgrep**

**Goal:** Run agentnative against 3 real-world CLIs and document results. Satisfies design doc success criteria.

**Requirements:** R4

**Dependencies:** Units 1-4 (run validation after all checks and tests are in place)

**Files:**

- None committed — validation results are ephemeral

**Approach:**

- Build agentnative: `cargo build --release`
- Run against local repos:
- `agentnative check ~/dev/bird` — Rust CLI, should produce meaningful results
- `agentnative check ~/dev/xurl-rs` — Rust CLI, should produce meaningful results
- Run against installed binary:
- `agentnative check --command rg` — behavioral checks only against ripgrep (or `agentnative check $(which rg)` if
    `--command` is not yet implemented)
- Review output for:
- False positives (checks that flag correct code)
- False negatives (violations not caught)
- Crashes or panics
- Unclear evidence messages
- Fix any issues discovered

**Test expectation:** None — manual validation.

**Verification:**

- agentnative runs successfully against all 3 targets without crashes
- Output is meaningful (not all Skip or Error)
- No false positives that would undermine user trust
- Results documented in the PR description or session notes

---

- [ ] **Unit 6: Update design doc to reflect revised Python scope**

**Goal:** Formally document that v0.1 ships 2-3 starter Python checks, with full coverage in v0.2.

**Requirements:** R5

**Dependencies:** Units 1-3 (know exactly which checks shipped)

**Files:**

- Modify: `~/.gstack/projects/brettdavies-agentnative/brett-main-design-20260327-214808.md`

**Approach:**

- Update the "What's Deferred" table: change "Full Python source checks | 5-6 key checks ship in v0.1 | v0.2" to reflect
  that 2-3 starter checks ship in v0.1 and the remaining checks move to v0.2
- Update the constraints section if it references "5-6 key Python checks"
- Keep the table row for "Full Python source checks" as a v0.2 item

**Test expectation:** None — documentation update.

**Verification:**

- Design doc accurately reflects what shipped

## System-Wide Impact

- **Interaction graph:** Python checks integrate through the same `Check` trait and
  `all_source_checks(Language::Python)` dispatch. No new infrastructure needed.
- **Error propagation:** ast-grep parse failures for Python source become `CheckStatus::Error` (same as Rust). No new
  error types.
- **Integration coverage:** Un-ignoring fixture tests adds coverage for the source-only and broken-project code paths
  that are currently untested in CI.
- **Unchanged invariants:** All existing Rust checks, behavioral checks, and project checks unchanged. The check count
  in scorecard output increases by 2-3 (from 30 to 32-33).

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Python ast-grep patterns don't match expected tree-sitter nodes | Write unit tests with inline Python source; iterate on patterns |
| `test_perfect_fixture` requires a compiled binary | Scope the test to `--source` mode to avoid build dependency |
| Real-world validation reveals major false positive issues | Fix the check logic; defer checks that can't be fixed to v0.2 |
| `sys.exit()` detection has high false positive rate | Start with Warn instead of Fail; tune threshold in v0.2 |

## Sources & References

- **Design doc:** `~/.gstack/projects/brettdavies-agentnative/brett-main-design-20260327-214808.md`
- **Rust check pattern:** `src/checks/source/rust/unwrap.rs`
- **ast-grep language support:** `ast-grep-language` crate with `tree-sitter-python` feature
- Related code: `src/source.rs`, `src/checks/source/python/mod.rs`
