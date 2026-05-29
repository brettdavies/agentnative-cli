---
title: "fix: Prevent fork bomb when dogfooding agentnative against itself"
type: fix
status: completed
date: 2026-04-02
origin: eng-review session 2026-04-02 (gstack /plan-eng-review)
---

# fix: Prevent fork bomb when dogfooding agentnative against itself

## Overview

When agentnative audits itself (dogfooding via `agentnative audit .` or `cargo test`), behavioral audits spawned child
processes that recursively triggered full audit suites, creating exponential process growth that killed machines. The
fix applies the tool's own P1 principle: bare invocation prints help instead of running `audit .`, and subcommand
probing uses safe `--help`/`--version` suffixes exclusively.

## Problem Frame

agentnative's behavioral audits work by spawning the target binary with various flags (`--help`, `--version`, `--output
json`) and inspecting the output. When the target binary IS agentnative itself (the dogfood case), two paths caused
recursive self-invocation:

1. **NonInteractiveAudit** ran the binary with no args. Bare invocation defaulted to `audit .`, triggering a full audit
   suite in the child. That child's audits spawned grandchildren, each spawning more. Exponential growth.
2. **JsonOutputAudit** probed subcommands bare (`audit --output json`), which also triggered full recursive audit
   suites.

The existing `AGENTNATIVE_CHECK=1` sentinel was intended to prevent this but only guarded NonInteractiveAudit. The root
cause (bare invocation defaulting to `audit .`) was never addressed.

## Requirements Trace

- R1. `agentnative audit .` on the agentnative repo must complete without fork bombing
- R2. `cargo test` must complete all integration tests without fork bombing
- R3. JSON output validation must still work correctly for the user-facing case
- R4. Behavioral audits must still produce meaningful results when dogfooding
- R5. The fix must be systemic (prevent future audits from accidentally causing recursion)
- R6. Safe dogfooding practices must be documented in CLAUDE.md

## Scope Boundaries

- NOT adding `AGENTNATIVE_DEPTH` env var or depth counter (removed during implementation — unnecessary once bare
  invocation prints help)
- NOT adding `--max-depth` CLI flag (YAGNI)
- NOT adding process-level ulimit/cgroup isolation (over-engineered for a CLI linter)

## Context & Research

### Relevant Code and Patterns

- `src/cli.rs:4-6` — `Cli` struct, clap derive, where `arg_required_else_help` was added
- `src/main.rs:61-76` — `run()` entry point, `None` branch (previously defaulted to `audit .`, now `unreachable!()`)
- `src/audits/behavioral/json_output.rs:156-192` — `validate_json_output()` safe_suffixes (removed bare `&[]`)
- `src/audits/behavioral/non_interactive.rs:28-42` — removed `is_child`/`AGENTNATIVE_CHECK` branching
- `src/runner.rs:106,163` — removed `AGENTNATIVE_CHECK=1` from child env
- `brew` CLI behavior — bare invocation prints help, the standard pattern

### Institutional Learnings

- The `AGENTNATIVE_CHECK` sentinel was modeled after Make's `MAKELEVEL` but was an over-engineered solution to a problem
  that should have been solved by following CLI conventions
- The design doc explicitly calls out "Running arbitrary binaries is inherently risky" under Behavioral Audit Error
  Handling, but the recursion case was not anticipated

### What We Learned During Implementation

The original plan proposed a 3-layer depth counter system (AGENTNATIVE_DEPTH env var, depth-aware probing, hard bail in
main.rs). During implementation, we discovered that even bounded recursion (not exponential) overwhelmed the machine: 6
parallel integration tests each triggered nested full audit suites, producing 14+ concurrent ast-grep parsing processes.

The breakthrough: agentnative violated its own P1 principle. `brew` prints help on bare invocation. Every well-designed
CLI does this. One clap attribute (`arg_required_else_help = true`) made bare invocation safe, eliminating all recursion
paths without any depth tracking machinery.

## Key Technical Decisions

- **`arg_required_else_help = true` on Cli**: Bare invocation prints help and exits via clap, before any application
  code runs. This means NonInteractiveAudit's bare probe gets instant help output instead of triggering `audit .`.
  Follows the same pattern as brew, gh, kubectl. This is the primary fix.
- **Remove `&[]` from safe_suffixes in json_output.rs**: Subcommands are only probed with `--help`/`--version` suffixes.
  Bare `subcmd --output json` is unsafe for any CLI with side-effecting subcommands (kubectl apply, docker rm, terraform
  plan). This is both a correctness fix and the second recursion fix.
- **Remove AGENTNATIVE_CHECK sentinel entirely**: No depth tracking needed. Clap handles all child invocations before
  application code runs. The sentinel was dead code once `arg_required_else_help` was added.
- **NonInteractiveAudit always probes bare**: No `is_child` branching needed. Bare invocation is safe for any
  well-behaved CLI (prints help). If a CLI blocks on bare invocation, NonInteractiveAudit correctly reports WARN
  (timeout).
- **json-output probe returns WARN instead of FAIL when safe probes can't validate**: `--help` overrides `--output` in
  most CLIs (clap, cobra, argparse). The audit correctly detects the flag exists but cannot validate JSON output through
  safe probes. WARN is the right severity — it's a probe limitation, not a tool deficiency.
- **All changes in one commit**: No intermediate CI-breaking state.
- **No depth counter, no AGENTNATIVE_DEPTH, no BAIL_DEPTH**: Stripped during eng review. The depth counter was the right
  answer to the wrong question. The right question was: why does bare invocation run an expensive operation?

## Open Questions

### Resolved During Planning/Implementation

- **Where does the fork bomb originate?** Two paths: NonInteractiveAudit bare invocation and JsonOutputAudit bare
  subcommand probing. Both fixed.
- **Why not depth counter?** Even bounded recursion (1 level deep) overwhelmed the machine with 14+ concurrent processes
  during parallel test execution. The depth counter was addressing a symptom, not the root cause.
- **Is `arg_required_else_help` sufficient?** Yes. Clap handles `--help`, `--version`, errors, and now bare invocation
  before `run()` dispatches. No child process ever reaches `Project::discover()` or `BinaryRunner`.

## Implementation Units

- [x] **Unit 1: Add `arg_required_else_help` to Cli**

**Goal:** Make bare invocation print help and exit, following CLI conventions (P1 principle).

**Requirements:** R1, R2, R5

**Files:**

- Modify: `src/cli.rs`
- Modify: `src/main.rs` (change `None` branch to `unreachable!()`, remove `PathBuf` import)

**Verification:**

- `agentnative` (no args) prints help to stderr, exits 2
- `anc` (no args) prints help to stderr, exits 2

---

- [x] **Unit 2: Remove bare subcommand probing from JsonOutputAudit**

**Goal:** Fix the root cause of json_output recursion. Subcommands are only probed with `--help`/`--version` suffixes.

**Requirements:** R1, R3, R5

**Files:**

- Modify: `src/audits/behavioral/json_output.rs`

**Verification:**

- Existing json_output unit tests pass
- Dogfood scorecard shows WARN (not FAIL) for p2-json-output

---

- [x] **Unit 3: Remove AGENTNATIVE_CHECK and simplify NonInteractiveAudit**

**Goal:** Remove dead sentinel code. NonInteractiveAudit always probes bare (safe because of Unit 1).

**Requirements:** R5

**Files:**

- Modify: `src/audits/behavioral/non_interactive.rs`
- Modify: `src/runner.rs` (remove `.env("AGENTNATIVE_CHECK", "1")` from both spawn methods)

**Verification:**

- No references to `AGENTNATIVE_CHECK` remain in `src/`
- Existing non_interactive unit tests pass

---

- [x] **Unit 4: Add regression test and document dogfooding safety**

**Goal:** Test bare invocation prints help. Document the two safety rules in CLAUDE.md.

**Requirements:** R2, R6

**Files:**

- Modify: `tests/integration.rs`
- Modify: `CLAUDE.md`

**Verification:**

- `test_bare_invocation_prints_help` passes
- CLAUDE.md contains `## Dogfooding Safety` section

## System-Wide Impact

- **Interaction graph:** `Cli` (clap) handles bare invocation before any application code. `BinaryRunner` no longer sets
  sentinel env vars. `json_output.rs` and `non_interactive.rs` no longer need depth awareness.
- **Error propagation:** No changes. Audits still report PASS/WARN/FAIL/SKIP normally.
- **State lifecycle risks:** None. No env vars, no filesystem state, no cache.
- **API surface parity:** No external API changes. `arg_required_else_help` changes bare invocation behavior from "run
  audit ." to "print help", which is the correct CLI behavior.
- **Integration coverage:** `test_bare_invocation_prints_help` is the regression test. Existing dogfood tests
  (`test_audit_self`, `test_audit_json_output`, `test_audit_quiet`, etc.) prove end-to-end safety.
- **Unchanged invariants:** All existing CLI behavior with explicit subcommands is unchanged. `audit .`, `audit --output
  json`, `--help`, `--version`, `completions` all work identically.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| `arg_required_else_help` changes bare invocation from `audit .` to help output | This IS the fix. Users who relied on bare invocation as a shortcut must now type `audit .` explicitly. This is standard CLI behavior. |
| json-output audit can't validate JSON via safe probes (--help overrides --output) | Returns WARN instead of FAIL. Accurate severity — probe limitation, not tool deficiency. |
| Future behavioral audit probes subcommands bare | Documented in CLAUDE.md: never probe without --help/--version suffixes. |

## Sources & References

- **Eng review:** gstack /plan-eng-review sessions 2026-04-02 (initial depth counter review + scope reduction review)
- **Design doc:** `~/.gstack/projects/brettdavies-agentnative/brett-main-design-20260327-214808.md`
- **Pattern reference:** `brew` bare invocation behavior (prints help to stderr, exits 0)
- Related code: `src/cli.rs`, `src/main.rs`, `src/audits/behavioral/json_output.rs`,
  `src/audits/behavioral/non_interactive.rs`, `src/runner.rs`
