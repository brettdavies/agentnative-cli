# agentnative

[![agent-native](https://anc.dev/badge/anc.svg)](https://anc.dev/score/anc)
[![Crates.io](https://img.shields.io/crates/v/agentnative.svg)](https://crates.io/crates/agentnative)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT_OR_Apache--2.0-blue.svg)](#license)

The agent-native CLI linter. Audits whether your CLI follows the 8 agent-readiness principles.

`anc` dogfoods the spec it enforces. The badge above is its own live score.

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

## Install the skill

`anc` ships a companion skill bundle (`agentnative-skill`) that teaches AI coding agents how to operate the linter and
where to apply the principles. Install it with one command per host:

```bash
anc skill install claude_code   # ~/.claude/skills/agent-native-cli
anc skill install codex         # ~/.codex/skills/agent-native-cli
anc skill install cursor        # ~/.cursor/skills/agent-native-cli
anc skill install factory       # ~/.factory/skills/agent-native-cli   (Factory Droid)
anc skill install kiro          # ~/.kiro/skills/agent-native-cli      (Kiro)
anc skill install opencode      # ~/.config/opencode/skills/agent-native-cli
```

Inspect the resolved command before running it:

```bash
anc skill install --dry-run claude_code
# git clone --depth 1 https://github.com/brettdavies/agentnative-skill.git /home/you/.claude/skills/agent-native-cli
```

JSON output (modes `dry-run` and `install`, success and error) shares one envelope shape. Agents parse the same fields
on every outcome:

```bash
anc skill install --dry-run claude_code --output json
```

If the site adds a host before this `anc` release knows about it, fall back to a manual `git clone`:

```bash
git clone --depth 1 https://github.com/brettdavies/agentnative-skill.git <host-skills-dir>/agent-native-cli
```

The host map is hardcoded in this binary; new hosts ship via patch release after the site updates `skill.json`.

## Quick Start

```bash
# Audit the current project (`audit` is the default subcommand)
anc .

# Audit a specific binary
anc ./target/release/mycli

# Resolve a command on PATH and run behavioral audits against it
anc --command ripgrep

# Run only behavioral audits (skip source analysis)
anc . --binary

# Run only source audits (skip the compiled binary)
anc . --source

# JSON output for CI
anc . --output json

# Print the scorecard JSON Schema (draft 2020-12)
anc emit schema

# Filter by principle
anc . --principle 3

# Quiet mode (warnings and failures only)
anc . -q
```

## The 8 Principles

agentnative audits your CLI against eight agent-readiness principles:

| # | Principle | What It Means |
| - | --------- | ------------- |
| P1 | Non-Interactive by Default | No prompts, no browser popups, stdin from `/dev/null` works |
| P2 | Structured Output | `--output json` exists, produces valid JSON, and the shape is discoverable via `schema` subcommand or `--schema` flag |
| P3 | Progressive Help | `--help` has examples, `--version` works |
| P4 | Actionable Errors | Structured error types, named exit codes, no `.unwrap()` |
| P5 | Safe Retries | `--dry-run` for write operations |
| P6 | Composable Structure | SIGPIPE handled, NO_COLOR respected, shell completions, `AGENTS.md`, SIGTERM cleanup |
| P7 | Bounded Responses | `--quiet` flag, no unbounded list output, clamped pagination |
| P8 | Discoverable Skill Bundle | Top-level `AGENTS.md` / `SKILL.md` with YAML frontmatter, `tool skill install [<host>]` for agent runtimes |

## Example Output

```text
P1 — Non-Interactive by Default
  [PASS] Non-interactive by default (p1-non-interactive)
  [PASS] Flags advertise env-var bindings in --help (p1-env-hints)
  [PASS] Secret-bearing flags expose stdin or *-file companion (p1-secret-non-leaky-path)
  [PASS] TTY detection for color output (p1-tty-detection-source)
  [PASS] No interactive prompt dependencies (p1-non-interactive-source)

P2 — Structured Output
  [PASS] Structured-output CLI exposes its schema at runtime (p2-schema-print)
  [PASS] --json / --jsonl short aliases for --output (p2-json-aliases)
  [PASS] Output schema exported to a stable file path (p2-schema-file)

P6 — Composable Structure
  [PASS] Handles SIGPIPE gracefully (p6-sigpipe)
  [PASS] Subcommand verbs follow community-standard names (p6-standard-names)
  [PASS] Long-running CLI handles SIGTERM (p6-sigterm)
  [PASS] Shell completions support (p6-completions)

P8 — Discoverable Skill Bundles
  [PASS] Skill bundle has install path (`tool skill install [<host>]`) (p8-bundle-install)
  [PASS] Top-level AGENTS.md / SKILL.md bundle present (p8-bundle-exists)

Code Quality
  [PASS] No .unwrap() in source (code-unwrap)

69 audits: 59 pass, 0 warn, 0 fail, 0 opt_out, 0 n_a, 10 skip, 0 error

🏆 Score: 100% — your tool qualifies for the agent-native badge.
   Embed in your README:
     [![agent-native](https://anc.dev/badge/anc.svg)](https://anc.dev/score/anc)
   Convention: https://anc.dev/badge
```

The badge hint appears in `text` output when a tool scores at or above the 70% eligibility floor. Below the floor, `anc`
prints nothing badge-related. The convention is to surface the embed only when earned.

## Three Audit Layers

agentnative uses three layers to analyze your CLI:

- **Behavioral**: runs the compiled binary, checks `--help`, `--version`, `--output json`, SIGPIPE, NO_COLOR, SIGTERM,
  exit codes. Language-agnostic. Isolate with `anc . --binary`.
- **Source**: ast-grep pattern matching on source code. Detects `.unwrap()`, missing error types, naked `println!`,
  closed-set rejection, and more. Supports Rust and Python. Isolate with `anc . --source`.
- **Project**: inspects files and manifests. Checks for `AGENTS.md` / `SKILL.md` bundle, recommended dependencies,
  dedicated error/output modules, output-schema file at the repo root. Runs alongside the other layers; no isolation
  flag.

`--binary` and `--source` are useful when one layer regresses and you want a focused gate (CI step for source quality,
release-gate against the compiled artifact). Without either flag, all three layers run together.

## Scoring

The public score reflects **shipped-binary behavior only**: only behavioral-layer audits enter the formula. Source- and
project-layer audits still run and still appear in the scorecard, but they do not move the score: what a tool's source
looks like does not change how an agent experiences the installed binary.

Every audit result lands in one of seven statuses. The score is a credit-weighted ratio over the behavioral rows that
verified something one way or the other (the *denominator set*):

| Status    | Credit | In denominator | Meaning                                          |
| --------- | -----: | -------------- | ------------------------------------------------ |
| `pass`    |    1.0 | yes            | Behavior present and correct.                    |
| `warn`    |    0.5 | yes            | Behavior present, partially correct.             |
| `fail`    |    0.0 | yes            | Behavior expected, absent or broken.             |
| `opt_out` |    0.0 | yes            | Behavior deliberately declined (counts against). |
| `n_a`     |    n/a | no             | Inapplicable: a conditional antecedent is unmet. |
| `skip`    |    n/a | no             | Unmeasurable: the probe could not determine.     |
| `error`   |    n/a | no             | The probe itself raised an exception.            |

With per-tier weights `w` (currently flat, with MUST = SHOULD = MAY = 1), `score_pct = round(100 × Σ w·credit / Σ w)`
over the denominator set. Badge eligibility floor: **70%**. The floor is deliberately low so the badge spreads the
standard; exclusivity is carried by four cohort bands the site renders: Exemplary (≥ 85), Strong (80–84), Solid (75–79),
Qualified (70–74). The eligibility floor is not a high gate. The formula, weights, floor, and bands are defined in the
spec's `principles/scoring.md` and held stable for at least six months from publication.

### Tier mapping

Each spec requirement is tagged MUST / SHOULD / MAY. A missing requirement maps to a different result status depending
on tier:

| Tier   | On miss | Example                  |
| ------ | ------- | ------------------------ |
| MUST   | `fail`  | `p1-must-no-interactive` |
| SHOULD | `warn`  | `p2-should-schema-file`  |
| MAY    | `warn`  | `p8-may-install-all`     |

### Dogfood

`anc` runs the same scoring on itself. Because the public score is behavioral-only, a source-only run has no rows to
score. It reports `0%` and ineligible by design, since there is no shipped-binary behavior to measure:

| Mode                   | Audits | Pass | Skip | Behavioral rows | Score | Eligible |
| ---------------------- | -----: | ---: | ---: | --------------: | ----: | -------- |
| `anc audit . --binary` |     43 |   34 |    9 |              43 |  100% | yes      |
| `anc audit . --source` |     26 |   25 |    1 |               0 |    0% | no       |
| `anc audit .` (full)   |     69 |   59 |   10 |              43 |  100% | yes      |

The score is identical in `--binary` and full mode: source and project audits add scorecard rows but never change the
number. The nine behavioral skips are probe limitations (`skip`), excluded from the denominator, so they neither help
nor hurt the score.

## CLI Reference

When the first non-flag argument is not a recognized subcommand, `audit` is inserted automatically. `anc .`, `anc -q .`,
and `anc --command ripgrep` all resolve to `anc audit …`. Bare `anc` (no arguments) still prints help and exits 2: a
deliberate fork-bomb guard for when agentnative dogfoods itself.

```text
Usage: anc audit [OPTIONS] [PATH]

Arguments:
  [PATH]  Path to project directory or binary [default: .]

Options:
      --command <NAME>           Resolve a command from PATH and run behavioral audits against it
      --binary                   Run only behavioral audits (skip source analysis)
      --source                   Run only source audits (skip behavioral)
      --principle <PRINCIPLE>    Filter audits by principle number (1-8)
      --output <OUTPUT>          Output format [default: text] [possible values: text, json]
  -q, --quiet                    Suppress non-essential output [env: AGENTNATIVE_QUIET=]
      --include-tests            Include test code in source analysis
      --audit-profile <CATEGORY> Exemption category for the target [possible values:
                                 human-tui, file-traversal, posix-utility, diagnostic-only]
  -h, --help                     Print help
```

`--command` and `[PATH]` are mutually exclusive; pick one. `--command` runs behavioral audits only. Source and project
audits are skipped because there is no source tree to analyze.

`--audit-profile` suppresses audits that legitimately do not apply to a class of tool. Profiles: `human-tui` for TUI
apps like `lazygit` whose contract IS the TTY, `posix-utility` for stdin-primary tools like `cat`/`sed`/`awk`,
`diagnostic-only` for read-only tools like `nvidia-smi`, `file-traversal` reserved for upcoming subcommand-structure
relaxations on `fd`/`find`-class tools. Suppressed audits emit `Skip` with structured evidence. The full per-category
mapping lives in `coverage/matrix.json` under `audit_profiles[]`. Agents should read that file rather than scrape
`--help`.

### Exit Codes

| Code | Meaning                           |
| ---- | --------------------------------- |
| 0    | All audits passed                 |
| 1    | Warnings present (no failures)    |
| 2    | Failures, errors, or usage errors |

Exit 2 covers both audit failures (a real `[FAIL]` or `[ERROR]` result) and usage errors (bare `anc`, unknown flag,
mutually exclusive flags). Agents distinguishing the two should parse `stderr` (usage errors print `Usage:`) or call
`anc --help` first to confirm the invocation shape.

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
anc audit . --output json
```

Produces a self-describing scoring run record (`schema_version: "0.5"`) with results, summary, coverage against the
eight principles, plus contextual metadata identifying which tool was scored, by which `anc` build, on which platform,
and how. Each scorecard conforms to the JSON Schema emitted by `anc emit schema` (also committed at
`schema/scorecard.schema.json`):

```json
{
  "schema_version": "0.5",
  "results": [
    {
      "id": "p3-help",
      "label": "Help flag produces useful output",
      "group": "P3",
      "layer": "behavioral",
      "status": "pass",
      "evidence": null,
      "confidence": "high"
    }
  ],
  "summary": {
    "total": 33,
    "pass": 28,
    "warn": 1,
    "fail": 0,
    "skip": 4,
    "error": 0
  },
  "coverage_summary": {
    "must":   { "total": 27, "verified": 21 },
    "should": { "total": 20, "verified": 6  },
    "may":    { "total": 10, "verified": 3  }
  },
  "audience": "agent-optimized",
  "audit_profile": null,
  "spec_version": "0.4.0",
  "tool":   { "name": "ripgrep", "binary": "rg", "version": "ripgrep 15.1.0" },
  "anc":    { "version": "0.4.0" },
  "run":    {
    "invocation": "anc audit --command rg --output json",
    "started_at": "2026-04-29T16:00:00Z",
    "duration_ms": 412,
    "platform":   { "os": "linux", "arch": "x86_64" }
  },
  "target": { "kind": "command", "path": null, "command": "rg" },
  "badge":  {
    "eligible": true,
    "score_pct": 97,
    "embed_markdown": "[![agent-native](https://anc.dev/badge/ripgrep.svg)](https://anc.dev/score/ripgrep)",
    "scorecard_url":  "https://anc.dev/score/ripgrep",
    "badge_url":      "https://anc.dev/badge/ripgrep.svg",
    "convention_url": "https://anc.dev/badge"
  }
}
```

- `coverage_summary`: how many MUSTs/SHOULDs/MAYs the audits that ran actually verified, against the spec registry's
  totals. See `docs/coverage-matrix.md` for the per-requirement breakdown. Audits suppressed by `--audit-profile` do
  **not** count toward `verified`. Suppression means the requirement was not verified, even when the audit shows as Skip
  rather than running.
- `audience`: derived classification from 4 signal behavioral audits (`p1-non-interactive`, `p2-json-output`,
  `p7-quiet`, `p6-no-color-behavioral`). Emits `agent-optimized` (0-1 Warns), `mixed` (2 Warns), or `human-primary` (3-4
  Warns). Returns `null` when any signal audit failed to run (source-only mode, missing runner, or `--audit-profile`
  suppression). Informational only; never gates totals or exit codes. Values serialize as kebab-case to match
  `audit_profile`'s format within the same JSON document.
- `audience_reason`: present only when `audience` is `null`. Values: `suppressed` (at least one signal audit was masked
  by `--audit-profile`) or `insufficient_signal` (signal audit never produced, e.g. source-only run). Additive to schema
  `0.2`; older consumers feature-detect.
- `audit_profile`: echoes the applied `--audit-profile <category>` flag value (`human-tui`, `file-traversal`,
  `posix-utility`, or `diagnostic-only`). `null` when no profile is set. See `coverage/matrix.json` under
  `audit_profiles` for the committed per-category mapping of which audit IDs each profile suppresses.
- `tool`: identifies what was scored. `name` is always present and follows a four-tier fallback (command name, binary
  basename, manifest package name, project directory basename) that matches the site registry's slug convention.
  `binary` is the executable basename when one is located; `null` for project-mode runs without a built artifact.
  `version` is best-effort: project-mode prefers the manifest version (`Cargo.toml`/`pyproject.toml`), command/binary
  mode probes `<bin> --version` then `-V`. `null` when probing failed or was declined by the self-spawn guard. The
  site's `registry.yaml` `version_extract` snippets remain authoritative for tools whose self-report is unreliable.
  Schema `0.4` addition.
- `anc`: identifies the `anc` build that produced the scorecard. `version` is the crate version at compile time.
  Informational, not a signed provenance signal. Pair with a Sigstore-signed release artifact when provenance is
  required. Schema `0.4` addition.
- `run`: run-level facts. `invocation` is the user's argv joined with shell-safe quoting, captured **before**
  default-subcommand injection so it reflects what the user typed (`anc .`, not `anc audit .`). `started_at` is RFC 3339
  UTC. `duration_ms` is wall-clock milliseconds. `platform.os` / `platform.arch` come from `std::env::consts`. Schema
  `0.4` addition.
- `target`: what `anc` was pointed at. `kind` is `"project"` (directory), `"binary"` (executable file), or `"command"`
  (PATH-resolved name from `--command`). `path` is the **basename** of the resolved target (project directory name or
  binary file name), never the absolute path, so home-dir usernames and employer directory layouts do not leak into
  scorecards committed to repos or posted by agents. `command` carries the user-supplied name for command mode. The
  unused field is always `null`, never missing. Consumer code can access both fields unconditionally. Schema `0.4`
  addition.
- `badge`: agent-native badge derivation from the live run. `score_pct` is the credit-weighted, behavioral-only
  leaderboard score (see [Scoring](#scoring)); `n_a` / `skip` / `error` and all non-behavioral rows are excluded from
  the ratio. `eligible` is true iff `score_pct >= 70` **and** a tool slug was derivable. `embed_markdown` is `null`
  below the floor (the convention is "do not nag" until earned). `scorecard_url` and `badge_url` are populated whenever
  a slug exists, even below the floor, so the site renders an SVG for every scored tool (a regression below the floor
  shifts color rather than 404s). `convention_url` always points at `https://anc.dev/badge`. Schema `0.5` addition.

> Publishing a scorecard? `run.invocation` may carry usernames or absolute paths from the machine that produced the
> scorecard. `target.path` is intentionally the basename only and is safe to commit. Review `run.invocation` before
> publishing. `anc` does not silently redact, since that would surprise users debugging their own runs.

## Contributing

Three shapes of contribution, in order of cost:

1. **Signal** (false-positive report, scoring bug, feature request, registry submission): file an issue with the
   matching template at
   [github.com/brettdavies/agentnative-cli/issues/new/choose](https://github.com/brettdavies/agentnative-cli/issues/new/choose).
2. **Proposal** (new language auditor, scoring-engine rework, registry expansion): open a design issue first; the
   maintainer signs off before code lands.
3. **Code**: PR against `dev` (per branch discipline).

Local setup:

```bash
git clone https://github.com/brettdavies/agentnative-cli
cd agentnative-cli
git config core.hooksPath scripts/hooks  # fast checks on commit, full CI mirror on push
cargo test
cargo run -- audit .
```

The full tier breakdown, pre-push hook contents, and commit-message conventions live in
[`CONTRIBUTING.md`](./CONTRIBUTING.md). Cross-repo routing: principle-level discussion (MUST/SHOULD/MAY tier changes,
new principles, applicability clauses) goes to the
[spec repo](https://github.com/brettdavies/agentnative/issues/new/choose); site bugs (rendering, performance) to
[brettdavies/agentnative-site](https://github.com/brettdavies/agentnative-site/issues/new/choose).

## License

MIT OR Apache-2.0
