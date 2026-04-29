---
id: p1
title: Non-Interactive by Default
last-revised: 2026-04-22
status: active
requirements:
  - id: p1-must-env-var
    level: must
    applicability: universal
    summary: Every flag settable via environment variable (falsey-value parser for booleans).
  - id: p1-must-no-interactive
    level: must
    applicability: universal
    summary: "`--no-interactive` flag gates every prompt library call; when set or stdin is not a TTY, use defaults/stdin or exit with an actionable error."
  - id: p1-must-no-browser
    level: must
    applicability:
      if: CLI authenticates against a remote service
    summary: Headless authentication path (`--no-browser` / OAuth Device Authorization Grant).
  - id: p1-should-tty-detection
    level: should
    applicability: universal
    summary: Auto-detect non-interactive context via TTY detection; suppress prompts when stderr is not a terminal.
  - id: p1-should-defaults-in-help
    level: should
    applicability: universal
    summary: Document default values for prompted inputs in `--help` output.
  - id: p1-may-rich-tui
    level: may
    applicability: universal
    summary: Rich interactive experiences (spinners, progress bars, menus) when TTY is detected and `--no-interactive` is not set.
---

# P1: Non-Interactive by Default

## Definition

Every automation path MUST run without human input. A CLI tool that blocks on an interactive prompt is invisible to an
agent — the agent hangs, the user sees nothing, and the operation times out silently.

**Decision record:** this principle's MUST is worded in terms of observable behavior rather than enumerated APIs.
[`docs/decisions/p1-behavioral-must.md`](../docs/decisions/p1-behavioral-must.md) records the reasoning and names the
verification boundary: automated checks verify behavior under non-TTY stdin; TTY-driving-agent scenarios are covered by
the MUST but are not PTY-probed at the current scale.

## Why Agents Need It

An agent calling a CLI cannot type. When the tool prompts for a confirmation or a credential, the agent's process stalls
until timeout: no tokens recovered, no structured signal that interaction was requested, and no way to distinguish
"waiting for input" from "still processing." Interactive prompts in automation paths are the single most common cause of
agent-tool deadlock.

## Requirements

**MUST:**

- Every flag settable via environment variable. Use a falsey-value parser for booleans so that `TOOL_QUIET=0` and
  `TOOL_QUIET=false` correctly disable the flag rather than being treated as truthy non-empty strings. In Rust / clap:

  ```rust
  #[arg(long, env = "TOOL_QUIET", global = true,
        value_parser = FalseyValueParser::new())]
  quiet: bool,
  ```

- A `--no-interactive` flag gating every prompt library call (`dialoguer`, `inquire`, `read_line`, `TTY::Prompt`,
  `inquirer`, equivalents in other frameworks, or any TUI event loop that takes over the terminal). When the flag is
  set, or when stdin is not a TTY, the tool uses defaults, reads from stdin, or exits with an actionable error. It never
  blocks.
- A headless authentication path if the CLI authenticates. The canonical flag is `--no-browser`, which triggers the
  OAuth 2.0 Device Authorization Grant ([RFC 8628](https://www.rfc-editor.org/rfc/rfc8628)): the CLI prints a URL and a
  code; the user authorizes on another device. Agents cannot open browsers. Non-canonical alternatives (`--device-code`,
  `--remote`, `--headless`) are acceptable but should migrate toward `--no-browser`.

**SHOULD:**

- Auto-detect non-interactive context via TTY detection (`std::io::IsTerminal` in Rust 1.70+, `process.stdin.isTTY` in
  Node, `sys.stdout.isatty()` in Python) and suppress prompts when stderr is not a terminal, even without an explicit
  `--no-interactive` flag.
- Document default values for prompted inputs in `--help` output so agents can pass them explicitly instead of accepting
  whatever default ships.

**MAY:**

- Offer rich interactive experiences — spinners, progress bars, multi-select menus — when a TTY is detected and
  `--no-interactive` is not set, provided the non-interactive path remains fully functional.

## Evidence

- `--no-interactive` flag in the CLI struct with an env-var binding.
- Boolean env vars parsed with a falsey-value parser (not the default string parser).
- TTY guard wrapping every `dialoguer`, `inquire`, or equivalent prompt call.
- `--no-browser` flag present on authenticated CLIs.
- `env = "TOOL_..."` attribute on every flag that takes user input.

## Anti-Patterns

- Bare `dialoguer::Confirm::new().interact()` with no TTY check and no `--no-interactive` override — agents hang
  indefinitely.
- Boolean environment variables parsed as plain strings, so `TOOL_QUIET=false` is truthy because the string is
  non-empty.
- `stdin().read_line()` in a code path reached during normal operation without a TTY check first.
- Hard-coded credentials prompts with no env-var or config-file alternative.
- OAuth flow that unconditionally opens a browser with no headless escape hatch.

Measured by check IDs `p1-non-interactive` (behavioral) and `p1-non-interactive-source` (source). Run `agentnative check
--principle 1 .` against your CLI to see both.

## Pressure test notes

### 2026-04-27 — Show HN launch red-team pass

Adversarial review via `compound-engineering:ce-adversarial-document-reviewer` ahead of the v0.3.0 launch. Findings
recorded verbatim per `principles/AGENTS.md` § "Pressure-test protocol".

- **[edit]** *Internal inconsistency.* "The `--no-interactive` MUST bullet says 'uses defaults, reads from stdin, or
  exits with an actionable error' but the principle's behavioral framing (per decision record) covers TUI session init
  too. The prose bullet only enumerates prompt libraries (`dialoguer`, `inquire`, `read_line`, `TTY::Prompt`,
  `inquirer`), not TUI frameworks (`ratatui`, `bubbletea`) — readers will infer the MUST excludes TUIs, contradicting
  the decision record's explicit 'blocking-interactive surface includes... TUI session initialization.'" Resolved: prose
  bullet's parenthetical now includes "or any TUI event loop that takes over the terminal." Mirrors the behavioral
  framing in [`docs/decisions/p1-behavioral-must.md`](../docs/decisions/p1-behavioral-must.md). No frontmatter change;
  the summary already says "gates every prompt library call" and stays.
- **[wontfix]** *Prior art.* "RFC 8628 citation is correct in name but incomplete in framing. The prose says Device
  Authorization Grant means 'the CLI prints a URL and a code; the user authorizes on another device' — this still
  requires a human on another device, which an unattended agent does not have. An HN commenter will note this is a
  *human-assisted* headless path, not an agent-headless path; true unattended agents need service-account / API-token
  auth (which the principle doesn't mention)." Rationale: P1 scopes "headless" as "no local browser required," not "no
  human anywhere." A human-in-the-loop is acceptable here. Service-account auth is orthogonal and would belong to a
  separate principle if it were added.
