---
id: p2
title: Structured, Parseable Output
last-revised: 2026-05-06
status: active
requirements:
  - id: p2-must-output-flag
    level: must
    applicability: universal
    summary: "`--output` flag selects format with `json` and `jsonl` as canonical machine-readable values; `text` is the default human-facing form."
  - id: p2-must-stdout-stderr-split
    level: must
    applicability: universal
    summary: Data goes to stdout; diagnostics/progress/warnings go to stderr, never interleaved.
  - id: p2-must-exit-codes
    level: must
    applicability: universal
    summary: Exit codes are structured and documented (0 success, 1 general, 2 usage, 77 auth, 78 config).
  - id: p2-must-json-errors
    level: must
    applicability: universal
    summary: When `--output json` is active, errors are emitted as JSON (to stderr) with at least `error`, `kind`, and `message` fields.
  - id: p2-must-schema-print
    level: must
    applicability:
      if: CLI emits structured output
    summary: "CLIs that emit structured output expose the output schema via a `schema` subcommand or `--schema` flag: runtime-discoverable, with a documented format identifier."
  - id: p2-should-consistent-envelope
    level: should
    applicability: universal
    summary: JSON output uses a consistent envelope (a top-level object with predictable keys) across every command.
  - id: p2-should-schema-file
    level: should
    applicability:
      if: CLI emits structured output
    summary: "Output schemas are also exported to a stable file path (e.g., `schema/<command>.json`) so CI/static-analysis consumers pin without invoking the tool."
  - id: p2-should-json-aliases
    level: should
    applicability: universal
    summary: "`--json` and `--jsonl` are accepted as aliases for `--output json` and `--output jsonl`; the short forms work alongside the canonical enum."
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
best-effort text parsing that fails unpredictably across versions, locales, and edge cases: silently at first,
catastrophically later.

## Requirements

**MUST:**

- Structured-output CLIs MUST offer at least one machine-readable format selectable via `--output`, with `json` and
  `jsonl` as canonical values; `text` is the default human-facing form. The format selection threads through every
  output path, so a single invocation never mixes formats.
- Data goes to stdout. Diagnostics, progress indicators, and warnings go to stderr. The split is decades-old Unix
  practice (POSIX, ESR's Rule of Repair, clig.dev's "Output" rules); for an agent it is load-bearing: a JSON consumer
  reading stdout MUST NOT encounter an interleaved progress line.
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

- When `--output json` is active, errors MUST be emitted as JSON to stderr with at least `error`, `kind`, and `message`
  fields. A plain-text error inside a JSON run breaks the consumer's parser on the only shape it was told to expect.
- CLIs that emit structured output (`--output json|jsonl`) MUST expose the output schema at runtime via a `schema`
  subcommand (or a `--schema` flag on each data-emitting subcommand). The schema MUST identify its format (canonical
  recommendation is JSON Schema 2020-12, the same dialect OpenAPI 3.1 uses), so an agent reading the schema loads the
  right validator without parsing prose. A consumer asking "what shape am I about to receive?" gets a machine-readable
  answer in one call.

**SHOULD:**

- JSON output uses a consistent envelope (a top-level object with predictable keys) across every command so agents can
  rely on the same shape.
- The schema SHOULD also be exported to a stable file path in the source repo (e.g., `schema/<command>.json`) so
  consumers can pin against it at install or CI time without invoking the tool. The print form is the runtime contract;
  the file form is the build-time contract.
- CLIs SHOULD accept `--json` as an alias for `--output json` and `--jsonl` as an alias for `--output jsonl`. The
  `--output` enum remains the canonical surface for the format MUST (`p2-must-output-flag`); a Cloudflare-style CLI
  shipping only the short forms still satisfies the canonical MUST through the alias path.

**MAY:**

- Additional `--output` values (CSV, TSV, YAML) MAY be offered beyond the canonical text/json/jsonl. The canonical three
  remain mandatory.
- A `--raw` flag for unformatted output suitable for piping to other tools.

## Evidence

- `OutputFormat` enum with `Text`, `Json`, `Jsonl` variants deriving `ValueEnum`.
- `OutputConfig` struct with `format`, `use_color`, and `quiet` fields.
- `serde_json` in `Cargo.toml`.
- No `println!` in `src/` outside the output module: every print goes through `OutputConfig`.
- Exit-code constants or match arms mapping error variants to distinct numeric codes.
- `eprintln!` (or an equivalent diagnostic macro) for every diagnostic line.

## Anti-Patterns

- `println!` scattered across handlers instead of routing through the output config.
- A single exit code (1) for everything: agents cannot distinguish auth failures from config errors.
- Status lines ("Fetching data…") printed to stdout where they contaminate JSON output.
- `process::exit()` in library code, bypassing structured error propagation.
- Human-formatted tables as the only output mode with no JSON alternative.

Measured by check IDs `p2-output-json`, `p2-output-format`, `p2-stderr-diagnostics`. Run `agentnative check --principle
2 .` against the CLI under test to see each.

## Pressure test notes

### 2026-04-27: Show HN launch red-team pass

Adversarial review via `compound-engineering:ce-adversarial-document-reviewer` ahead of the v0.3.0 launch. Findings
recorded verbatim per `principles/AGENTS.md` § "Pressure-test protocol".

- **[edit]** *Prior art.* "The exit-code table conflicts with `sysexits.h`. `EX_NOPERM=77` is 'permission denied'
  (close), but `EX_CONFIG=78` is correct. However, `sysexits.h` reserves `EX_USAGE=64`, `EX_DATAERR=65`,
  `EX_NOINPUT=66`, `EX_UNAVAILABLE=69`, `EX_SOFTWARE=70`. P2 puts 'usage error' at 2 (bash convention), not 64. HN will
  note the principle straddles two conventions (bash 0/1/2 + sysexits 77/78) without naming the hybrid." Resolved: added
  one sentence under the exit-code table acknowledging the bash + `sysexits.h` blend. The same citation now appears in
  P4's exit-code table (per Row #13 of the same review pass) so both files agree.
- **[later]** *MUST-vs-SHOULD.* "A single-number-emitting CLI (e.g., `epoch`, `uuidgen`) plausibly violates the
  `--output text|json|jsonl` MUST for a defensible reason. Universal applicability is a strong claim." Deferred: revisit
  whether `applicability` SHOULD soften when the launch landscape clarifies actual single-number agent-facing CLIs. The
  applicability change would fire coupled-release (CLI registry impact), so it is held for a v0.4.0 cleanup PR rather
  than churned during launch week.
