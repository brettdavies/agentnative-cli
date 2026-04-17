---
title: "Spike: Why `p1-non-interactive` passes on `claude` even though the bare CLI launches a TUI"
status: complete
date: 2026-04-17
---

# Spike: Why `p1-non-interactive` passes on `claude` even though the bare CLI launches a TUI

## Question

Running `anc check` against the Claude Code binary at `/home/brett/.local/share/claude/versions/2.1.113` reports `[PASS]
Non-interactive by default (p1-non-interactive)`. But typing `claude` with no arguments in an interactive terminal
launches Claude's TUI. That feels like a straight-up P1 violation — an agent-native linter should not be handing out
passes to tools whose bare-invocation UX is a full-screen interactive session. So: is the pass a bug, a principle gap,
or a real P1-compliant case that only *looks* wrong from the outside?

## TL;DR

- **Reproduced the pass.** `anc check /home/brett/.local/share/claude/versions/2.1.113` prints `[PASS] Non-interactive
  by default (p1-non-interactive)`.
- **The pass is technically correct per the principle as written.** Claude auto-detects non-TTY stdin, suppresses the
  TUI, and exits with an actionable error in 2.2s. That is exactly what P1's SHOULD clause asks for, and P1's MAY clause
  explicitly permits rich interactive UX when a TTY is detected provided the non-interactive path works. Claude is
  therefore the textbook "MAY" case, not a violation.
- **But the check is narrow.** It only probes one thing: *does the binary complete under `stdin = /dev/null` within 5s?*
  It does not verify any of P1's MUST requirements — the existence of a `--no-interactive`-equivalent flag, env-var
  bindings on flags, or a `--no-browser` / headless auth path. A tool that passes this probe has cleared a minimum bar,
  not demonstrated real P1 compliance.
- **Concrete gap**: three of P1's four MUST bullets are unverified by any check today (behavioral OR source). The only
  source-layer check that mentions P1 (`p1-non-interactive-source`) is applicable only to Rust projects with a
  Cargo.toml, greps for four prompt-library names, and does not run against a binary-only target like claude.
- **Recommendation**: the bare-invocation timeout probe is doing what it can. Strengthening P1 coverage is a
  principle-level gap, not a probe-level bug. Split follow-up work into three tractable extensions (a `--help`-scan
  flag-existence check, an env-var-hint scan, and an auth-subcommand headless-flag scan) before trying harder things
  like PTY probing.

## Methodology

All probes run on the real binary, not the shell wrapper:

```text
/home/brett/.local/share/claude/versions/2.1.113   (ELF, 225 MB — Node.js SEA)
```

The shell `claude` on this machine is a function that may route to `caam run` for `-p` invocations; `anc` was pointed at
the underlying ELF to bypass that wrapper. Claude Code CLI version reported in `--help`: documented below.

The `p1-non-interactive` behavioral check is implemented at `src/checks/behavioral/non_interactive.rs`. Its probe is a
direct call to `BinaryRunner::run(&[], &[])` — no arguments, default env (plus `NO_COLOR=1` and a 5-second timeout).
`BinaryRunner::spawn_and_wait` at `src/runner.rs` sets:

- `stdin = Stdio::null()`
- `stdout = Stdio::piped()`
- `stderr = Stdio::piped()`
- `env NO_COLOR=1`

The check's classification logic (excerpted verbatim from `non_interactive.rs`):

```text
RunStatus::Timeout       -> Warn ("binary may be waiting for interactive input")
RunStatus::Ok            -> Pass
RunStatus::Crash{signal} -> Warn ("binary crashed on bare invocation (signal N)")
_                        -> Pass
```

`RunStatus::Ok` is assigned whenever the child process exits with any code (including non-zero) and did not time out or
receive a signal. Exit code is **not consulted** for this check's verdict.

## What claude actually does under this probe

Reproduced the harness directly:

```text
NO_COLOR=1 /home/brett/.local/share/claude/versions/2.1.113 </dev/null >stdout.log 2>stderr.log
exit_code = 1
elapsed   = 2.21s
stdout    = (empty)
stderr    = "Error: Input must be provided either through stdin or as a prompt argument when using --print\n"
```

Read against the check's classification:

- `RunStatus::Timeout` — no (elapsed 2.21s ≪ 5s timeout).
- `RunStatus::Crash` — no (process exited cleanly with code 1, no signal).
- `RunStatus::Ok` — yes (ExitStatus has a `.code()`, no `.signal()`).

→ `Pass`. Exit code 1 is irrelevant to this check.

**Interpretation.** Claude Code detects that stdin is not a TTY, assumes `--print` (non-interactive) mode, and — having
no prompt argument or stdin content — errors out with a crisp, machine-parseable message. This is precisely the
non-interactive behavior P1's SHOULD clause asks for. The TUI is suppressed; no hang; no blocked agent.

## Is claude actually P1-compliant?

Reading `content/principles/p1-non-interactive-by-default.md` against claude's `--help` output:

| Requirement level | Requirement | Claude | Check verifies? |
| ----------------- | ----------- | ------ | --------------- |
| MUST | Every flag settable via env var (with falsey parser on booleans) | Unverified from `--help` — Claude uses commander.js which can wire env vars, but the default `--help` format does not print `env = ...` hints | No check today |
| MUST | A `--no-interactive` flag gating every prompt call | Has `-p / --print` — non-canonical name but the principle explicitly allows equivalents. This is claude's non-interactive gate | No check today |
| MUST | Headless auth path (`--no-browser` or device-code equivalent) on authenticated CLIs | Claude authenticates (subcommands `auth`, `setup-token`). `--help` does not advertise `--no-browser`; the auth flow may or may not have a headless branch — needs empirical probe to confirm | No check today |
| SHOULD | Auto-detect non-TTY and suppress prompts | **Confirmed empirically** — non-TTY stdin → `--print` mode, no TUI, exits | Indirectly: the timeout probe catches a hang, which would be the symptom of failing this |
| MAY | Rich interactive UX when TTY detected AND non-interactive path works | **Confirmed empirically** — bare `claude` in a terminal launches a TUI, bare `claude < /dev/null` does not. Non-interactive path (`-p "prompt"`) works. Textbook MAY | n/a |

Net: **claude clears SHOULD empirically and clears MAY trivially. The three MUST bullets are unverified by any automated
check — not just this one.**

## Why the pass is not wrong

The user's objection was "bare invocation launches a TUI, that can't be P1-compliant." The principle actually permits
exactly that, under one condition: the non-interactive path must be fully functional. The check is structured to detect
the inverse — a tool that *looks* non-interactive but hangs when an agent calls it. That is a different and more
immediately-harmful failure mode:

- Tool with TUI + working non-TTY path (claude): agent runs it with piped stdin, gets error message, proceeds. No
  deadlock.
- Tool with TUI + TUI-even-under-non-TTY (the P1 failure mode the check is looking for): agent calls it, process hangs
  until 5s timeout, agent gets no useful output, user sees nothing. This is the scenario P1 is written to prevent.

Claude falls in the first bucket. The pass is earned, narrowly. It says "this tool will not hang your agent," which is
true. It does not say "this tool satisfies every MUST in P1" — no single check does today.

## What the check cannot see

Even if `p1-non-interactive` were expanded, some things the bare-invocation probe architecturally cannot detect:

1. **TUI-when-stdin-is-a-TTY behavior.** Would require allocating a pseudo-terminal (`forkpty`, `pty-process` etc.) and
   interacting with the child. Expensive, flaky, and platform-dependent. Not in the current probe design.
2. **Prompt mid-stream.** A tool that reads stdin cleanly but then blocks on a confirmation dialog half-way through
   would pass this probe (no hang on bare invocation) and still deadlock an agent mid-run.
3. **Environment-dependent interactivity.** A tool that reads `$TERM`, `$CI`, or `$DEBIAN_FRONTEND` to decide whether to
   prompt. The check could set these to non-interactive values but currently does not.
4. **Subcommands.** The probe only tests the top-level binary with no args. A tool with 20 subcommands, one of which is
   an always-interactive wizard, would still pass.

Most of these are inherent to "probe a compiled binary from outside" — they are not fixable by adjusting the current
check. The source-layer analog (below) is where structural coverage should grow.

## The source-layer check today

There is exactly one source-layer P1 check: `p1-non-interactive-source` in `src/checks/project/non_interactive.rs`. It:

- Is applicable only when `project.path.is_dir() && project.language == Some(Language::Rust) &&
  project.manifest_path.is_some()`.
- Reads `Cargo.toml` and greps for any of `dialoguer`, `inquire`, `rustyline`, `crossterm`.
- Returns `Warn` if any match, `Pass` otherwise.

Against the claude probe, this check is **not applicable** (the input is a binary file, not a Rust directory project).
Against a Python, Go, Ruby, TypeScript, or Node project, this check is **not applicable** either (Rust-only). Against a
Rust project that uses a prompt library not on the list (e.g., `requestty`, `demand`, `promkit`, `cliclack`), this check
would return Pass.

This is a narrow safety net, not a P1 compliance test. The principle doc's footer claim that P1 is "Measured by check
IDs `p1-non-interactive` (behavioral) and `p1-non-interactive-source` (source)" is technically accurate but under-sells
how limited both checks are.

## Recommendations

These are scoped as a standalone follow-up plan; not in scope for this spike. Roughly in increasing order of cost:

1. **Flag-existence scan in `--help` output** (new behavioral check, or extend `p1-non-interactive`). Look for at least
   one of `--no-interactive`, `-p`, `--print`, `--no-input`, `--batch`, `--headless`. Downgrade today's unconditional
   Pass to Warn when the probe succeeds but no such flag is advertised. Does not help claude (it has `-p`) but would
   catch tools that pass the timeout probe trivially without offering any documented non-interactive surface.
2. **Env-var hint scan in `--help` output** (new behavioral check, `p1-env-var-bindings`). Parse `--help` for per-flag
   `env = FOO` or `[env: FOO]` hints (clap/cobra/commander all emit these with a flag). Warn on flags with no env
   binding. Claude's `--help` does not advertise these even if they exist, so the scan would Warn on claude — accurate,
   given the MUST requirement.
3. **Auth-subcommand headless-flag scan** (new behavioral check, `p1-no-browser`). If `<binary> --help` lists an auth
   subcommand, probe `<binary> auth --help` for a `--no-browser`, `--device-code`, `--remote`, or `--headless` flag.
   Warn if none found. Complex but catches a specific MUST.
4. **Extend `p1-non-interactive-source`** to other languages once Unit 5 of the multi-language plan ships. Python:
   `click.confirm`, `typer.prompt`, `rich.prompt`, `questionary`, `inquirer`, bare `input()`. Node: `inquirer`,
   `prompts`, `enquirer`, `@clack/prompts`, `readline.question`. Go: `survey`, `promptui`, `tview`, `bubbletea`. Ruby:
   `TTY::Prompt`, `thor` (prompt helpers), `highline`. This is the largest payoff per unit of complexity.
5. **PTY probe as a fifth RunStatus path.** Deferred — high complexity, unclear signal-to-noise. Revisit only if
   categories 1–4 leave a meaningful gap.

## Proposed plan-level framing for follow-up

If a follow-up ships, frame it as "P1 check depth, not breadth." The MUST list in the principle has four bullets;
today's checks cover zero of them structurally (the behavioral probe verifies an implication of the SHOULD, not a MUST).
Four additive checks (one per MUST bullet where feasible) would close that gap without disturbing the existing probe.

Do NOT strengthen `p1-non-interactive` itself by downgrading it to Warn when the bare-invocation probe succeeds but MUST
items are unverified. That would conflate two distinct signals ("this tool will hang your agent" vs "this tool lacks an
env-var binding on its --quiet flag"). Keep them as separate checks so the scorecard remains legible.

## Sources

- `src/checks/behavioral/non_interactive.rs` — current behavioral probe; the source of the Pass verdict under
  investigation.
- `src/runner.rs` — `BinaryRunner::spawn_and_wait`, which sets stdin to `/dev/null` and enforces the 5-second timeout.
- `src/checks/project/non_interactive.rs` — the Rust-only source-layer analog.
- `/home/brett/dev/agentnative-site/content/principles/p1-non-interactive-by-default.md` — the principle definition.
- Empirical probe: direct invocation of the ELF binary at `/home/brett/.local/share/claude/versions/2.1.113` with `stdin
  </dev/null`, matching the harness exactly.
- Claude Code `--help` output (version as of 2026-04-17): lists `-p / --print`, `--output-format`, `auth` subcommand,
  `setup-token` subcommand, and several dozen other flags.
