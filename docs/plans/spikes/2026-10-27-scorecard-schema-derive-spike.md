---
title: "Spike: derive the scorecard JSON Schema from the Rust types, or keep it hand-written"
plan: docs/plans/2026-04-30-002-feat-scorecard-json-schema-plan.md
status: done
completed: 2026-10-27
date: 2026-10-27
---

# Spike: derive the scorecard JSON Schema from the Rust types, or keep it hand-written

## Question

`schema/scorecard.schema.json` (schema 0.9, `$id` `https://anc.dev/scorecard-v0.9.schema.json`) is written by hand, and
every bump from 0.5 to 0.9 edits it by hand. The plan that introduced it records "Derive vs hand-write? Derive". Why is
it hand-written, can `schemars` derive it from the serialized types, and what does the hand-written file carry that a
derived one would not?

## TL;DR

- **No recorded reason exists for hand-writing it.** The plan decides to derive. PR #54 ships the file as "the
  hand-written first cut" and names the plan as the follow-up. Nothing in the PRs, reviews, plan, or `docs/solutions`
  explains why the derive never followed. The likeliest reason (inference, labeled below) is that #54 closes two dogfood
  findings for the v0.4.0 release candidate and a hand-written file closes both.
- **Deriving works.** `schemars` 1.2.3 with `SchemaSettings::draft2020_12().for_serialize()` reproduces every property
  name, every `required` list (the always-present-null keys included), and every base type the hand-written schema
  declares. All nine real scorecards validate against the derived schema.
- **A bare derive is a much weaker contract.** It lacks 13 `additionalProperties: false`, 8 closed `enum` sets, 5
  `format`s, the `const`, `maximum`, `minItems`, `$id`, and every example, and it admits `null` on the 4 fields serde
  omits when `None`. Of 24 contract-breaking mutations, the hand-written schema rejects 23 and the bare derive
  rejects 4.
- **Attributes reach parity, at a price.** 60 `#[schemars(...)]` attributes (about 220 lines after rustfmt, 6.6 KB of
  description text moved into Rust), a 73-line example file, and a post-derive transform bring the derived document to
  semantic parity: 23 of 24 mutations rejected, no structural difference left. Four closed value sets still have to be
  written out by hand in the attributes, because the view stores those fields as `String`.
- **The hand-written structure has not drifted. Its prose has.** The root description says "schema version 0.7", and the
  `BadgeInfo` description gives a `pass / (pass + warn + fail)` formula and an 80 floor, neither of which the code uses
  (it is credit-weighted, floor 70). A test asserts the same stale floor and passes only because its fixture scores 0.
- **Recommendation: option (a).** Keep the hand-written file as the published contract and add the derive-backed drift
  test this spike built (one dev-dependency, 14 `cfg_attr` derive lines, one test). It catches a nested rename that
  every existing cargo test misses. Fix the stale prose and the 80-floor test in the same change.

## Findings: why it is hand-written

### The sources

| Source                                               | What it says                                                                                                                                                                                                |
| ---------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Plan `2026-04-30-002`, Problem Frame                 | "**Derive, don't hand-write.** Hand-written schemas drift from the Rust types within the first month. The Rust struct is the only durable authority" and "the schema must be a derived artifact."           |
| Plan, Open Questions                                 | "**Derive vs hand-write?** Derive (Brett's call)." Overlay-merge is "the escape hatch, not the default".                                                                                                    |
| Plan, U5 and U7 notes                                | `anc emit schema` "prints the hand-written `schema/scorecard.schema.json` embedded with `include_str!`". U2, U3, U4, and U6 (the derive, `build.rs`, and the drift tests) stay unchecked.                   |
| PR #54 body (2026-05-21)                             | "Hand-written coverage of the 0.5 shape ... Closes the `p2-should-schema-file` WARN." Related: "Schema follow-up plan ... (full schemars-derive implementation; this PR ships the hand-written first cut)." |
| PR #54 summary                                       | "Closes the dogfood gaps surfaced by `anc check .` at the v0.4.0 release candidate". Deployment notes cherry-pick the squash onto `release/v0.4.0`.                                                         |
| PR #54 reviews and comments                          | None. The PR has no reviews, no review comments, and no issue comments.                                                                                                                                     |
| PR #58                                               | Folds `anc schema` under `anc emit schema` and updates the schema's descriptions to say `anc audit`. It does not discuss derivation.                                                                        |
| PRs #62, #65, #83, #115, #147                        | Each edits the file in place for its bump (0.6, 0.7, 0.8, 0.9, descriptions). None discusses derivation.                                                                                                    |
| `docs/solutions`                                     | `qmd query "scorecard JSON schema derive schemars hand-written" --collection solutions` returns no document on the decision.                                                                                |
| A session handoff dated 2026-05-07, kept outside git | Hands the plan to the next session with the instruction to settle the verb shape first. It records that the owner asked to revert a hand-rolled `anc schema` MVP and "pick this work up properly later".    |

The May 2026 session transcripts are not on disk, so no primary source records the moment the hand-written cut was
chosen.

### Inference (not sourced to a statement)

My read: #54 is scoped to clear the v0.4.0 release candidate's dogfood findings (`p2-must-schema-print` FAIL,
`p2-should-schema-file` WARN). A committed file plus an `include_str!` verb clears both without the `build.rs` and
`schemars` work. The derive units were then never picked up, and each later bump was cheapest as an in-place edit.

### One plan step is not buildable as written

Plan R3 and U4 have `build.rs` call `schema_for!(Scorecard)`. Cargo compiles and runs a build script before the package
it belongs to, so `build.rs` cannot import `Scorecard`. Deriving at build time needs the types moved into a separate
crate that `build.rs` depends on. The workable alternatives are a test that writes the committed file (the route this
spike uses), or `schemars` as a normal dependency generating at runtime, which the plan rules out.

## Feasibility: what the derive needed

The spike adds `schemars = "1.2.3"` under `[dev-dependencies]` and `#[cfg_attr(test, derive(schemars::JsonSchema))]` on
`Scorecard`, `AuditResultView`, `Summary`, `CoverageSummary`, `LevelCounts`, `ToolInfo`, `AncInfo`, `RunInfo`,
`PlatformInfo`, `TargetInfo`, `BadgeInfo`, `ConfigHint`, `ConfigFile`, and `ConfigScope`. Nothing ships: the derive
compiles only for the library's own unit tests, and `cargo tree -e normal -i schemars` prints nothing.

| serde behavior                                                                                                                                         | What the derive does                                                                                                                                  | Attribute needed for parity                                                                                                                                                                                                 |
| ------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `rename_all = "kebab-case"` / `"snake_case"` / `"lowercase"` on enums                                                                                  | Honored                                                                                                                                               | None                                                                                                                                                                                                                        |
| `Option<T>` without `skip_serializing_if` (always-present-null: `tool.version`, `target.path`, `badge.embed_markdown`, ...)                            | `for_serialize()` puts the key in `required` and types it `[T, "null"]`                                                                               | None                                                                                                                                                                                                                        |
| `Option<T>` with `skip_serializing_if = "Option::is_none"` (`audience_reason`, `using_domain_verbs`, `domain_match_count`, `config_hint`)              | Leaves the key out of `required`, and still admits `null`, which serde never emits                                                                    | `with = "T"` on each of the 4 fields                                                                                                                                                                                        |
| `&'static str` (`schema_version`, `spec_version`, `anc.version`, `platform.*`, `convention_url`)                                                       | Plain `string`                                                                                                                                        | None                                                                                                                                                                                                                        |
| `String` fields holding a closed set (`group`, `layer`, `status`, `confidence`, `tier`, `audience`, `audience_reason`, `audit_profile`, `target.kind`) | Plain `string`, no `enum`                                                                                                                             | `with = "<Enum>"` where a Rust enum exists (`AuditLayer`, `Confidence`, `Level`, `ExceptionCategory`); `extend("enum" = [...])`, a hand-kept list, where none does (`status`, `audience`, `audience_reason`, `target.kind`) |
| `Mitigation` enum                                                                                                                                      | Never reaches the scorecard. `AuditResultView::from_row` flattens `Mitigation::DomainVerbs` into two optional row fields, so the derive never sees it | None                                                                                                                                                                                                                        |
| Integer widths                                                                                                                                         | Adds `format: "uint"`, `"uint32"`, `"uint64"`, which no JSON Schema vocabulary defines                                                                | A transform that strips them                                                                                                                                                                                                |
| No unknown keys                                                                                                                                        | Not expressed                                                                                                                                         | `deny_unknown_fields` on each of 13 structs                                                                                                                                                                                 |
| Doc comments                                                                                                                                           | Become `description`, verbatim, line breaks included                                                                                                  | `description = "..."` to override, or rewrite the doc comments                                                                                                                                                              |

Doc comments as descriptions carry text written for Rust readers: hard-wrapped `\n` inside paragraphs, version history
("Reserved in `anc` v0.1.1 / v0.1.2", "Pre-launch additive (schema `0.2`)"), and Rust vocabulary (`None`,
`Option::is_none`, rustdoc links such as ``[`ConfigHint`]``). One counterpoint: the `BadgeInfo` doc comment is current
while the hand-written description is stale, because the doc comment sits beside the code each PR changes.

## The diff

Both documents normalize into one keyword map per node (root, each property, each `$def`, each `$def` property), with
`type` and `required` sorted, `enum` compared as a set, a `oneOf` of `const` branches read as an `enum`, and
`anyOf: [$ref, null]` read as a nullable `$ref`. The script is `schema_diff.py` under
[Reproducing the spike](#reproducing-the-spike).

### Bare derive against the hand-written schema

`required` lists: identical on every object. Property names: identical on every object. Base types: identical except the
four rows under "Mismatches".

#### Content only the hand-written schema has

| Keyword                       | Count   | Where                                                                                                                                                                                                                  |
| ----------------------------- | ------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `$id`                         | 1       | Root: `https://anc.dev/scorecard-v0.9.schema.json`                                                                                                                                                                     |
| `examples` (document)         | 1       | Root: a full one-row scorecard                                                                                                                                                                                         |
| `additionalProperties: false` | 13      | Root, `AuditResultView`, `Summary`, `CoverageSummary`, `LevelCounts`, `ToolInfo`, `AncInfo`, `RunInfo`, `PlatformInfo`, `TargetInfo`, `BadgeInfo`, `ConfigHint`, `ConfigFile`                                          |
| `enum`                        | 8       | `audience` (3 labels and `null`), `audience_reason` (2), `audit_profile` (4 and `null`), `results[].layer` (3), `results[].status` (7), `results[].confidence` (3), `results[].tier` (3 and `null`), `target.kind` (3) |
| `examples` (field)            | 7       | `schema_version`, `spec_version`, `results[].group` (5 values), `anc.version`, `platform.os`, `platform.arch`, `run.invocation`                                                                                        |
| `format`                      | 5       | `run.started_at` (`date-time`); `badge.scorecard_url`, `badge.badge_url`, `badge.convention_url`, `config_hint.docs` (`uri`)                                                                                           |
| `const`                       | 1       | `badge.convention_url` = `https://anc.dev/badge`                                                                                                                                                                       |
| `maximum`                     | 1       | `badge.score_pct` <= 100                                                                                                                                                                                               |
| `minItems`                    | 1       | `config_hint.files` >= 1                                                                                                                                                                                               |
| `description`                 | 9 nodes | `results`, `schema_version`, `CoverageSummary`, and `results[].id`, `label`, `group`, `layer`, `status`, `evidence` (no doc comment on the Rust side)                                                                  |

Neither side has `if`/`then`, `dependentRequired`, or any other cross-field rule. The hand-written descriptions state
two cross-field facts in prose only ("`domain_match_count` present exactly when `using_domain_verbs` is",
"`audience_reason` omitted when `audience` has a value"). The one docs URL outside `$id` is `badge.convention_url`'s
`const`.

#### Content only the derived schema has

| Keyword                                           | Count   | Where                                                                                                               |
| ------------------------------------------------- | ------- | ------------------------------------------------------------------------------------------------------------------- |
| `format: uint` / `uint32` / `uint64`              | 13      | Every `Summary` and `LevelCounts` counter, `domain_match_count`, `score_pct`, `duration_ms`                         |
| `$defs/ConfigScope` with per-variant descriptions | 1       | The hand-written schema inlines `scope` as a bare `enum`; the derive adds a description to each of the three values |
| `description`                                     | 8 nodes | The `tool`, `anc`, `run`, `target`, `badge` properties, the `AuditResultView` object, `tool.binary`, `tool.version` |

#### Mismatches

| Path                           | Hand-written            | Derived                 | Which is right                                         |
| ------------------------------ | ----------------------- | ----------------------- | ------------------------------------------------------ |
| Root `title`                   | `agentnative scorecard` | `Scorecard`             | Hand-written (consumer-facing name)                    |
| `audience_reason`              | `string`                | `string` or `null`      | Hand-written: serde omits the key, never writes `null` |
| `results[].using_domain_verbs` | `boolean`               | `boolean` or `null`     | Hand-written, same reason                              |
| `results[].domain_match_count` | `integer`               | `integer` or `null`     | Hand-written, same reason                              |
| `results[].config_hint`        | `$ref ConfigHint`       | `ConfigHint` or `null`  | Hand-written, same reason                              |
| `ConfigFile.scope`             | inline `enum`           | `$ref` to `ConfigScope` | Equivalent (representation only)                       |
| `description` text             | 25 nodes differ         |                         | See below                                              |

None of the mismatches is a bug in the hand-written structure or in the serde attributes. The four nullable rows are a
`schemars` behavior: in serialize mode it drops a `skip_serializing_if` field from `required` but keeps `null` in its
type.

### Stale content in the hand-written schema

The structural diff cannot see these; reading the file against the code finds them.

| Location                   | Says                                                                                         | Code says                                                                                                         |
| -------------------------- | -------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| Root `description`         | "scorecards (schema version 0.7)"                                                            | `SCHEMA_VERSION = "0.9"`, and `$id` says 0.9                                                                      |
| `BadgeInfo.description`    | "`score_pct` is `pass / (pass + warn + fail)`", "`eligible` is `true` iff `score_pct >= 80`" | Credit-weighted, behavioral rows only (`score_pct` in `src/scorecard/mod.rs`); `BADGE_ELIGIBILITY_FLOOR_PCT = 70` |
| `results[].id` description | "Previously (≤0.5) this carried the probe id."                                               | Historical narration in a present-tense contract                                                                  |
| Root `examples[0]`         | `spec_version` and `anc.version` `0.4.0`; coverage totals 23 / 14 / 7                        | Live dogfood run: `0.6.0`; 28 / 22 / 10 (illustrative values, low severity)                                       |

A related test bug: `schema_v05_badge_eligibility_flag_matches_score` in `tests/scorecard_schema_v05.rs` asserts
`eligible == (score_pct >= 80)`. The floor is 70, so a fixture scoring 70 to 79 would fail it. It passes because
`perfect-rust` scores 0 in the test build (observed: `badge.score_pct` 0 for that fixture).

### Attributed derive against the hand-written schema

With the attributes in [option (b)](#options-and-costs), the structural diff leaves two rows, both representation:
`audit_profile` is an `anyOf` of a `oneOf` of `const`s and `null` instead of an `enum` with `null`, and
`ConfigFile.scope` and `ExceptionCategory` carry per-variant descriptions from their doc comments. Every description,
`examples`, `format`, `const`, `maximum`, `minItems`, `additionalProperties`, `title`, and `$id` matches.

## Validation

### Real scorecards

Nine scorecards from the debug build at `origin/dev`, run with `AGENTNATIVE_HOME_CONFIG` pointing at a missing file:

| Card | Invocation                                                           | Covers                                                               |
| ---- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| 1    | `anc audit . --output json`                                          | Project mode, 70 rows                                                |
| 2    | `anc audit . --audit-profile human-tui`                              | `audit_profile` set, `audience: null`, `audience_reason: suppressed` |
| 3    | `anc audit --command ls`                                             | Command mode, `n_a` rows                                             |
| 4    | `anc audit --command git --audit-profile posix-utility`              | Command mode with a profile, `fail` and `opt_out` rows               |
| 5    | `anc audit tests/fixtures/binary-only/test.sh`                       | Binary mode                                                          |
| 6    | `anc audit tests/fixtures/source-only`                               | Source-only, `audience_reason: insufficient_signal`, score 0         |
| 7    | `anc audit <shell CLI with one non-standard verb>`                   | A row with `config_hint`                                             |
| 8    | `anc audit tests/fixtures/perfect-rust`                              | The fixture the schema tests use                                     |
| 9    | Card 7's CLI with `--repo` naming `[p6] domain_verbs = ["mentions"]` | A row with `using_domain_verbs` and `domain_match_count`             |

`uvx check-jsonschema` 0.38.2 (format checking on by default) validates all nine against the hand-written, bare-derived,
and attributed-derived schemas, and `--check-metaschema` passes for all three documents.

### Mutations

Each mutation breaks one contract the runtime keeps, applied to card 7 or card 9 and validated with Python `jsonschema`
(Draft 2020-12, format checker on):

| Mutation                                                        | Hand-written | Bare derive | Attributed derive |
| --------------------------------------------------------------- | ------------ | ----------- | ----------------- |
| `status` outside the 7-value taxonomy                           | rejects      | accepts     | rejects           |
| `layer` outside its 3 values                                    | rejects      | accepts     | rejects           |
| `group` outside P1-P8, CodeQuality, ProjectStructure            | accepts      | accepts     | accepts           |
| `tier` outside must, should, may                                | rejects      | accepts     | rejects           |
| `confidence` outside high, medium, low                          | rejects      | accepts     | rejects           |
| `audience` outside its 3 labels                                 | rejects      | accepts     | rejects           |
| `audience_reason` outside its 2 reasons                         | rejects      | accepts     | rejects           |
| `audit_profile` outside the 4 categories                        | rejects      | accepts     | rejects           |
| `target.kind` outside project, binary, command                  | rejects      | accepts     | rejects           |
| Extra top-level key                                             | rejects      | accepts     | rejects           |
| Extra key on a result row                                       | rejects      | accepts     | rejects           |
| Extra key in `badge`                                            | rejects      | accepts     | rejects           |
| Always-present-null key dropped (`target.path`)                 | rejects      | rejects     | rejects           |
| Absent-when-None field written as `null` (`using_domain_verbs`) | rejects      | accepts     | rejects           |
| Absent-when-None field written as `null` (`config_hint`)        | rejects      | accepts     | rejects           |
| `config_hint.files` empty                                       | rejects      | accepts     | rejects           |
| `config_hint.docs` not a URI                                    | rejects      | accepts     | rejects           |
| Config file `scope` outside the closed set                      | rejects      | rejects     | rejects           |
| `badge.convention_url` changed                                  | rejects      | accepts     | rejects           |
| `badge.score_pct` above 100                                     | rejects      | accepts     | rejects           |
| `badge.scorecard_url` not a URI                                 | rejects      | accepts     | rejects           |
| `run.started_at` not a date-time                                | rejects      | accepts     | rejects           |
| `summary.pass` negative                                         | rejects      | rejects     | rejects           |
| `schema_version` a number                                       | rejects      | rejects     | rejects           |
| **Rejected**                                                    | **23 of 24** | **4 of 24** | **23 of 24**      |

`group` stays open on both sides: the hand-written schema lists examples only. Adding `with = "AuditGroup"` (and the
derive on `AuditGroup`) closes it, which is content only a derive offers for free.

### What the existing guards catch

The `rt_*` tests in `tests/scorecard_schema_v05.rs` check the `$id` version, the `status` enum, `Summary.required`,
`tier` and `audit_id`, the example against `required`, the live top-level keys against root `required`, and live row
keys against `AuditResultView.properties`. Nested objects (`summary`, `coverage_summary`, `tool`, `anc`, `run`,
`target`, `badge`, `config_hint`) are compared with the schema only by gate 3 of `scripts/release/smoke.sh`, at release
time, when `uvx` is on `PATH`.

Observed: renaming `LevelCounts.verified` with `#[serde(rename = "checked")]` leaves every existing test target green
(`cargo test --tests`, with `dogfood_no_row_fails_or_errors_on_this_repo` skipped for the reason below). Only the
spike's drift test fails, naming `$.coverage_summary.must.checked` and the `required` difference.

## Options and costs

Measured on the spike branches, rustc 1.96.0, `schemars` 1.2.3.

### Shared cost of any derive

- **Dependencies:** six crates enter `Cargo.lock` (`schemars`, `schemars_derive`, `serde_derive_internals`, `dyn-clone`,
  `ref-cast`, `ref-cast-impl`), 63 lockfile lines, all dev-only. `cargo tree -e normal -i schemars` prints nothing, so
  the binary is unchanged.
- **cargo deny:** `cargo deny check` (0.20.2) reports `advisories ok, bans ok, licenses ok, sources ok`.
- **Build time:** in a cold `cargo test --lib --no-run`, the six crates take 8.1 s of compile time between them
  (`schemars_derive` 3.2 s, `serde_derive_internals` 2.0 s, `schemars` 1.4 s), built in parallel with the rest of the
  graph. Wall-clock samples on the shared host were noise-dominated (12 to 41 s for the base, 28 to 38 s for the spike
  over three cold builds each), so no reliable wall-clock delta exists.
- **Source lines:** 14 `#[cfg_attr(test, derive(schemars::JsonSchema))]` lines and a `#[cfg(test)] mod` declaration.
- **Dogfood gotcha:** a test module whose `#[cfg(test)]` sits on the `mod` line in the parent file still gets audited as
  source by `code-unwrap`. One `.unwrap()` in the spike's test module fails
  `dogfood_no_row_fails_or_errors_on_this_repo`. A shipping version uses `expect` or an inner `#![cfg(test)]`.

### (a) Keep hand-written, add a derive-backed drift test

A unit test derives the schema with no attributes and compares, at every depth, property names, `required`, and the JSON
types each property admits. It drops `null` from both sides for keys outside `required`, since the derive admits it
there and the hand-written schema correctly does not. Built and run: a 157-line test module, the schema printer
included. Observed failures under mutation:

| Mutation                                            | Drift test output                                                                                                                       |
| --------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| `#[serde(rename = "pass_count")]` on `Summary.pass` | `$.summary.pass: declared by only the hand-written schema`, `$.summary.pass_count: declared by only the Rust types`, `required differs` |
| Schema drops `n_a` from `Summary.required`          | `$.summary: required differs`                                                                                                           |
| Schema retypes `score_pct` as `string`              | `$.badge.score_pct: types differ: hand {"string"}, serde {"integer"}`                                                                   |
| `skip_serializing_if` added to `TargetInfo.path`    | `$.target: required differs` (the existing `schema_v05_command_mode_emits_full_shape` also fails)                                       |
| `rename = "checked"` on `LevelCounts.verified`      | `$.coverage_summary.must.checked: declared by only the Rust types`, and two more lines (no other test fails)                            |
| Schema admits `null` on `using_domain_verbs`        | Passes: the stated limitation                                                                                                           |

Preserves: all hand-only content, since the published file stays the hand-written one. Does not catch: an enum value
added in code but not in the schema (the view's fields are `String`), and stale prose. Ongoing cost: each bump still
edits the JSON by hand, with the test naming every key, `required`, and type change to make.

### (b) Derive, with attributes and a transform

Built and run to parity. On top of the shared cost:

| Item                          | Size                                                                                                                                                                                                                                                             |
| ----------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `#[schemars(...)]` attributes | 60: 34 descriptions, 8 empty descriptions suppressing a doc comment, 13 `deny_unknown_fields`, 8 `with`, 7 `extend("examples")`, 5 `inline`, 4 `extend("enum")`, 4 `url`, 1 each of `extend("format")`, `extend("const")`, `range`, `length`, `title`, `example` |
| Lines after rustfmt           | `src/scorecard/mod.rs` +172, `src/types.rs` +45, `src/principles/registry.rs` +4                                                                                                                                                                                 |
| Text moved into Rust          | 6.6 KB of descriptions, beside the existing doc comments                                                                                                                                                                                                         |
| Root example                  | A 73-line JSON file read by `#[schemars(example = ...)]`                                                                                                                                                                                                         |
| Transform                     | Sets `$id` from `SCHEMA_VERSION`, strips the `uint` formats: about 25 lines                                                                                                                                                                                      |
| Shipping path                 | Not built: a test that rewrites the committed file on request, plus a byte comparison, since `build.rs` cannot see the types                                                                                                                                     |

Preserves: all hand-only content, and adds closed `enum`s sourced from Rust enums where they exist. The four
`extend("enum")` lists (`status`, `audience`, `audience_reason`, `target.kind`) stay a second source of truth until
those view fields become Rust enums. Each struct field gains a 3 to 8 line attribute block, and every field ends up with
two descriptions (rustdoc and schema) unless the doc comments are rewritten as consumer prose.

### (c) Derive the structure, merge a hand-kept overlay

Not built. The derive supplies names, `required`, and types; an overlay JSON keyed by path supplies descriptions,
examples, the closed sets, formats, `const`, `maximum`, `minItems`, `additionalProperties`, `title`, `$id`, and the root
example; a merge function joins them. Estimate from option (b)'s content: an overlay of about 250 lines (the
hand-written file is 313), a merge function of about 60 lines, and a test that fails when an overlay path is missing
from the derived document. Preserves all hand-only content and keeps the Rust types free of attributes. The plan already
weighs it: "Overlay-merge is rejected as the default approach (drift risk, custom merge code)".

### Summary

|                                | (a) Hand-written + drift test | (b) Attributed derive           | (c) Derive + overlay |
| ------------------------------ | ----------------------------- | ------------------------------- | -------------------- |
| Structure can drift from serde | No: test fails                | No                              | No                   |
| Closed sets can drift          | Yes                           | Only the 4 hand-kept lists      | Yes (overlay)        |
| Prose can go stale             | Yes                           | Yes (attributes)                | Yes (overlay)        |
| Hand-only content kept         | All                           | All                             | All                  |
| New source lines               | about 160                     | about 360, plus a shipping path | about 350 (estimate) |
| Edit per bump                  | JSON, guided by the test      | Attributes on the struct        | Overlay entry        |
| Rust types touched             | 14 derive lines               | Every serialized field          | 14 derive lines      |

## Recommendation

Option (a). The hand-written structure matches serde on every key, `required` list, and type after five bumps, and its
failures are prose, which no option prevents. The drift test closes the real gap (nested shape changes that reach
release smoke unseen) for a fraction of option (b)'s source cost, and it leaves the published document readable and
reviewable as one file. In the same change:

- Fix the root description's "0.7" and the `BadgeInfo` formula and floor, and drop the "Previously (≤0.5)" sentence.
- Make `schema_v05_badge_eligibility_flag_matches_score` compare against `BADGE_ELIGIBILITY_FLOOR_PCT`.
- Add a guard that the root description names `SCHEMA_VERSION`, the way `rt_schema_id_pins_to_published_schema_version`
  already guards `$id`.

Option (b) becomes cheaper if `AuditResultView`'s `status`, `audience`, `audience_reason`, and `target.kind` turn into
Rust enums: their closed sets would then derive too, and only descriptions and formats would stay as attributes.

## Decisions for the owner

1. **Which option.** (a), (b), or (c), and whether the plan's "Derive vs hand-write? Derive" record changes to match.
2. **The plan's remaining units.** U2 to U4 and U6 assume `build.rs` codegen, which needs the types in a separate crate.
   Mark them dropped or rewrite them around the chosen option.
3. **The stale prose and the 80-floor test.** Independent of the option; fixable now.
4. **Typed view fields.** Whether `AuditResultView`'s string fields become enums, which strengthens any option.
5. **The `$id` URLs.** `https://anc.dev/scorecard-v0.9.schema.json` and `https://anc.dev/web-scorecard-v0.5.schema.json`
   both return 404 today; the plan's site-side archive (U9) has not landed.

### `schema/web-scorecard.schema.json`

The web scorecard's Rust types in `src/web_audit/scorecard.rs` mirror the anc.dev engine's `WebScorecard`, so the site
owns that shape and the Rust mirror follows it. Its schema is hand-written too, and
`the_committed_schema_describes_exactly_what_the_mirror_serializes` in `tests/web_audit_score_parity.rs` holds it to the
mirror by key sets and enums ("rather than through a JSON Schema validator crate, the same approach
`tests/scorecard_schema_v05.rs` takes", per #100). Deriving it from the mirror would make the mirror the authority over
a shape the site defines. Its types already use `deny_unknown_fields` and `Deserialize`, so a drift test of the option
(a) kind would apply there with less normalization.

## Reproducing the spike

### Dev-dependency and derives

```toml
[dev-dependencies]
schemars = "1.2.3"
```

```rust
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct Summary { /* unchanged */ }
```

Repeat the `cfg_attr` line on the 14 types listed under [Feasibility](#feasibility-what-the-derive-needed), and declare
the test module with `#[cfg(test)] mod schema_derive;` in `src/scorecard/mod.rs`.

### Generating the schema

```rust
use schemars::generate::SchemaSettings;

fn derived() -> serde_json::Value {
    let schema = SchemaSettings::draft2020_12()
        .for_serialize()
        .into_generator()
        .into_root_schema_for::<super::Scorecard>();
    serde_json::to_value(schema).expect("schema is JSON")
}
```

`for_serialize()` is what makes the always-present-null keys required; the deserialize contract would leave every
`Option` out of `required`.

### Option (a)'s comparison, in outline

```rust
fn shape_diff(hr: &Value, h: &Value, dr: &Value, d: &Value, path: &str, out: &mut Vec<String>) {
    let (h, d) = (non_null(hr, h), non_null(dr, d)); // follow $ref, unwrap anyOf [X, null]
    // 1. property names: symmetric difference, each side named
    // 2. required: set equality
    // 3. per shared property: the JSON types admitted by type / enum / const / anyOf / oneOf,
    //    with "null" dropped when the key is not required
    // 4. recurse into each shared property and into array items
}
```

### Option (b)'s attributes, a representative sample

```rust
#[cfg_attr(test, schemars(deny_unknown_fields, description = "Agent-native badge metadata ..."))]
pub struct BadgeInfo {
    pub eligible: bool,
    #[cfg_attr(test, schemars(range(max = 100)))]
    pub score_pct: u32,
    #[cfg_attr(test, schemars(url, extend("const" = "https://anc.dev/badge")))]
    pub convention_url: &'static str,
    // ...
}

// AuditResultView
#[cfg_attr(test, schemars(with = "Option<crate::principles::registry::Level>"))]
pub tier: Option<String>,
#[cfg_attr(test, schemars(extend("enum" = ["pass", "warn", "fail", "opt_out", "n_a", "skip", "error"])))]
pub status: String,
#[serde(skip_serializing_if = "Option::is_none")]
#[cfg_attr(test, schemars(with = "bool"))]
pub using_domain_verbs: Option<bool>,
```

`AuditLayer`, `Confidence`, `Level`, `ExceptionCategory`, and `ConfigScope` take
`#[cfg_attr(test, derive(schemars::JsonSchema))]` and `#[cfg_attr(test, schemars(inline))]` so their values inline as an
`enum` instead of a `$ref`.

### Diff and validation

- `schema_diff.py HAND DERIVED` flattens each node into a keyword map and prints `hand-only`, `derived-only`,
  `mismatch`, and `text-differs` rows with per-keyword counts.
- `uvx check-jsonschema --schemafile <schema> <cards...>` validates the real scorecards;
  `uvx check-jsonschema --check-metaschema <schema>` checks each document.
- The mutation table comes from a script that applies each mutation to a copy of a card and validates it with
  `uv run --no-project --with 'jsonschema[format-nongpl]'` and
  `Draft202012Validator(schema, format_checker=FormatChecker())`.
