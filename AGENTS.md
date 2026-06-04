---
name: agentnative
binary: anc
description: Agent-native CLI linter that audits whether a CLI follows the 8 agent-readiness principles. Bundle covers operator-facing usage, project structure, and the audit catalog.
homepage: https://anc.dev
repository: https://github.com/brettdavies/agentnative-cli
---

# AGENTS.md

## Running anc

The crate is `agentnative`. The installed binary is `anc`.

```bash
# Audit current project — `audit` is implicit when the first non-flag arg is a path
anc .

# Resolve a command on PATH and run behavioral audits against it
anc --command ripgrep

# JSON output for parsing
anc . --output json

# Quiet mode (warnings and failures only)
anc . -q

# Filter by principle (1-7)
anc . --principle 4

# Behavioral audits only (no source analysis)
anc . --binary

# Source audits only (no binary execution)
anc . --source

# Suppress inapplicable MUSTs for a categorical exception
anc . --audit-profile human-tui

# Install the companion skill bundle into your host's skills dir
anc skill install claude_code             # ~/.claude/skills/agent-native-cli
anc skill install --dry-run codex         # print resolved git command, don't run
anc skill install factory --output json   # emit envelope on success and error
```

Bare `anc` (no arguments) prints help and exits 2. This is a non-negotiable fork-bomb guard: when agentnative dogfoods
itself, children spawned without arguments must not recurse into `audit .`. Bare `anc skill` likewise prints help and
exits 2.

## Skill install

`anc skill install <host>` clones the `agentnative-skill` bundle into a host's canonical skills directory. Six hosts
ship at v0.1: `claude_code`, `codex`, `cursor`, `factory`, `kiro`, `opencode`. `--help` enumerates them; the JSON
envelope's `host` field reports the chosen one verbatim.

Output envelope (`--output json`) is uniform across success and error and across `--dry-run` and live install:

```json
{
  "action": "skill-install",
  "host": "claude_code",
  "mode": "dry-run",
  "command": "git clone --depth 1 <url> <dest>",
  "destination": "<resolved-dest>",
  "destination_status": "absent",
  "status": "success",
  "would_succeed": true
}
```

Field-presence rules: `would_succeed` only on `mode: "dry-run"`; `exit_code` only on `mode: "install"` AND only when
`git` actually spawned (e.g. `git-not-found` leaves it absent); `reason` only when `status: "error"`, with one of the
typed values `destination-not-empty` / `destination-is-file` / `home-not-set` / `git-not-found` / `git-clone-failed`.
`destination_status` is one of `absent` / `empty-dir` / `non-empty-dir` / `file`.

Exit codes follow the P4 convention: `0` for success, `1` for any envelope error (typed `reason` set), `2` for clap
usage errors (unknown host, missing positional, bare `anc skill`).

The `git clone` invocation runs with named-const hardening (`GIT_HARDEN_FLAGS`, `GIT_HARDEN_ENV_REMOVE`,
`GIT_HARDEN_ENV_SET`; the last includes `GIT_CONFIG_GLOBAL=/dev/null` and `GIT_CONFIG_SYSTEM=/dev/null` to disable
user-controlled git config, plus `GIT_TERMINAL_PROMPT=0`). No `sh -c`, no `env_clear`. Defense against `insteadOf`
URL-rewriting comes from disabling user config wholesale, not from a `-c url.<repo>.insteadOf=` flag (which would do the
opposite of blocking).

The host map (`SkillHost` enum, `KNOWN_HOSTS`, `resolve_host`, `host_envelope_str`) is **build-time-generated** from
`src/skill_install/skill.json` by `build.rs::emit_skill_hosts`. To add or change a host, edit the JSON (or run `bash
scripts/sync-skill-fixture.sh` to pull the upstream site contract) and `cargo build` regenerates the Rust map; no hand
edits to `src/skill_install.rs` are required. CI's `skill-fixture-drift.yml` runs `--check` on every PR to catch fixture
vs upstream drift.

## Agent-facing JSON surface

`anc audit <target> --output json` emits a `schema_version: "0.5"` scorecard. The schema is at `0.x` while `anc` is
pre-launch: shape may evolve before first public release, when it locks at `1.0`. During `0.x`, additive fields are the
norm; consumers should feature-detect new keys rather than pinning to an exact value. The current shape includes the
following scorecard-level fields beyond the base `results` / `summary`:

- `audience`: `"agent-optimized"` / `"mixed"` / `"human-primary"` / `null`. Derived from 4 signal behavioral audits
  (`p1-non-interactive`, `p2-json-output`, `p7-quiet`, `p6-no-color-behavioral`). Informational only; never gates totals
  or exit codes.
- `audience_reason`: present only when `audience` is `null`. Values: `"suppressed"` (signal audit masked by
  `--audit-profile`) or `"insufficient_signal"` (signal audit never produced). Tells an agent *why* there's no label.
- `audit_profile`: echoes the applied `--audit-profile <category>` flag value. `null` when no profile is set.
- `coverage_summary.{must,should,may}.verified`: requirements verified by an audit that actually ran. Audits suppressed
  by `--audit-profile` do not count as verified; suppression means verification was intentionally skipped.
- `spec_version`: the `agentnative-spec` version this CLI was built against. Sourced at build time from
  `src/principles/spec/VERSION` by `build.rs`; reads `"unknown"` if that file was missing at build time. Pin against
  this to know which spec contract the scorecard's requirement IDs reference.
- `tool`: `{ name, binary, version }`. Identifies what was scored. `version` is best-effort (manifest field for project
  mode, `<bin> --version` / `-V` for binary/command mode); `null` when probing fails or is declined by the self-spawn
  guard. Schema `0.4` addition.
- `anc`: `{ version, commit }`. Identifies the `anc` build that produced the scorecard. `commit` is `null` for builds
  outside a Git checkout. Informational, not signed provenance. Schema `0.4` addition.
- `run`: `{ invocation, started_at, duration_ms, platform: { os, arch } }`. `invocation` reflects what the user typed
  (captured pre-injection). `started_at` is RFC 3339 UTC. Schema `0.4` addition.
- `target`: `{ kind, path, command }`. `kind` is `"project"` / `"binary"` / `"command"`. The unused field is always
  `null`, never missing. Schema `0.4` addition.
- `badge`: `{ eligible, score_pct, embed_markdown, scorecard_url, badge_url, convention_url }`. Agent-native badge
  derivation from the live run. `score_pct` is the rounded percent of `pass / (pass + warn + fail)` (Skips and Errors
  excluded from the ratio). `eligible` is true iff `score_pct >= 80` and a tool slug was derivable. `embed_markdown` is
  `null` below the floor (do-not-nag contract). `scorecard_url` / `badge_url` are populated whenever a slug exists, even
  below the floor; `convention_url` always points at `https://anc.dev/badge`. Schema `0.5` addition. The text-mode hint
  (`--output text`) prints the same embed snippet only when eligible; below-floor runs print nothing badge-related.

`--audit-profile` accepts exactly 4 values: `human-tui`, `file-traversal`, `posix-utility`, `diagnostic-only`. Unknown
values exit 2 with a structured error. The full per-category mapping of suppressed audit IDs is committed to
`coverage/matrix.json` under the `audit_profiles` section. Agents should read that file rather than scraping `--help`:

```bash
jaq '.audit_profiles' coverage/matrix.json
```

Suppressed audits appear in `results[]` as `status: "skip"` with evidence starting with `"suppressed by audit_profile:
"` (the shared prefix is pinned in `src/principles/registry.rs` as `SUPPRESSION_EVIDENCE_PREFIX`).

## Exit Codes

- `0`: all audits passed
- `1`: warnings present, no failures
- `2`: failures, errors, or usage errors (bare `anc`, unknown flag, mutually exclusive flags, command not found on PATH)

Exit 2 is overloaded. To distinguish "ran but found problems" from "called incorrectly", parse stderr; usage errors
include `Usage:` text, and audit failures don't.

## Project Structure

- `src/audit.rs`: Audit trait definition
- `src/audits/behavioral/`: audits that run the compiled binary
- `src/audits/source/rust/`: ast-grep source analysis audits
- `src/audits/project/`: file and manifest inspection audits
- `src/runner.rs`: binary execution with timeout and caching
- `src/project.rs`: project discovery and source file walking
- `src/scorecard.rs`: output formatting (text and JSON)
- `src/types.rs`: AuditResult, AuditStatus, AuditGroup, AuditLayer
- `src/principles/registry.rs`: single source of truth linking spec requirements (P1–P7 MUSTs/SHOULDs/MAYs) to the
  audits that verify them
- `src/principles/matrix.rs`: coverage-matrix generator + drift detector

## Adding a New Audit

1. Create a file in the appropriate `src/audits/` subdirectory
2. Implement the `Audit` trait: `id()`, `group()`, `layer()`, `applicable()`, `run()`, and `covers()` if the audit
   verifies requirements in `src/principles/registry.rs` (return a `&'static [&'static str]` of requirement IDs)
3. Register in the layer's `mod.rs` (e.g., `all_rust_audits()`)
4. Add inline `#[cfg(test)]` tests
5. Regenerate the coverage matrix: `cargo run -- generate coverage-matrix` (produces `docs/coverage-matrix.md` +
   `coverage/matrix.json`, both tracked in git)

See `CLAUDE.md` §"Principle Registry" and §"`covers()` Declaration" for the registry conventions and drift-detector
behavior.

## Voice and prose rules

User-facing prose follows the **linter channel** rules in [`PRODUCT.md`](PRODUCT.md). Short version: second-person
imperative, no RFC 2119 keywords in error messages, no marketing voice, errors name "what failed / why / what to do."
`BRAND.md` and `PRODUCT.md` are the authoritative voice contract. The Vale rule pack maintained in `agentnative-spec`
(mirrored onto this repo's `dev` branch) encodes the literal phrases for enforcement on contributor PRs against `dev`;
the pack and `scripts/prose-check.sh` are dev-only tooling and do not ship to `main`.

## Testing

```bash
cargo test                    # unit + integration tests
cargo test -- --ignored       # fixture tests (slower)
```

### Test fixtures

Rust crates under `tests/fixtures/*/` (e.g. `broken-rust/`, `perfect-rust/`, `source-only/`, `cfg-test-edge-cases/`) are
standalone fake projects the audits run against. They are intentionally **not** workspace members — making them members
would cause `cargo build` from the root to compile every fixture and would apply workspace-level lints, dependencies,
and profile overrides to them, changing what the audits see and defeating the purpose of the fixture.

Because there is no workspace, Cargo's `field.workspace = true` inheritance is unavailable. The edition on every fixture
`Cargo.toml` must be set explicitly and **must match the main crate's edition** (see the top-level `Cargo.toml`,
currently `edition = "2024"`). When the main crate bumps its edition (e.g. Rust 2027), bump every fixture in lockstep in
the same PR — a fixture lagging behind main is a silent skew that can mask audit regressions on edition-specific syntax.

The audits themselves parse fixture sources via tree-sitter and do not invoke `cargo build`, so the edition has no
effect on current audit behavior. The lockstep rule exists for the future case where an audit reads edition-specific
constructs, and for the general "the project tests against the edition the project ships with" principle.

## Spec source (principles)

The canonical specification of the 7 agent-readiness principles lives in
[`brettdavies/agentnative`](https://github.com/brettdavies/agentnative), one file per principle under `principles/`. A
snapshot is **vendored** into this crate at `src/principles/spec/`, and `build.rs` parses its frontmatter at build time
to generate the `REQUIREMENTS` slice: IDs in the spec frontmatter are the contract this CLI audits against. There is no
manual sync of requirement IDs; only the `Audit::covers()` declarations are hand-maintained.

The `anc` audits in `src/audits/` themselves are derived **manually** from each principle's prose. When a principle's
spec adds, removes, or reworks a requirement, propagate to the relevant audit(s) deliberately.

**Resync cadence:** rerun `scripts/sync-spec.sh` after every new `agentnative-spec` tag. The script queries the remote
for the latest `v*` tag automatically and falls back to a local checkout (`$HOME/dev/agentnative-spec` by default) if
the remote is unreachable. The companion `repository_dispatch` from the spec's publish workflow is the canonical
trigger; if a future GitHub Action opens a resync PR automatically, this script becomes that action's body.

For iteration workflow, pressure-test protocol, and per-file structure of the spec itself, see
[`agentnative:principles/AGENTS.md`](https://github.com/brettdavies/agentnative/blob/main/principles/AGENTS.md). Read
before proposing a new audit that stretches the existing `P<n>` coverage.

When an audit is added or revised, its code or doc comment should name the principle code (`P<n>`) it implements for
traceability. Do not embed the principle text in the audit source.

## External signal / research

Curated external signal that informs principle iteration, audit rules, and positioning lives in the sibling research
folder:

- `~/obsidian-vault/Projects/brettdavies-agentnative/research/index.md`: top of the research tree. Lists every extract
  with date, topic, and which principles it maps to. Read this before adding new audits driven by external patterns or
  competitor behavior.
- `extracts/`: curated, topic-scoped files (verbatim quotes, principle mapping, recommended uses).
- `raw/`: full-text captures.

When an extract names concrete linter-rule candidates, walk its **"Linter rule coverage audit"** or equivalent section
against existing audits in `src/audits/` before opening a new audit.

## Documented Solutions

`docs/solutions/` (symlink to `~/dev/solutions-docs/`) is a searchable archive of documented solutions to past problems
(bugs, best practices, workflow patterns), organized by category with YAML frontmatter (`module`, `tags`,
`problem_type`). Search with `qmd query "<topic>" --collection solutions`. Relevant when implementing or debugging in
documented areas.
