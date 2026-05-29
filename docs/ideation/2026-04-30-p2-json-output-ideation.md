---
date: 2026-04-30
topic: p2-json-output-upgrade
focus: Graduate `src/audits/behavioral/json_output.rs` from "warn" to "pass" without breaking dogfood/side-effect/comparability/spec-authority constraints
mode: repo-grounded
---

# Ideation: p2-json-output upgrade — warn → pass without breaking constraints

The audit at `src/audits/behavioral/json_output.rs:201` currently emits warn for every JSON-supporting CLI:
*"--output/--format flag detected but could not validate JSON via safe probes (--help/--version override output flags in
most CLIs)"*. This caps `anc`'s own dogfood at 97% (project) / 89% (binary) and every JSON-supporting CLI on the
leaderboard at warn on this dimension.

Six survivors after adversarial filtering of 69 raw candidates across 6 ideation frames. Top-2 emerged from the
post-adversarial re-rank as a coupled pair (A+B), which the user selected to brainstorm next.

## Grounding Context

### Codebase

- `BinaryRunner::run(args, env)` and `run_partial` are the existing safe-spawn primitives. Cache by `(args, env)`,
  NO_COLOR=1, timeout, partial-read with SIGPIPE.
- Three probe shapes already exist: `--help`/`--version` (safe suffixes), bad-arg trigger
  (`--this-flag-does-not-exist-agentnative-probe` from `bad_args.rs` — clap rejects before any handler), env-var
  injection.
- `arg_required_else_help = true` on `Cli` is load-bearing (fork-bomb safety on dogfood). CLAUDE.md "Dogfooding Safety"
  codifies the two non-negotiables: never bare-probe subcommands; never remove `arg_required_else_help`.
- `Confidence::High/Medium/Low` already exists on `AuditResult`.
- `covers()` on each `Audit` declares which requirement IDs it evidences; drift detector at
  `src/principles/matrix.rs::dangling_cover_ids` enforces.
- Active plan `docs/plans/2026-04-30-001-feat-spec-output-envelope-shoulds-plan.md` adds four new SHOULDs (U3-U6)
  including `p2-should-json-envelope-on-error` that pioneers the bad-arg + `--output json` shape for error-path probing.

### Institutional doctrine (load-bearing)

- Aggregate verdicts are informational, never authoritative
  (`docs/solutions/architecture-patterns/aggregate-verdicts-are-informational-not-authoritative-20260420.md`).
- Behavioral vs structural MUST: split via new audit IDs at the right layer; don't deepen one audit
  (`docs/solutions/best-practices/behavioral-vs-structural-must-when-authoring-spec-requirements-20260420.md`).
- Reliable static-analysis: SRP per audit; one audit, one property; multi-signal scoring hides which signal failed
  (`docs/solutions/best-practices/reliable-static-analysis-compliance-auditors-20260327.md`).
- Audit scripts are documentation's immune system: when a rule isn't enforceable, downgrade PROSE to preference — not
  the LEVEL of an existing MUST
  (`docs/solutions/best-practices/audit-scripts-as-documentation-immune-system-2026-04-20.md`).
- Direct precedent: `p1-env-hints` v0.1.2 → v0.1.3 widened from clap `[env: FOO]`-only to clap+prose-pattern detection,
  stayed at `Confidence::Medium`. Two regressions surfaced — fixture-driven leaderboard anchoring matters
  (`docs/solutions/best-practices/cli-env-var-shape-heuristic-2026-04-21.md`).

### External grounding

- `cargo metadata --format-version=1` — Rust ecosystem precedent for opt-in safe machine-readable subcommand
- `clap::CommandFactory` — Rust-only build-time schema extraction without execution
- `terraform show -json <plan-file>` — read static artifact, never live command
- HTTP safe-vs-idempotent-vs-unsafe taxonomy — formal vocabulary for probe classification
- SQL `EXPLAIN` vs `EXECUTE` — exercise argument-parsing path without execution path

## Ranked Ideas

### 1. A — Tolerance-gauge bad-arg-VALUE probe (X1)

**Status:** Explored

**Description:** Extend `validate_json_output()` in `src/audits/behavioral/json_output.rs` with a third safe probe
shape: invoke `<bin> --output __invalid_format_value_agentnative_probe__`. clap, cobra, and argparse all respond to
known-flag-with-invalid-value with a parse error that includes the *declared value enumeration* (`error: invalid value
'__invalid__' for '--output <FORMAT>': must be one of [text, json, yaml]`). Parse stderr for the value list; pass if
`json` is enumerated. Side-effect-safe by construction — the parser rejects the value before any subcommand handler
runs.

**Warrant:** `direct:` `bad_args.rs` already uses a parallel technique for exit-code probing; the docstring at
`json_output.rs:165-169` explicitly names the safe-suffix set as expandable. The active output-envelope plan U4
documents this exact probe-pattern as universally side-effect-free. `external:` clap's `[possible values: ...]`
rendering is a documented stable surface across versions; argparse's `choices=` rendering is similarly stable; cobra's
`pflag` exposes the same shape.

**Rationale:** Materially stronger than today's safe-suffix probes — doesn't just confirm the flag exists, it confirms
`json` is a *declared accepted value* of that flag.

**Downsides:** Doesn't validate the CLI actually emits JSON when honoring the flag (only that it declares acceptance).
Some CLIs may not echo their value enumeration on invalid-value errors (older Go cobra, custom parsers, click).
Fixture-driven leaderboard anchoring (per `p1-env-hints` v0.1.3 lesson) is required to avoid silent regressions on
currently-scored tools.

**Confidence:** 80% **Complexity:** Low (~150 LOC + fixture matrix)

### 2. B — Sibling source-layer audit `p2-json-source-evidence` with covers()-OR (X2)

**Status:** Explored

**Description:** New source-layer audit (Rust + Python via existing ast-grep infra) detecting (1) clap `ValueEnum` with
`Json` variant gating output, OR (2) `serde_json::to_writer`/`json.dumps` reachable from `--output`/`--format` argument
handler. Returns its own verdict. Declares `covers() = &["p2-must-output-flag"]` — the SAME requirement ID
`p2-json-output` (behavioral) covers. Coverage matrix at the registry layer credits the requirement as verified when
either covering audit passes (covers()-OR semantics at the requirement layer, NOT verdict aggregation at the audit
layer).

**Warrant:** `direct:` CLAUDE.md "Source Audit Convention" is the project's pre-committed shape for cross-layer
evidence. `behavioral-vs-structural-must-when-authoring-spec-requirements-20260420.md` explicitly says: when a
behavioral audit can't safely attest, ADD a source-layer sibling — don't deepen the behavioral one.

**Rationale:** Lifts the dogfood cap for Rust/Python CLIs (the bulk of the leaderboard) without touching behavioral
safety. `anc` itself is Rust → its source unambiguously confirms a `clap::ValueEnum` with `Json` variant + `serde_json`
adapter → source audit passes → `p2-must-output-flag` requirement is verified-via-source even when the behavioral audit
stays at warn.

**Downsides:** Language-restricted (Rust + Python today). False confidence from declared-but-unwired adapter is
possible; B stays at `Confidence::Medium` and evidence message names the source path it found. Requires confirmation
that the registry's coverage logic is OR-semantics across multiple covering audits (likely already true per CLAUDE.md
"covers() Declaration" wording, but needs explicit drift-test).

**Confidence:** 75% **Complexity:** Medium (~300 LOC + new source audit file + drift test for covers()-OR semantics +
spec note documenting the dual-layer pattern)

### 3. D — Self-declared manifest field opt-in (C5)

**Status:** Unexplored

**Description:** CLI authors opt in via `[package.metadata.agentnative]` (Cargo.toml) / `[tool.agentnative]`
(pyproject.toml) with `json_probe = ["render", "--format", "json"]` declaring a safe-probe argv. The audit spawns
exactly that argv via `BinaryRunner`, parses stdout as JSON, passes if valid. No manifest → falls through to A's
behavioral probe.

**Warrant:** `external:` `cargo metadata --format-version=1` ships dedicated safe-probe paths via
`[package.metadata.*]`; cargo-deny, cargo-release, cargo-dist all use the convention.

**Rationale:** Highest-comparability evidence path (author-attested, deterministic across runs).

**Downsides:** Adoption gap. Useful as complement to A+B, not replacement.

**Confidence:** 70% **Complexity:** Medium (~200 LOC + new requirement registration + spec note)

### 4. E — Spec-side narrowing folded into active output-envelope plan (X5) — QUESTIONED

**Status:** Unexplored — recommend deferring or replacing

**Description:** Rewrite `p2-must-output-flag` MUST as "advertises JSON output via `--output|--format` flag with `json`
in declared value-set." Honor-the-flag becomes SHOULDs in active U4-U6.

**Warrant:** `reasoned:` MUST-level signals critical capability; if it can't be safely attested, level may be wrong.

**Downsides (adversarial round):** Per `audit-scripts-as-documentation-immune-system-2026-04-20.md`, the doctrine is
"downgrade PROSE to preference, not LEVEL of MUST." Demoting the MUST→SHOULD weakens enforcement during the gap window
between U4-U6 spec landing and U4-U6 audits shipping. Cleaner moves: keep MUST and let A+B carry honest attestation, OR
delete MUST entirely and let U3-U6 carry the surface. E's halfway demote is worst of both worlds.

**Confidence:** 35% (post-adversarial; was 75% before) **Complexity:** Lowest (spec PR + counter bumps)

### 5. F — `<tool> agentnative-probe` spec convention (F4.5)

**Status:** Unexplored — long-term direction

**Description:** Spec proposes a self-describing introspection subcommand: any CLI MAY ship `<tool> agentnative-probe
--output json` returning a known JSON envelope.

**Warrant:** `external:` `cargo metadata --format-version=1` precedent.

**Rationale:** Highest ceiling. Compounds across ecosystem. Belongs after A+B prove demand.

**Downsides:** Slowest adoption. Doesn't lift today's leaderboard cap.

**Confidence:** 65% **Complexity:** High

### 6. G — Skip-with-evidence + scoring layer recognizes Skip (C7)

**Status:** Unexplored — recommend as complement

**Description:** Warn → Skip with rich evidence. Scoring layer treats Skip as "no opinion" — excluded from BOTH
numerator AND denominator of pass-rate.

**Warrant:** `direct:` existing `--audit-profile` precedent skips audits that don't apply rather than warning.

**Downsides:** Hides the warn from leaderboard rendering. Better as complement to A+B (Skip when both can't fire) than
standalone.

**Confidence:** 55% **Complexity:** Low

## Top-2 selected for brainstorm: A+B

A handles cross-language safe-probe upgrade. B handles Rust/Python source-grounded upgrade. Independent — neither blocks
the other. Each gives its own honest verdict. Threads SRP-per-audit + behavioral/source split + no-verdict-aggregation +
requirement-level coverage-OR simultaneously.

### Failure modes carried by A+B (per handoff exit criterion)

- **A's failure mode:** Silent regressions on CLIs whose value-enum echo format differs from clap's (older Go cobra,
  custom parsers, Python click). The `p1-env-hints` v0.1.3 precedent surfaced two regressions of this exact shape — only
  fixture-driven leaderboard anchoring caught them. A's plan must include a fixture matrix keyed off currently-scored
  tools BEFORE detection widens.
- **B's failure mode:** False confidence from declared-but-unwired adapter. A Rust crate with `serde_json` in
  `Cargo.toml` and `clap::ValueEnum` with `Json` variant could ship a buggy `format` match arm that silently falls
  through to Display. B passes; A fails; the leaderboard renders the conflict honestly via per-audit verdicts (which is
  exactly the design); but a casual reader sees "B passes, requirement covered" and concludes the CLI is fine.
  Mitigation: B stays at `Confidence::Medium`, evidence message names the source path it found.

## Rejection Summary

Sub-agents generated 64 raw + 5 cross-cutting = 69 candidates. 63 rejected.

| #                          | Idea                                                                            | Reason                                                 |
| -------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------ |
| F2.7 / F6.11               | jc external wrapper                                                             | Subject-tangential                                     |
| F6.1                       | anc-probe sibling binary                                                        | Too expensive vs value                                 |
| F6.5 / F3.8                | Landlock/seccomp sandbox                                                        | Heavy infra; doesn't address verdict-authority         |
| F6.6                       | Per-CLI baseline regression                                                     | Sacrifices comparability for trend                     |
| F6.7                       | Adversarial schema-witness                                                      | No warrant for current threat model                    |
| F6.8                       | 100ms time-cap probe                                                            | Latency orthogonal to JSON property                    |
| F6.9                       | CommandFactory shim                                                             | Cross-compilation hell                                 |
| F1.11                      | Operator --probe-aggressive                                                     | Violates comparability constraint                      |
| F1.6                       | Stdin empty-payload probe                                                       | Narrow + brittle                                       |
| F2.5                       | Dry-run sentinel probe                                                          | Violates `no-flags-are-circuit-breakers`               |
| F1.5                       | Read-only subcommand probe                                                      | Duplicates F                                           |
| F2.2                       | Conventional subcommand allowlist                                               | Heuristic guessing                                     |
| F2.9                       | Carapace/argc completion corpus                                                 | Third-party SHA dependency                             |
| F4.6                       | clap_complete artifact parsing                                                  | Duplicates B narrowly                                  |
| F3.1                       | Move audit to Project layer                                                     | Duplicates B weakly                                    |
| F4.7 / F1.7 / F2.8 / F1.13 | Fixture-driven verdict variants                                                 | Duplicates of D/A                                      |
| F6.2                       | N=3 consistency probe                                                           | Orthogonal property                                    |
| F2.10                      | Delete probe; trust vocabulary                                                  | Too weak alone                                         |
| F1.12 / F3.5               | Improve evidence string / rebrand UI                                            | Incremental presentation-only                          |
| F2.11 / F3.7               | Cross-check triangulation in derived block                                      | Violates `aggregate-verdicts-not-authoritative`        |
| F5.2-9                     | ELISA / ZK / abstract-interpretation / refinement-types / OPA-Rego / traceroute | Duplicates of A with weaker framing                    |
| F4.8                       | Probe cache as ProbeContext                                                     | Supporting infra                                       |
| F4.1 / F4.2                | ProbeKind enum / shared help_parser                                             | Supporting infra subsumed by A                         |
| F3.10 / F3.9               | Split into N audits / parseability of error envelopes                           | Subsumed                                               |
| F4.4 / F4.9                | Tier / confidence informational fields                                          | Layer-of-rendering                                     |
| F2.3                       | Replace behavioral with source                                                  | Weaker than B (covers()-OR keeps both layers)          |
| C (X4)                     | Triple-evidence widening within single audit                                    | Adversarial-rejected: violates SRP, collapses into A+B |
