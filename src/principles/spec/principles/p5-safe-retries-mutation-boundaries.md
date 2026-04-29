---
id: p5
title: Safe Retries and Explicit Mutation Boundaries
last-revised: 2026-04-22
status: active
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

Every CLI with write operations MUST support `--dry-run` so agents can preview a mutation before committing it. Commands
MUST make the read-vs-write distinction visible from name and `--help` alone, and destructive writes MUST require
explicit confirmation. An agent that cannot distinguish a safe read from a dangerous write will either avoid the tool or
execute mutations blindly — both are failure modes.

## Why Agents Need It

Agent harnesses commonly retry failed operations. If a write operation is not idempotent, a retry creates duplicates,
corrupts data, or trips rate limits. When destructive operations require explicit confirmation (`--force`, `--yes`) and
support preview (`--dry-run`), an agent can safely explore what a command would do before committing to it. Read-only
tools are inherently safe for retries, but they still benefit from help text that names the mutation contract — "this
does not modify state" is a better sentence to put in `--help` than to assume.

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

Measured by check IDs `p5-dry-run`, `p5-destructive-guard`. Run `agentnative check --principle 5 .` against your CLI to
see each.

## Pressure test notes

### 2026-04-27 — Show HN launch red-team pass

Adversarial review via `compound-engineering:ce-adversarial-document-reviewer` ahead of the v0.3.0 launch. Findings
recorded verbatim per `principles/AGENTS.md` § "Pressure-test protocol".

- **[edit]** *Internal inconsistency.* "Definition opens 'Every CLI MUST support `--dry-run`' as universal, but
  `p5-must-dry-run` is gated on 'CLI has write operations' — read-only CLIs would falsely fail this prose claim."
  Resolved: Definition sentence 1 narrowed to "Every CLI with write operations MUST support `--dry-run`..." Read-only
  CLIs are no longer falsely accused by the prose.
- **[edit]** *Internal inconsistency.* "Definition's 'Write operations MUST clearly separate destructive actions from
  read-only queries' garbles the read/write MUST. The actual requirement is read-vs-write distinction; 'destructive vs
  read-only' is a different axis (writes can be non-destructive, e.g., `create`)." Resolved: Definition sentence 2
  rewritten to "Commands MUST make the read-vs-write distinction visible from name and `--help` alone, and destructive
  writes MUST require explicit confirmation." The two axes are now stated separately.
- **[later]** *Internal inconsistency.* "`--force`/`--yes` MUST + P1 `--no-interactive` MUST should compose (agent path
  is `--force --no-interactive`); composition isn't called out, leaving the 'without it, the tool refuses or enters
  dry-run' clause ambiguous when stdin is non-TTY." Deferred: tightening the MUST to specify error-vs-dry-run behavior
  under `--no-interactive` modifies the bullet's contract semantics. Bundled with other MUST-content cleanups for a
  v0.4.0 PR.
- **[later]** *Must-vs-should.* "`read-write-distinction` MUST hinges on 'clear from command name and help text alone' —
  subjective and unverifiable by `anc`. The `sync` anti-pattern proves the bar is taste, not a checkable property."
  Deferred: rewriting to a verifiable form ("Help text for every write command MUST contain an explicit mutation
  statement; command names SHOULD signal intent") creates a new SHOULD-shape claim, which is a `requirements[]` change.
  Coupled-release fires firmly. Defer to v0.4.0 with explicit registry-coordination plan.
- **[later]** *Prior art.* "Principle prescribes flag *names* (`--dry-run`, `--force`, `--yes`) without naming the
  contract behind them. kubectl `--dry-run=server|client`, Terraform `plan`/`apply`, apt `--simulate`, rsync `-n` all
  satisfy the contract under different surfaces." Deferred: worth revisiting whether to add a
  'name-or-contract-equivalent' clause that names the contract first and treats canonical flag spelling as one
  realization. Hold for v0.4.0 alongside the verifiability rewrite above.
- **[wontfix]** *Must-vs-should.* "'Why Agents Need It' leans on retry-safety, then idempotency lands as SHOULD. If
  retries are the framing, idempotency-where-domain-allows is the load-bearing property; `--dry-run` is mitigation, not
  cure." Rationale: domain-gated idempotency genuinely cannot be a universal MUST (some domains forbid it: append-only
  logs, payment capture). The current SHOULD is correct; the prose framing in "Why Agents Need It" is fine because it
  explains *why* idempotency matters when it is available, not that it is universally required.
- **[edit]** *Vague agent-native.* "'Agents retry failed operations by default' — true for Claude Code/Cursor/Aider tool
  loops; not universally true for one-shot harnesses or human-in-the-loop agents." Resolved: "Why Agents Need It" hedged
  to "Agent harnesses commonly retry failed operations." Same operational point; more accurate across harness shapes.
