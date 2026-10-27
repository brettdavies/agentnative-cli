---
title: "feat: agentnative-spec output-envelope SHOULDs + matching audits"
type: feat
status: proposed
date: 2026-04-30
---

# feat: agentnative-spec output-envelope SHOULDs + matching audits

## Summary

Add two SHOULDs (one in P2, one in P4) that codify the parts of the output-envelope contract `anc` itself enforces and
no requirement covers, then land matching behavioral audits here so the linter verifies them. The registry grows by two
IDs, two audits land under `src/audits/behavioral/`, and the coverage matrix regenerates. `anc` must pass both audits
against itself (dogfood loop).

---

## Problem Frame

The `anc skill install` work shipped a uniform JSON envelope (success and error sharing one schema, typed `reason` on
error, `--output {text,json}` honored on every code path) and pinned dogfood guards via `tests/dogfood.rs`. The contract
is captured in prose at `docs/solutions/architecture-patterns/anc-cli-output-envelope-pattern-2026-04-29.md`. Per the
institutional learning *audit-scripts-as-documentation-immune-system* (`docs/solutions/best-practices/`), prose without
enforcement rots silently while the linter reports green.

Existing requirements and audits cover part of the contract. Five behavioral audits share one bad-flag probe
(`src/audits/behavioral/error_probe.rs`) under the JSON mode the help declares. Among them, `consistent_envelope.rs`
compares the success and error key sets for `p2-should-consistent-envelope`, `json_error_output.rs` checks that an
error parses as JSON for `p4-should-json-error-output`, and `json_errors.rs` checks the error envelope for a
discriminant, a type identifier, and a message for `p2-must-json-errors`. `p4-should-json-error-output` covers a JSON
envelope on the error path and `p2-should-consistent-envelope` covers one envelope schema across success and error, so
this plan adds neither. No requirement or audit covers `--output` on every subcommand or a typed `reason` field on
errors.

---

## Requirements

- R1. Add two SHOULDs covering output-envelope behavior:
- P2 `output-applies-to-every-subcommand` (Conditional: `if: CLI uses subcommands`).
- P4 `json-error-includes-typed-reason` (Universal).
- R2. Phrase each SHOULD behaviorally per *behavioral-vs-structural-must* — observable behavior under a stated
  condition, not enumerated subcommands or library lists.
- R3. The two IDs need requirement text in the vendored spec. `build.rs` generates `REQUIREMENTS` from
  `src/principles/spec/principles/`, `dangling_cover_ids` fails the build for a `covers()` ID the registry lacks, and
  `scripts/sync-spec.sh` vendors the spec from the latest `agentnative-spec` `v*` tag. Both SHOULDs therefore need text
  in `agentnative-spec` and a tag that carries it before either audit can merge here.
- R4. Re-vendor the spec into `src/principles/spec/`. Bump the two drift counters in
  `src/principles/registry.rs::tests`: `registry_size_matches_spec` (60 → 62) and `level_counts_match_spec` (Should: 22
  → 24).
- R5. Land two behavioral audits in `src/audits/behavioral/`, one per new SHOULD, each declaring its own single-element
  `covers()` slice. SRP-style — no bundled "envelope conformance" audit (per
  *reliable-static-analysis-compliance-auditors*).
- R6. Update `SUPPRESSION_TABLE` in `src/principles/registry.rs` for any audit that should be excluded under an existing
  audit-profile category. Specifically: `output-applies-to-every-subcommand` joins the empty `FileTraversal` slot
  reserved for "subcommand-structure SHOULDs" (`src/principles/registry.rs:196-201`). Verify the other one against
  each existing category and document the mapping decision.
- R7. Extend `tests/dogfood.rs` to pin both new IDs against the `anc` binary itself — `anc audit . --output json`
  must show no `fail` on any of the new audit IDs.
- R8. Regenerate `docs/coverage-matrix.md` and `coverage/matrix.json` via `anc emit coverage-matrix`. CI's existing
  `--check` integration test
  (`tests/integration.rs::test_generate_coverage_matrix_drift_check_passes_on_committed_artifacts`) will fail until both
  artifacts ship in the same PR.

---

## Scope Boundaries

- No new audit-profile categories. The committed surface stays at four (`human-tui`, `file-traversal`, `posix-utility`,
  `diagnostic-only`). New SHOULDs map into existing categories or stay unsuppressed (per CLAUDE.md: "Adding a fifth
  category requires a plan revision").
- No changes to the existing four SHOULDs already shipping in `feat/skill-install` (R-OUT, R-DRY, etc.). The new spec
  entries codify the patterns those requirements already implement; they do not retroactively edit them.
- No refactoring of existing P2/P4 audits beyond the minimum needed to land the new ones.
- No spec-version major or minor bump beyond what additive SHOULD additions require. The spec is at `0.x` and uses
  additive evolution; consumers feature-detect new requirement IDs.
- No subcommand-discovery infrastructure beyond what `output-applies-to-every-subcommand` needs.
  `HelpOutput::subcommands()` (`src/runner/help_probe/mod.rs`) is the existing primitive; reuse it.
- No envelope-pattern documentation in `docs/solutions/` — that's the parallel `/ce-compound` work in the same task
  queue, separate from this plan.
- No `coverage_summary` schema changes in the scorecard. New requirement IDs flow through automatically.
- No per-host or per-action SHOULDs. The new requirements apply universally (or with the one explicit conditional).

### Deferred to Follow-Up Work

- **Reconciliation of `p2-must-json-errors` ("on stderr") with the as-shipped convention (envelope on stdout).** The
  existing MUST says errors emit JSON to stderr; `anc`'s shipped `feat/skill-install` envelope goes to stdout (matching
  every other `--output json` surface). The MUST's audit accepts the envelope on either stream, as
  `p4-should-json-error-output` does. Reconciling the MUST text is a separate spec-edit PR. Not in scope because this
  plan adds two SHOULDs and rewrites no existing MUST.
- **Source-layer fallbacks for the two audits.** Behavioral probing covers the binary; an ast-grep source-layer
  companion (e.g., detecting `#[serde(skip_serializing_if)]` on envelope structs) would extend coverage to projects
  whose binaries fail to launch. Defer until usage shows behavioral audits miss real cases.

---

## Context & Research

### Relevant Code and Patterns

- `src/audit.rs` — `Audit` trait. New audits implement `id`, `label`, `group`, `layer`, `applicable`, `run`, and declare
  `covers()` returning a `&'static [&'static str]` of new requirement IDs.
- `src/audits/behavioral/json_output.rs` — canonical behavioral P2 audit shape. Subcommand enumeration via
  `HelpOutput::subcommands()` (`src/runner/help_probe/mod.rs`). Probes use `--help`/`--version` only (safe-probing
  rule). JSON parsing via `serde_json::from_str::<serde_json::Value>(...)`.
- `src/audits/behavioral/error_probe.rs`: the shared bad-flag probe (`BAD_FLAG`, `--this-flag-does-not-exist-anc`)
  and JSON-mode gate behind the five error-battery audits. The two new behavioral audits reuse it (bad-flag trigger
  under the declared JSON mode).
- `src/audits/behavioral/mod.rs` (lines 61-115) — `test_project_with_sh_script(script)` test helper that materializes a
  temp `/bin/sh` script with `case "$*" in ... esac` branches simulating CLI behavior. Use for unit tests.
- `src/principles/registry.rs` — drift counters at lines 337-345. ID format enforced by tests at lines 280-301.
  `SUPPRESSION_TABLE` shape at lines 174-225.
- `src/principles/matrix.rs` — `dangling_cover_ids()` (lines 330-340) fails the build when a `covers()` ID is missing
  from `REQUIREMENTS`. `UNVERIFIED_MUSTS` table (lines 469-486) — note the existing `p2-must-json-errors` and
  `p4-must-actionable-errors` entries; the new SHOULDs do not move those out (different requirement IDs).
- `src/principles/spec/principles/p2-structured-parseable-output.md` — target file for the P2 SHOULD.
- `src/principles/spec/principles/p4-fail-fast-actionable-errors.md` — target file for the P4 SHOULD.
- `tests/dogfood.rs` — extension point for dogfood guards. Existing tests pin `p2-*` and `p5-*` collectively; add new
  assertions naming the two new IDs explicitly so a future regression names the broken contract.
- `tests/integration.rs` (lines 66-75) — `test_generate_coverage_matrix_drift_check_passes_on_committed_artifacts`. Will
  fail CI if `docs/coverage-matrix.md` or `coverage/matrix.json` are stale at PR time.
- `scripts/sync-spec.sh` — vendor the new spec tag remote-first. Mirrors the `scripts/sync-skill-fixture.sh` shape.
- `agentnative-spec/principles/AGENTS.md` — pressure-test protocol for new requirements. Apply when drafting prose.

### Institutional Learnings

- **anc CLI output envelope pattern**
  (`docs/solutions/architecture-patterns/anc-cli-output-envelope-pattern-2026-04-29.md`) — names these SHOULDs and
  the two that `p4-should-json-error-output` and `p2-should-consistent-envelope` cover. The canonical statement of the
  convention; cite as the rationale source in spec-edit PR descriptions.
- **Consistent JSON schema across success and error paths**
  (`docs/solutions/best-practices/consistent-json-schema-across-success-and-error-paths-2026-04-20.md`) — the
  rationale `p2-should-consistent-envelope` carries. Provides a "every JSON path has these fields" contract-
  test pattern.
- **CLI structure for machines: typed JSON fields, not English in display strings**
  (`docs/solutions/best-practices/cli-structure-for-machines-typed-json-fields-over-display-strings-2026-04-20.md`) —
  rationale for the P4 typed-reason SHOULD. Suggests a grep heuristic for the audit
  (`rg -n '"[A-Z][a-z]+ [a-z]+ [a-z]+'` flags sentence-shaped JSON values).
- **Agent-native CLIs: semantic JSON fields, not stderr warnings**
  (`docs/solutions/best-practices/agent-native-semantic-json-fields-over-stderr-warnings-2026-04-20.md`) — sister doc
  clarifying that `--quiet` suppresses stderr human echo, not stdout envelope. The new audits respect this
  distinction.
- **SoT contract for spec repos with downstream consumers**
  (`docs/solutions/best-practices/sot-contract-for-spec-repos-with-downstream-consumers-2026-04-22.md`) — sequencing is
  dictated: tag the spec first, vendor here, then add registry rows. Decoupled versioning means lag is honest and
  visible via `spec_version` in the scorecard.
- **Behavioral vs structural MUST when authoring spec requirements**
  (`docs/solutions/best-practices/behavioral-vs-structural-must-when-authoring-spec-requirements-20260420.md`) — phrase
  each new SHOULD as observable behavior under a condition. Don't enumerate libraries or subcommands.
- **CLI linter fork bomb from recursive self-invocation during dogfood**
  (`docs/solutions/logic-errors/cli-linter-fork-bomb-recursive-self-invocation-20260401.md`) — non-negotiables for
  behavioral audits: no bare subcommand probes; do not remove `arg_required_else_help`. The error-induction technique
  (clap-rejected bad arg) is the safe path because it's universally side-effect-free.
- **Reliable static-analysis compliance auditors — SRP scripts, no multi-signal scoring**
  (`docs/solutions/best-practices/reliable-static-analysis-compliance-auditors-20260327.md`) — two distinct audits, not
  one bundled. Each requirement independently visible in `anc audit .` output.
- **Audit scripts are documentation's immune system**
  (`docs/solutions/best-practices/audit-scripts-as-documentation-immune-system-2026-04-20.md`) — strongest argument for
  landing the audits. Decline any reviewer suggestion to "just document the convention more clearly" — the convention is
  documented; the gap is enforcement.

### External References

None — internal/cross-repo work, no external doc gathering needed.

---

## Key Technical Decisions

- **Behavioral layer for both audits.** Each SHOULD describes observable runtime behavior of the target CLI;
  source-layer probes can't generally tell whether `--output json` is honored on a path or whether the error envelope
  matches the success envelope. The audit IDs follow `p2-output-every-subcommand` and
  `p4-typed-error-reason`, concise IDs that don't echo `should-` (since that level is
  carried by the registry entry, not the audit ID per existing convention).
- **Use `HelpOutput::subcommands()` as-is** for `output-applies-to-every-subcommand`. Iterate every parsed subcommand
  and probe each with `--output json --help` (safe). Aggregate per-subcommand verdicts: pass when every subcommand
  accepts `--output`, fail when any one rejects, skip when no subcommands or no `--output` on the root.
- **Error-path elicitation via clap-rejected bad arg.** Reuse the shared bad flag from `error_probe.rs`
  (`--this-flag-does-not-exist-anc`) under the declared JSON mode to elicit the error envelope. Universally safe: clap
  rejects before any subcommand handler runs, no side-effecting code path is entered. The five error-battery audits
  probe error paths the same way.
- **`output-applies-to-every-subcommand` is `Conditional`.** Gate: `if: CLI uses subcommands`. Same shape as
  `p6-must-global-flags` (`src/principles/spec/principles/p6-composable-predictable-command-structure.md`). The P4
  SHOULD is `Universal`: every CLI has an error path, and the typed-reason contract is universal once `--output json`
  is honored.
- **`SUPPRESSION_TABLE` mapping.** `output-applies-to-every-subcommand` enters the currently-empty `FileTraversal` slot
  (the slot was reserved for subcommand-structure SHOULDs at `registry.rs:196-201` — exact fit). The P4 audit does not
  suppress under any current category by default; declare that explicitly in the table comment so future audit-profile
  reviewers don't assume an oversight. Drift tests in `registry.rs` will validate.
- **No SHOULD for the covered envelope checks.** `consistent_envelope.rs` compares the success and error envelopes'
  top-level key sets for `p2-should-consistent-envelope`, and `json_error_output.rs` checks that an error under the
  declared JSON mode parses as JSON for `p4-should-json-error-output`, on either stream.
- **Dogfood guards by ID, not aggregate.** Existing `tests/dogfood.rs` asserts no fail on `p2-*` / `p5-*` collectively.
  Add two new test functions, one per new audit ID, so a regression names the broken contract — collective assertions
  hide which audit newly broke.
- **Land coverage-matrix regen in the same PR.** Both `docs/coverage-matrix.md` and `coverage/matrix.json` are committed
  artifacts. `tests/integration.rs` runs `anc emit coverage-matrix --check` and fails when stale. The PR cannot merge
  without regenerated artifacts.

---

## Open Questions

### Resolved During Planning

- **Do a uniform-schema SHOULD and an error-envelope SHOULD still need new IDs?** No. `p2-should-consistent-envelope`
  and `p4-should-json-error-output` cover them.
- **Do new SHOULDs require new audit-profile categories?** No. `output-applies-to-every-subcommand` slots into
  `FileTraversal`; the P4 SHOULD applies universally without suppression.
- **Spec versioning?** Additive — agentnative-spec's `0.x` schema treats new requirement IDs as additive. The version
  bumps to the next minor; consumers feature-detect new IDs.
- **Should the registry counters bump and the audit additions land as separate PRs?** No — atomic PR after the spec tag
  lands. Splitting would leave two `**UNCOVERED**` SHOULDs in the matrix transiently; one merge keeps the matrix
  coherent at every commit.

### Deferred to Implementation

- **Exact prose for each SHOULD's `summary` and (where present) `examples`/`how-to-verify` fields.** The
  agentnative-spec authoring AGENTS.md prescribes a pressure-test protocol; final prose emerges from running that
  protocol against draft text. Plan-time we know the *shape* (behavioral phrasing, condition gates) but not the exact
  words.
- **The `audience` classifier interaction.** Neither new SHOULD is an obvious signal candidate (signals are
  binary capability indicators: `p1-non-interactive`, `p2-json-output`, `p7-quiet`, `p6-no-color-behavioral`). Verify
  during implementation that `audience::SIGNAL_AUDIT_IDS` does not need extension. If it does, that's a separate
  decision and would need its own test extension.
- **The exact wording of each new audit's `Skip` evidence.** Suppression evidence prefix (`SUPPRESSION_EVIDENCE_PREFIX`
  in `registry.rs`) is shared. Per-audit `Skip` reasons (e.g., "no subcommands detected", "tool does not honor
  `--output json`") emerge during implementation.

---

## Implementation Units

- U1. **`agentnative-spec`: add two SHOULDs and tag a release**

  **Goal:** Land the two SHOULDs in `agentnative-spec/principles/p2-structured-parseable-output.md` and
  `p4-fail-fast-actionable-errors.md`, then tag a new `v*` release so `scripts/sync-spec.sh` here can pull them.

  **Target repo:** `agentnative-spec` (separate repo at `~/dev/agentnative-spec`).

  **Requirements:** R1, R2, R3.

  **Dependencies:** None.

  **Files:**
- Modify (in `agentnative-spec`): `principles/p2-structured-parseable-output.md` — add one SHOULD.
- Modify (in `agentnative-spec`): `principles/p4-fail-fast-actionable-errors.md` — add one new SHOULD.
- Modify (in `agentnative-spec`): `VERSION` — bump to next minor.

  **Approach:**
- Each SHOULD entry is a single new YAML list item under the relevant principle's frontmatter `requirements` section,
  mirroring the format of existing SHOULDs (`id`, `level: should`, `applicability`, `summary`, plus optional `examples`
  / `how-to-verify` if the existing siblings include them).
- `output-applies-to-every-subcommand` carries `applicability: { if: "CLI uses subcommands" }` (same prose as
  `p6-must-global-flags`'s gate). The P4 SHOULD carries `applicability: universal`.
- Run the AGENTS.md per-file pressure-test protocol on each draft summary before committing.
- Tag with an annotated `v*` tag (`git tag -a -m "..."`); push tags. The producer side's release automation handles
  distribution.

  **Patterns to follow:** existing SHOULD entries in the same file; the cross-repo SoT contract for spec repos.

  **Test scenarios:**
- **Happy path:** Each new SHOULD parses cleanly via `parser.rs` (running locally against a checkout of this repo with
  the spec re-vendored — verify `cargo build` succeeds with the new IDs in `REQUIREMENTS`).
- **Edge case:** `output-applies-to-every-subcommand`'s `applicability.if` value is non-empty after trimming —
  `parser.rs:387-395` rejects empty bare strings.

  **Verification:**
- `agentnative-spec` repo's own `cargo test` (or equivalent) passes.
- New tag visible via `git ls-remote --tags agentnative-spec`.
- `VERSION` reflects the bump.

- U2. **Sync vendored spec + bump drift counters**

  **Goal:** Pull the new spec tag into `src/principles/spec/`, fix the build by bumping the two registry-size
  counter tests, and confirm the two new IDs appear in `REQUIREMENTS`.

  **Requirements:** R4.

  **Dependencies:** U1 must have shipped (tag exists at `agentnative-spec`'s remote).

  **Files:**
- Modify: `src/principles/spec/principles/p2-structured-parseable-output.md` (overwrite via `sync-spec.sh`).
- Modify: `src/principles/spec/principles/p4-fail-fast-actionable-errors.md` (same).
- Modify: `src/principles/spec/VERSION` (same).
- Modify: `src/principles/registry.rs` — bump `registry_size_matches_spec` count from 60 to 62 and
  `level_counts_match_spec` Should count from 22 to 24.

  **Approach:**
- Run `bash scripts/sync-spec.sh` to vendor the new tag's content. The script is remote-first; pulls the latest `v*` tag
  automatically.
- `cargo build` regenerates `$OUT_DIR/generated_requirements.rs` automatically on next compile (build.rs reruns via
  `cargo:rerun-if-changed=src/principles/spec/`).
- Run `cargo test --bin anc principles::registry::tests::registry_size_matches_spec
  principles::registry::tests::level_counts_match_spec` first to confirm they fail (proving the new IDs were parsed).
  Then bump the counters.
- At this point the two new SHOULDs appear in `docs/coverage-matrix.md` as `**UNCOVERED**` rows. That is expected and
  resolves in U3 and U6.

  **Patterns to follow:** the existing spec-vendor flow already in use at this repo. No new mechanism.

  **Test scenarios:**
- **Happy path:** `cargo test --bin anc principles::registry::tests` passes after counter bumps.
- **Edge case:** Forgetting to bump `level_counts_match_spec` while bumping `registry_size_matches_spec` — both must
  move together. The plan calls this out explicitly so the implementer doesn't half-bump.
- **Drift signal:** `docs/coverage-matrix.md` rendered locally shows two new `**UNCOVERED**` rows for the new SHOULD
  IDs — proof the spec sync worked.

  **Verification:**
- All registry tests pass.
- `cargo run -- emit coverage-matrix` runs without dangling-cover-id errors.
- The two new requirement IDs appear in `coverage/matrix.json` (as uncovered rows for now).

- U3. **`p2-output-every-subcommand` audit**

  **Goal:** Verify that every subcommand of the target CLI accepts `--output {text,json}` if the root command does.

  **Requirements:** R5; covers `p2-should-output-applies-to-every-subcommand`.

  **Dependencies:** U2.

  **Files:**
- Create: `src/audits/behavioral/output_every_subcommand.rs` (~120 LOC).
- Modify: `src/audits/behavioral/mod.rs` — register in `all_behavioral_audits()`.
- Test: in-file `#[cfg(test)] mod tests` using `test_project_with_sh_script` from `src/audits/behavioral/mod.rs`.

  **Approach:**
- Probe root help; if no `--output` flag detected, return `Skip` with evidence "root CLI does not accept --output".
- Use `HelpOutput::subcommands()` to enumerate. If empty, `Skip` with "no subcommands detected".
- For each subcommand, probe `<subcmd> --help` (safe — never bare). Look for `--output` in the help text.
- Aggregate: `Pass` when every subcommand has `--output`. `Fail` when at least one rejects. Evidence names the failing
  subcommand(s).
- `applicable()` returns `project.runner.is_some()`.
- `covers()` returns `&["p2-should-output-applies-to-every-subcommand"]`.

  **Patterns to follow:** `src/audits/behavioral/json_output.rs` for layout, subcommand enumeration, and probe
  shape.

  **Test scenarios:**
- **Happy path:** Multi-subcommand CLI where every subcommand accepts `--output` → `Pass`.
- **Happy path:** Single-command CLI (no subcommands) → `Skip` with "no subcommands".
- **Happy path:** CLI without `--output` on root → `Skip` with "root does not accept --output".
- **Error path:** Two-subcommand CLI where one accepts `--output` and one does not → `Fail` naming the offender.
- **Edge case:** Subcommand with non-zero exit on `--help` (rare but real) → `Skip` with "could not probe subcommand X".

  **Verification:**
- `cargo test --bin anc behavioral::output_every_subcommand::tests` passes.
- `anc audit . --output json` includes the new ID in `results[]`.

- U4. **Not in scope: an error-envelope audit.** `p4-should-json-error-output` (`json_error_output.rs`) checks that an
  error under the declared JSON mode parses as JSON on stdout or stderr.

- U5. **Not in scope: a uniform-schema audit.** `p2-should-consistent-envelope` (`consistent_envelope.rs`) compares the
  success and error envelopes' top-level key sets.

- U6. **`p4-typed-error-reason` audit**

  **Goal:** Verify that when `--output json` is set and the CLI hits an error, the JSON envelope includes a typed
  machine-readable `reason` (or equivalent) identifier — kebab-case or camelCase, not English prose.

  **Requirements:** R5; covers `p4-should-json-error-includes-typed-reason`.

  **Dependencies:** U2.

  **Files:**
- Create: `src/audits/behavioral/typed_error_reason.rs` (~80 LOC).
- Modify: `src/audits/behavioral/mod.rs` — register.
- Test: in-file `#[cfg(test)] mod tests`.

  **Approach:**
- Reuse the bad-flag probe in `error_probe.rs` under the declared JSON mode to capture the error envelope.
- Walk the JSON top-level keys looking for any of: `reason`, `error_code`, `code`, `kind`, `error_kind`. Take the first
  match.
- Validate the field is a string and matches a typed-identifier pattern: lowercase ASCII alphanumeric plus `-`/`_`, no
  spaces, length ≤ 64, no sentence-shaped content. Use a simple regex audit.
- `Pass` when a typed identifier is present. `Warn` (not `Fail`) when the field exists but contains English-prose style
  content (e.g., `"reason": "Destination directory is not empty"`) — the field exists but isn't typed. The warn signals
  fixable drift toward typed values without breaking the SHOULD audit entirely. `Fail` when no candidate field is
  present at all. `Skip` when no `--output` support or no envelope elicited.
- `applicable()` returns `project.runner.is_some()`.
- `covers()` returns `&["p4-should-json-error-includes-typed-reason"]`.

  **Patterns to follow:** the typed-fields-over-display-strings doc's grep heuristic for catching sentence-shaped
  values.

  **Test scenarios:**
- **Happy path:** Envelope has `"reason": "destination-not-empty"` → `Pass`.
- **Happy path:** Envelope has `"kind": "permission_denied"` (snake_case, also typed) → `Pass`.
- **Warn:** Envelope has `"reason": "Destination not empty"` (English) → `Warn` with evidence about typed- identifier
  preference.
- **Error path:** Envelope has only `"message": "..."`, no typed-identifier field → `Fail`.
- **Skip:** No envelope elicited → `Skip`.

  **Verification:** `cargo test` passes; new ID in `anc audit .` output.

- U7. **Suppression entries, dogfood guards, and coverage-matrix regen**

  **Goal:** Add the audit-profile suppression mapping for the new audits, extend the dogfood guards to pin the new
  IDs against `anc` itself, and regenerate the committed coverage-matrix artifacts.

  **Requirements:** R6, R7, R8.

  **Dependencies:** U3 and U6 merged (or at least the new IDs and audit IDs are stable).

  **Files:**
- Modify: `src/principles/registry.rs` — add `output_every_subcommand` audit ID to the `FileTraversal` entry of
  `SUPPRESSION_TABLE` (lines 196-201). Document the P4 audit as intentionally unsuppressed via a comment
  naming them. Test `every_suppression_table_audit_id_exists` (or equivalent — verify exact name during implementation)
  confirms the entry.
- Modify: `tests/dogfood.rs` — add two new test functions: `dogfood_no_p2_should_output_every_subcommand_fail` and
  `dogfood_no_p4_should_json_error_typed_reason_fail`. Each spawns `anc audit . --output json` and asserts no `fail`
  status on the named audit ID. Mirrors existing `dogfood_no_p2_fail_after_skill_subcommand` shape but names the audit
  ID specifically.
- Modify: `docs/coverage-matrix.md` — regenerated by `anc emit coverage-matrix`. Manual prose summary at the top updates
  the requirement counts and notes the two new SHOULDs.
- Modify: `coverage/matrix.json` — regenerated by `anc emit coverage-matrix`.

  **Approach:**
- Run `cargo run -- emit coverage-matrix` to regenerate both artifacts. Review `git diff coverage/matrix.json` for
  the two new rows (now covered, not uncovered).
- For dogfood guards: each test follows the existing `dogfood_no_p2_fail_after_skill_subcommand` pattern at
  `tests/dogfood.rs:53-65` but filters results by exact audit ID, not prefix. The exact-ID assertion is the intentional
  change — collective `p2-*` assertions hide which audit newly fails.

  **Patterns to follow:** existing `SUPPRESSION_TABLE` entries; existing dogfood tests; existing coverage-matrix
  regeneration step.

  **Test scenarios:**
- **Happy path:** Both new dogfood tests pass against the current `anc` binary (proves the dogfood loop —
  `anc skill install` already implements both contracts).
- **Drift catch:** `anc emit coverage-matrix --check` exits 0 after the regen commit; exits 2 if either artifact is
  stale.

  **Verification:**
- `cargo test` passes (all 519+ tests, including new dogfood tests).
- `anc emit coverage-matrix --check` exits 0.
- `anc audit . --output json` shows both new audit IDs at status `pass` (or appropriately `skip`/`warn`).

---

## System-Wide Impact

- **Interaction graph:** new audits integrate at `src/audits/behavioral/mod.rs::all_behavioral_audits()`. No middleware
  or callback impact. The dogfood tests at `tests/dogfood.rs` extend existing patterns. Coverage matrix regeneration
  touches `docs/coverage-matrix.md` and `coverage/matrix.json` — both committed artifacts already drift-checked by CI.
- **Error propagation:** new audits return `AuditStatus` per the existing convention. Failures in error-induction probes
  (e.g., binary crashes) surface as `Skip` with structured evidence rather than propagating up.
- **State lifecycle risks:** none. Behavioral probes spawn the target binary with `BinaryRunner` which has built-in
  timeouts, output caps, and result caching. No persistent state.
- **API surface parity:** the new audit IDs are external API surface (referenced by `coverage/matrix.json`, by consumer
  site `/coverage` page, by user-written `--audit-profile` consumers). Once published, IDs are stable — rename requires
  a deliberate spec edit and downstream coordination.
- **Integration coverage:** existing integration tests at `tests/integration.rs` will pick up the new audits
  automatically (any test that runs `anc audit . --output json` and inspects `results[]` sees the new IDs). The
  `test_audit_json_output` family already exercises this surface.
- **Unchanged invariants:**
- `arg_required_else_help` stays on `Cli`. The new behavioral audits must NOT bare-probe subcommands (per fork-
  bomb-safety rule).
- The four committed audit-profile categories. No fifth category.
- The schema 0.4 scorecard envelope. New requirement IDs flow through `coverage_summary` automatically.
- `audience` classifier signal IDs (`p1-non-interactive`, `p2-json-output`, `p7-quiet`, `p6-no-color-behavioral`). Neither
  new audit is a signal candidate.

---

## Risks & Dependencies

| Risk                                                                                                                                                                                                                                           | Mitigation                                                                                                                                                                                                                                           |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| The two new behavioral audits induce error paths in target binaries through the shared bad-flag probe in `error_probe.rs`, which five audits already run under the declared JSON mode.                                                         | The combination is structurally safe (clap rejects before any subcommand handler runs), and `RunStatus::Crash` is handled explicitly. Each audit's test suite includes a crash-handling scenario.                                                    |
| `anc` itself fails one of the new audits against itself, breaking the dogfood claim before the PR can land.                                                                                                                                    | The two SHOULDs codify behavior `anc skill install` already implements. If a new audit fails against `anc`, that's a real bug to fix in the same PR (not a reason to weaken the audit). The dogfood guards in `tests/dogfood.rs` catch this loudly.  |
| `output-applies-to-every-subcommand` aggregates per-subcommand verdicts; the aggregation rule (fail-on-first vs allow-some-skip) affects how the audit reports.                                                                                | Documented decision: fail when ANY subcommand rejects `--output`, name the offender(s) in evidence. Skip when there are no subcommands or no `--output` on root. The aggregation is conservative and human-readable.                                 |
| Existing `p2-must-json-errors` says "errors emitted as JSON to stderr"; `anc`'s actual envelope goes to stdout. `p4-should-json-error-output` accepts either stream, so the MUST-vs-SHOULD tension is real.                                    | Deferred: see Scope Boundaries. A spec PR outside this plan reconciles the MUST text.                                                                                                                                                                |
| Counter-bumps in `registry_size_matches_spec` and `level_counts_match_spec` are easy to forget.                                                                                                                                                | CLAUDE.md notes the deliberate-act intent; U2 explicitly enumerates both counters; failing tests guide the implementer.                                                                                                                              |
| Coverage-matrix regen is an easy step to forget at PR time.                                                                                                                                                                                    | Existing CI integration test fails the PR if either artifact is stale. The regen step is U7's last task.                                                                                                                                             |
| `HelpOutput::subcommands()` parses help text heuristically; some CLI help formats may not match the patterns it recognizes.                                                                                                                    | The audit returns `Skip` with structured evidence ("no subcommands detected") rather than `Fail` when enumeration fails. This biases conservative — better a missed `Pass` than a false `Fail`.                                                      |

---

## Documentation / Operational Notes

- The two new requirement IDs become external API surface as soon as the spec tag publishes. Any rename later requires
  a deliberate spec edit and consumer coordination (`agentnative-site` `/coverage` page reads the IDs from
  `coverage/matrix.json`; tools may pin against them in `--audit-profile` configurations).
- CLAUDE.md's "Principle Registry" section may grow a paragraph about the two new audits once they ship — the
  hand-maintained prose summary at the top of `docs/coverage-matrix.md` updates per the existing convention.
- Pre-release checklist (`RELEASES.md`) does not gain a new step. The existing
  `bash scripts/sync-spec.sh && git diff src/principles/spec/` step covers the new SHOULDs automatically.

---

## Sources & References

- Parent plan (just-shipped feature this follows up on):
  [docs/plans/2026-04-29-002-feat-skill-subcommand-plan.md](2026-04-29-002-feat-skill-subcommand-plan.md), Pattern
  Documentation Note section.
- Convention being codified: `docs/solutions/architecture-patterns/anc-cli-output-envelope-pattern-2026-04-29.md`.
- Cross-repo coordination playbook:
  `docs/solutions/best-practices/sot-contract-for-spec-repos-with-downstream-consumers-2026-04-22.md`.
- SRP for static-analysis audits:
  `docs/solutions/best-practices/reliable-static-analysis-compliance-auditors-20260327.md`.
- Behavioral phrasing for SHOULDs:
  `docs/solutions/best-practices/behavioral-vs-structural-must-when-authoring-spec-requirements-20260420.md`.
- Fork-bomb safety: `docs/solutions/logic-errors/cli-linter-fork-bomb-recursive-self-invocation-20260401.md`.
- Existing P2 behavioral audit shape: `src/audits/behavioral/json_output.rs`.
- Existing P4 behavioral audit shape: `src/audits/behavioral/bad_args.rs`.
- Registry conventions: `src/principles/registry.rs`, `src/principles/matrix.rs`.
- Spec authoring protocol: `agentnative-spec/principles/AGENTS.md`.
