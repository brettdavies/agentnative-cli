---
title: "feat: Go, Ruby, and TypeScript source checks"
type: feat
status: active
date: 2026-04-17
---

# feat: Go, Ruby, and TypeScript source checks

## Overview

Extend source checks beyond Rust and Python to cover Go, Ruby, and TypeScript. Start with a scoped research spike to
characterize ast-grep support, meta-variable conventions, framework idioms, and code-pattern viability per language,
then ship 3–4 starter checks per language behind the same `Check` trait architecture already proven by the 16 Rust
checks and 3 Python checks. This is the next increment on the design doc's commitment that source checks "go deeper for
supported languages" without expanding the project's dependency surface.

## Problem Frame

The tool currently compiles `tree-sitter-rust` and `tree-sitter-python` into `anc` and ships language-specific source
checks for both. Behavioral and project checks already run language-agnostically, so any CLI benefits from the tool on
day one. But the design doc's positioning — "deeply useful to Rust/Python/Go/Node developers" — is still half-shipped:
Go and Node projects are *detected* (`detect_language` reads `go.mod` and `package.json`) but get zero source checks,
and Ruby is not represented at all.

The work is not symmetric. Go's pattern surface is closest to Rust's (explicit errors, `panic`, stdlib idioms).
TypeScript is the most idiom-diverse language we support — `.ts` vs `.tsx`, multiple runtime frameworks (commander,
oclif, yargs, @clack/prompts), and a sibling `Language::Node` variant that already exists. Ruby is entirely new to the
project and uses ast-grep's alternate meta-variable character (`µ` via `impl_lang_expando!`), meaning the cross-language
helpers in `src/source.rs` cannot just be extended by analogy — we have to confirm and design for a non-`$` meta-var
surface.

Because the languages differ enough that the wrong assumption would cost an entire rewrite of a check module, the user
requested a research spike first. The spike produces a written report that informs every subsequent unit.

## Requirements Trace

- R1. A spike document that characterizes ast-grep support, meta-variable conventions, viable pattern shapes, starter
  check candidates, and binary-size/compile-time cost per language (Go, Ruby, TypeScript) before any check code is
  written.
- R2. Go source checks: 3–4 starter checks implemented and covered by unit tests, registered via
  `all_source_checks(Language::Go)`.
- R3. TypeScript source checks: 3–4 starter checks implemented and covered by unit tests, registered under whichever
  Language variant the spike recommends (new `Language::TypeScript` or reuse of `Language::Node`).
- R4. Ruby source checks: 3–4 starter checks implemented and covered by unit tests, including a new `Language::Ruby`
  variant, `Gemfile`/`*.gemspec` manifest detection, `.rb` walk extension, and source-helper dispatch that handles the
  non-`$` meta-variable character.
- R5. Integration tests: one new fixture per language exercising at least one passing and one failing check path.
- R6. Documentation: README, CLAUDE.md conventions, and the design doc reference table updated to reflect the new
  language coverage; scorecard/output remains stable for existing Rust/Python consumers.
- R7. All existing Rust and Python checks continue to pass dogfooding (`anc check .`) with no regressions in scorecard
  output, binary size beyond the new grammar cost, or CI matrix.

## Scope Boundaries

- Not implementing the full 7-principle check set per language at launch — 3–4 starter checks each, with the remainder
  deferred to a follow-up iteration once the pipeline is proven (same approach that worked for Python in the v0.1 plan).
- Not adding `--fix`/rewrite support for the new checks — the ast-grep architecture supports it, but it is still
  deferred to v0.2 for every language.
- Not extending behavioral checks — they are already language-agnostic and need nothing here.
- Not adding JavaScript-specific source checks. If the spike recommends keeping `Language::Node` as a JS-only variant
  distinct from a new `Language::TypeScript`, JS source checks are deferred.
- Not adding Tsx-specific AST walking quirks beyond what `.tsx` fixtures require — frontend-framework checks (React
  hooks, etc.) are out of scope.
- Not auditing external CLI projects (bird, xurl-rs, ripgrep) as part of this plan — that is a v0.x validation exercise
  tracked separately.

### Deferred to Separate Tasks

- Full principle coverage (P1–P7) per language: each language ships 3–4 starter checks here; the remainder is tracked in
  a follow-up plan once starter coverage lands.
- JavaScript-specific source checks (separate from TypeScript): deferred until the Node/TypeScript variant decision is
  implemented.
- `--fix`/rewrite support: deferred to a dedicated v0.2 plan.
- Real-world validation across additional Go/Ruby/TS CLIs: deferred to a validation plan analogous to the v0.1 Python
  validation work.

## Context & Research

### Relevant Code and Patterns

- `src/project.rs` — `Language` enum (`Rust | Python | Go | Node`), `detect_language()` (manifest → language),
  `walk_source_files()` (single-extension walk), `parsed_files()` cache keyed by language-to-extension map.
- `src/source.rs` — cross-language ast-grep helpers: `has_pattern_in`, `find_pattern_matches_in`,
  `has_string_literal_in`. Today these dispatch on `Language::Rust` and `Language::Python` only; Go/Node fall through to
  empty results (intentional placeholder).
- `src/checks/source/mod.rs` — `all_source_checks(Language)` router; today only Rust and Python have populated
  sub-modules.
- `src/checks/source/python/bare_except.rs` — canonical AST-walking check (no ast-grep `Pattern` — walks `except_clause`
  nodes directly). Shows the idiomatic shape for grammar-specific node-kind checks.
- `src/checks/source/python/sys_exit.rs` — canonical call-site check with guard-scope awareness (`if __name__ ==
  "__main__":`). Shows how to structure context-sensitive Python checks.
- `src/checks/source/python/no_color.rs` — canonical "pattern-list plus string-literal fallback" check; easiest template
  for cross-language adaptations.
- `src/checks/source/rust/unwrap.rs` — simplest ast-grep `Pattern` check; template for Go equivalents.
- `src/checks/source/rust/process_exit.rs` — multi-pattern check with file-scope exemption (main.rs); template for TS/Go
  entry-point exemptions.
- `src/checks/source/rust/mod.rs` and `src/checks/source/python/mod.rs` — registration pattern (`all_<lang>_checks()`
  returns `Vec<Box<dyn Check>>`).
- `src/check.rs` — `Check` trait: `id()`, `group()`, `layer()`, `applicable()`, `run()`. `run()` is the sole
  `CheckResult` constructor (see CLAUDE.md's Source Check Convention).
- `Cargo.toml` — `ast-grep-language` feature list currently `[tree-sitter-rust, tree-sitter-python]`; new features
  confirmed available upstream are `tree-sitter-go`, `tree-sitter-ruby`, `tree-sitter-typescript` (pinned at `=0.42.0`).
- `tests/integration.rs` and `tests/fixtures/` — existing fixture architecture (per-language subdirs with manifest + src
  files + expected scorecard snapshots).

### Institutional Learnings

- `docs/plans/2026-04-02-004-feat-python-checks-validation-coverage-plan.md` — the successful v0.1 Python expansion.
  Pattern: start with 2–3 starter checks per new language, prove the pipeline end-to-end (parse → match → evidence), add
  fixtures, defer breadth. Same playbook applies here three times over.
- `CLAUDE.md` conventions (repo-local):
- `ast-grep-core` and `ast-grep-language` pinned to exact version (`=0.42.0`) — any grammar addition flows through the
  same pin.
- `Position` uses `.line()` / `.column(&node)` methods, not tuple access.
- Pre-build `Pattern` objects for `find_all()` — `&str` rebuilds on every node.
- Feature flags are `tree-sitter-<lang>`, not `language-<lang>`.
- `run()` is the sole `CheckResult` constructor; `check_x()` helper returns `CheckStatus`.
- For cross-language pattern helpers, extend `source::has_pattern_in()` / `find_pattern_matches_in()` /
  `has_string_literal_in()` with new `Language` variants — no private per-language helpers in individual check files.

### External References

- ast-grep documentation on meta-variables and language-specific conventions
  (<https://ast-grep.github.io/guide/pattern-syntax.html>).
- ast-grep-language crate source confirms exposed types: `Go`, `Ruby`, `TypeScript`, `Tsx`, `JavaScript`. `Go` and
  `Ruby` are registered via `impl_lang_expando!` with meta-var char `µ`, meaning `$VAR`-style patterns are not the
  syntax for those languages — this is the single most important spike input.
- Go CLI framework landscape: standard library `flag`/`os`, `spf13/cobra`, `urfave/cli`. Idioms include explicit error
  returns, `os.Exit(code)` at entry point, `log.Fatal*` family, `panic()` for unrecoverable state, and `!= nil` error
  checks. The `os.Getenv("NO_COLOR")` convention is a direct analog to the Rust/Python NO_COLOR check.
- Ruby CLI framework landscape: `OptionParser` (stdlib), `Thor`, `GLI`, `Dry::CLI`. Idioms include `exit` / `abort` /
  `raise`, `ENV["NO_COLOR"]`, rescue with no class (analog to bare `except:`), and `puts` as naked output.
- TypeScript CLI framework landscape: `commander`, `yargs`, `oclif`, `@clack/prompts`, `@inquirer/prompts`. Idioms
  include `process.exit(code)`, `console.log`, `process.env.NO_COLOR`, synchronous prompts (anti-pattern under agents),
  and `throw new Error(...)` vs structured exit codes. `.ts` and `.tsx` both parse under the `TypeScript`/`Tsx` grammars
  in ast-grep.

## Key Technical Decisions

- **Research spike first, implementation second.** The user explicitly requested a spike. The spike is its own
  implementation unit producing a written report — the plan treats spike outputs as the inputs for all subsequent
  per-language units. This is identical to the pattern that produced the successful `ast-grep-core v0.42.0 validated via
  spike (3 PoC checks, 18 tests pass)` note in CLAUDE.md.
- **Language-extension map becomes a slice, not a string.** `parsed_files()` currently maps each language to a single
  extension string (`"rs"`, `"py"`, `"go"`, `"js"`). TypeScript needs `.ts` + `.tsx`; Ruby wants `.rb` + optionally
  `.rake`. Change `ext` to a slice of extensions per language so `walk_source_files` can accept multiple extensions
  without breaking the current single-extension callers. Rationale: preserves the cache's one-pass walk and keeps the
  public API of `Project::parsed_files` stable.
- **TypeScript is a distinct `Language::TypeScript` variant, not a sub-mode of `Language::Node`.** Rationale: (1) the
  check set diverges — TypeScript has structural checks (type annotations, interface shapes) that do not apply to JS;
  (2) the grammar is literally different (`TypeScript` / `Tsx` vs `JavaScript`); (3) manifest detection can distinguish
  `tsconfig.json` from plain `package.json`, giving a clean applicable() boundary. Node → JS remains a placeholder for
  future JS-specific work but is not populated in this plan. The spike must validate this split by confirming
  `tsconfig.json` detection fits cleanly into `detect_language()`'s ordered match.
- **Ruby gets a new `Language::Ruby` variant** with `Gemfile`, `*.gemspec`, and `.ruby-version` as detection signals (in
  that order). This requires extending the `Language` enum, the manifest table, and all match arms currently enumerating
  languages.
- **Meta-variable character for Go and Ruby.** ast-grep-language 0.42.0 registers Go and Ruby via `impl_lang_expando!`
  with meta-var char `µ`, meaning `$VAR` is not the meta-var syntax for those grammars. Cross-language helpers
  (`has_pattern_in`, `find_pattern_matches_in`, `has_string_literal_in`) must either (a) accept language-specific
  pattern strings as-is, trusting callers to use the right meta-var char, or (b) normalize a `$`-style convention
  internally and rewrite to `µ` for those grammars. Recommend option (a) — pattern strings are already pre-language in
  every call site, and option (b) would hide a non-obvious transformation. The spike must confirm the exact meta-var
  character empirically (validate with a `µRECV.unwrap()`-style probe) before units 3–5 commit patterns.
- **Grammar cost accepted, measured.** Adding three tree-sitter grammars materially grows the `anc` binary. The spike
  measures binary-size and compile-time delta (stripped release build) per grammar so the decision to ship all three is
  grounded in numbers rather than guess. Hard cap: if a single grammar adds >5 MB stripped binary size, pause and
  reconsider scope.
- **No private per-language helpers in check files.** All Go/TS/Ruby checks must use `src/source.rs` helpers extended
  with their `Language` variant. This mirrors the Python convention codified in CLAUDE.md and prevents drift.
- **Starter check selection criteria.** Each language ships 3–4 checks chosen to (a) prove the pipeline (one simple
  `Pattern` check, one AST-walking check), (b) map to at least two distinct principles (P1–P7), and (c) exercise
  cross-language helper semantics. The spike produces the candidate list; this plan anchors the criteria.
- **Ordering: Go → TypeScript → Ruby.** Go reuses an existing Language variant, uses the familiar `.go` single
  extension, and has the closest idiom overlap with Rust checks already written. TypeScript is second — adds
  multi-extension walk and new Language variant but stays in `$`-style meta-var territory. Ruby is last — introduces the
  new Language variant, meta-var character, and the highest idiomatic distance from the existing check set, so it
  benefits most from learnings in the earlier two.

## Open Questions

### Resolved During Planning

- **Should Ruby be added at all given it is entirely new to the codebase?** Yes — the user's request is explicit, and
  `ast-grep-language 0.42.0` exposes `Ruby` via `impl_lang_expando!` with the same API shape used for every other
  grammar. The `impl_lang_expando!` registration is a concrete signal that Ruby is a first-class grammar, not a
  second-class one.
- **Should TypeScript share a `Language` variant with Node/JS?** No — split into a distinct `Language::TypeScript`
  variant. See Key Technical Decisions.
- **Does this plan change the behavioral check set?** No. Behavioral checks run the binary and are language-agnostic;
  they need no changes. Only the source-check layer grows.
- **Feature flag naming confirmed.** Upstream names are `tree-sitter-go`, `tree-sitter-ruby`, `tree-sitter-typescript`,
  `tree-sitter-javascript`. No `tree-sitter-tsx` feature — `Tsx` is part of `tree-sitter-typescript`.

### Deferred to Implementation

- **Exact starter check list per language.** Candidate sets appear in Units 3–5 but must be validated by the spike (Unit

1) — a pattern may fail to match the AST shape we expect, or a check may turn out to be redundant with an existing
   behavioral check. The spike is the gate.

- **Precise tree-sitter node kind names per language** (e.g., what is Go's bare-panic node? what is Ruby's `rescue` node
  kind?). These surface during spike probing; encoding them in the plan would risk committing to wrong identifiers.
- **TypeScript entry-point heuristic.** For a `process::exit`-style check in TS, what counts as the entry point? Options
  include `bin` entries in `package.json`, files named after `main` in `package.json`, or a convention like `cli.ts`.
  Resolve after spike examines real TS CLI repos.
- **Ruby entry-point heuristic.** Equivalent question for `exit` / `abort` outside `if __FILE__ == $0` idiom.
- **Whether to include a `.rake` extension in Ruby's walk.** Decide during Unit 2 based on whether starter checks apply
  to Rakefiles — likely not.
- **Node-kind-based Tsx handling.** If any TS check needs to run against `.tsx` files specifically (React components),
  decide whether to dispatch on `Tsx` vs `TypeScript` per-file or uniformly parse all `.ts*` as `TypeScript`. The
  starter checks are unlikely to need this distinction; revisit if a check becomes Tsx-sensitive.
- **Binary-size budget enforcement.** Whether to gate grammars behind Cargo features (so advanced users can opt out)
  depends on measured size. Resolve once the spike reports actual deltas.

## High-Level Technical Design

> *This illustrates the intended approach and is directional guidance for review, not implementation specification. The
> implementing agent should treat it as context, not code to reproduce.*

```text
Language enum expansion
  Rust | Python | Go | Node              (today)
  Rust | Python | Go | Node | TypeScript | Ruby   (target)

detect_language() manifest priority  (first match wins, more-specific-first ordering)
  Cargo.toml       -> Rust
  pyproject.toml   -> Python
  tsconfig.json    -> TypeScript          (NEW, must precede package.json)
  package.json     -> Node                (unchanged, now JS-only fallback)
  go.mod           -> Go
  Gemfile          -> Ruby                (NEW)
  *.gemspec        -> Ruby                (NEW, checked after Gemfile)

walk_source_files language->extensions
  Rust        -> ["rs"]
  Python      -> ["py"]
  Go          -> ["go"]
  Node        -> ["js", "mjs", "cjs"]     (widened from "js")
  TypeScript  -> ["ts", "tsx"]            (NEW, multi-extension)
  Ruby        -> ["rb"]                   (NEW)

source.rs dispatch (has_pattern_in, find_pattern_matches_in, has_string_literal_in)
  match lang {
    Rust       => <use Rust grammar, $-meta-var patterns>,
    Python     => <use Python grammar, $-meta-var patterns>,
    Go         => <use Go grammar, µ-meta-var patterns>,        (NEW)
    TypeScript => <use TypeScript grammar, $-meta-var patterns>,(NEW)
    Ruby       => <use Ruby grammar, µ-meta-var patterns>,      (NEW)
    Node       => placeholder — empty results until JS checks land,
  }

Per-language check module layout (mirrors src/checks/source/python/)
  src/checks/source/go/mod.rs            -> all_go_checks()
  src/checks/source/go/<check>.rs        -> Check impl + check_x() helper + tests
  src/checks/source/typescript/mod.rs    -> all_typescript_checks()
  src/checks/source/typescript/<check>.rs
  src/checks/source/ruby/mod.rs          -> all_ruby_checks()
  src/checks/source/ruby/<check>.rs

all_source_checks(Language) router extends to include Go, TypeScript, Ruby branches.
```

## Implementation Units

- [x] **Unit 1: Research spike — ast-grep viability and starter check candidates** — completed 2026-04-17, report at
  `docs/plans/spikes/2026-04-17-multi-language-source-checks-spike.md`

**Goal:** Produce a written spike report that characterizes, per language (Go, TypeScript, Ruby): the exact
ast-grep-language feature flag name, the Rust type exposed for that grammar, the meta-variable character, 4–6 candidate
starter checks with their proposed ast-grep pattern strings or AST-walk node kinds, the approximate binary-size delta
from enabling the grammar (stripped release), the approximate compile-time delta, and any surprises uncovered (e.g.,
unexpected node kinds, pattern rejections). The spike report is the gate for Units 3–5.

**Requirements:** R1.

**Dependencies:** None.

**Files:**

- Create: `docs/plans/spikes/2026-04-17-multi-language-source-checks-spike.md`
- Create (temporary, scratch): spike can use `examples/spike-*/` scratch crates or a local throwaway branch for
  empirical probes; no source code under `src/` is modified in this unit.
- Test: none — the spike's output is the report, not code.

**Approach:**

- Read the ast-grep-language 0.42.0 source (GitHub `ast-grep/ast-grep` tag `0.42.0`, `crates/language/src/lib.rs`) to
  confirm each exposed type and its `impl_lang_*` macro. Note any grammar that uses `impl_lang_expando!` — those have a
  non-`$` meta-variable character (confirmed `µ` for Go and Ruby).
- For each language, write a minimal probe binary (scratch crate, not shipped) that parses a representative source
  snippet and attempts 3–4 patterns. Capture which patterns match cleanly and which trip on AST shape. Record the
  node-kind name for the structural concepts we will walk (e.g., Ruby `rescue` node, Go `panic_call` node).
- Measure stripped release binary size with and without each grammar feature enabled by toggling `Cargo.toml` features
  and running `cargo build --release --locked` — record the delta.
- Enumerate 4–6 candidate starter checks per language, mapping each to an agent-readiness principle (P1–P7 or
  `code-quality`) and citing an existing Rust/Python check as the template.
- For Ruby, document the `Gemfile` / `*.gemspec` detection strategy and confirm the manifest ordering in
  `detect_language` does not create conflicts.
- For TypeScript, document the `tsconfig.json` vs `package.json` ordering and whether the presence of both should
  classify as TypeScript (recommended yes, if the spike confirms).

**Execution note:** Characterization-first — this unit exists to de-risk the remaining units. No production code is
written until the spike findings are in the report.

**Patterns to follow:**

- The original `ast-grep-core v0.42.0` spike noted in `CLAUDE.md` (three PoC checks + 18 tests) is the precedent for
  this spike's format and depth.
- Learnings recorded under `docs/solutions/` per `CLAUDE.md` conventions — the spike report may spawn a
  `docs/solutions/architecture-patterns/multi-language-ast-grep-*.md` entry if a generally-useful decision emerges.

**Test scenarios:**

- Test expectation: none — this unit produces a written report, not behavior. The report is the deliverable.

**Verification:**

- Report exists at the target path.
- Report documents, for each of Go, TypeScript, and Ruby: feature flag, exposed type, meta-var char, 4–6 starter check
  candidates with pattern strings or node kinds, stripped binary-size delta, any known pattern-rejection surprises.
- Subsequent units can quote specific findings from the report as the basis for their per-language pattern choices.

---

- [ ] **Unit 2: Cross-cutting infrastructure — Language variants, extension walk, dispatch, feature flags**

**Goal:** Add `Language::TypeScript` and `Language::Ruby` variants; widen `walk_source_files` to accept multi-extension
lists per language; extend `source::has_pattern_in` / `find_pattern_matches_in` / `has_string_literal_in` to dispatch
Go, TypeScript, and Ruby (accounting for Ruby's and Go's `µ` meta-variable character when applicable); extend
`detect_language` to recognize `tsconfig.json` and `Gemfile` / `*.gemspec`; enable the `tree-sitter-go`,
`tree-sitter-ruby`, and `tree-sitter-typescript` feature flags on `ast-grep-language` in `Cargo.toml`.

**Requirements:** R2, R3, R4 (infrastructure prerequisites for all three).

**Dependencies:** Unit 1 (spike must confirm meta-var char, manifest priority, and grammar cost before this commits).

**Files:**

- Modify: `Cargo.toml` (add three feature flags to the `ast-grep-language` dependency line).
- Modify: `src/project.rs` (Language enum, detect_language manifest table, parsed_files extension map, walk_source_files
  signature if extensions becomes a slice).
- Modify: `src/source.rs` (has_pattern_in, find_pattern_matches_in, has_string_literal_in — extend match arms).
- Modify: `src/checks/source/mod.rs` (all_source_checks router — add TypeScript, Ruby branches; Go branch already exists
  but currently returns `vec![]`, will be wired to `go::all_go_checks()` in Unit 3).
- Test: `src/project.rs::tests` (detect_language for tsconfig.json, Gemfile, gemspec); `src/source.rs::tests`
  (has_pattern_in for Go, TypeScript, Ruby; has_string_literal_in for each; confirm `µ`-meta-var patterns match for Go
  and Ruby; confirm `$`-meta-var patterns match for TypeScript).

**Approach:**

- Change `parsed_files()`'s `ext` from a `&str` to a `&[&str]` (or equivalent) and adjust `walk_source_files` /
  `walk_source_files_inner` signatures to accept multiple extensions. Update the `Rust | Python | Go | Node` extension
  lists so the existing languages continue to match exactly the files they match today; add `TypeScript -> ["ts",
  "tsx"]` and `Ruby -> ["rb"]`.
- `detect_language`: insert the `tsconfig.json` check between `pyproject.toml` and `package.json` so a repo containing
  both `tsconfig.json` and `package.json` is classified as TypeScript. Insert `Gemfile` before `*.gemspec` detection.
  Use glob-like detection for `*.gemspec` (list directory, look for any file ending in `.gemspec`).
- `source.rs` helpers: each match arm dispatches to the matching `ast-grep-language` type. The four existing helpers
  should be extended to cover every populated `Language`; `Language::Node` remains in the "return empty" placeholder
  until JS checks land.
- For Go and Ruby, the helpers' contract is unchanged — the caller passes a pattern string, the helper forwards it to
  ast-grep unchanged. Add a doc comment on each helper explicitly noting that Go and Ruby use `µ` as meta-var char and
  that callers must write patterns accordingly.
- Check CI impact: the stripped binary will grow; update `scripts/hooks/pre-push` and CI matrix expectations only if the
  spike recorded a binary-size cliff that warrants a new check.

**Execution note:** Land this unit as a single commit. It is a cross-cutting refactor that should keep every existing
test green; splitting further would create awkward intermediate states where the Language enum is partially wired.

**Patterns to follow:**

- The existing `src/source.rs` dispatch pattern (match on `Language`, return empty vec / false for unsupported).
- The existing `detect_language` manifest table ordering.
- Python-check file-name → extension convention.

**Test scenarios:**

- Happy path: `detect_language` on a directory with only `tsconfig.json` returns `Language::TypeScript`.
- Happy path: `detect_language` on a directory with only `Gemfile` returns `Language::Ruby`.
- Happy path: `detect_language` on a directory with only `foo.gemspec` returns `Language::Ruby`.
- Edge case: directory with both `tsconfig.json` and `package.json` returns `Language::TypeScript` (tsconfig wins).
- Edge case: directory with neither manifest returns `None`.
- Happy path: `walk_source_files` for TypeScript picks up both `.ts` and `.tsx` files.
- Happy path: `walk_source_files` for Ruby picks up `.rb` files.
- Happy path: `has_pattern_in` returns true for a valid `$FOO.bar()` pattern in TypeScript source.
- Happy path: `has_pattern_in` returns true for a valid `µVAR` pattern in Go source matching a well-known construct.
- Happy path: `has_pattern_in` returns true for a valid `µVAR` pattern in Ruby source matching `raise µERR`.
- Edge case: `has_pattern_in` with a malformed pattern returns false without panicking (existing contract).
- Integration: existing Rust and Python helper tests continue to pass without modification (regression guard).

**Verification:**

- `cargo test --locked` passes on all three platforms covered by the current CI matrix (Linux, macOS, Windows).
- `cargo build --release --locked` produces a binary whose size delta matches the spike's recorded measurement to within
  ~10% (larger deltas indicate an unexpected dependency addition).
- `anc check .` (dogfooding) returns the same scorecard it did before the change — no Go/TS/Ruby checks run on this Rust
  repo, so the scorecard is unchanged.

---

- [ ] **Unit 3: Go source checks — starter set**

**Goal:** Implement 3–4 Go starter checks in `src/checks/source/go/`, register them via `all_go_checks()`, and wire that
into `all_source_checks(Language::Go)`.

**Requirements:** R2.

**Dependencies:** Unit 1 (pattern and check-list are spike outputs), Unit 2 (Language enum and dispatch must be in
place).

**Files:**

- Create: `src/checks/source/go/mod.rs` (re-exports plus `all_go_checks()`).
- Create: `src/checks/source/go/<check>.rs` for each starter check. Starter list confirmed by Unit 1's spike report
  (section "Per-grammar findings: Go → Starter check candidates"):
- `code-go-panic` (code-quality — analog to `code-unwrap`); `Pattern::try_new("panic($MSG)", Go)` works at top level.
- `p4-go-os-exit` (P4 — analog to `p4-process-exit`); **AST walking required** — top-level `os.Exit($CODE)` patterns
    fail to parse as a call. Walk `call_expression > selector_expression(os, Exit)`; exempt files whose `package_clause`
    is `package main`.
- `p4-go-log-fatal` (P4); AST walking for `call_expression > selector_expression(log, Fatal|Fatalf|Fatalln)`. Same
    main-package exemption.
- `p6-go-no-color` (P6 — analog to `p6-no-color`); pattern `os.Getenv($KEY)` fails top-level so use AST walk plus
    `source::has_string_literal_in(source, "NO_COLOR", Go)` fallback.
- Modify: `src/checks/source/mod.rs` — wire `all_source_checks(Language::Go)` to `go::all_go_checks()`.
- Test: inline `#[cfg(test)]` module per check file (matches Python convention: `check_x()` helper tested directly,
  `run()` tested with a temporary-directory fixture).

**Approach:**

- Each check follows the Python file layout exactly (see `src/checks/source/python/bare_except.rs` for AST-walking shape
  and `src/checks/source/python/sys_exit.rs` for guard-scope shape). The `Check` trait implementation is the sole
  `CheckResult` constructor; `check_x()` helpers return `CheckStatus`.
- Use `src/source.rs` helpers (extended in Unit 2) — do not write private per-language helpers.
- Include explanatory module doc comments naming the principle and the analogous Rust/Python check.

**Execution note:** Implement test-first — write the inline unit tests around `check_x()` before the `run()` wiring so
spike pattern decisions are validated empirically per check.

**Patterns to follow:**

- `src/checks/source/rust/unwrap.rs` — simplest pattern check (pattern string + helper-call + Pass/Fail).
- `src/checks/source/rust/process_exit.rs` — multi-pattern + entry-point exemption.
- `src/checks/source/python/no_color.rs` — pattern-list + string-literal fallback.

**Test scenarios:**

- Happy path: Each check's Pass case — a Go source string that should not trigger — returns `CheckStatus::Pass`.
- Happy path: Each check's Fail/Warn case — a Go source string that should trigger — returns the right status with
  evidence text that includes the file path, line, column, and matched snippet (the existing Python evidence format).
- Edge case: For `os_exit_outside_main`, confirm `main` package is exempted; confirm non-main packages are flagged.
- Edge case: For `no_color`, confirm string-literal fallback catches a `const noColor = "NO_COLOR"` reference.
- Edge case: Each check called with `""` (empty source) returns `Pass` without panicking.
- Error path: Each check with a syntactically invalid Go source does not panic — either ast-grep parses best-effort or
  the helper returns `Pass` / empty matches (must match existing Rust/Python behavior).
- Integration: `Check::applicable` returns true for a project whose `Project::language` is `Some(Language::Go)` and
  false for any other variant.
- Integration: `Check::run` against a temp-dir project with a Go file in `src/` aggregates evidence across multiple
  files and returns the combined `CheckStatus`.

**Verification:**

- `cargo test --locked` passes all new Go tests plus every existing test.
- `anc check` against a small Go fixture repo (see Unit 6) produces the expected scorecard rows for the new checks.
- Dogfooding (`anc check .`) remains green — Go checks are not applicable to this Rust repo.

---

- [ ] **Unit 4: TypeScript source checks — starter set**

**Goal:** Implement 3–4 TypeScript starter checks in `src/checks/source/typescript/`, register them via
`all_typescript_checks()`, and wire into `all_source_checks(Language::TypeScript)`.

**Requirements:** R3.

**Dependencies:** Unit 1, Unit 2.

**Files:**

- Create: `src/checks/source/typescript/mod.rs` and per-check files. Canonical candidates (final list from spike):
- `process_exit_outside_entry` (P4 — analog to `p4-process-exit`); identifies entry point via `package.json`'s `bin`
  field or a `cli.ts` / `index.ts` convention recommended by the spike.
- `no_color` (P6 — analog to `p6-no-color`); looks for `process.env.NO_COLOR`, `process.env["NO_COLOR"]`, and the
  string-literal fallback.
- `naked_console_log` (code-quality — analog to `code-naked-println`); flags `console.log` outside an output module.
  Entry-point heuristic same as `process_exit_outside_entry`.
- Optional fourth: `throw_string_literal` (P4 — analog to `p4-error-types`); flags `throw "message"` and `throw new
  Error("message")` in favor of typed custom-error classes.
- Modify: `src/checks/source/mod.rs` — wire `all_source_checks(Language::TypeScript)`.
- Test: inline `#[cfg(test)]` per check file.

**Approach:**

- TypeScript uses the standard `$`-meta-var character, so patterns follow the Rust/Python shape directly.
- Dispatch via `ast-grep-language::TypeScript`. If a check must run against both `.ts` and `.tsx`, parse each file
  individually using the extension to pick `TypeScript` vs `Tsx` — only add this branching if the spike says a starter
  check needs it. For the starter list, uniform `TypeScript` parsing is likely sufficient.
- Entry-point heuristic: recommended algorithm (to be confirmed in spike) — read `package.json`'s `bin` object; if
  present, treat those paths as entry points; otherwise, fall back to files named `cli.ts`, `cli.tsx`, `index.ts`, or
  the file referenced by `main`. Document the heuristic's limitations in the check's module doc comment.

**Execution note:** Characterization-first for `process_exit_outside_entry` — start with a passing and a failing
fixture so the entry-point heuristic is validated against real code before the check ships.

**Patterns to follow:**

- `src/checks/source/rust/process_exit.rs` — file-path exemption for main.
- `src/checks/source/rust/naked_println.rs` — output-module heuristic.
- `src/checks/source/python/no_color.rs` — pattern-list + string-literal fallback.

**Test scenarios:**

- Happy path: Each check's Pass and Fail cases produce expected statuses and evidence.
- Edge case: `.tsx` parsing — a React component file does not cause a panic or false positive for any starter check (the
  starter checks target backend-flavored code).
- Edge case: `process_exit_outside_entry` correctly exempts a file declared as `bin` in `package.json`.
- Edge case: `no_color` catches `process.env["NO_COLOR"]` (bracket access) as well as `process.env.NO_COLOR`.
- Edge case: `throw_string_literal` does not flag `throw new CustomError("msg")` where `CustomError` is not the built-in
  `Error`.
- Error path: TypeScript file with a syntax error does not crash the run (matches existing contract).
- Integration: `Check::applicable` returns true only for `Language::TypeScript`; Node, Rust, Python, Go, Ruby all return
  false.
- Integration: `Check::run` aggregates evidence across `.ts` and `.tsx` files under the project.

**Verification:**

- `cargo test --locked` passes.
- `anc check` against a small TS fixture produces expected scorecard rows.
- Dogfooding remains green.

---

- [ ] **Unit 5: Ruby source checks — starter set**

**Goal:** Implement 3–4 Ruby starter checks in `src/checks/source/ruby/`, register via `all_ruby_checks()`, wire into
`all_source_checks(Language::Ruby)`, and validate `µ` meta-var usage end-to-end.

**Requirements:** R4.

**Dependencies:** Unit 1, Unit 2.

**Files:**

- Create: `src/checks/source/ruby/mod.rs` and per-check files. Canonical candidates (final list from spike):
- `bare_rescue` (code-quality — analog to Python's `code-bare-except`); flags `rescue` with no class name. AST-walking
  check on the `rescue` node kind.
- `no_color` (P6 — analog to `p6-no-color`); looks for `ENV["NO_COLOR"]`, `ENV.fetch("NO_COLOR")`, and the
  string-literal fallback.
- `exit_outside_entry` (P4 — analog to `p4-process-exit`); flags `exit`, `abort`, `Kernel.exit` outside the `if __FILE__
  == $0` idiom.
- Optional fourth: `raise_string_literal` (P4 — analog to Rust's error-types check); flags `raise "message"` without a
  StandardError subclass.
- Modify: `src/checks/source/mod.rs` — wire `all_source_checks(Language::Ruby)`.
- Test: inline `#[cfg(test)]` per check file.

**Approach:**

- Patterns for Ruby use the `µ` meta-variable character (confirmed by spike Unit 1). Document this prominently in
  `src/checks/source/ruby/mod.rs` as a module-level doc comment so future contributors don't write `$VAR` patterns by
  mistake.
- AST-walking for `bare_rescue` follows the Python `bare_except` pattern exactly — walk the tree, match on the `rescue`
  node kind, check whether the node has an exception-class child.
- `exit_outside_entry` requires detecting Ruby's entry-point idiom `if __FILE__ == $0` or `if $PROGRAM_NAME == __FILE__`
  (several canonical forms exist; list comes from spike). The scope-tracking approach mirrors Python's `sys_exit`
  `is_main_guard()` logic.

**Execution note:** Test-first — Ruby has the lowest local pattern density in this codebase (no existing Ruby code),
so confidence in pattern correctness should come from tests, not eye-balling.

**Patterns to follow:**

- `src/checks/source/python/bare_except.rs` — AST-walking structural check.
- `src/checks/source/python/sys_exit.rs` — entry-point idiom detection via header-text pattern match across several
  canonical forms.
- `src/checks/source/python/no_color.rs` — pattern-list + string-literal fallback.

**Test scenarios:**

- Happy path: `bare_rescue` passes for `rescue StandardError`, fails for bare `rescue`.
- Happy path: `no_color` passes when `ENV["NO_COLOR"]` or the string-literal is present.
- Happy path: `exit_outside_entry` passes when `exit 0` appears inside `if __FILE__ == $0`; fails when it appears at top
  level or inside a regular method.
- Edge case: `bare_rescue` ignores `rescue` inside a string literal or a heredoc (AST-awareness guard).
- Edge case: `exit_outside_entry` recognizes both `if __FILE__ == $0` and `if $PROGRAM_NAME == __FILE__` forms.
- Edge case: `raise_string_literal` does not flag `raise MyError, "msg"` (class + message) or `raise
  MyError.new("msg")`.
- Edge case: Each check called with `""` returns `Pass`.
- Error path: Ruby file with a syntax error does not crash the run.
- Integration: `Check::applicable` returns true only for `Language::Ruby`.
- Integration: `Check::run` aggregates evidence across `.rb` files.
- Integration: Ruby pattern with `µ`-meta-var is exercised in at least one test, confirming Unit 2's dispatch path is
  wired correctly.

**Verification:**

- `cargo test --locked` passes.
- `anc check` against a small Ruby fixture produces expected scorecard rows.
- Dogfooding remains green.

---

- [ ] **Unit 6: Fixtures, integration tests, documentation**

**Goal:** Add one fixture directory per new language exercising at least one passing and one failing check; add or
update integration tests to cover the new fixtures; update README, CLAUDE.md conventions, and the design-doc reference
table to reflect the new coverage.

**Requirements:** R5, R6.

**Dependencies:** Units 3, 4, 5.

**Files:**

- Create: `tests/fixtures/broken-go/` with `go.mod` plus a `.go` file containing at least one violation per Go starter
  check.
- Create: `tests/fixtures/perfect-go/` with a clean Go project that passes every Go starter check.
- Create: `tests/fixtures/broken-typescript/` with `tsconfig.json` + `package.json` plus `.ts` and `.tsx` files
  containing at least one violation per TS starter check.
- Create: `tests/fixtures/perfect-typescript/` (clean TS project).
- Create: `tests/fixtures/broken-ruby/` with `Gemfile` plus `.rb` files containing at least one violation per Ruby
  starter check.
- Create: `tests/fixtures/perfect-ruby/` (clean Ruby project).
- Modify: `tests/integration.rs` — add test cases that run `anc check` against each new fixture and assert on the
  scorecard output. Follow the existing Rust/Python integration test structure.
- Modify: `README.md` — update the Supported Languages section (list Go, TypeScript, Ruby with their starter check
  counts).
- Modify: `CLAUDE.md` — update the "Architecture" / "Source Check Convention" sections to reference the new per-language
  modules and note Go and Ruby's `µ` meta-variable character.
- Modify: `~/.gstack/projects/brettdavies-agentnative/brett-main-design-20260327-214808.md` (the design doc, referenced
  from `CLAUDE.md`) — update the supported-languages table only if the design doc already has one; otherwise defer to a
  follow-up docs plan. (This is explicitly optional; do not block Unit 6 on design-doc edits.)
- Modify: `docs/solutions/` — add a solution entry capturing the spike's outcome and the Go/Ruby meta-variable
  convention under `docs/solutions/architecture-patterns/`. Ensure the solution has the `module`, `tags`, `problem_type`
  frontmatter required for qmd search.

**Approach:**

- Fixtures follow the existing `tests/fixtures/broken-python/` and `tests/fixtures/perfect-python/` layout for parity.
- Integration tests follow the existing assertion style (check the aggregated scorecard includes expected rows with
  expected statuses; avoid exact-text snapshots that brittle-out on cosmetic output changes unless the existing tests
  already use insta snapshots — in which case use insta and accept the snapshot review cost).
- README update: add the three languages to the "supported languages" listing; keep the existing tone and structure.
- CLAUDE.md update: append a note to the "Conventions" or "Source Check Convention" section documenting that Go and Ruby
  patterns use `µ` rather than `$`, with a pointer to `docs/solutions/` for the rationale.

**Patterns to follow:**

- `tests/fixtures/broken-python/`, `tests/fixtures/perfect-python/`, and `tests/fixtures/broken-rust/`,
  `tests/fixtures/perfect-rust/`.
- `tests/integration.rs` existing structure for per-fixture test methods.
- Existing CLAUDE.md "Source Check Convention" and "Conventions" sections.

**Test scenarios:**

- Integration: `anc check tests/fixtures/broken-go` exits non-zero and the scorecard shows each Go starter check with a
  failure/warning as appropriate.
- Integration: `anc check tests/fixtures/perfect-go` exits zero and the scorecard shows each Go starter check as Pass.
- Integration: Same pair for TypeScript and Ruby fixtures.
- Integration: Scorecard output format remains stable for existing Rust/Python fixtures (regression guard — Unit 6 must
  not change existing snapshots beyond adding rows for newly registered checks).
- Edge case: A TypeScript fixture containing both `.ts` and `.tsx` files produces evidence from both when a check
  matches in both.

**Verification:**

- `cargo test --locked` passes including all new integration tests.
- Manual: `anc check tests/fixtures/broken-ruby/` (etc.) produces human-readable scorecard output consistent with
  existing output for broken-python.
- README renders correctly when previewed locally (headings, code blocks, tables).
- CLAUDE.md changes merged without accidentally breaking the existing `Source Check Convention` instructions the team
  relies on.

## System-Wide Impact

- **Interaction graph:** `Project::discover` → `detect_language` → `parsed_files` → `all_source_checks` per-language
  routing. All four touchpoints change in Unit 2 and must stay mutually consistent — a new Language variant without a
  corresponding `detect_language` branch, `parsed_files` extension entry, or `all_source_checks` arm creates a
  silently-dead code path. Unit 2's cross-checks (exhaustive match arms, new tests per extension) are the guard.
- **Error propagation:** Ast-grep parse failures for malformed sources remain silent — ast-grep does best-effort
  parsing. Checks rely on `find_pattern_matches_in` / `has_pattern_in` returning empty-or-false on parse failure; this
  contract holds for the new languages via the existing dispatch pattern. No changes to error propagation semantics.
- **State lifecycle risks:** `Project::parsed_files` is a `OnceLock` one-shot cache. Adding new languages does not
  affect that lifecycle; each project still parses its single detected language. The cache is populated once per process
  and is immutable thereafter.
- **API surface parity:** The `Check` trait is unchanged. The `Language` enum gains two variants (`TypeScript`, `Ruby`);
  every external consumer that match-exhaustively destructures `Language` must update. Internal consumers are enumerated
  in Unit 2's file list; verify no external consumer exists (this is a binary-only crate today, so external API concern
  is limited to JSON output stability — confirm the `language` field serializes the new variants as `"typescript"` and
  `"ruby"` under `serde(rename_all = "snake_case")`).
- **Integration coverage:** Unit 6's fixtures cover the end-to-end path (`Project::discover` → `all_source_checks` →
  scorecard) per language, which is the only coverage surface that unit tests against in-memory source strings cannot
  prove.
- **Unchanged invariants:** Existing Rust and Python checks must continue to run on Rust/Python projects with identical
  scorecard output. Behavioral checks are unchanged. `anc`'s CLI surface (commands, flags) is unchanged — this plan adds
  source checks only, no new CLI entry points, no new flags. The dogfooding rules in `CLAUDE.md` ("bare invocation
  prints help", "safe probing only") are unchanged.

## Risks & Dependencies

| Risk | Mitigation |
| ---- | ---------- |
| ~~Ruby or Go `µ` meta-var breaks existing `$`-pattern assumptions in shared helpers.~~ **Defused by spike (2026-04-17).** `Rust` and `Python` already use `impl_lang_expando!` with `µ` internally; user-facing syntax is `$VAR` for every supported language. No cross-language helper change needed. | n/a |
| Go top-level pattern-parse failure for selector-expression calls (e.g. `os.Exit($CODE)` parses as `ERROR > type_conversion_expression` at the top level, not as a `call_expression`). Surfaced by spike (2026-04-17). | Unit 3 uses AST walking for selector-call checks (`os.*`, `log.*`, `fmt.*`) and reserves `Pattern::try_new` for bare builtin calls (`panic`, `print`). Every Go starter check ships with a Fail test case that would surface a pattern-parse regression immediately. |
| Ruby uppercase-global literal (`$PROGRAM_NAME`) cannot appear in a `Pattern` string — the expando-char rewriter turns it into a meta-var. Surfaced by spike (2026-04-17). | Unit 5's entry-point check uses AST walking + header-text inspection (same shape as Python's `sys_exit::is_main_guard`) rather than a literal `Pattern`. The `$0` digit global is unaffected and can appear in patterns as-is. |
| Stripped binary size grows enough that release artifacts cross a meaningful boundary (e.g., Homebrew formula size limits, CI artifact size). | Spike (Unit 1) measures the delta per grammar. If a grammar exceeds the 5 MB per-grammar cap called out in Key Technical Decisions, pause and reconsider — either gate that grammar behind a Cargo feature or drop the language from this plan. |
| `tsconfig.json` check in `detect_language` ordering conflicts with a real repo where only `package.json` exists but the project is TypeScript via other signals (e.g., only `.ts` files). | Spike validates ordering against representative repos. Document the ordering and the fallback behavior (project without tsconfig.json falls back to `Language::Node`). |
| `Gemfile` / `*.gemspec` detection misfires for a polyglot repo (Rust + Ruby scripts). | `detect_language` already uses first-match-wins on manifest files; extending the ordered table preserves this semantics. A Rust+Ruby repo with a `Cargo.toml` will classify as Rust; this is the desired behavior and the same as today's Rust+Python handling. |
| ast-grep-language 0.42.0 grammar versions lag upstream tree-sitter grammars, and a pattern working against current upstream tree-sitter-go fails against 0.42.0's pinned version. | Spike empirically validates every pattern against the pinned version, not against documentation or upstream examples. |
| Starter check selection turns out to be redundant with existing behavioral checks (e.g., a TS `process_exit_outside_entry` check duplicates what `--help exits 0` already proves). | Spike criteria include "maps to at least two distinct principles" and "proves cross-language helper semantics" — this forces diversity. Any check that only duplicates an existing behavioral check is dropped in Unit 1. |
| Meta-var character confusion causes silent pattern mismatches (no panic, just never matches). | Every per-language check's Fail test case is a concrete source string known to trigger — if the pattern is wrong the test fails, surfacing the issue at Unit 3/4/5 commit time rather than in production. |
| Adding three grammars meaningfully slows compile times for every contributor, even those working only on Rust checks. | Accepted — this is core product work, not optional tooling. If the slowdown is painful, a follow-up could split grammars behind Cargo features; not in scope here. |

## Documentation / Operational Notes

- The spike report itself (Unit 1) is a permanent doc at
  `docs/plans/spikes/2026-04-17-multi-language-source-checks-spike.md` and is referenced from this plan; do not delete
  it after implementation.
- README's supported-languages section is the user-facing promise; update it in Unit 6 *only* once the starter checks
  are actually merged and passing to avoid promising coverage that is not yet shipped.
- Changelog: each unit that changes behavior visible to users (Units 2, 3, 4, 5, 6) contributes bullets to the PR's `##
  Changelog` section per the global PR convention — specifically: "Added Go source checks (4 starter checks)", "Added
  TypeScript source checks", "Added Ruby source checks", and (if split into multiple PRs) feature-flag-enable notes per
  grammar.
- Release cadence: this work is a minor version bump (`v0.2.0`) consistent with the design-doc positioning that v0.2 is
  "more languages + `--fix`"; coordinate with whoever is cutting the next release before Unit 6 merges so the release
  notes land correctly. (The `--fix` half of v0.2 is a separate plan.)
- Toolchain: no toolchain bump is expected. If the spike reveals that a grammar requires a newer Rust edition or
  compiler, escalate — the project's `rust-toolchain.toml` pinning policy (reviewed PR, ≥7-day stable quarantine)
  applies.

## Alternative Approaches Considered

- **Add only Go first, defer TypeScript and Ruby to separate plans.** Rejected: the user explicitly asked for all three,
  and the cross-cutting infrastructure (multi-extension walk, `µ` dispatch, new Language variants) is easier to land as
  one coherent change than to re-open three times. Within this plan, sequencing (Unit 3 → 4 → 5) still lets reviewers
  merge language-by-language if PR-splitting is preferred.
- **Ship without a spike — skip straight to Unit 2.** Rejected: the user explicitly asked for a spike first, and the
  Go/Ruby `µ` meta-var discovery during plan research validated that instinct. Without the spike, Unit 2 would commit
  dispatch code based on assumptions that could turn out wrong.
- **Gate each grammar behind a Cargo feature so users can opt out.** Considered but not recommended for v0.2. The
  design-doc positioning is "zero external dependencies, everything compiles into a single binary" — feature-gated
  grammars muddy that pitch. If the spike reveals a painful binary-size cliff, revisit as a follow-up plan.
- **Unify Go and TypeScript under a single "dynamic languages" module rather than per-language subdirectories.**
  Rejected: the Python + Rust precedent is per-language directories, and each language has enough grammar-specific
  node-kind logic (AST walking) that a shared module would muddle the code.
- **Use ast-grep's YAML rule files instead of Rust code for pattern definitions.** Considered. Upside: easier to edit
  for non-Rust contributors, matches ast-grep's native distribution format. Downside: adds a runtime parse step,
  requires bundling YAML as string assets, and diverges from the existing Rust/Python check architecture. Rejected for
  this plan; revisit as a distinct architectural plan if external contributor velocity becomes a bottleneck.

## Success Metrics

- 10–12 new source checks (3–4 per language × 3 languages) land, each with ≥80% test coverage on its `check_x()` helper.
- `cargo test --locked` runs the full new suite green on Linux, macOS, and Windows CI runners.
- `anc` binary size growth falls within the cap documented in the spike (strictly below 15 MB total delta across all
  three grammars).
- `anc check tests/fixtures/broken-<lang>/` produces non-zero exit and scorecard rows that match the checks defined for
  that language, for all three languages.
- Dogfooding (`anc check .` on the agentnative repo itself) remains green throughout — no regressions in the scorecard
  for this Rust repo.
- Zero breaking changes to the public Rust API (none today beyond the binary) or JSON output schema for existing
  consumers.

## Phased Delivery

### Phase 1 — Spike

- Unit 1 lands as a single doc PR. No code changes. Purpose: unblock Units 2–5 with validated assumptions.

### Phase 2 — Infrastructure

- Unit 2 lands as a single code PR. Purpose: make `Language::TypeScript` and `Language::Ruby` real, dispatch the new
  grammars, and keep every existing test green. No new checks yet.

### Phase 3 — Per-language checks

- Units 3, 4, and 5 land sequentially as three code PRs (Go → TypeScript → Ruby). Each PR is self-contained: new
  language module + tests + registration. Reviewers can merge language-by-language.

### Phase 4 — Fixtures and docs

- Unit 6 lands as a single docs+tests PR once Phase 3 is complete. Purpose: integration coverage + user-facing docs.

## Sources & References

- Design doc: `~/.gstack/projects/brettdavies-agentnative/brett-main-design-20260327-214808.md` (the design doc
  committed to Rust/Python at launch and Go/Node "deeper" support as the next increment; this plan is that increment
  plus Ruby).
- Prior plan: `docs/plans/2026-04-02-004-feat-python-checks-validation-coverage-plan.md` (the successful Python
  starter-check rollout this plan mirrors).
- Existing code: `src/source.rs`, `src/project.rs`, `src/checks/source/python/`, `src/checks/source/rust/`,
  `src/check.rs`, `src/checks/source/mod.rs`.
- External: ast-grep-language 0.42.0 crate source — feature flags `tree-sitter-go`, `tree-sitter-ruby`,
  `tree-sitter-typescript`; exposed types `Go`, `Ruby`, `TypeScript`, `Tsx`, `JavaScript`; Go/Ruby registered via
  `impl_lang_expando!` with `µ` meta-var.
- Conventions: `CLAUDE.md` (Source Check Convention, Dogfooding Safety, CI and Quality sections).
