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

When the first non-flag argument is not a recognized subcommand, `check` is inserted automatically. `anc .`, `anc -q .`,
and `anc --command ripgrep` all resolve to `anc check …`. Bare `anc` (no arguments) still prints help and exits 2 — this
is deliberate fork-bomb prevention when agentnative dogfoods itself.

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

| Code | Meaning                           |
| ---- | --------------------------------- |
| 0    | All checks passed                 |
| 1    | Warnings present (no failures)    |
| 2    | Failures, errors, or usage errors |

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

Produces a self-describing scoring run record (`schema_version: "0.4"`) with results, summary, coverage against the 7
principles, plus contextual metadata identifying which tool was scored, by which `anc` build, on which platform, and
how:

```json
{
  "schema_version": "0.4",
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
    "pass": 26,
    "warn": 2,
    "fail": 0,
    "skip": 2,
    "error": 0
  },
  "coverage_summary": {
    "must": { "total": 23, "verified": 17 },
    "should": { "total": 16, "verified": 2 },
    "may":   { "total": 7,  "verified": 0 }
  },
  "audience": "agent-optimized",
  "audit_profile": null,
  "spec_version": "0.3.0",
  "tool":   { "name": "ripgrep", "binary": "rg",  "version": "ripgrep 15.1.0" },
  "anc":    { "version": "0.2.0", "commit": "abc1234" },
  "run":    {
    "invocation": "anc check --command rg --output json",
    "started_at": "2026-04-29T16:00:00Z",
    "duration_ms": 412,
    "platform":   { "os": "linux", "arch": "x86_64" }
  },
  "target": { "kind": "command", "path": null, "command": "rg" }
}
```

- `coverage_summary` — how many MUSTs/SHOULDs/MAYs the checks that ran actually verified, against the spec registry's
  totals. See `docs/coverage-matrix.md` for the per-requirement breakdown. Checks suppressed by `--audit-profile` do
  **not** count toward `verified` — suppression means the requirement was not verified, even if the check is skipped
  rather than run.
- `audience` — derived classification from 4 signal behavioral checks (`p1-non-interactive`, `p2-json-output`,
  `p7-quiet`, `p6-no-color-behavioral`). Emits `agent-optimized` (0-1 Warns), `mixed` (2 Warns), or `human-primary` (3-4
  Warns). Returns `null` when any signal check failed to run (source-only mode, missing runner, or `--audit-profile`
  suppression). Informational only — never gates totals or exit codes. Values serialize as kebab-case to match
  `audit_profile`'s format within the same JSON document.
- `audience_reason` — present only when `audience` is `null`. Values: `suppressed` (at least one signal check was masked
  by `--audit-profile`) or `insufficient_signal` (signal check never produced, e.g. source-only run). Additive to schema
  `0.2`; older consumers feature-detect.
- `audit_profile` — echoes the applied `--audit-profile <category>` flag value (`human-tui`, `file-traversal`,
  `posix-utility`, or `diagnostic-only`). `null` when no profile is set. See `coverage/matrix.json` under
  `audit_profiles` for the committed per-category mapping of which check IDs each profile suppresses.
- `tool` — identifies what was scored. `name` is always present (deterministic from path or command). `binary` is the
  executable basename when one is located; `null` for project-mode runs without a built artifact. `version` is
  best-effort: project-mode prefers the manifest version (`Cargo.toml`/`pyproject.toml`), command/binary mode probes
  `<bin> --version` then `-V`. `null` when probing failed or was declined by the self-spawn guard. The site's
  `registry.yaml` `version_extract` snippets remain authoritative for tools whose self-report is unreliable. Schema
  `0.4` addition.
- `anc` — identifies the `anc` build that produced the scorecard. `version` is the crate version at compile time.
  `commit` is the short Git SHA at compile time, or `null` for builds outside a Git checkout (e.g., `cargo install` from
  crates.io). Informational, not a signed provenance signal — pair with a Sigstore-signed release artifact if provenance
  is required. Schema `0.4` addition.
- `run` — run-level facts. `invocation` is the user's argv joined with shell-safe quoting, captured **before**
  default-subcommand injection so it reflects what the user typed (`anc .`, not `anc check .`). `started_at` is RFC 3339
  UTC. `duration_ms` is wall-clock milliseconds. `platform.os` / `platform.arch` come from `std::env::consts`. Schema
  `0.4` addition.
- `target` — what `anc` was pointed at. `kind` is `"project"` (directory), `"binary"` (executable file), or `"command"`
  (PATH-resolved name from `--command`). `path` carries the resolved filesystem path for project / binary modes;
  `command` carries the user-supplied name for command mode. The unused field is always `null`, never missing — consumer
  code can access both unconditionally. Schema `0.4` addition.

> Publishing a scorecard? `run.invocation` and `target.path` may carry usernames or absolute paths from the machine that
> produced the scorecard. Review before publishing — `anc` does not silently redact, since that would surprise users
> debugging their own runs.

## Contributing

```bash
git clone https://github.com/brettdavies/agentnative-cli
cd agentnative
cargo test
cargo run -- check .
```

### Reporting issues

Open an issue at
[github.com/brettdavies/agentnative-cli/issues/new/choose](https://github.com/brettdavies/agentnative-cli/issues/new/choose).
Seven structured templates cover the common cases:

| Template        | Use it when                                                                                                                         |
| --------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| False positive  | A check flagged your CLI but you believe your CLI is doing the right thing.                                                         |
| Scoring bug     | Results don't match what the check should be doing (wrong status, miscategorized group/layer, evidence pointing at the wrong line). |
| Feature request | Missing capability, flag, or output format in the checker itself.                                                                   |
| Grade a CLI     | Nominate a CLI for an `anc`-graded readiness review.                                                                                |
| Pressure test   | Challenge a principle or check definition — "this check is too strict / too loose / wrong on this class of CLI."                    |
| Spec question   | Ambiguity or gap in the 7-principle spec (not the checker).                                                                         |
| Something else  | Chooser for anything outside the templates above.                                                                                   |

Filing on the right template front-loads the triage context we need and keeps issues out of a single-bucket backlog.

## License

MIT OR Apache-2.0
