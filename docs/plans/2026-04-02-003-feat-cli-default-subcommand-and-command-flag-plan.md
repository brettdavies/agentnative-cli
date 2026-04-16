---
title: "feat: Default subcommand (anc .) and --command flag for PATH lookup"
type: feat
status: shipped
date: 2026-04-02
deepened: 2026-04-02
shipped: 2026-04-15
origin: ~/.gstack/projects/brettdavies-agentnative/brett-main-design-20260327-214808.md
---

# feat: Default subcommand (anc .) and --command flag for PATH lookup

## Status

**Shipped on `dev`** — both implementation units complete plus a follow-on cluster of edge-case fixes surfaced by
post-merge code review.

- PR [#12](https://github.com/brettdavies/agentnative/pull/12) — initial implementation (commit `4afef67`, merged
  2026-04-15)
- PR [#13](https://github.com/brettdavies/agentnative/pull/13) — 7 edge-case fixes + refactor surfaced by `/ce-review`
  of the merged PR (open against `dev`)
- Pattern documented for reuse: `docs/solutions/best-practices/clap-default-subcommand-via-argv-pre-parse-20260415.md`

See **Post-Implementation Notes** at the end for the delta between the planned design
and what actually shipped.

## Overview

Two CLI contract additions from the design doc: (1) `anc .` should work as shorthand for `anc check .`, making `check`
the implicit default subcommand; (2) `--command <name>` resolves a binary from PATH via `which` for behavioral-only
checking. Both improve ergonomics for the primary use case.

## Problem Frame

Today, `anc .` fails because `.` is not a recognized subcommand. Users must always type `anc check .`. The design doc
(line 126) explicitly shows `anc .` as a supported invocation. Similarly, there's no way to check a binary already on
PATH without manually resolving its location — the design doc (line 209) specifies `--command <name>` for this.

## Requirements Trace

- R1. `anc .` and `agentnative .` must behave identically to `anc check .` and `agentnative check .`
- R2. `anc` with no arguments must still print help and exit with code 2 (handled by clap's
  `arg_required_else_help=true` — non-negotiable fork bomb safety constraint)
- R3. `--command <name>` resolves binary from PATH and runs behavioral checks only
- R4. `--command` and `path` are mutually exclusive
- R5. All existing CLI behavior (exit codes, flags, output formats) unchanged

## Scope Boundaries

- No changes to check logic, scoring, or output format
- `--command` is behavioral-only (no source or project checks)
- No changes to the `completions` subcommand

## Context & Research

### Relevant Code and Patterns

- `src/cli.rs` — clap derive definitions, `arg_required_else_help = true` safety constraint
- `src/main.rs` — CLI routing, `run()` function, `None => unreachable!()` arm
- `src/project.rs` — `Project::discover()` already handles file paths (sets `language: None`, skips source/project
  checks automatically)
- `src/runner.rs` — `BinaryRunner::new()`, takes a binary path
- `src/error.rs` — `AppError` enum: `ProjectDetection(anyhow::Error)` and `Io(std::io::Error)`

### Institutional Learnings

- `~/dev/solutions-docs/logic-errors/cli-linter-fork-bomb-recursive-self-invocation-20260401.md` —
  `arg_required_else_help` is a non-negotiable safety constraint. Bare invocation MUST print help, not run `check .`.

## Key Technical Decisions

- **Flatten approach rejected**: The idiomatic clap pattern for default subcommands is `#[command(flatten)]` — flatten
  `Check` args into the top-level struct. However, this conflicts with `arg_required_else_help=true`: clap cannot
  require args globally while also treating the entire flattened variant as optional. Since `arg_required_else_help` is
  a non-negotiable safety constraint (fork bomb prevention, see solutions-docs citation), flatten is not viable.
- **Chosen approach: external pre-parse with subcommand injection**: Before calling clap, scan argv to determine if the
  user is invoking a path-based check without the explicit `check` subcommand. If so, inject `check` into the arg list
  and parse with `Cli::parse_from()`. This preserves all existing clap behavior untouched and keeps the safety
  constraint intact.
- **Pre-parse must scan past leading global flags**: `anc -q .` and `anc --quiet .` are realistic invocations. The
  pre-parse cannot just check `argv[1]` — it must skip known global flags to find the first non-flag argument, then
  decide whether to inject `check`. Use clap introspection (`Cli::command().get_subcommands()`) to derive the known
  subcommand list at runtime instead of maintaining a fragile static list.
- **`--command` resolves via `which` on Unix, `where` on Windows**: Shell out to `which`/`where` rather than adding a
  crate dependency. Gate with `#[cfg(unix)]` / `#[cfg(windows)]` per existing project conventions (CLAUDE.md).
- **`Project::discover()` already handles file paths**: When given an executable file, `discover()` sets `language:
  None` and the file as `path`. Source checks are skipped (no language), project checks are skipped (not a dir). No new
  `Project::from_binary()` constructor needed — just pass the resolved path to `discover()`.

## Open Questions

### Resolved During Planning

- **Will default subcommand break `arg_required_else_help`?**: No — the pre-parse only activates when argv has
  arguments. Bare invocation (`anc` with no args) still hits clap's help gate before any pre-parse logic runs.
- **Should `anc check` still work?**: Yes — all existing invocations continue to work. The pre-parse only triggers when
  the first non-flag arg is not a known subcommand.
- **Does `anc --command rg` work via default subcommand?**: Yes — `--command` is not a known subcommand, so the
  pre-parse injects `check`, producing `anc check --command rg`. This is correct by design since `--command` belongs to
  the `Check` subcommand.
- **Does `Project::from_binary()` need to be created?**: No. `Project::discover()` already handles executable file paths
  correctly — sets `language: None` (skipping source checks), and `is_dir()` returns false (skipping project checks).
  Just pass the resolved binary path.

### Deferred to Implementation

- **Typo handling** _(status: as planned)_: `anc chekc .` (typo of `check`) becomes `anc check chekc .` where `chekc`
  becomes the path. Produces "path does not exist: chekc" instead of "unrecognized subcommand 'chekc'." Acceptable for
  v0.1 — the error is still actionable. Note: the `help` subcommand is special-cased in PR #13 because clap's
  auto-generated `help` is not returned by `get_subcommands()` and would otherwise hit this path.
- **Clap error message context** _(status: as planned)_: When pre-parse injects `check`, clap error messages reference
  the `check` subcommand context. Users who typed `anc . --bogus` see errors mentioning `check` in the usage line. Minor
  UX imperfection, acceptable for v0.1.

## High-Level Technical Design

> _This illustrates the intended approach and is directional guidance for review, not implementation specification. The
> implementing agent should treat it as context, not code to reproduce._

```text
argv = ["anc", "-q", ".", "--output", "json"]
         │
         ▼
    skip leading global flags: -q is a known global flag, skip it
         │
         ▼
    first non-flag arg is "." — is it a known subcommand?
         │
    NO ──┼── YES ──▶ pass to clap as-is
         │
         ▼
    inject "check" after the binary name: ["anc", "check", "-q", ".", "--output", "json"]
         │
         ▼
    Cli::parse_from(modified_args)
```

For `--command`:

```text
argv = ["anc", "--command", "ripgrep"]
         │
         ▼
    pre-parse: "--command" is not a known subcommand → inject "check"
         │
         ▼
    clap parses: ["anc", "check", "--command", "ripgrep"]
         │
         ▼
    resolve via which("ripgrep") → /usr/bin/rg
         │
         ▼
    Project::discover(resolved_path) — language: None → behavioral checks only
```

## Implementation Units

- [x] **Unit 1: Default subcommand — `anc .` as `anc check .`**

**Goal:** Make `anc .` (and `agentnative .`) work by injecting `check` when the first non-flag arg is not a known
subcommand.

**Requirements:** R1, R2, R5

**Dependencies:** None

**Files:**

- Modify: `src/main.rs` — add pre-parse logic before `Cli::parse()`
- Test: `tests/integration.rs` — add tests for default subcommand behavior

**Approach:**

- Before calling `Cli::parse()`, collect `std::env::args()` into a Vec
- If args.len() <= 1 (bare invocation), skip pre-parse entirely — let clap handle it via `arg_required_else_help`
- Use clap introspection (`Cli::command().get_subcommands()`) to build the known subcommand set at runtime, avoiding a
  static list that drifts
- Scan args starting from index 1, skipping known global flags (`-q`, `--quiet`, `-h`, `--help`, `-V`, `--version`). The
  first non-flag arg is the candidate.
- If the candidate is a known subcommand name → pass to clap unchanged
- If the candidate is not a known subcommand (looks like a path) → insert `"check"` at position 1 in the args vec
- Use `Cli::parse_from()` with the (possibly modified) args vec
- Keep the pre-parse logic minimal and clearly commented to explain the safety constraint

**Patterns to follow:**

- Keep the pre-parse logic minimal and clearly commented to explain the safety constraint

**Test scenarios:**

- Happy path: `anc .` produces the same output as `anc check .`
- Happy path: `anc . --output json` produces valid JSON (flags pass through)
- Happy path: `anc . -q` respects quiet flag
- Happy path: `anc -q .` respects quiet flag (global flag before path)
- Happy path: `anc --quiet .` respects quiet flag (long form before path)
- Happy path: `anc check .` still works (explicit subcommand unchanged)
- Happy path: `anc completions bash` still works (other subcommands unaffected)
- Edge case: `anc` with no args still prints help (exit 2) — safety constraint preserved
- Edge case: `anc --help` still prints help (flags are not treated as paths)
- Edge case: `anc --version` still prints version
- Edge case: `anc -q` with no path still prints help (not treated as default subcommand)

**Verification:**

- All existing integration tests pass unchanged
- New tests for `anc .` and `anc -q .` behavior pass
- `test_bare_invocation_prints_help` still passes (safety constraint)

---

- [x] **Unit 2: `--command <name>` flag for PATH lookup**

**Goal:** Add a `--command` flag to the `Check` subcommand that resolves a binary from PATH and runs behavioral checks.

**Requirements:** R3, R4, R5

**Dependencies:** Unit 1 (so `anc --command rg` also works via default subcommand — by design, `--command` is not a
known subcommand, so pre-parse injects `check`)

**Files:**

- Modify: `src/cli.rs` — add `--command` arg to `Check`, with conflicts_with for `path`
- Modify: `src/main.rs` — handle `--command` by resolving path and calling `Project::discover()`
- Test: `tests/integration.rs` — add tests for `--command`

**Approach:**

- Add `command: Option<String>` to the `Check` variant in `cli.rs` with `#[arg(long, conflicts_with = "path")]`
- In `main.rs`, when `command` is `Some(name)`:
- Resolve using `which` on Unix (`std::process::Command::new("which").arg(&name)`), `where` on Windows
- If resolution fails, return `AppError::ProjectDetection` wrapping an anyhow error: "command '{name}' not found on
  PATH"
- Call `Project::discover(&resolved_path)` — this already sets `language: None` (skipping source checks) and `is_dir()`
  is false (skipping project checks), so behavioral-only behavior is automatic
- Update `--help` text to document the flag

**Patterns to follow:**

- `src/project.rs` `Project::discover()` for how Project handles executable file paths
- `src/error.rs` `AppError::ProjectDetection` for error wrapping

**Test scenarios:**

- Happy path: `anc check --command ls` runs behavioral checks against `/bin/ls`, output contains only behavioral check
  results (no source or project checks)
- Happy path: `anc --command echo` works via default subcommand (pre-parse injects `check`)
- Happy path: `anc check --command ls --output json` produces valid JSON containing only behavioral-layer results
- Error path: `anc check --command nonexistent-binary-xyz` exits with code 2 and error message "command
  'nonexistent-binary-xyz' not found on PATH"
- Error path: `anc check --command rg .` — conflicts_with prevents both path and command

**Verification:**

- `--command` resolves a known binary and runs behavioral checks only
- `--command` with unknown binary produces a clear, actionable error
- Existing behavior unchanged

## System-Wide Impact

- **Interaction graph:** The pre-parse logic sits before clap parsing, so all downstream behavior is unchanged once clap
  receives the modified args.
- **Error propagation:** `which` failure maps to `AppError::ProjectDetection` (exit code 2) with a clear message.
- **Unchanged invariants:** `arg_required_else_help = true` remains on `Cli`. All existing exit codes, output formats,
  and check behavior unchanged. The pre-parse is additive only.
- **API surface parity:** `--command` flag appears in `--help` and shell completions. Completions need regenerating
  after this change (coordinate with release infrastructure plan).

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Pre-parse heuristic misidentifies a flag as a path | Use clap introspection for subcommand list; scan past known global flags |
| Typos like `anc chekc .` produce path-not-found instead of subcommand error | Acceptable for v0.1 — error is still actionable |
| `which` not available on Windows | Use `where` on Windows via `cfg(target_os)`, consistent with existing `libc` gating |
| Adding `--command` changes shell completions | Regenerate completions after implementation (coordinate with Plan 002) |
| Default subcommand breaks fork bomb safety | Bare invocation still hits `arg_required_else_help` before pre-parse |
| Clap error messages reference injected `check` context | Minor UX imperfection, acceptable for v0.1 |

## Sources & References

- **Design doc:** `~/.gstack/projects/brettdavies-agentnative/brett-main-design-20260327-214808.md` (lines 126, 209)
- **Safety constraint:** `~/dev/solutions-docs/logic-errors/cli-linter-fork-bomb-recursive-self-invocation-20260401.md`
- Related code: `src/cli.rs`, `src/main.rs`, `src/argv.rs`, `src/project.rs`, `src/error.rs`

## Post-Implementation Notes

What the planning sections above don't capture: the design above shipped in PR #12 and worked, but `/ce-review` of the
  merged commit surfaced seven edge cases. PR #13 closed all of them. This section is the delta — readers picking up
  later need both halves.

### Final code locations

- `src/argv.rs` (new module, ~340 lines including 19 unit tests) — owns `inject_default_subcommand` and the supporting
  helpers (`build_known_subcommand_set`, `build_value_flag_set`, `build_top_level_flag_set`, `consumes_next`,
  `base_form`). Extracted from `src/main.rs` in PR #13's refactor.
- `src/main.rs` (203 lines, down from 538 mid-PR) — `run()`, `resolve_command_on_path()`, and the `match cli.command`
  arm.
- `src/cli.rs` — `Cli` derive plus `after_help` block, `value_hint = ValueHint::CommandName` on `--command`, and
  `conflicts_with = "source"` on `--command`.
- `scripts/generate-completions.sh` — `post_process()` function that patches bash completion to swap `compgen -f` →
  `compgen -c` for `--command` (clap_complete's bash backend ignores `ValueHint::CommandName`).

### Edge cases resolved beyond the original design

1. **Clap's auto `help` subcommand is NOT in `get_subcommands()`** — `anc help` and `anc help check` were misclassified
   as paths. Fixed by appending `"help"` to the known set.
2. **Value-taking flags must be paired with their values during scanning** — `anc --command check` mis-routed because
   `check` (the value) was treated as the explicit subcommand. Fixed by walking clap's `get_arguments()` for both `Cli`
   and every subcommand to learn which flags consume the next token.
3. **Subcommand-scoped flags imply default-subcommand intent even with no positional** — `anc --command rg` and `anc
   --output json --source` produced clap errors. Fixed by tracking whether any encountered flag is subcommand-scoped
   (not in the top-level Cli flag set) and injecting `check` if so when no positional was found.
4. **POSIX `--` separator** — `anc -- .` was untested and ill-defined. Now injects `check` before the separator so clap
   routes the remaining tokens to Check.
5. **`arg_required_else_help` only fires on zero args** — `anc -q` (or `--quiet`) reaches `match cli.command` with
   `command = None` and previously panicked via `unreachable!()`. The `None` arm now renders help to stderr and exits 2.
   This was a pre-existing bug, not introduced by this plan, but surfaced under the new ergonomics.
6. **`--command` + `--source` is contradictory** — added `conflicts_with = "source"` so clap rejects the combination at
   parse time instead of producing a silent empty result.
7. **Bash completion suggests file paths instead of PATH commands** — added `value_hint = ValueHint::CommandName`.
   zsh/fish/elvish honor it; bash needs the post-generation `sed` patch documented above.

### Design-time decisions that survived contact with reality

- `flatten` rejection (Key Technical Decisions §1) — confirmed correct; `flatten` remains incompatible with
  `arg_required_else_help`.
- Clap introspection for subcommand list (Key Technical Decisions §2) — proved its worth: introspection-driven flag
  catalogues were essential for the value-pairing fix in PR #13. A static list would have multiplied the failure modes.
- `which`/`where` shell-out (Key Technical Decisions §3) — works as designed; the cross-reviewer security take in PR #13
  noted hostile-PATH redirection as a low-risk residual but recommended the `which` crate as a future improvement, not a
  blocker.
- `Project::discover()` already handles file paths (Key Technical Decisions §4) — true, no new constructor needed.

### Test parity

| Stage | Unit | Integration | Notes |
|-------|------|-------------|-------|
| Pre-Plan 003 baseline | 233 | 12 | from commit `45b5234` |
| After PR #12 (initial impl) | 244 | 26 | +11 unit, +14 integration |
| After PR #13 (edge-case fixes) | 253 | 34 | +9 unit, +8 integration |

### Plan 002 coordination

The completions regeneration noted in System-Wide Impact happened twice (once per PR); both PRs commit the regenerated
  `completions/anc.{bash,zsh,fish,elvish,powershell}` files. No separate Plan 002 step needed for this plan's completion
  deltas.

### Solutions-docs follow-up

The full pattern (with all seven gotchas, before/after code, and the working invocation matrix) was compounded into:

- `~/dev/solutions-docs/best-practices/clap-default-subcommand-via-argv-pre-parse-20260415.md`

Future Rust CLIs in this orbit that want a default subcommand should read that doc before reimplementing — the cluster
  of edge cases is the kind of footgun that's much cheaper to avoid than rediscover.
