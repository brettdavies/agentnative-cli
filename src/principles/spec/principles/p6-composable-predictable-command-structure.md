---
id: p6
title: Composable and Predictable Command Structure
last-revised: 2026-04-22
status: active
requirements:
  - id: p6-must-sigpipe
    level: must
    applicability: universal
    summary: SIGPIPE is handled so piping to `head`/`tail` does not crash the process (Rust example below; Python/Go/Node have language-specific equivalents).
  - id: p6-must-no-color
    level: must
    applicability: universal
    summary: TTY detection plus support for `NO_COLOR` and `TERM=dumb` — color codes suppressed when stdout/stderr is not a terminal.
  - id: p6-must-completions
    level: must
    applicability: universal
    summary: Shell completions available via a `completions` subcommand (Tier 1 meta-command — needs no config/auth/network).
  - id: p6-must-timeout-network
    level: must
    applicability:
      if: CLI makes network calls
    summary: Network CLIs ship a `--timeout` flag with a sensible default (e.g., 30 seconds).
  - id: p6-must-no-pager
    level: must
    applicability:
      if: CLI invokes a pager for output
    summary: If the CLI uses a pager (`less`, `more`, `$PAGER`), it supports `--no-pager` or respects `PAGER=""`.
  - id: p6-must-global-flags
    level: must
    applicability:
      if: CLI uses subcommands
    summary: Agentic flags (`--output`, `--quiet`, `--no-interactive`, `--timeout`) propagate to every subcommand (e.g., `global = true` in clap).
  - id: p6-should-stdin-input
    level: should
    applicability:
      if: CLI has commands that accept input data
    summary: Commands that accept input read from stdin when no file argument is provided.
  - id: p6-should-consistent-naming
    level: should
    applicability:
      if: CLI uses subcommands
    summary: Subcommand naming follows a consistent `noun verb` or `verb noun` convention throughout the tool.
  - id: p6-should-tier-gating
    level: should
    applicability: universal
    summary: "Three-tier dependency gating: Tier 1 (meta) needs nothing, Tier 2 (local) needs config, Tier 3 (network) needs config + auth."
  - id: p6-should-subcommand-operations
    level: should
    applicability:
      if: CLI performs multiple distinct operations
    summary: Operations are modeled as subcommands, not flags (`tool search "q"`, not `tool --search "q"`).
  - id: p6-may-color-flag
    level: may
    applicability: universal
    summary: "`--color auto|always|never` flag for explicit color control beyond TTY auto-detection."
---

# P6: Composable and Predictable Command Structure

## Definition

CLI tools MUST integrate cleanly with pipes, scripts, and other tools. That means handling SIGPIPE, detecting TTY for
color and formatting decisions, supporting stdin for piped input, and maintaining a consistent, predictable subcommand
structure.

## Why Agents Need It

Agents compose CLI tools into pipelines:

```bash
tool list --output json | jaq '.[] | .id' | xargs tool get
```

Every link in that chain has to behave predictably. A tool that panics on SIGPIPE when piped to `head` breaks the
pipeline. A tool that emits ANSI color codes into a pipe pollutes downstream JSON parsing. A tool with inconsistent
subcommand naming forces the agent to memorize exceptions rather than apply patterns. Composability is what makes a CLI
tool a building block rather than a dead end.

## Requirements

**MUST:**

- SIGPIPE is handled so that piping to `head`, `tail`, or any tool that closes the pipe early does not crash the
  process. In Rust, restore the default SIGPIPE handler as the first executable statement in `main()`:

  ```rust
  unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL); }
  ```

  Equivalents in other languages: Python — restore the default `SIGPIPE` handler at startup
  (`signal.signal(signal.SIGPIPE, signal.SIG_DFL)`); Go — the runtime's default handling already exits cleanly on
  EPIPE writes; Node.js — handle `EPIPE` on `process.stdout`.

- TTY detection, plus support for `NO_COLOR` and `TERM=dumb`. When stdout or stderr is not a terminal, color codes are
  suppressed automatically.
- Shell completions available via a `completions` subcommand (clap_complete in Rust; equivalents elsewhere). This is a
  Tier 1 meta-command — it works without config, auth, or network.
- Network CLIs ship a `--timeout` flag with a sensible default (30 seconds). Agents operating under their own time
  budgets need to fail fast rather than block on a slow upstream.
- If the CLI uses a pager (`less`, `more`, `$PAGER`), it supports `--no-pager` or respects `PAGER=""`. Pagers block
  headless execution indefinitely.
- When the CLI uses subcommands, agentic flags (`--output`, `--quiet`, `--no-interactive`, `--timeout`) propagate to
  every subcommand automatically (e.g., `global = true` in clap).

**SHOULD:**

- Commands that accept input read from stdin when no file argument is provided. Pipeline composition depends on it.
- Subcommand naming follows a consistent `noun verb` or `verb noun` convention throughout the tool. Mixing patterns
  (e.g., `list-users` alongside `user show`) forces agents to learn exceptions.
- A three-tier dependency gating pattern: Tier 1 (meta-commands like `completions`, `version`) needs nothing; Tier 2
  (local commands) needs config; Tier 3 (network commands) needs config + auth. `completions` and `version` always work,
  even in broken environments.
- Operations are modeled as subcommands, not flags. `tool search "query"` is correct; `tool --search "query"` is wrong.
  Flags modify behavior (`--quiet`, `--output json`); subcommands select operations.

**MAY:**

- A `--color auto|always|never` flag for explicit color control beyond TTY auto-detection.

## Evidence

- `libc::signal(libc::SIGPIPE, libc::SIG_DFL)` (or the equivalent in the target language) as the first statement of
  `main()`.
- `IsTerminal` trait usage (`std::io::IsTerminal` or the `is-terminal` crate).
- `NO_COLOR` and `TERM=dumb` checks.
- `clap_complete` in `Cargo.toml`.
- A `completions` subcommand in the CLI enum.
- Tiered match arms in `main()` separating meta-commands from config-dependent commands.

## Anti-Patterns

- Missing SIGPIPE handler — `cargo run -- list | head` panics with "broken pipe".
- Hard-coded ANSI escape codes without TTY detection.
- Color output in JSON mode — ANSI codes inside JSON string values break downstream parsing.
- A `completions` command that requires auth or config to run.
- No stdin support on commands where piped input is a natural use case.

Measured by check IDs `p6-sigpipe`, `p6-no-color`, `p6-completions`, `p6-timeout`, `p6-agents-md`. Run `agentnative
check --principle 6 .` against your CLI to see each.

## Pressure test notes

### 2026-04-27 — Show HN launch red-team pass

Adversarial review via `compound-engineering:ce-adversarial-document-reviewer` ahead of the v0.3.0 launch. Findings
recorded verbatim per `principles/AGENTS.md` § "Pressure-test protocol".

- **[edit]** *Prior art / vague agent-native.* "The SIGPIPE MUST prescribes `unsafe { libc::signal(libc::SIGPIPE,
  libc::SIG_DFL); }` as the first `main()` statement — that is a Rust-specific remedy. Python raises `BrokenPipeError`
  by default (different fix), Go's runtime already exits cleanly on EPIPE writes (no fix needed), Node.js needs
  `process.stdout.on('error')`. The MUST as written is correct in spirit but the prescription leaks Rust into a
  universal-applicability rule." Resolved: prose bullet now leads with the language-neutral MUST ("SIGPIPE is handled so
  that piping to `head`, `tail`, or any tool that closes the pipe early does not crash the process"); the Rust snippet
  stays as the canonical example; per-language one-liners cover Python, Go, and Node. Frontmatter summary updated to
  match.
- **[edit]** *Must-vs-should.* "The `global = true` MUST is a clap-API artifact — the behavioral requirement is 'agentic
  flags propagate to every subcommand,' which is what the prose actually says. The frontmatter summary baking `global =
  true` into a universal contract overfits to one library." Resolved: frontmatter summary and prose bullet now lead with
  the behavioral requirement ("propagate to every subcommand"), with `global = true` cited as the clap-specific example.
  Behavior unchanged; language-neutrality restored.
