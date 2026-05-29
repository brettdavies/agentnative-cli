---
title: "refactor: Remove implicit default subcommand (require explicit `anc audit`)"
type: refactor
status: proposed
date: 2026-05-27
origin: maintainer decision — require the explicit verb (git/cargo/kubectl/docker/gh/terraform convention); supersedes the default-subcommand portion of docs/plans/2026-04-02-003-feat-cli-default-subcommand-and-command-flag-plan.md
---

# refactor: Remove implicit default subcommand (require explicit `anc audit`)

## Summary

Delete the implicit-`audit` injection from the `anc` CLI. Today `anc .` is silently rewritten to `anc audit .` by
`src/argv.rs::inject_default_subcommand` (~163 LOC of clap-introspection heuristics, plus a ~225-line unit-test block).
After this change the verb is required — `anc audit .` — matching every multi-subcommand CLI of its class (git, cargo,
kubectl, docker, gh, terraform). Removing the injection lets clap emit its native, suggestion-bearing `unrecognized
subcommand` error for typos like `anc audit .`, deletes fragile code, and makes `anc` obey its own P6 principle. The
only cost is five extra characters for interactive humans; agents — the primary audience — parse a contract and gain
nothing from the keystroke savings.

This is a mild, documented breaking behavior change. It is pre-1.0 (current version `0.4.0`); CHANGELOG/version handling
is a release-PR concern, not part of this plan's units.

---

## Problem Frame

The implicit default subcommand is wrong for `anc` on three counts:

1. **Ambiguous for multi-subcommand CLIs.** Single-purpose tools (rg, fd, jq, cat) have no subcommands, so a bare first
   token is unambiguous. `anc` has four subcommands (`audit`, `completions`, `emit`, `skill`, plus clap's auto `help`),
   so a bare first token is ambiguous: is it a subcommand or a positional for the default? git/cargo/kubectl resolve the
   ambiguity by requiring the explicit verb. `anc` should too.

2. **It violates P6 ("Composable, Predictable Command Structure").** `anc` *is* the agent-native CLI linter. With the
   injection, `anc foo` behaves differently depending on whether `foo` happens to be a recognized subcommand — and a
   typo silently becomes a path. Dogfooding credibility demands the tool follow its own rule.

3. **It is the root cause of a confusing-error class.** Because any unrecognized first token is treated as the `audit`
   PATH target, `anc audit .` produces `error: unexpected argument '.'` instead of clap's native `error: unrecognized
   subcommand 'audit'` with `tip: a similar subcommand exists: 'audit'`. clap's `suggestions` feature is already enabled
   (`Cargo.toml:41` declares `clap = { version = "4.4", features = ["derive", "env"] }` with no `default-features =
   false`, so the default `suggestions` feature is on). Removing the injection lets clap emit the native, did-you-mean
   error in both the text path and the existing JSON-envelope path (`handle_clap_error` in `src/main.rs`).

Removing the injection also deletes ~163 LOC of flag-introspection heuristics (`--` separator handling, `--command`
value-collision pairing, subcommand-scoped-flag detection) plus the large unit-test block that exists only to pin that
behavior.

---

## Scope Boundaries

**In scope:**

- Delete `inject_default_subcommand` and its private helpers from `src/argv.rs`, plus its unit-test block.
- Rewire `src/main.rs::run` to parse raw argv directly (no injection rewrite).
- Simplify `run.invocation` capture (the pre-injection capture point becomes moot — see Key Technical Decisions).
- Update the integration-test surface in `tests/integration.rs` (delete injection-behavior tests; add native-error
  tests).
- Update `tests/scorecard_schema_v05.rs::schema_v05_run_invocation_captures_user_intent_pre_injection` to use a
  now-explicit invocation while still asserting `run.invocation` reflects user intent.
- Update the advertised-surface docs/help that show bare `anc .` (blast-radius list below): `README.md`, `AGENTS.md`,
  `src/cli.rs` (`after_help`), `src/main.rs` (`EXAMPLES_BLOCK`), `schema/scorecard.schema.json` description,
  `src/scorecard/mod.rs` doc comment, and the stale `inject_default_subcommand` mention in `CLAUDE.md` Scorecard v0.5
  section.

**Out of scope:**

- The `--command <name>` flag — it stays exactly as is. Only the implicit-`audit` injection is removed. Post-refactor,
  `anc --command rg` must be typed as `anc audit --command rg`.
- The U2 text-renderer per-row parity bug (separate plan:
  `docs/plans/2026-05-27-002-fix-text-renderer-per-row-parity-plan.md`).
- Any subcommand renaming (e.g. `audit` → `audit`). The verb stays `audit`.
- Scorecard `schema_version` bump — `run.invocation` semantics are preserved, only the capture mechanics simplify.
- `arg_required_else_help` on `Cli` — stays; it is the non-negotiable fork-bomb guard.

---

## Context & Research

### The injection mechanism (to be deleted)

- `src/argv.rs:22-185` — `inject_default_subcommand<I>`. Collects argv; short-circuits bare invocation (`args.len() <=
  1`) as the fork-bomb guard; builds known-subcommand and flag catalogues via clap introspection (`get_subcommands()`,
  `get_arguments()`); scans tokens, pairs value-taking flags with their values, special-cases the POSIX `--` separator,
  tracks subcommand-scoped vs top-level flags, and injects `audit` at position 1 when the first non-flag token is not a
  known subcommand (or when a subcommand-scoped flag appears with no positional). All of this is deleted.
- `src/argv.rs:257-485` — the `#[cfg(test)] mod tests` block. The injection-specific tests
  (`bare_invocation_is_untouched` through `trailing_flags_pass_through`, ~21 tests) are deleted. The `format_invocation`
  tests (`format_invocation_*`, lines 425-484) are **kept** — `format_invocation` survives.

### `format_invocation` and `run.invocation` (must keep working)

- `src/argv.rs:197-255` — `format_invocation(&[OsString]) -> String` and its `quote_arg` / `needs_quoting` helpers are
  **independent of injection** and stay. They shell-quote argv for the scorecard's `run.invocation` field.
- `src/main.rs:56` — `main()` captures `raw_argv = std::env::args_os().collect()` once.
- `src/main.rs:86` — `Cli::try_parse_from(inject_default_subcommand(raw_argv.iter().cloned()))`. This is the sole call
  site; after the refactor it becomes `Cli::try_parse_from(raw_argv.iter().cloned())`.
- `src/main.rs:297` — `let invocation = format_invocation(&raw_argv);`. Already operates on `raw_argv`, which is the
  unmodified argv. **Because the injection only ever rewrote argv `main` passed to clap — never `raw_argv` itself — this
  line already records exactly what the user typed.** Once injection is gone, the pre-injection capture is simply the
  argv; the field semantics are unchanged. The capture point does not need to move; the only change is that there is no
  longer an injection step to capture "before."
- `src/scorecard/mod.rs:257-270` — `RunInfo` doc comment references `inject_default_subcommand`; needs a prose update.
- `schema/scorecard.schema.json:179` — `run` description references `inject_default_subcommand`; needs a prose update.

### clap error handling (the new error must flow through cleanly)

- `src/cli.rs:7-9` — `Cli` derives `Parser` with `arg_required_else_help = true`. Bare `anc` still prints help, exits 2.
- `src/cli.rs:100-191` — `Commands` enum: `Audit`, `Completions`, `Emit`, `Skill`. clap auto-adds `help`.
- `src/main.rs:84-89` — JSON-mode sniff runs on raw argv (`json_error::json_mode_in_argv`), then `try_parse_from`; parse
  failures route to `handle_clap_error(e, json_mode)`.
- `src/main.rs:367-423` — `handle_clap_error`. The `kind => { … }` arm (line 400) handles `InvalidSubcommand` (and every
  other non-help/version variant): text mode prints clap's rendering via `error.print()` (which includes the `tip: a
  similar subcommand exists` line when `suggestions` is enabled); JSON mode distills the first non-empty error line into
  the `{"kind":"usage","error":<slug>,"message":...}` envelope.
- `src/json_error.rs:99-122` — `classify_clap_error` already maps `K::InvalidSubcommand => "invalid-subcommand"`. The
  native error after removal will be `InvalidSubcommand` (for `anc audit .`), which already has a slug. No new mapping
  needed.

### Predecessor plan

- `docs/plans/2026-04-02-003-feat-cli-default-subcommand-and-command-flag-plan.md` (status: shipped) — **added** the
  implicit default. Its Post-Implementation Notes (lines 285-355) document the seven edge cases the injection accreted
  (clap's auto `help` not in `get_subcommands()`, value-flag pairing, subcommand-scoped-flag intent, `--` separator,
  `anc -q` panic fix, `--command`+`--source` conflict, bash completion `compgen` patch). This plan **supersedes the
  default-subcommand portion** of that plan. The `--command` flag and the `anc -q` exit-2 fix (Post-Impl note #5)
  survive independently of injection — the `None` arm in `src/main.rs:147-162` already renders help and exits 2 without
  injection. Note: the predecessor's Post-Impl notes call the injected verb `audit`; the shipped verb is `audit` (the
  prose drifted, the code is `audit`).

### Solutions-docs prior art

- The injection pattern was compounded to
  `~/dev/solutions-docs/best-practices/clap-default-subcommand-via-argv-pre-parse-20260415.md`. After this refactor
  lands, that doc should gain a note that `agentnative-cli` reversed the decision and why (out of scope for this plan's
  units; follow-up via `/ce-compound`).

### Advertised-surface inventory (blast radius)

| Location                       | Lines    | What shows the bare form                                                                                                                                          |
| ------------------------------ | -------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `AGENTS.md`                    | 16-38    | Six `anc .` / `anc . <flag>` examples + `anc --command ripgrep`; line 16 comment "`audit` is implicit"                                                            |
| `AGENTS.md`                    | 46-48    | "must not recurse into `audit .`" prose (verb-drift; also references the guard, which stays)                                                                      |
| `README.md`                    | 67-93    | Quick Start: `anc .`, `anc ./target/release/mycli`, `anc --command ripgrep`, `anc . --binary/--source/--output/--principle/-q`                                    |
| `README.md`                    | 155, 157 | "Isolate with `anc . --binary`" / "`anc . --source`"                                                                                                              |
| `README.md`                    | 206-208  | CLI Reference paragraph: "the first non-flag argument … `audit` is inserted automatically … resolve to `anc audit …`"                                             |
| `README.md`                    | 359      | Scorecard field doc: "default-subcommand injection so it reflects what the user typed (`anc .`, not `anc audit .`)"                                               |
| `src/cli.rs`                   | 19-32    | Top-level `after_help` "Examples:" + the explicit "When the first argument is not a subcommand, `audit` is inserted automatically: `anc .` ≡ `anc audit .`" block |
| `src/main.rs`                  | 335-346  | `EXAMPLES_BLOCK` (already uses `anc audit .` — verify no bare forms; it is clean, but confirm)                                                                    |
| `src/scorecard/mod.rs`         | 257-270  | `RunInfo` doc comment references `inject_default_subcommand`                                                                                                      |
| `schema/scorecard.schema.json` | 179      | `run` property description references `inject_default_subcommand`                                                                                                 |
| `CLAUDE.md`                    | 175      | Scorecard v0.5 section: "`invocation` is captured **before** `inject_default_subcommand` rewrites argv (so `anc .` records as `"anc ."`…)"                        |

Confirmed clean (no bare-form edits needed): generated `completions/anc.{bash,zsh,fish,elvish,powershell}` (injection
was pre-parse, never surfaced in completions); `src/skill_install/` bundle; `PRODUCT.md`; `CONTRIBUTING.md`;
`Cargo.toml` description field. `CLAUDE.md` line 278 ("NEVER remove `arg_required_else_help`") stays — that guard is
unaffected.

---

## Key Technical Decisions

- **Require the explicit verb; delete the injection wholesale.** No partial heuristic, no deprecation shim that warns on
  bare `anc .` then runs it. Pre-1.0 and agent-primary: a clean break with a CHANGELOG note is cheaper than carrying a
  two-mode parser. The injection's entire reason to exist (ergonomic shorthand for humans) is the thing being removed.

- **Rely on clap's already-enabled `suggestions` feature for the native error.** No new dependency, no feature-flag
  change to `Cargo.toml`. `anc audit .` will produce `error: unrecognized subcommand 'audit'` + `tip: a similar
  subcommand exists: 'audit'`; `anc foobar .` produces the same shape without a tip (no near match). Both already route
  through `handle_clap_error`'s `kind` arm and `classify_clap_error`'s `invalid-subcommand` slug.

- **`run.invocation` capture simplifies but does not change semantics.** `main` already builds `run.invocation` from
  `raw_argv` (the unmodified argv), and the injection only rewrote the *copy* handed to clap. Removing injection makes
  the "captured before injection" framing a no-op: the argv `format_invocation` sees is already the verbatim user input.
  The doc comments at `src/argv.rs:194-196`, `src/scorecard/mod.rs:257-270`, `schema/scorecard.schema.json:179`, and
  `CLAUDE.md:175` lose their "before `inject_default_subcommand`" clause; the field still records what the user typed.
  Post-refactor a user types `anc audit .`, so `run.invocation` reads `"anc audit ."` — which is now also what they
  literally typed (no daylight between intent and the verb).

- **`format_invocation` stays.** It is the scorecard's invocation renderer and is orthogonal to injection. Keep the
  function and its tests; only its doc comment's injection clause is edited.

- **`arg_required_else_help` and the `None` arm both stay.** Bare `anc` → help, exit 2 (fork-bomb guard). `anc -q` (a
  global flag, no subcommand) → the `None` arm at `src/main.rs:147-162` renders help, exits 2. Neither depends on
  injection.

---

## Implementation Units

### U1. Delete injection; rewire `main.rs`; trim `argv.rs`

**Goal:** Remove `inject_default_subcommand` and its helpers, parse raw argv directly, keep `format_invocation` and
`run.invocation` working.

**Files:**

- Modify: `src/argv.rs` — delete `inject_default_subcommand` (lines 22-185) and its module doc lines (1-3, 10-21
  framing). Keep `format_invocation`, `quote_arg`, `needs_quoting`. Rewrite the module doc to describe only invocation
  rendering. Edit the `format_invocation` doc comment (lines 187-196) to drop the "captured *before*
  `inject_default_subcommand`" clause.
- Modify: `src/main.rs` — change line 25 import from `use argv::{format_invocation, inject_default_subcommand};` to `use
  argv::format_invocation;`. Change line 86 to `Cli::try_parse_from(raw_argv.iter().cloned())`. Edit the comment block
  at lines 74-79 (drop the injection framing). Verify `EXAMPLES_BLOCK` (lines 335-346) shows only `anc audit .` forms
  (it already does — confirm, no edit expected).
- Modify: `src/scorecard/mod.rs` — edit `RunInfo` doc comment (lines 257-270) to drop the `inject_default_subcommand`
  clause.

**Approach:**

- Delete the function and helpers; run `cargo build` to surface the now-unused import (the compiler flags it as an error
  under `-Dwarnings` in CI, so the import edit is mandatory, not cosmetic).
- Confirm `format_invocation(&raw_argv)` at `src/main.rs:297` still compiles unchanged.

**Test scenarios:**

- `anc audit .` → runs, exits 0/1/2 with a scorecard (unchanged).
- `anc .` → clap error `unexpected argument '.'`-class? No: with no injection, `.` is parsed as the first token; since
  `Cli` has `arg_required_else_help` and no top-level positional, clap treats `.` as an `InvalidSubcommand` → `error:
  unrecognized subcommand '.'`, exit 2. (Verify the exact clap kind during implementation; the assertion in U2 keys on
  exit code 2 + an `unrecognized`/`error` stderr substring, not the precise phrasing.)
- `anc audit .` → `error: unrecognized subcommand 'audit'` + `tip: a similar subcommand exists: 'audit'`, exit 2.
- `anc` (bare) → help, exit 2 (fork-bomb guard intact).

**Verification:**

- `cargo build` exits 0 with no unused-import warning.
- `cargo run -- audit .` (dogfood) produces a scorecard.
- `cargo run -- audit . --output json | jq '.run.invocation'` → `"anc audit ."`.

---

### U2. Update the test surface

**Goal:** Delete tests that pin injection behavior; add tests for the native `unrecognized subcommand` error in both
text and JSON modes; update the `run.invocation` scorecard test to an explicit invocation.

**Files:**

- Modify: `src/argv.rs` — in the `#[cfg(test)] mod tests` block, delete the injection tests
  (`bare_invocation_is_untouched`, `dot_path_gets_audit_injected`,
  `global_short_flag_before_path_gets_audit_injected_in_canonical_position`,
  `global_long_flag_before_path_gets_audit_injected`, `explicit_audit_subcommand_is_untouched`,
  `explicit_completions_subcommand_is_untouched`, `help_flag_alone_is_untouched`, `version_flag_alone_is_untouched`,
  `quiet_flag_alone_is_untouched`, `help_subcommand_passes_through`, `help_subcommand_with_target_passes_through`,
  `command_flag_value_matching_subcommand_name_is_paired`, `command_flag_with_no_positional_injects_audit`,
  `output_flag_with_no_positional_injects_audit`, `equals_form_value_flag_is_recognized_as_subcommand_scoped`,
  `principle_value_flag_pairs_with_numeric_value`, `double_dash_separator_injects_audit_before_separator`,
  `double_dash_alone_passes_through`, `directory_path_gets_audit_injected`, `trailing_flags_pass_through` — ~20 tests).
  Keep all `format_invocation_*` tests. Drop the now-unused `inject_default_subcommand` and `names` imports/helpers from
  the test module; keep `format_invocation` and `args`.
- Modify: `tests/integration.rs` — delete the "Default subcommand tests" block (lines 366-470:
  `test_default_subcommand_dot_matches_explicit_audit`, `test_default_subcommand_preserves_global_flag_before_path`,
  `test_default_subcommand_preserves_global_long_flag_before_path`,
  `test_default_subcommand_passes_trailing_flags_through`, `test_default_subcommand_rejects_nonexistent_path`,
  `test_default_subcommand_does_not_fire_for_bare_flags`, `test_default_subcommand_does_not_fire_for_version`). Keep
  `test_explicit_subcommand_still_works`. Convert `test_command_flag_via_default_subcommand` (504-516) and
  `test_subcommand_flag_alone_injects_audit` (609-622) to explicit `anc audit --command ls` forms (or delete as
  redundant with `test_command_flag_resolves_path_and_runs_behavioral_only`). Convert
  `test_command_flag_value_matching_subcommand_name` (630-642) to `anc audit --command audit`. Convert
  `test_double_dash_separator_with_path` (646-655) to `anc audit -- .`. Keep `test_help_subcommand_works`,
  `test_help_subcommand_with_target`, `test_quiet_flag_alone_exits_2_not_panic`, `test_quiet_long_flag_alone_exits_2`
  (these exercise clap/`None`-arm behavior, not injection).
- Modify: `tests/scorecard_schema_v05.rs` — update `schema_v05_run_invocation_captures_user_intent_pre_injection` (lines
  178-197): change args to the explicit `["audit", &path, "--output", "json"]` and rename to
  `schema_v05_run_invocation_reflects_user_argv`. Keep the assertion that `run.invocation` contains the path and matches
  what was typed; the `" audit "` negative-assertion can be dropped or repointed to assert the invocation equals the
  explicit command.

**Approach:**

- Add new integration tests for the native error:
- `test_unrecognized_subcommand_errors_with_suggestion` — `anc audit .` → exit 2, stderr contains `unrecognized` and
    `audit` (the did-you-mean).
- `test_unrecognized_subcommand_json_envelope` — `anc audit . --output json` → exit 2, stderr is a JSON object with
    `kind == "usage"` and `error == "invalid-subcommand"` (assert via `serde_json`).
- `test_bare_path_now_errors` — `anc .` → exit 2 (no longer runs an audit).

**Test scenarios:**

- All retained explicit-form tests pass unchanged.
- New native-error tests pass in both text and JSON modes.
- `format_invocation` unit tests still pass.

**Verification:**

- `cargo test` green (unit + integration).
- `cargo test --test scorecard_schema_v05` green.
- Manual: `cargo run -- audit . --output json` emits a single-line JSON usage envelope on stderr, exit 2.

---

### U3. Update advertised-surface docs and in-binary help

**Goal:** Replace every bare-invocation example/claim with the explicit `anc audit …` form; delete the "inserted
automatically" prose; refresh doc comments and the JSON-schema description that reference `inject_default_subcommand`.

**Files:**

- Modify: `src/cli.rs` — `after_help` block (lines 19-32): keep the `Examples:` lines (already `anc audit …`);
  **delete** the "When the first argument is not a subcommand, `audit` is inserted automatically: `anc .` ≡ `anc audit
  .` / `anc --command ripgrep` ≡ `anc audit --command ripgrep`" paragraph (lines 27-30). Keep the bare-`anc` fork-bomb
  note (lines 31-32).
- Modify: `README.md` — Quick Start (67-93): rewrite `anc .` → `anc audit .`, `anc ./target/release/mycli` → `anc audit
  ./target/release/mycli`, `anc --command ripgrep` → `anc audit --command ripgrep`, and each `anc . <flag>` → `anc audit
  . <flag>`; update the "(`audit` is the default subcommand)" comment on line 68. Lines 155/157: `anc . --binary` → `anc
  audit . --binary`, `anc . --source` → `anc audit . --source`. CLI Reference paragraph (206-208): replace the "inserted
  automatically … resolve to `anc audit …`" text with a statement that the `audit` verb is required (e.g. "Every audit
  is invoked as `anc audit <path>`; there is no implicit default subcommand"). Line 359: drop the "default-subcommand
  injection" clause from the scorecard field description.
- Modify: `AGENTS.md` — code block (16-38): rewrite every `anc .` / `anc --command ripgrep` to the explicit `anc audit`
  form; delete the line-16 "`audit` is implicit when the first non-flag arg is a path" comment. Fix the line-46-48
  fork-bomb prose verb-drift (`audit .` → `audit .`) while keeping the guard description.
- Modify: `src/scorecard/mod.rs` — `RunInfo` doc comment (257-270): drop "captured *before* `inject_default_subcommand`
  rewrites bare paths into `audit <path>`"; replace with "captured from the user's argv".
- Modify: `schema/scorecard.schema.json` — `run` description (line 179): drop "captured before
  `inject_default_subcommand` rewrites bare paths"; replace with "argv joined with spaces".
- Modify: `CLAUDE.md` — Scorecard v0.5 section (line 175): drop the `inject_default_subcommand` clause; the example
  `run.invocation` now reads `"anc audit ."`.

**Approach:**

- The `schema/scorecard.schema.json` edit is a description-string change only — `schema_version` stays `"1.0"`/`"0.5"`,
  no structural change. Verify no schema drift test keys on the description text (it does not; tests assert shape).
- README/AGENTS.md are consumer-facing → standard feature-branch + PR flow (per branch-discipline: these ship verbatim
  to consumers). `src/cli.rs` and `src/main.rs` help strings ship in the binary → same.

**Test scenarios:**

- `cargo run -- audit --help` shows the trimmed `after_help` with no "inserted automatically" block.
- `cargo run -- emit schema | jq '.properties.run.description'` shows the updated description (no
  `inject_default_subcommand`).
- Doc grep: `rg 'inject_default_subcommand|inserted automatically' src/ README.md AGENTS.md schema/ CLAUDE.md` returns
  no production/doc hits (test-file mentions in `tests/` are deleted in U2; plan files retain historical references).

**Verification:**

- `markdownlint-cli2` clean on edited markdown (the PostToolUse hook handles wrapping).
- `cargo build` regenerates nothing schema-related (description is data, not codegen input).
- Manual read of `anc audit --help` confirms the help reads correctly.

---

## System-Wide Impact

- **Parsing path:** `main()` → `Cli::try_parse_from(raw_argv)` directly. One fewer transformation. The JSON-mode sniff
  (`json_error::json_mode_in_argv`) is unaffected — it always ran on raw argv.
- **Error surface:** `anc audit .` and `anc <bad> .` now produce `InvalidSubcommand` (with did-you-mean where a near
  match exists) instead of a positional-argument error. Both modes already handled by `handle_clap_error` +
  `classify_clap_error`; no new code paths.
- **Scorecard:** `run.invocation` semantics preserved; capture mechanics simplified. `schema_version` unchanged. No
  consumer (site `/score/<tool>`) needs to feature-detect anything new.
- **Completions:** Generated completions are unaffected (injection never appeared in them). No regeneration required —
  but a regeneration is harmless if the release flow runs it anyway.
- **Blast radius (advertised surface):** the table in Context & Research, fully covered by U3.
- **Unchanged invariants:** `arg_required_else_help` on `Cli`; the `None`-arm help+exit-2 for `anc -q`; the `--command`
  flag and its `conflicts_with` constraints; behavioral-audit fork-bomb safety (probes only spawn `--help`/`--version`).

---

## Risk Analysis

| Risk                                                                                    | Likelihood                                                              | Mitigation                                                                                                                                                                                                                                                    |
| --------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Anyone scripting `anc .` (or `anc <path>`) breaks — it now exits 2 instead of auditing. | Low (pre-1.0, agent-primary, most in-repo usage already `anc audit .`). | Document as a `### Changed` CHANGELOG entry at release time; the new error is actionable (`unrecognized subcommand … tip: 'audit'`). v0.x pre-1.0 carries no stability guarantee.                                                                             |
| `anc .` clap error kind differs from expectation (positional vs invalid-subcommand).    | Medium.                                                                 | U2 tests assert exit code 2 + a robust stderr substring, not exact clap phrasing; confirm the actual kind during implementation and (if it is `UnknownArgument` rather than `InvalidSubcommand`) verify `classify_clap_error` already maps it (it maps both). |
| A bare-form example is missed in docs and ships stale.                                  | Low.                                                                    | U3 includes a `rg 'inject_default_subcommand\|inserted automatically'` sweep as a verification gate; the Context table is exhaustive (grepped repo-wide).                                                                                                     |
| solutions-docs pattern doc still recommends the injection to future CLIs.               | Low (external doc).                                                     | Out of scope for this plan's units; flag a `/ce-compound` follow-up to annotate the reversal.                                                                                                                                                                 |

**Biggest open question:** the exact `clap::error::ErrorKind` for `anc .` after removal (is `.` classified as
`InvalidSubcommand` or `UnknownArgument`?). It does not change the design — both kinds already route through
`handle_clap_error` and have `classify_clap_error` slugs — but it determines whether the U2 JSON-envelope test asserts
`error == "invalid-subcommand"` or `"unknown-argument"`. Resolve empirically in U1 with `cargo run -- . --output json`
before finalizing the U2 assertion.

---

## Verification

Run after all three units:

```bash
# Explicit verb works (scorecard, exit 0/1/2)
cargo run -- audit .
cargo run -- audit . --output json | jq '.run.invocation'   # => "anc audit ."

# Bare path now errors
cargo run -- .                       # exit 2, error on stderr
cargo run -- . --output json         # exit 2, JSON usage envelope on stderr

# Unrecognized subcommand → native did-you-mean (text)
cargo run -- audit .                 # error: unrecognized subcommand 'audit'
                                     # tip: a similar subcommand exists: 'audit'  (exit 2)

# Unrecognized subcommand → JSON envelope
cargo run -- audit . --output json   # {"kind":"usage","error":"invalid-subcommand",...} (exit 2)

# Guards intact
cargo run --                         # bare anc → help, exit 2
cargo run -- -q                      # global flag, no subcommand → help, exit 2

# Help no longer advertises the implicit default
cargo run -- audit --help            # no "inserted automatically" block

# Full suite + drift sweep
cargo test
rg 'inject_default_subcommand|inserted automatically' src/ README.md AGENTS.md schema/ CLAUDE.md   # no hits

# Pre-push parity (fmt, clippy -Dwarnings, test, cargo-deny, Windows)
scripts/hooks/pre-push
```

CHANGELOG/version handling (a `### Changed` note documenting that `anc .` now requires `anc audit .`) is a release-PR
concern, applied via the PR body's `## Changelog` section at ship time — not part of this plan's units.
