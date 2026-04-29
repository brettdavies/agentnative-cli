---
id: p2
title: Structured, Parseable Output
last-revised: 2026-04-22
status: active
requirements:
  - id: p2-must-output-flag
    level: must
    applicability: universal
    summary: "`--output text|json|jsonl` flag selects output format; `OutputFormat` enum threaded through output paths."
  - id: p2-must-stdout-stderr-split
    level: must
    applicability: universal
    summary: Data goes to stdout; diagnostics/progress/warnings go to stderr — never interleaved.
  - id: p2-must-exit-codes
    level: must
    applicability: universal
    summary: Exit codes are structured and documented (0 success, 1 general, 2 usage, 77 auth, 78 config).
  - id: p2-must-json-errors
    level: must
    applicability: universal
    summary: When `--output json` is active, errors are emitted as JSON (to stderr) with at least `error`, `kind`, and `message` fields.
  - id: p2-should-consistent-envelope
    level: should
    applicability: universal
    summary: JSON output uses a consistent envelope — a top-level object with predictable keys — across every command.
  - id: p2-may-more-formats
    level: may
    applicability: universal
    summary: Additional output formats (CSV, TSV, YAML) beyond the core three.
  - id: p2-may-raw-flag
    level: may
    applicability: universal
    summary: "`--raw` flag for unformatted output suitable for piping to other tools."
---

# P2: Structured, Parseable Output

## Definition

CLI tools MUST separate data from diagnostics and offer machine-readable output formats. Mixing status messages with
data forces agents into fragile regex extraction that breaks on any format change.

## Why Agents Need It

An agent calling a CLI needs three things from each invocation: the data, the error (if any), and the exit code. When
data goes to stdout, diagnostics go to stderr, and errors carry machine-readable fields, the agent parses the result
reliably without heuristics. Mix these channels or ship human-formatted output only, and the agent falls back to
best-effort text parsing that fails unpredictably across versions, locales, and edge cases — silently at first,
catastrophically later.

## Requirements

**MUST:**

- A `--output text|json|jsonl` flag selects the output format. Text is the default for humans; JSON and JSONL are the
  agent-facing formats. Implementation surfaces an `OutputFormat` enum and an `OutputConfig` struct threaded through
  every function that produces output.
- Data goes to stdout. Diagnostics, progress indicators, and warnings go to stderr. An agent consuming JSON from stdout
  must never encounter an interleaved progress message.
- Exit codes are structured and documented:

| Code | Meaning                           |
| ---: | --------------------------------- |
|    0 | Success                           |
|    1 | General command error             |
|    2 | Usage error (bad arguments)       |
|   77 | Authentication / permission error |
|   78 | Configuration error               |

  These codes blend the bash 0/1/2 convention with BSD `sysexits.h` 77/78 (`EX_NOPERM`, `EX_CONFIG`); the result is the
  de-facto agent-facing dialect, not strict `sysexits.h` compliance.

- When `--output json` is active, errors are emitted as JSON (to stderr) with at least `error`, `kind`, and `message`
  fields. Plain-text errors in a JSON run break the agent's parser on the only output it was told to expect.

**SHOULD:**

- JSON output uses a consistent envelope — a top-level object with predictable keys — across every command so agents can
  rely on the same shape.

**MAY:**

- Additional output formats (CSV, TSV, YAML) beyond the core three. The core three remain mandatory.
- A `--raw` flag for unformatted output suitable for piping to other tools.

## Evidence

- `OutputFormat` enum with `Text`, `Json`, `Jsonl` variants deriving `ValueEnum`.
- `OutputConfig` struct with `format`, `use_color`, and `quiet` fields.
- `serde_json` in `Cargo.toml`.
- No `println!` in `src/` outside the output module — every print goes through `OutputConfig`.
- Exit-code constants or match arms mapping error variants to distinct numeric codes.
- `eprintln!` (or an equivalent diagnostic macro) for every diagnostic line.

## Anti-Patterns

- `println!` scattered across handlers instead of routing through the output config.
- A single exit code (1) for everything — agents cannot distinguish auth failures from config errors.
- Status lines ("Fetching data…") printed to stdout where they contaminate JSON output.
- `process::exit()` in library code, bypassing structured error propagation.
- Human-formatted tables as the only output mode with no JSON alternative.

Measured by check IDs `p2-output-json`, `p2-output-format`, `p2-stderr-diagnostics`. Run `agentnative check --principle
2 .` against your CLI to see each.

## Pressure test notes

### 2026-04-27 — Show HN launch red-team pass

Adversarial review via `compound-engineering:ce-adversarial-document-reviewer` ahead of the v0.3.0 launch. Findings
recorded verbatim per `principles/AGENTS.md` § "Pressure-test protocol".

- **[edit]** *Prior art.* "The exit-code table conflicts with `sysexits.h`. `EX_NOPERM=77` is 'permission denied'
  (close), but `EX_CONFIG=78` is correct. However, `sysexits.h` reserves `EX_USAGE=64`, `EX_DATAERR=65`,
  `EX_NOINPUT=66`, `EX_UNAVAILABLE=69`, `EX_SOFTWARE=70` — P2 puts 'usage error' at 2 (bash convention), not 64. HN will
  note the principle straddles two conventions (bash 0/1/2 + sysexits 77/78) without naming the hybrid." Resolved: added
  one sentence under the exit-code table acknowledging the bash + `sysexits.h` blend. The same citation now appears in
  P4's exit-code table (per Row #13 of the same review pass) so both files agree.
- **[later]** *Must-vs-should.* "A single-number-emitting CLI (e.g., `epoch`, `uuidgen`) plausibly violates the
  `--output text|json|jsonl` MUST for a defensible reason. Universal applicability is a strong claim." Deferred: revisit
  whether `applicability` should soften when the launch landscape clarifies actual single-number agent-facing CLIs. The
  applicability change would fire coupled-release (CLI registry impact), so it is held for a v0.4.0 cleanup PR rather
  than churned during launch week.
