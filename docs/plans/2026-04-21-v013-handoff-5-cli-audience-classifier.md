---
title: "feat(v0.1.3): CLI handoff 5 — audience classifier, audit_profile suppression, env-hints pattern 2"
type: feat
status: implemented
date: 2026-04-21
completed_date: 2026-04-21
branch: feat/v013-audience-classifier
origin: "docs/plans/2026-04-20-v012-handoff-4-behavioral-checks.md (references H5 in sibling handoffs table); ~/dev/agentnative-site/docs/plans/2026-04-20-v013-handoff-5-audience-leaderboard.md (the original combined H5 — CLI-side scope split out here)"
---

# v0.1.3 CLI Handoff 5 — audience classifier, audit_profile suppression, env-hints pattern 2

## Overview

Split the CLI-side work out of the original combined H5 that lives in the `agentnative-site` repo. The CLI ships v0.1.3
with three things: a derived **audience classifier** that reads a scorecard run and labels the target as
`agent-optimized`, `mixed`, or `human-primary`; honoring of categorical exceptions via a new `--audit-profile` flag that
suppresses inapplicable MUSTs per `ExceptionCategory` (already in the registry, `#[allow(dead_code)]` reserved); and
Pattern 2 of `p1-env-hints` (bash-style `$FOO` / `TOOL_FOO` near flag descriptions — the half of the H4 plan that didn't
ship in v0.1.2, tracked as todo `013`).

All three land together because the site's H6 leaderboard launch reads whatever this release emits. Bundling them means
the site regen wave picks up the new fields and the corrected `p1-env-hints` verdicts in a single pass.

## Cross-repo context and ordering

This plan is the source of truth for the CLI-side v0.1.3 scope. The original combined H5 in
`agentnative-site/docs/plans/2026-04-20-v013-handoff-5-audience-leaderboard.md` listed both CLI and site work in one
document. Per the SoT principle ("each repo's `docs/plans/` owns the work that happens in that repo"), this plan carries
the CLI half and that H5 is expected to shrink to site-only scope (and renumber to H6 at the site's discretion — that
renumbering lives entirely on the site side and is out of scope here).

**Release ordering (hard sequence):**

1. **This plan (CLI H5) ships first** as `anc` v0.1.3 on crates.io, GitHub Release, Homebrew bottles. The CLI emits
   `audience` and `audit_profile` as real (non-null) values.
2. **Regenerate 10 committed scorecards on `agentnative-site`** with v0.1.3, matching the H3 workflow proven for v0.1.1.
   Each site scorecard now carries `audience`, `audit_profile`, and the corrected `p1-env-hints` verdicts.
3. **Site H6 ships next** — audience banner on `/score/<tool>`, leaderboard filters, `/scorecards` go-live, methodology
   note. The site's H6 plan document owns that scope; this plan only notes that CLI H5 unblocks it.

The CLI cannot wait for the 100-tool registry baseline (the site's H6 hard prereq); H5 is a CLI feature release
that stays backwards-compatible with v1.1 consumers. Site H6 is where the launch-narrative risk lives.

## Problem Frame

v0.1.2 ships with two scorecard JSON fields reserved for v0.1.3: `audience: null` and `audit_profile: null`. The site's
H2 renderers already feature-detect those keys and degrade gracefully (rendered as absent). For the H6 leaderboard to do
anything meaningful with them, the CLI has to start emitting real values.

Meanwhile, `p1-env-hints` shipped with only Pattern 1 (clap-style `[env: FOO]`). Pattern 2 (bash-style `$FOO` and
`TOOL_FOO` mentions near flag definitions) was in the H4 plan but never reached the binary. Three of ten committed site
scorecards (`ripgrep`, `gh`, `aider`) currently carry a false-positive Warn for `p1-env-hints` because their tools
document env bindings in free prose rather than clap annotations. Landing Pattern 2 in the same release as the audience
classifier flips those Warns to Pass in the same regen wave, pre-empting the
"leaderboard-looks-punitive-toward-famous-tools" launch narrative the site H5 CEO review flagged.

The three workstreams are orthogonal inside the CLI but share a release vehicle.

## Requirements Trace

- R1. Emit a derived `audience` label on every scorecard run. Rule: count `Warn` status across exactly 4 signal checks
  (`p1-non-interactive`, `p2-json-output`, `p7-quiet`, `p6-no-color-behavioral`). 0-1 → `agent-optimized`; 2 → `mixed`;
  3-4 → `human-primary`. Carried from origin H5 "audience classifier" scope.
- R2. When any of the 4 signal checks fails to run (missing runner, Skipped by applicability, or suppressed by
  audit_profile), emit `audience: null` rather than a partial-count verdict. Keeps the classifier honest about
  insufficient signal — per CEO review Finding #3, "aggregate signal is strictly weaker than per-check evidence."
- R3. Accept `--audit-profile <category>` on `anc check` where `<category>` is one of `human-tui`, `file-traversal`,
  `posix-utility`, `diagnostic-only`. Unknown values are a usage error (exit 2).
- R4. When `--audit-profile` is set, check IDs that the category excludes emit `CheckStatus::Skip` with evidence
  `"suppressed by audit_profile: <category>"` — they appear in `results[]` so readers see what was excluded, status is
  Skip not a new enum variant (schema stays v1.1-compatible), and the top-level scorecard `audit_profile` field echoes
  the applied category.
- R5. Add a drift test that fails the build if any of the 4 signal check IDs (R1) are missing from
  `src/principles/registry.rs::REQUIREMENTS`. Prevents silent classifier breakage from a rename.
- R6. Extend `p1-env-hints` with Pattern 2: bash-style `$FOO` or `TOOL_FOO` tokens co-occurring within the same
  paragraph as a flag definition, with the three mitigations from todo `013` (tool-scoped uppercase identifier,
  paragraph co-occurrence constraint, shell-env blacklist).
- R7. Regenerate `docs/coverage-matrix.md` and `coverage/matrix.json` if the scope touches registry entries or
  `covers()` declarations (it shouldn't — this release adds no new requirement IDs or new checks, only widens one
  existing check and adds classifier/suppression infrastructure).
- R8. `anc check .` dogfood on the agentnative repo itself continues to pass all new behavior without regression.
- R9. Coverage matrix drift integration test continues to pass against committed artifacts.

## Scope Boundaries

- No new check IDs or registry requirements added. The 46-entry registry stays 46.
- No changes to `CheckStatus` enum (no new `NotApplicable` variant). Suppressed checks use existing `Skip` with a
  structured evidence prefix.
- `schema_version` stays `"1.1"`. The `audience` and `audit_profile` fields go from `null` to real values, which is
  backwards-compatible and was the intent of reserving them in v1.1.
- No audience classification refinements beyond the 4-signal rule. If the classifier disagrees with intuition on a
  particular tool, the fix goes in the registry (add an `audit_profile`) or in the MUST set (new check in a future
  release), not in the classifier rules. Per origin H5 "Known gotchas."
- The `--audit-profile` flag accepts one value per invocation. No composition (`human-tui,file-traversal`). Composition
  can land later if a real tool needs it.
- The CLI does not read the site's `scorecards/registry.yaml`. The site's regen script looks up each tool's
  `audit_profile` and passes it to `anc check --audit-profile=<value>`. This keeps the CLI repo-agnostic.

### Deferred to Separate Tasks

- **Site H6 (leaderboard launch)**: banner, filters, methodology note, go-live — owned by
  `agentnative-site/docs/plans/`. Depends on this plan shipping v0.1.3 and the site's 100-tool registry baseline.
- **Homebrew-tap finalize-release dispatch fix**: tracked as todo `006` in `brettdavies/.github`. Not blocking this
  release; recovery is manual `gh api` dispatch (proven on v0.1.2). Fix before v0.1.4 ideally.
- **Existing site H5 rename/rescope** to H6 + site-only content: site repo work, not this plan's concern.
- **Composition of audit_profile values** (multiple categories per invocation): wait for a concrete consumer.
- **Registry-YAML-aware auto-detection** of audit_profile (CLI reads a file and picks a profile): intentionally not
  built; caller passes the value. Keeps the CLI honest about knowing only what it was told.

## Context & Research

### Relevant Code and Patterns

- `src/scorecard.rs:14-26` — `Scorecard` struct already carries `audience: Option<String>` and `audit_profile:
  Option<String>`. `build_scorecard()` at line 224 takes both as parameters. `format_json()` at line 240 currently
  always passes `None`. Wiring is half-done.
- `src/principles/registry.rs:32-46` — `ExceptionCategory { HumanTui, FileTraversal, PosixUtility, Diagnostic }` enum
  already defined with `#[allow(dead_code)]` noting "Reserved for v0.1.3 audit_profile consumption." Serde serializes as
  kebab-case (`human-tui`, `file-traversal`, etc.). Our flag values map directly.
- `src/cli.rs` — `Commands::Check` variant is where `--audit-profile` lives. Parse with clap's `value_parser!` on a new
  enum type that mirrors `ExceptionCategory` names.
- `src/main.rs` check execution loop (`for check in &all_checks { ... }`) — the seam where audit_profile suppression
  fires. Before calling `check.run()`, look up `(check.id(), profile)` and emit a Skip directly if suppressed.
- `src/checks/behavioral/non_interactive.rs` — the existing canonical behavioral check; mirrored by every new one in
  this release. Convention already documented in `CLAUDE.md` §"Source Check Convention."
- `src/runner/help_probe.rs::parse_env_hints` (v0.1.2) — Pattern 1 implementation; Pattern 2 extends the same function
  (or adds a sibling and a merge step). Fixture constants at the top of the `#[cfg(test)]` module model
  ripgrep/clap/bare/non-English shapes — add a `gh`-style fixture with an ENVIRONMENT section.
- `src/checks/behavioral/env_hints.rs` — check body already delegates to `help.env_hints().len()`; no change needed when
  the parser widens.

### Institutional Learnings

- Pre-1.0 additive JSON fields on the scorecard have the same feature-detect-or-miss pattern consistently: the site
  renderer tolerates missing keys (H2, H3), so going from `null` to a value is non-breaking.
- The H5 CEO review Finding #3 (audience classifier is informational, not authoritative) is load-bearing: the classifier
  MUST NOT gate scorecard totals or override per-check verdicts. Implementation should compute audience last (after
  every check has run) and never mutate the `results[]` vector based on the label.
- Dogfood safety: `arg_required_else_help = true` stays on `Cli` (CLAUDE.md §"Dogfooding Safety"). New `--audit-profile`
  flag doesn't change this.
- `Confidence::Medium` stays appropriate for `p1-env-hints` even after Pattern 2 widens detection. The heuristic is
  still heuristic; broader does not mean higher confidence.

### External References

- Origin H5 doc (site repo): `~/dev/agentnative-site/docs/plans/2026-04-20-v013-handoff-5-audience-leaderboard.md` §"In
  `agentnative` (code)" — the authoritative description of the audience rule + audit_profile suppression.
- H4 plan: `docs/plans/2026-04-20-v012-handoff-4-behavioral-checks.md` — defines Pattern 2 for `p1-env-hints`; this
  release finishes that work.
- Todo `013`: `.context/compound-engineering/todos/013-pending-p3-p1-env-hints-pattern-2-bash-style-detection.md` —
  scoping, fixture tools (`ripgrep`/`gh`/`aider`), and mitigations for Pattern 2.
- CEO plan: `~/.gstack/projects/brettdavies-agentnative/ceo-plans/2026-04-20-p1-doctrine-spec-coverage.md` §"Accepted
  Scope (v0.1.3)" — the product-side framing for audience + audit_profile.

## Key Technical Decisions

- **Suppression uses existing `CheckStatus::Skip`, not a new `NotApplicable` variant.** Reason: avoids a schema change;
  Skip already has evidence-string semantics; readers see the check in `results[]` with a clear reason.
- **Audience emits `null` when <4 of 4 signal checks run.** Reason: a partial-denominator label would be less honest
  than "no signal." Skipped or suppressed signal checks are a legitimate reason to withhold the label.
- **`--audit-profile` takes one value.** Reason: no current consumer needs composition; YAGNI.
- **Suppression table lives in `src/principles/registry.rs` next to `ExceptionCategory`.** Reason: the enum is already
  there; the mapping is small (4 categories × handful of suppressed check IDs each); colocating keeps the exception
  semantics in one file.
- **Audience label serialization is kebab-case** (`"agent-optimized"`, `"mixed"`, `"human-primary"`). Reason: unifies
  with `audit_profile`'s kebab-case values (`"human-tui"`, etc.) within the same JSON document, so consumers don't
  juggle two casing conventions in one scorecard. `audit_profile` MUST be kebab-case because it echoes the CLI flag
  value (`--audit-profile human-tui`); `audience` adopts the same convention. Per-result enum values in
  `results[].group` / `layer` / `confidence` stay snake_case — they are a different contract (one row per check) with
  broader consumer history and share spelling with the Rust type identifiers they come from. (Revised post-code-review
  on 2026-04-22; the original decision was snake_case to match the per-result enums. See Implementation Log.)
- **Drift test uses `const` array of 4 signal IDs** so a grep can find them in one place. Reason: the test failure
  message points back to this array, and renaming a check surfaces both the test failure and the array in the same hunk
  of the diff.
- **Pattern 2 parser extends `parse_env_hints`, does not replace it.** Reason: Pattern 1's behavior is frozen by shipped
  scorecards on the site — widening has to be strictly additive. Dedup pass merges the two pattern results by `var`
  name.

## Open Questions

### Resolved During Planning

- **Where does the audit_profile value come from?** → Caller passes via `--audit-profile`. The CLI doesn't read the
  site's YAML.
- **Should suppressed checks disappear from `results[]`?** → No; they emit Skip with evidence so readers see what was
  excluded and why. Per origin H5 "Known gotchas."
- **Do we bump `schema_version` to 1.2?** → No. All changes are additive (new non-null values on reserved fields). v1.1
  consumers feature-detect correctly.
- **Can the audience classifier override a check verdict?** → No. It's read-only over `results[]`. Per CEO review
  Finding #3.
- **Does coverage matrix need to regenerate?** → No, unless Pattern 2 work incidentally touches `covers()` declarations
  (it shouldn't — `p1-env-hints` still covers `p1-must-env-var`, nothing else).

### Deferred to Implementation

- **Exact suppression table** (category → check IDs): implementer fills in per category semantics with the registry
  entries on hand. Initial guess: `human-tui` suppresses `p1-must-no-interactive`, `p1-should-tty-detection`,
  `p1-should-defaults-in-help`, `p6-must-sigpipe` (TUI apps legitimately intercept), `p7-should-verbose` (same).
  `posix-utility` suppresses the `p1-must-no-interactive` family (satisfied vacuously via stdin-primary).
  `file-traversal` suppresses `p6-should-subcommand-operations`, `p3-must-subcommand-examples` (tools like `fd` have no
  subcommands by design). `diagnostic-only` suppresses the P5 MUSTs (no write operations). Validate against `lazygit`,
  `ripgrep`, `fd`, `nvidia-smi` during implementation.
- **Signal check denominator semantics when one check Errors vs Skips**: start with "count Warn only; any other status
  (Pass, Skip, Error, Fail, suppressed) is not a Warn." Revisit if dogfood reveals a category the simple rule mislabels.
- **Pattern 2 paragraph boundary** (how aggressively to scope `$FOO` co-occurrence): start with a 4-line window centered
  on the flag definition; tighten if dogfood on ripgrep/gh/aider produces false positives.

## High-Level Technical Design

> *This illustrates the intended approach and is directional guidance for review, not implementation
> specification. The implementing agent should treat it as context, not code to reproduce.*

**Check execution pipeline with audit_profile:**

```text
main::run(cli)
  ├─ parse --audit-profile → Option<ExceptionCategory>
  ├─ build Project, collect all_checks
  └─ for check in all_checks:
        if !check.applicable(project): continue
        if let Some(profile) = audit_profile:
            if registry::suppresses(check.id(), profile):
                results.push(Skip("suppressed by audit_profile: <profile>"))
                continue
        results.push(check.run(project))
  └─ audience = audience::classify(&results)  // reads, never writes
  └─ scorecard::build_scorecard(&results, &all_checks, audience, audit_profile)
```

**Audience classifier data flow:**

```text
classify(results: &[CheckResult]) -> Option<String>
  signal_ids = ["p1-non-interactive", "p2-json-output", "p7-quiet", "p6-no-color-behavioral"]
  signals = results.filter(|r| signal_ids.contains(&r.id))
  if signals.len() < 4: return None
  warns = signals.filter(|r| matches!(r.status, Warn(_))).count()
  match warns {
    0..=1 => Some("agent-optimized"),
    2     => Some("mixed"),
    3..=4 => Some("human-primary"),
    _     => unreachable!(),
  }
```

**Pattern 2 env-hint detection (sketch):**

```text
parse_env_hints(raw):
  pattern1 = scan "[env: FOO]" annotations         // existing
  pattern2 = for each flag in parse_flags(raw):
    within_window(raw, flag.position, ±2 lines):
      for token matching /\$?[A-Z][A-Z0-9_]{2,}/:
        if token in SHELL_BLACKLIST: skip
        else: record as EnvHint(var=token.strip_prefix('$'), source=flag)
  dedupe_by(var_name, pattern1 ++ pattern2)
```

## Implementation Units

- [x] **Unit 1: Audience classifier module + drift test**

**Goal:** Add `src/scorecard/audience.rs` with the `classify()` function, the 4 signal IDs as a const array, and
an inline drift test that fails loudly if any signal ID is missing from `REQUIREMENTS`.

**Requirements:** R1, R2, R5

**Dependencies:** None — reads existing `CheckResult` / `CheckStatus` types only.

**Files:**

- Create: `src/scorecard/audience.rs` (includes `#[cfg(test)]` tests)
- Create: `src/scorecard/mod.rs` (promote `scorecard.rs` to a directory module so `audience.rs` can live alongside the
  existing scorecard code — mirrors the `runner.rs` → `runner/` promotion from H4)
- Modify: `src/main.rs` (adjust imports if promotion changes paths)

**Approach:**

- Constants: `SIGNAL_CHECK_IDS: &[&str; 4]` listing the 4 IDs.
- Pure function `classify(&[CheckResult]) -> Option<String>` emits a kebab-case label or `None`.
- Drift test iterates `SIGNAL_CHECK_IDS`, calls `principles::registry::find(id)`, asserts `Some(_)`. Failure message
  cites which ID is missing so a rename produces an actionable hit.
- Directory promotion is purely mechanical; no behavior change beyond module reorganization.

**Execution note:** Test-first for the classifier — write the classifier unit tests before the implementation.
The signal-ID drift test can land alongside.

**Patterns to follow:**

- `src/runner/help_probe.rs` — module layout pattern (directory + `mod.rs` + submodules).
- `src/principles/matrix.rs::dangling_cover_ids` — drift-test pattern (iterates a set of IDs, validates against
  registry, emits actionable failure).

**Test scenarios:**

- Happy path — 4 Pass results with the signal IDs → `Some("agent-optimized")`.
- Happy path — 4 Warn results → `Some("human-primary")`.
- Happy path — 2 Warn, 2 Pass → `Some("mixed")`.
- Happy path — 1 Warn, 3 Pass → `Some("agent-optimized")`.
- Edge case — only 3 of 4 signal checks present in results (one missing entirely) → `None`.
- Edge case — one signal check emits Skip (not Warn) → count as not-a-Warn; 0 Warns total → `Some("agent-optimized")`.
- Edge case — one signal check emits Error → count as not-a-Warn (Error ≠ Warn); if total Warns in remaining are 0 →
  `Some("agent-optimized")`.
- Edge case — non-signal checks in `results[]` are ignored (their Warn/Pass doesn't count).
- Drift — `REQUIREMENTS` contains every `SIGNAL_CHECK_IDS` entry (current state) → test passes.
- Drift — remove an ID from `REQUIREMENTS` (mutate via test fixture if possible; otherwise rely on compile-time
  cross-reference) → test fails with an actionable message naming the ID.

**Verification:** `cargo test audience` green; drift test fails loudly when an ID is deliberately broken.

- [x] **Unit 2: Thread audience into scorecard emission**

**Goal:** Call `audience::classify()` at the right seam in `main::run()` and pass the result into
`scorecard::build_scorecard()` in place of `None`. Same for `format_json` call site when audit_profile work is also
wired (Unit 4); for this unit, scope to audience only.

**Requirements:** R1

**Dependencies:** Unit 1.

**Files:**

- Modify: `src/main.rs` (compute audience after results loop, pass to formatter).
- Modify: `src/scorecard.rs` — `format_json()` signature to accept `audience: Option<String>` and `audit_profile:
  Option<String>` (or keep as thin wrapper; implementer decides between flag-parameter threading vs. main-level call to
  `build_scorecard` directly).
- Test: extend `src/scorecard.rs` `#[cfg(test)]` module.

**Approach:**

- The simplest wiring: `main::run` computes audience and audit_profile, calls `build_scorecard` directly, then
  serializes. `format_json` becomes the bridge (or gets replaced at the call site). Implementer picks whichever
  minimizes churn.
- Keep `format_text` unchanged — audience does not surface in the text renderer for this release.

**Patterns to follow:**

- `build_scorecard` already accepts both params; this unit is mostly plumbing.

**Test scenarios:**

- Integration — a `format_json` run with results matching "4 Pass signals" emits `"audience": "agent-optimized"`.
- Integration — a run with results matching "3 Warn, 1 Pass signals" emits `"audience": "human-primary"`.
- Backwards-compat — a run without the 4 signals (e.g., source-only mode, no behavioral checks) emits `"audience":
  null`.
- Schema — `schema_version` stays `"1.1"`.

**Verification:** `anc check ripgrep --output json | jaq .audience` returns a string; `anc check .` (self)
returns a string (`anc` should classify as `agent-optimized` per its own dogfood).

- [x] **Unit 3: Suppression table + `registry::suppresses()` helper**

**Goal:** Extend `src/principles/registry.rs` with a static mapping from `ExceptionCategory` to the check IDs it
suppresses, plus a `pub fn suppresses(check_id: &str, category: ExceptionCategory) -> bool` helper. Remove the
`#[allow(dead_code)]` on `ExceptionCategory` — it's now consumed.

**Requirements:** R4

**Dependencies:** None — lives entirely inside the registry module.

**Files:**

- Modify: `src/principles/registry.rs` (add mapping + helper + tests).
- Test: extend `#[cfg(test)] mod tests` in the same file.

**Approach:**

- Static `SUPPRESSION_TABLE: &[(ExceptionCategory, &[&str])]` — entries are `(category, &[check_ids])`.
- `suppresses()` walks the table, matches the category, membership-checks the check ID.
- Initial table per the deferred-decision list in "Open Questions § Deferred": `human-tui` suppresses P1
  interactive-prompt MUSTs + `p6-must-sigpipe` + `p7-should-verbose`; `posix-utility` suppresses
  `p1-must-no-interactive` (satisfied vacuously); `file-traversal` suppresses `p3-must-subcommand-examples` +
  `p6-should-subcommand-operations`; `diagnostic-only` suppresses P5 MUSTs. Implementer validates against
  lazygit/ripgrep/fd/nvidia-smi.

**Test scenarios:**

- Happy — `suppresses("p1-must-no-interactive", HumanTui)` → `true`.
- Happy — `suppresses("p2-must-output-flag", HumanTui)` → `false` (TUIs still need structured output when asked).
- Happy — `suppresses("p5-must-dry-run", DiagnosticOnly)` → `true`.
- Edge — unknown check ID `"totally-fake-id"` → `false` for every category.
- Drift — every check ID listed in `SUPPRESSION_TABLE` exists in `REQUIREMENTS` (mirrors the `dangling_cover_ids` test
  pattern for the suppression side).
- Category coverage — each of the 4 `ExceptionCategory` variants has at least one entry; failing this test means the
  table was accidentally emptied.

**Verification:** `cargo test registry::tests` green, including the new suppression-table drift guard.

- [x] **Unit 4: `--audit-profile` CLI flag + suppression wiring + scorecard field**

**Goal:** Parse `--audit-profile <category>` on `anc check`, thread the value into the check execution loop,
emit `CheckStatus::Skip` for suppressed checks, and echo the applied profile as the top-level scorecard `audit_profile`
field.

**Requirements:** R3, R4

**Dependencies:** Unit 3.

**Files:**

- Modify: `src/cli.rs` — add `audit_profile: Option<AuditProfile>` field to `Commands::Check`.
- Modify: `src/main.rs` — pass the parsed profile into the check execution loop; emit Skip with structured evidence;
  thread into `build_scorecard`.
- Modify: `src/principles/registry.rs` if a helper parse-from-string is needed for clap's `value_parser!`
  (alternatively, define a `ValueEnum` mirror in `src/cli.rs` and convert).
- Test: add integration tests in `tests/audit_profile.rs` (new file) or extend an existing integration file.

**Approach:**

- Define a `ValueEnum` in `src/cli.rs` with the same variants + kebab-case serialization as `ExceptionCategory`. Convert
  to `ExceptionCategory` via a small `impl From`.
- In `main::run()`, wrap the check execution loop: before `check.run(project)`, consult
  `registry::suppresses(check.id(), profile)`. If true, push a Skip result with evidence `format!("suppressed by
  audit_profile: {category}")` and `continue`.
- Pass the profile's serde string representation (e.g., `"human-tui"`) into `build_scorecard()`'s `audit_profile`
  parameter.
- Unknown `--audit-profile` values: clap rejects with usage error, exit 2. No custom handling needed.

**Patterns to follow:**

- `src/cli.rs` `OutputFormat` enum — `ValueEnum` pattern already present.
- Existing behavioral-check Skip emissions with structured evidence (e.g., `p1-flag-existence`'s "target satisfies P1
  via alternative gate").

**Test scenarios:**

- Happy — `anc check <tui-target> --audit-profile human-tui --output json` produces a scorecard with `"audit_profile":
  "human-tui"` and the P1 interactive-prompt checks showing Skip with evidence matching `/suppressed by audit_profile:
  human-tui/`.
- Happy — same run, checks NOT suppressed by human-tui (e.g., `p2-must-output-flag`) behave identically to a no-profile
  run.
- Edge — `anc check . --audit-profile invalid-value` exits 2 with clap's usage error.
- Integration — running without `--audit-profile` emits `"audit_profile": null` (behavior unchanged from v0.1.2).
- Integration — when `--audit-profile human-tui` is set AND audience classifier runs, the `audience` label reflects only
  the Warns among the 4 signal checks that actually ran (signal checks suppressed by the profile are Skipped, so
  denominator shrinks below 4, so `audience: null` — matches R2).

**Verification:** `anc check --command lazygit --audit-profile human-tui --output json | jaq '.audit_profile'`
returns `"human-tui"`; suppressed checks visible in `results[]` with the expected evidence string.

- [x] **Unit 5: `p1-env-hints` Pattern 2 — bash-style `$FOO` detection**

**Goal:** Extend `parse_env_hints` in `src/runner/help_probe.rs` with bash-style detection. Add the three
mitigations from todo `013`: tool-scoped uppercase identifier shape, same-paragraph co-occurrence with a flag
definition, and a shell-env blacklist.

**Requirements:** R6

**Dependencies:** None (independent of audience/audit_profile work; could ship in any order inside this release).

**Files:**

- Modify: `src/runner/help_probe.rs` (extend `parse_env_hints`; add `SHELL_ENV_BLACKLIST` const; add helpers).
- Test: inline `#[cfg(test)]` module in the same file — 5 new fixtures per todo `013`.

**Approach:**

- Add a second pass that uses `parse_flags` output to locate flag positions, then scans a ±4-line window around each
  flag for tokens matching `/\$?([A-Z][A-Z0-9_]{2,})/` (uppercase, underscore, digit; min length 3).
- Blacklist `PATH`, `HOME`, `USER`, `SHELL`, `PWD`, `LANG`, `TERM`, `TMPDIR`.
- Dedupe by `var` name against Pattern 1's hits.
- Confidence on the `p1-env-hints` check remains `Medium`.

**Patterns to follow:**

- Existing `parse_env_hints` function structure (mutable accumulator, single pass); Pattern 2 is a second pass or merged
  scan.
- Existing fixture constants `RIPGREP_HELP` / `CLAP_HELP` / `BARE_HELP` / `NON_ENGLISH_HELP` — add `GH_HELP` fixture
  with ENVIRONMENT section.

**Test scenarios:**

- Happy — `gh`-style fixture with `ENVIRONMENT` section containing `GH_TOKEN`, `GH_HOST`, `GH_REPO` → all three detected
  as EnvHints.
- Happy — `ripgrep`-style fixture with prose mentioning `RIPGREP_CONFIG_PATH` within 4 lines of a flag definition →
  detected.
- Negative — `$PATH` in flag description → blacklisted, not recorded.
- Negative — uppercase identifier `MYVAR` in an unrelated prose section (no flag nearby within window) → not recorded.
- Dedup — fixture with both `[env: FOO]` AND prose `$FOO` mention → exactly one `EnvHint` with `var == "FOO"`.
- Backwards-compat — existing Pattern 1 tests (`parse_env_hints_captures_clap_style`,
  `parse_env_hints_multiple_occurrences`, `parse_env_hints_rejects_invalid_names`) still pass unchanged.

**Verification:** `anc check --command rg/gh/aider --output json` — `p1-env-hints` flips Warn → Pass on all
three. `anc check .` still Pass.

- [x] **Unit 6: Version bump, CHANGELOG prep, dogfood, smoke, docs**

**Goal:** Verify end-to-end on the v0.1.3 release branch — everything builds, tests pass, coverage matrix drift
check passes, dogfood and smoke produce sensible verdicts. Exception section in `docs/coverage-matrix.md` drops the
"clap-only" qualifier on `p1-env-hints` now that Pattern 2 ships.

**Requirements:** R7, R8, R9

**Dependencies:** Units 1–5.

**Files:**

- Modify: `docs/coverage-matrix.md` exception prose (remove "clap-only" on `p1-env-hints`, note Pattern 2 ships with
  mitigations).
- Regenerate if drifted: `docs/coverage-matrix.md`, `coverage/matrix.json` (should NOT drift in this release — no new
  checks, no new `covers()` changes).
- Release-branch mechanics happen in `RELEASES.md` flow, not this plan.

**Approach:**

- Run `cargo test` full suite — all green.
- Run `cargo clippy --all-targets -- -Dwarnings` — all green.
- Run `cargo deny check` — all green.
- Run `anc check .` (self) — no regressions; `audience` field populates.
- Run `anc check --command rg/gh/aider/bat/bird/xr` — p1-env-hints results as expected from Unit 5.
- Run `anc check --command lazygit --audit-profile human-tui` (if lazygit available) — suppression visible.
- Run the coverage-matrix drift test; if it fires, regenerate and commit.

**Test scenarios:**

- This unit verifies the other units; no net-new test code here.
- `anc check . --output json | jaq '.schema_version, .audience, .audit_profile'` returns `"1.1"`, a non-null audience
  string, and `null` (no profile set for dogfood).
- Smoke matrix (per-tool expected verdict on the new fields) captured informally in the PR body for reviewer sanity.

**Verification:** PR to `dev` passes CI; pre-push hook green; `anc check --command anc --audit-profile
diagnostic-only` (whimsical edge-case) works without panic.

## System-Wide Impact

- **Interaction graph:** The check execution loop in `main::run()` gets a new pre-flight stage (audit_profile
  suppression) that wraps every `check.run()` call. The audience classifier runs once, after all results are collected,
  and is read-only over the vector. No check implementation needs to know about either.
- **Error propagation:** Suppression emits `Skip`, not `Error`; exit-code policy (`scorecard::exit_code`) treats Skip as
  neutral (neither 1 nor 2), so suppressed checks don't flip the exit code. Audience classifier doesn't touch exit code.
- **State lifecycle risks:** None. Audience is computed from already-complete results; suppression prevents a check from
  running, so there's no partial-state concern.
- **API surface parity:** Scorecard `Scorecard` struct already carries both fields. `Check` trait unchanged.
  `CheckStatus` unchanged. `schema_version` unchanged.
- **Integration coverage:** Need one end-to-end test that exercises `--audit-profile human-tui` + full scorecard
  emission + audience label. Unit tests alone won't prove the Skip-evidence string format across the integration seam.
- **Unchanged invariants:**
- Scorecard JSON `schema_version` stays `"1.1"`.
- Every check's public `id()`, `group()`, `layer()`, `covers()` return unchanged values.
- The registry's 46-entry count stays 46; `level_counts_match_spec` test continues to pass.
- `CheckStatus` enum variants unchanged.
- `Confidence` enum unchanged; no existing check changes confidence.
- `arg_required_else_help = true` on `Cli` (fork-bomb guard) stays.
- `--audit-profile` is optional and defaults to `None`; default invocation behavior matches v0.1.2 exactly.

## Risks & Dependencies

| Risk | Mitigation |
| --- | --- |
| Audience classifier mislabels a tool in a way that's surfaced on the site's `/score/<tool>` page and the classifier gets patched instead of the registry | CEO review Finding #3 is recorded verbatim in the plan and the classifier module docstring. Any future "the label is wrong for tool X" fix goes through a registry `audit_profile` addition or a new MUST, not through classifier logic. |
| Suppression table gets a typo (check ID that doesn't exist in `REQUIREMENTS`) and shadows a real check silently | Unit 3 drift test (mirror of `dangling_cover_ids`) catches it at build time. |
| Pattern 2 of `p1-env-hints` over-matches and flips tools to Pass that shouldn't | The three mitigations (tool-scoped identifier, same-paragraph co-occurrence, shell-env blacklist) plus explicit negative tests for `$PATH`/`$HOME`/unrelated `$MYVAR`. Confidence stays Medium. |
| `--audit-profile` becomes a crutch — every problematic tool gets one rather than the MUSTs being reconsidered | The 4 categories are fixed for v0.1.3; adding a fifth requires a plan revision. Registry author discipline. |
| The 100-tool baseline prerequisite on the site side slips, delaying H6 indefinitely | CLI H5 ships independently; crate + Homebrew are live once this merges. Site H6 is on its own track; this plan has no cross-repo gating. |

## Documentation / Operational Notes

- **CHANGELOG** will auto-generate from squash-commit `## Changelog` bodies via `git-cliff` during the release- branch
  prep (same flow as v0.1.1 and v0.1.2). Each unit's commit body should include the one-liner user-facing change under
  `### Added` or `### Changed`.
- **`docs/coverage-matrix.md` exception section**: drop the "clap-only" qualifier on `p1-env-hints` (Unit 6) and add a
  line noting the audience classifier's 4 signal checks. The exception prose is hand-edited (not generated); the
  per-principle rows are auto-generated.
- **Release-branch flow** follows `RELEASES.md` exactly — cherry-pick feature commits onto `release/v0.1.3` off
  `origin/main`, bump `Cargo.toml` to `0.1.2 → 0.1.3`, regenerate completions (expect a diff if `--audit-profile` adds a
  new flag to `check`), run `generate-changelog.sh`, PR to `main`.
- **Homebrew finalize-release dispatch** will again land on the wrong repo until todo `006`
  (`.github/homebrew-tap-finalize-dispatch-wrong-repo`) is fixed. Plan on a manual `gh api
  repos/brettdavies/agentnative-cli/dispatches --method POST -f event_type=finalize-release -f
  'client_payload[tag]=v0.1.3'` after the Homebrew tap's `Publish bottles` workflow completes. Proven on v0.1.2.
- **Post-release site coordination**: after v0.1.3 on crates.io + Homebrew settle, install the new anc on the site box,
  regenerate the 10 committed scorecards, commit atomically. H6 picks up from there.

## Sources & References

- **Origin document (combined H5):**
  `~/dev/agentnative-site/docs/plans/2026-04-20-v013-handoff-5-audience-leaderboard.md`
- **H4 handoff that designed Pattern 2:** `docs/plans/2026-04-21-v012-h4-eng-agent-handoff.md`
- **Todo for Pattern 2 scope (now `done`):**
  `.context/compound-engineering/todos/013-done-p3-p1-env-hints-pattern-2-bash-style-detection.md`
- **Todo for tap dispatch recovery (operational):** `brettdavies/.github` repo, todo `006`.
- **Release flow:** `RELEASES.md`
- **Current scorecard structure (reserved fields):** `src/scorecard.rs` lines 14-26, 221-238
- **Reserved ExceptionCategory enum:** `src/principles/registry.rs` lines 29-46
- **Existing module-promotion precedent:** `src/runner/` (promoted from `src/runner.rs` during H4)

## Implementation Log

**Branch:** `feat/v013-audience-classifier` (6 commits ahead of `dev`). **Status:** all six units implemented;
pre-release state — `Cargo.toml` still at `0.1.2`, completions not regenerated. Release-branch work per `RELEASES.md` is
the next step.

### Commits

1. `docs(plans)` — plan doc (this file).
2. `feat(scorecard)` — Units 1 + 2 bundled. Promoted `src/scorecard.rs` → `src/scorecard/` module, added
   `scorecard/audience.rs` with `classify()`, wired into `main::run()` and `format_json()`.
3. `feat(cli)` — Units 3 + 4 bundled. `SUPPRESSION_TABLE` + `suppresses()` helper + `--audit-profile` flag + Skip
   emission in the check execution loop.
4. `feat(p1-env-hints)` — Unit 5. Pattern 2 `$FOO` / `TOOL_FOO` detection with the three mitigations.
5. `docs` — CLAUDE.md sync (removed the `ExceptionCategory` reservation callout, updated the scorecard-fields section to
   reflect the consumed fields).
6. `refactor(help_probe)` — dropped three genuinely-redundant guards in `extract_env_tokens` that a
   `code-simplicity-reviewer` pass surfaced. No behavior change.

### Deviations from the plan

- **Units 1+2 and Units 3+4 committed together** rather than as separate commits. The plan's unit boundaries are
  conceptually correct, but Rust's `-Dwarnings` CI flag treats unused symbols as errors, so a unit that introduces a
  helper and the unit that consumes it have to land together (or carry a temporary `#[allow(dead_code)]`). The paired
  bundles are still clean conventional commits with value-first changelog bodies.
- **`ExceptionCategory::Diagnostic` renamed to `DiagnosticOnly`** so serde renders `"diagnostic-only"` matching the
  plan's committed flag value. Plan listed the variant as `Diagnostic` which would have serialized as `"diagnostic"`.
- **Suppression table uses check IDs, not requirement IDs.** The plan's "Open Questions § Deferred" listed the initial
  guess in requirement-ID form (`p1-must-no-interactive`, `p6-must-sigpipe`). At implementation time it became clear the
  runtime call site has `check.id()` in hand, so the table stores check IDs directly. Drift test
  `suppression_table_check_ids_exist_in_catalog` validates every entry against the behavioral + source + project
  catalog.
- **Classifier Skip semantics refined mid-implementation.** The plan said "start with Skip = not-a-Warn" in the
  deferred-decision list. R2 said "suppressed by audit_profile → audience: null". The first implementation of
  `classify()` honored only the deferred decision and an integration test for R2 caught the gap. Final behavior: a Skip
  whose evidence prefix starts with `"suppressed by audit_profile:"` drops the signal from the denominator (R2); any
  other Skip counts as not-a-Warn (deferred decision). Unit test `audit_profile_suppressed_signal_drops_denominator`
  pins the new semantics; `organic_skipped_signal_counts_as_not_warn` pins the original.
- **`docs/coverage-matrix.md` prose change not needed.** The plan asked to "drop the clap-only qualifier" from the
  exception section. The rendered matrix had no such qualifier in prose to begin with (Exception handling was not
  rendered into the committed file); no edit was required.
- **Pattern 2 heuristic tightened beyond the plan's three mitigations.** The plan listed tool-scoped identifier shape
- same-paragraph co-occurrence + shell-env blacklist. The first implementation using only those regressed two
  pre-existing tests (captured `OPTIONS` and `HTTP` as env vars). Final rule requires `$` prefix OR underscore in the
  identifier (separating real env vars from acronyms/placeholders) AND rejects bare `[FOO]`/`<FOO>` placeholders AND
  strips `[env: ...]` regions before Pattern 2 scans. Documented as a reusable best-practice in
  `docs/solutions/best-practices/cli-env-var-shape-heuristic-2026-04-21.md`.

- **`gh` did not flip Warn → Pass as the todo predicted.** Pattern 2 probes `<binary> --help`; `gh` documents env vars
  in `gh help environment`, a separate help topic. The Warn is accurate for the actual `--help` surface. Named
  limitation — not in scope for v0.1.3.

- **Audience values flipped from snake_case to kebab-case (post-code-review, 2026-04-22).** The initial plan (see Key
  Technical Decisions above, now revised) chose `"agent_optimized"` / `"mixed"` / `"human_primary"` to match the
  existing `CheckGroup` / `CheckLayer` / `Confidence` snake_case enum convention. The `/ce:review` pass flagged the
  resulting mix (snake_case `audience` + kebab-case `audit_profile` inside one JSON document) as P3 #17. Assessment
  surfaced that the mix wasn't design — it was two legitimate conventions meeting. `audit_profile`'s kebab-case is
  non-negotiable (it echoes the CLI flag value a user types); `audience`'s snake_case was pure convention-inertia since
  audience values are never typed, never matched against a flag, and never exposed outside that one JSON field. Landed
  kebab-case on both: `audience: "agent-optimized" | "mixed" | "human-primary"`. Per-result enum values stay snake_case
  (different contract, broader consumer history). Window rationale: v0.1.2 emitted `audience: null`; site H6 hasn't
  shipped; no live consumer had pinned on the snake_case values. Changes: `src/scorecard/audience.rs::classify()` return
  values + doc comment; 13+ tests in `audience.rs` / `scorecard/mod.rs` / `tests/integration.rs`; README.md, AGENTS.md,
  CLAUDE.md v1.1 fields section; this plan doc.

### Verification results

- Unit tests: **382 passed, 0 failed, 1 ignored.**
- Integration tests: **47 passed, 0 failed** (includes 6 new `test_audit_profile_*` cases and 5 new Pattern 2 tests).
- `cargo clippy --all-targets -- -Dwarnings`: **clean.**
- `cargo fmt --check`: **clean.**
- `cargo deny check`: **advisories/bans/licenses/sources ok** (unchanged warnings about unused license allowances are
  pre-existing).
- `anc generate coverage-matrix --check`: **exit 0** (no drift; no registry changes).
- Dogfood `anc check . --output json`: `schema_version: "1.1"`, `audience: "agent-optimized"`, `audit_profile: null`, 27
  pass / 2 warn / 4 skip / 0 fail.
- Dogfood `anc check . --audit-profile diagnostic-only`: clean run, echoes profile, suppresses `p5-dry-run` with
  structured evidence, no panic.
- Smoke test Pattern 2 on live binaries:
- `rg`: `p1-env-hints` Warn → Pass (catches `RIPGREP_CONFIG_PATH` in `--config` description prose).
- `aider`: `p1-env-hints` Warn → Pass (catches `$FOO`-style prose mentions).
- `gh`: still Warn (named limitation above).

### Compounded learnings

- `docs/solutions/best-practices/cli-env-var-shape-heuristic-2026-04-21.md` — the reusable tool-scoped env-var-shape
  rule with before/after examples, derived during Pattern 2 implementation. Pushed to
  `brettdavies/solutions-docs@a577c5b`.

### Follow-ups (out of scope for this plan)

- **Release-branch mechanics** per `RELEASES.md`: branch off `origin/main`, cherry-pick these 6 commits, bump
  `Cargo.toml` `0.1.2 → 0.1.3`, regenerate completions (flag surface grew by `--audit-profile`), run
  `generate-changelog.sh`, PR to `main`.
- **Homebrew-tap finalize-release dispatch** will again land on the wrong repo (todo `006`). Manual recovery command
  pre-staged in `Documentation / Operational Notes` above.
- **Site H6 leaderboard launch** owned by `agentnative-site/docs/plans/`; unblocked by this release.
