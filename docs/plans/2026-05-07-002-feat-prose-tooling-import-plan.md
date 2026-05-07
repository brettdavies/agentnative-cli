---
title: "feat: Import shared prose tooling and author linter-channel design context"
type: feat
status: active
date: 2026-05-07
---

# feat: Import shared prose tooling and author linter-channel design context

## Summary

Vendor the shared prose-linting tooling from `agentnative-spec` (`BRAND.md`, four vale rule packs, `prose-check.sh`,
test harness) into `agentnative-cli` via a new `scripts/sync-prose-tooling.sh`, author a fresh CLI-channel
`.impeccable.md` codifying linter-channel voice (distinct from spec-channel RFC-2119 register), wire prose-check into CI
on every PR, and lay down a constrained design brief for ast-grep-based in-code prose extraction so future passes can
lint clap help text, error messages, and panic strings. Sibling to the v0.4.0 spec sync (PR A); independent release
cadence, no governance window.

---

## Problem Frame

The `agentnative-spec` repo ships a five-pack vale config plus a 10K-line `prose-check.sh` that gate every prose change
against community style rules (proselint, write-good) and brand voice. The CLI repo has zero prose tooling — its README,
AGENTS.md, RELEASES.md, CHANGELOG.md, and (eventually) Rust string literals (clap `about=`, `eprintln!`,
`anyhow::bail!`) ship without any voice or quality gate. The spec channel and the CLI channel have *different* prose
registers (RFC 2119 third-person standards register vs. second-person imperative CLI prose), so wholesale-copying the
spec's `.impeccable.md` is wrong. The cross-channel base (`BRAND.md`) is the only file that should travel verbatim.

Without this work, every prose change to the CLI is inconsistent with the broader `agentnative` voice; AI-slop patterns
(em-dash density, "It's not X, it's Y", forced enthusiasm) leak into user-facing surfaces; and the shared tooling
improvements landing in `agentnative-spec` (e.g., the v0.4.0 vocabulary additions and LT denylist) never reach the CLI
because there's no sync mechanism.

---

## Requirements

- R1. A new `scripts/sync-prose-tooling.sh` exists and follows the same shape as `scripts/sync-spec.sh`: remote-first
  resolution, `git show <ref>:<path>` extraction (no working-tree perturbation), local fallback, `--check` drift mode.
- R2. The script vendors `BRAND.md`, `.vale.ini`, four vale style packs (`styles/{brand,config,proselint,write-good}/`),
  `scripts/prose-check.sh`, and `scripts/test-prose-check.mjs` from `agentnative-spec`. The `styles/spec/` pack is
  explicitly skipped (RFC-2119 register doesn't apply to CLI prose).
- R3. The vendored `prose-check.sh` is adapted to lint the CLI's prose-bearing surfaces: `README.md`, `AGENTS.md`,
  `RELEASES.md`, `CHANGELOG.md`, `.impeccable.md`, `docs/**/*.md`. Adaptation is path/glob changes only — rule logic is
  not forked.
- R4. A new `.impeccable.md` at repo root codifies linter-channel voice. Inherits from `BRAND.md`. Documents the
  CLI-prose register (second-person imperative *is* allowed and expected; RFC-2119 *is not* the register; error messages
  name what failed + why + what to do; help text follows clap conventions).
- R5. A new CI workflow `.github/workflows/prose-check.yml` runs `scripts/prose-check.sh` on every PR touching a
  prose-bearing file. Workflow uses pinned-SHA actions per global supply-chain policy.
- R6. A new CI workflow `.github/workflows/prose-tooling-drift.yml` runs `sync-prose-tooling.sh --check` on push to
  `dev`/`main` and on a weekly schedule. Catches drift between vendored copies and the upstream `agentnative-spec`.
- R7. A constrained design brief exists for ast-grep-based in-code prose extraction (`scripts/prose-check-rust.sh`) with
  implementation landing in this PR. Extracts clap `about=`/`long_about=`/`help=` strings, panic strings,
  `eprintln!`/`println!` literal args, and `anyhow::bail!`/`Error::msg` literals to a transient markdown file fed
  through `prose-check.sh`. False-positive rules skip ID strings (`pN-must-*`), file paths, and semver-shaped version
  constants.
- R8. Existing `scripts/SYNCS.md` documents the new sync script alongside `sync-spec.sh` and `sync-skill-fixture.sh`.
- R9. PR follows `.github/pull_request_template.md`. No AI attribution. Conventional Commits.
- R10. The auto-format hook on the developer's machine continues to handle markdown wrapping (120-col + markdownlint);
  prose-check is additive, not replacing.

---

## Scope Boundaries

- **Out: Pre-push integration.** Prose-check runs in CI only on first delivery. Adding it to `scripts/hooks/pre-push` is
  deferred — gate friction is real, and CI-only catches the same regressions one merge cycle later. Revisit if drift
  becomes painful.
- **Out: The `styles/spec/` vale pack.** RFC-2119 register doesn't apply to CLI prose. Vendoring it would produce
  systematic false-positives on every CLI README sentence ("To install, run `cargo install agentnative`" violates "no
  second-person imperative" from the spec pack — but that *is* the CLI register).
- **Out: Vale-on-Rust full design.** This PR ships markdown linting + clap-string extraction. A complete Rust prose
  pipeline (every error message, every log statement) is a follow-up after the markdown layer settles.
- **Out: Touching v0.4.0 spec sync files.** PR A handles `src/principles/spec/`, registry, checks. This PR keeps its
  diff to prose tooling.
- **Out: New skill bundle prose rules.** P8 is brand-new; bundle-prose-specific lint rules belong in a future iteration
  once shipped bundles accumulate enough convention to lint against.
- **Out: `BRAND.md` editing.** This PR vendors the upstream copy as-is. Brand updates flow upstream (PR against
  `agentnative-spec`) and are pulled here via the sync script.

---

## Context & Research

### Relevant Code and Patterns

- `scripts/sync-spec.sh` — canonical template for the new `sync-prose-tooling.sh`. Remote-first resolution (`git
  ls-remote --tags`, then `git clone --depth 1 --branch`), local fallback via `SPEC_ROOT` env var, `git show
  <ref>:<path> > dest` extraction. Trap-based cleanup. **Does NOT take a `--check` mode currently** — pattern for that
  comes from `scripts/sync-skill-fixture.sh --check` (the skill fixture drift script).
- `scripts/sync-skill-fixture.sh` — second template, especially for `--check` drift mode (clones upstream, `cmp`s blob
  by blob, exits non-zero on diff). The new prose-tooling sync extends to multiple files but uses the same shape.
- `scripts/SYNCS.md` (18.5K) — existing index of sync scripts; the new script registers here.
- `scripts/hooks/pre-push` — Rust gates only today (fmt, clippy, test, deny, Windows). Out-of-scope to extend.
- `.github/workflows/skill-fixture-drift.yml` — pattern for the new `prose-tooling-drift.yml`. Cron schedule + push
  trigger + drift exit code.
- `agentnative-spec` upstream layout (validated via earlier ground-truth reads):
- `BRAND.md` (~5.8K, repo root)
- `.vale.ini` (~1.1K, repo root)
- `styles/{brand,config,proselint,spec,write-good}/*.yml` (~2.8KB across 5 packs; we vendor 4)
- `scripts/prose-check.sh` (~10.5K)
- `scripts/test-prose-check.mjs` (~2.3K)
- `.impeccable.md` (~4.8K — read for shape, NOT copied)

### Institutional Learnings

- `docs/solutions/architecture-patterns/cross-repo-artifact-sync-commit-over-fetch-20260420.md` — committed-copy
  pattern. Producer-side `--check` drift guard makes the pattern safe. Same shape applies here.
- `docs/solutions/best-practices/byte-equivalence-regression-tests-for-copied-design-artifacts-2026-04-14.md` — the
  drift-detection contract. The CI drift workflow + manual `--check` mode together close the loop.
- `docs/solutions/best-practices/agentnative-version-model-2026-05-01.md` — names the four-repo ecosystem and the
  load-bearing rule "vendored copies must remain byte-equivalent to upstream until intentionally re-synced." The
  `--check` mode enforces this.

### External References

- ast-grep documentation: `https://ast-grep.github.io/` (used for U6's extraction patterns).
- Vale documentation: `https://vale.sh/docs` (no version-pinning needed; vale binary is pinned in CI).
- Spec repo at HEAD: `https://github.com/brettdavies/agentnative` (source of truth for vendored files).

---

## Key Technical Decisions

- **Vendor, do not symlink.** Symlinks break on Windows checkouts and obscure the byte-equivalence audit trail. The `git
  show <ref>:<path>` pattern from `sync-spec.sh` writes a real file; CI drift detection uses `cmp` against upstream.
- **Skip the `styles/spec/` vale pack.** RFC-2119 register is wrong for CLI prose. Vendoring would systematic-false-
  positive every README install instruction. The four packs we vendor (`brand`, `config`, `proselint`, `write-good`) are
  register-neutral.
- **CI-only on first delivery; pre-push deferred.** Prose-check runtime is a few seconds per file but cumulative
  pre-push pain is real. CI catches the same regressions; if drift between PR-only and merge-time becomes painful, add
  to pre-push as a follow-up.
- **`.impeccable.md` lives at repo root.** Mirrors spec's location. The name is unconventional but established; rename
  would fork from upstream's voice-tooling discovery.
- **`BRAND.md` is vendored, not authored.** Even though it's not strictly part of "lint contract," treating it as shared
  content with single upstream source prevents brand drift across the four-repo ecosystem (`agentnative`,
  `agentnative-cli`, `agentnative-site`, `agentnative-skill`).
- **Sync source ref is `dev`, not `main` or a tag.** The spec repo uses dev/main forever-branch flow. Prose tooling
  evolves continuously; pinning to a tag would freeze the CLI behind spec releases unnecessarily. Drift workflow uses
  the same ref. Override via `SYNC_REF` env var (e.g., for local testing against a feature branch).
- **In-code prose extraction lands in this PR but as a constrained design.** U6 specifies extraction targets,
  false-positive rules, output format, and integration point. The implementer fills in exact ast-grep patterns — Rust
  string-literal extraction has enough surface-area variance (raw strings, byte strings, format string fragments) that
  pre-specifying every pattern in the plan would over-constrain. The plan defines the design space and the acceptance
  bar; ast-grep patterns are an implementation detail.
- **Drift workflow runs weekly + on push.** Push catches "did this PR accidentally diverge?". Weekly catches "did
  upstream change without us pulling?". Both gates needed.

---

## Open Questions

### Resolved During Planning

- **Extend `sync-spec.sh` or create a parallel script?** Parallel script. `sync-spec.sh` is scoped to "the contract
  `anc` lints against" (`principles/`); prose tooling has different release cadence (continuous) vs. spec sync
  (tag-pinned). Conflating them would couple unrelated sync clocks.
- **Symlink `BRAND.md` to spec or vendor a copy?** Vendor. Windows-checkout breakage + byte-equivalence audit trail.
- **Which vale packs to vendor?** Four: `brand`, `config`, `proselint`, `write-good`. Skip `spec` (register mismatch).
- **CI-only or pre-push integration?** CI-only first. Pre-push later if drift becomes painful.
- **Where does `.impeccable.md` live?** Repo root, mirroring spec.
- **Author `.impeccable.md` from scratch or copy spec's?** Author from scratch. Spec voice rules ("no second-person
  imperative") are wrong for CLI prose; the linter channel needs its own voice document. Reference spec's `.impeccable
  .md` for *shape*, not content.

### Deferred to Implementation

- **Exact ast-grep patterns for clap macro extraction.** Surface variance (`#[arg(help = "…")]`, `#[command(about =
  "…")]`, doc-comment-as-help) means the implementer surveys real `src/cli.rs` usage during U6 and picks canonical
  patterns. The plan specifies extraction *targets* and *false-positive rules*, not the literal selectors.
- **Output format for extracted strings.** Concrete shape (one literal per line vs. grouped by file vs.
  source-location-anchored) decided at U6 implementation. Constraint: must be valid markdown so existing
  `prose-check.sh` consumes it without further adaptation.
- **Whether to run drift workflow nightly or weekly.** Default to weekly in plan; switch to nightly if a quarter of
  noise from spec-side commits without flow-down proves the cadence wrong.

---

## Implementation Units

### U1. Author `scripts/sync-prose-tooling.sh` (new sync script with `--check` mode)

**Goal:** Land the cross-repo sync mechanism that vendors prose tooling from `agentnative-spec`. Remote-first, local
fallback, `--check` drift mode. No content vendored yet — script only.

**Requirements:** R1.

**Dependencies:** None.

**Files:**

- Create: `scripts/sync-prose-tooling.sh`
- Modify: `scripts/SYNCS.md` (register the new script alongside `sync-spec.sh` / `sync-skill-fixture.sh`)
- Test: ad-hoc — invoke the script with `--check` against a known-clean state (no vendored files yet → drift report
  exits 1 because nothing matches; that's expected pre-U2 state).

**Approach:**

- Mirror `scripts/sync-spec.sh`'s top-level shape: env-var-configurable remote URL (default
  `https://github.com/brettdavies/agentnative.git`) and local fallback path (default `$HOME/dev/agentnative-spec`).
- Use `git ls-remote refs/heads/dev` (or `refs/heads/$SYNC_REF`) to resolve the upstream commit, then `git clone --depth
  1 --branch <ref>` into a temp directory; trap-cleanup as in `sync-spec.sh`.
- For each upstream path in the manifest, `git show <ref>:<path> > dest` to write the vendored copy.
- The manifest (the list of files and destinations) lives inline as a bash array — explicit pairs `[upstream-path ->
  local-dest]`. Keeps the script self-documenting; no separate config file.
- `--check` mode: clone same ref, read each upstream blob via `git show`, `cmp` against the local vendored copy. Print
  diffs. Exit 0 on byte-equal, 1 on any diff.
- Skip `styles/spec/` explicitly in the manifest. Document the skip in a comment.

**Patterns to follow:**

- `scripts/sync-spec.sh` (overall shape, error handling, trap cleanup).
- `scripts/sync-skill-fixture.sh` (specifically the `--check` mode pattern — the skill-fixture script does this
  byte-by-byte against the upstream JSON blob).
- Existing scripts header comment style (purpose, usage, env vars, resync cadence note).

**Test scenarios:**

- *Happy path*: Run `bash scripts/sync-prose-tooling.sh` with a clean upstream; vendored files appear; exit 0.
- *Happy path*: Run `bash scripts/sync-prose-tooling.sh --check` immediately after; exit 0 (just-vendored bytes
  byte-equal upstream).
- *Edge case*: Upstream unreachable + `SYNC_PROSE_ROOT` unset → exits 1 with clear error pointing at the env var.
- *Edge case*: Upstream unreachable + `SYNC_PROSE_ROOT` points at a clean local checkout → falls back successfully.
- *Failure path*: Edit a vendored file by hand to introduce drift; run `--check` → exit 1 with diff report.
- *Edge case*: Manifest path doesn't exist upstream → script exits 1 with clear error naming the missing path.

**Verification:**

- `shellcheck scripts/sync-prose-tooling.sh` clean.
- Script runs successfully against the live `agentnative-spec` `dev` branch.
- `--check` mode exits 1 before U2 runs (nothing vendored yet) and exits 0 after U2.

---

### U2. Vendor initial prose tooling + adapt `prose-check.sh` for CLI prose surfaces

**Goal:** Run U1's script for the first time, then adapt the vendored `scripts/prose-check.sh` for CLI prose surfaces.
Adaptation is path/glob changes only — rule logic stays untouched (any logic divergence breaks the byte-equivalence
contract on next sync).

**Requirements:** R2, R3, R10.

**Dependencies:** U1.

**Files:**

- Create: `BRAND.md` (vendored)
- Create: `.vale.ini` (vendored)
- Create: `styles/brand/*.yml` (vendored, 4-pack subset)
- Create: `styles/config/*.yml` (vendored)
- Create: `styles/proselint/*.yml` (vendored)
- Create: `styles/write-good/*.yml` (vendored)
- Create: `scripts/prose-check.sh` (vendored)
- Create: `scripts/test-prose-check.mjs` (vendored)
- Modify: `scripts/prose-check.sh` — adjust path globs ONLY (point at CLI prose surfaces, not spec's `principles/*.md`).
  All rule logic, vocabulary handling, LT denylist handling preserved verbatim.
- Modify: `.gitignore` — verify `styles/` and `BRAND.md` aren't accidentally ignored; tighten if needed.
- Test: invoke `bash scripts/prose-check.sh` on the existing `README.md` and `AGENTS.md` to surface any current prose
  findings (record but don't fix in this PR — see U3 / future work).

**Approach:**

- Run `bash scripts/sync-prose-tooling.sh` to write all vendored files. Verify the diff matches expected manifest.
- Adapt `prose-check.sh` path globs by editing the file's "files to lint" section. The CLI surfaces: `README.md`,
  `AGENTS.md`, `RELEASES.md`, `CHANGELOG.md`, `.impeccable.md` (will exist post-U3), `docs/**/*.md`. Spec-side globs
  (`principles/*.md`) are removed.
- **Critical: the adaptation must be tracked separately from the vendored content.** The byte-equivalence check in U1
  runs against the upstream `prose-check.sh`. To keep both mechanisms (vendor + adapt) coexisting, the recommended shape
  is:
- `scripts/prose-check.sh` — vendored verbatim from upstream. `--check` mode validates byte-equality against upstream.
- `scripts/prose-check-cli.sh` — thin wrapper that exports the CLI's path globs as env vars and invokes
    `prose-check.sh`. The wrapper is CLI-owned and not under sync.
- `prose-check.sh` upstream is parameterized via env vars (`PROSE_FILES`, `PROSE_VOCAB_PATH`) — verify upstream supports
    this; if not, file an upstream PR before this PR lands. **Decision: verify upstream parameterization during
    implementation; if absent, route to "fork-with-divergence-justification" pattern with a tracking issue.**
- Run `bash scripts/prose-check-cli.sh` (or equivalent) to record current findings on existing prose. Findings get fixed
  in-place when trivial (typo, em-dash density), or annotated with TODO comments and tracked separately.
- Update `scripts/SYNCS.md` to document the new tooling and its sync rhythm.

**Patterns to follow:**

- `scripts/sync-spec.sh` invocation pattern (one-time vendoring, then standard developer workflow).
- The unslop / vale workflow on the spec side — `prose-check.sh` already supports vocab additions and LT denylist; reuse
  those mechanisms verbatim.

**Test scenarios:**

- *Happy path*: Vendored files exist after script run; `bash scripts/sync-prose-tooling.sh --check` exits 0.
- *Happy path*: `bash scripts/prose-check-cli.sh` runs end-to-end without crashing; produces a prose-findings report.
- *Edge case*: A markdown file with no prose findings → script exits 0 with "OK" output.
- *Failure path*: A markdown file with deliberate slop (e.g., `It's not a feature, it's a way of life`) → script exits
  non-zero and names the offending line.
- *Integration*: Re-run `--check` after the wrapper edits → upstream `prose-check.sh` byte-equal, wrapper script not
  under check (correct — wrapper is CLI-owned).

**Verification:**

- `bash scripts/prose-check-cli.sh` runs clean (or surfaces only acknowledged-and-tracked findings).
- `vale --version` shows the binary is callable from `.vale.ini`.
- `git diff` shows the expected file-creation pattern (no spurious deletions, no spec/ pack vendored).

---

### U3. Author `.impeccable.md` for the linter channel

**Goal:** Codify the CLI-channel voice rules in a fresh `.impeccable.md`. Inherits from `BRAND.md` (vendored in U2);
explicitly diverges from spec-channel rules where the register differs.

**Requirements:** R4.

**Dependencies:** U2 (`BRAND.md` must exist before `.impeccable.md` references it).

**Files:**

- Create: `.impeccable.md`
- Modify: `AGENTS.md` — add a one-line pointer to `.impeccable.md` under conventions/style guidance (so future agents
  loading AGENTS.md discover the voice rules).

**Approach:**

- Structure mirrors spec's `.impeccable.md`: H1 frontmatter prose, "Channel: linter" section, "Audience" narrowed for
  CLI (developers using the tool, agents probing the tool, CI integrators), "Register" rules specific to CLI prose,
  "Linter-specific anti-patterns", "Voice anchor application", "Status".
- **Register rules to codify (key divergences from spec):**
- **Second-person imperative IS the register.** "Run `anc check`", "Set `--audit-profile human-tui`", "Pipe to `jq`".
    The spec channel bans this; the linter channel embraces it.
- **RFC 2119 is NOT the register.** No MUST/SHOULD/MAY in error messages or help text — those map to spec requirement
    IDs, not user-facing behavior.
- **Errors name three things.** What failed, why it failed, what to do next. Maps to P4 spec requirement; the
    `.impeccable.md` codifies the prose shape (not the structured-error JSON, which is a code concern).
- **Help text follows clap conventions.** `<arg>` for required, `[arg]` for optional, `--flag <VALUE>` for valued flags.
    No marketing copy in `--help` output.
- **No marketing voice.** No "powerful", no "blazing-fast", no "elegant". Describe what it does, not how it feels to
    use.
- **Diagnostic messages stay neutral.** No exclamation points, no apology, no anthropomorphizing the CLI ("I think this
    might be wrong" → "the value is invalid").
- **Linter-specific anti-patterns to call out:**
- "Helpful" multi-paragraph error messages that bury the actionable line.
- Suggestion text that names a flag that doesn't exist (false canonicalization).
- Mixing structured output and diagnostic prose on the same stream.
- Color codes in the prose itself (vale flags these as content, not formatting).
- Reference `BRAND.md` for cross-channel content (audience, anti-patterns universal across channels).
- Reference spec's `.impeccable.md` (file path, NOT content) so a future maintainer can see the sibling document.

**Patterns to follow:**

- Spec's `.impeccable.md` shape (channel, audience, register, anti-patterns, voice anchor, status sections).
- Existing `BRAND.md` conventions for voice anchor framing.

**Test scenarios:**

- *Happy path*: `bash scripts/prose-check-cli.sh .impeccable.md` runs clean (the voice rules eat their own dogfood).
- *Edge case*: After `.impeccable.md` is added, re-running prose-check on `README.md` doesn't suddenly flip from green
  to red (the new rules document existing voice; they don't impose new constraints retroactively).
- *Integration*: A future PR that violates a rule (e.g., adds "blazing-fast" to README) gets flagged by the proselint
  pack — manual verification with a test edit.

**Verification:**

- File exists at repo root.
- `vale .impeccable.md` runs clean.
- AGENTS.md update places the pointer under an existing conventions/style-guidance section (not a new top-level
  heading).

---

### U4. Add CI workflow for prose-check on every PR

**Goal:** Ship the CI gate so prose-check fires on every PR that touches a prose-bearing file. Pinned-SHA actions per
global supply-chain policy. Workflow scopes to dev/main as base branches.

**Requirements:** R5, R9.

**Dependencies:** U2.

**Files:**

- Create: `.github/workflows/prose-check.yml`

**Approach:**

- Trigger on `pull_request: { branches: [dev, main], paths: ['**.md', '.impeccable.md', 'styles/**', '.vale.ini',
  'scripts/prose-check*.sh'] }`. Path filter ensures the workflow doesn't fire on Rust-only PRs.
- Single job, single OS (`ubuntu-latest` is fine — vale is cross-platform but CI parity is one concern less).
- Steps:

1. Checkout (pinned `actions/checkout@<sha> # v4.x`).
2. Install vale (download release binary or use `errata-ai/vale-action@<sha>`; pick whichever is more stable per pinning
     helper output during implementation).
3. Install Bun or Node (pinned setup action) for the test harness.
4. Run `bash scripts/prose-check-cli.sh` — fails on non-zero exit.
5. Run `node scripts/test-prose-check.mjs` — sanity-tests the harness itself.

- All `uses:` lines pin to 40-char SHAs with trailing `# vX.Y.Z` comment per global SHA-pinning policy.
- Use `~/.claude/skills/github-repo-setup/scripts/pin-actions.sh` (per global CLAUDE.md) to resolve and validate pinned
  SHAs.

**Patterns to follow:**

- Existing `.github/workflows/skill-fixture-drift.yml` for action-pinning style.
- Other repo workflows for the path-filter idiom.

**Test scenarios:**

- *Happy path*: Open a PR that edits `README.md` cleanly → workflow runs and passes.
- *Edge case*: PR that doesn't touch any prose → workflow doesn't fire (path filter).
- *Failure path*: PR that introduces an em-dash density violation → workflow runs and fails; PR is blocked.
- *Pre-flight*: `actionlint .github/workflows/prose-check.yml` clean before merge.

**Verification:**

- Workflow runs on PRs against `dev`.
- A test PR that introduces deliberate slop fails CI on the prose-check step.
- All actions show 40-char SHA pins with trailing version comments.

---

### U5. Add drift-detection workflow + manifest test

**Goal:** Wire `sync-prose-tooling.sh --check` into a scheduled workflow + push trigger so drift between vendored copies
and upstream `agentnative-spec` surfaces fast. Add a smoke test that exercises the script's manifest list end-to-end.

**Requirements:** R6.

**Dependencies:** U1, U2.

**Files:**

- Create: `.github/workflows/prose-tooling-drift.yml`
- Create: `tests/prose_tooling_manifest.rs` — Rust integration test (since the project's test harness is `cargo test`)
  that shell-invokes `sync-prose-tooling.sh --check` and asserts exit 0.

**Approach:**

- Workflow triggers: `push: { branches: [dev, main] }`, `schedule: { cron: '0 9 * * 1' }` (Mondays 09:00 UTC),
  `workflow_dispatch:` for manual runs.
- Single job:

1. Checkout pinned.
2. Run `bash scripts/sync-prose-tooling.sh --check`.
3. On failure, post a GitHub issue (or comment on a tracking issue) summarizing the drift. Use existing
     `actions/github-script` or similar — pinned per supply-chain policy.

- Rust integration test: walk the vendored files; for each, assert it exists. Don't run the network drift check from
  Rust (that's the workflow's job) — the test only validates that vendored files are present and non-empty.

**Patterns to follow:**

- `.github/workflows/skill-fixture-drift.yml` (the closest sibling — same shape, different blob).
- Existing `tests/` directory pattern for integration tests.

**Test scenarios:**

- *Happy path*: Vendored files match upstream → workflow succeeds; Rust test passes.
- *Failure path*: Hand-edit a vendored file to simulate drift → the workflow fails on next run.
- *Edge case (workflow)*: Upstream renames `BRAND.md` → workflow fails with clear error (manifest mismatch); the failure
  is the signal to update `sync-prose-tooling.sh`'s manifest.
- *Edge case (Rust test)*: A vendored file is accidentally deleted from git → test fails locally and in CI.

**Verification:**

- `cargo test --test prose_tooling_manifest` passes.
- Workflow appears in `.github/workflows/`; `actionlint` clean.
- Workflow runs and passes on the U2-state branch.

---

### U6. Implement ast-grep-based in-code prose extraction (`scripts/prose-check-rust.sh`)

**Goal:** Extract prose from Rust source — clap macro arguments, panic strings, `eprintln!`/`println!` literals,
`anyhow::bail!`/`Error::msg` literals — into a transient markdown file, then feed it through the existing
`prose-check.sh` pipeline. False-positive rules skip ID strings, file paths, and version constants.

**Requirements:** R7.

**Dependencies:** U2 (the prose-check pipeline must be vendored and adapted first).

**Files:**

- Create: `scripts/prose-check-rust.sh`
- Create: `scripts/prose-extract-rust.sh` (or merge into above — implementer's call) — the ast-grep extraction step
- Create: `scripts/prose-check-rust.test.sh` — bash-based test using fixture Rust files
- Create: `tests/fixtures/prose_extraction/` — small Rust fixture files exercising extraction targets and false-positive
  rules
- Modify: `.github/workflows/prose-check.yml` (from U4) — add a step that runs `prose-check-rust.sh` on `src/**/*.rs`
  changes

**Approach:**

**Extraction targets** (the implementer designs the exact ast-grep selectors during U6):

- Clap derive attributes: `#[arg(help = "…")]`, `#[arg(long_help = "…")]`, `#[command(about = "…")]`,
  `#[command(long_about = "…")]`. Doc-comments-as-help (`/// …` above clap fields) where clap interprets them as help
  text.
- Direct user-facing prints: `eprintln!("…", …)`, `println!("…", …)`, `print!("…", …)`, `writeln!(stderr, "…", …)`.
  Capture only the format-string literal (first arg).
- Error construction: `anyhow::bail!("…", …)`, `anyhow::Error::msg("…")`, `format!("…", …)` when it appears in an
  error-construction context (heuristic; deferred to implementer).
- Panic strings: `panic!("…")`, `unreachable!("…")`, `todo!("…")`. (`unwrap_or_else(|e| panic!("…"))` etc.)

**False-positive rules** (the implementer encodes these as filters on the extracted set):

- Skip strings matching ID patterns: `^p\d-(must|should|may)-` (spec requirement IDs, not prose).
- Skip strings matching common file-path patterns: contains `/` AND ends in `.rs|.toml|.md|.yml|.yaml|.json`.
- Skip strings matching semver: `^\d+\.\d+\.\d+(-\w+)?$`.
- Skip strings under 5 chars (likely sigils, not prose: `"x"`, `"|"`, `" "`).
- Skip strings that are pure punctuation/format placeholders.
- Skip `cfg!`/`feature` attribute strings.

**Output format:**

- Single transient markdown file (e.g., `target/prose-extraction-rust.md`) with each extracted literal as its own
  bullet, anchored to source location:

  ```text
  - [src/cli.rs:42] "Long-running operations…"
  - [src/main.rs:118] "the value is invalid"
  ```

- Existing `prose-check.sh` consumes markdown — the format is markdown lists with prose content inside double-quotes.
- vale processes prose-bearing strings; anchor lines are skipped via vale comment markers.
- File written under `target/` so it's gitignored (Rust convention).

**Integration point:**

- `scripts/prose-check-rust.sh` orchestrates: invoke ast-grep extraction → write transient markdown → invoke
  `prose-check.sh` (or `prose-check-cli.sh`) on the transient file → propagate exit code.
- Optionally: support `--source-files <glob>` arg so CI runs only on changed files in a PR (faster gate).

**Patterns to follow:**

- Existing `src/source.rs` cross-language helpers (`has_pattern_in()`, etc.) — same `ast-grep-core` API surface; the
  bash script invokes the `ast-grep` CLI rather than the library, but the patterns translate.
- `scripts/sync-spec.sh` for shell-script structure (set -euo pipefail, trap cleanup, env-var configurability).

**Test scenarios:**

- *Happy path (extraction)*: Fixture Rust file with `#[arg(help = "Run the check.")]` → extracted line contains `"Run
  the check."`.
- *Happy path (extraction)*: Fixture with `eprintln!("permission denied: {}", path)` → extracted line contains
  `"permission denied: {}"`.
- *Happy path (extraction)*: Fixture with `panic!("internal invariant violated")` → extracted.
- *False-positive (skip)*: Fixture with `let id = "p1-must-no-interactive";` → NOT extracted.
- *False-positive (skip)*: Fixture with `let path = "src/main.rs";` → NOT extracted.
- *False-positive (skip)*: Fixture with `const VERSION: &str = "0.4.0";` → NOT extracted.
- *False-positive (skip)*: Fixture with `let separator = "|";` → NOT extracted (sub-5-char filter).
- *Happy path (pipeline)*: Fixture file produces transient markdown; `prose-check.sh` runs on it; offending strings fail
  the check.
- *Failure path*: Fixture with `panic!("Oh no something went wrong!!!")` (multi-bang em-dash density violation) →
  prose-check fails.
- *Edge case*: Empty source file → no extraction; pipeline exits 0.
- *Integration*: Run on real `src/` tree; record the prose findings (don't fix in this PR; track separately).

**Verification:**

- `bash scripts/prose-check-rust.test.sh` exits 0 (all fixture-based tests pass).
- `bash scripts/prose-check-rust.sh src/cli.rs` runs end-to-end and produces a prose-findings report or clean exit.
- `actionlint` validates the updated `.github/workflows/prose-check.yml`.
- ast-grep selectors handled in U6 are documented inline in the script (not just in the plan).

---

## System-Wide Impact

- **Interaction graph:** New CI workflows fire on PR + push + schedule. They don't interact with existing Rust gates
  (separate workflow files). Drift workflow may post issues — confirm `permissions: { issues: write }` is set if it
  does.
- **Error propagation:** Prose-check failures in CI block PR merges (good). The drift workflow failures don't block
  merges directly but should page someone via issue creation.
- **State lifecycle risks:** Vendored files are a new commit-discipline surface. The byte-equivalence contract means
  hand-edits to vendored files break the drift gate. Document this loudly in `scripts/prose-check.sh` header (or
  whatever the vendored content is) and in `scripts/SYNCS.md`.
- **API surface parity:** No CLI flags, no env vars in `anc` itself. The new env vars (`SYNC_PROSE_REMOTE_URL`,
  `SYNC_PROSE_ROOT`, `SYNC_REF`) are script-internal.
- **Integration coverage:** The U6 ast-grep extraction interacts with U2's adapted `prose-check.sh` via a transient
  markdown file. Integration test in U6's bash test harness exercises this end-to-end.
- **Unchanged invariants:** Existing `scripts/hooks/pre-push` Rust gates (fmt/clippy/test/deny/Windows) untouched.
  Existing CHANGELOG generation untouched. The auto-format hook on the developer's machine (markdown 120-col +
  markdownlint-cli2) continues to handle markdown wrapping; prose-check is additive on top.

---

## Risks & Dependencies

| Risk                                                                                                                                                     | Mitigation                                                                                                                                                                                                                                                                                                                                                     |
| -------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Upstream `prose-check.sh` doesn't support env-var-configurable file globs.                                                                               | Discover during U2; if absent, file an upstream PR (`agentnative-spec`) parameterizing the file list and add `prose-check.sh` to a tracking list of "fork-with-justification" until upstream merges. The wrapper-script pattern (U2) softens the impact.                                                                                                       |
| Drift workflow noisy due to weekly spec-side commits.                                                                                                    | Drift workflow only fails if vendored files diverge from upstream — most spec commits don't touch BRAND.md/vale-packs/prose-check.sh. If it does prove noisy, drop the schedule trigger and rely on push-only.                                                                                                                                                 |
| ast-grep false-positive rules under-coverage causes spurious prose findings on legitimate non-prose strings (URLs, regex patterns, format placeholders). | U6 fixture set must include every false-positive class encountered during initial real-source extraction. Triage cycle: extract → review findings → expand false-positive rules → re-run. Document the cycle in `scripts/prose-check-rust.sh` header.                                                                                                          |
| Vendored vale packs ship rules that fire on existing CLI prose (massive day-1 backlog).                                                                  | Run `prose-check-cli.sh` during U2 implementation; if backlog is huge, scope U2 to "wire the gate, don't fail CI yet" — make the new CI workflow run with `continue-on-error: true` until backlog is addressed in a follow-up PR. Decision deferred to implementation based on actual backlog size.                                                            |
| ast-grep CLI binary not available in CI.                                                                                                                 | Add an install step in `prose-check.yml` that downloads ast-grep release binary at a pinned SHA. The repo already uses `ast-grep-core` library; the CLI binary is a separate concern.                                                                                                                                                                          |
| Sync-script SHA pinning rule violated for one-off `git clone` invocation.                                                                                | The sync script clones a CLI tool target (not running CI actions); the SHA-pinning rule applies to GitHub Actions `uses:` lines, not arbitrary `git clone` in shell scripts. The script does pin the *ref* (`SYNC_REF`); SHA pinning of upstream tags applies if/when the script switches from branch to tag-based sync. Document the choice in script header. |
| `.impeccable.md` voice rules conflict with proselint/write-good defaults.                                                                                | The four vendored vale packs are register-neutral (not RFC-2119); conflicts unlikely. If found, vale supports per-file `<!-- vale off -->` comments — use surgically, not as a global escape valve.                                                                                                                                                            |

---

## Documentation / Operational Notes

- `scripts/SYNCS.md` registers the new sync script and documents the byte-equivalence contract loudly so future
  contributors don't hand-edit vendored files.
- `AGENTS.md` gets a one-line pointer to `.impeccable.md` so agents loading AGENTS.md discover voice rules.
- New CI workflows show up in the repo's status checks page; document their purpose in a CONTRIBUTING.md note (or
  `.github/workflows/README.md` if one exists).
- Post-merge, run `scripts/sync-prose-tooling.sh --check` locally on `dev` to confirm stability.
- The follow-up sequence after this PR ships:

1. Address the prose-check backlog on existing prose surfaces (separate PR or PRs).
2. Decide whether to flip CI from `continue-on-error` to hard-fail (if the backlog approach was used).
3. Eventually consider pre-push integration after CI proves the gate is reliable.

---

## Sources & References

- **Sibling PR (independent release):** PR A — v0.4.0 spec sync at
  `docs/plans/2026-05-07-001-feat-v0.4.0-spec-sync-plan.md`
- **Cross-repo sync template:** `scripts/sync-spec.sh`, `scripts/sync-skill-fixture.sh`
- **Sync script registry:** `scripts/SYNCS.md`
- **Vendored upstream:** `https://github.com/brettdavies/agentnative` — branch `dev` is the sync source of truth
- **Cross-repo artifact sync pattern:**
  `docs/solutions/architecture-patterns/cross-repo-artifact-sync-commit-over-fetch-20260420.md`
- **Byte-equivalence contract:**
  `docs/solutions/best-practices/byte-equivalence-regression-tests-for-copied-design-artifacts-2026-04-14.md`
- **Version model:** `docs/solutions/best-practices/agentnative-version-model-2026-05-01.md`
- **Spec channel `.impeccable.md` (reference shape only, NOT content):**
  `https://github.com/brettdavies/agentnative/blob/dev/.impeccable.md`
- **Spec `BRAND.md`:** `https://github.com/brettdavies/agentnative/blob/dev/BRAND.md`
- **PR template:** `.github/pull_request_template.md`
- **GitHub Actions SHA-pinning helper:** `~/.claude/skills/github-repo-setup/scripts/pin-actions.sh` (per global
  CLAUDE.md)
