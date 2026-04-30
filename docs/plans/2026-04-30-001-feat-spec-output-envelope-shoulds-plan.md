---
title: "feat: agentnative-spec output-envelope SHOULDs + matching checks"
type: feat
status: active
date: 2026-04-30
---

# feat: agentnative-spec output-envelope SHOULDs + matching checks

## Summary

Add four new SHOULDs to `agentnative-spec` (three in P2, one in P4) that codify the output-envelope contract `anc`
itself already enforces, then land matching behavioral checks here so the linter verifies them. Cross-repo: spec tag
lands first (producer side); this repo re-vendors via `scripts/sync-spec.sh`, grows the registry by four IDs, adds four
checks under `src/checks/behavioral/`, and regenerates the coverage matrix. `anc` must pass all four checks against
itself (dogfood loop).

---

## Problem Frame

The `anc skill install` work shipped a uniform JSON envelope (success and error sharing one schema, typed `reason` on
error, `--output {text,json}` honored on every code path) and pinned dogfood guards via `tests/dogfood.rs`. The contract
is captured in prose at `docs/solutions/architecture-patterns/anc-cli-output-envelope-pattern-2026-04-29.md`, but no
agent-native-spec requirement names it and no `Check` verifies it. Per the institutional learning *audit-scripts-as-
documentation-immune-system* (`docs/solutions/best-practices/`), prose without enforcement rots silently while the
linter reports green. This plan closes that gap.

The closest existing P2 requirement (`p2-must-json-errors`) sits in `UNVERIFIED_MUSTS`
(`src/principles/matrix.rs:476-487`) because no behavioral check probes error paths. The four new SHOULDs are the first
behavioral checks that *induce* an error path and inspect the envelope — genuinely new ground for this codebase.

---

## Requirements

- R1. Add four new SHOULDs to `agentnative-spec` covering output-envelope behavior:
- P2 `output-applies-to-every-subcommand` (Conditional: `if: CLI uses subcommands`).
- P2 `json-envelope-on-error` (Universal).
- P2 `output-envelope-schema-uniform` (Universal). Distinct from existing `p2-should-consistent-envelope` which
  addresses cross-command consistency; this new SHOULD addresses success-vs-error consistency *within* a single
  command's envelope.
- P4 `json-error-includes-typed-reason` (Universal).
- R2. Phrase each SHOULD behaviorally per *behavioral-vs-structural-must* — observable behavior under a stated
  condition, not enumerated subcommands or library lists.
- R3. Tag a new `agentnative-spec` release that includes the four new SHOULDs. The tag is the single propagation point —
  this repo's `scripts/sync-spec.sh` resolves the latest `v*` tag remotely.
- R4. Re-vendor the spec into `src/principles/spec/`. Bump the two drift counters in
  `src/principles/registry.rs::tests`: `registry_size_matches_spec` (46 → 50) and `level_counts_match_spec` (Should: 16
  → 20).
- R5. Land four behavioral checks in `src/checks/behavioral/`, one per new SHOULD, each declaring its own single-element
  `covers()` slice. SRP-style — no bundled "envelope conformance" check (per
  *reliable-static-analysis-compliance-checkers*).
- R6. Update `SUPPRESSION_TABLE` in `src/principles/registry.rs` for any check that should be excluded under an existing
  audit-profile category. Specifically: `output-applies-to-every-subcommand` joins the empty `FileTraversal` slot
  reserved for "subcommand-structure SHOULDs" (`src/principles/registry.rs:196-201`). Verify the other three against
  each existing category and document the mapping decision.
- R7. Extend `tests/dogfood.rs` to pin all four new IDs against the `anc` binary itself — `anc check . --output json`
  must show no `fail` on any of the new check IDs.
- R8. Regenerate `docs/coverage-matrix.md` and `coverage/matrix.json` via `anc generate coverage-matrix`. CI's existing
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
- No refactoring of existing P2/P4 checks beyond the minimum needed to land the new ones.
- No spec-version major or minor bump beyond what additive SHOULD additions require. The spec is at `0.x` and uses
  additive evolution; consumers feature-detect new requirement IDs.
- No subcommand-discovery infrastructure beyond what `output-applies-to-every-subcommand` needs.
  `parse_subcommand_names()` (`src/checks/behavioral/json_output.rs:100-137`) is the existing primitive — reuse it, do
  not extract a general-purpose subcommand-introspection module yet.
- No envelope-pattern documentation in `docs/solutions/` — that's the parallel `/ce-compound` work in the same task
  queue, separate from this plan.
- No `coverage_summary` schema changes in the scorecard. New requirement IDs flow through automatically.
- No per-host or per-action SHOULDs. The new requirements apply universally (or with the one explicit conditional).

### Deferred to Follow-Up Work

- **Reconciliation of `p2-must-json-errors` ("on stderr") with the as-shipped convention (envelope on stdout).** The
  existing MUST says errors emit JSON to stderr; `anc`'s shipped `feat/skill-install` envelope goes to stdout (matching
  every other `--output json` surface). The MUST is currently unverified; the new SHOULD `json-envelope-on-error` is
  more permissive (says "envelope appears" without pinning the stream). Reconciling the MUST text is a separate
  spec-edit PR; flagged here so a future plan picks it up. Not in scope because the user-named scope is "add four
  SHOULDs", not "rewrite an existing MUST".
- **Source-layer fallbacks for the four checks.** Behavioral probing covers the binary; an ast-grep source-layer
  companion (e.g., detecting `#[serde(skip_serializing_if)]` on envelope structs) would extend coverage to projects
  whose binaries fail to launch. Defer until usage shows behavioral checks miss real cases.
- **Subcommand-introspection module extraction.** If a third or fourth check needs to enumerate subcommands, lifting
  `parse_subcommand_names()` into a reusable module is the natural cleanup. Defer until that demand exists.

---

## Context & Research

### Relevant Code and Patterns

- `src/check.rs` — `Check` trait. New checks implement `id`, `label`, `group`, `layer`, `applicable`, `run`, and declare
  `covers()` returning a `&'static [&'static str]` of new requirement IDs.
- `src/checks/behavioral/json_output.rs` — canonical behavioral P2 check shape. Subcommand enumeration via
  `parse_subcommand_names()` (lines 100-137). Probes use `--help`/`--version` only (safe-probing rule, lines 165-170).
  JSON parsing via `serde_json::from_str::<serde_json::Value>(...)`.
- `src/checks/behavioral/bad_args.rs` — only existing precedent for inducing an error path. Injects
  `--this-flag-does-not-exist-agentnative-probe` to force exit-2. Pattern reuses across the four new behavioral checks
  (combine bad-arg trigger with `--output json`).
- `src/checks/behavioral/mod.rs` (lines 61-115) — `test_project_with_sh_script(script)` test helper that materializes a
  temp `/bin/sh` script with `case "$*" in ... esac` branches simulating CLI behavior. Use for unit tests.
- `src/principles/registry.rs` — drift counters at lines 337-345. ID format enforced by tests at lines 280-301.
  `SUPPRESSION_TABLE` shape at lines 174-225.
- `src/principles/matrix.rs` — `dangling_cover_ids()` (lines 330-340) fails the build when a `covers()` ID is missing
  from `REQUIREMENTS`. `UNVERIFIED_MUSTS` table (lines 469-486) — note the existing `p2-must-json-errors` and
  `p4-must-actionable-errors` entries; the new SHOULDs do not move those out (different requirement IDs).
- `src/principles/spec/principles/p2-structured-parseable-output.md` — target file for three of the four new SHOULDs.
- `src/principles/spec/principles/p4-fail-fast-actionable-errors.md` — target file for the fourth SHOULD.
- `tests/dogfood.rs` — extension point for dogfood guards. Existing tests pin `p2-*` and `p5-*` collectively; add new
  assertions naming the four new IDs explicitly so a future regression names the broken contract.
- `tests/integration.rs` (lines 66-75) — `test_generate_coverage_matrix_drift_check_passes_on_committed_artifacts`. Will
  fail CI if `docs/coverage-matrix.md` or `coverage/matrix.json` are stale at PR time.
- `scripts/sync-spec.sh` — vendor the new spec tag remote-first. Mirrors the `scripts/sync-skill-fixture.sh` shape.
- `agentnative-spec/principles/AGENTS.md` — pressure-test protocol for new requirements. Apply when drafting prose.

### Institutional Learnings

- **anc CLI output envelope pattern**
  (`docs/solutions/architecture-patterns/anc-cli-output-envelope-pattern-2026-04-29.md`) — anticipates these four
  SHOULDs by name. The canonical statement of the convention; cite as the rationale source in spec-edit PR descriptions.
- **Consistent JSON schema across success and error paths**
  (`docs/solutions/best-practices/consistent-json-schema-across-success-and-error-paths-2026-04-20.md`) — direct
  rationale for the `output-envelope-schema-uniform` SHOULD. Provides a "every JSON path has these fields" contract-
  test pattern.
- **CLI structure for machines: typed JSON fields, not English in display strings**
  (`docs/solutions/best-practices/cli-structure-for-machines-typed-json-fields-over-display-strings-2026-04-20.md`) —
  rationale for the P4 typed-reason SHOULD. Suggests a grep heuristic for the check (`rg -n '"[A-Z][a-z]+ [a-z]+
  [a-z]+'` flags sentence-shaped JSON values).
- **Agent-native CLIs: semantic JSON fields, not stderr warnings**
  (`docs/solutions/best-practices/agent-native-semantic-json-fields-over-stderr-warnings-2026-04-20.md`) — sister doc
  clarifying that `--quiet` suppresses stderr human echo, not stdout envelope. New `json-envelope-on-error` check must
  respect this distinction.
- **SoT contract for spec repos with downstream consumers**
  (`docs/solutions/best-practices/sot-contract-for-spec-repos-with-downstream-consumers-2026-04-22.md`) — sequencing is
  dictated: tag the spec first, vendor here, then add registry rows. Decoupled versioning means lag is honest and
  visible via `spec_version` in the scorecard.
- **Behavioral vs structural MUST when authoring spec requirements**
  (`docs/solutions/best-practices/behavioral-vs-structural-must-when-authoring-spec-requirements-20260420.md`) — phrase
  each new SHOULD as observable behavior under a condition. Don't enumerate libraries or subcommands.
- **CLI linter fork bomb from recursive self-invocation during dogfood**
  (`docs/solutions/logic-errors/cli-linter-fork-bomb-recursive-self-invocation-20260401.md`) — non-negotiables for
  behavioral checks: no bare subcommand probes; do not remove `arg_required_else_help`. The error-induction technique
  (clap-rejected bad arg) is the safe path because it's universally side-effect-free.
- **Reliable static-analysis compliance checkers — SRP scripts, no multi-signal scoring**
  (`docs/solutions/best-practices/reliable-static-analysis-compliance-checkers-20260327.md`) — four distinct checks, not
  one bundled. Each requirement independently visible in `anc check .` output.
- **Audit scripts are documentation's immune system**
  (`docs/solutions/best-practices/audit-scripts-as-documentation-immune-system-2026-04-20.md`) — strongest argument for
  landing the checks. Decline any reviewer suggestion to "just document the convention more clearly" — the convention is
  documented; the gap is enforcement.

### External References

None — internal/cross-repo work, no external doc gathering needed.

---

## Key Technical Decisions

- **Behavioral layer for all four checks.** Each SHOULD describes observable runtime behavior of the target CLI;
  source-layer probes can't generally tell whether `--output json` is honored on a path or whether the error envelope
  matches the success envelope. The check IDs follow `p2-output-every-subcommand`, `p2-json-envelope-on-error`,
  `p2-envelope-schema-uniform`, `p4-typed-error-reason` — concise IDs that don't echo `should-` (since that level is
  carried by the registry entry, not the check ID per existing convention).
- **Use `parse_subcommand_names()` as-is** for `output-applies-to-every-subcommand`. Iterate every parsed subcommand and
  probe each with `--output json --help` (safe). Aggregate per-subcommand verdicts: pass when every subcommand accepts
  `--output`, fail when any one rejects, skip when no subcommands or no `--output` on the root.
- **Error-path elicitation via clap-rejected bad arg.** Combine the existing `bad_args.rs` injection
  (`--this-flag-does-not-exist-agentnative-probe`) with `--output json` to elicit the error envelope. Universally safe:
  clap rejects before any subcommand handler runs, no side-effecting code path is entered. This is the load- bearing
  innovation of the four checks — the first time `anc` probes an error path.
- **`output-applies-to-every-subcommand` is `Conditional`.** Gate: `if: CLI uses subcommands`. Same shape as
  `p6-must-global-flags` (`src/principles/spec/principles/p6-composable-predictable-command-structure.md`). The other
  three SHOULDs are `Universal` — every CLI has an error path; the JSON envelope contract is universal once `--output
  json` is honored.
- **`SUPPRESSION_TABLE` mapping.** `output-applies-to-every-subcommand` enters the currently-empty `FileTraversal` slot
  (the slot was reserved for subcommand-structure SHOULDs at `registry.rs:196-201` — exact fit). The other three do not
  suppress under any current category by default; declare that explicitly in the table comment so future audit-profile
  reviewers don't assume an oversight. Drift tests in `registry.rs` will validate.
- **Distinguish `output-envelope-schema-uniform` from existing `p2-should-consistent-envelope`.** The existing SHOULD is
  *cross-command* ("every command has a predictable envelope shape"). The new SHOULD is *within-command,
  success-vs-error* ("a given action's envelope keys are uniform across success and error"). Both apply, on different
  axes. The new SHOULD's prose must call out the distinction so reviewers don't read it as a duplicate.
- **Dogfood guards by ID, not aggregate.** Existing `tests/dogfood.rs` asserts no fail on `p2-*` / `p5-*` collectively.
  Add four new test functions, one per new check ID, so a regression names the broken contract — collective assertions
  hide which check newly broke.
- **Land coverage-matrix regen in the same PR.** Both `docs/coverage-matrix.md` and `coverage/matrix.json` are committed
  artifacts. `tests/integration.rs` runs `anc generate coverage-matrix --check` and fails when stale. The PR cannot
  merge without regenerated artifacts.

---

## Open Questions

### Resolved During Planning

- **Should the new `output-envelope-schema-uniform` consolidate with the existing `p2-should-consistent-envelope`?** No
  — they address distinct axes (cross-command vs within-command success-vs-error). Both stay. The new SHOULD's prose
  explicitly calls out the distinction.
- **Do new SHOULDs require new audit-profile categories?** No. `output-applies-to-every-subcommand` slots into
  `FileTraversal`; the other three apply universally without suppression.
- **Spec versioning?** Additive — agentnative-spec's `0.x` schema treats new requirement IDs as additive. The version
  bumps to the next minor; consumers feature-detect new IDs.
- **Should the registry counters bump and the check additions land as separate PRs?** No — atomic PR after the spec tag
  lands. Splitting would leave four `**UNCOVERED**` SHOULDs in the matrix transiently; one merge keeps the matrix
  coherent at every commit.

### Deferred to Implementation

- **Exact prose for each SHOULD's `summary` and (where present) `examples`/`how-to-verify` fields.** The
  agentnative-spec authoring AGENTS.md prescribes a pressure-test protocol; final prose emerges from running that
  protocol against draft text. Plan-time we know the *shape* (behavioral phrasing, condition gates) but not the exact
  words.
- **The `audience` classifier interaction.** None of the four new SHOULDs are obvious signal candidates (signals are
  binary capability indicators: `p1-non-interactive`, `p2-json-output`, `p7-quiet`, `p6-no-color-behavioral`). Verify
  during implementation that `audience::SIGNAL_CHECK_IDS` does not need extension. If it does, that's a separate
  decision and would need its own test extension.
- **The exact wording of each new check's `Skip` evidence.** Suppression evidence prefix (`SUPPRESSION_EVIDENCE_PREFIX`
  in `registry.rs`) is shared. Per-check `Skip` reasons (e.g., "no subcommands detected", "tool does not honor `--output
  json`") emerge during implementation.

---

## Implementation Units

- U1. **`agentnative-spec`: add four new SHOULDs and tag a release**

  **Goal:** Land the four new SHOULDs in `agentnative-spec/principles/p2-structured-parseable-output.md` and
  `p4-fail-fast-actionable-errors.md`, then tag a new `v*` release so `scripts/sync-spec.sh` here can pull them.

  **Target repo:** `agentnative-spec` (separate repo at `~/dev/agentnative-spec`).

  **Requirements:** R1, R2, R3.

  **Dependencies:** None.

  **Files:**
- Modify (in `agentnative-spec`): `principles/p2-structured-parseable-output.md` — add three new SHOULDs.
- Modify (in `agentnative-spec`): `principles/p4-fail-fast-actionable-errors.md` — add one new SHOULD.
- Modify (in `agentnative-spec`): `VERSION` — bump to next minor.

  **Approach:**
- Each SHOULD entry is a single new YAML list item under the relevant principle's frontmatter `requirements` section,
  mirroring the format of existing SHOULDs (`id`, `level: should`, `applicability`, `summary`, plus optional `examples`
  / `how-to-verify` if the existing siblings include them).
- `output-applies-to-every-subcommand` carries `applicability: { if: "CLI uses subcommands" }` (same prose as
  `p6-must-global-flags`'s gate). The other three carry `applicability: universal`.
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
  counter tests, and confirm the four new IDs appear in `REQUIREMENTS`.

  **Requirements:** R4.

  **Dependencies:** U1 must have shipped (tag exists at `agentnative-spec`'s remote).

  **Files:**
- Modify: `src/principles/spec/principles/p2-structured-parseable-output.md` (overwrite via `sync-spec.sh`).
- Modify: `src/principles/spec/principles/p4-fail-fast-actionable-errors.md` (same).
- Modify: `src/principles/spec/VERSION` (same).
- Modify: `src/principles/registry.rs` — bump `registry_size_matches_spec` count from 46 to 50 and
  `level_counts_match_spec` Should count from 16 to 20 (`registry.rs:337,342-344`).

  **Approach:**
- Run `bash scripts/sync-spec.sh` to vendor the new tag's content. The script is remote-first; pulls the latest `v*` tag
  automatically.
- `cargo build` regenerates `$OUT_DIR/generated_requirements.rs` automatically on next compile (build.rs reruns via
  `cargo:rerun-if-changed=src/principles/spec/`).
- Run `cargo test --bin anc principles::registry::tests::registry_size_matches_spec
  principles::registry::tests::level_counts_match_spec` first to confirm they fail (proving the new IDs were parsed).
  Then bump the counters.
- At this point the four new SHOULDs appear in `docs/coverage-matrix.md` as `**UNCOVERED**` rows. That is expected and
  resolves in U3-U6.

  **Patterns to follow:** the existing spec-vendor flow already in use at this repo. No new mechanism.

  **Test scenarios:**
- **Happy path:** `cargo test --bin anc principles::registry::tests` passes after counter bumps.
- **Edge case:** Forgetting to bump `level_counts_match_spec` while bumping `registry_size_matches_spec` — both must
  move together. The plan calls this out explicitly so the implementer doesn't half-bump.
- **Drift signal:** `docs/coverage-matrix.md` rendered locally shows four new `**UNCOVERED**` rows for the new SHOULD
  IDs — proof the spec sync worked.

  **Verification:**
- All registry tests pass.
- `cargo run -- generate coverage-matrix` runs without dangling-cover-id errors.
- The four new requirement IDs appear in `coverage/matrix.json` (as uncovered rows for now).

- U3. **`p2-output-every-subcommand` check**

  **Goal:** Verify that every subcommand of the target CLI accepts `--output {text,json}` if the root command does.

  **Requirements:** R5; covers `p2-should-output-applies-to-every-subcommand`.

  **Dependencies:** U2.

  **Files:**
- Create: `src/checks/behavioral/output_every_subcommand.rs` (~120 LOC).
- Modify: `src/checks/behavioral/mod.rs` — register in `all_behavioral_checks()`.
- Test: in-file `#[cfg(test)] mod tests` using `test_project_with_sh_script` from `src/checks/behavioral/mod.rs`.

  **Approach:**
- Probe root help; if no `--output` flag detected, return `Skip` with evidence "root CLI does not accept --output".
- Use `parse_subcommand_names()` to enumerate. If empty, `Skip` with "no subcommands detected".
- For each subcommand, probe `<subcmd> --help` (safe — never bare). Look for `--output` in the help text.
- Aggregate: `Pass` when every subcommand has `--output`. `Fail` when at least one rejects. Evidence names the failing
  subcommand(s).
- `applicable()` returns `project.runner.is_some()`.
- `covers()` returns `&["p2-should-output-applies-to-every-subcommand"]`.

  **Patterns to follow:** `src/checks/behavioral/json_output.rs` for layout, subcommand enumeration, and probe
  shape.

  **Test scenarios:**
- **Happy path:** Multi-subcommand CLI where every subcommand accepts `--output` → `Pass`.
- **Happy path:** Single-command CLI (no subcommands) → `Skip` with "no subcommands".
- **Happy path:** CLI without `--output` on root → `Skip` with "root does not accept --output".
- **Error path:** Two-subcommand CLI where one accepts `--output` and one does not → `Fail` naming the offender.
- **Edge case:** Subcommand with non-zero exit on `--help` (rare but real) → `Skip` with "could not probe subcommand X".

  **Verification:**
- `cargo test --bin anc behavioral::output_every_subcommand::tests` passes.
- `anc check . --output json` includes the new ID in `results[]`.

- U4. **`p2-json-envelope-on-error` check**

  **Goal:** Verify that when `--output json` is set and the CLI hits an error path, a JSON envelope is emitted
  (on either stdout or stderr) — not human-only stderr text.

  **Requirements:** R5; covers `p2-should-json-envelope-on-error`.

  **Dependencies:** U2.

  **Files:**
- Create: `src/checks/behavioral/json_envelope_on_error.rs` (~100 LOC).
- Modify: `src/checks/behavioral/mod.rs` — register.
- Test: in-file `#[cfg(test)] mod tests`.

  **Approach:**
- First, probe `<root> --help` to confirm `--output` is accepted. If not, `Skip` with "tool does not accept --output
  json".
- Inject the bad-arg trigger from `bad_args.rs`: `--this-flag-does-not-exist-agentnative-probe` plus `--output json`.
  Capture stdout and stderr.
- Try `serde_json::from_str::<serde_json::Value>` on stdout; on failure, try stderr. Pass if either parses to a JSON
  object. Fail if neither parses.
- Evidence on Pass names the stream that carried the envelope (informational). Evidence on Fail includes a short excerpt
  of stderr to help the user reproduce.
- `applicable()` returns `project.runner.is_some()`.
- `covers()` returns `&["p2-should-json-envelope-on-error"]`.

  **Patterns to follow:** `src/checks/behavioral/bad_args.rs` for error-induction; `json_output.rs:211-217` for JSON
  parsing.

  **Test scenarios:**
- **Happy path:** Tool emits JSON envelope on stdout on bad-arg + `--output json` → `Pass`, evidence "envelope on
  stdout".
- **Happy path:** Tool emits JSON envelope on stderr → `Pass`, evidence "envelope on stderr". (Anticipates the existing
  `p2-must-json-errors` text; supports either stream during the reconciliation window — see Scope Boundaries deferred
  item.)
- **Error path:** Tool emits human stderr only with no JSON anywhere → `Fail` with stderr excerpt.
- **Skip:** Tool's `--help` text doesn't mention `--output` → `Skip`.
- **Edge case:** `RunStatus::Crash` from the probe → `Skip` with `"probe crashed: <signal>"`.

  **Verification:** `cargo test` passes; `anc check .` emits the new ID.

- U5. **`p2-envelope-schema-uniform` check**

  **Goal:** Verify that a given action's JSON envelope keys are uniform across success and error — only the `status`
  value and the optional typed-reason key differ.

  **Requirements:** R5; covers `p2-should-output-envelope-schema-uniform`.

  **Dependencies:** U2, U4 (reuses error-induction technique).

  **Files:**
- Create: `src/checks/behavioral/envelope_schema_uniform.rs` (~120 LOC).
- Modify: `src/checks/behavioral/mod.rs` — register.
- Test: in-file `#[cfg(test)] mod tests`.

  **Approach:**
- Probe `<root> --help` to check `--output` support; `Skip` if absent.
- Run a known-success invocation: typically `<root> --help --output json` (gracefully handles when `--help` overrides
  `--output`; if so, `Skip` with "could not elicit success envelope safely"). Parse stdout JSON. Capture its top-level
  key set.
- Run an error invocation: bad-arg trigger + `--output json`. Parse the envelope wherever it appears (per U4 logic).
  Capture its top-level key set.
- Compare key sets. The error set must equal the success set, with up to two permitted differences: a `status` value
  change and the addition of a `reason` (or equivalent typed-error) key. Other key drift fails the check.
- Evidence on Fail lists the keys present in one envelope but not the other.
- `applicable()` returns `project.runner.is_some()`.
- `covers()` returns `&["p2-should-output-envelope-schema-uniform"]`.

  **Patterns to follow:** `json_output.rs` for parsing; U4 for error-induction. Set comparison via
  `std::collections::HashSet`.

  **Test scenarios:**
- **Happy path:** Success and error envelopes share the same key set modulo `status` and `reason` → `Pass`.
- **Error path:** Error envelope adds three new keys not present in success → `Fail` listing the divergent keys.
- **Error path:** Success envelope has key `data` that error envelope drops → `Fail` listing the missing key.
- **Edge case:** Success probe couldn't elicit a usable envelope (some tools refuse `--help --output json`) → `Skip`.
- **Skip:** Tool doesn't accept `--output` → `Skip`.

  **Verification:** `cargo test` passes; new ID in `anc check .` output.

- U6. **`p4-typed-error-reason` check**

  **Goal:** Verify that when `--output json` is set and the CLI hits an error, the JSON envelope includes a typed
  machine-readable `reason` (or equivalent) identifier — kebab-case or camelCase, not English prose.

  **Requirements:** R5; covers `p4-should-json-error-includes-typed-reason`.

  **Dependencies:** U2, U4 (reuses error-induction).

  **Files:**
- Create: `src/checks/behavioral/typed_error_reason.rs` (~80 LOC).
- Modify: `src/checks/behavioral/mod.rs` — register.
- Test: in-file `#[cfg(test)] mod tests`.

  **Approach:**
- Reuse error-induction from U4 to capture the error envelope.
- Walk the JSON top-level keys looking for any of: `reason`, `error_code`, `code`, `kind`, `error_kind`. Take the first
  match.
- Validate the field is a string and matches a typed-identifier pattern: lowercase ASCII alphanumeric plus `-`/`_`, no
  spaces, length ≤ 64, no sentence-shaped content. Use a simple regex check.
- `Pass` when a typed identifier is present. `Warn` (not `Fail`) when the field exists but contains English-prose style
  content (e.g., `"reason": "Destination directory is not empty"`) — the field exists but isn't typed. The warn signals
  fixable drift toward typed values without breaking the SHOULD check entirely. `Fail` when no candidate field is
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

  **Verification:** `cargo test` passes; new ID in `anc check .` output.

- U7. **Suppression entries, dogfood guards, and coverage-matrix regen**

  **Goal:** Add the audit-profile suppression mapping for the new checks, extend the dogfood guards to pin the new
  IDs against `anc` itself, and regenerate the committed coverage-matrix artifacts.

  **Requirements:** R6, R7, R8.

  **Dependencies:** U3-U6 all merged (or at least the new IDs and check IDs are stable).

  **Files:**
- Modify: `src/principles/registry.rs` — add `output_every_subcommand` check ID to the `FileTraversal` entry of
  `SUPPRESSION_TABLE` (lines 196-201). Document the other three checks as intentionally unsuppressed via a comment
  naming them. Test `every_suppression_table_check_id_exists` (or equivalent — verify exact name during implementation)
  confirms the entry.
- Modify: `tests/dogfood.rs` — add four new test functions: `dogfood_no_p2_should_output_every_subcommand_fail`,
  `dogfood_no_p2_should_json_envelope_on_error_fail`, `dogfood_no_p2_should_envelope_schema_uniform_fail`,
  `dogfood_no_p4_should_json_error_typed_reason_fail`. Each spawns `anc check . --output json` and asserts no `fail`
  status on the named check ID. Mirrors existing `dogfood_no_p2_fail_after_skill_subcommand` shape but names the check
  ID specifically.
- Modify: `docs/coverage-matrix.md` — regenerated by `anc generate coverage-matrix`. Manual prose summary at the top
  updates the requirement counts and notes the four new SHOULDs.
- Modify: `coverage/matrix.json` — regenerated by `anc generate coverage-matrix`.

  **Approach:**
- Run `cargo run -- generate coverage-matrix` to regenerate both artifacts. Review `git diff coverage/matrix.json` for
  the four new rows (now covered, not uncovered).
- For dogfood guards: each test follows the existing `dogfood_no_p2_fail_after_skill_subcommand` pattern at
  `tests/dogfood.rs:53-65` but filters results by exact check ID, not prefix. The exact-ID assertion is the intentional
  change — collective `p2-*` assertions hide which check newly fails.

  **Patterns to follow:** existing `SUPPRESSION_TABLE` entries; existing dogfood tests; existing coverage-matrix
  regeneration step.

  **Test scenarios:**
- **Happy path:** All four new dogfood tests pass against the current `anc` binary (proves the dogfood loop — `anc skill
  install` already implements all four contracts).
- **Drift catch:** `anc generate coverage-matrix --check` exits 0 after the regen commit; exits 2 if either artifact is
  stale.

  **Verification:**
- `cargo test` passes (all 519+ tests, including new dogfood tests).
- `anc generate coverage-matrix --check` exits 0.
- `anc check . --output json` shows all four new check IDs at status `pass` (or appropriately `skip`/`warn`).

---

## System-Wide Impact

- **Interaction graph:** new checks integrate at `src/checks/behavioral/mod.rs::all_behavioral_checks()`. No middleware
  or callback impact. The dogfood tests at `tests/dogfood.rs` extend existing patterns. Coverage matrix regeneration
  touches `docs/coverage-matrix.md` and `coverage/matrix.json` — both committed artifacts already drift-checked by CI.
- **Error propagation:** new checks return `CheckStatus` per the existing convention. Failures in error-induction probes
  (e.g., binary crashes) surface as `Skip` with structured evidence rather than propagating up.
- **State lifecycle risks:** none. Behavioral probes spawn the target binary with `BinaryRunner` which has built-in
  timeouts, output caps, and result caching. No persistent state.
- **API surface parity:** the new check IDs are external API surface (referenced by `coverage/matrix.json`, by consumer
  site `/coverage` page, by user-written `--audit-profile` consumers). Once published, IDs are stable — rename requires
  a deliberate spec edit and downstream coordination.
- **Integration coverage:** existing integration tests at `tests/integration.rs` will pick up the new checks
  automatically (any test that runs `anc check . --output json` and inspects `results[]` sees the new IDs). The
  `test_check_json_output` family already exercises this surface.
- **Unchanged invariants:**
- `arg_required_else_help` stays on `Cli`. The new behavioral checks must NOT bare-probe subcommands (per fork-
  bomb-safety rule).
- The four committed audit-profile categories. No fifth category.
- The schema 0.4 scorecard envelope. New requirement IDs flow through `coverage_summary` automatically.
- `audience` classifier signal IDs (`p1-non-interactive`, `p2-json-output`, `p7-quiet`, `p6-no-color-behavioral`). None
  of the four new checks are signal candidates.

---

## Risks & Dependencies

| Risk                                                                                                                                                                                                                                           | Mitigation                                                                                                                                                                                                                                           |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| The four new behavioral checks are the first to induce error paths in target binaries; the bad-arg injection technique has only been used in `bad_args.rs` for exit-code probing, never combined with `--output json`.                         | The combination is structurally safe (clap rejects before any subcommand handler runs), and `RunStatus::Crash` is handled explicitly. Each check's test suite includes a crash-handling scenario.                                                    |
| `anc` itself fails one of the new checks against itself, breaking the dogfood claim before the PR can land.                                                                                                                                    | The four SHOULDs codify behavior `anc skill install` already implements. If a new check fails against `anc`, that's a real bug to fix in the same PR (not a reason to weaken the check). The dogfood guards in `tests/dogfood.rs` catch this loudly. |
| `output-applies-to-every-subcommand` aggregates per-subcommand verdicts; the aggregation rule (fail-on-first vs allow-some-skip) affects how the check reports.                                                                                | Documented decision: fail when ANY subcommand rejects `--output`, name the offender(s) in evidence. Skip when there are no subcommands or no `--output` on root. The aggregation is conservative and human-readable.                                 |
| Existing `p2-must-json-errors` says "errors emitted as JSON to stderr"; `anc`'s actual envelope goes to stdout. The new SHOULD `json-envelope-on-error` is more permissive (envelope on either stream) but the MUST-vs-SHOULD tension is real. | Explicitly deferred — see Scope Boundaries. The new SHOULD permits either stream during the reconciliation window. A future spec PR reconciles the MUST text.                                                                                        |
| Counter-bumps in `registry_size_matches_spec` and `level_counts_match_spec` are easy to forget.                                                                                                                                                | CLAUDE.md notes the deliberate-act intent; U2 explicitly enumerates both counters; failing tests guide the implementer.                                                                                                                              |
| Coverage-matrix regen is an easy step to forget at PR time.                                                                                                                                                                                    | Existing CI integration test fails the PR if either artifact is stale. The regen step is U7's last task.                                                                                                                                             |
| `parse_subcommand_names()` parses help text heuristically; some CLI help formats may not match the patterns it recognizes.                                                                                                                     | The check returns `Skip` with structured evidence ("no subcommands detected") rather than `Fail` when enumeration fails. This biases conservative — better a missed `Pass` than a false `Fail`.                                                      |

---

## Documentation / Operational Notes

- The four new requirement IDs become external API surface as soon as the spec tag publishes. Any rename later requires
  a deliberate spec edit and consumer coordination (`agentnative-site` `/coverage` page reads the IDs from
  `coverage/matrix.json`; tools may pin against them in `--audit-profile` configurations).
- CLAUDE.md's "Principle Registry" section may grow a paragraph about the four new checks once they ship — the
  hand-maintained prose summary at the top of `docs/coverage-matrix.md` updates per the existing convention.
- Pre-release checklist (`RELEASES.md`) does not gain a new step. The existing `bash scripts/sync-spec.sh && git diff
  src/principles/spec/` step covers the new SHOULDs automatically.

---

## Sources & References

- Parent plan (just-shipped feature this follows up on):
  [docs/plans/2026-04-29-002-feat-skill-subcommand-plan.md](2026-04-29-002-feat-skill-subcommand-plan.md), Pattern
  Documentation Note section.
- Convention being codified: `docs/solutions/architecture-patterns/anc-cli-output-envelope-pattern-2026-04-29.md`.
- Cross-repo coordination playbook:
  `docs/solutions/best-practices/sot-contract-for-spec-repos-with-downstream-consumers-2026-04-22.md`.
- SRP for static-analysis checks:
  `docs/solutions/best-practices/reliable-static-analysis-compliance-checkers-20260327.md`.
- Behavioral phrasing for SHOULDs:
  `docs/solutions/best-practices/behavioral-vs-structural-must-when-authoring-spec-requirements-20260420.md`.
- Fork-bomb safety: `docs/solutions/logic-errors/cli-linter-fork-bomb-recursive-self-invocation-20260401.md`.
- Existing P2 behavioral check shape: `src/checks/behavioral/json_output.rs`.
- Existing P4 behavioral check shape: `src/checks/behavioral/bad_args.rs`.
- Registry conventions: `src/principles/registry.rs`, `src/principles/matrix.rs`.
- Spec authoring protocol: `agentnative-spec/principles/AGENTS.md`.
