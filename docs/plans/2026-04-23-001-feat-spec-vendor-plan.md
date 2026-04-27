---
title: "feat: vendor agentnative-spec — generate REQUIREMENTS at build time + drift-check"
type: feat
status: active
date: 2026-04-23
parents:
  - https://github.com/brettdavies/agentnative/blob/dev/docs/plans/2026-04-22-002-post-frontmatter-roadmap.md
  - https://github.com/brettdavies/agentnative/blob/dev/docs/plans/2026-04-22-001-feat-requirement-id-frontmatter-plan.md
roadmap-item: 5 (spec-repo roadmap 002, item 5)
---

# feat: vendor agentnative-spec — generate REQUIREMENTS at build time + drift-check

## Overview

Complete the IDs-as-SoT contract established in the spec-repo plan
[`2026-04-22-001-feat-requirement-id-frontmatter-plan.md`](https://github.com/brettdavies/agentnative/blob/dev/docs/plans/2026-04-22-001-feat-requirement-id-frontmatter-plan.md)
by vendoring `agentnative-spec`'s `principles/*.md` frontmatter and deriving `REQUIREMENTS` from it at build time,
replacing the hand-maintained `&'static [Requirement]` slice currently in `src/principles/registry.rs`. Adds:

- `scripts/sync-spec.sh` — mirrors the site-side script from
  [`agentnative-site/docs/plans/2026-04-23-001-feat-sync-spec-plan.md`](https://github.com/brettdavies/agentnative-site/blob/dev/docs/plans/2026-04-23-001-feat-sync-spec-plan.md),
  pulling `principles/*.md`, `VERSION`, and `CHANGELOG.md` from `agentnative-spec` into `src/principles/spec/`.
- `build.rs` at the crate root — parses vendored frontmatter, emits `OUT_DIR/generated_requirements.rs` consumed via
  `include!()` from `src/principles/registry.rs`. Fails the build loudly on parse errors, duplicate IDs, or schema
  drift.
- Drift-check tests — every check's `covers()` ID exists in the generated `REQUIREMENTS` set; every MUST/SHOULD/MAY in
  the vendored spec is addressable; vendored frontmatter schema matches what the Rust parser expects.
- Scorecard emits a `spec_version` field (from vendored `VERSION`) so consumers of `anc check --output json` can pin
  against the exact spec build the CLI was compiled against.

Tracked upstream as item 5 of
[agentnative-spec roadmap 002](https://github.com/brettdavies/agentnative/blob/dev/docs/plans/2026-04-22-002-post-frontmatter-roadmap.md).

The plan is **Standard** depth — real feature work, real risk (a build-time failure mode affects every developer
compile), cross-repo artifact dependency.

## Problem Frame

Today `src/principles/registry.rs` hand-maintains a 767-line `&'static [Requirement]` slice covering every
MUST/SHOULD/MAY across P1-P7 (`p1-must-env-var`, `p1-must-no-interactive`, …, `p7-may-auto-verbosity`). That table was
correct when this crate shipped, but the spec now owns those IDs as canonical frontmatter
(`agentnative:principles/p*-*.md`). Two copies of the same contract will drift — already a live risk, since the
spec bumped from 46 to N entries via the v0.2.0 frontmatter migration and any future add/rename in the spec requires a
coordinated hand-edit on this side.

The `sot_contract.md` doctrine (spec-repo session memory) settled the design: **IDs are the contract, versions are
decoupled per repo.** This crate vendors the spec at a pinned SHA/tag, parses the frontmatter at build time, and fails
loud on any mismatch between the vendored set and the checks that reference it. The v0.2.0 tag (commit `83bf0fd`) is the
first stable target; tag `v0.2.0` is already cut and propagated via `repository_dispatch`.

## Requirements Trace

- R1. This crate vendors `agentnative-spec/principles/*.md`, `VERSION`, and `CHANGELOG.md` at a pinned tag/SHA via a
  manually-run `scripts/sync-spec.sh`. Matches the site-repo pattern from its sibling plan.
- R2. `REQUIREMENTS` (and the `Requirement` struct's fields) are generated at build time from vendored frontmatter. No
  hand-maintained duplicate remains in `src/principles/registry.rs`.
- R3. Build fails loudly and specifically if vendored frontmatter:
- cannot be parsed as YAML,
- is missing required fields (`id`, `level`, `applicability`, `summary`),
- contains duplicate requirement IDs,
- uses an unknown `level` (only `must`/`should`/`may` accepted),
- uses an unknown `applicability` shape (must be `universal` or `{if: "<prose>"}`).
- R4. A `cargo test` target asserts every ID referenced by any check's `covers()` exists in the generated `REQUIREMENTS`
  set — catches orphan check IDs that don't map to any real requirement.
- R5. A `cargo test` target asserts every MUST in the vendored spec is referenced by at least one check OR is explicitly
  listed as "unverified at current scale" in an allowlist with rationale — catches spec-side additions that this crate
  silently ignores.
- R6. `anc check --output json` emits a new `spec_version` field (value from vendored `VERSION`) as an additive v1.2
  scorecard field. v1.1 consumers feature-detect and tolerate its presence, per the existing additive-schema policy in
  `AGENTS.md`.
- R7. The coupled-release protocol in
  [`agentnative:CONTRIBUTING.md`](https://github.com/brettdavies/agentnative/blob/main/CONTRIBUTING.md) is
  honored: this plan's execution is the "companion PR" that resolves Open Question (a) in spec plan 001 (vendoring
  pattern choice — now definitively commit-a-copy).

## Scope Boundaries

- No new checks — this plan doesn't add behavioral or source analysis coverage. Every existing check keeps its current
  `covers()` declarations.
- No change to the scoring algorithm, exit codes, or audit-profile suppression logic. The `SUPPRESSION_TABLE` in
  `src/principles/registry.rs` stays hand-maintained (it maps check IDs to audit profiles, not requirement IDs to
  anything).
- No CI automation to auto-sync when spec cuts a new release. Manual `sync-spec.sh` matches the site-side model and the
  existing `sync-coverage-matrix.sh` precedent.
- No change to the existing `coverage-matrix.json` artifact that the site vendors from this crate — `anc generate
  coverage-matrix` still runs against the generated `REQUIREMENTS` and produces the same output shape.
- No dependency on `serde_yaml`'s 0.9+ API surface beyond basic parsing — keep the `build.rs` yaml handling narrow.

### Deferred to Follow-Up Work

- CI enforcement that `sync-spec.sh` was run at the latest spec tag (drift-detection in this crate's own CI): only if
  manual resync cadence proves insufficient. Revisit if two consecutive releases ship with a stale vendored spec.
- Exposing `spec_version` in the human-readable `--output text` scorecard footer: optional polish; scorecard footer
  currently doesn't cite anything, so adding it is a small UX decision for later.
- Migrating the `SUPPRESSION_TABLE` to also vendor from spec frontmatter: orthogonal; suppression is CLI-side policy
  about which requirements an audit-profile masks, not a contract the spec owns. Stays hand-maintained.

---

## Context & Research

### Relevant Code and Patterns

- `src/principles/registry.rs` — the 767-line file that this plan largely demolishes. Current shape:
- `pub struct Requirement { id, principle, level, summary, applicability }` (lines 241-247)
- `pub static REQUIREMENTS: &[Requirement] = &[...];` (lines 251-573)
- `pub fn find(id: &str) -> Option<&'static Requirement>` (line 584)
- `pub fn count_at_level(level: Level) -> usize` (line 590)
- `src/principles/mod.rs` — re-exports `REQUIREMENTS`, `Requirement`, `Level`, `Applicability`, `ExceptionCategory`.
  These re-exports must continue to work after the refactor.
- `src/principles/registry.rs` unit tests at the tail — pattern for "table-references-known-IDs" cross-check
  (`suppression_table_check_ids_exist_in_catalog`). Model the drift tests on this same shape.
- `src/principles/matrix.rs` — reads `REQUIREMENTS` to generate `coverage/matrix.json`. Must continue to work; the
  generated `REQUIREMENTS` must be a drop-in replacement.
- `agentnative:principles/p1-non-interactive-by-default.md` — canonical frontmatter shape:

  ```yaml
  ---
  id: p1
  title: Non-Interactive by Default
  last-revised: 2026-04-22
  status: draft
  requirements:
    - id: p1-must-env-var
      level: must
      applicability: universal
      summary: "..."
    - id: p1-must-no-browser
      level: must
      applicability:
        if: CLI authenticates against a remote service
      summary: "..."
  ---
  ```

-
  [`agentnative:scripts/validate-principles.mjs`](https://github.com/brettdavies/agentnative/blob/main/scripts/validate-principles.mjs)
  — source-side schema validator. The `build.rs` YAML parser must accept everything this validator accepts (no stricter,
  no looser). The validator is the authoritative schema.

### Institutional Learnings

- `sot_contract.md` (spec-repo session memory) — hybrid propagation + IDs-as-SoT. Settled.
-
  [`cross-repo-artifact-consumption-static-sites-2026-04-21.md`](../../docs/solutions/best-practices/cross-repo-artifact-consumption-static-sites-2026-04-21.md)
  — commit-a-copy vs build-time fetch vs symlink. Commit-a-copy wins on network independence and reproducibility.
  Feature-detect new fields rather than version-gate.
-
  [`build-module-srp-dry-refactor-20260421.md`](../../docs/solutions/best-practices/build-module-srp-dry-refactor-20260421.md)
  — build-time code-generation patterns in this ecosystem; precedent for keeping generated code out of `src/` (prefer
  `OUT_DIR`).

### External References

- [Cargo build script docs](https://doc.rust-lang.org/cargo/reference/build-scripts.html) — `build.rs` must be at the
  crate root. Use `println!("cargo:rerun-if-changed=src/principles/spec/")` so the build re-runs only when vendored
  sources actually change.
- [`serde_yaml` crate](https://docs.rs/serde_yaml) — the YAML frontmatter parser. Last stable is 0.9.x; pin tightly
  (e.g., `=0.9.34`) since the spec repo validator (`validate-principles.mjs`) uses `yaml@2` on the JS side and any
  divergence between YAML parsers risks schema drift at the byte level. Note: `serde_yaml` was deprecated in late 2024;
  evaluate `saphyr` or `yaml-rust2` as alternatives during U3 if the dep-deny config flags it.

---

## Key Technical Decisions

- **`build.rs` at crate root, not a proc macro.** Build scripts are simpler, debuggable, and match Rust conventions for
  generated-from-data code. Proc macros would over-engineer a one-way data pipeline.
- **Emit `generated_requirements.rs` into `OUT_DIR`, `include!()` from `src/principles/registry.rs`.** Keeps generated
  code out of `src/` (no stale committed generated file, no merge conflicts, no git diff noise), follows Cargo idiom.
- **Vendor destination is `src/principles/spec/`, not `spec/` or `docs/spec/`.** Keeps vendored content inside the
  published crate (`Cargo.toml`'s `exclude = [ "docs/", "scripts/", "tests/" ]` already excludes non-`src/` paths).
  Vendoring under `src/` means the spec ships to crates.io with this crate, which matches the contract this crate claims
  to implement. Licensing is fine: spec is CC BY 4.0; vendoring into an MIT/Apache-dual-licensed crate requires
  attribution only, which the vendored files' frontmatter + `src/principles/spec/README.md` provide.
- **Initial pin is v0.2.0 tag (commit `83bf0fd`).** Matches site-side pin from its sibling plan. First vendored state
  shipped is a real release, not a pre-release SHA.
- **Additive scorecard schema bump to v1.2.** `spec_version` is the only new field. v1.1 consumers tolerate its presence
  (feature detection is already the established policy per `AGENTS.md`).
- **`VERSION` file is vendored but not strictly required at build time.** The `spec_version` field reads from the
  vendored file; if missing, emit `null` in the scorecard (matches existing `audience_reason` pattern for unresolvable
  fields). Build succeeds without `VERSION` but emits a warning — errors only on malformed `principles/` content.
- **Drift-check tests live alongside the generated data.** New test file `tests/requirements_drift.rs` uses
  `agentnative::principles::REQUIREMENTS` and `agentnative::checks::all_checks_catalog()`. Failing these tests is a
  legitimate release-blocking signal; they run in the same `cargo test` invocation the pre-push hook already executes.

## Open Questions

### Resolved During Planning

- **Hand-maintained `Requirement` struct field for `principle: u8`.** Not in spec frontmatter (each file is a single
  principle; the `principle` number is derivable from the file's top-level `id: p1`). Resolution: `build.rs` computes
  `principle` from the file-level `id` field by parsing the numeric suffix. No manual mapping.
- **Ordering of generated `REQUIREMENTS`.** Current hand-maintained order is by principle then level (`MUST → SHOULD →
  MAY`). Resolution: generator sorts by `(principle, level_sort_key)` then preserves source-file order within a level,
  matching current semantics. `level_sort_key`: MUST=0, SHOULD=1, MAY=2.
- **What if the spec adds a new `level` value (e.g., `recommended`)?** Resolution: build fails with a specific,
  actionable error. The spec is the SoT but new levels are a coordinated change requiring a CLI-side struct update (enum
  addition). Silent tolerance would mask intent.
- **Choice between `serde_yaml` and alternatives.** `serde_yaml` is deprecated but stable and widely depended-on. Pin
  tightly and accept the dep; if `cargo-deny` flags it, evaluate `saphyr` or `yaml-rust2` as follow-up. Not blocking.

### Deferred to Implementation

- **Whether to use a one-shot YAML parse or extract just the frontmatter block first.** The spec files are markdown with
  `---`-delimited frontmatter; `serde_yaml` doesn't handle markdown. Likely approach: split file on the second `---`,
  parse the block between the first and second. Confirm during U3.
- **Whether `build.rs` should also vendor the file-level `last-revised` date into a per-principle metadata struct.** Out
  of scope for R1-R7 but trivially addable if we want it in the scorecard later. Decide once U3 is in place.
- **Exact error message copy for the build-time failures.** Draft during U3; pressure-test by deliberately corrupting
  the vendored files and checking that the error points to the offending file + field + line.

---

## Output Structure

```text
agentnative/  (this crate; name: agentnative, binary: anc)
├── build.rs                                          (NEW — U3)
├── Cargo.toml                                        (MODIFIED — U3 adds serde_yaml build-dep)
├── scripts/
│   └── sync-spec.sh                                  (NEW — U1)
├── src/
│   └── principles/
│       ├── mod.rs                                    (MODIFIED — U4 may re-export regenerated symbols)
│       ├── matrix.rs                                 (unchanged)
│       ├── registry.rs                               (MODIFIED — U4 demolishes hand-maintained REQUIREMENTS)
│       └── spec/                                     (NEW — U1/U2 vendored tree)
│           ├── README.md                             (NEW — attribution + resync pointer)
│           ├── VERSION
│           ├── CHANGELOG.md
│           └── principles/
│               ├── p1-non-interactive-by-default.md
│               ├── p2-structured-parseable-output.md
│               ├── p3-progressive-help-discovery.md
│               ├── p4-fail-fast-actionable-errors.md
│               ├── p5-safe-retries-mutation-boundaries.md
│               ├── p6-composable-predictable-command-structure.md
│               └── p7-bounded-high-signal-responses.md
└── tests/
    └── requirements_drift.rs                         (NEW — U5)
```

---

## High-Level Technical Design

> *This illustrates the intended approach and is directional guidance for review, not implementation specification.
> The implementing agent should treat it as context, not code to reproduce.*

**Data flow at build time:**

```text
agentnative-spec/principles/p*.md          (source of truth)
    │
    │  scripts/sync-spec.sh (manual, pinned to tag)
    ▼
src/principles/spec/principles/p*.md       (vendored, committed)
    │
    │  build.rs — read, split on `---`, serde_yaml::from_str, validate
    ▼
$OUT_DIR/generated_requirements.rs         (not committed; regen on source change)
    │
    │  include!()
    ▼
src/principles/registry.rs                 (tiny; just the include + struct defs)
    │
    ▼
pub static REQUIREMENTS: &[Requirement]    (consumed by checks, matrix, scorecard)
```

**Generated file shape (sketch, not implementation):**

```rust
// Auto-generated by build.rs from src/principles/spec/principles/*.md.
// Do not edit. Rerun `cargo build` or `./scripts/sync-spec.sh` to update.
pub static REQUIREMENTS: &[Requirement] = &[
    Requirement {
        id: "p1-must-env-var",
        principle: 1,
        level: Level::Must,
        summary: "...",
        applicability: Applicability::Universal,
    },
    // ... one entry per frontmatter requirement, sorted (principle, level, source-order)
];

pub const SPEC_VERSION: &str = "0.2.0";  // from src/principles/spec/VERSION
```

---

## Implementation Units

- [ ] U1. **Add `scripts/sync-spec.sh` and `src/principles/spec/README.md`**

**Goal:** Establish the vendoring mechanism. No code changes yet; purely file-plumbing.

**Requirements:** R1

**Dependencies:** None.

**Files:**

- Create: `scripts/sync-spec.sh`
- Create: `src/principles/spec/README.md`

**Approach:**

- Mirror the site-side
  [`scripts/sync-spec.sh`](https://github.com/brettdavies/agentnative-site/blob/dev/docs/plans/2026-04-23-001-feat-sync-spec-plan.md)
  U1 — same env var name `SPEC_ROOT`, same default `$HOME/dev/agentnative-spec`, same `set -euo pipefail`, same
  hard-error on missing source.
- Destination paths:
- `$SPEC_ROOT/VERSION` → `src/principles/spec/VERSION`
- `$SPEC_ROOT/CHANGELOG.md` → `src/principles/spec/CHANGELOG.md`
- `$SPEC_ROOT/principles/*.md` → `src/principles/spec/principles/`
- `src/principles/spec/README.md` contents: short (~15 lines). Identifies the folder as vendored from
  `brettdavies/agentnative`, cites the CC BY 4.0 license + attribution, points at `scripts/sync-spec.sh` for
  resync, and names the current pinned tag.
- Add `chmod +x` on creation.

**Patterns to follow:**

- Site-repo `scripts/sync-spec.sh` (when it lands) — voice, structure, error handling.
- `AGENTS.md` (this repo) existing voice for the README.

**Test scenarios:**

- Happy path: `./scripts/sync-spec.sh` against a checkout of spec at `v0.2.0` produces the expected tree under
  `src/principles/spec/`.
- Error path: unset `SPEC_ROOT` with no default path on disk → script exits with clear message.
- Error path: `$SPEC_ROOT/principles/` missing entirely → script errors citing the missing dir, not a cryptic "file not
  found".
- Edge case: stale orphan `.md` in `src/principles/spec/principles/` (from a prior resync after a spec rename) — accept;
  `git diff` surfaces at the next commit. Same trade-off as site sibling plan.

**Verification:**

- `shellcheck scripts/sync-spec.sh` passes.
- `./scripts/sync-spec.sh` run against `~/dev/agentnative-spec` at tag `v0.2.0` produces a non-empty vendored tree;
  `diff -r src/principles/spec/principles/ ~/dev/agentnative-spec/principles/` reports no differences.

---

- [ ] U2. **Initial vendored commit at v0.2.0**

**Goal:** Land the first vendored state as a committed artifact.

**Requirements:** R1

**Dependencies:** U1

**Files:**

- Create: `src/principles/spec/VERSION`
- Create: `src/principles/spec/CHANGELOG.md`
- Create: `src/principles/spec/principles/p{1..7}-*.md` (7 files)

**Approach:**

- Run `./scripts/sync-spec.sh` with spec repo checked out at `v0.2.0`.
- Commit the resulting tree. Commit message: `feat: vendor agentnative-spec v0.2.0 under src/principles/spec/
  (spec@83bf0fd)`.
- Ensure `Cargo.toml`'s `exclude` list does not yet exclude `src/principles/spec/` — verify before commit. (Current list
  excludes `docs/`, `scripts/`, `tests/`, etc., but not subpaths of `src/`. Confirm.)
- Keep `src/principles/registry.rs` unchanged in this commit — the generator doesn't exist yet, so both the vendored
  files and the hand-maintained `REQUIREMENTS` coexist briefly. U3 is the "switchover" unit.

**Patterns to follow:**

- Commit message citation format from spec-repo plan
  [`2026-04-22-003-release-infra-and-v0.2.0-cut-plan.md`](https://github.com/brettdavies/agentnative/blob/dev/docs/plans/2026-04-22-003-release-infra-and-v0.2.0-cut-plan.md)
  U2 — tag + short SHA.

**Test scenarios:**

- Happy path: crate builds and tests pass unchanged after the vendored commit (nothing references the new files yet).
- Edge case: verify `cargo publish --dry-run` still works with the new `src/principles/spec/` tree present — no
  unexpected includes or licensing warnings.
- Test expectation: `cargo test` still passes (no behavior change yet).

**Verification:**

- `cargo build --release` succeeds.
- `cargo test` passes.
- `cargo publish --dry-run` shows the vendored tree in the packaged files list without errors.

---

- [ ] U3. **Add `build.rs`; generate `REQUIREMENTS` from vendored frontmatter**

**Execution note:** Test-first for the generator. Before writing `build.rs` logic, craft a handful of fixture
inputs (valid, missing-field, duplicate-ID, unknown-level, unknown-applicability-shape) and assert the generator
produces the expected output or fails with the expected error. This is new build-time-failure surface; the diagnostic
quality is the feature.

**Goal:** Parse vendored frontmatter and emit `$OUT_DIR/generated_requirements.rs` at build time.

**Requirements:** R2, R3

**Dependencies:** U2

**Files:**

- Create: `build.rs` (crate root)
- Modify: `Cargo.toml` — add `[build-dependencies]` section with `serde = { version = "1.0", features = ["derive"] }`
  and `serde_yaml = "=0.9.34"` (pin tightly).
- Create: `tests/build_fixtures/` for U3's test-first fixtures (colocated with their test driver).

**Approach:**

- `build.rs` steps:

1. `println!("cargo:rerun-if-changed=src/principles/spec/");` — rebuild only when vendored content changes.
2. For each file matching `src/principles/spec/principles/p*-*.md`: a. Read to string. b. Split on the second `---`;
     YAML-parse the block between the first and second. c. Validate: required fields present (`id`, `title`,
     `requirements`); each requirement has `id`, `level`, `applicability`, `summary`; `level` ∈ {`must`, `should`,
     `may`}; `applicability` is either the string `universal` or an object with an `if` key whose value is a non-empty
     string. d. Compute `principle: u8` from the file-level `id` (strip `p` prefix, parse integer).
3. Aggregate all requirements across all 7 files. Detect duplicate IDs (hard error with file + ID).
4. Sort by `(principle, level_sort_key, source_order)`.
5. Read `src/principles/spec/VERSION` → emit `pub const SPEC_VERSION: &str = "..."`. If file missing, emit `"unknown"`
     and print a `cargo:warning=...`.
6. Write generated Rust to `$OUT_DIR/generated_requirements.rs`.

- Keep the generator in a single `build.rs` file. No separate `build/` module; if the logic grows past ~200 lines,
  revisit splitting into `build/parser.rs`, `build/emit.rs`, etc. — but start simple.
- Error messages are a feature: every failure cites the offending file path, field name, and (if possible) line number
  from `serde_yaml`'s error.

**Patterns to follow:**

- `src/principles/registry.rs`'s existing `assert!` / `panic!` error-message style in the unit tests — cite the
  offending construct and suggest the likely fix.
- Existing `Cargo.toml` dependency pinning style (`= "<version>"` for pre-1.0 deps).

**Test scenarios (test-first, before writing `build.rs`):**

- Happy path: 7 valid fixture files produce the expected `REQUIREMENTS` array with correct IDs, levels, applicabilities,
  and sort order.
- Edge case: single file with a `may` requirement marked `applicability: { if: "..." }` → parses to
  `Applicability::Conditional("...")`.
- Edge case: multiple MUSTs in one file with different `applicability` shapes → all parse; sort order preserved per
  source-order within level.
- Error path: duplicate ID across two files → build fails citing both file paths and the ID.
- Error path: missing `summary` field → build fails citing file + requirement ID + missing field.
- Error path: `level: recommended` (unknown) → build fails citing file + ID + allowed values.
- Error path: `applicability: { unless: "..." }` (unknown shape) → build fails citing file + ID + allowed shapes.
- Error path: frontmatter unterminated (no second `---`) → build fails citing file + "frontmatter not terminated".
- Integration: running `cargo build` with the real vendored v0.2.0 content produces a `REQUIREMENTS` array identical in
  content to the current hand-maintained one (modulo sort stability — verify by test in U5).

**Verification:**

- `cargo build` succeeds with real vendored v0.2.0.
- `cat $OUT_DIR/build/agentnative-*/out/generated_requirements.rs` (or wherever Cargo emits it) shows a well-formed Rust
  file.
- Deliberately corrupting one vendored file (e.g., renaming `summary` to `summry`) causes `cargo build` to fail with an
  actionable error pointing at the file + field.

---

- [ ] U4. **Replace hand-maintained `REQUIREMENTS` with generated include**

**Goal:** Cut over to the build.rs-emitted slice. Demolish the 320-line hand-maintained `REQUIREMENTS` static.

**Requirements:** R2

**Dependencies:** U3

**Files:**

- Modify: `src/principles/registry.rs` — replace the hand-maintained `REQUIREMENTS` with
  `include!(concat!(env!("OUT_DIR"), "/generated_requirements.rs"));`. Keep the `Requirement` struct, `Level` and
  `Applicability` enums, `ExceptionCategory`, `SUPPRESSION_TABLE`, `SUPPRESSION_EVIDENCE_PREFIX`, `find()`, and
  `count_at_level()` — all unchanged.
- Modify: `src/principles/mod.rs` — re-exports unchanged; confirm no symbols need renaming.

**Approach:**

- The hand-maintained `REQUIREMENTS = &[ ... ]` block (lines 251-573) is replaced with a single `include!()`.
- The `include!()` file must produce `pub static REQUIREMENTS: &[Requirement] = &[...];` directly at module scope.
- Verify that `src/principles/matrix.rs` and every consumer of `REQUIREMENTS` (`src/checks/`, `src/scorecard/`) still
  compiles without changes — the generated slice's type and element shape are identical.
- Run the full existing test suite (`cargo test`) after the cutover; any failure indicates a subtle mismatch (likely
  ordering, which U5 will further lock down).

**Patterns to follow:**

- Cargo idiom for including build-script-generated code — `include!(concat!(env!("OUT_DIR"), "..."))` at module scope.

**Test scenarios:**

- Happy path: `cargo test` passes unchanged (every existing test continues to pass).
- Integration: `anc check . --output json` against a known target produces identical output (byte-for-byte) to pre-U4.
  Run before and after U4 against a golden target (e.g., `ripgrep`) and diff the scorecards.
- Integration: `anc generate coverage-matrix` produces identical output to pre-U4.
- Edge case: running the pre-push hook's `cargo clippy -- -D warnings` produces no new warnings from the generated code
  (generated file should carry `#[allow(...)]` attributes if needed for clippy lints that trip on auto-generated
  patterns — decide case-by-case in U3/U4).

**Verification:**

- `cargo build --release` succeeds.
- `cargo test` passes (full existing suite).
- `diff <(anc check ~/dev/ripgrep --output json) <previous-known-good>` is empty (or the only difference is the new
  `spec_version` field added by U6, not U4 — if U6 hasn't run yet, it's empty).
- `cargo clippy --all-targets -- -D warnings` clean.
- Line count of `src/principles/registry.rs` drops by ~320 lines (the hand-maintained `REQUIREMENTS` block).

---

- [ ] U5. **Drift-check tests**

**Goal:** Lock in the invariants R4 and R5 so any future change to checks or spec surfaces the mismatch immediately.

**Requirements:** R4, R5

**Dependencies:** U4

**Files:**

- Create: `tests/requirements_drift.rs`

**Approach:**

- Test 1 (R4): every `Check::covers()` ID exists in `REQUIREMENTS`. Iterate `all_checks_catalog()`, collect every
  covered ID, `HashSet::difference` against `REQUIREMENTS` IDs. Non-empty diff = failure citing the orphan IDs.
- Test 2 (R5): every MUST in `REQUIREMENTS` is covered by at least one check OR is listed in an explicit allowlist.
- Allowlist lives in the test file as `const UNVERIFIED_MUSTS: &[&str] = &[...]` with a `why:` comment per entry.
    Allowlist entries are the "MUST covered by the spec but intentionally not automated at current scale" cases (see
    [`agentnative:docs/decisions/p1-behavioral-must.md`](https://github.com/brettdavies/agentnative/blob/main/docs/decisions/p1-behavioral-must.md)
    for the precedent — TTY-driving-agent scenarios are in P1 but not PTY-probed).
- Every MUST not in the allowlist must be covered by at least one check.
- If a new MUST lands in the spec and no check references it AND it's not in the allowlist, this test fails loudly on
    next `cargo test`.
- Test 3 (schema sanity): `REQUIREMENTS` length > 0, every entry has non-empty `summary`, `principle` is in `1..=7`, no
  duplicate IDs (redundant with build.rs check but cheap insurance).
- Follow the "loud failure message cites the diagnostic next step" voice of the existing
  `suppression_table_check_ids_exist_in_catalog` test in `src/principles/registry.rs`.

**Patterns to follow:**

- `src/principles/registry.rs` `#[cfg(test)] mod tests` block at the file tail — voice, assert macros, diagnostic copy.
- Existing `tests/` integration test shape (`tests/*.rs` files are Cargo integration tests auto-picked-up).

**Test scenarios:**

- Happy path (Test 1): every current check's covered IDs resolve — `cargo test` green.
- Happy path (Test 2): every MUST is either covered or allowlisted — `cargo test` green.
- Induced failure (Test 1): add a fake `covers()` call with ID `"nonexistent-id"` temporarily → test fails with
  actionable message. Revert.
- Induced failure (Test 2): remove a check's coverage for a real MUST without adding the MUST to the allowlist → test
  fails. Revert.
- Induced failure (Test 3): hand-edit a generated `Requirement` to have empty `summary` via a deliberate build.rs bug
  (skip validation for this test only) → test fails.
- Edge case: the allowlist itself — a MUST can be allowlisted only with a `why:` comment; linter in the test (or a
  separate test) verifies every allowlist entry has documentation. If YAGNI, skip.

**Verification:**

- `cargo test --test requirements_drift` passes.
- Deliberately inducing each failure mode produces a clear, actionable error message naming the offending ID and the
  fix.
- `cargo test` full run remains clean.

---

- [ ] U6. **Emit `spec_version` in scorecard JSON**

**Goal:** Additive v1.2 scorecard field — `anc check --output json` includes `spec_version` sourced from vendored
`VERSION`.

**Requirements:** R6

**Dependencies:** U4

**Files:**

- Modify: `src/scorecard/` (exact file depends on current layout — likely `src/scorecard/mod.rs` or
  `src/scorecard/json.rs`). Add `spec_version: Option<&'static str>` (or `String`) to the scorecard struct and its serde
  serialization.
- Modify: `src/principles/registry.rs` or `src/principles/mod.rs` — re-export `SPEC_VERSION` generated by `build.rs` in
  U3.
- Modify: `AGENTS.md` — update the "Agent-facing JSON surface" section to document the new v1.2 field.
- Modify: test fixtures or snapshot tests (`insta` is already a dev-dep) that assert JSON scorecard shape. New snapshots
  accepted via `cargo insta review`.

**Approach:**

- Plumb `SPEC_VERSION` from the registry through to the scorecard struct.
- Bump the scorecard's `schema_version` from `"1.1"` to `"1.2"`. `AGENTS.md` already notes that fields are additive; the
  policy survives the bump.
- `spec_version` is an owned string or a `&'static str` depending on how the scorecard currently models string fields —
  prefer `&'static str` since the value comes from a `const` (fewer allocations, matches existing pattern for fixed
  metadata).
- If vendored `VERSION` was missing at build time, `SPEC_VERSION` = `"unknown"` — preserve that value in the scorecard
  rather than emitting `null`. `"unknown"` is still a v1.2 field; the shape is additive regardless.
- Update `AGENTS.md` "Agent-facing JSON surface" block:

  ```markdown
  As of schema_version 1.2, an additional field is present:

  - `spec_version` — the agentnative-spec version the CLI was built against (value from vendored `VERSION`, or
    `"unknown"` if the vendored file was missing at build time). v1.1 consumers tolerate its presence.
  ```

**Patterns to follow:**

- Existing `audience`, `audience_reason`, `audit_profile`, `coverage_summary` fields — all added in the same
  additive-schema style.
- `insta` snapshot test pattern already in the repo (if present).

**Test scenarios:**

- Happy path: `anc check <target> --output json` contains `"spec_version": "0.2.0"` (or whatever the vendored `VERSION`
  reads).
- Edge case: if the vendored `VERSION` file is missing (U3's fallback path), `spec_version` reads `"unknown"` in the
  JSON output.
- Integration: an existing v1.1-aware consumer that uses `schema_version` for feature gating still parses the v1.2
  output without error. Hard to automate — verification is "we're honoring the additive policy, and the field is
  nullable-or-string".
- Covers AE: N/A (no acceptance examples in origin roadmap; R6 is tracked directly).
- Test expectation: one new unit test asserting the JSON output includes `spec_version` as a non-empty string; one
  snapshot test updated to include the new field.

**Verification:**

- `cargo test` passes (including updated snapshots).
- `anc check --output json . | jaq '.spec_version'` outputs `"0.2.0"`.
- `AGENTS.md` lists the new field and the bumped `schema_version`.

---

## System-Wide Impact

- **Interaction graph:**
- `build.rs` (NEW) → vendored `src/principles/spec/` (NEW) → `$OUT_DIR/generated_requirements.rs` (NEW) →
    `src/principles/registry.rs` (MODIFIED) → every existing consumer (`src/checks/`, `src/principles/matrix.rs`,
    `src/scorecard/`).
- No change to the binary's runtime behavior in U1-U5 — the plan intentionally preserves byte-identical scorecard output
    until U6's additive field.
- **Error propagation:**
- Build-time: all failures are `cargo build` errors with file + field context. Never silent.
- Runtime: no new failure modes. The runtime reads a `&'static` slice exactly as before.
- **API surface parity:**
- Public API (`pub use` in `src/principles/mod.rs`) unchanged. Downstream consumers of the crate (if any) see no
    breaking change.
- Scorecard v1.2 is additive; v1.1 consumers compatible.
- **Unchanged invariants:**
- `Requirement`, `Level`, `Applicability`, `ExceptionCategory` type definitions — untouched.
- `SUPPRESSION_TABLE`, `SUPPRESSION_EVIDENCE_PREFIX`, `ALL_EXCEPTION_CATEGORIES` — hand-maintained, unchanged.
- `find()` and `count_at_level()` — unchanged; still operate on `REQUIREMENTS`.
- `coverage-matrix.json` output shape — identical (derives from `REQUIREMENTS`, and the generated version matches the
    hand-maintained one byte-for-byte post-U4).
- Exit codes, CLI grammar, `--audit-profile` semantics — all unchanged.
- **Integration coverage:** The spec repo's `scripts/validate-principles.mjs` and this crate's `build.rs` are now two
  parsers of the same YAML. U3 explicitly requires the two accept the same schema. If they diverge, the spec-side
  validator is authoritative; a test fixture that re-parses every real spec file on `cargo test` indirectly proves
  parity.

---

## Risks & Dependencies

| Risk                                                                                  | Likelihood | Impact | Mitigation                                                                                                                                                                                                                                                                          |
| ------------------------------------------------------------------------------------- | ---------- | ------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `serde_yaml` is deprecated; `cargo-deny` flags it                                     | Med        | Low    | Pin tight version; if flagged, evaluate `saphyr`/`yaml-rust2` in U3. Parser is isolated to `build.rs`.                                                                                                                                                                              |
| Build-time YAML parser diverges from spec-repo JS validator                           | Med        | Med    | U3 test-first covers every error shape; manual parity check against `agentnative:scripts/validate-principles.mjs` rules. Drift surfaces as a real spec file failing to parse on this side.                                                                                     |
| Generated sort order differs from hand-maintained order → `coverage-matrix.json` diff | Low        | Low    | U4 integration test verifies byte-for-byte scorecard identity on a golden target. Sort key is explicit and tested.                                                                                                                                                                  |
| Vendored spec licensing concern (CC BY 4.0 in MIT/Apache crate)                       | Low        | Low    | Compatible per CC BY 4.0 — attribution only. `src/principles/spec/README.md` carries the attribution; spec frontmatter already in each file. Precedent: site repo vendors identically.                                                                                              |
| Spec adds a `level` or `applicability` shape not in this crate's enums                | Low        | Med    | Build fails loudly; a coordinated CLI-side enum addition + resync is the remediation. Better than silent tolerance.                                                                                                                                                                 |
| `cargo publish` rejects vendored non-Rust content                                     | Low        | Low    | Dry-run in U2 verifies. Cargo includes all of `src/` by default; `Cargo.toml` exclude list doesn't touch `src/principles/spec/`.                                                                                                                                                    |
| Drift tests allowlist becomes a dumping ground                                        | Med        | Low    | Every allowlist entry requires a `why:` comment citing the decision record (like [`agentnative:docs/decisions/p1-behavioral-must.md`](https://github.com/brettdavies/agentnative/blob/main/docs/decisions/p1-behavioral-must.md)). Enforce via prose review, not tooling. |

**External dependencies:**

- `agentnative-spec` at tag `v0.2.0` (commit `83bf0fd`) checked out at `$SPEC_ROOT` during vendoring. No runtime
  dependency.
- `serde_yaml = "=0.9.34"` (build-dependency only; does not ship in the binary).

---

## Documentation / Operational Notes

- PR body's `## Changelog` section: "Added: vendored spec under `src/principles/spec/` and `scripts/sync-spec.sh`.
  Added: `spec_version` field in `--output json` scorecard (additive, schema_version 1.2). Changed: `REQUIREMENTS` now
  generated at build time from vendored spec frontmatter — no behavior change in scoring."
- When this plan lands, strike item 5 in
  [agentnative-spec roadmap 002](https://github.com/brettdavies/agentnative/blob/dev/docs/plans/2026-04-22-002-post-frontmatter-roadmap.md)
  (mark shipped with PR link + vendored spec version).
- Update `agentnative:CONTRIBUTING.md`'s coupled-release protocol note if any of U1-U6 surface a gap in the
  existing documentation.
- The `agent-native-cli` skill (`~/.claude/skills/agent-native-cli/`) includes `references/principles-deep-dive.md`;
  that doc is currently downstream of `agentnative:principles/`. This plan doesn't re-home it, but landing U4
  creates a natural moment to consider whether that skill's principles file should become a third consumer of the
  vendored spec (out of scope here; flag as a future-consideration item).
- Resync cadence guidance (to land in this repo's `AGENTS.md` under U1): "Rerun `scripts/sync-spec.sh` after every new
  `agentnative-spec` tag. The `repository_dispatch` from the spec's publish workflow is the trigger. If the dispatch is
  handled by a future GitHub Action that opens a resync PR, this script becomes the action's body."

## Sources & References

- **Parent roadmap (spec repo):**
  [`2026-04-22-002-post-frontmatter-roadmap.md`](https://github.com/brettdavies/agentnative/blob/dev/docs/plans/2026-04-22-002-post-frontmatter-roadmap.md),
  item 5
- **Parent plan (spec repo):**
  [`2026-04-22-001-feat-requirement-id-frontmatter-plan.md`](https://github.com/brettdavies/agentnative/blob/dev/docs/plans/2026-04-22-001-feat-requirement-id-frontmatter-plan.md)
  — Open Question (a) from that plan is resolved here (commit-a-copy chosen).
- **Sibling plan (site repo):**
  [`2026-04-23-001-feat-sync-spec-plan.md`](https://github.com/brettdavies/agentnative-site/blob/dev/docs/plans/2026-04-23-001-feat-sync-spec-plan.md)
  — sync-spec.sh pattern source.
- Relevant code (this repo):
- `src/principles/registry.rs` (767 lines — the demolition target)
- `src/principles/mod.rs` (re-exports)
- `src/principles/matrix.rs` (downstream consumer)
- `Cargo.toml` (build-dependencies + exclude list)
- `AGENTS.md` (agent-facing JSON surface documentation)
- Relevant code (spec repo):
- [`principles/p*-*.md`](https://github.com/brettdavies/agentnative/tree/main/principles) (canonical frontmatter)
- [`principles/AGENTS.md`](https://github.com/brettdavies/agentnative/blob/main/principles/AGENTS.md) (schema
    governance)
-
    [`scripts/validate-principles.mjs`](https://github.com/brettdavies/agentnative/blob/main/scripts/validate-principles.mjs)
    (authoritative schema validator)
- External: [Cargo build scripts](https://doc.rust-lang.org/cargo/reference/build-scripts.html),
  [`serde_yaml`](https://docs.rs/serde_yaml), [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/)
- Target pin: `agentnative-spec@v0.2.0` (commit `83bf0fd`)
