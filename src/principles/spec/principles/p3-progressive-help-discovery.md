---
id: p3
title: Progressive Help Discovery
last-revised: 2026-04-22
status: active
requirements:
  - id: p3-must-subcommand-examples
    level: must
    applicability:
      if: CLI uses subcommands
    summary: Every subcommand ships at least one concrete invocation example (`after_help` in clap).
  - id: p3-must-top-level-examples
    level: must
    applicability: universal
    summary: The top-level command ships 2–3 examples covering the primary use cases.
  - id: p3-should-paired-examples
    level: should
    applicability: universal
    summary: Examples show human and agent invocations side by side (text then `--output json` equivalent).
  - id: p3-should-about-long-about
    level: should
    applicability: universal
    summary: Short `about` for command-list summaries; `long_about` reserved for detailed descriptions visible with `--help`.
  - id: p3-may-examples-subcommand
    level: may
    applicability: universal
    summary: Dedicated `examples` subcommand or `--examples` flag for curated usage patterns.
---

# P3: Progressive Help Discovery

## Definition

Help text MUST be layered so agents (and humans) can drill from a short summary to concrete usage examples without
reading the entire manual. The critical layer is the one that appears **after** the flags list, because that is where
readers look for invocation patterns.

## Why Agents Need It

Agents discover how to use a tool by calling `--help` and scanning the output. They skip past flag definitions (which
describe what is *possible*) and hunt for examples (which describe what to *do*). A flags list alone is enough rope to
produce a failed invocation; examples are what turn discovery into action. Without examples in the help output, an agent
trial-and-errors its way into a working call, burning tokens and sometimes landing on a wrong-but-silent success.

## Requirements

**MUST:**

- Every subcommand ships at least one concrete invocation example showing the command with realistic arguments, rendered
  in the section that appears after the flags list. In clap this is the `after_help` attribute.
- The top-level command ships 2–3 examples covering the primary use cases.

**SHOULD:**

- Examples show human and agent invocations side by side — a text-output example followed by its `--output json`
  equivalent. Readers see the pair; agents see the JSON form.
- Short `about` for command-list summaries; `long_about` reserved for detailed descriptions visible with `--help` but
  not `-h`.

**MAY:**

- A dedicated `examples` subcommand or `--examples` flag that outputs a curated set of usage patterns for agent
  consumption.

## Evidence

- `after_help` (or `after_long_help`) attribute on the top-level parser struct.
- `after_help` attribute on every subcommand variant.
- Example invocations in `after_help` text that include realistic arguments, not placeholder `<foo>` tokens.
- Both `about` (short) and `after_help` (examples) present on each subcommand.

## Anti-Patterns

- Relying solely on `///` doc comments — those populate `about` / `long_about`, not `after_help`, so no examples render
  after the flags list.
- A single `about` string serving as both summary and usage documentation.
- Examples buried in a README or man page but absent from `--help` output.
- `after_help` text that describes the flags in prose instead of demonstrating them in code.

Measured by check IDs `p3-help`, `p3-after-help`, `p3-version`. Run `agentnative check --principle 3 .` against your CLI
to see each.

## Pressure test notes

### 2026-04-27 — Show HN launch red-team pass

Adversarial review via `compound-engineering:ce-adversarial-document-reviewer` ahead of the v0.3.0 launch. Findings
recorded verbatim per `principles/AGENTS.md` § "Pressure-test protocol".

- **[later]** *Internal inconsistency.* "The SHOULD on paired examples ('text then `--output json` equivalent')
  implicitly gates P3 conformance on a `--output json` mode, which is P2's territory. A CLI without structured output
  cannot satisfy this SHOULD even in spirit, yet `applicability: universal`." Deferred: narrowing `applicability` from
  `universal` to conditional (`if: CLI exposes a structured-output mode`) fires the coupled-release norm (CLI registry
  parses `applicability`). Bundled with other applicability cleanups for a v0.4.0 PR with explicit registry
  coordination.
- **[later]** *Must-vs-should.* "'Top-level command ships 2–3 examples' as a universal MUST is too strong for genuinely
  single-purpose CLIs (e.g., `cat`, `true`, a one-shot wrapper) where one canonical invocation is the entire surface.
  The '2–3' count baked into a MUST will draw HN fire as cargo-culted." Deferred: softening to "at least one example,
  and 2–3 when the tool has multiple primary use cases" is a MUST-content change that drifts the frontmatter summary.
  Bundled with other MUST-content softenings for a v0.4.0 PR.
- **[later]** *Prior art.* "Principle is clap-flavored throughout (`after_help`, `about`/`long_about`, `///` doc
  comments in Anti-Patterns) without a single sentence acknowledging the non-Rust analog (docopt usage block, `argparse`
  epilog, `cobra` Example field, `gh`/`kubectl` Examples convention). HN will call this 'a clap style guide, not a CLI
  standard.'" Deferred: a cross-framework analog appendix is a meaningful addition. The Definition / Why-Agents-Need-It
  sections are framework-agnostic; the Evidence section is intentionally clap-keyed. Worth revisiting in v0.4.0 once the
  standard's multi-language reach is clearer; site copy may also be a better home than the principle file itself.
