---
title: "Handoff: P1 doctrine call"
type: handoff
date: 2026-04-20
---

# Handoff: P1 doctrine call

**Written for**: the session that picks up the P1 follow-up work from the 2026-04-17 investigation. Read this doc, then
the two pre-reads, then run the doctrine call.

## The question, in one sentence

Does `agentnative` consider TTY-driving agents (tmux panes, `ssh -t` sandbox shells, `expect`/Ansible automation,
computer-use desktop agents) a first-class audience for the P1 principle, or does it treat "agent = subprocess with
piped stdin" as the canonical shape and leave TTY-driving agents out of scope?

The answer drives every follow-up decision about strengthening the `p1-non-interactive` check and shapes the principle
doc itself. It is a doctrine decision, not an implementation decision, and should not be resolved by writing code.

## Why this is the right next move

The P1 investigation (`docs/plans/spikes/2026-04-17-p1-non-interactive-check-gap.md`) found that `anc`'s current probe
correctly verifies an implication of P1's SHOULD clause but verifies zero of P1's four MUST bullets. Three cheap
`--help`-scan extensions would close most of that gap. However, any stricter probe (especially a PTY probe) will start
flagging tools the principle explicitly permits under its MAY clause — which today allows rich interactive UX when a TTY
is detected, as long as the non-interactive path works. The MAY clause is the reason the Claude Code CLI passes P1
despite launching a TUI when typed bare in a terminal.

If the project wants stricter checks, the MAY clause likely needs to move before a stricter probe can speak with
authority. Skipping this decision and writing a P1 follow-up plan first risks producing checks whose verdicts contradict
the principle doc they claim to enforce.

## Pre-reads (in this order)

1. `~/dev/agentnative-site/content/principles/p1-non-interactive-by-default.md` — the principle as currently written.
   Pay attention to the MUST / SHOULD / MAY split, especially the MAY that permits TUI-when-TTY.
2. `docs/plans/spikes/2026-04-17-p1-non-interactive-check-gap.md` — the investigation that surfaced this question. The
   key sections are:

- "Is claude actually P1-compliant?" (the MUST/SHOULD/MAY table)
- "Addendum: is the 'agent = non-TTY' assumption even true?" (the empirical case for TTY-driving agents)
- "Open doctrine question: what does P1 actually require?" (the three framings this handoff operationalizes)

If time allows, skim `docs/plans/spikes/2026-04-17-session-handoff.md` for the broader session context — optional.

## The three framings

Each framing is a self-consistent answer. The doctrine call picks one and records the reasoning.

### Framing A — Tighten MAY

The MAY clause becomes: "rich interactive UX when a TTY is detected, *and* the tool advertises a scriptable escape
hatch from any TUI default (e.g., `-p / --print`, `--no-tui`, `--headless`, `--batch`)."

- **Signals that favor this**: the project wants subprocess-piped *and* TTY-driving agents to have a uniform compliance
  story. Claude Code already clears this (it has `-p`), so the real target is tools whose bare invocation is a TUI with
  no documented escape hatch at all.
- **Consequences**: P1 stays as a single principle. Three cheap behavioral checks (flag-existence, env-var hint,
  headless-auth subcommand scans from the P1 spike's Recommendations) become P1-compliance tests, not just MUST
  coverage. A PTY probe eventually becomes reasonable as a companion check.
- **Principle-doc edits required**: yes, the MAY clause is rewritten. Likely 2–4 lines changed in the principle doc.

### Framing B — Add a sibling principle

P1 stays exactly as written. Introduce a new principle (working titles: "P1b: TUI-mode must be scriptable," or
"P8: Rich UI must not be the only UI") with its own MUST/SHOULD/MAY.

- **Signals that favor this**: the project wants P1's identity to remain "don't hang the agent" and considers
  TUI-scriptability a distinct concern worth its own namespace.
- **Consequences**: cleaner separation on the scorecard. Claude passes both P1 and P1b (has a TUI + has `-p`). A tool
  without any TUI doesn't get measured by P1b at all (applicability gate). PTY probe lives under P1b, not P1.
- **Principle-doc edits required**: yes, a new file at `~/dev/agentnative-site/content/principles/` plus a mention in
  whatever aggregates the principle list. No edits to the existing P1 doc.

### Framing C — Scope TTY-driving agents out

`agentnative` explicitly targets subprocess-piped agents. TTY-driving agents are a different product; tools that
fail under a PTY but work under `/dev/null` are not this linter's problem.

- **Signals that favor this**: the data says most shipping commercial coding agents use piped subprocesses (Claude
  Code's `Bash` tool, Cursor, Aider, Zed Assistant all default to piped); the TTY-driving agent ecosystem is smaller and
  more specialized. Keeping scope narrow keeps the linter's opinions defensible.
- **Consequences**: P1 stays as-is. The P1 spike's three cheap extensions still ship (they verify MUST bullets that are
  MUST regardless of audience). A PTY probe is explicitly rejected. The principle doc gets a scope note explaining what
  "agent" means.
- **Principle-doc edits required**: small — a new "Scope" section clarifying "agent" = piped subprocess. No rewording of
  existing MUST/SHOULD/MAY text.

## How to settle it

This is a product-doctrine decision, not a research question. Signals worth gathering before deciding:

1. **Who actually uses `anc` today?** `git log` + any issue tracker + `docs/` are the obvious sources. The `docs/plans/`
   history and release notes may name real-world CLIs the tool was validated against.
2. **What do the adjacent principles already commit to?** If P3 (Progressive Help), P6 (Composable Structure), or P7
   (Bounded Responses) implicitly assume piped stdin, the doctrine call should stay consistent with that assumption. If
   they are agnostic, P1 has more freedom. Read all principle docs in `~/dev/agentnative-site/content/principles/` and
   note any PTY-sensitive claims.
3. **What's the friction cost of Framing A?** Many legitimate tools ship with interactive TUIs by default and require a
   flag to disable. Will this produce a lot of Warn verdicts on well-built tools, desensitizing the scorecard? A quick
   mental inventory of five CLIs the user cares about ( `gh`, `aws`, `kubectl`, `npm`, `pnpm`, `cargo`, `git`, etc.) —
   does each have a documented TUI escape hatch?

None of these need exhaustive research. 30 minutes of reading and judgment is the intended budget.

## Recommended route

`/office-hours` or `/ce-brainstorm` rather than `/ce-plan`. The goal is a short written decision, not a plan. The
decision should not attempt to resolve the P1 *implementation* — that is a separate, downstream plan.

## Deliverable

A single requirements/doctrine doc at `docs/brainstorms/2026-04-NN-p1-audience-scope-requirements.md` that records:

1. The framing chosen (A, B, or C) with a one-paragraph rationale.
2. If A or B: the exact principle-doc edits required, written verbatim so they can be applied to the sibling repo
   (`~/dev/agentnative-site`) by copy-paste.
3. The scope definition of "agent" as it applies to `agentnative`'s audience, in one or two sentences.
4. A one-line trigger for when this decision should be revisited (e.g., "revisit if PTY-driving agents exceed 20% of
   `anc` invocations in any telemetry we collect").

The doc should be short — one page, maybe two. It is a decision record, not a design doc.

## What this unblocks

Once the doctrine doc is committed:

1. **The P1 follow-up plan.** `/ce-plan` against whichever P1-check scope the doctrine chose. Candidate checks are
   already listed in the P1 spike's "Recommendations" section (five items); the doctrine choice selects which subset
   applies. The plan file goes at `docs/plans/2026-04-NN-NNN-feat-p1-check-depth-plan.md` (sequence number determined by
   other files dated the same day).
2. **Any principle-doc edits to `~/dev/agentnative-site`.** Apply the verbatim edits from the doctrine doc, commit on a
   feature branch in that repo, open a PR (that repo's convention, not this one's).
3. **Independent of this**: multi-lang plan Unit 2 can still execute on its own branch whenever the project wants to
   build rather than decide. Doctrine does not block code work on the Go/Ruby/TypeScript checks.

## Out of scope for this doctrine call

- Any edit to `src/` or `src/checks/`. The check code stays untouched until the plan built from the doctrine doc says
  otherwise.
- Implementation details of the PTY probe. Whether/how to probe is a plan-level question downstream of doctrine.
- Rewording of principles other than P1. Other principles may have adjacent issues; those are separate doctrine calls if
  the project wants to run them.

## Pointers

- Current P1 check code: `src/checks/behavioral/non_interactive.rs` and `src/checks/project/non_interactive.rs`.
- Current P1 principle doc: `~/dev/agentnative-site/content/principles/p1-non-interactive-by-default.md`.
- The investigation that produced this handoff: `docs/plans/spikes/2026-04-17-p1-non-interactive-check-gap.md`.
- Broader session context: `docs/plans/spikes/2026-04-17-session-handoff.md`.
