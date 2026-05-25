# PRODUCT.md: linter channel design context

Channel-specific design context for the **linter channel** of agentnative — concretely, `anc`, the agent-native CLI
linter. Inherits the shared identity, voice anchor, audiences, and universal anti-patterns from [`BRAND.md`](BRAND.md).
Read that first.

## Inheritance

The linter channel sits in a three-tier waterfall. Each tier owns a different concern; nothing duplicates.

1. **Universal — [`BRAND.md`](BRAND.md).** Shared identity, voice anchor, audiences, universal anti-patterns. Vendored
   from `agentnative-spec` via [`scripts/sync-prose-tooling.sh`](scripts/sync-prose-tooling.sh).
2. **Channel delta — this file (`PRODUCT.md`).** Second-person imperative register, the three-part error shape (what
   failed, why, what to do next), 80-column help-text discipline, neutral diagnostics, stdout/stderr separation. The
   narrative companion to the executable Vale rule pack at [`styles/brand/`](styles/brand/) plus the CLI-local
   vocabulary at [`styles/config/vocabularies/cli/`](styles/config/vocabularies/cli/).
3. **Implementation — `src/`.** The Rust source for `anc`. Behavioral checks executed against compiled binaries; the
   principle registry codegen'd from `src/principles/spec/` (vendored from `agentnative-spec` via
   [`scripts/sync-spec.sh`](scripts/sync-spec.sh)).

## Channel: linter

The linter channel is `anc`'s own user-facing surface: the binary's `--help` output, error messages, diagnostic prose,
exit-code reasons, and the markdown docs that describe how to use the tool (`README.md`, `RELEASES.md`, `CLAUDE.md`,
`scripts/SYNCS.md`). It does not include the spec contract `anc` lints against (that's the spec channel) or the site
that hosts the badge artifact (that's the site channel).

The linter's job is to take a CLI and tell its operator — human or agent — what fails the standard, why, and what to
fix. Both halves of that sentence shape this channel's voice: a verdict that lands, and a remediation path the reader
can act on without re-reading.

## Audience (narrowed)

- **Humans running `anc`** during a refactor or pre-release pass. They are on the command line, mid-flow, and want a
  verdict plus an action. They read at terminal speed, not at desk speed. The high-leverage moment is the first line of
  a finding — if the actionable lede sits in paragraph three, the human stops reading.
- **AI agents invoking `anc audit --output json`** as a step in a larger pipeline. They consume the structured envelope,
  not the diagnostic prose. But when a finding's `message` field appears in a downstream agent's reasoning trace, that
  message has to make sense without the surrounding context. Agent UX is "does each `message` stand on its own; does
  each `id` stay stable across versions."
- **CI integrators** wiring `anc` into a release pipeline. They read prose at install time (README), at error time (CI
  log), and never in between. Both moments need to land on first read; CI logs in particular get screenshot, pasted into
  PR comments, and never re-read with surrounding context.

## Register

The narrative below describes the linter channel's voice rules; the executable contract for the literal phrases lives in
[`styles/brand/README.md`](styles/brand/README.md), generated from the Vale rule pack at `styles/brand/*.yml`, plus the
CLI-local vocabulary at [`styles/config/vocabularies/cli/`](styles/config/vocabularies/cli/).

- **Second-person imperative IS the register.** "Run `anc audit`", "Set `--audit-profile human-tui`", "Pipe the output
  to `jq`". This is the linter channel's defining departure from the spec channel — the spec describes contracts in
  third-person standards register; the linter addresses the reader directly. README install steps, CLI examples, and
  help text all use this voice.
- **RFC 2119 is NOT the register.** No MUST / SHOULD / MAY in error messages, help text, or user-facing prose. Those
  keywords map to spec requirement IDs (e.g., `p1-must-no-interactive`) and stay reserved for citing the spec, never for
  describing what `anc` itself does.
- **Errors name three things.** What failed (the rule ID and its plain-prose label), why (one short clause), what to do
  next (a verb + the artifact to touch). The order is load-bearing; the actionable verb cannot land in clause three.
  Maps to the P4 spec requirement; this rule fixes the prose shape, not the JSON envelope.
- **Help text follows clap conventions.** `<arg>` for required, `[arg]` for optional, `--flag <VALUE>` for valued flags,
  one-line summary in `about=`, multi-paragraph detail in `long_about=`. No marketing copy in `--help` output. No
  cute-but-undescriptive verbs; use the boring verb that names what happens.
- **Diagnostic messages stay neutral.** No exclamation points, no apology, no anthropomorphizing. "I think this might be
  wrong" → "the value is invalid". "Sorry, that didn't work!" → "exit code 2: invalid argument". Failure is a state, not
  an apology.
- **Present tense for state, imperative for actions.** "The binary is unreadable" + "make the file readable, then re-run
  `anc audit`." Not "The binary was unreadable" or "You would need to make the file readable."

## Linter-specific anti-patterns

These extend the universal bans in [`BRAND.md`](BRAND.md):

- **"Helpful" multi-paragraph error messages that bury the actionable line.** A reader skimming a CI log finds the rule
  ID and stops — if the fix lives three sentences later, it might as well not exist. Lead with the actionable verb;
  context goes underneath, not above.
- **Suggestion text that names a flag that doesn't exist.** False canonicalization on the CLI surface — a "did you mean
  `--audit-profiles`?" hint that points at a non-existent flag is worse than no hint at all because the reader acts on
  it. If the suggester can't verify the suggestion is real, the suggester does not run.
- **Mixing structured output and diagnostic prose on the same stream.** `anc audit --output json` writes JSON to stdout;
  diagnostics go to stderr. Mixing them strands consumers who pipe stdout into `jq` and get a parse error from a banner.
  The prose shape (verb + artifact) is the same on either stream; the stream choice is the load-bearing part.
- **Color codes in the prose itself.** Vale and prose-check operate on content, not formatting. ANSI escapes belong in
  the rendering layer, never in the source `&str` literal. The literal `"\x1b[31merror\x1b[0m"` is content rot; a
  separate `colorize(level, "error")` is the right shape.
- **Marketing voice in CLI surface.** "blazing-fast", "elegant", "powerful", "delightful" — banned. Describe what `anc`
  does, never how it feels to use. The brand vocab's `MarketingRegister` pack fires on this set; the per-channel rule is
  that the entire CLI surface inherits it, not just the README.
- **Multi-bang punctuation in panic strings or error messages.** `panic!("This should never happen!!!")` violates voice
  in two ways: the bangs read as anxious, and "should never happen" tells the operator nothing about what did happen.
  Replace with the neutral failure state: `panic!("unreachable: <invariant>")`.

## Voice anchor application

The pattern in [`BRAND.md`](BRAND.md), specialized for the linter channel:

- Each finding `message` opens with the verb the reader should run, or the state that broke.
- Each `--help` summary states what the subcommand does in one clause, no preamble.
- Each README install step is a complete shell invocation, executable as-is.
- Each error message ends with the next action, not the failure description.

## Status

This file is the linter channel's `PRODUCT.md`. The spec channel's equivalent at `agentnative-spec/PRODUCT.md` covers
the RFC-2119 contract register; the site channel's at `agentnative-site/PRODUCT.md` covers visual-system decisions;
cross-channel content lives in [`BRAND.md`](BRAND.md).

Coverage today: markdown surfaces only (`README.md`, `RELEASES.md`, `CLAUDE.md`, `scripts/SYNCS.md`, this file). The
high-leverage surface — clap `about=` / `help=` / `long_about=` literals, `eprintln!` / `anyhow::bail!` messages, panic
strings, check `label()` returns — is not yet linted. The deferred U6 unit of
[`docs/plans/2026-05-07-002-feat-prose-tooling-import-plan.md`](docs/plans/2026-05-07-002-feat-prose-tooling-import-plan.md)
introduces ast-grep-based extraction of those literals into a transient markdown file and runs them through this same
rule set. When U6 ships, the linter channel's voice rules apply to everything `anc` says, not just everything it
documents.
