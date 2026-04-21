---
title: "v0.1.2 Handoff 4 — Eng Agent Execution Brief"
type: handoff
order: 4
phase: v0.1.2
written_for: eng-agent
depends_on: 2026-04-20-v012-handoff-4-behavioral-checks.md
---

# v0.1.2 H4 — Eng Agent Execution Brief

**Read-this-first for the engineer/agent implementing v0.1.2.** Self-contained: do not re-read sibling handoffs
(H1/H2/H3/H5), the CEO plan, or the eng-review test plan to do this work. The plan file
`2026-04-20-v012-handoff-4-behavioral-checks.md` in this same directory carries the *scope contract*; this doc carries
the *execution contract*.

## The job, in one sentence

Ship three new behavioral checks — `p1-flag-existence`, `p1-env-hints`, `p6-no-pager-behavioral` — plus a shared
`HelpOutput` cache so each check doesn't re-spawn `<binary> --help`.

## State snapshot (as of 2026-04-21)

**Live (shipped):**

- `agentnative` v0.1.1 — crates.io + GitHub Release + Homebrew bottles, all marked Latest.
- `PrincipleRegistry` at `src/principles/registry.rs` with 46 entries (23 MUST / 16 SHOULD / 7 MAY).
- `Check::covers()` trait method: each check declares the registry IDs it verifies via `fn covers(&self) -> &'static
  [&'static str]` (default: `&[]`).
- `anc generate coverage-matrix` subcommand: emits `docs/coverage-matrix.md` + `coverage/matrix.json` from registry
- `covers()` declarations. Drift detector (`anc generate coverage-matrix --check`) runs in CI.

- Scorecard JSON schema v1.1: `schema_version`, `coverage_summary: {must,should,may} × {total,verified}`, `audience:
  null` (reserved), `audit_profile: null` (reserved).

**On agentnative-site `dev` (not yet promoted to `main`):**

- H2 merged — P1/P5/P6/P7 spec text reworded, applicability gates annotated, `/coverage` page live, scorecard renderers
  consume v1.1 fields with graceful degradation for missing keys.
- H3 merged — all 10 committed scorecards regenerated with `anc` v0.1.1: `ripgrep`, `fd`, `jq`, `bat`, `dust`, `gh`,
  `claude-code`, `aider`, `llm`, `anc`. Zero residual `p6-tty-detection` / `p6-env-flags` IDs.

**Implication for you:** nothing about H2/H3 gates your work. They consume what `anc` emits. Post-merge
coordination is a follow-on PR to agentnative-site, not a blocker.

## Branch + first commands

```bash
cd /home/brett/dev/agentnative
git checkout dev && git pull
git checkout -b feat/v012-behavioral-check-expansion
```

Pre-push hook is already configured (mirrors CI: fmt / clippy -D warnings / test / cargo-deny / Windows compat check).
Trust it — if it fails, fix the root cause, don't bypass.

## Reference patterns (read once, reuse)

- **Behavioral check shape:** `src/checks/behavioral/non_interactive.rs`. Struct implements `Check`; `check_x()` helper
  takes `(source, run_status, ...)` and returns `CheckStatus`; `run()` is the sole `CheckResult` constructor, never
  hardcoding IDs outside `id()`.
- **`covers()` declaration pattern:** `CLAUDE.md` §"covers() Declaration". Static slice, no allocation, list all IDs the
  check verifies.
- **Registry entry shape:** `src/principles/registry.rs` — structs are `Requirement { id, level, applicability,
  description, ... }`. **Do not add new registry entries for H4**; this is `covers()` linkage only (see next section).

## Check-by-check implementation

### 1. `p1-flag-existence` (behavioral)

**File:** `src/checks/behavioral/flag_existence.rs`
**Registry linkage:** `covers(&["p1-must-no-interactive"])` **Why this is distinct from existing `p1-non-interactive`:**
that check probes runtime behavior (bare-invocation → help, agentic flag present, stdin-as-primary). This one probes
*flag surface area* — does the CLI advertise a non-interactive gate in `--help` at all. Two different behavioral proofs
of the same MUST. **Confidence:** `high` (regex match on parsed flag list is deterministic).

**Algorithm:**

1. Consume `HelpOutput::flags()` (see §"HelpOutput" below).
2. Pass if any of these flags is present: `--no-interactive`, `-p`, `--print`, `--no-input`, `--batch`, `--headless`,
   `-y`, `--yes`, `--assume-yes`.
3. Skip (Applicability::NotApplicable) if the target already satisfies P1's alternative gates: help-on-bare-invocation
   OR stdin-as-primary-input detected. Leave that determination to the existing `p1-non-interactive` behavioral check;
   read its result from the `CheckResult` accumulator rather than re-detecting.
4. Otherwise Warn with evidence: `"no non-interactive flag found in --help; expected one of: …"`.

### 2. `p1-env-hints` (behavioral)

**File:** `src/checks/behavioral/env_hints.rs`
**Registry linkage:** `covers(&["p1-must-env-var"])` **Why:** currently only source-verified via `p1-env-flags-source`.
Behavioral proof adds dual-layer coverage. **Confidence:** `medium` (heuristic — false-positives possible when `$PAGER`
etc. appears unrelated to flags).

**Algorithm:**

1. Consume `HelpOutput::env_hints()`.
2. Pass if `env_hints().len() > 0`. Detection via two patterns: clap-style `[env: FOO_BAR]` immediately after a flag
   description, OR bash-style `$FOO` / `TOOL_FOO` appearing within ~2 lines of a flag's description block.
3. Skip if `HelpOutput::flags().is_empty()` (a tool with no flags can't have env hints for flags).
4. Warn if flags exist but no env hints. Evidence lists the flag count found.

### 3. `p6-no-pager-behavioral` (behavioral)

**File:** `src/checks/behavioral/no_pager_behavioral.rs`
**Registry linkage:** `covers(&["p6-must-no-pager"])` **Why:** currently only source-verified via `p6-no-pager`. Adds
behavioral proof. **Confidence:** `medium` (heuristic — pager inference from `--help` text is soft).

**Algorithm:**

1. Consume `HelpOutput::flags()` and the full help text.
2. Pass if `--no-pager` is in `flags()`.
3. Skip if no pager signal in help text: none of `less`, `more`, `$PAGER`, `--pager`, `pager` mentioned.
4. Warn if pager is mentioned but `--no-pager` escape hatch is missing.

## Shared infrastructure: `HelpOutput`

**File:** `src/runner/help_probe.rs` (new module; add `pub mod help_probe;` to `src/runner/mod.rs`).

**Invariant:** one `<binary> --help` spawn per `Check` run per target. Multiple checks consuming `HelpOutput`
must share the same `Arc<HelpOutput>` for a given target. State leak between different target tools = wrong verdicts;
this is the main correctness risk.

**Shape:**

```rust
pub struct HelpOutput {
    raw: String,
    flags: OnceLock<Vec<Flag>>,
    env_hints: OnceLock<Vec<EnvHint>>,
    subcommands: OnceLock<Vec<String>>,
}

impl HelpOutput {
    pub fn probe(binary: &Path, timeout: Duration) -> Result<Self, RunnerError> { … }
    pub fn flags(&self) -> &[Flag] { self.flags.get_or_init(|| parse_flags(&self.raw)) }
    pub fn env_hints(&self) -> &[EnvHint] { … }
    pub fn subcommands(&self) -> &[String] { … }
}
```

`parse_flags`/`parse_env_hints`/`parse_subcommands` are the lazy parsers. Unit-test each independently with fixture
`--help` outputs for ripgrep, bird, xurl-rs.

**Runner wiring:** extend `src/runner.rs` so behavioral checks declaring a `HelpOutput` dependency receive the shared
`Arc<HelpOutput>` via a new param on `Check::run()`, or via a builder/context pattern — pick whichever minimizes churn
to existing behavioral check signatures. Guidance: prefer adding a `HelpContext` struct threaded through `run()` over
changing the `Check` trait signature, so existing checks keep compiling unchanged.

## Confidence field

Each `CheckResult` gains a `confidence: Confidence` field (`enum Confidence { High, Medium, Low }`). Extend
`CheckResult` and the scorecard serializer. Existing checks default to `High` (direct probe). Only `p1-env-hints` and
`p6-no-pager-behavioral` emit `Medium`.

**Schema impact:** adding a field to `CheckResult` changes the JSON scorecard shape. This is additive — existing
consumers (agentnative-site) feature-detect and tolerate missing fields, so no schema_version bump. Leave
`schema_version: "1.1"` as-is.

## Test plan (per check)

Mirror `src/checks/behavioral/non_interactive.rs`'s `#[cfg(test)] mod tests` block. Four cases per check:

1. **Happy path** — `--help` output contains the expected signal → `CheckStatus::Pass`.
2. **Skip applicability** — target doesn't apply to this check → `CheckStatus::Skip`.
3. **Warn condition** — target violates the requirement → `CheckStatus::Warn(evidence)`.
4. **Non-English help exception** — fixture with localized help → `CheckStatus::Skip` with evidence noting the
   English-only regex limitation (named exception in `docs/coverage-matrix.md`).

**`HelpOutput` unit tests** live in `src/runner/help_probe.rs`. Fixtures: short `--help` stubs as `&'static str`
constants. Test each lazy parser (`flags`, `env_hints`, `subcommands`) in isolation. Test caching by calling twice and
asserting idempotent results (not a second parse).

**Integration smoke:** after the unit tests pass, run `cargo run -- check ripgrep`, `cargo run -- check bird`, `cargo
run -- check xurl-rs`. Verdicts should be sensible — spot-check that `p1-flag-existence` passes on ripgrep (has
`--null`, `--no-messages`; warns — none of the listed flags — expected), `p6-no-pager-behavioral` passes on `bat` (has
`--paging=never`). These are sanity checks, not gating tests.

## Safety constraints (do not violate)

- **`arg_required_else_help = true` on the top-level `Cli` must remain.** Removing it re-enables the fork bomb when
  dogfooding. Documented in `CLAUDE.md` §"Dogfooding Safety".
- **Never probe subcommands without `--help`/`--version` suffixes.** Bare `subcmd` invocations on unknown CLIs are
  unsafe. `HelpOutput` only spawns `<binary> --help` (with timeout).
- **Windows compat:** all `libc`/SIGPIPE code stays inside `#[cfg(unix)]`. New modules with spawn/runner code must
  compile cleanly on Windows (pre-push hook checks this).
- **No network, no file writes outside tempdir.** Behavioral checks spawn and read stdout/stderr; they do not touch the
  target's working directory.

## Coverage matrix regeneration

After the three checks land and their `covers()` declarations are wired:

```bash
cargo run -- generate coverage-matrix
git add docs/coverage-matrix.md coverage/matrix.json
```

The drift detector in `src/principles/matrix.rs::dangling_cover_ids` runs during the integration test suite — it
fails the build if any ID returned by `covers()` is missing from the registry. You won't add new registry entries (see
§"Check-by-check implementation"), so the drift detector's job here is to catch typos in `covers()` IDs.

Expected matrix diff: `p1-must-no-interactive` gains a second behavioral verifier; `p1-must-env-var` and
`p6-must-no-pager` each gain a behavioral verifier (previously source-only). Coverage summary header
(`docs/coverage-matrix.md` prose) shifts from "19 covered / 27 uncovered" to the same 19 covered count but with three
requirements moving from single-layer to dual-layer — the prose should say something like "19 covered (12 dual-layer
after v0.1.2, up from 9)". Update the prose summary; don't rely on git-cliff to do it.

Bump the counter test `registry_size_matches_spec` only if you add registry entries (you shouldn't — see above).
`level_counts_match_spec` likewise.

## Acceptance criteria (copy from H4 plan DoD)

- [ ] All three checks have full unit-test coverage: happy + Skip + Warn + non-English exception.
- [ ] `HelpOutput` has unit tests for each lazy parser + caching invariant.
- [ ] `anc check ripgrep`, `anc check bird`, `anc check xurl-rs` produce sensible verdicts for the new checks.
- [ ] `docs/coverage-matrix.md` + `coverage/matrix.json` regenerated; committed diff reflects the three new behavioral
  verifiers.
- [ ] Dogfood: `cargo run -- check .` on the agentnative repo itself passes all new checks (or Warn with clear evidence
  if a new check catches a real v0.1.2-era gap in agentnative's own help surface — fix agentnative in the same PR).
- [ ] Pre-push hook passes (fmt, clippy -D warnings, test, cargo-deny, Windows compat).
- [ ] Integration test `test_generate_coverage_matrix_drift_check_passes_on_committed_artifacts` still passes.

## Out of scope (do not do, do not plan for)

- Do not ship `p1-headless-auth-behavioral`, `p5-dry-run-behavioral`, or `p6-timeout-behavioral`. Each was cut during
  eng review with concrete rationale. If any becomes sensible later, it lands as its own plan, not this PR.
- Do not add audience classifier / banner / leaderboard code. That's H5 (v0.1.3).
- Do not bump `schema_version` past `1.1`. The `confidence` field addition is backwards-compatible (additive key in
  `results[]`).
- Do not touch the renamed checks' files (`p1-tty-detection-source`, `p1-env-flags-source`) beyond any `covers()` review
  if genuinely needed. The v0.1.1 rename is settled.

## Files you will touch

Implementation:

- `src/runner/help_probe.rs` — new module.
- `src/runner/mod.rs` — register the new module.
- `src/runner.rs` — thread `Arc<HelpOutput>` into behavioral-check context.
- `src/checks/behavioral/flag_existence.rs` — new check.
- `src/checks/behavioral/env_hints.rs` — new check.
- `src/checks/behavioral/no_pager_behavioral.rs` — new check.
- `src/checks/behavioral/mod.rs` — register the three new checks.
- `src/checks/mod.rs` — any cross-layer registration.
- `src/types.rs` — add `Confidence` enum; extend `CheckResult`.
- `src/scorecard.rs` — serialize `confidence` on each result.

Artifacts (regenerated, don't hand-edit):

- `docs/coverage-matrix.md`
- `coverage/matrix.json`

Prose:

- `docs/coverage-matrix.md` prose header — update the "N covered, M uncovered" summary sentence.

## Files you will NOT touch

- `src/principles/registry.rs` — no new entries (only `covers()` linkage, which lives on each check, not in the registry
  file). Exception: if an eng-review gap surfaces a missing requirement ID, surface it back via a decision request
  before editing.
- `Cargo.toml` version — the release branch bumps this, not your feature branch.
- `CHANGELOG.md` — `generate-changelog.sh` builds this during release prep from the PR body's `## Changelog` section.
  Write a good PR body; don't hand-edit the changelog.
- Anything under `.github/workflows/` — v0.1.1's rename-resilience fixes already landed. H4 is feature code.
- Any `Formula/*.rb` or `homebrew-tap` files. Release pipeline is settled.

## Known gotchas (from prior sessions, absorb once)

- **`cargo pkgid` output format varies** between local (`path+file:///…#version`) and CI (`name@version`). The tolerant
  `sed -E 's|.*[@#]||'` fix landed in `brettdavies/.github` PR #8. You will not interact with this directly — mentioned
  so you recognize it in CI logs if it surfaces.
- **`rustup-target-add` for pinned toolchains** — the reusable release workflow already adds non-cross targets
  explicitly. Not your concern, but visible in CI logs.
- **Fork-bomb when dogfooding** — `arg_required_else_help = true` prevents `anc check .` from recursively invoking
  itself via bare `anc`. Do not remove this guard when extending subcommand detection for `p6-no-pager-behavioral`.

## After you merge

Do not tag `v0.1.2` directly. Follow the release-branch flow documented in the revised H4 plan
(`2026-04-20-v012-handoff-4-behavioral-checks.md` §"After this PR merges"). It covers tag + crates.io + agentnative-site
scorecard regen + coverage-matrix sync.

Open the PR to `dev` with a body including a `## Changelog` section — `generate-changelog.sh` reads it verbatim when the
release-branch cuts.

## Quick-start command sequence

```bash
# 1. Branch off dev
cd /home/brett/dev/agentnative
git checkout dev && git pull
git checkout -b feat/v012-behavioral-check-expansion

# 2. Implement in order (HelpOutput first so checks can consume it)
#    - src/runner/help_probe.rs
#    - src/runner.rs wiring
#    - src/types.rs Confidence enum
#    - src/checks/behavioral/flag_existence.rs
#    - src/checks/behavioral/env_hints.rs
#    - src/checks/behavioral/no_pager_behavioral.rs
#    - mod.rs registrations
#    - src/scorecard.rs confidence serialization

# 3. Unit tests pass locally
cargo test
cargo clippy --all-targets -- -Dwarnings

# 4. Integration smoke
cargo run -- check ripgrep
cargo run -- check bird
cargo run -- check xurl-rs
cargo run -- check .   # dogfood

# 5. Coverage artifact regen
cargo run -- generate coverage-matrix
git add docs/coverage-matrix.md coverage/matrix.json

# 6. Push; pre-push hook runs the full CI-equivalent gate
git push -u origin feat/v012-behavioral-check-expansion

# 7. gh pr create --base dev --title "feat(v0.1.2): behavioral check expansion + HelpOutput cache"
#    with a populated `## Changelog` section in the body
```
