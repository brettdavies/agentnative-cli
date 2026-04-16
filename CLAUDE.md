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

## Documented Solutions

`docs/solutions/` (symlink to `~/dev/solutions-docs/`) — searchable archive of past
solutions and best practices, organized by category with YAML frontmatter (`module`, `tags`, `problem_type`). Search
with `qmd query "<topic>" --collection solutions`. Relevant when implementing or debugging in documented areas.

## gstack Project History

This project was designed in the `brettdavies/agent-skills` repo, then moved here. gstack project data (design doc, eng
review, naming rationale, review history) has been copied to `~/.gstack/projects/brettdavies-agentnative/`.

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

## Source Check Convention

Every source check follows this structure:

- **Struct** implements `Check` trait with `id()`, `group()`, `layer()`, `applicable()`, `run()`
- **`check_x()` helper** takes `(source: &str, file: &str)` and returns `CheckStatus` (not `CheckResult`) — this is the
  unit-testable core
- **`run()` is the sole `CheckResult` constructor** — uses `self.id()`, `self.group()`, `self.layer()` to build the
  result. Never hardcode ID/group/layer string literals in `check_x()` or anywhere outside `run()`
- **Tests call `check_x()`** and match on `CheckStatus` directly, not `result.status`

This prevents ID triplication (the same string literal in `id()`, `run()`, and `check_x()`) and ensures the `Check`
trait is the single source of truth for check metadata.

For cross-language pattern helpers, use `source::has_pattern_in()` / `source::find_pattern_matches_in()` /
`source::has_string_literal_in()` with a `Language` parameter — do not write private per-language helpers in individual
check files.

## Dogfooding Safety

Behavioral checks spawn the target binary as a child process. When dogfooding (`anc check .`), the target IS
agentnative. Two rules prevent recursive fork bombs:

1. **Bare invocation prints help** (`cli.rs`): `arg_required_else_help = true` means children spawned with no args get
   instant help output instead of running `check .`. This is also correct CLI behavior (P1 principle).
2. **Safe probing only** (`json_output.rs`): Subcommands are probed with `--help`/`--version` suffixes only, never bare.
   Bare `subcmd --output json` is unsafe for any CLI with side-effecting subcommands.

**Rules for new behavioral checks:**

- NEVER probe subcommands without `--help`/`--version` suffixes
- NEVER remove `arg_required_else_help` from `Cli` — it prevents recursive self-invocation

## CI and Quality

**Pre-push hook:** `scripts/hooks/pre-push` mirrors CI exactly: fmt, clippy with `-Dwarnings`, test, cargo-deny, and a
Windows compatibility check. Tracked in git and activated via `core.hooksPath`. After cloning, run: `git config
core.hooksPath scripts/hooks`

**Windows compatibility:** Only `libc` belongs in `[target.'cfg(unix)'.dependencies]`. All SIGPIPE/signal code must be
inside `#[cfg(unix)]` blocks. The pre-push hook checks this statically.

**After pushing:** Check CI status in the background with `gh run watch --exit-status` (use `run_in_background: true` so
it doesn't block). Report failures when notified.
