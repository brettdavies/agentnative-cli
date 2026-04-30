---
title: "feat: scorecard JSON Schema — derive, embed, expose, publish"
type: feat
status: active
date: 2026-04-30
---

# feat: scorecard JSON Schema — derive, embed, expose, publish

## Overview

Publish a JSON Schema for `anc check --output json` so consumers (the `agentnative-site` renderer, third-party
leaderboards, agent integrations, README badge tooling) can validate scorecards against a stable, versioned contract
instead of reverse-engineering the shape from sample output. The schema lives upstream in this repo (single source of
truth), is **derived from the existing `serde::Serialize` structs** in `src/scorecard/mod.rs` via the
[`schemars`](https://crates.io/crates/schemars) crate, **prebuilt at compile time** via `build.rs` and embedded into the
binary, and surfaced through a new `anc generate scorecard-schema` verb.

The repo holds **only the current schema version** in committed form (`schemas/scorecard.schema.json` — always the
latest); the `agentnative-site` repo archives past versions under versioned URLs
(`https://anc.dev/scorecard-v0.5.schema.json`, `scorecard-v0.6.schema.json`, etc.) for consumers pinning to specific
releases. The site's sync mechanism (a `sync-scorecard-schema.sh` script parallel to the existing `sync-spec.sh` and
`sync-coverage-matrix.sh`) pulls the file at site-build time and renames it per the canonical archive convention. That
cross-repo plumbing is documented here but executed in the site repo.

This plan is the deliverable for the kickoff session that produced it; **execution begins in a follow-up session** per
`## Implementation Units` below.

---

## Problem Frame

Today the scorecard's shape is documented in three places that drift independently:

1. The Rust struct in `src/scorecard/mod.rs` (the runtime authority).
2. CLAUDE.md's "Scorecard v0.5 Fields" prose section (operator-facing reference).
3. The integration test `tests/scorecard_schema_v05.rs` (drift guard, but only for keys that the test happens to
   enumerate).

Consumers outside this repo have no machine-readable contract. The site renderer infers fields by reading sample JSON;
an external leaderboard consuming `scorecards/*.json` has to handle the always-present-null contract by trial-and-error;
an agent told to "produce a scorecard" has no way to validate its output before posting.

A published JSON Schema closes that gap. Every consumer can validate against
`https://anc.dev/scorecard-v0.5.schema.json`, the contract is enforceable in CI, and downstream tooling becomes easier
to write because the shape is discoverable.

The build/distribution shape matters too. Three constraints from the kickoff:

- **Derive, don't hand-write.** Hand-written schemas drift from the Rust types within the first month. The Rust struct
  is the only durable authority — the schema must be a derived artifact.
- **Prebuilt and embedded.** The verb should not need internet, schemars at runtime, or any extra dependency surface
  beyond what the binary already carries. `build.rs` does the work; `include_str!` brings the prebuilt JSON into the
  binary; the verb prints or saves the embedded copy. This matches the existing `coverage/matrix.json` pattern's spirit
  (committed artifact + drift check) while shifting the actual generation from runtime to compile-time.
- **Rich human-readable text.** A schema without `description`s and `examples` is hostile to humans reading the file.
  The plan makes the description/example surface a first-class concern, not a post-hoc layer.

---

## Requirements

- R1. **`schemas/scorecard.schema.json`** is committed to the repo, contains the current schema version's full shape,
  and is regenerable via `anc generate scorecard-schema --output schemas/scorecard.schema.json`.
- R2. **Derived from Rust types.** Every key in the schema corresponds 1:1 to a field on `Scorecard` or its sub-structs
  in `src/scorecard/mod.rs`. No hand-written JSON paths. Drift between struct and schema is caught at CI by the
  integration test in R7.
- R3. **`build.rs` writes the schema to `$OUT_DIR/scorecard.schema.json`** and the binary `include_str!`s it. The verb's
  output is byte-identical to what `build.rs` produced. No runtime schemars invocation.
- R4. **`anc generate scorecard-schema` verb** with two operating modes:
- `--output -` (default) — write the embedded schema to stdout.
- `--output <path>` — write to a file (used by the committed-artifact regeneration step).
- `--check` — exit non-zero with a structured error if `<path>` (or `schemas/scorecard.schema.json` when no path given)
    disagrees with the embedded schema. Mirrors `anc generate coverage-matrix --check`.
- R5. **Rich descriptions** sourced from doc comments on the Rust types. `schemars`'s `derive(JsonSchema)` already
  surfaces doc comments as `description` fields; the existing struct doc comments carry most of what's needed and the
  remainder land via additions to those comments (not via overlay files).
- R6. **Examples** for the top-level `Scorecard` and a representative sub-struct or two (at minimum
  `BadgeInfo`/`TargetInfo`/`CoverageSummary`) via `#[schemars(example = path::to::fn)]` attributes that reference small
  Rust functions producing the example values. The example functions live alongside the struct definitions and are
  unit-testable (the test asserts the example values themselves serialize cleanly through the struct).
- R7. **Integration drift test** at `tests/scorecard_schema_drift.rs::generated_schema_matches_committed_artifact`
  spawns the binary, runs `anc generate scorecard-schema --check`, and asserts exit zero. CI fails when the embedded
  schema and `schemas/scorecard.schema.json` disagree.
- R8. **Round-trip validation test** at `tests/scorecard_schema_drift.rs::scorecards_validate_against_embedded_schema`
  runs `anc check tests/fixtures/perfect-rust --output json`, parses the output, and validates against the embedded
  schema using a JSON Schema validator (likely `jsonschema` crate, dev-dep only). Catches bugs where the schema is
  internally consistent but doesn't match what the binary actually emits.
- R9. **`$schema` and `$id`** at the schema root:
- `$schema: "https://json-schema.org/draft/2020-12/schema"` (current published draft).
- `$id: "https://anc.dev/scorecard-v{X.Y}.schema.json"` — the versioned URL the site archives at.
- R10. **`title`** at the schema root reads `"agentnative scorecard"` and `description` summarizes "JSON Schema for `anc
  check --output json` scorecards, schema version X.Y. Generated from Rust types in `src/scorecard/mod.rs`. See
  https://anc.dev/scorecard-schema for the published archive of past versions."

---

## Scope Boundaries

- **In scope:** schema generation in this repo, the new `anc generate scorecard-schema` verb, the committed
  `schemas/scorecard.schema.json` artifact, drift + round-trip integration tests, doc-comment additions to existing
  scorecard types, example-value functions for the top-level type and a small selection of sub-types.
- **Out of scope:** the site-side sync script (`sync-scorecard-schema.sh`) — that lives in `agentnative-site` and is
  filed as a sibling plan there, referencing this one. Same for the site's archive page
  (`https://anc.dev/scorecard-schema`) and any per-version landing page rendering.
- **Out of scope:** schema-version bump. This work is delivered against the current `schema_version: "0.5"`; the plan
  does not touch the scorecard's value contract (no field additions, no semantics changes).
- **Out of scope:** validation infrastructure for *consumers*. The schema is the artifact; how the site, leaderboards,
  or agents validate against it is their own implementation concern.
- **Out of scope:** retroactive validation of historical scorecards (`scorecards/*.json` already committed in the site
  repo). Once the schema ships, those would either pass (if shape was already conforming) or be regenerated. Not this
  plan's problem.

### Deferred to Follow-Up Work

- Multi-version support **inside the binary** (today's binary embeds only the current schema version). If consumers ever
  need `anc generate scorecard-schema --version 0.4`, that's a follow-up; the site's archive surface handles
  past-version retrieval today.
- A schema linter / style-guide (e.g., "every property has a description"). schemars + careful doc comments handle it
  without ceremony for v0.5; revisit if descriptions go missing on additions.
- Localized descriptions. English only at launch.

---

## Context & Research

### Existing artifact-lifecycle pattern (the `coverage/matrix.json` precedent)

The repo already ships one machine-readable artifact via the same workflow this plan extends:

- **`anc generate coverage-matrix [--check]`** emits `docs/coverage-matrix.md` (human) + `coverage/matrix.json`
  (machine, `schema_version: "1.0"`).
- Both files are committed; `--check` exits non-zero on drift.
- Integration test `test_generate_coverage_matrix_drift_check_passes_on_committed_artifacts` mirrors `--check` so CI
  catches drift from either side.
- Project CLAUDE.md § "Coverage Matrix Artifact Lifecycle" documents the contract.

The scorecard schema is the second artifact in this family. The plan deliberately mirrors structure and naming so
operators see one pattern, not two.

**Departure from precedent:** coverage-matrix generates *at runtime* from the registry static slice (cheap, no
schemars). The scorecard schema generates at *compile time* via `build.rs` + schemars + `include_str!`. The reason is
schemars's runtime dependency surface — it's a non-trivial dep with proc-macros and a large transitive graph — and
moving it to `[build-dependencies]` keeps it out of the runtime binary entirely.

### `schemars` crate

The Rust ecosystem's de-facto JSON Schema generator. Releases > 1.0 default to JSON Schema Draft 2020-12; earlier
releases defaulted to Draft 07. Version selection lands as a Cargo.toml pinned-tight choice during U2 (research U2
deliberately confirms the latest stable major + the draft default it ships with).

Relevant features:

- `derive(JsonSchema)` — generates a `JsonSchema` impl for any struct/enum.
- `schema_for!(T)` macro — produces a `RootSchema` containing the full schema for `T`.
- Doc comments → `description` fields automatically.
- `#[schemars(example = path::to::fn)]` — appends the function's return value to the field's `examples[]` array (or
  `example` singular on Draft 07; Draft 2020-12 uses plural).
- `#[schemars(rename = "...")]`, `#[schemars(skip)]`, etc. — fine-grained per-field control.
- `#[schemars(extend("key" = value))]` — arbitrary additions to the generated schema for fields where schemars's output
  isn't quite what we want.

### Rust scorecard types (the derive surface)

`src/scorecard/mod.rs` defines:

- `Scorecard` (top-level)
- `CheckResultView` (one per result row)
- `Summary`
- `CoverageSummary`, `LevelCounts`
- `ToolInfo`
- `AncInfo`
- `RunInfo`, `PlatformInfo`, `RunMetadata`
- `TargetInfo`
- `BadgeInfo`

All are `#[derive(Serialize)]`. Adding `JsonSchema` to the derive list is a one-line change per struct. The structs
already carry rich doc comments (the `## Scorecard v0.5 Fields` section in CLAUDE.md is largely a transcription of those
comments); the gaps are mostly enum value docs (`audience` discriminants, `audit_profile` discriminants, the
`results[].group/layer/confidence` enums) which need the schema-friendly enum derive.

### Cross-repo consumer (`agentnative-site`)

The site's existing precedent for vendoring upstream artifacts:

- **`sync-spec.sh`** — pulls `agentnative-spec` content into `src/principles/spec/`. Remote-first since PR #33.
- **`sync-coverage-matrix.sh`** — pulls `coverage/matrix.json` from this CLI repo into the site (path documented in
  central tracker; site-side fix tracked under TODO 014 per the launch tracker).

A new **`sync-scorecard-schema.sh`** in the site repo follows the same shape: clone or `git show` the CLI repo, extract
`schemas/scorecard.schema.json`, write it to `<site-root>/static/scorecard-v0.5.schema.json` (or wherever the site's
static-asset convention places it), and verify the embedded `$id` URL matches the destination. The file lands alongside
the binary archive at `https://anc.dev/scorecard-v0.5.schema.json`.

The **site-side plan** lives in `agentnative-site/docs/plans/` (filed by the site session that picks this up); this plan
documents the contract this repo emits, not the site's pull mechanism.

### Best-practice references

- **JSON Schema spec, Draft 2020-12** — https://json-schema.org/draft/2020-12/schema. The current published draft. Every
  modern validator (`ajv`, `jsonschema`, `python-jsonschema`, etc.) supports it.
- **`$id` and `$schema` mandates** — JSON Schema spec §8.2 recommends `$id` be the canonical retrieval URL; consumers
  use it for resolution and pinning. The plan honors this with `https://anc.dev/scorecard-v0.5.schema.json`.
- **Filename convention** — schemastore.org's catalog uses `<name>.json` (no `.schema.` infix) for current versions and
  versioned filenames in archive. Mixed practice elsewhere. Plan picks `<name>.schema.json` for the in-repo file
  (mirrors the schemastore.org versioned-archive convention) because it makes the file's role obvious in `ls schemas/`.

### Institutional learnings (`docs/solutions/`)

Searched via `qmd query "json schema generation rust"` and `qmd query "schemars compile-time"` during plan authoring. No
prior learnings cover this — the scorecard schema is a new ground for this codebase. Plan to capture a learning
post-implementation if any non-obvious schemars behavior bites (e.g., enum representation, `Option<T>` flattening,
`#[serde(skip_serializing_if)]` interaction with `JsonSchema`).

---

## Key Technical Decisions

- **`schemars` as the generator.** No alternative considered seriously. `okapi` and `apistos` are OpenAPI-flavored;
  `boon` and `jsonschema` are validators, not generators; hand-rolling is rejected by R2.
- **Generation at `build.rs`, not runtime.** Keeps schemars out of the runtime binary's dep graph. Trade-off: rebuild
  cost on schemars version bumps, vs. zero-cost startup + smaller binary forever after. Same trade chosen by the
  vendored-spec mechanism (compile-time codegen of `REQUIREMENTS`).
- **Single in-repo file, versioned site archive.** Brett's call. The CLI binary always knows about exactly one schema
  (the one it emits); the site holds the historical archive. Simpler in-repo lifecycle (no version-suffix proliferation
  in `schemas/`) and the consumer-facing surface (anc.dev) handles archival on its own terms.
- **Draft 2020-12 unless schemars's stable default differs.** schemars >= 1.0 defaults to 2020-12; if the latest stable
  lags, U2 either pins a 2020-12-emitting version or accepts the default with explicit rationale.
- **Descriptions via doc comments, examples via `#[schemars(example = fn)]`.** Single source of truth in Rust source.
  Overlay-merge is rejected as the default approach (drift risk, custom merge code) but documented as the escape hatch
  if schemars's coverage proves insufficient mid-implementation.
- **Round-trip validation in tests, not at runtime.** The binary trusts its own emission; tests spawn the binary and
  validate output against the embedded schema. Keeps the runtime hot-path free of validator code while still pinning the
  contract end-to-end.

---

## Open Questions

### Resolved during planning

- **Derive vs hand-write?** Derive (Brett's call).
- **Embed prebuilt vs generate at runtime?** Embed prebuilt (Brett's call). `build.rs` does the work.
- **Single-version vs multi-version in binary?** Single (Brett's call). Site archives.
- **Filename / `$id` URL convention?** Repo: `schemas/scorecard.schema.json` (always-current). Site: per-version
  archive, `$id` carries the canonical versioned URL the site publishes at. Brett's "follow best practices" deferred to
  plan.
- **Description / example mechanism?** Doc comments + `#[schemars(example = fn)]` attrs. Overlay-merge is the escape
  hatch, not the default. Brett's choice was "make upstream edits so the local generator works" — that's exactly what
  doc-comment-driven descriptions plus inline example fns mean.

### Deferred to implementation

- **Q1: Latest stable schemars version + JSON Schema draft default.** Resolved at U2 via `cargo search schemars` and
  inspection of the crate's `README` / `CHANGELOG`. Plan assumes 1.x with Draft 2020-12. If 0.x is still latest stable
  at implementation time, U2 documents the deviation and either pins a 0.x version that emits 2020-12 (via
  `SchemaSettings`) or accepts Draft 07 with a Risks-table entry covering consumer-validator compatibility.
- **Q2: schemars's handling of `&'static str` fields** (used in `Scorecard::schema_version`, `Scorecard::spec_version`,
  `BadgeInfo::convention_url`) and `Option<&'static str>`. These should serialize as `string` per JSON Schema, but the
  derive may treat lifetimes specially. Verify during U3 — if it produces something off, add `#[schemars(with =
  "String")]` on the field.
- **Q3: Enum representation for `audience`, `audit_profile`, and the per-result enums.** The runtime serializes these as
  plain strings (kebab-case for scorecard-level, snake_case for per-result). schemars's default for unit enums is
  `"enum": [...]`. Verify at U3 that the kebab/snake casing is preserved through the derive — if `#[serde(rename_all)]`
  doesn't propagate to schemars, add a parallel `#[schemars(rename_all)]`.
- **Q4: How many examples to ship?** Minimum: one fully-populated `Scorecard` example at the root. Stretch: per-mode
  examples (project, binary, command) for the `target` block, two `BadgeInfo` examples (eligible + ineligible). Plan
  treats Stretch as a U5 nice-to-have; U4 lands the minimum.
- **Q5: Drift-test runtime cost.** Spawning the binary in two integration tests is fine for `cargo test` but adds ~1s to
  CI. If that becomes a hot spot, refactor to a single test that does both `--check` and round-trip validation in one
  binary spawn. Defer measurement to post-implementation.
- **Q6: Site sync scripts plumbing.** Where does the site repo write `scorecard-v0.5.schema.json`? Decided in the
  site-side plan, not here. This plan's R9 only fixes the `$id` URL; the site honors that URL by writing to a path that
  resolves there.

---

## Implementation Units

- U1. **Plan-only — this document.**

  **Goal:** capture the design so a follow-up session can execute without re-deriving decisions.

  **Status:** `done` (this commit).

  **Files:** `docs/plans/2026-04-30-002-feat-scorecard-json-schema-plan.md` (added).

  **Verification:** plan committed direct to `dev` per the docs-only carve-out in global CLAUDE.md.

---

- U2. **Add `schemars` build-dep + version research.**

  **Goal:** pin the right schemars version and confirm the JSON Schema draft it emits.

  **Status:** `not-started`.

  **Requirements:** R2, Q1.

  **Dependencies:** None.

  **Files:**

- Modify: `Cargo.toml` — add `schemars` to `[build-dependencies]` (NOT `[dependencies]` — runtime stays clean). Pin
    tightly per repo convention for pre-1.0 ecosystem deps; if schemars is at >= 1.0 stable, use a `1.x` caret; if 0.x,
    exact-pin (`=0.X.Y`).
- Modify: `Cargo.lock` — regenerated by `cargo build`.

  **Approach:**

1. `cargo search schemars --limit 1` → confirms latest stable.
2. `cargo info schemars` (or web fetch the crate page) — confirms which JSON Schema draft the latest stable emits by
     default.
3. Add to `[build-dependencies]`. The `derive` feature is needed (we'll add `JsonSchema` derives in U3); set
     `default-features = true` unless something objectionable shows up.
4. `cargo build --release` — confirm it compiles cleanly. Smoke test only; no real generation yet.

  **Patterns to follow:** `Cargo.toml`'s `[build-dependencies]` block already pins `serde_yaml = "=0.9.34"` exactly;
  follow that style for any 0.x schemars version.

  **Test scenarios:** none in this unit — it's a dep add only. Build pass is the verification.

  **Verification:** `cargo build --release` succeeds; `cargo tree -i schemars` shows the dep at the expected version.

---

- U3. **Derive `JsonSchema` on every scorecard struct + enum.**

  **Goal:** make the existing scorecard types generate a coherent baseline schema with no hand-edits.

  **Status:** `not-started`.

  **Requirements:** R2, R5, Q2, Q3.

  **Dependencies:** U2.

  **Files:**

- Modify: `src/scorecard/mod.rs` — add `JsonSchema` to every `#[derive(Serialize)]` line (or to a separate
    `#[cfg_attr(feature = "schemars", derive(JsonSchema))]` if we want it gated; default is unconditional, since
    schemars is build-only and the derive macro just generates impl code at compile time).
- Modify: `src/types.rs` (or wherever `CheckGroup`, `CheckLayer`, `Confidence`, `CheckStatus` live) — add `JsonSchema`
    to those enums. Verify `#[serde(rename_all = "snake_case")]` is mirrored by `#[schemars(rename_all = "snake_case")]`
    if schemars doesn't propagate it.
- Modify: `src/scorecard/audience.rs` (or wherever the audience enum is defined) — same treatment.

  **Approach:**

1. Walk `src/scorecard/mod.rs` top-to-bottom. For each `#[derive(Serialize)]`, change to `#[derive(Serialize,
     JsonSchema)]`.
2. For each `#[serde(rename_all = "...")]`, add a parallel `#[schemars(rename_all = "...")]` (per Q3 — verify during a
     smoke build whether schemars picks up the serde attr automatically; recent versions do).
3. For each `#[serde(skip_serializing_if = "Option::is_none")]`, verify the schema output marks the field as not
     required — schemars typically does this when it sees `Option<T>` plus a serde `skip` attr; confirm via the
     generated schema.
4. Smoke test: write a one-shot `examples/print-schema.rs` that does `println!("{}",
     serde_json::to_string_pretty(&schema_for!(Scorecard)).unwrap())`. Run, eyeball the output, fix any obvious casing /
     lifetime / rename issues. Delete the example file at end of U3.

  **Patterns to follow:** existing `Serialize` placements in `src/scorecard/mod.rs` are the model.

  **Test scenarios:** none in this unit (the schema isn't yet wired through `build.rs` — that's U4). U3's
  verification is "the temporary `examples/print-schema.rs` produces a schema that *looks right* on visual
  inspection". Real tests land in U6.

  **Verification:**

- `cargo build --release` clean.
- Eyeball the generated schema for: (a) every field present, (b) correct casing on enum values, (c)
    `additionalProperties: false` where we want it (probably on every struct), (d) `description` populated from doc
    comments, (e) no surprise lifetime artifacts on `&'static str` fields.

---

- U4. **`build.rs` codegen + `include_str!` in the runtime.**

  **Goal:** prebuild the schema at compile time, embed in the binary.

  **Status:** `not-started`.

  **Requirements:** R3.

  **Dependencies:** U3.

  **Files:**

- Modify: `build.rs` — add a function (`emit_scorecard_schema()`) that calls `schema_for!(Scorecard)`, serializes via
    `serde_json::to_string_pretty`, writes to `$OUT_DIR/scorecard.schema.json`. Add the `cargo:rerun-if-changed`
    directive for `src/scorecard/mod.rs` (and any other file the schema depends on).
- Create / modify: `src/scorecard/schema.rs` (new submodule) — `pub const EMBEDDED_SCHEMA: &str =
    include_str!(concat!(env!("OUT_DIR"), "/scorecard.schema.json"));`. Submodule keeps the include at one well-known
    site and gives the verb a stable import path.

  **Approach:**

1. Sketch `emit_scorecard_schema()` in `build.rs`. Mirror the existing `emit_skill_hosts()` and `emit_requirements()`
     patterns — both already write to `$OUT_DIR`.
2. Drop the temporary `examples/print-schema.rs` from U3 — its job is now `build.rs`'s.
3. Wire up the `EMBEDDED_SCHEMA` constant in `src/scorecard/schema.rs`. Re-export it from `src/scorecard/mod.rs` so
     external call sites only need `use crate::scorecard::EMBEDDED_SCHEMA;`.
4. `cargo build --release` — verify the file appears in `target/release/build/agentnative-*/out/`.

  **Patterns to follow:** `build.rs`'s `emit_skill_hosts()` (writes `$OUT_DIR/generated_hosts.rs`) and
  `emit_requirements()` (writes `$OUT_DIR/generated_requirements.rs`). Same basic shape: open file, write content,
  done. The new function writes JSON instead of Rust, but the I/O surface is identical.

  **Test scenarios:**

- **Happy path:** `cargo build --release` succeeds; the generated JSON file exists in `$OUT_DIR`; binary embeds via
    `include_str!`; a quick `cargo run --release -- check . --output json` still works (regression smoke for accidental
    compile-time breakage).
- **Edge case:** schema generation panics during `build.rs` (e.g. a struct field schemars can't handle). Mitigation:
    fail the build with a clear message naming the struct + field; do not silently emit a stub.

  **Verification:**

- `head $(find target -name 'scorecard.schema.json' | head -1)` shows valid JSON starting with `"$schema"` and `"$id"`
    keys.
- `EMBEDDED_SCHEMA.starts_with("{")` returns true at runtime (informal check; real test in U6).

---

- U5. **`anc generate scorecard-schema` CLI verb.**

  **Goal:** expose the embedded schema via the documented verb.

  **Status:** `not-started`.

  **Requirements:** R4.

  **Dependencies:** U4.

  **Files:**

- Modify: `src/cli.rs` — add `ScorecardSchema` variant under the existing `Generate` subcommand enum, with `--output
    <PATH>` and `--check` flags. Mirror the existing `coverage-matrix` arms exactly (same `--output -` / `--check`
    semantics).
- Modify: `src/main.rs` — dispatch the new arm. Reads `EMBEDDED_SCHEMA`; either prints to stdout, writes to file, or
    compares against file (per `--check`).
- Modify: `completions/` — regenerate (the `scripts/generate-completions.sh` step from RELEASES.md). The new subcommand
    needs to surface in shell completions.

  **Approach:**

1. Read `src/cli.rs`'s existing `coverage-matrix` arm. Copy structure, swap names.
2. Read `src/main.rs`'s dispatch on `Generate::CoverageMatrix` and copy the `--check` branch's diff logic. The diff
     logic for our case is just `embedded == file_contents` (pretty-printed JSON byte-for-byte equal); wrap in a
     friendly error message naming the file path and instructing to re-run without `--check`.
3. Regenerate completions; commit them.
4. Manual smoke: `anc generate scorecard-schema` prints to stdout; `anc generate scorecard-schema --output /tmp/x`
     writes a file; `anc generate scorecard-schema --check --output schemas/scorecard.schema.json` exits zero (after U7
     lands the committed file); flipping a byte in the committed file makes `--check` exit nonzero with a clear diff.

  **Patterns to follow:** `src/cli.rs`'s existing `Generate::CoverageMatrix` is the canonical model.

  **Test scenarios:**

- **Happy path stdout:** `anc generate scorecard-schema` prints the embedded JSON to stdout; exits zero.
- **Happy path file:** `anc generate scorecard-schema --output /tmp/test-schema.json` writes the file; content matches
    the embedded copy byte-for-byte.
- **Drift detected:** `anc generate scorecard-schema --check --output /tmp/wrong.json` (where `/tmp/wrong.json` has
    unrelated content) exits nonzero with a structured error envelope (per repo convention — text mode prints a message,
    JSON mode emits the standard error envelope).
- **No drift:** `anc generate scorecard-schema --check --output schemas/scorecard.schema.json` exits zero after U7's
    committed-file regeneration.

  **Verification:** all four scenarios above pass when run by hand. Real automation lives in U6.

---

- U6. **Drift + round-trip integration tests.**

  **Goal:** CI fails when schema and binary disagree.

  **Status:** `not-started`.

  **Requirements:** R7, R8.

  **Dependencies:** U5.

  **Files:**

- Create: `tests/scorecard_schema_drift.rs` — two integration tests as named in R7 / R8.
- Modify: `Cargo.toml` `[dev-dependencies]` — add a JSON Schema validator. `jsonschema` (the Rust crate, not the Python
    lib of the same name) is the leading choice; verify Draft 2020-12 support at U6 time. If `jsonschema` doesn't carry
    2020-12 yet, use `boon` (a newer entrant with explicit 2020-12 support).

  **Approach:**

1. **`generated_schema_matches_committed_artifact`** — spawns the binary with `assert_cmd::Command`, runs `anc generate
     scorecard-schema --check --output schemas/scorecard.schema.json`, asserts exit zero. If the committed file is
     stale, the test prints a clear "run `anc generate scorecard-schema --output schemas/scorecard.schema.json && git
     add schemas/`" hint.
2. **`scorecards_validate_against_embedded_schema`** — spawn the binary with `anc check tests/fixtures/perfect-rust
     --output json`. Parse stdout. Load the embedded schema from `schemas/scorecard.schema.json` (committed file is the
     test's source of truth). Validate the parsed scorecard against the schema. Assert validation passes with zero
     errors.
3. Repeat the round-trip test with `binary-only/test.sh` and `--command echo` modes — three flavors of the same test
     pattern, factored through a small helper.

  **Patterns to follow:** `tests/scorecard_schema_v05.rs` is the closest precedent — same `assert_cmd::Command` +
  `serde_json::from_str` shape.

  **Test scenarios:** the tests *are* the scenarios. CI green = drift caught at PR time.

  **Verification:**

- `cargo test --test scorecard_schema_drift` — both tests pass when the schema is freshly regenerated.
- Manual mutation: flip a byte in `schemas/scorecard.schema.json` → re-run → drift test fails with the regenerate hint;
    round-trip test still passes (the embedded schema in the binary, not the file, is what validates).
- Manual mutation: change a struct field's JSON name via `#[serde(rename = "...")]` without updating the schema →
    rebuild → drift test fails.

---

- U7. **Initial commit of `schemas/scorecard.schema.json`.**

  **Goal:** land the committed artifact for the first time.

  **Status:** `not-started`.

  **Requirements:** R1.

  **Dependencies:** U5 (the verb), U6 (drift test gate).

  **Files:** Create: `schemas/scorecard.schema.json` (committed, regenerable via the verb).

  **Approach:**

1. `cargo build --release`.
2. `./target/release/anc generate scorecard-schema --output schemas/scorecard.schema.json`.
3. `git add schemas/scorecard.schema.json`.
4. Commit on the feature branch as `feat(scorecard): commit initial scorecard.schema.json`.

  **Patterns to follow:** the very first commit of `coverage/matrix.json` (whichever PR introduced it; `git log
  --diff-filter=A coverage/matrix.json` finds it). The shape of that commit is the model.

  **Test scenarios:** `cargo test --test scorecard_schema_drift` passes after the file commits.

  **Verification:** committed file exists; CI drift test green.

---

- U8. **`anc generate --help` parity + AGENTS.md / README updates.**

  **Goal:** discoverability — readers landing on `anc --help` see the new verb and a one-line description; CLAUDE.md
- AGENTS.md mention the artifact.

  **Status:** `not-started`.

  **Requirements:** R10 (the schema's own `title`/`description`), AGENTS-side discovery.

  **Dependencies:** U5.

  **Files:**

- Modify: `src/cli.rs` — verify the clap `about` / `long_about` on the new arm reads cleanly. The verb's help is the
    only doc most readers will see.
- Modify: `AGENTS.md` — add a one-line entry under the existing `anc generate coverage-matrix` callout pointing at `anc
    generate scorecard-schema`.
- Modify: `README.md` — if there's a "Scorecard" or "JSON output" section, add a sentence linking to
    `https://anc.dev/scorecard-v0.5.schema.json`.
- Modify: `CLAUDE.md` § "Coverage Matrix Artifact Lifecycle" — rename to "Generated Artifact Lifecycle" and add a
    parallel paragraph for the scorecard schema. Keep the same shape (committed, drift-checked, regeneration is a
    deliberate commit).

  **Patterns to follow:** the existing AGENTS.md and CLAUDE.md sections describing `anc generate coverage-matrix`.

  **Test scenarios:** none beyond manual eyeball — these are docs.

  **Verification:** `anc generate scorecard-schema --help` renders cleanly; AGENTS.md and README.md grep cleanly for
  the new verb name.

---

- U9. **(Cross-repo) — file the site-side companion plan in `agentnative-site`.**

  **Goal:** trigger the site-side work to consume the new artifact (sync script, archive page, eventual validator
  tooling).

  **Status:** `not-started`.

  **Requirements:** Cross-repo coordination (R9 implies the site honors the `$id` URL).

  **Dependencies:** U7 (the artifact must exist at a stable path before the site syncs it).

  **Files:** None in this repo. The companion plan lives in `agentnative-site/docs/plans/<date>-feat-scorecard-schema-sync-plan.md`.

  **Approach:**

- File the site-side plan with `parent: <this plan's path>`. Site-side scope: `sync-scorecard-schema.sh`, archive page
    rendering, optional validator-as-CI for committed `scorecards/*.json`.

  **Verification:** site-side plan exists and references this one. Marked done in this plan when the site plan lands.

---

## System-Wide Impact

- **Interaction graph:** the new artifact joins `coverage/matrix.json` as the second cross-repo machine-readable
  contract this CLI publishes upstream. Two consumers (the site + any third-party validator that hits
  `https://anc.dev/scorecard-v0.5.schema.json`); zero new internal consumers.
- **Error propagation:** drift between schema and binary is caught at CI by U6 tests. There is no graceful-degrade path
  — broken schema = red build = fix before merge.
- **State lifecycle risks:** the `schemas/scorecard.schema.json` file sits between `git add` and binary embedding. If a
  developer forgets to regen + commit after a struct change, U6 catches it. If they regen the file but flip a field's
  `#[schemars(rename = "...")]` post-commit, the drift test catches it on the next CI run.
- **API surface parity:** the schema's `$id` URL is a public contract once published. Bumping the scorecard schema
  version means a new `$id` URL → a new committed file (post version bump) → a new site archive entry. The repo's
  always-current convention means `schemas/scorecard.schema.json` always matches the latest binary; consumers wanting a
  specific past version pin against the site's archive.
- **Integration coverage:** U6's round-trip test is the load-bearing integration. Without it, a schema can be internally
  consistent but say nothing about what the binary actually emits. With it, every CI run validates one real scorecard
  against one real schema end-to-end.
- **Unchanged invariants:** `schema_version`'s string value (`"0.5"`); the always-present-null contract on
  `tool.{version,binary}` / `target.{path,command}`; the kebab-case `audience` / `audit_profile` enum values; the
  snake-case `results[].group/layer/confidence` enum values. The schema *documents* these invariants; it does not change
  them.

---

## Risks & Dependencies

| Risk                                                                                                                                                                                | Mitigation                                                                                                                                                                                                                                                                             |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `schemars` produces a schema that's structurally correct but doesn't match what the binary emits (e.g., wrong casing on an enum, wrong handling of `#[serde(skip_serializing_if)]`) | U6's round-trip test catches this. Mitigation per discrepancy: add `#[schemars(...)]` attrs to mirror the serde attrs, or fall back to `#[schemars(extend(...))]` for the corner case.                                                                                                 |
| `schemars` 1.x defaults to a different draft than 2020-12 at the time of implementation                                                                                             | U2 explicitly verifies the default and either pins to a 2020-12-emitting version or accepts the default with a Risks-table entry on consumer-validator compat.                                                                                                                         |
| Build-time schemars version conflicts with another build-dep                                                                                                                        | Cargo's resolver handles this; pre-launch dep graph is small (`serde`, `serde_yaml` build-deps today). Low likelihood.                                                                                                                                                                 |
| The `$id` URL anchors at `anc.dev` but the site repo never wires up the corresponding archive endpoint                                                                              | U9 files the cross-repo plan that owns this. Until the site lands the archive, the `$id` URL 404s — a visible failure rather than a silent one. Document this as a transient state in the U9 plan.                                                                                     |
| Doc comments diverge from CLAUDE.md's "Scorecard v0.5 Fields" prose section                                                                                                         | Once the schema ships with rich descriptions, CLAUDE.md's prose becomes redundant for the *content* and is rewritten in U8 to point at the schema as authoritative. CLAUDE.md retains operator workflow notes (regeneration cadence, drift-check semantics) that aren't in the schema. |
| Drift test is flaky (e.g., due to JSON pretty-print whitespace differences across Rust versions)                                                                                    | Pin `serde_json` `to_string_pretty` indentation explicitly (default is 2 spaces; spell it out). Sort schema keys deterministically — schemars usually does, verify at U6.                                                                                                              |
| Round-trip test runtime cost exceeds CI budget                                                                                                                                      | Q5 — measure post-implementation, refactor to one binary spawn if needed.                                                                                                                                                                                                              |

---

## Documentation / Operational Notes

- **Regeneration cadence:** any change to a scorecard struct or sub-struct triggers schema regeneration. The verb +
  drift-test pattern catches the omission at PR time — no separate developer ceremony.
- **Schema-version bumps:** when scorecard `schema_version` increments (additive evolution per project policy), the next
  release ships a new `schemas/scorecard.schema.json` (in-repo file) and the site archives a new
  `scorecard-v{X.Y}.schema.json`. The committed file in the CLI repo is always the current version; past versions live
  only on the site.
- **Public surface URL:** `https://anc.dev/scorecard-v0.5.schema.json` is a published public URL once the site-side plan
  lands. Treat it as an API surface — breaking-change discipline applies (additive only on `0.x`, full lock at `1.0`).

---

## Sources & References

- **Coverage matrix artifact lifecycle** (the precedent this plan extends): `CLAUDE.md` § "Coverage Matrix Artifact
  Lifecycle"; `tests/integration.rs::test_generate_coverage_matrix_drift_check_passes_on_committed_artifacts`.
- **Scorecard v0.5 fields reference**: `CLAUDE.md` § "Scorecard v0.5 Fields".
- **Scorecard struct definitions**: `src/scorecard/mod.rs`; `src/scorecard/audience.rs`; `src/types.rs` (per-result
  enums).
- **Build-script codegen patterns**: `build.rs::emit_skill_hosts`, `build.rs::emit_requirements`.
- **JSON Schema Draft 2020-12 spec**: <https://json-schema.org/draft/2020-12/schema>.
- **`schemars` crate**: <https://crates.io/crates/schemars>; <https://docs.rs/schemars/>.
- **`jsonschema` crate (validator)**: <https://crates.io/crates/jsonschema>; alternative `boon`:
  <https://crates.io/crates/boon>.
- **PR #39** (the upstream basename PII fix that motivated this conversation, since the saved scorecard JSON in
  `.context/dogfood/` was the artifact under inspection when the schema-publishing question arose).
