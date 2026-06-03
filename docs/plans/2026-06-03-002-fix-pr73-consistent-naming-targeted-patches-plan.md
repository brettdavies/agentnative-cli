---
title: "fix: PR #73 follow-up — probe-failure, leaf-noun, emit removal in p6-consistent-naming"
type: fix
status: proposed
priority: P1
date: 2026-06-03
origin: "Adversarial review of merged PR #73 (commit 91c5a798) surfaced 8 findings; this plan ships the three
  highest-impact tactical patches while the larger redesign lives in plan
  2026-06-03-001-refactor-audit-philosophy-structure-over-vocabulary-plan.md"
---

# fix: PR #73 follow-up — probe-failure, leaf-noun, emit removal in p6-consistent-naming

## Summary

PR #73 introduced a three-bucket classification for `p6-should-consistent-naming` so that `anc`'s own surface (`audit` +
`emit coverage-matrix` + `skill install`) passes. The redesign is broadly defensible but ships three leaks that let
unrelated surfaces silently Pass: probe failures default to Pass instead of Warn, leaf non-verb subcommands classify as
TopLevelVerb without any probe evidence, and the addition of `emit` to `COMMON_VERBS` was unnecessary
belt-and-suspenders (the structural rule already passes `anc emit coverage-matrix`).

This plan does not redesign the bucketing. The bucketing redesign — including whether to drop the `COMMON_VERBS`
vocabulary check entirely, whether to add a cross-group consistency check, and how to handle deep-noun trees like `gh
repo issue create` — is owned by plan #001 (audit philosophy). This plan ships the smallest defensible patches that stop
the audit from actively passing surfaces the spec describes as anti-patterns, plus four adversarial tests.

PR reference: <https://github.com/brettdavies/agentnative-cli/pull/73> (merged 2026-06-01).

## Findings inventory

The adversarial review (`/compound-engineering:ce-code-review` on 2026-06-03) surfaced 8 findings against this PR. The
table below names each finding and where it gets addressed.

| #   | Severity | Title                                                             | Disposition                                 |
| --- | -------- | ----------------------------------------------------------------- | ------------------------------------------- |
| 1   | P1       | Heuristic redefines `consistent`; bucketing contradicts spec text | Parked → plan #001 owns redesign            |
| 2   | P2       | Leaf non-verb classified as TopLevelVerb unconditionally          | **Fixed here (R2)**                         |
| 3   | P1       | `verb_count==0` ⇒ TopLevelVerb misclassifies deep-noun trees      | Parked → plan #001                          |
| 4   | P2       | Cross-group verb divergence invisible                             | Parked → plan #001                          |
| 5   | P1       | Probe-failure defaults to TopLevelVerb (silent Pass)              | **Fixed here (R1)**                         |
| 6   | P2       | Tests lock in permissive behavior; no adversarial coverage        | **Partially fixed here (R4)** — 4 new tests |
| 7   | P3       | Per-run probe scales O(N) without cross-audit reuse               | Deferred                                    |
| 8   | P3       | `emit` addition to `COMMON_VERBS` is convenient, not standard     | **Fixed here (R3)**                         |

The four "Parked" findings are not abandoned — they are the design-level work plan #001 owns. Tactical patches here must
not make those harder to land.

## Scope

### In scope

- Probe-failure handling: any `ProbeResult` that returns an empty children vec because the probe itself failed
  (`NotFound`, `Timeout`, `Crash`, non-zero exit, non-clap-shaped output, localized help) classifies the parent as
  `Mixed` (Warn), never `TopLevelVerb`.
- Leaf-non-verb handling: a non-verb top-level subcommand classifies as `TopLevelVerb` only when the probe succeeded AND
  children are all non-verbs. An empty children vec from a successful probe (true leaf) still classifies as
  `TopLevelVerb`; an empty children vec from a failed probe does not.
- Remove `emit` from `COMMON_VERBS`. Verify that `anc emit coverage-matrix` still passes via the structural rule
  (`verb_count==0` ⇒ `TopLevelVerb` is one of the parked items — until plan #001 redesigns it, the rule remains; `emit`
  doesn't need duplicate coverage in the verb list).
- Four adversarial unit tests covering: probe-failure on a non-verb top-level; leaf non-verb whose `--help` errors;
  parser-empty case from localized help; verification that `emit` is no longer in `COMMON_VERBS`.

### Out of scope

- The bucketing redesign (`Mixed`/`TopLevelVerb`/`HierarchicalNounVerb` model itself).
- Cross-group consistency check.
- Deep-noun-tree handling (`gh repo issue create` style).
- Spec PR in `agentnative-spec` (owned by plan #001).
- Performance characterization of the probe (deferred; finding #7).

## Requirements

- **R1**: When `probe_subcommand_children` returns an empty children vec because the probe itself failed (any
  `RunStatus` other than `Ok`, or `Ok` with empty stdout+stderr that cannot be parsed as a clap help), the affected
  top-level subcommand classifies as `Classification::Mixed`. The audit's evidence string names the offending subcommand
  and the probe-failure reason. Source: `src/audits/behavioral/consistent_naming.rs:142-156`.

- **R2**: A non-verb top-level subcommand classifies as `Classification::TopLevelVerb` only when the probe succeeded AND
  the parsed children vec is empty. An empty children vec from a failed probe routes per R1. Source:
  `src/audits/behavioral/consistent_naming.rs:69-78`.

- **R3**: `emit` is removed from `COMMON_VERBS`. The `anc emit coverage-matrix` self-audit continues to pass because its
  children (`coverage-matrix`, `schema`) are non-verbs, satisfying the structural rule unchanged by this PR. Source:
  `src/audits/behavioral/consistent_naming.rs:33`.

- **R4**: New unit tests:
- `probe_failure_classifies_as_mixed` — a fake binary whose subcommand `--help` returns a non-zero exit gets classified
    as `Mixed`, not `TopLevelVerb`.
- `non_english_help_does_not_silent_pass` — a probe whose stdout begins with `Comandos:` (or any non-English `Commands:`
    header) parses as empty children and classifies as `Mixed`.
- `leaf_non_verb_with_successful_empty_probe_passes` — a true leaf (probe succeeds, children parsed as empty) still
    classifies as `TopLevelVerb`. This pins the R2 distinction explicitly.
- `emit_not_in_common_verbs` — direct assertion against `COMMON_VERBS.contains(&"emit")`.

## Implementation Units

### U1. Disambiguate probe-success-with-empty-children from probe-failure

In `probe_subcommand_children`, return a `ProbeResult` enum rather than a bare `Vec<String>`:

```rust
enum ProbeResult {
    Children(Vec<String>),
    Failed { reason: &'static str },
}
```

The `Failed` variant covers `RunStatus::NotFound`, `PermissionDenied`, `Error`, non-zero exit on `Ok`, and
`Ok`-with-empty-output cases. `Children(vec![])` is reserved for the genuine "binary printed parseable help with no
subcommands" case.

### U2. Thread `ProbeResult` through `classify`

`classify` currently takes the children slice directly. Update it to take `ProbeResult` and branch:

- `Failed { reason }` → `Classification::Mixed` with the reason carried into the evidence string.
- `Children(c)` if `c.is_empty()` → `Classification::TopLevelVerb` (leaf case, R2).
- `Children(c)` otherwise → existing verb-count / non-verb-count classification logic, unchanged.

### U3. Drop `emit` from `COMMON_VERBS`

Single-line array edit at `src/audits/behavioral/consistent_naming.rs:33`. No callers reference `emit` by symbol; the
test pinned in R4 verifies the removal.

### U4. Add four adversarial unit tests

Tests live in `src/audits/behavioral/consistent_naming.rs` alongside the existing 10 tests. Mock the probe via a
trait-bound stand-in or by constructing `ProbeResult` directly in test setup.

## Open questions

- **Should `Failed` be its own `Classification` variant?** Cleaner separation (`Classification::Unclassifiable { reason
  }` distinct from `Mixed`), but a larger diff and not strictly required. Default: collapse to `Mixed` to keep the diff
  minimal; revisit if the registry test counts surface that the audit's evidence strings need finer-grained reason
  codes.

- **`--audit-profile` opt-out for probe-failures?** Some legitimate CLIs may have subcommands whose `--help` exits
  non-zero (rare but possible: a subcommand that requires args to be present even for help). Should the audit gain a way
  to suppress this specifically? Default: no — failure to probe IS the principle's signal. If a real CLI fails
  legitimately, the right answer is to fix the CLI's help behavior.

- **Does the self-audit (`anc audit .`) still pass `p6-consistent-naming` at 100?** Verify by dogfood after U3. The
  prediction: yes — `emit coverage-matrix` and `emit schema` are non-verbs, so the structural rule (`verb_count==0` ⇒
  `TopLevelVerb`) still passes `emit`. Plan #001 may eventually tighten that rule; until then, the score holds.

- **Localized-help detection.** The `non_english_help_does_not_silent_pass` test pins one specific failure mode (Spanish
  `Comandos:`). The underlying issue is that `parse_subcommands` only knows English `Commands:` / `Subcommands:`. Should
  this plan also expand the parser, or is "Mixed" (Warn) the right answer for any non-parseable help? Default: keep the
  parser scope as-is; the probe-failure path is the right answer for parseable-but-unrecognized output too.

## Acceptance

- All four new unit tests pass.
- The full `cargo test` suite continues at the post-PR-#73 baseline (800+ passing, 2 ignored).
- `cargo clippy --all-targets -- -Dwarnings` clean.
- Dogfood: `anc audit .` returns `pass` (or unchanged score) on `p6-consistent-naming`.
- Score deltas for `gh`, `uv`, `cargo`, `jq`, `bc` (PR #73's six audit targets) are unchanged by this plan — those CLIs
  do not regress.
- Reference to plan #001 in this plan's body, and reciprocal reference back from #001's "per-PR plans" table.

## Notes for the implementer

- The `ProbeResult` enum is internal to the audit module; no public-facing API change.
- The runner cache (`src/runner/mod.rs::BinaryRunner`) is unchanged. The probe still goes through the cache; only the
  return-shape interpretation changes.
- Conventional commit shape: `fix(p6-consistent-naming): warn on probe failure, require probe for leaf non-verb`.
- Open the PR against `dev`. This plan does not touch the vendored spec — leave
  `src/principles/spec/principles/p6-composable-predictable-command-structure.md` untouched. Spec PR is plan #001's
  responsibility.
- Before merging: run `anc emit coverage-matrix --check` (must stay green) and `cargo deny check advisories` (must stay
  ok).
