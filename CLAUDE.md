# agentnative

The agent-native CLI linter. Checks whether CLI tools follow 7 agent-readiness principles.

## Architecture

Two-layer check system:

- **Behavioral checks** — run the compiled binary, language-agnostic (any CLI)
- **Source checks** — ast-grep pattern matching via bundled `ast-grep-core` crate (Rust, Python at launch)
- **Project checks** — file existence, manifest inspection

Design doc: `~/.gstack/projects/brettdavies-agentnative/brett-main-design-20260327-214808.md`

## Skill Routing

When the user's request matches an available skill, ALWAYS invoke it using the Skill
tool as your FIRST action. Do NOT answer directly, do NOT use other tools first.

**gstack skills (ideation, planning, shipping, ops):**

- Product ideas, "is this worth building", brainstorming → invoke office-hours
- Plan review, scope challenge, "think bigger" → invoke autoplan (or plan-ceo-review, plan-eng-review)
- Ship, deploy, push, create PR → invoke ship
- Bugs, errors, "why is this broken" → invoke investigate
- What did we learn, persist learnings → invoke learn
- Weekly retro → invoke retro
- Security audit → invoke cso
- Second opinion → invoke codex

**compound-engineering skills (code loop):**

- Implementation plan from repo code → invoke ce-plan
- Write code following a plan → invoke ce-work
- Code review before PR → invoke ce-review
- Document solution in docs/solutions/ → invoke ce-compound

For the full routing table, see `~/.claude/skills/docs/workflow-routing.md`.

## gstack Project History

This project was designed in the `brettdavies/agent-skills` repo, then moved here.
gstack project data (design doc, eng review, naming rationale, review history) has been copied to
`~/.gstack/projects/brettdavies-agentnative/`.

Key decisions already made:

- Name: `agentnative` with `anc` alias (see naming rationale)
- Approach B: bundled ast-grep hybrid (behavioral + source checks)
- ast-grep-core v0.42.0 validated via spike (3 PoC checks, 18 tests pass)
- Eng review: CLEARED, 10 issues resolved, 1 critical gap addressed
- Codex review: 12 findings, 3 actioned

## Conventions

- `ast-grep-core` and `ast-grep-language` pinned to exact version (`=0.42.0`) — pre-1.0 API
- `Position` uses `.line()` / `.column(&node)` methods, not tuple access
- Pre-build `Pattern` objects for `find_all()` — `&str` rebuilds on every node
- Feature flag is `tree-sitter-rust`, not `language-rust`
- Edition 2024, dual MIT/Apache-2.0 license

## CI and Quality

**Pre-push hook:** `scripts/ci-check.sh` mirrors CI exactly: fmt, clippy with `-Dwarnings`, test, cargo-deny, and a
Windows compatibility check. Installed as a git pre-push hook via symlink. If the hook is missing after a fresh clone,
reinstall: `ln -sf ../../scripts/ci-check.sh .git/hooks/pre-push`

**Windows compatibility:** Only `libc` belongs in `[target.'cfg(unix)'.dependencies]`. All SIGPIPE/signal code must be
inside `#[cfg(unix)]` blocks. The pre-push hook checks this statically.

**After pushing:** Check CI status in the background with `gh run watch --exit-status` (use `run_in_background: true` so
it doesn't block). Report failures when notified.
