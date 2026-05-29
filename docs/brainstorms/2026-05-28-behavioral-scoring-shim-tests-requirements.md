---
title: "Hermetic shim-binary smoke tests for behavioral-layer scoring"
date: 2026-05-28
topic: behavioral-scoring-shim-tests
---

# Hermetic shim-binary smoke tests for behavioral-layer scoring

## Summary

A deterministic, cross-platform smoke test for `anc`'s behavioral-layer scoring, living in this repo as a test-only
build target isolated from the CLI's default build/dev loop. A Rust test-binary shim presents tool "shapes" drawn from a
declarative table; `anc` audits each one and the result is compared against a committed golden of the projected scoring
view (the per-row `{id → status}` map, badge eligibility/score, and exit code). Realistic tool personas back the table,
with a coverage gate that proves every behavioral audit is exercised.

## Problem Frame

`anc` scores a target in two layers: source audits (ast-grep over Rust/Python) and behavioral audits (spawn the binary,
probe `--help`/`--version`/flags). The source layer has hermetic project fixtures (`tests/fixtures/perfect-rust`,
`broken-rust`, `source-only`, `broken-python`). The behavioral layer does not have an equivalent for *scoring* outcomes.

Today, behavioral scoring is verified two indirect ways, and neither covers the real path end-to-end:

- **Real PATH tools** — integration tests audit `--command ls` and (manually) `bat`. These depend on whichever version
  of the tool is installed on the machine, and a tool changing its `--help` text silently drifts the expected verdicts.
- **Synthetic unit tests** — the per-row + antecedent-propagation pipeline (the `opt_out` antecedent collapsing a
  conditional MUST to `n_a`) is exercised only by `FakeAudit` unit tests that never spawn a real binary. The behavior
  that shipped in PR #63 (text/JSON per-row parity) has no test that drives it through an actually-spawned executable.

The cost is a blind spot exactly where `anc`'s value lives: the verdict a human or agent reads for a real tool. A
regression in how a spawned binary's probe output maps to per-row statuses, badge eligibility, or exit code would pass
the current suite. Existing prior art shows the shape of the fix is already in the repo's vocabulary —
`binary-only/test.sh` is a known-shape shim and the `hostile-*/probe.sh` trio are probe-robustness shims — but nothing
keys a shim to a *scoring* outcome and asserts it.

## Key Decisions

- **Declarative table as the single source of truth.** One table declares, per shape, both the probe behavior (help
  text, flags, subcommand help, exit codes) and the expected projected scoring view. Unifying input and expected output
  in one place means they cannot silently diverge, and it mirrors the repo's existing committed-artifact + `--check`
  drift convention (`anc emit coverage-matrix`).

- **Rust test-binary substrate, not shell shims.** The shim is a built Rust target so the suite runs on every CI
  platform including Windows, with no shebang/exec-bit issues and no `#[cfg(unix)]` gating. The existing `.sh` fixtures
  are unix-fragile — `test_binary_only_fixture` audits a `.sh` and is not even `cfg`-gated, a latent inconsistency this
  approach avoids.

- **Projected scoring view, not full-scorecard golden.** The golden captures only what "scoring correctness" means — the
  per-row `{id → status}` map, badge `{eligible, score_pct}`, and exit code — and ignores volatile run metadata
  (duration, timestamp, paths, probed version). This keeps golden diffs low-noise and avoids scrubbing machinery for
  metadata other tests already cover.

- **Personas + coverage gate, not one shim per audit.** Realistic tool personas back the table; a coverage gate proves
  completeness. Pinpointing "which audit broke" comes from the per-row golden diff (a specific row id flips inside a
  specific persona's golden), not from isolating one shim per audit — so the suite gets exhaustiveness without the
  carrying cost of dozens of narrow, unrealistic shims, and without pretending to isolate statuses that only arise in
  combination.

- **In-repo, test-only build target.** Keeping the shim, the table, the golden, and the audits in one repo lets an audit
  change land with its shim shape and golden regen in a single atomic PR. A separate repo would fracture that change
  across repos and reintroduce the cross-repo-sync tax. The guardrail that protects CLI development is the build
  isolation in R10.

## Requirements

### Shapes and shim

- R1. A Rust test-binary shim presents a selectable "shape" (tool persona); its `--help`/`--version`/flag/exit behavior
  is chosen per invocation so one executable stands in for many tool profiles.
- R2. Shapes are defined in a single declarative table that is the source of truth for each shape's probe behavior and
  its expected projected scoring view.
- R3. Shapes model realistic tool personas (for example: a structured-output "good citizen", an opt-out tool, a mixed
  warn/fail tool), not one narrow shim per audit.

### Golden and assertions

- R4. Each shape has a committed golden capturing the projected scoring view: the per-row `{id → status}` map, badge
  `{eligible, score_pct}`, and the process exit code. Volatile run metadata (duration, timestamp, paths, probed version)
  is excluded.
- R5. A test audits each shape's shim through `anc`'s behavioral path and asserts the produced projected view equals the
  committed golden.
- R6. Goldens are updated by a deliberate, reviewed regen step, with a `--check`-style drift mode that fails when a
  committed golden disagrees with current output — mirroring the `anc emit coverage-matrix` artifact lifecycle.

### Coverage completeness

- R7. A coverage gate asserts that the union of all shapes' rows exercises every behavioral audit id, in at least one
  pass state and at least one reachable non-pass state.
- R8. The gate fails the build when a newly added behavioral audit is exercised by no shape, so coverage gaps cannot
  land silently.

### Build isolation and placement

- R9. The shim and harness live in this repo.
- R10. The shim is a test-only build target: the default `cargo build` / `cargo run` does not compile it, and it adds
  zero runtime dependencies to the `anc` binary.
- R11. The suite runs on all CI platforms including Windows without `#[cfg(unix)]` gating.

## Acceptance Examples

- AE1. Opt-out persona. **Given** a shape whose help advertises no `--output`/`--format` flag (but mentions JSON),
  **when** `anc` audits it, **then** `p2-must-output-flag` is `opt_out` and the conditional `p2-must-schema-print`
  propagates to `n_a`, and the exit code reflects the per-row truth (the raw-Fail consequent does not lift it). **Covers
  R3, R4, R5.**
- AE2. Good-citizen persona. **Given** a shape that advertises `--output json` and a runtime schema surface, **when**
  audited, **then** the projected view shows a high pass rate and the badge is `eligible: true` with `score_pct` at or
  above the floor. **Covers R3, R4.**
- AE3. Coverage gate catches an unexercised audit. **Given** a newly added behavioral audit that no shape triggers,
  **when** the suite runs, **then** the coverage gate fails naming the uncovered audit id. **Covers R7, R8.**
- AE4. Golden drift gate. **Given** a shape's probe behavior changed without regenerating its golden, **when** the drift
  `--check` runs, **then** it fails showing the row(s) whose status changed. **Covers R6.**

## Scope Boundaries

- Source-layer audits (Rust/Python ast-grep) are out of scope — the accepted behavioral-only limitation; existing
  project fixtures cover the source layer.
- The real-tool scoring tests (`--command ls` parity, `bat`) stay as integration-drift coverage running alongside the
  shims; this suite does not replace them.
- Literal one-shim-per-audit isolation is rejected in favor of personas + a coverage gate (see Key Decisions).
- A separate repo for the shim is rejected for the first version; it is the fallback only if the build-isolation
  constraint (R10) ever proves too costly to maintain in-repo.

## Dependencies / Assumptions

- The shim is audited through `anc`'s existing behavioral entry point (a binary target resolved via `--command` or a
  path, which runs the behavioral-only layer). It reuses the probe-safety conventions in the repo's dogfooding rules —
  notably never probing subcommands without `--help`/`--version` suffixes.
- Some statuses are only reachable in combination (for example `n_a` requires an `opt_out` antecedent feeding a
  conditional consequent; envelope-consistency requires JSON across multiple probes). R7's "reachable non-pass state"
  wording acknowledges this: completeness is "every audit exercised across the persona set", not "every status of every
  audit producible in isolation".

## Outstanding Questions

Deferred to planning (resolved during planning or codebase exploration, not blocking):

- The exact cargo target type for the shim (workspace member, `example`, or feature-gated `[[bin]]`) and how the test
  harness locates the built shim binary, subject to the R10 isolation constraint.
- The minimal persona set that satisfies the R7 coverage gate, and an enumeration of which behavioral audits are
  reachable via a single shape versus only via a persona combination.
- The golden file layout (one file per persona versus one combined snapshot) and the surface of the regen command.

## Sources / Research

- Existing shim prior art: `tests/fixtures/binary-only/test.sh` (known-shape shim; note its test
  `test_binary_only_fixture` is not `cfg`-gated), `tests/fixtures/hostile-hang/probe.sh`,
  `tests/fixtures/hostile-nonzero-exit/probe.sh`, `tests/fixtures/hostile-stdout-flood/probe.sh` (probe-robustness
  shims).
- Integration patterns: `tests/integration.rs` — the `--command ls` tests (gated `#[cfg(unix)]`) and
  `test_text_and_json_agree_on_row_count_and_exit_code` (the per-row parity guard added in PR #63).
- Scoring pipeline: `src/scorecard/mod.rs` — `build_row_results`, `fan_out_per_row`, `propagate_antecedents`,
  `compute_badge`, `score_pct`, `exit_code`, `build_scorecard`.
- Behavioral audits catalog: `src/audits/behavioral/` (~45 audits); requirement registry: `src/principles/registry.rs`.
- Drift-gate convention to mirror: `anc emit coverage-matrix --check` (committed `docs/coverage-matrix.md` +
  `coverage/matrix.json` with a drift check).
- Origin: PR #63 (text/JSON per-row parity) surfaced that behavioral scoring is verified only via real PATH tools plus
  synthetic `FakeAudit` unit tests, never end-to-end through a spawned binary.
