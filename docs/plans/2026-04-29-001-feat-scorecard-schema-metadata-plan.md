---
title: "feat: scorecard schema 0.3 → 0.4 — embed run / tool / anc / target metadata"
type: feat
status: active
date: 2026-04-29
deepened: 2026-04-29
---

# feat: scorecard schema 0.3 → 0.4 — embed run / tool / anc / target metadata

## Overview

Bump the scorecard JSON schema from `0.3` to `0.4` (additive) to embed the contextual metadata a consumer needs to
understand a scorecard in isolation: which tool was scored, at which version, by which `anc` build, on which platform,
when, and with what invocation. The shape changes from "a list of check results" into "a self-describing scoring run
record."

The downstream effect is structural: the website's `registry.yaml` currently carries `version` and `scored_at` per tool
— two facts that are properties of *a particular scoring run*, not of *the tool*. Once those facts are inside the
scorecard, the site's regen pipeline reads them from the JSON instead of the YAML, and the YAML can drop both fields.
`version_extract` shell snippets stay in `registry.yaml` — they remain site-build infrastructure (not anc's concern).

The bump is **additive** within the documented `0.x` "additive fields are the norm" policy. Older consumers
feature-detect and tolerate missing keys; the new fields appear alongside the existing top-level shape without
relocating anything that already exists.

The plan is **Standard** depth — small, contained code change in this repo, but it touches an external contract surface
(JSON consumers in `agentnative-site`) and removes registry fields downstream, so it warrants real test coverage and
explicit cross-repo coordination notes.

---

## Problem Frame

`anc check --output json` today emits a scorecard at `schema_version: "0.3"` with: `results`, `summary`,
`coverage_summary`, `audience`, `audience_reason`, `audit_profile`, `spec_version`. That is enough for a
checks-and-pass-rate view, but a reader of the JSON in isolation cannot answer:

- Which tool did this score? (just `results[].id`s with no target identifier)
- At which version of that tool? (no version field — the site provides this externally from `registry.yaml`)
- Which build of `anc` produced it? (the spec version is captured, but not the CLI version or commit)
- On which platform did the run happen? (no OS/arch — affects which behavioral checks ran and what they observed)
- When was it produced and how long did it take? (no timestamp, no duration)
- What command produced it? (no invocation — meaningful when reproducing or auditing a result)

Because these facts are missing from the scorecard, `agentnative-site/registry.yaml` carries `version: "15.1.0"` and
`scored_at: "2026-04-23"` per scored entry — a parallel source of truth that has to be hand-edited every time a tool is
re-scored. Roughly 10 entries currently carry these fields; every regen requires touching the YAML, not just dropping in
a fresh JSON.

The win is two-sided:

1. **Consumers** (the site, leaderboards, anyone re-rendering scorecards) get a self-describing artifact. No
   cross-reference required to know what was tested.
2. **Producers** (`scripts/regen-scorecards.sh` on the site) stop maintaining `version` and `scored_at` in YAML; the
   regen pipeline writes only fields that aren't derivable from the scorecard.

---

## Requirements Trace

- R1. `anc check --output json` emits `schema_version: "0.4"` with four new top-level objects: `tool`, `anc`, `run`,
  `target`. All other existing top-level fields are preserved unchanged.
- R2. `tool` carries `{ name, binary, version }` describing the scored target as best `anc` can determine. `version` is
  populated from the binary's own `--version` / `-V` / `version` self-report; if none parse cleanly, `version` is `null`
  (the site continues to fall back to `registry.yaml`'s `version_extract` snippet — see R8).
- R3. `anc` carries `{ version, commit }` describing the `anc` build that produced the scorecard. `version` is the crate
  version at compile time. `commit` is the short Git SHA at compile time, or `null` for builds outside a Git checkout
  (e.g., `cargo install agentnative` from crates.io). `spec_version` stays at the top level (no relocation — additive
  policy).
- R4. `run` carries `{ invocation, started_at, duration_ms, platform }`. `invocation` is the full argv as the user typed
  it (joined with spaces, shell-quoted where necessary), captured *before* the `inject_default_subcommand` rewrite so
  the recorded command reflects the user's intent verbatim. `started_at` is RFC 3339 / ISO 8601 in UTC. `duration_ms` is
  wall-clock milliseconds from the start of `Commands::Check` to scorecard emission. `platform` is `{ os, arch }`
  populated from `std::env::consts::{OS, ARCH}`.
- R5. `target` carries `{ kind, path, command }`. `kind` is `"project"`, `"binary"`, or `"command"`. `path` is the
  resolved repo-relative or absolute path when applicable; `null` for `command` mode. `command` is the resolved binary
  name when `--command <NAME>` was used; `null` for project / binary mode.
- R6. The new fields appear regardless of whether `--output json` or `--output text` is selected — `text` mode does not
  render them, but the in-memory `Scorecard` struct always carries them so the JSON path stays a pure formatter.
- R7. `audience: null` semantics from `0.3` are preserved unchanged. `audience_reason` continues to use
  `skip_serializing_if = "Option::is_none"`. The new objects do not interact with audience / audit-profile
  classification.
- R8. `version_extract` shell snippets remain owned by `agentnative-site/registry.yaml`. This plan does not move them
  into `anc`. The site's regen pipeline continues to use `version_extract` as a fallback when `tool.version` in the
  scorecard is `null`.
- R9. The site's removal of `version` and `scored_at` from `registry.yaml` is **out of scope for this plan** — see Scope
  Boundaries below. This plan only commits the producer side; the consumer-side cleanup is tracked separately.

---

## Scope Boundaries

- No changes to checks, principles, registry, or coverage matrix shape.
- No changes to `--output text` rendering. Text output is for humans; metadata noise hurts it.
- No changes to `audit_profile` semantics, `audience` classifier, or exit codes.
- `spec_version` stays at the top level — moving it into `anc.spec_version` would be a *breaking* relocation, which
  contradicts the additive policy.
- `tool.version` is best-effort capture only. This plan does not implement `version_extract`-equivalent shell logic in
  `anc`. Site override snippets stay in `registry.yaml`.

### Deferred to Follow-Up Work

- **Site-side YAML cleanup:** removing `version` and `scored_at` from `registry.yaml` and updating
  `scripts/regen-scorecards.sh` + `docker/score/score-anc100.sh` to read those fields out of the new scorecard. Tracked
  as a parallel plan in `agentnative-site` repo. This plan only ships the producer side.
- **Schema lock to 1.0:** the `0.x` additive convention will eventually freeze. That decision is its own plan, not
  bundled here.
- **`anc.commit` for non-Git builds:** if crates.io publishes carry `null` for `anc.commit`, a follow-up could embed the
  release tag instead. Out of scope until a real consumer needs it.

---

## Context & Research

### Relevant Code and Patterns

- `src/scorecard/mod.rs` — `Scorecard` struct and `SCHEMA_VERSION`. Pattern for additive bumps already exists (the `0.1
  → 0.2 → 0.3` history at the top of the file is the model to follow). Existing `#[serde(skip_serializing_if =
  "Option::is_none")]` usage on `audience_reason` is the precedent for optional sub-fields.
- `src/principles/registry.rs` — `SPEC_VERSION` is generated by `build.rs` from vendored `src/principles/spec/VERSION`.
  The same `build.rs` pattern is the right place to emit `ANC_VERSION` (from `CARGO_PKG_VERSION`) and `ANC_COMMIT` (from
  `git rev-parse --short HEAD`, with graceful fallback to `None`).
- `src/argv.rs` — `inject_default_subcommand` is where `anc .` becomes `anc check .`. The `run.invocation` field MUST
  capture argv *before* this rewrite, so it reflects what the user actually typed. `main.rs` is the only caller of
  `inject_default_subcommand`, so the capture point is well-defined.
- `src/cli.rs` — `Commands::Check { path, command, ... }` carries the resolved fields needed to populate `target`.
- `src/main.rs` — the entry point that owns the `Commands::Check` arm and currently calls `format_json`. The new
  metadata is captured here and threaded into `build_scorecard`.

### Institutional Learnings

- The 0.1.3 release note in `src/scorecard/mod.rs` documents the precedent: pre-launch additive bumps populate new
  fields with `null` when the data isn't yet available, then turn them on in a later patch release. Consumers
  feature-detect.
- `docs/solutions/` (symlinked to `~/dev/solutions-docs`) — search at execution time for `agent-native-cli` /
  `scorecard` / `additive schema` for any prior decisions about JSON shape or version-capture trade-offs. None known to
  conflict at planning time.

### External References

- `std::env::consts::{OS, ARCH}` — stable since 1.0, returns rustc-known platform tuples (e.g. `linux` / `x86_64`,
  `macos` / `aarch64`). Sufficient for `run.platform`.
- Neither `chrono` nor `time` is currently in this crate's dependency tree (verified against `Cargo.lock`). The
  timestamp dep is a real net-new addition; `time` is the smaller pick (no chrono compatibility bridge needed) but must
  be added explicitly with the repo's pre-1.0 `=X.Y.Z` pinning convention and added to `cargo deny` allowlist. Decision
  deferred to U4.

---

## Key Technical Decisions

- **Additive 0.3 → 0.4, not breaking 1.0.** Matches the documented pre-launch policy. Locking to 1.0 is its own future
  plan.
- **Top-level objects, not flat fields.** `tool.version` reads better than `tool_version` and groups related facts. The
  serialization cost is identical.
- **Capture invocation pre-injection.** The recorded command reflects user intent. A user who typed `anc .` should see
  `"anc ."` in the scorecard, not `"anc check ."` — the latter is a fact about anc's internals, not the user's command.
- **`anc.commit` is best-effort, never an error.** A missing Git directory at build time emits a warning and produces
  `commit: null`. Consistent with how `spec_version` handles a missing `VERSION` file (`"unknown"` fallback).
- **Re-emit `tool.version` even when `null`.** Always-present keys make consumer code simpler than conditional fields.
  `null` is the unambiguous "not captured" signal; `version_extract` on the site fills the gap.

---

## Open Questions

### Resolved During Planning

- **Schema bump strategy** → Additive `0.3 → 0.4`. Future `1.0` lock is a separate plan.
- **Where does `version_extract` live?** → Stays in `registry.yaml` on the site. anc's `tool.version` is best-effort
  self-report only.
- **Should `spec_version` move under `anc.{}`?** → No. Top-level location preserved for non-breaking add. A future major
  bump may relocate.

### Deferred to Implementation

- **Timestamp library choice.** Neither `chrono` nor `time` is in `Cargo.lock` today — both are net-new direct deps.
  `time` is the lighter pick (~3 transitive crates vs `chrono`'s `serde` feature drag). Decision settled in U4: add
  `time = "=X.Y.Z"` per the repo's pre-1.0 pinning convention, document in `cargo deny` allowlist, produce RFC 3339 in a
  1-line call.
- **`run.duration_ms` start point** — fully resolved by R4 (start of `Commands::Check` arm to scorecard emission). No
  further deferral needed.
- **Quoting in `run.invocation`.** `shell-words` is also not in `Cargo.lock`. Hand-roll the `OsStr → String` join with
  `'\''` escape for whitespace and metacharacter-bearing args (~30 LOC) — avoids the dep entirely. Lives in
  `src/argv.rs` per U3.

---

## Implementation Units

- U1. **Build-time `ANC_VERSION` / `ANC_COMMIT` constants**

**Goal:** Make `anc`'s own version and commit SHA available at runtime as `&'static str` constants generated by
`build.rs`, mirroring the existing `SPEC_VERSION` pattern.

**Requirements:** R3.

**Dependencies:** None.

**Files:**

- Modify: `build.rs`
- Modify: `src/main.rs` (or a new `src/build_info.rs`) to expose the generated constants
- Test: `tests/build_info.rs` (smoke test that constants compile and are non-empty for `ANC_VERSION`)

**Approach:**

- In `build.rs`, read `CARGO_PKG_VERSION` (always present) and shell out to `git rev-parse --short HEAD` with graceful
  fallback to `"unknown"` (or emit `None` as `Option<&'static str>` — implementer's call).
- Emit a generated file in `OUT_DIR` (e.g., `build_info.rs`) consumed via `include!()` from a `build_info.rs` module
  alongside `main.rs`.
- Match the spec-vendor warn-don't-fail behavior: a missing `.git` does not fail the build.
- **Critical: emit the right `cargo:rerun-if-changed` directives** so `ANC_COMMIT` doesn't go stale across local
  commits. The current `build.rs` only declares `cargo:rerun-if-changed=src/principles/spec/`. This unit must add:
- `cargo:rerun-if-changed=.git/HEAD` — covers branch switches and direct HEAD updates
- `cargo:rerun-if-changed=.git/refs/heads/<current-branch>` — covers commits on the current branch (resolve the branch
  name by reading `.git/HEAD` first; if HEAD is detached, the symbolic ref read is the SHA itself and the refs/heads
  watch is unnecessary)
- `cargo:rerun-if-changed=.git/packed-refs` — covers packed-ref repos where the branch ref isn't a loose file Without
  all three, cached builds will silently embed a stale SHA and every dev-build scorecard will advertise a wrong
  `anc.commit`. Skip the directives only when `.git` is absent (release-from-tarball case).

**Patterns to follow:**

- `build.rs` already does this for the spec — mirror its structure for the new constants.

**Test scenarios:**

- Happy path: `ANC_VERSION` equals `env!("CARGO_PKG_VERSION")` at runtime.
- Edge case: build without `.git` directory still produces a usable `ANC_COMMIT` value (either `"unknown"` or `None`,
  whichever the design picks). Use a `tempdir` test or a `cfg`-gated assertion.
- Edge case: a commit made between two `cargo build` invocations updates `ANC_COMMIT` (smoke-test via a tempdir Git
  repo: build, commit, build, assert SHA changed). Catches missing `rerun-if-changed` directives.

**Verification:**

- `cargo build` from a non-Git directory completes without error and emits a build-script warning naming the missing Git
  checkout.
- `anc --version` continues to work unchanged (clap consumes `CARGO_PKG_VERSION` separately — no regression).

---

- U2. **`Scorecard` schema additions**

**Goal:** Define the four new top-level objects as serde-derived structs, wire them into `Scorecard`, and bump
`SCHEMA_VERSION` to `"0.4"`.

**Requirements:** R1, R2, R3, R4, R5, R7.

**Dependencies:** U1 (`ANC_VERSION` / `ANC_COMMIT` constants must exist).

**Files:**

- Modify: `src/scorecard/mod.rs`
- Test: `src/scorecard/mod.rs` (extend the existing `mod tests` block — keep tests co-located with the module per the
  existing convention)

**Approach:**

- Add four `#[derive(Serialize)]` structs: `ToolInfo`, `AncInfo`, `RunInfo`, `TargetInfo`.
- Add four fields to `Scorecard`: `pub tool: ToolInfo`, `pub anc: AncInfo`, `pub run: RunInfo`, `pub target:
  TargetInfo`.
- For each `Option<String>` sub-field that should appear as `null` rather than be omitted (e.g., `tool.version`,
  `target.path`, `target.command`), do **not** apply `skip_serializing_if`. Always-present keys simplify consumer code.
- Update `SCHEMA_VERSION` to `"0.4"` and extend the doc-comment history line.
- Update `format_json` and `build_scorecard` signatures to accept the new metadata as parameters (don't fabricate it
  inside the scorecard module — the runner owns the capture).

**Patterns to follow:**

- Existing `Summary`, `CoverageSummary`, `LevelCounts` derive-Serialize structs.
- Existing `audience` doc-comment block as the model for "this field is best-effort, may be `null`."

**Test scenarios:**

- Happy path: `format_json` with synthetic metadata emits all four objects with expected keys and values.
- Edge case: `tool.version: null`, `target.path: null`, `target.command: null` all serialize as JSON `null`, not as
  missing keys.
- Edge case: `audience_reason` still respects `skip_serializing_if` — adding new fields did not change its behavior.
- `schema_version` test pins to `"0.4"`.
- All existing tests pass unchanged after the field additions (no breaking relocation).

**Verification:**

- `cargo test -p agentnative` is green.
- A manual `anc check . --output json | jaq .` shows the four new objects in the expected shape.

---

- U3. **Capture `run.invocation` pre-injection in `main.rs`**

**Goal:** Snapshot the raw argv before `inject_default_subcommand` rewrites it, so `run.invocation` reflects what the
user actually typed.

**Requirements:** R4.

**Dependencies:** None (parallel-safe with U1, U2).

**Files:**

- Modify: `src/main.rs`
- Modify: `src/argv.rs` (only if a small `quote_arg(&OsStr) -> String` helper makes sense alongside the injection logic
  — implementer's call)
- Test: `src/argv.rs` (extend tests with a `format_invocation` round-trip)

**Approach:**

- In `main.rs`, capture `std::env::args_os().collect::<Vec<_>>()` *before* the `inject_default_subcommand` call.
- Convert each arg to a displayable form: lossy-UTF-8 conversion is fine; quote with single quotes when the arg contains
  whitespace, `'`, `"`, or shell metacharacters; escape embedded single quotes as `'\''`.
- Hand-rolled is fine — a 30-line helper avoids pulling in `shell-words` or `shellish`. If the implementer prefers a
  crate, justify in the PR.
- Pass the formatted invocation through to `build_scorecard` via the `Commands::Check` arm.

**Patterns to follow:**

- The existing `argv` module's unit-test style (table-driven `&[(input, expected)]`).

**Test scenarios:**

- Happy path: `["anc", "check", "."]` → `"anc check ."`.
- Edge case: arg with spaces — `["anc", "check", "/tmp/with space/repo"]` → `"anc check '/tmp/with space/repo'"`.
- Edge case: arg with single quote — `["anc", "check", "ab'cd"]` → `"anc check 'ab'\\''cd'"`.
- Edge case: invalid UTF-8 in argv (Linux only) — round-trips via lossy conversion without panicking; the field carries
  the lossy form.

**Verification:**

- A run of `anc .` (default-subcommand injection path) records `run.invocation: "anc ."`, **not** `"anc check ."`.

---

- U4. **Capture `run.platform`, `run.started_at`, `run.duration_ms` in the runner**

**Goal:** Time the run and stamp it with platform info.

**Requirements:** R4.

**Dependencies:** U2 (the struct must exist before it can be filled).

**Files:**

- Modify: `src/main.rs` (the `Commands::Check` arm)
- Modify: `Cargo.toml` (add `chrono` or `time` if not already present — see Open Questions)
- Test: integration test (new file under `tests/` or extend an existing one) that runs the binary end-to-end and parses
  the emitted JSON

**Approach:**

- Capture `std::time::Instant::now()` at the start of `Commands::Check`.
- Capture `started_at` as RFC 3339 / ISO 8601 in UTC. Use whichever timestamp library the dependency tree already
  resolves; add at most one new dep.
- Compute `duration_ms` as `Instant::now().duration_since(start).as_millis() as u64` immediately before scorecard
  serialization.
- `platform.os` = `std::env::consts::OS`, `platform.arch` = `std::env::consts::ARCH`.

**Patterns to follow:**

- No existing precedent for run timing in this repo. Pick the smallest dep that produces RFC 3339; pin per the repo's
  exact-version convention for pre-1.0 deps.

**Test scenarios:**

- Happy path: `run.platform.os` matches the test runner's `OS`; `run.platform.arch` matches its `ARCH`.
- Happy path: `run.started_at` parses as RFC 3339 / ISO 8601.
- Happy path: `run.duration_ms` is a non-negative integer; for a near-empty project under test, it's bounded above
  (e.g., `< 60_000`).
- Edge case: a run that produces no checks still emits `started_at` and `duration_ms` (don't gate on `results`).

**Verification:**

- `cargo test` covers the new fields. A manual run on a real project shows a sub-second `duration_ms` and a current
  `started_at`.

---

- U5. **Populate `tool` and `target` from the resolved `Commands::Check` args**

**Goal:** Translate the three `anc check` modes (project / binary / command) into structured `target` + `tool` metadata,
including a best-effort `tool.version` self-report.

**Requirements:** R2, R5.

**Dependencies:** U2.

**Files:**

- Modify: `src/main.rs` (the `Commands::Check` arm)
- Modify: `src/runner/mod.rs` if the resolution helpers need surfacing — but prefer to keep the metadata capture in
  `main.rs` to avoid changing runner signatures
- Test: `tests/scorecard_metadata.rs` (new) — integration test exercising the three modes

**Approach:**

- For each `Commands::Check` arm:
- **`path`** (project mode): `target.kind = "project"`, `target.path = path.to_string_lossy().into_owned()`,
  `target.command = null`. `tool.name` = basename of the project directory (deterministic, never null); `tool.binary` =
  `tool.name` when a built binary exists at `<path>/target/release/<name>` or `<path>/target/debug/<name>`, else `null`;
  `tool.version` = `Cargo.toml`/`pyproject.toml` version field if parseable, else fall through to a binary `--version`
  probe when `tool.binary` is `Some`, else `null`. **No-binary case (fresh clone with no build artifact):**
  `tool.binary: null` and `tool.version` from manifest only — the four fields are always emitted, only `binary` and
  `version` may be null.
- **`binary`** (binary mode, when `path` resolves to a file): `target.kind = "binary"`, `target.path = resolved_path`,
  `target.command = null`. `tool.name` = filename stem, `tool.binary` = filename. `tool.version` = best-effort `<binary>
  --version` first line trimmed; if nonzero exit, try `-V`; if still nonzero, `null`.
- **`command`** (`--command <NAME>`): `target.kind = "command"`, `target.path = null`, `target.command = name`.
  `tool.name` = `name`, `tool.binary` = `name`. `tool.version` same fallback chain as binary mode. **Path resolution
  reuses the existing helper:** `src/main.rs` already has `resolve_command_on_path` that shells out to system
  `which`/`where` — call that, do not introduce a new `which` crate dep.
- The version-probe is best-effort and **never** fails the run. Any error → `tool.version = null`.
- **Reuse the existing condvar-based timeout primitive at `src/runner/mod.rs::spawn_and_wait`** rather than a fresh
  `Command::new(...).output()`. That primitive already (a) kills the child on timeout (no orphan probes from a TUI
  binary that intercepts stdin), (b) caps stdout at the existing 1MB ceiling so a pathological `--version` cannot
  exhaust memory, (c) handles cross-platform process-group cleanup. Use a 2-second timeout. Do **not** reach for the
  `wait-timeout` crate — it is only in the dev-dependency tree (transitively via `assert_cmd`) and is not linkable from
  the binary target.
- **Self-spawn guard:** before invoking the version probe, compare the resolved target path to
  `std::env::current_exe()`. If they match, skip the probe and emit `tool.version: null` with evidence `"self-spawn
  declined"`. Defense in depth against the recursive-fork-bomb hazard already covered by `arg_required_else_help` in
  `Cli`.

**Patterns to follow:**

- `src/runner/help_probe.rs` — existing pattern for spawning the target with `--help` for behavioral checks. The
  `--version` probe should mirror its safety conventions (no bare invocation, suffix-only).
- `src/runner/mod.rs::spawn_and_wait` — the timeout + output-cap primitive to reuse, not duplicate.

**Test scenarios:**

- Happy path: `anc check --command echo` produces `target.kind = "command"`, `target.command = "echo"`, `tool.name =
  "echo"`, `tool.version` = whatever `echo --version` first line trims to (system-dependent — assert shape, not exact
  text).
- Happy path: `anc check ./fixture-project` produces `target.kind = "project"`, `target.path` ends with
  `fixture-project`.
- Edge case: binary that exits nonzero on `--version` and `-V` → `tool.version: null`, run succeeds.
- Edge case: command not on PATH → `target.kind = "command"`, `tool.version: null`, run succeeds with the existing
  command-resolution error path unchanged.
- Edge case: project mode with no built binary (`target/` absent) → `tool.binary: null`, `tool.version` from manifest if
  present else `null`, run succeeds.
- Edge case (security): `anc check --command anc` (or `--command <path-to-current-anc-exe>`) → self-spawn guard
  triggers, `tool.version: null` with evidence `"self-spawn declined"`, no recursion, run succeeds.
- Edge case (security): hostile binary fixture that emits >1MB on `--version` stdout → output truncated at the runner's
  1MB cap, run succeeds, no memory exhaustion.
- Edge case (security): hostile binary fixture that hangs on `--version` (sleep 30s) → killed at 2s timeout,
  `tool.version: null`, run succeeds.
- Integration: `anc check . --output json | jaq '.target.kind'` returns `"project"` for the project's own root.

**Verification:**

- `cargo test` is green. Manual end-to-end: `anc check --command rg --output json | jaq .tool.version` returns a
  SemVer-shaped string for ripgrep.

---

- U6. **Schema test: drift guard + golden snippet**

**Goal:** Pin `0.4` schema shape against accidental regression.

**Requirements:** R1, R6, R7.

**Dependencies:** U2, U3, U4, U5.

**Files:**

- Modify: `src/scorecard/mod.rs` (extend tests)
- Test: `tests/scorecard_schema_v04.rs` (new integration test using a stub-results fixture and `assert_eq!` on key
  presence — not a string-match snapshot)

**Approach:**

- Add a unit test in `src/scorecard/mod.rs` that builds a `Scorecard` with synthetic data, calls `format_json`, parses
  the result with `serde_json::Value`, and asserts every documented key is present at the expected path.
- The test pins exact key names (`tool.name`, `tool.binary`, `tool.version`, `anc.version`, `anc.commit`,
  `run.invocation`, `run.started_at`, `run.duration_ms`, `run.platform.os`, `run.platform.arch`, `target.kind`,
  `target.path`, `target.command`).
- Use `parsed.get("...").is_some()` to assert key presence (not absence), so `null` values still pass — `null` keys are
  part of the contract.
- Add a regression test that confirms the existing `0.3` fields are still emitted (defends against an accidental removal
  during refactor).

**Patterns to follow:**

- Existing `test_format_json_valid` and `scorecard_level_enum_values_are_kebab_case` tests in `src/scorecard/mod.rs` —
  same shape: build → serialize → assert on parsed JSON.

**Test scenarios:**

- Happy path: every documented `0.4` key path resolves on a synthetic Scorecard.
- Regression: every documented `0.3` key path also resolves (no accidental field deletions).
- Negative: a hand-edit that drops a field (e.g., commenting out `pub run: RunInfo`) fails this test loudly with a
  named-field assertion.

**Verification:**

- `cargo test` is green.
- Inspecting the test failure for a manually broken field gives an actionable message (e.g., `assertion failed: tool
  field present`).

---

## System-Wide Impact

- **Interaction graph:** The metadata flows from `main.rs` (capture point) → `build_scorecard` (struct assembly) →
  `format_json` (serialize). No checks, registry, or coverage code is touched.
- **Error propagation:** Best-effort fields (`tool.version`, `anc.commit`) MUST never fail the run. Any error during
  capture is logged at `--quiet=false` and the field becomes `null`. The exit code is computed only from
  `results[].status`; new metadata cannot influence it.
- **State lifecycle risks:** None — the scorecard is a snapshot, no persisted state.
- **API surface parity:** The `--output text` renderer does not display the new fields. Any future TUI / dashboard
  consumer reading the JSON gets them automatically.
- **Integration coverage:** U6's integration test exercises the three `anc check` modes end-to-end. Unit-only coverage
  cannot prove that argv capture actually flows through `inject_default_subcommand` correctly — U3's test must be a real
  spawn with `Command::new(env!("CARGO_BIN_EXE_anc"))`.
- **Unchanged invariants:** `schema_version` remains a `&'static str`, additive policy unchanged. `audience_reason`
  still skips serialization when `None`. `audit_profile` still echoes the kebab-case CLI flag value. The existing `0.x →
  1.0` lock decision is deferred, not pre-empted.

---

## Risks & Dependencies

| Risk                                                                                                                          | Mitigation                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| ----------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `tool.version` capture invokes the target binary, which is a recursive-fork-bomb hazard if the target is `anc` itself.        | Two layers: (a) suffix-only invocation (`<bin> --version` / `<bin> -V`), never bare — the existing `arg_required_else_help` guard on `Cli` enforces this; (b) explicit self-spawn check in U5 that compares the resolved target path to `std::env::current_exe()` and skips the probe on match. U5 test scenarios MUST include `anc check --command anc` asserting no recursion and `tool.version: null` with the self-spawn evidence string. |
| New `time` dependency increases compile time and supply-chain surface.                                                        | Single direct dep, audited at U4. Pinned exact-version (pre-1.0 convention), added to `cargo deny` allowlist with rationale. `chrono` rejected (heavier transitive footprint).                                                                                                                                                                                                                                                                |
| Site consumers parsing the JSON break on the new top-level objects.                                                           | Pre-launch policy: consumers feature-detect. Coordinate with `agentnative-site` repo maintainers — open a tracking issue when this PR lands so the deferred YAML cleanup is visible.                                                                                                                                                                                                                                                          |
| `run.invocation` accidentally captures a path containing PII or a secret (e.g., `--command /home/me/secret-tool`).            | Best-effort lossy joining only. Document in the changelog that scorecards should be reviewed before publication, same as existing scorecards. No silent redaction (would surprise users debugging their own scorecards).                                                                                                                                                                                                                      |
| `target.path` discloses absolute filesystem path including username and directory layout when published.                      | Same review-before-publish posture as `run.invocation`. Implementation MAY relativize to current working directory when the target is inside CWD (cheap, reduces accidental leak); MUST NOT silently strip the home prefix from absolute paths outside CWD (would lose information needed for debugging). Document in the README's "publishing scorecards" note alongside `run.invocation`.                                                   |
| `tool.version` probe spawns a child for every scorecard — pathological/hostile binaries can hang, daemonize, or flood stdout. | Reuses `src/runner/mod.rs::spawn_and_wait` (kill-on-drop, 1MB stdout cap, 2-second timeout). Do not write a fresh `Command::new(...).output()`. U5 includes a regression test against a hostile-binary fixture that emits >1MB on stdout — assert truncation, not memory exhaustion.                                                                                                                                                          |
| `anc.commit` capture in CI builds picks up the wrong commit (e.g., a merge SHA).                                              | `git rev-parse --short HEAD` returns whatever the build's HEAD points at. CI configurations that detach HEAD intentionally should accept the resulting SHA; documented in the build-script comment.                                                                                                                                                                                                                                           |
| `anc.commit` short SHA (7 chars) collides under high commit volume; consumers may treat it as provenance assertion.           | Document in the README that `anc.commit` is informational, not a signed provenance signal. If consumers need provenance, pair with a Sigstore-signed release artifact. Implementer MAY emit the full 40-char SHA; the field is `String`, not `&str`-sized.                                                                                                                                                                                    |
| Stale embedded `ANC_COMMIT` due to missing `cargo:rerun-if-changed` directives.                                               | U1 explicitly enumerates `.git/HEAD`, `.git/refs/heads/<branch>`, and `.git/packed-refs` as rerun triggers. Smoke test in U1 verifies `ANC_COMMIT` updates after a fresh commit in a tempdir Git repo.                                                                                                                                                                                                                                        |

---

## Documentation / Operational Notes

- Update `README.md` "Output" section to document the new top-level scorecard objects with a small example.
- Update `CLAUDE.md` "Scorecard v1.1 Fields" section (currently describes `0.3`) — extend with a `0.4` block, or retitle
  to "Scorecard v0.4 Fields" and consolidate.
- The `agentnative-site` README / CLAUDE.md reference to `version_extract` policy should be left alone — site still owns
  those snippets per R8.
- A coordinated PR description should call out the deferred site-side YAML cleanup so the receiving maintainer can open
  the follow-up issue.

---

## Sources & References

- Existing scorecard module: `src/scorecard/mod.rs`
- Existing argv injection: `src/argv.rs`
- Existing build-time generation pattern: `build.rs` + `src/principles/registry.rs` `SPEC_VERSION` precedent
- Existing site contract: `agentnative-site/registry.yaml` (entries with `version` / `scored_at` / `version_extract`)
- Site regen pipeline: `agentnative-site/scripts/regen-scorecards.sh`, `agentnative-site/docker/score/score-anc100.sh`,
  `agentnative-site/docker/score/audit-version-extract.sh`
- Sibling plan (skill subcommand): `docs/plans/2026-04-29-002-feat-skill-subcommand-plan.md`

---

## Document Review (2026-04-29)

Reviewed via `/ce-doc-review` (coherence, feasibility, scope-guardian, security-lens, adversarial). Applied in-place
fixes for high-confidence factual errors. Below are findings considered but not absorbed — recorded so the implementer
(and a future deepener) can reopen them deliberately.

**Applied:**

- Dep claims corrected: `time`, `chrono`, `wait_timeout`, `shell-words`, `which` are NOT in `Cargo.lock`. U5 reuses
  `src/main.rs::resolve_command_on_path` (existing helper) and `src/runner/mod.rs::spawn_and_wait` (existing timeout +
  1MB cap primitive) instead of pulling new deps. `time` is the only net-new direct dep, added with exact-version pin.
- U1 augmented with `cargo:rerun-if-changed` directives for `.git/HEAD`, `.git/refs/heads/<branch>`, `.git/packed-refs`
  — avoids stale `ANC_COMMIT` in cached builds.
- U5 augmented with explicit self-spawn guard (`std::env::current_exe()` comparison) and a hostile-binary regression
  test for the >1MB stdout case.
- Risks table extended: `target.path` PII row, hostile-binary spawn row, short-SHA collision row, stale-rerun row.
- R4 vs U4 timing ambiguity resolved: R4 is authoritative; U4 deferral removed.
- Project-mode `tool.version` clarified for the no-binary case (manifest-only, `tool.binary: null`).
- `tools/version_extract` ownership clarified: stays in the site's `registry.yaml` (R8 unchanged).

**Deferred (worth revisiting before implementation):**

- **Scorecard "self-describing" framing.** Adversarial review noted that `tool.version` being best-effort + site
  retaining `version_extract` as authoritative undermines the framing that the scorecard is fully self-describing.
  Tempered language elsewhere implicitly accepts the trade-off; a future plan that actually moves version-extraction
  into anc could revisit.
- **`spec_version` top-level vs `anc.spec_version`.** Plan keeps `spec_version` at top level for non-breaking add.
  Adversarial review proposed dual-emit (top-level deprecated + `anc.spec_version` canonical) for a graceful relocation
  path. Punt to the future `1.0` lock plan rather than fold in mid-flight.
- **Always-emit-null-keys vs `skip_serializing_if`.** Plan re-emits `tool.version`, `target.path`, `target.command` even
  when null, but `audience_reason` keeps `skip_serializing_if`. Convention is mixed by design (the `audience_reason`
  field has different semantics). Documented inconsistency, not a bug.
- **`run.invocation` reproducibility vs intent.** Plan captures pre-injection argv (user intent). A reproducer needs the
  post-injection form too. If a real consumer surfaces, add `run.resolved_invocation` as an additive field — not in this
  plan.
- **U6 dissolution into U2/U5.** Scope-guardian proposed folding the schema-pinning unit into the units that introduce
  the fields. Defensible, but U6's integration test (separate file under `tests/`) is genuinely separate from U2's unit
  tests; keep the split.
- **Cross-repo coordination concretization.** Site-side YAML cleanup is referenced as a parallel plan but with no named
  filename or issue number. Resolve at PR-merge time by opening the tracking issue and updating the Sources & References
  block.
