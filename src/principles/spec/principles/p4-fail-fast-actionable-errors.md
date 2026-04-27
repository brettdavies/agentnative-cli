---
id: p4
title: Fail Fast with Actionable Errors
last-revised: 2026-04-22
status: draft
requirements:
  - id: p4-must-try-parse
    level: must
    applicability: universal
    summary: Parse arguments with `try_parse()` instead of `parse()` so `--output json` can emit JSON parse errors.
  - id: p4-must-exit-code-mapping
    level: must
    applicability: universal
    summary: Error types map to distinct exit codes (0, 1, 2, 77, 78).
  - id: p4-must-actionable-errors
    level: must
    applicability: universal
    summary: Every error message contains what failed, why, and what to do next.
  - id: p4-should-structured-enum
    level: should
    applicability: universal
    summary: Error types use a structured enum (via `thiserror` in Rust) with variant-to-kind mapping for JSON serialization.
  - id: p4-should-gating-before-network
    level: should
    applicability:
      if: CLI makes network calls
    summary: Config and auth validation happen before any network call (three-tier dependency gating).
  - id: p4-should-json-error-output
    level: should
    applicability: universal
    summary: "Error output respects `--output json`: JSON-formatted errors go to stderr when JSON output is selected."
---

# P4: Fail Fast with Actionable Errors

## Definition

CLI tools MUST detect invalid state early, exit with a structured error, and tell the caller three things: what failed,
why, and what to do next. An error that says "operation failed" gives an agent nothing to act on.

## Why Agents Need It

Agents operate in a retry loop: attempt, observe, decide. When an error is vague or unstructured — a bare stack trace, a
one-word failure, a mixed-channel splurge — the agent cannot tell whether to retry, re-authenticate, fix configuration,
or escalate to the user. Distinct exit codes with actionable messages let the agent act correctly on the first read. The
difference between exit code 77 (re-authenticate) and exit code 78 (fix config) determines whether the agent retries
OAuth or asks the user to check their config file. Getting that wrong wastes entire conversation turns.

## Requirements

**MUST:**

- Parse arguments with `try_parse()` instead of `parse()`. Clap's `parse()` calls `process::exit()` directly, bypassing
  custom error handlers — which means `--output json` cannot emit JSON parse errors. `try_parse()` returns a `Result`
  the tool can format:

  ```rust
  let cli = Cli::try_parse()?;
  ```

- Error types map to distinct exit codes. At minimum:

  | Code | Meaning                       |
  | ---: | ----------------------------- |
  |    0 | Success                       |
  |    1 | General command error         |
  |    2 | Usage / argument error        |
  |   77 | Auth / permission error       |
  |   78 | Configuration error           |

- Every error message contains **what failed**, **why**, and **what to do next**. Example:

  ```text
  Authentication failed: token expired (expires_at: 2026-03-25T00:00:00Z).
  Run `tool auth refresh` or set TOOL_TOKEN.
  ```

**SHOULD:**

- Error types use a structured enum (via `thiserror` in Rust) with variant-to-kind mapping for JSON serialization.
  Agents match on error kinds programmatically rather than parsing message text.
- Config and auth validation happen before any network call. A three-tier dependency gating pattern (meta-commands,
  local-only commands, network commands) fails at the earliest possible point.
- Error output respects `--output json`: JSON-formatted errors go to stderr when JSON output is selected.

## Evidence

- `Cli::try_parse()` in `main()`, not `Cli::parse()`.
- Error enum with `#[derive(Error)]` and distinct variants for config, auth, and command errors.
- `exit_code()` method on the error type returning variant-specific codes.
- `kind()` method returning a machine-readable string for JSON serialization.
- `run()` function returning `Result<(), AppError>`, not calling `process::exit()` internally.
- Error messages containing remediation steps ("run X" or "set Y") alongside the cause.

## Anti-Patterns

- `Cli::parse()` anywhere in the codebase — it silently prevents JSON error output.
- `process::exit()` in library code or command handlers. Only `main()` may call it, after all error handling.
- A single catch-all error variant that maps everything to exit code 1.
- Error messages that state the symptom without the cause or fix ("Error: request failed").
- Panics (`unwrap()`, `expect()`) on recoverable errors in production code paths.

Measured by check IDs `p4-bad-args`, `p4-process-exit`, `p4-unwrap`, `p4-exit-codes`. Run
`agentnative check --principle 4 .` against your CLI to see each.
