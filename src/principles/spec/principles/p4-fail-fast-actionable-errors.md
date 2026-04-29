---
id: p4
title: Fail Fast with Actionable Errors
last-revised: 2026-04-22
status: active
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
    summary: Config and auth validation happen before any network call, failing at the earliest possible point.
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

- Error types map to distinct exit codes. Use 77 when the CLI has an auth surface and 78 when it has a config surface;
  0/1/2 are universal:

| Code | Meaning                 |
| ---: | ----------------------- |
|    0 | Success                 |
|    1 | General command error   |
|    2 | Usage / argument error  |
|   77 | Auth / permission error |
|   78 | Configuration error     |

  These codes blend the bash 0/1/2 convention with BSD `sysexits.h` 77/78 (`EX_NOPERM`, `EX_CONFIG`); the result is the
  de-facto agent-facing dialect, not strict `sysexits.h` compliance.

- Every error message contains **what failed**, **why**, and **what to do next**. Example:

  ```text
  Authentication failed: token expired (expires_at: 2026-03-25T00:00:00Z).
  Run `tool auth refresh` or set TOOL_TOKEN.
  ```

**SHOULD:**

- Error types use a structured enum (via `thiserror` in Rust) with variant-to-kind mapping for JSON serialization.
  Agents match on error kinds programmatically rather than parsing message text.
- Config and auth validation happen before any network call, failing at the earliest possible point. The structural
  three-tier definition (meta-commands, local-only commands, network commands) lives in P6 (`p6-should-tier-gating`);
  this requirement specifies the network-call ordering consequence.
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

Measured by check IDs `p4-bad-args`, `p4-process-exit`, `p4-unwrap`, `p4-exit-codes`. Run `agentnative check --principle
4 .` against your CLI to see each.

## Pressure test notes

### 2026-04-27 — Show HN launch red-team pass

Adversarial review via `compound-engineering:ce-adversarial-document-reviewer` ahead of the v0.3.0 launch. Findings
recorded verbatim per `principles/AGENTS.md` § "Pressure-test protocol".

- **[edit]** *Internal inconsistency.* "Three-tier gating is labeled identically as a SHOULD in both P4
  (`p4-should-gating-before-network`) and P6 (`p6-should-tier-gating`) — same pattern, two homes, no cross-reference.
  Readers can't tell which is canonical, and a CLI that satisfies one auto-satisfies the other." Resolved: P4's bullet
  now focuses on the network-call ordering consequence and points to P6 as the canonical home of the structural
  three-tier definition. Frontmatter summary tightened to match. Requirement ID is unchanged so CLI registry pinning is
  unaffected.
- **[edit]** *Must-vs-should.* "`p4-must-exit-code-mapping` is `applicability: universal` and the prose says 'At
  minimum' 0/1/2/77/78 — but a CLI with no auth surface and no config file legitimately has nothing to assign to either
  77 or 78, and the MUST forces empty-by-construction error variants. Same shape as P6, which correctly gates
  `p6-must-timeout-network` behind `if: CLI makes network calls`." Resolved: prose now reads "Use 77 when the CLI has an
  auth surface and 78 when it has a config surface; 0/1/2 are universal." Frontmatter summary stays universal because
  the *mapping discipline* is universal even if the specific 77/78 codes are conditional. The summary-prose drift is a
  known launch-week tradeoff; full alignment of the summary text is on the v0.4.0 punch list.

- **[edit]** *Prior art.* "77/78 align with BSD `sysexits.h` (`EX_NOPERM`, `EX_CONFIG`) — the alignment is a strength
  but neither P2 nor P4 cites BSD sysexits, leaving an HN commenter to 'discover' it as a gotcha." Resolved: added a
  one-liner under the P4 exit-code table acknowledging the `sysexits.h` alignment. Same sentence added to P2's exit-code
  table for consistency.
- **[later]** *Must-vs-should.* "`p4-must-try-parse` names a clap-specific Rust API in a `applicability: universal`
  MUST. A Go/Python/Node CLI has no `try_parse()`. The underlying requirement — 'argument-parse failures route through
  the same error/output formatter as runtime errors, not a library-internal `process::exit()`' — is universal; the API
  name is not." Deferred: language-neutralizing the bullet ("Argument parsing returns a structured error rather than
  calling `process::exit()` internally; in Rust+clap, this means `try_parse()` not `parse()`") drifts the frontmatter
  summary. Bundled with P6's SIGPIPE and `global = true` rewrites for a coordinated v0.4.0 language-neutralization PR.
