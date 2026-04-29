---
title: "feat: `anc skill` subcommand — install the agent-native-cli skill into a host"
type: feat
status: active
date: 2026-04-29
deepened: 2026-04-29
---

# feat: `anc skill` subcommand — install the agent-native-cli skill into a host

## Overview

Add an `anc skill` subcommand whose primary verb is `install`, taking a host name (`claude_code`, `codex`, `cursor`,
`opencode`, …) and writing the `agentnative-skill` bundle into the host's canonical skills directory. `anc` becomes a
single-binary front door for the entire CLI/skill ecosystem: install the binary, run `anc skill install <host>`, and the
host's agent picks the skill up on next reload. Host names match the `install` keys in
[`anc.dev/skill.json`](https://anc.dev/skill.json) verbatim — note `claude_code` (with underscore), not `claude`.

The mechanism is deliberately thin. `agentnative-site` already publishes `https://anc.dev/skill.json` containing the
canonical per-host install command (`git clone --depth 1 https://github.com/brettdavies/agentnative-skill.git
~/.claude/skills/agent-native-cli` and equivalents). The skill repo (`agentnative-skill`) already implements an
update-check via `bin/check-update` that compares the local `VERSION` file against the latest release tag on
`brettdavies/agentnative-skill`. There is **no SHA pinning** — the producer side iterates faster than `anc` releases,
and the user-facing freshness signal is the release-tag check baked into the skill itself.

That gives the design an easy shape:

- Embed a snapshot of `skill.json`'s host map at compile time so `anc skill install <host>` is offline-capable.
- Add a `--refresh` flag that fetches the live `https://anc.dev/skill.json` and uses that map instead. Use this when the
  embedded copy is stale.
- Add a `--path <DIR>` override for users who want the bundle in a non-canonical location (or for testing).
- Validate every install command against a defense-in-depth allowlist (`git clone --depth 1
  https://github.com/brettdavies/agentnative-skill[.git]`) before executing it. The skill.json contract already
  guarantees this shape, but `anc` runs the command — it is the trust boundary.

The plan is **Standard** depth — one new subcommand, one network surface, an embedded resource, and a process spawn,
with explicit security framing. ~6 implementation units.

---

## Problem Frame

A user installing `anc` today has three artifacts to think about:

1. The `anc` binary (this repo).
2. The `agentnative-skill` bundle (separate repo, distributed via `git clone`).
3. The host (Claude Code, Codex, Cursor, OpenCode) that consumes the bundle.

The skill bundle is `git clone`-installed at one of N host-specific paths. `anc.dev/skill` already publishes those
commands as documentation. But a user who just `brew install`-ed `anc` still has to:

- Open a browser, find the right command for their host, copy-paste a `git clone` line, and run it.
- Remember to update by `cd`-ing into the install dir and running `git pull`.
- Repeat once per host they use.

That is friction precisely where `anc` should be friction-free. `anc skill install claude_code` should be a single
command that does the right thing — and `anc skill install --refresh codex` should use the latest published map without
requiring a new `anc` release.

The problem is *not*:

- Embedding the skill content itself in the binary. The skill repo ships independently and updates faster than `anc`.
  Embedding the bundle would create exactly the drift the user flagged. Distribution stays via `git clone`.
- Reinventing the update mechanism. `agentnative-skill/bin/check-update` already handles "is there a newer version?" via
  release-tag comparison. `anc skill` does not duplicate that logic.

---

## Requirements Trace

- R1. `anc skill` is a new top-level subcommand. Its only initial verb is `install <host>`. Future verbs (`list`,
  `path`, `update`) are out of scope.
- R2. `install` accepts `<host>` as a positional argument. The host name matches the `install` keys in
  `https://anc.dev/skill.json` (e.g., `claude_code`, `codex`, `cursor`, `opencode`). Unknown hosts produce an actionable
  error listing the available hosts.
- R3. `install --refresh` fetches `https://anc.dev/skill.json` over HTTPS, parses it, and uses its `install` map for
  this run. Without `--refresh`, the embedded snapshot baked in at build time is used. Embedded is the default —
  zero-network paths must work.
- R4. `install --path <DIR>` overrides the default destination encoded in the resolved install command. The override
  replaces the trailing destination path (last token of the `git clone` command) with `<DIR>`. Existing safety checks
  (R6) still apply.
- R5. `install --print` prints the resolved install command to stdout and exits 0 without executing it. Lets users
  review what `anc` would run, scriptable via `$(anc skill install --print claude_code)`. **When combined with
  `--refresh` and the network fetch fails, `--print` emits a hard error** (not a stderr-warned fallback) so scripted
  callers cannot silently capture a stale embedded command — see R8.
- R6. `anc` only executes commands that match the defense-in-depth allowlist. **The validator tokenizes on whitespace
  and asserts an exact 6-token shape** — any deviation is a hard rejection:
- Token 0 MUST be `git`.
- Token 1 MUST be `clone`.
- Token 2 MUST be `--depth`.
- Token 3 MUST be `1`.
- Token 4 (URL) MUST equal exactly `https://github.com/brettdavies/agentnative-skill.git` or
  `https://github.com/brettdavies/agentnative-skill` — both forms accepted, no other variants. (No `.git` suffix
  preference; either is allowed once.)
- Token 5 (destination) MUST be a path expression (not flag-shaped — does NOT start with `-`).
- Token count MUST be exactly 6 — no extra tokens accepted between or after the validated positions. This eliminates the
  `--config <key>=<value>` injection class entirely (e.g., `--config core.askPass=/tmp/evil`, `--config
  core.sshCommand=...`, `--config url.<base>.insteadOf=...`) by structural rejection rather than blocklist enumeration.
- Any deviation produces an error with the offending token position and value highlighted; the command is not executed.
- R6a. **Tilde expansion happens before validation, not after.** `skill.json` ships literal `~`-prefixed destinations
  (e.g., `~/.claude/skills/agent-native-cli`). `Command::new("git").arg(...)` does NOT invoke a shell, so the OS does
  NOT expand `~`. `anc` MUST expand a leading `~` or `~/` to `$HOME` (via `home::home_dir()` or equivalent) before the
  destination is validated and before it is passed to git. Destinations that do not start with `~`, `~/`, or `/` (i.e.,
  relative paths and `~user/...` user-home references) are rejected — this plan does not support them. The expanded path
  is then used as the canonical destination string for both validation and execution.
- R6b. **Destination canonicalization MUST resolve symlinks before the `$HOME` policy check.** The validator computes
  `canonicalize(parent(destination))` and asserts the canonicalized parent is under `canonicalize($HOME)`. Resolving
  prevents a pre-placed symlink at the destination from redirecting `git clone` to a sensitive system path. When the
  destination's parent does not exist yet, walk up to the nearest existing ancestor and canonicalize that. Acknowledge a
  residual TOCTOU window between validation and `git clone` exec — acceptable for the threat model (single-user
  machine).
- R6c. **The `git clone` invocation MUST run with a sanitized environment and explicit config flags** to defeat ambient
  git config and env-var subversion:
- Pass `-c credential.helper=` (empty) to disable any credential helper — the target is a public repo, no credentials
  are required.
- Pass `-c core.askPass=` (empty) to disable interactive credential prompts.
- Pass `-c protocol.allow=https-only` to refuse any non-HTTPS protocol redirect (covers `url.<base>.insteadOf` rewrites
  that would replace the allowlisted URL with `git://` or `ssh://`).
- Pass `-c http.followRedirects=false` to refuse cross-host redirects.
- Pass an explicit `-c url.https://github.com/brettdavies/agentnative-skill.insteadOf=` (empty) to block any `insteadOf`
  rule against the allowlisted URL.
- Unset `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_SYSTEM`, `GIT_SSH`, `GIT_SSH_COMMAND`, `GIT_PROXY_COMMAND`, `GIT_ASKPASS`,
  `GIT_TERMINAL_PROMPT`, `GIT_EXEC_PATH` for the spawned process. Use `Command::env_remove(...)` per variable; do NOT
  call `env_clear()` (that strips PATH and breaks `git`'s ability to find its own helpers).
- These hardenings apply uniformly to the default install path, the `--path` override path, and the
  `--print`-then-execute round-trip.
- R7. The embedded `skill.json` snapshot is generated at build time from a vendored copy of `skill.json` in this repo
  (see U1 — vendoring not network-fetching). The build verifies the vendored snapshot still parses against the declared
  schema; a malformed snapshot fails the build.
- R8. The `--refresh` HTTPS fetch has a 5-second connect timeout and a 10-second total timeout, **with TLS certificate
  validation REQUIRED and not configurable** (no `danger_accept_invalid_certs`, no custom trust-store override). Body
  size is capped at 64 KiB to prevent memory exhaustion on a malicious server. Failure (network error, non-200 response,
  TLS error, parse error, body-too-large) reports the failure clearly. Without `--print`: fall back to the embedded
  snapshot with an `eprintln!` warning naming the fallback. **With `--print`: hard-error and exit non-zero** so `$(anc
  skill install --print --refresh ...)` cannot quietly capture stale embedded output. Acknowledge in the risks table
  that DNS hijack of `anc.dev` remains a residual risk that TLS alone does not mitigate; an attacker who controls a
  CA-trusted certificate for `anc.dev` could MITM `--refresh`. The allowlist-validator (R6) is the last line of defense
  and rejects any `skill.json` whose `install` strings deviate from the canonical shape.
- R8a. **`schema_version` drift policy.** The current `skill.json` carries `schema_version: 1`. Both the embedded
  snapshot and any `--refresh`-fetched copy MUST be checked: a `schema_version` greater than the binary's known maximum
  (initially `1`) is treated as an unknown-future-shape error. With `--refresh` this emits a warning and falls back to
  embedded; the embedded snapshot's schema version is, by definition, one the binary understands.
- R8b. **`source.commit` and `verify.expected` policy.** The published `skill.json` carries a 40-char `source.commit`
  and an identical `verify.expected` field, labeled by the producer as an "advisory freshness probe". This plan does NOT
  use those fields to pin the clone (the producer's stated policy is rolling `main` distribution; pinning would defeat
  the bundle's own update-check loop). The fields are parsed and surfaced in `--print` output as a comment line so users
  can see them, but they do not gate execution. A future plan MAY add `--verify` that compares the cloned `HEAD` against
  `verify.expected` and warns on drift.
- R9. When the install destination already exists and is non-empty, `install` errors out with an explicit suggestion
  (`use --path <other-dir>` or `remove the existing directory first`). Never overwrite without consent. `--force` is a
  future flag, not part of this plan. The existence check uses `read_dir(&dest).next()` after canonicalization per R6b —
  symlinks pointing at non-empty targets count as non-empty.

---

## Scope Boundaries

- No `update` / `uninstall` / `list` / `path` verbs. Those land in a follow-up plan once `install` has shipped and real
  users surface real needs.
- No automatic update check. `agentnative-skill/bin/check-update` runs out of the installed bundle; `anc` does not
  shadow that.
- No skill-content vendoring or embedding. `anc` does not ship the skill bundle. The vendored `skill.json` snapshot
  contains the install **map**, not the skill content.
- No bundled host adapters (frontmatter rewriting, etc., as gstack does). Hosts that consume the `agentnative-skill`
  bundle as-is are supported now; hosts that need transformation are deferred.
- No new HTTP client dependency unless the dep tree already carries one. `ureq` (synchronous, no async runtime) is the
  most likely fit; `reqwest` is overkill. Decision deferred to U3.
- No telemetry, no analytics, no install counter.

### Deferred to Follow-Up Work

- **`anc skill update`** — `cd` into the installed bundle and `git pull --ff-only`. Trivial wrapper, but only worth
  shipping once `install` has settled.
- **`anc skill list`** — enumerate hosts known from the embedded map plus `--refresh` for the live one.
- **`anc skill path <host>`** — print the canonical install destination without installing. Useful for shell scripting.
- **Host adapters** for hosts that need frontmatter rewrites (e.g., Cursor's stricter frontmatter schema). Track
  separately when a real consumer reports breakage.

---

## Context & Research

### Relevant Code and Patterns

- `src/cli.rs` — `Commands` enum and `Subcommand` derives. `Skill` becomes a sibling of `Check`, `Completions`,
  `Generate`. `GenerateKind` is the closest precedent for a nested verb enum (`SkillCmd::Install { host, refresh, path,
  print }`).
- `src/main.rs` — top-level command dispatch. Adding a `Commands::Skill { … }` arm is purely additive.
- `build.rs` — already generates code from vendored sources (the `agentnative-spec` precedent). Adding a second
  generated file for the embedded `skill.json` snapshot is a straightforward pattern extension.
- `src/principles/spec/` — vendored content lives in-tree under a clearly-marked directory with a README explaining it
  is mirror-only. `src/skill_dist/` (or similar) follows the same convention for the vendored `skill.json`.

### Institutional Learnings

- The "vendor a snapshot, generate Rust constants from it" pattern is already proven by the spec-vendor work
  (`docs/plans/2026-04-23-001-feat-spec-vendor-plan.md`). Reuse the structure: a `scripts/sync-skill.sh` mirrors
  `scripts/sync-spec.sh`.
- Search `docs/solutions/` at execution time for `embed`, `include_str`, `git clone`, `network fallback`, `HTTPS
  client`, `ureq` — no known prior decisions at planning time, but worth checking before committing on a dep.

### External References

- `agentnative-site/src/build/skill.mjs` — the canonical schema for `skill.json`. The Rust-side parser must match its
  required fields and value-shape rules (commit SHA hex, semver, `git clone --depth 1` prefix).
- `agentnative-skill/bin/check-update` — illustrates the existing release-tag-based update flow that `anc skill` must
  not duplicate.
- gstack's `hosts/<name>.ts` files (`~/dev/agent-skills/gstack/hosts/`) — informational reference for how a more
  ambitious host-adapter system can be modeled later. Not implemented in this plan.

---

## Key Technical Decisions

- **Vendored snapshot, not network-only.** Offline `anc skill install <host>` is a hard requirement. The `--refresh`
  flag opts into the live fetch.
- **No SHA pinning, no release-tag pinning.** Matches the producer-side decision: `agentnative-skill` distributes via
  rolling `main`, freshness is signaled by release-tag check inside the bundle. `anc` mirrors that policy.
- **Allowlist commands by structural shape, not regex.** Tokenize the command and check token-by-token. Regex on a shell
  command is the wrong abstraction.
- **Use `Command::new("git").args([...])`, never `sh -c`.** Pass tokens directly to `git`. No shell interpretation.
  Defends against any allowlist bypass via shell metacharacters.
- **`--refresh` falls back to embedded on failure.** The embedded snapshot is the authoritative offline-capable source.
  Surface the failure (explicit message), but proceed with the embedded copy. Never silent-fallback — print what was
  substituted.
- **One direct dep, ~30-40 transitives — disclose, don't understate.** This crate has zero TLS/HTTP deps today (verified
  against `Cargo.lock`). Adding `ureq` with HTTPS support pulls `rustls` (or `native-tls`), `ring`, `webpki-roots`,
  `rustls-pki-types`, base64, flate2, and ~30 more transitive crates. `cargo deny` allowlist updates are non-trivial.
  The win — single-binary skill install — is worth that cost; understating it as "single new dep" misleads the reviewer.
  Pin `ureq` with `default-features = false, features = ["tls"]` (rustls-backed, no native-tls — avoids platform
  trust-store divergence and Windows MSVC build pain). Audit the licenses of the full transitive set during U3 and
  document each in `cargo deny`.

---

## Open Questions

### Resolved During Planning

- **Skill source** → not embedded as content. `anc` ships the install **map** only; the skill itself is `git
  clone`-distributed.
- **Drift handling** → no SHA pinning. Matches producer-side model. Update flow is owned by the bundle, not by `anc`.
- **Resolver model** → embedded default + `--refresh` for live fetch + `--path` for destination override.
- **Destination namespacing** → match `skill.json`'s declared paths exactly. No invention. The site is the contract.
- **Hosts at launch** → claude_code, codex, cursor, opencode (the four currently in `skill.json`). New hosts ship by
  re-vendoring after the site updates `skill.json`.

### Deferred to Implementation

- **HTTPS client choice.** Settled: `ureq` with `default-features = false, features = ["tls"]` (rustls backend, no
  native-tls). Pinned exact-version per repo convention. `cargo deny` allowlist documents the full ~40 transitive crates
  with license rationale. Decision lives in U3.
- **Schema strictness.** The Rust parser MAY accept extra unknown fields on the JSON for forward-compat (the site is
  free to add fields between `anc` releases). Decision: `#[serde(default)]` and ignore unknowns; pin existing field
  names tightly.
- **Where the vendored `skill.json` lives.** `src/skill_dist/skill.json` keeps it adjacent to its consumer; `dist/` is
  taken. Implementer's call.

---

## Implementation Units

- U1. **Vendor `skill.json` and generate `EMBEDDED_SKILL_JSON` constant**

**Goal:** Bring a snapshot of the canonical `skill.json` into this repo and expose its parsed contents at compile time
as a `&'static [u8]` (or pre-parsed struct).

**Requirements:** R3, R7.

**Dependencies:** None.

**Files:**

- Create: `scripts/sync-skill.sh`
- Create: `src/skill_dist/skill.json` (vendored snapshot)
- Create: `src/skill_dist/README.md` (mirror-only, do-not-edit notice)
- Modify: `build.rs`
- Modify: `src/main.rs` or `src/skill.rs` (new module) to expose the constant
- Test: `src/skill.rs` (or `tests/skill_embed.rs`) — parse-and-validate test on the embedded snapshot

**Approach:**

- `scripts/sync-skill.sh` mirrors `scripts/sync-spec.sh`'s shape and **defaults to the source repo, not the deployed
  site** — fetch from `agentnative-site` at a named ref to avoid deploy-lag. Resolution order:

1. `--from-local <path>` for offline / dev: copy from `<path>/src/data/skill.json`.
2. Default: `git -C <site-checkout> show <ref>:src/data/skill.json` if `AGENTNATIVE_SITE_DIR` is set.
3. Fallback: `curl -fsSL https://raw.githubusercontent.com/brettdavies/agentnative-site/main/src/data/skill.json` —
   pulls from the source repo's `main` branch, not the site's deployed CDN copy. Eliminates deploy-lag between a merged
   change to `skill.json` and re-vendoring.
4. The script prints `git diff` against the prior vendored copy after fetching — required reading at the RELEASES.md
   checklist step. A whitespace-only diff is fine; a URL change is a stop-and-review event.

- `build.rs` reads `src/skill_dist/skill.json`, parses it once with `serde_json` to surface schema errors at build time,
  then emits the raw bytes as `pub const EMBEDDED_SKILL_JSON: &[u8] = include_bytes!(...);`. Fails the build with a
  named-field error if the parse fails. **`schema_version` MUST be `1`** at build time; an unknown version fails the
  build (forces a deliberate plan revision before vendoring).
- The build's parse step does NOT need to commit a generated `.rs` file — `include_bytes!` is sufficient and the
  parse-time validation is purely a guard.

**Patterns to follow:**

- `scripts/sync-spec.sh` and the `build.rs` spec-vendor block are the model.

**Test scenarios:**

- Happy path: the embedded constant parses to a `SkillJson` struct with `install` containing the four expected hosts.
- Edge case: `cargo test` against a malformed `skill.json` (commented out — manual repro) fails with an actionable error
  citing the offending field. Document the test as `#[ignore]`-d.

**Verification:**

- `cargo build` succeeds. `cargo test` parses the embedded JSON without error.
- A diff between vendored `skill.json` and `https://anc.dev/skill.json` at sync time is visible to the committer.

---

- U2. **`Commands::Skill { … }` clap surface**

**Goal:** Wire `anc skill install <host> [--refresh] [--path DIR] [--print]` into clap.

**Requirements:** R1, R2, R4, R5.

**Dependencies:** None (parallel-safe with U1).

**Files:**

- Modify: `src/cli.rs`
- Test: `src/cli.rs` (extend tests with parse-round-trip on a few invocations)

**Approach:**

- Add `Skill { #[command(subcommand)] cmd: SkillCmd }` to `Commands`.
- Define `SkillCmd::Install { host, refresh, path, print }` mirroring `GenerateKind::CoverageMatrix`'s shape.
- `host` is `String` (validated in U4 against the parsed map — clap-side validation against an `enum` would lock the set
  at build time, defeating `--refresh`).
- `path` is `Option<PathBuf>`, default `None`.
- `refresh` and `print` are `bool` flags.
- Doc-comments on every field — they show up in `--help` output.

**Patterns to follow:**

- `Commands::Generate { artifact: GenerateKind }` is the precedent for nested subcommands.

**Test scenarios:**

- Happy path: `anc skill install claude_code` parses cleanly.
- Happy path: `anc skill install --refresh --path /tmp/foo --print codex` parses with all flags set.
- Edge case: `anc skill` with no verb prints the subcommand help (existing `arg_required_else_help` covers this).
- Edge case: `anc skill install` with no host produces clap's "missing required argument" error.

**Verification:**

- `cargo test` is green. Manual `anc skill install --help` shows the documented flags.

---

- U3. **HTTPS fetcher with timeout + fallback**

**Goal:** Implement the `--refresh` path: fetch `https://anc.dev/skill.json`, parse it, surface failures clearly, fall
back to the embedded snapshot.

**Requirements:** R3, R8.

**Dependencies:** U1 (the embedded fallback must exist), U2 (the flag must be defined).

**Files:**

- Modify: `Cargo.toml` (add `ureq` if not present, with the repo's pre-1.0 `=X.Y.Z` pinning convention)
- Create: `src/skill/fetch.rs` (or extend the new `src/skill.rs` module)
- Test: `src/skill/fetch.rs` plus an integration harness in `tests/`. **`ureq`'s `TestTransport` is `pub(crate)`, not
  part of the stable public API** — do not depend on it. Use a `std::net::TcpListener` on `127.0.0.1:0` (random port)
  plus a hand-rolled HTTP/1.1 responder in a `dev-dependencies`-only helper module. To exercise HTTPS specifically, add
  an `AGENTNATIVE_SKILL_URL` env override (read by `load_skill_json` when set) so the test can point at
  `http://127.0.0.1:<port>/skill.json` instead of the production HTTPS URL — env override is dev-only, undocumented in
  the public surface, never appears in `--help`. (Alternative: pull `mockito` into `[dev-dependencies]`. Hand-rolled is
  simpler and doesn't grow the dev-dep footprint.)

**Approach:**

- A single function: `fn load_skill_json(refresh: bool) -> SkillJson`.
- When `refresh` is `false`, parse `EMBEDDED_SKILL_JSON` and return.
- When `refresh` is `true`:
- Build a `ureq::Agent` with `connect_timeout(5s)` and `timeout(10s)`.
- GET `https://anc.dev/skill.json`.
- Parse the body.
- On any failure (connect, HTTP status, parse), emit a single `eprintln!("warning: --refresh fetch failed: {err}; using
  embedded snapshot")` and return the embedded copy.
- Never panic on a network error.

**Patterns to follow:**

- No precedent in this repo for HTTP. Look to `bin/check-update`'s shell-side conventions for the right user-facing feel
  (silent-on-disabled, single-line warnings on failure).

**Test scenarios:**

- Happy path: `load_skill_json(false)` returns the embedded snapshot in <1ms.
- Happy path: `load_skill_json(true)` against a local mock server returns the served map.
- Edge case: mock server returns HTTP 500 → fallback path used, warning logged. Use a captured-stderr assertion.
- Edge case: mock server returns malformed JSON → fallback path used.
- Edge case: connect refused (port closed) → fallback path used within the connect-timeout window.

**Verification:**

- `cargo test --features integration` (or the existing test command) passes.
- A manual `anc skill install --refresh claude_code --print` against the live `anc.dev` returns the live install
  command. With network disabled, the same command falls back gracefully and warns.

---

- U4. **Allowlist validator + `git clone` runner**

**Goal:** Validate that the resolved install command matches the structural allowlist, then execute it via
`Command::new("git")` with explicit args (no shell).

**Requirements:** R4, R6, R9.

**Dependencies:** U1, U2, U3.

**Files:**

- Create: `src/skill/install.rs`
- Test: `src/skill/install.rs` (unit tests for the validator, integration test for the runner via a fixture destination)

**Approach:**

- `fn parse_install_command(cmd: &str) -> Result<InstallCommand>`: tokenize on whitespace (the site contract guarantees
  no embedded whitespace; reject if tokenization yields anything ambiguous). Validate the prefix exactly matches
  `["git", "clone", "--depth", "1"]`. Validate the URL token matches the allowlist. Validate the destination is a path
  expression (does not start with `-`).
- `fn apply_path_override(cmd: &mut InstallCommand, path: Option<&Path>)`: if `--path` was given, replace the
  destination. The new path MUST also pass the under-`$HOME` (or absolute-path-not-under-`/`-itself) check.
- `fn install(cmd: &InstallCommand) -> Result<()>`:
- Refuse if the destination already exists and is non-empty (R9). Suggest `--path` in the error.
- `Command::new("git").arg("clone").arg("--depth").arg("1").arg(cmd.url).arg(&cmd.dest).status()?`.
- Surface non-zero exit codes with the destination path included in the error.

**Patterns to follow:**

- The runner's existing process-spawn conventions — see `src/runner/help_probe.rs` for the `Stdio::piped()` / timeout
  idioms (though `git clone` doesn't need a timeout; it inherits the network's natural one).

**Test scenarios:**

- Happy path: parse the canonical claude_code command from `skill.json` → `InstallCommand` with the documented fields.
- Happy path: `--path /tmp/anc-skill-test` rewrites the destination, validator still passes.
- Edge case (rejection): URL is `https://github.com/evil/agentnative-skill.git` → validator returns a typed error.
- Edge case (rejection): an extra `--config remote.origin.url=...` token slipped in → validator returns a typed error
  citing the offending token.
- Edge case (rejection): destination starts with `--` (flag-shaped) → validator returns a typed error.
- Edge case: destination already exists and is non-empty → installer returns a typed error before invoking `git`.
- Integration: against a local fixture URL serving an empty repo (use a `tempdir` with `git init --bare`), install
  succeeds; the destination contains a `.git/` directory.

**Verification:**

- `cargo test` green. Manual `anc skill install claude_code --path /tmp/anc-skill-smoke` clones the bundle into the
  path. Re-running errors out per R9.

---

- U5. **`Commands::Skill` arm in `main.rs`**

**Goal:** Wire the `Skill::Install` arm to the parts built in U1-U4 and produce user-facing output.

**Requirements:** R1, R2, R5, R8, R9.

**Dependencies:** U1, U2, U3, U4.

**Files:**

- Modify: `src/main.rs`
- Test: `tests/skill_install.rs` (end-to-end via `Command::new(env!("CARGO_BIN_EXE_anc"))`)

**Approach:**

- Match `Commands::Skill { cmd: SkillCmd::Install { host, refresh, path, print } }`.
- Load skill.json via U3.
- Look up the install command for `host` in the parsed map. Unknown host → error listing available host names.
- Apply `--path` override.
- Validate via U4's parser. Validation failure → error.
- If `--print`, print the resolved command (single line, suitable for `$(...)` capture) and exit 0.
- Otherwise, execute via U4's installer. Print a single completion line on success (`installed agent-native-cli skill
  into <dest>`).
- Exit codes: `0` on success, `1` on user error (unknown host, allowlist rejection, destination conflict), `2` on
  unrecoverable internal error (matches existing `--output text` exit-code convention).

**Patterns to follow:**

- The existing `Commands::Generate` arm in `main.rs`.

**Test scenarios:**

- Happy path: `anc skill install --print claude_code` writes the canonical command to stdout, exits 0.
- Happy path: `anc skill install claude_code --path <tempdir>/x` clones into `<tempdir>/x`, exits 0.
- Edge case: `anc skill install nonexistent-host` exits 1, stderr names the available hosts.
- Edge case: re-run into the same `--path` exits 1, stderr suggests `--path <other-dir>`.
- Edge case: `anc skill install --refresh claude_code --print` with the network disabled exits 0, stderr has the
  fallback warning, stdout has the embedded command.

**Verification:**

- All integration tests green. Manual smoke for each of the four hosts (with `--print`) shows the canonical command
  shape per host.

---

- U6. **Documentation + sync workflow**

**Goal:** Document the new subcommand and the manual `sync-skill.sh` workflow.

**Requirements:** R7 (sync workflow visibility).

**Dependencies:** U1-U5 conceptually complete.

**Files:**

- Modify: `README.md` (add a small `anc skill install` section under "Quick start" or wherever installation lives)
- Modify: `CLAUDE.md` (a short paragraph on the embed-vs-refresh model so future agents understand the contract)
- Modify: `AGENTS.md` if present and applicable (same content as CLAUDE.md, audience-appropriate)
- Modify: `RELEASES.md` to add a "re-vendor `skill.json`" step before each release (analogous to the existing
  spec-vendor step)

**Approach:**

- README "Install the skill" section: one paragraph + the four `anc skill install <host>` commands, with a note about
  `--refresh` and `--path`.
- CLAUDE.md: a paragraph on the two-source-of-truth model (anc.dev/skill.json is canonical; the embedded snapshot is an
  offline-capable mirror), and the security boundary (allowlist runner).
- RELEASES.md: an explicit pre-release checklist item: `bash scripts/sync-skill.sh && git diff
  src/skill_dist/skill.json` to surface any drift before tag.

**Patterns to follow:**

- Existing spec-vendor entries in RELEASES.md.

**Test scenarios:**

- Test expectation: none — pure documentation. Verification is link-rot-free copy review.

**Verification:**

- README renders correctly with the new section. `RELEASES.md` checklist includes the new step. No broken links.

---

## System-Wide Impact

- **Interaction graph:** New module under `src/skill/`; touched files are `src/cli.rs`, `src/main.rs`, `build.rs`,
  `Cargo.toml`. No existing module's behavior changes.
- **Error propagation:** New `SkillError` enum (or reuse the existing top-level error type — prefer reuse) carries the
  four primary failure modes (unknown host, allowlist rejection, destination conflict, network failure on `--refresh`).
  All surface via the existing `--quiet` and exit-code conventions.
- **State lifecycle risks:** `git clone` writes to disk. R9 prevents accidental overwrite. A user invoking with `--path
  /tmp` creates a `/tmp/anc-skill-...` directory; cleanup is the user's responsibility (we never `rm` anything).
- **API surface parity:** Adding a verb to a subcommand. `--help` and shell-completions get it for free. No back-compat
  concern for existing commands.
- **Integration coverage:** U5's integration tests exercise the end-to-end path against a real tempdir destination.
  Network paths use a local mock server (or are gated `#[ignore]`d for offline test runs).
- **Unchanged invariants:** `arg_required_else_help` stays on. The fork-bomb guard is not affected — `skill install`
  does not spawn `anc` recursively. The existing `Check`, `Completions`, and `Generate` arms are untouched.

---

## Risks & Dependencies

| Risk                                                                                              | Mitigation                                                                                                                                                                                                                                                                                                                                                                                                             |
| ------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `skill.json` schema evolves on the site faster than vendored snapshots are refreshed.             | `--refresh` always pulls the live file. The embedded snapshot is opportunistically refreshed at every release via the RELEASES.md checklist (U6). `schema_version` mismatch fails the build (R8a).                                                                                                                                                                                                                     |
| Allowlist bypass via injected `--config` tokens, shell metacharacters, or extra flags.            | R6 enforces an exact 6-token shape. `Command::new("git").args([...])` is the only invocation, no shell. `git clone` is invoked with hardening flags (R6c) — `-c protocol.allow=https-only`, `-c credential.helper=`, `-c core.askPass=`, `-c http.followRedirects=false`, plus an explicit `insteadOf` blocker — and a sanitized environment (`GIT_CONFIG_*`, `GIT_SSH*`, `GIT_PROXY_COMMAND`, `GIT_ASKPASS` removed). |
| Symlink at the destination redirects `git clone` to a sensitive system path.                      | R6b mandates parent-canonicalization before the `$HOME` policy check. TOCTOU window between validation and exec acknowledged as residual risk (single-user-machine threat model).                                                                                                                                                                                                                                      |
| Tilde-prefixed destinations from `skill.json` not expanded by `Command::new("git")`.              | R6a explicitly tilde-expands `~`/`~/` to `$HOME` before validation and exec. Test fixture exercises the canonical claude_code path end-to-end. Without R6a, every default install would fail.                                                                                                                                                                                                                          |
| `--path` override allows writing outside `$HOME` (e.g., `--path /etc/agent-native-cli`).          | The validator includes a path-sanity check: canonicalized destination parent MUST be under canonicalized `$HOME`. `--path /etc/...` rejected. Document the policy in `--help`.                                                                                                                                                                                                                                         |
| Network fetch (`--refresh`) hangs on a slow connection.                                           | 5s connect timeout + 10s total timeout in `ureq`. 64 KiB body cap. Failure falls back to embedded snapshot (without `--print`) or hard-errors (with `--print`).                                                                                                                                                                                                                                                        |
| TLS cert validation accidentally disabled by a future contributor.                                | R8 mandates that TLS validation MUST be enforced. `cargo deny` lints against any code that calls `danger_accept_invalid_certs` or sets a custom unverified verifier. A test with a self-signed cert asserts the fetch fails.                                                                                                                                                                                           |
| DNS hijack of `anc.dev` serves a malicious `skill.json` over a CA-trusted MITM cert.              | Acknowledged residual risk. The R6 allowlist is the last line of defense — even an attacker-controlled `skill.json` MUST pass the exact 6-token shape with the canonical URL. An attacker can serve only those install commands the validator would have accepted from a legitimate source. A future plan MAY add cert pinning for `anc.dev`.                                                                          |
| `agentnative-skill` repo compromised — rolling-`main` distribution executes attacker code.        | This is the no-SHA-pinning trade-off (R8b). The producer's update model is rolling `main`; pinning would defeat the bundle's own freshness loop. Documented explicitly in R8b. `--print` lets users inspect the resolved command before executing. Future `anc skill install --verify` could compare cloned HEAD against `verify.expected` and warn on drift.                                                          |
| Vendored `skill.json` poisoned at sync time (compromised `anc.dev` between releases).             | `scripts/sync-skill.sh` defaults to fetching from the **source repo** at a named ref, not the deployed site (U1) — eliminates deploy-window injection. Script prints a `git diff` against the prior vendored copy; RELEASES.md checklist requires reviewing the diff before commit. URL changes are stop-and-review events.                                                                                            |
| `agentnative-skill` repo is renamed or moved to a different owner.                                | Allowlist update + `skill.json` update on the site; `anc` re-vendors and ships a new release. The drift window is bounded by release cadence.                                                                                                                                                                                                                                                                          |
| User installs into a location their host doesn't actually scan.                                   | The default destination comes from `skill.json`, which is owned by the maintainer of `agentnative-site`. If a path is wrong, fix it on the site, not in `anc`. `--path` is for explicit user override only.                                                                                                                                                                                                            |
| `--depth 1` shallow clone breaks the bundle's own `bin/check-update` release-tag flow.            | Verify before U5 ships: read `agentnative-skill/bin/check-update` and confirm it calls `git ls-remote --tags origin` (works on shallow) rather than relying on local tag history. If it requires deeper history, U5 must drop `--depth 1` or call `git fetch --depth 100` post-clone.                                                                                                                                  |
| Embedded `skill.json` snapshot becomes weeks-stale, host paths change, users get silent failures. | Embed a `built_at: <YYYY-MM-DD>` constant alongside `EMBEDDED_SKILL_JSON` (sourced from sync-time). When the embedded copy is older than 60 days at runtime, `anc skill install` emits a one-line stderr nudge: `note: embedded skill map is N days old; pass --refresh for the latest`. Converts silent staleness into actionable signal.                                                                             |

---

## Documentation / Operational Notes

- Pre-release checklist gains one step: re-vendor `skill.json` and review the diff (mirrors the spec-vendor step).
- The README's existing "Install" section grows by ~10 lines. The skill's own README continues to document the bundle —
  `anc skill install` is one of N install methods, not the only method.
- `cargo deny` allow-list documents `ureq` (or whichever HTTP client is chosen), with a one-line rationale.
- A post-launch issue should track real-world host requests (Hermes? Factory? Slate?) — once 2-3 user requests come in,
  re-vendor `skill.json` to add them.

---

## Sources & References

- Existing CLI surface: `src/cli.rs`
- Existing top-level orchestration: `src/main.rs`
- Existing build-time vendoring precedent: `build.rs` + `src/principles/spec/`
- Existing pre-release sync precedent: `scripts/sync-spec.sh`, `RELEASES.md`
- Site contract: `agentnative-site/src/data/skill.json`, `agentnative-site/src/build/skill.mjs`
- Skill repo update mechanism: `agentnative-skill/bin/check-update`
- Sibling plan (scorecard schema): `docs/plans/2026-04-29-001-feat-scorecard-schema-metadata-plan.md`
- Reference architecture (more ambitious host adapters): `~/dev/agent-skills/gstack/hosts/` and
  `~/dev/agent-skills/gstack/scripts/host-config.ts`

---

## Document Review (2026-04-29)

Reviewed via `/ce-doc-review` (coherence, feasibility, scope-guardian, security-lens, adversarial). This was the
higher-risk plan in the pair — `anc` executes `git clone` with input that flows from a network-fetched JSON contract, so
the security review applied substantial pressure. Key findings absorbed:

**Applied (correctness / security):**

- **Tilde expansion gap (R6a).** `skill.json` ships literal `~`-prefixed destinations; `Command::new("git")` does not
  invoke a shell, so without explicit expansion every default install would fail. Added R6a as a hard requirement.
- **Allowlist token-count check (R6).** Original allowlist checked prefix and position-keyed tokens but did not enforce
  total token count. R6 now requires exactly 6 tokens, eliminating the `git clone --depth 1 --config <evil>=<value> URL
  DEST` injection class structurally.
- **`git clone` env + config sanitization (R6c).** Original plan invoked `git` with no env hardening; added explicit
  `-c` flags (`credential.helper=`, `core.askPass=`, `protocol.allow=https-only`, `http.followRedirects=false`,
  `insteadOf=` blocker) plus removal of `GIT_CONFIG_*`, `GIT_SSH*`, `GIT_PROXY_COMMAND`, `GIT_ASKPASS` from the spawned
  process environment.
- **Symlink canonicalization on destination (R6b).** A symlink at the destination would let a pre-positioned attacker
  redirect `git clone` to a sensitive system path; canonicalize parent before policy check.
- **TLS verification explicit in R8.** Original plan deferred dep choice with no TLS-validation requirement; R8 now
  mandates rustls-backed TLS with no `danger_accept_invalid_certs` escape hatch and a 64 KiB body cap.
- **`source.commit` / `verify.expected` policy explicit (R8b).** The producer's own `skill.json` carries a SHA and a
  verification field; original plan ignored both. Now explicitly documented as advisory-only with a future `--verify`
  flag noted.
- **`schema_version` drift handling (R8a).** Build-time check rejects unknown versions; runtime `--refresh` warns and
  falls back.
- **`--refresh + --print` interaction (R8).** Silent-fallback with stderr warning was wrong for scripted callers;
  `--print` now hard-errors on `--refresh` failure.
- **Dep footprint disclosure.** "Single new dep" understated reality (~30-40 transitives via rustls/ring); rewritten
  honestly with a `cargo deny` audit step in U3.
- **`ureq` mock test approach.** `TestTransport` is `pub(crate)`; switched to a hand-rolled `TcpListener` responder with
  an `AGENTNATIVE_SKILL_URL` env override for dev-only HTTPS-vs-HTTP routing.
- **`sync-skill.sh` source-of-truth.** Original plan curl'd from `anc.dev` (deploy-lag risk); now defaults to the source
  repo (`agentnative-site/src/data/skill.json`) at a named ref, with diff review at vendor time.
- **`claude` vs `claude_code` consistency.** Narrative examples normalized to `claude_code` to match `skill.json` keys
  verbatim.
- **Risks table extended.** Eight new rows covering tilde, symlink, env sanitization, TLS, DNS hijack, rolling-main
  supply chain, vendoring poison, shallow-clone vs check-update, embedded staleness.

**Deferred (worth revisiting before implementation, but not blocking):**

- **Scope-guardian: collapse to 2 units.** Reviewer argued U1+U5 could subsume U2/U3/U4. The split keeps changes
  reviewable and tests scoped; defensible at current size. Reconsider if implementation reveals significant overlap.
- **Scope-guardian: drop `--print` flag.** Argued as scope creep over the named friction (one copy-paste step). Counter:
  `--print` enables `$(anc skill install --print <host>)` capture for users who prefer to vet before executing —
  security-positive UX, low cost.
- **Adversarial: `install.sh`-only alternative.** Reviewer argued a 30-line shell script published at
  `anc.dev/install-skill.sh` would deliver ~80% of the UX win for ~5% of the cost. Acknowledged: the binary path is
  path-dependent reasoning. Counter: keeping the install pipeline inside `anc` lets future `--verify`, `--update`, and
  `list` verbs share one trust boundary; the shell-script path fragments that. Decision left in plan.
- **Adversarial: shallow-clone may break `bin/check-update`.** Surfaced as a verify-before-U5-ships step in the risks
  table. Real chance of needing to drop `--depth 1` if check-update relies on local tag history.
- **Embedded staleness nudge** (60-day-old warning) is a risks-table mitigation, not a plan unit yet — promote to a real
  implementation item if it survives PR review.

**Not absorbed (review noise / context drift):**

- One reviewer cited CLAUDE.md's "Scorecard v1.1 Fields" section — that's stale documentation for the *scorecard*, not
  the *skill*; not relevant to this plan. Plan 1 owns the scorecard doc cleanup.
- One reviewer flagged `--quiet` and `--output text` references in U5 as undefined for `skill install`. Re-checked: U5's
  exit-code paragraph parenthetically references the existing `anc check` text-output convention to anchor the exit-code
  semantics, not to imply `skill install` accepts those flags. The intent is `0/1/2` exit-code shape parity. Wording
  could be tighter at implementation time.
