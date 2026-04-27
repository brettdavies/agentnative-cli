---
id: p7
title: Bounded, High-Signal Responses
last-revised: 2026-04-22
status: draft
requirements:
  - id: p7-must-quiet
    level: must
    applicability: universal
    summary: A `--quiet` flag suppresses non-essential output; only requested data and errors appear.
  - id: p7-must-list-clamping
    level: must
    applicability:
      if: CLI has list-style commands
    summary: "List operations clamp to a sensible default maximum; when truncated, indicate it (`\"truncated\": true` in JSON, stderr note in text)."
  - id: p7-should-verbose
    level: should
    applicability: universal
    summary: A `--verbose` flag (or `-v` / `-vv`) escalates diagnostic detail when agents need to debug failures.
  - id: p7-should-limit
    level: should
    applicability:
      if: CLI has list-style commands
    summary: A `--limit` or `--max-results` flag lets callers request exactly the number of items they want.
  - id: p7-should-timeout
    level: should
    applicability: universal
    summary: A `--timeout` flag bounds execution time so agents are not blocked indefinitely.
  - id: p7-may-cursor-pagination
    level: may
    applicability:
      if: CLI returns paginated results
    summary: Cursor-based pagination flags (`--after`, `--before`) for efficient traversal of large result sets.
  - id: p7-may-auto-verbosity
    level: may
    applicability: universal
    summary: Automatic verbosity reduction in non-TTY contexts (same behavior `--quiet` explicitly requests).
---

# P7: Bounded, High-Signal Responses

## Definition

CLI tools MUST provide mechanisms to control output volume. Agent context windows are finite and expensive — a tool that
dumps 10,000 lines of unfiltered output wastes tokens and may exceed the context limit entirely, breaking the
conversation that invoked it.

## Why Agents Need It

Every token of CLI output an agent consumes has a cost — both monetary (API tokens) and cognitive (context window
capacity). Unbounded output forces the agent to either truncate (losing potentially important data) or consume the full
response (wasting context on noise). Bounded output with `--quiet`, `--verbose`, and `--limit` flags gives the agent
precise control over how much data arrives, keeping responses high-signal and inside budget.

## Requirements

**MUST:**

- A `--quiet` flag suppresses non-essential output: progress indicators, informational messages, decorative formatting.
  When `--quiet` is set, only requested data and errors appear. Implementations typically route diagnostics through a
  macro that short-circuits when quiet is on:

  ```rust
  macro_rules! diag {
      ($cfg:expr, $($arg:tt)*) => {
          if !$cfg.quiet { eprintln!($($arg)*); }
      }
  }
  ```

- List operations clamp to a sensible default maximum. A `list` without `--limit` does not return more than a
  configurable ceiling (e.g., 100 items). If more items exist, the output indicates truncation — `"truncated": true` in
  JSON, a stderr note in text mode.

**SHOULD:**

- A `--verbose` flag (or `-v` / `-vv`) escalates diagnostic detail when agents need to debug failures.
- A `--limit` or `--max-results` flag lets callers request exactly the number of items they want.
- A `--timeout` flag bounds execution time. An agent waiting indefinitely on a hung network call cannot proceed.

**MAY:**

- Cursor-based pagination flags (`--after`, `--before`) for efficient traversal of large result sets.
- Automatic verbosity reduction in non-TTY contexts (the same behavior `--quiet` explicitly requests).

## Evidence

- `--quiet` flag with a falsey-value parser and env-var binding.
- A diagnostic macro (or equivalent gate) that short-circuits when `quiet` is true.
- `--limit` or `--max-results` on every list / search command.
- Pagination clamping logic (e.g., `min(requested, MAX_RESULTS)`).
- `--timeout` flag with a sensible default.
- `--verbose` flag for diagnostic escalation.
- A `suppress_diag()` method that returns true when quiet is set or when the output format is JSON / JSONL.

## Anti-Patterns

- List commands that return all results with no default limit — an agent listing 50,000 items floods its context window.
- No `--quiet` flag — agents consuming JSON output still receive interleaved diagnostic text on stderr.
- `--verbose` as the only output control. If there is no way to reduce output, bounded responses do not exist.
- Progress bars or spinners that write to stderr in non-TTY contexts, adding noise to agent logs.
- No `--timeout` on network operations. A stalled request blocks the agent indefinitely.

Measured by check IDs `p7-quiet`, `p7-limit`, `p7-timeout`. Run `agentnative check --principle 7 .` against
your CLI to see each.
