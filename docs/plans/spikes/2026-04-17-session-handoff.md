---
title: "Session handoff: multi-language source checks + P1 check-depth spikes"
type: handoff
date: 2026-04-17
---

# Session handoff: multi-language source checks + P1 check-depth spikes

**Written for**: a fresh session (human or agent) picking up `brettdavies/agentnative` where 2026-04-17's planning
session left off. Read this doc first, then follow the pointers below.

## TL;DR

- Three new artifacts shipped: one plan + two spikes. All committed directly to `dev` (doc-only commits are allowed
  there; the feature-branch rule only applies to code changes). Nothing has been pushed yet.
- The multi-language plan (Go + Ruby + TypeScript source checks) is ready to execute starting at Unit 2.
- The P1 investigation exposed a doctrine question that should probably be resolved *before* the P1 follow-up plan is
  written.
- Scratch probe crate at `/tmp/anc-spike/` is disposable, not committed. `rm`/`git rm` are denied in this repo — use
  `trash` via Bash if cleanup is needed.

## How we got here (narrative)

The session opened with: "add source checks for Go, Ruby, and TypeScript; run a research spike first."

A plan was drafted (`docs/plans/2026-04-17-001-feat-multi-language-source-checks-plan.md`) with six implementation
units. Unit 1 was the spike itself — the user's whole point was to de-risk before committing to a shape. The spike
(`docs/plans/spikes/2026-04-17-multi-language-source-checks-spike.md`) built an empirical probe against the pinned
`ast-grep-language 0.42.0` and produced concrete findings: `$VAR` patterns work for every supported language (the
"Go/Ruby µ meta-var risk" the plan had flagged was imaginary — Rust and Python are *already* `impl_lang_expando!`); Go
has a real top-level pattern-parse hazard for selector calls (`os.Exit($CODE)` parses as an `ERROR` node, so AST walking
is required for selector-shaped checks); binary-size deltas sit well under the plan's caps (~220 KB Go, ~2.1 MB Ruby,
~2.9 MB TypeScript, ~5.2 MB total). The plan was updated to reflect those findings; Unit 1 was marked done. Committed to
a feature branch initially, then consolidated onto `dev` at session end.

Mid-session the user pivoted: "why does `anc check /home/brett/.local/share/claude/versions/2.1.113` pass
`p1-non-interactive` when `claude` bare in a terminal launches a TUI?" A targeted investigation produced a spike
(`docs/plans/spikes/2026-04-17-p1-non-interactive-check-gap.md`) that root-caused the pass: claude detects non-TTY,
assumes `--print` mode, and exits in 2.2 s with `Error: Input must be provided either through stdin or as a prompt
argument when using --print`. The check classifies that as `RunStatus::Ok → Pass`, and the pass is technically correct
per the principle's MAY clause. The real gap is that the check verifies zero of P1's four MUST bullets (env-var
bindings, `--no-interactive` flag existence, `--no-browser` / headless auth, falsey-value parser).

A follow-up exchange surfaced a deeper point: the check's "agent = non-TTY" assumption is **not** universally true.
TTY-driving agents are a growing category (Claude Code's own `--tmux` flag, `ssh -t` sandbox drivers,
`expect`/Ansible-style automation, computer-use-style desktop agents). Covering them would require a PTY probe —
materially more expensive than the cheap `--help`-scan extensions — and likely also a principle-doctrine revision, since
P1 today explicitly permits TUI-when-TTY as a MAY. That analysis was folded into the P1 spike as two new sections:
"Addendum: is the 'agent = non-TTY' assumption even true?" and "Open doctrine question: what does P1 actually require?".

User directive at session end: commit the documentation, no PR, hand off to the next session.

## Repo state

All three commits live directly on `dev`. The two working feature branches that existed during the session
(`feat/multi-language-source-checks-spike` and `investigate/p1-claude-false-pass`) were pruned after cherry-picking —
everything they contained was doc-only, so the branches added no signal worth preserving. Nothing has been pushed to
`origin/dev` yet.

Commit chain on `dev` (newest first):

1. `docs(spikes): consolidate session docs onto dev and refresh handoff` — this commit.
2. `docs(spikes): extend p1 spike with tty addendum and add session handoff`.
3. `docs(spikes): investigate why p1-non-interactive passes on claude`.
4. `docs(plans): add multi-language source-checks plan and unit-1 spike`.

User directive: **do not push or open PRs without explicit go-ahead.**

## Artifacts (read in this order)

1. **`docs/plans/2026-04-17-001-feat-multi-language-source-checks-plan.md`** — six-unit plan for adding Go, Ruby, and
   TypeScript source checks. Unit 1 checkbox is done; Units 2–6 are concrete and implementation-ready.

2. **`docs/plans/spikes/2026-04-17-multi-language-source-checks-spike.md`** — Unit 1 deliverable. Binary-size table,
   meta-var findings (`$VAR` works everywhere), Go top-level pattern-parse hazard, 4-check starter list per language,
   manifest-ordering confirmation. Explicit recommendations for each of Units 2–5.

3. **`docs/plans/spikes/2026-04-17-p1-non-interactive-check-gap.md`** — investigation of why claude passes P1. Also
   contains the TTY-agent addendum (pushing back on the "agent = non-TTY" assumption) and the doctrine question (does
   the project want to cover TTY-driving agents as a first-class audience?).

## Decisions outstanding (in dependency order)

1. **Proceed with the multi-language plan?** Next step is Unit 2 (cross-cutting infrastructure — new Language variants,
   multi-extension walk, manifest detection, feature-flag enablement, dispatch extension). The spike de-risked it
   thoroughly; no outstanding unknowns. If greenlit, Unit 2 lands as a single PR.

2. **Does agentnative target TTY-driving agents?** Doctrine question, not implementation. Three possible framings,
   summarized in the P1 spike's "Open doctrine question" section:

- Tighten MAY (require scriptable escape hatches from any TUI default).
- Add a sibling principle covering TUI-mode scriptability specifically.
- Explicitly scope TTY-driving agents out (agentnative targets subprocess-piped agents only). Settling this before
  writing a P1 follow-up plan avoids building a check whose verdicts contradict the principle doc it claims to enforce.

1. **Write the P1 follow-up plan.** Should not begin until (2) is settled. The P1 spike lists five candidate extensions
   in order of cost; whichever subset the doctrine decision blesses becomes the plan's scope.

## Recommended next move

Three plausible next actions, any of which is defensible:

- **Execute multi-lang Unit 2.** `/ce-work` against Unit 2 of the multi-language plan. Create a new feature branch off
  `dev` (Unit 2 is code, not docs — the feature-branch rule applies). The plan's file list, approach, and test scenarios
  are concrete enough for direct execution.
- **Resolve the P1 doctrine question.** `/office-hours` or `/ce-brainstorm` on "what audiences does agentnative serve,
  and which TTY modes are in-scope?" The output is a short requirements/doctrine doc at
  `docs/brainstorms/YYYY-MM-DD-p1-audience-scope-requirements.md`. The principle doc at
  `~/dev/agentnative-site/content/principles/p1-non-interactive-by-default.md` might need a companion edit depending on
  how the doctrine lands.
- **Ship the cheap P1 extensions anyway.** If the doctrine call is "keep P1 as-is," the three `--help`-scan extensions
  from the P1 spike's Recommendations (flag-existence scan, env-var hint scan, headless-auth subcommand scan) verify
  MUST bullets that are currently unchecked and are safe to ship without touching the principle doc. `/ce-plan` against
  those three checks.

If unsure, (2) unblocks (3) and is cheap — a 30-minute decision that saves re-writing a plan.

## Known gotchas

- **Feature branches are mandatory** per the project's memory policy. Never commit code to `dev` or `main` directly.
  Both working branches are already created.
- **`rm` and `git rm` are denied** in this repo's `settings.json`. Use `trash` via Bash (per the global CLAUDE.md) for
  file deletion.
- **Markdown files auto-format on write** via a PostToolUse hook (`md-wrap.py` + `markdownlint-cli2 --fix`). Don't
  manually wrap lines to 120 chars — the hook does it. Don't use `mdformat` / `pandoc` / `prettier` for markdown.
- **Scratch crate at `/tmp/anc-spike/`** was used for the multi-language spike's empirical probes (pattern matching and
  binary-size measurement). Disposable; the spike report documents exactly how to reconstruct it if needed. May not
  survive a reboot.
- **Ruby caveat**: `$PROGRAM_NAME` and any uppercase-letter Ruby global variable cannot appear literally in a
  `Pattern::try_new` string — ast-grep's expando rewriter treats uppercase `$VAR` tokens as meta-variables. Unit 5's
  entry-point guard detection must use AST walking + header-text inspection (same shape as Python's
  `sys_exit::is_main_guard`). `$0` / `$1` / etc. digit globals are safe to use literally.
- **Go caveat**: top-level bare selector patterns like `os.Exit($CODE)` fail to parse. Wrap them (`func $F() {
  os.Exit($CODE) }`) or AST-walk `call_expression > selector_expression`. Unit 3 of the multi-language plan reflects
  this.

## Memory

Nothing new added to auto-memory this session. The existing pointers remain accurate and are listed in
`~/.claude/projects/-home-brett-dev-agentnative/memory/MEMORY.md`.

## Pointers

- Repo conventions: `CLAUDE.md` (root of `agentnative`).
- Global workflow conventions: `~/.claude/CLAUDE.md` (commit-message format, feature-branch rule, PR template cascade,
  CLI-tool preferences, supply-chain pinning).
- Principle definitions: `~/dev/agentnative-site/content/principles/` (sibling repo; `p1-non-interactive-by-default.md`
  is the file the P1 spike references).
- Existing Rust checks: `src/checks/source/rust/` — sixteen checks; `unwrap.rs` and `process_exit.rs` are the templates
  for most Go and TypeScript units.
- Existing Python checks: `src/checks/source/python/` — three checks; `bare_except.rs` and `sys_exit.rs` are the
  templates for AST-walking logic in Go (selector calls) and Ruby (rescue, entry-point guards).
- Source-check convention (critical, codified in `CLAUDE.md`): `run()` is the sole `CheckResult` constructor;
  `check_x()` helpers return `CheckStatus`. No hardcoded ID/group/layer string literals outside `run()`.
- Cross-language pattern helpers: `src/source.rs` — `has_pattern_in`, `find_pattern_matches_in`,
  `has_string_literal_in`. Unit 2 of the multi-language plan extends these to Go, TypeScript, Ruby.
