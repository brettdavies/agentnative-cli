---
id: p5
title: Safe Retries and Explicit Mutation Boundaries
last-revised: 2026-04-22
status: draft
requirements:
  - id: p5-must-force-yes
    level: must
    applicability:
      if: CLI has destructive operations
    summary: Destructive operations (delete, overwrite, bulk modify) require an explicit `--force` or `--yes` flag.
  - id: p5-must-read-write-distinction
    level: must
    applicability:
      if: CLI has both read and write operations
    summary: The distinction between read and write commands is clear from the command name and help text alone.
  - id: p5-must-dry-run
    level: must
    applicability:
      if: CLI has write operations
    summary: A `--dry-run` flag is present on every write command; dry-run output respects `--output json`.
  - id: p5-should-idempotency
    level: should
    applicability:
      if: CLI has write operations
    summary: Write operations are idempotent where the domain allows it — running the same command twice produces the same result.
---

# P5: Safe Retries and Explicit Mutation Boundaries

## Definition

Every CLI MUST support `--dry-run` so agents can preview any command before committing it. Write operations MUST clearly
separate destructive actions from read-only queries. An agent that cannot distinguish a safe read from a dangerous write
will either avoid the tool or execute mutations blindly — both are failure modes.

## Why Agents Need It

Agents retry failed operations by default. If a write operation is not idempotent, a retry creates duplicates, corrupts
data, or trips rate limits. When destructive operations require explicit confirmation (`--force`, `--yes`) and support
preview (`--dry-run`), an agent can safely explore what a command would do before committing to it. Read-only tools are
inherently safe for retries, but they still benefit from help text that names the mutation contract — "this does not
modify state" is a better sentence to put in `--help` than to assume.

## Requirements

**MUST:**

- Destructive operations (delete, overwrite, bulk modify) require an explicit `--force` or `--yes` flag. Without it, the
  tool refuses the operation or enters dry-run mode — never mutates silently.
- The distinction between read and write commands is clear from the command name and help text alone. An agent reading
  `--help` immediately knows whether a command mutates state.
- A `--dry-run` flag is present on every write command. When set, the command validates inputs and reports what it would
  do without executing. Dry-run output respects `--output json` so agents can parse the preview programmatically.

**SHOULD:**

- Write operations are idempotent where the domain allows it — running the same command twice produces the same result
  rather than doubling the effect.

## Evidence

- `--dry-run` flag on commands that create, update, or delete resources.
- `--force` or `--yes` flag on destructive commands.
- Command names that signal intent: `add`, `remove`, `delete`, `create` for writes; `list`, `show`, `get`, `search` for
  reads.
- Dry-run output that shows what *would* change without executing.

## Anti-Patterns

- A `delete` command that executes immediately without `--force` or confirmation.
- Write commands sharing a name pattern with read commands (e.g., a `sync` that silently overwrites local state).
- No `--dry-run` option on bulk operations, where a preview prevents costly mistakes.
- Operations that fail on retry because the first attempt partially succeeded — non-idempotent writes without rollback.

Measured by check IDs `p5-dry-run`, `p5-destructive-guard`. Run `agentnative check --principle 5 .` against your
CLI to see each.
