---
date: 2026-05-01
topic: p2-json-output-upgrade
---

# p2-json-output upgrade — bad-arg-VALUE probe + Rust tightening + Python source sibling

## Summary

Upgrade `p2-json-output` (behavioral) by adding a bad-arg-VALUE probe as a third evidence path inside the existing
check; tighten `p2-structured-output` (Rust source) from "enum exists" to a tiered Strong / Medium / Weak detection that
uses reachability and flow analysis; ship a new Python source sibling (`structured_output.rs` for Python) that mirrors
the Rust shape via argparse / click detection. Verdicts shift on already-scored tools land deliberately, anchored by a
fixture matrix; the audience classifier is allowed to re-derive labels naturally.

---

## Problem Frame

`src/checks/behavioral/json_output.rs` currently emits warn for every JSON-supporting CLI when its safe-suffix probes
(`--help` / `--version`) cannot validate JSON — the dominant case, because terminal flags short-circuit before
`--output` is honored in most CLIs. This single warn caps `anc`'s own dogfood score at ~97% (project) / ~89% (binary)
and caps every JSON-supporting CLI on the leaderboard at warn on this dimension. The badge `score_pct` formula (`pass /
(pass + warn + fail)` per `src/scorecard/mod.rs`) means the warn directly drags the ratio down.

Two structural facts shape the remedy:

- A third safe-probe shape already exists in the codebase (`src/checks/behavioral/bad_args.rs`'s bad-arg trigger).
  Combining it with a deliberately- invalid value for a known flag (`<bin> --output __invalid_format_value__`) elicits
  the parser's *declared value enumeration* in stderr — clap, cobra, argparse, and click all echo the form `must be one
  of [text, json, yaml]` on this class of error. The parser rejects the value before any subcommand handler runs, so the
  probe is universally side-effect-safe.
- A source-layer Rust check (`p2-structured-output`) already exists and covers the same requirement
  (`p2-must-output-flag`). Today it returns Pass when an `enum OutputFormat` or `enum Format` is detected — but says
  nothing about whether the enum is wired to a serializer. Source layer can do strictly more than behavioral via
  reachability and flow analysis; today it does less.

Python has no equivalent source-layer check at all (`src/checks/source/python/` covers `no_color`, `bare_except`,
`sys_exit`).

---

## Requirements

**Behavioral check upgrade (`p2-json-output`)**

- R1. Add a bad-arg-VALUE probe to `validate_json_output()` as a third evidence path, alongside the existing `--help`
  and `--version` safe-suffix probes. The probe injects a deliberately-invalid value for the detected `--output` /
  `--format` flag and parses stderr for the declared value enumeration.
- R2. The behavioral check's verdict semantics shift in-place (same check ID `p2-json-output`, same `covers()`
  declaration). When the value-enum echo confirms `json` is a declared accepted value, the verdict becomes Pass at
  `Confidence::Medium` (consistent with `p1-env-hints` v0.1.3 widening precedent — widening detection does NOT raise
  confidence).
- R3. When the bad-arg-VALUE probe cannot fire (no flag detected, parser rejects with a format the regex doesn't
  recognize, or the binary errors before parse), the check falls back to the existing safe-suffix probes and emits the
  existing Warn evidence message. No regression on currently-passing tools.

**Rust source tightening (`p2-structured-output`)**

- R4. Replace the binary "enum exists or not" detection with a tiered model: Strong (Confidence::High) when the
  OutputFormat enum has a `Json` variant AND a clap-derive field references it AND a `serde_json` serialization call
  site (`to_writer`, `to_string`, `to_string_pretty`, etc.) is reachable from the match arm gating that variant; Medium
  (Confidence::Medium) when enum + clap reference + any `serde_json` call in the same crate (looser reachability); Weak
  / Warn when only the enum exists with no serde_json call site.
- R5. Tools that previously passed at `Confidence::High` purely on enum existence shift to a tier consistent with their
  actual reachability. The shift is anchored by the fixture matrix (R12) and documented in the PR description.
- R6. The check ID stays `p2-structured-output`; `covers()` stays `&["p2-must-output-flag"]`. No new requirement IDs
  added to `src/principles/registry.rs`.

**Python source sibling (new check)**

- R7. Add a Python source-layer check that mirrors the Rust shape, applicable when `Project::language` is Python.
  Detects argparse `add_argument(..., choices=[..., 'json', ...])` / click `Choice([..., 'json', ...])` declarations + a
  `json.dumps` / `json.dump` call site reachable from the dispatch on that argument's value.
- R8. Detection produces the same tiered Strong / Medium / Weak verdict shape as Rust (R4), with the same Confidence
  levels and the same Warn message when only the choice declaration exists without a `json.dumps` call site.
- R9. The Python check declares `covers() = &["p2-must-output-flag"]` and registers in `src/checks/source/mod.rs` (or
  wherever the Python check catalog lives). The `dangling_cover_ids` test
  (`src/principles/matrix.rs::live_catalog_has_no_dangling_cover_ids`) passes after the addition.

**Cross-cutting**

- R10. The audience classifier in `src/scorecard/audience.rs` is unchanged; verdict shifts on `p2-json-output` cause
  natural re-derivation of audience labels. The changelog and the merging PR description explicitly enumerate which
  currently-scored tools shift audience labels and which way.
- R11. No scorecard schema bump. `schema_version` stays at `"0.5"`. The envelope contract is unchanged; only the
  verdicts flowing through it change.
- R12. A fixture matrix anchors the verdict shift before merge: at minimum, behavioral fixtures cover
  currently-warn-capped tools across clap, cobra, argparse, and click parser families (concrete tool list resolved
  during planning); source fixtures cover Rust tools with each tier of reachability (Strong / Medium / Weak) and Python
  tools with each tier. Each fixture documents the expected verdict and Confidence before the detection lands.
- R13. Dogfood guards (`tests/dogfood.rs`) update to reflect the new verdict for `p2-json-output` against `anc` itself
  (Pass at `Confidence::Medium`) and the tier `p2-structured-output` resolves to (Strong, given `anc`'s `OutputFormat`
  enum is wired to `serde_json::to_string_pretty` via the scorecard module).
- R14. Coverage matrix artifacts (`docs/coverage-matrix.md`, `coverage/matrix.json`) are regenerated via `anc emit
  coverage-matrix` as part of the same PR. The integration test
  `test_generate_coverage_matrix_drift_check_passes_on_committed_artifacts` passes.

---

## Acceptance Examples

- AE1. **Covers R1, R2.** Given a clap-derived CLI with `#[arg(long = "output", value_enum)] format: OutputFormat` and
  `enum OutputFormat { Json, Text }`, when `anc audit` runs `<bin> --output __invalid_format_value_agentnative_probe__`
  and the binary emits `error: invalid value '__invalid_format_value_agentnative_probe__' for '--output <FORMAT>': must
  be one of [text, json]` to stderr, then `p2-json-output` returns Pass at `Confidence::Medium` with evidence message
  naming the probe shape and the declared values.
- AE2. **Covers R3.** Given a CLI whose `--output` flag accepts a string but rejects invalid values with a free-form
  message that doesn't echo the declared enumeration (e.g., a custom Go parser printing "unknown output format"), the
  bad-arg-VALUE probe fails to extract a value list; `p2-json-output` falls back to the existing Warn with the existing
  evidence message. No regression.
- AE3. **Covers R4 (Strong tier).** Given a Rust crate with `enum OutputFormat { Json, Text }`, a clap field `output:
  OutputFormat`, a match arm `OutputFormat::Json => { serde_json::to_string_pretty(&result)? }`, when
  `p2-structured-output` runs against it, then the verdict is Pass at `Confidence::High`.
- AE4. **Covers R4 (Medium tier).** Given a Rust crate with `enum OutputFormat { Json, Text }`, a clap field referencing
  it, but the `serde_json::to_string` call lives in a helper module not directly reachable from the match arm via local
  AST traversal, when `p2-structured-output` runs, then the verdict is Pass at `Confidence::Medium`.
- AE5. **Covers R4 (Weak / Warn).** Given a Rust crate with `enum OutputFormat { Json }` declared but no `serde_json`
  call site anywhere in the crate, when `p2-structured-output` runs, then the verdict is Warn with evidence naming the
  detection gap.
- AE6. **Covers R7, R8.** Given a Python project with `parser.add_argument('--output', choices=['text', 'json'])` and a
  dispatch `if args.output == 'json': print(json.dumps(result))`, when the new Python source check runs, then the
  verdict is Pass at `Confidence::High`.

---

## Success Criteria

- `anc audit .` against the agentnative-cli repo reports `p2-json-output` at Pass (Confidence::Medium) and
  `p2-structured-output` at Pass (Confidence::High) — both contributing to the badge `score_pct` numerator. The dogfood
  project-mode score moves meaningfully above the current ~97% cap; binary-mode score moves above the current ~89% cap.
- Every fixture in the new fixture matrix produces its documented expected verdict exactly. No silent regression on
  currently-scored tools.
- The PR description explicitly enumerates audience-classifier label shifts; reviewers can verify each shift is a
  correctness win, not a regression.
- Downstream `/ce-plan` can write an implementation plan from this requirements doc without inventing product behavior,
  scope boundaries, or success criteria. The four R-groups (behavioral upgrade, Rust tightening, Python sibling,
  cross-cutting) are the natural Implementation Units.
- The follow-up P0 todo in the spec repo (`~/dev/agentnative`) tracking the broader version-bump-policy question is
  filed and discoverable by future agents working on spec or scorecard changes.

---

## Scope Boundaries

- No new requirement IDs added to `agentnative-spec`. A+B' attest the existing `p2-must-output-flag` MUST more
  rigorously; no spec PR.
- No `agentnative-spec` version bump. The vendored spec text is unchanged.
- No scorecard schema bump (stays `"0.5"`). The envelope contract is unchanged.
- No self-declared manifest field (`[package.metadata.agentnative]` author opt-in, ideation D). Defer.
- No `<tool> agentnative-probe` spec convention (ideation F). Long-term ecosystem move.
- No spec MUST rewriting (ideation E, adversarial-rejected: doctrine says downgrade prose to preference, not level of
  MUST).
- No Skip-with-evidence verdict change (ideation G). Audience-let-it-shift covers the ground.
- No change to `audience::SIGNAL_CHECK_IDS`. Classifier re-derives labels naturally.
- No CommandFactory build-time shim (ideation F6.9). Cross-compilation cost too high.
- No `jc` external-wrapper fallback (ideation F2.7 / F6.11). Subject-tangential.
- No coupling with active output-envelope plan (`docs/plans/2026-04-30-001`). Bad-arg probe primitive may be shared at
  implementation time but the two plans don't cross-depend at the requirements level.
- No third source language (Go, Bun, etc.) at this iteration. Rust + Python are the source-check launch languages per
  `Cargo.toml` feature flags.

---

## Key Decisions

- A's verdict lands in-place on the existing `p2-json-output` check (third evidence path), not as a new check ID.
  Rationale: A detects the same property the existing check claims to detect — flag honors JSON output — just via a
  different probe shape. `p1-env-hints` v0.1.3 is direct precedent for this widening pattern; SRP-per-check doctrine is
  about properties not probe shapes.
- Source-layer detection becomes asymmetrically more nuanced than behavioral via reachability and flow analysis. Source
  can attest things behavioral cannot (declaration wired to a serializer, not just declared) — and per `widening
  detection does NOT raise confidence` doctrine, behavioral stays at Medium while source can earn High when reachability
  is provable.
- Audience-classifier shifts are accepted as correctness wins, not pinned away. Tools previously under-read (warn-capped
  despite advertising JSON output) becoming `agent-optimized` is the right direction.
- No scorecard / spec version bump. Verdict shifts on already-scored tools are normal release-note territory; the
  envelope contract and the requirement IDs are unchanged.
- Plan landing target is a NEW top-level plan
  (`docs/plans/2026-05-01-NNN-feat-p2-json-output-bad-arg-and-source-tightening-plan.md`), not a fold-in to active
  output-envelope plan `2026-04-30-001`. A+B' don't add new spec SHOULDs.

---

## Dependencies / Assumptions

- The covers()-OR coverage logic in `src/principles/matrix.rs::build` is the shipped design (verified during
  brainstorm); no registry change required for multiple checks covering one requirement.
- `BinaryRunner::run(args, env)` already exposes the primitive needed for the bad-arg- VALUE probe. No runner change
  required.
- ast-grep's Rust language support (via the `tree-sitter-rust` feature flag) and Python language support are already
  wired in `Cargo.toml`. No dependency add.
- The bad-arg probe primitive may be shared with the active output-envelope plan (`docs/plans/2026-04-30-001`) U4
  (`p2-json-envelope-on-error`) at implementation time. Coordination is implementation-level only; the two plans do not
  block each other.

---

## Outstanding Questions

### Resolve Before Planning

(none — all scope-shaping decisions resolved during brainstorm)

### Deferred to Planning

- `Affects R12` · `Needs research` — Exact list of currently-warn-capped tools to anchor the fixture matrix. Resolve by
  querying the latest leaderboard data and grouping by parser family (clap, cobra, argparse, click, custom).
- `Affects R4` · `Technical` — Concrete ast-grep patterns for the Strong-tier reachability check in Rust. The
  match-arm-to-call-site traversal is the load-bearing detection step; expect iteration during implementation.
- `Affects R7` · `Technical` — Concrete ast-grep patterns for argparse `choices=` detection in Python, and click's
  `Choice([...])` shape. argparse exposes `choices` as a list literal; click wraps it in a `Choice` constructor. Both
  must be supported.
- `Affects R10` · `Operational` — Concrete enumeration of which currently-scored tools shift audience labels. Resolve by
  running `anc audit` against each leaderboard member with the new detection enabled and diffing the audience field;
  cite the diff in the PR description.
- `Affects R12` · `Operational` — Whether the fixture matrix lives under `tests/fixtures/known-shapes/<tool>/` (per the
  ideation F4.7 shape) or extends the existing `tests/fixtures/` layout. Lightweight infrastructure decision; planner
  decides.
