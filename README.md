# agentnative

The agent-native CLI linter. Checks whether your CLI follows the 7 agent-readiness principles.

## Install

The crate is published as `agentnative`. The binary is called `anc`.

```bash
# Homebrew (installs anc)
brew install brettdavies/tap/agentnative

# From crates.io
cargo install agentnative

# Pre-built binary via cargo-binstall
cargo binstall agentnative

# Pre-built binaries from GitHub Releases
# https://github.com/brettdavies/agentnative-cli/releases
```

## Quick Start

```bash
# Check the current project (`check` is the default subcommand)
anc .

# Check a specific binary
anc ./target/release/mycli

# Resolve a command on PATH and run behavioral checks against it
anc --command ripgrep

# JSON output for CI
anc . --output json

# Filter by principle
anc . --principle 3

# Quiet mode (warnings and failures only)
anc . -q
```

## The 7 Principles

agentnative checks your CLI against seven agent-readiness principles:

| # | Principle | What It Means |
| - | --------- | ------------- |
| P1 | Non-Interactive by Default | No prompts, no browser popups, stdin from `/dev/null` works |
| P2 | Structured Output | `--output json` exists and produces valid JSON |
| P3 | Progressive Help | `--help` has examples, `--version` works |
| P4 | Actionable Errors | Structured error types, named exit codes, no `.unwrap()` |
| P5 | Safe Retries | `--dry-run` for write operations |
| P6 | Composable Structure | SIGPIPE handled, NO_COLOR respected, shell completions, AGENTS.md |
| P7 | Bounded Responses | `--quiet` flag, no unbounded list output, clamped pagination |

## Example Output

```text
P1 — Non-Interactive by Default
  [PASS] Non-interactive by default (p1-non-interactive)
  [PASS] No interactive prompt dependencies (p1-non-interactive-source)

P3 — Progressive Help
  [PASS] Help flag produces useful output (p3-help)
  [PASS] Version flag works (p3-version)

P4 — Actionable Errors
  [PASS] Rejects invalid arguments (p4-bad-args)
  [PASS] No process::exit outside main (p4-process-exit)

P6 — Composable Structure
  [PASS] Handles SIGPIPE gracefully (p6-sigpipe)
  [PASS] Respects NO_COLOR (p6-no-color)
  [PASS] Shell completions support (p6-completions)

Code Quality
  [PASS] No .unwrap() in source (code-unwrap)

30 checks: 26 pass, 2 warn, 0 fail, 2 skip, 0 error
```

## Three Check Layers

agentnative uses three layers to analyze your CLI:

- **Behavioral** — runs the compiled binary, checks `--help`, `--version`, `--output json`, SIGPIPE, NO_COLOR, exit
  codes. Language-agnostic.
- **Source** — ast-grep pattern matching on source code. Detects `.unwrap()`, missing error types, naked `println!`, and
  more. Currently supports Rust.
- **Project** — inspects files and manifests. Checks for AGENTS.md, recommended dependencies, dedicated error/output
  modules.

## CLI Reference

When the first non-flag argument is not a recognized subcommand, `check` is inserted automatically. `anc .`,
`anc -q .`, and `anc --command ripgrep` all resolve to `anc check …`. Bare `anc` (no arguments) still prints help and
exits 2 — this is deliberate fork-bomb prevention when agentnative dogfoods itself.

```text
Usage: anc check [OPTIONS] [PATH]

Arguments:
  [PATH]  Path to project directory or binary [default: .]

Options:
      --command <NAME>         Resolve a command from PATH and run behavioral checks against it
      --binary                 Run only behavioral checks (skip source analysis)
      --source                 Run only source checks (skip behavioral)
      --principle <PRINCIPLE>  Filter checks by principle number (1-7)
      --output <OUTPUT>        Output format [default: text] [possible values: text, json]
  -q, --quiet                  Suppress non-essential output
      --include-tests          Include test code in source analysis
  -h, --help                   Print help
```

`--command` and `[PATH]` are mutually exclusive — pick one. `--command` runs behavioral checks only; source and project
checks are skipped because there is no source tree to analyze.

### Exit Codes

| Code | Meaning |
| ---- | ------- |
| 0 | All checks passed |
| 1 | Warnings present (no failures) |
| 2 | Failures, errors, or usage errors |

Exit 2 covers both check failures (a real `[FAIL]` or `[ERROR]` result) and usage errors (bare `anc`, unknown flag,
mutually exclusive flags). Agents distinguishing the two should parse `stderr` (usage errors print `Usage:`) or call
`anc --help` first to validate the invocation shape.

### Shell Completions

```bash
# Bash
anc completions bash > ~/.local/share/bash-completion/completions/anc

# Zsh (writes to the first directory on your fpath)
anc completions zsh > "${fpath[1]}/_anc"

# Fish
anc completions fish > ~/.config/fish/completions/anc.fish

# PowerShell
anc completions powershell > anc.ps1

# Elvish
anc completions elvish > anc.elv
```

Pre-generated scripts are also available in `completions/`.

## JSON Output

```bash
anc check . --output json
```

Produces a scorecard with results and summary:

```json
{
  "results": [
    {
      "id": "p3-help",
      "label": "Help flag produces useful output",
      "group": "P3",
      "layer": "behavioral",
      "status": "pass",
      "evidence": null
    }
  ],
  "summary": {
    "total": 30,
    "pass": 20,
    "warn": 8,
    "fail": 0,
    "skip": 2,
    "error": 0
  }
}
```

## Contributing

```bash
git clone https://github.com/brettdavies/agentnative-cli
cd agentnative
cargo test
cargo run -- check .
```

## License

MIT OR Apache-2.0
