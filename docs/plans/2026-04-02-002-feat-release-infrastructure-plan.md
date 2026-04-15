---
title: "feat: Release infrastructure — completions, RELEASING.md, changelog, Homebrew formula"
type: feat
status: active
date: 2026-04-02
origin: ~/.gstack/projects/brettdavies-agentnative/brett-main-design-20260327-214808.md
---

# feat: Release infrastructure — completions, RELEASING.md, changelog, Homebrew formula

## Overview

The release pipeline (`release.yml`) will fail on first tag because it copies `completions/` into archives but the
directory doesn't exist. Beyond that critical blocker, several files required by the `rust-tool-release` standard are
missing: `RELEASING.md`, populated `CHANGELOG.md`, and a Homebrew formula. README only documents 2 of 5 install
channels. Stale remote branches need cleanup.

## Problem Frame

Running `git tag v0.1.0 && git push --tags` today would trigger a release pipeline that fails at the archive step. Even
if it didn't fail, users couldn't `brew install` because no formula exists, and the GitHub Release body would be empty
because `CHANGELOG.md` has no content to extract. The `rust-tool-release` standard defines the full set of required
files and channels, and this plan closes every gap.

## Requirements Trace

- R1. `release.yml` archive step must succeed (requires `completions/` directory)
- R2. `RELEASING.md` must exist per rust-tool-release standard
- R3. `CHANGELOG.md` must have v0.1.0 content for GitHub Release body extraction
- R4. README must document all 5 install channels
- R5. Homebrew formula must be pre-seeded in `brettdavies/homebrew-tap`
- R6. Stale remote branches from merged PRs must be cleaned up
- R7. First `cargo publish` preparation (manual publish, then Trusted Publishing)

## Scope Boundaries

- This plan does NOT implement the first `cargo publish` — that happens at release time, not before
- This plan does NOT tag a release — it prepares the infrastructure so tagging works
- `CI_RELEASE_TOKEN` secret is assumed to already exist (shared across all brettdavies repos)
- `SECURITY.md` is not required by `rust-tool-release` — skip

## Context & Research

### Relevant Code and Patterns

- `~/.claude/skills/rust-tool-release/SKILL.md` — canonical release standard
- `~/.claude/skills/rust-tool-release/scripts/generate-completions.sh` — generates `completions/` directory
- `~/.claude/skills/rust-tool-release/scripts/generate-changelog.sh` — generates `CHANGELOG.md` from git-cliff + PR body
  expansion
- `~/dev/bird/RELEASING.md` — reference RELEASING.md
- `~/dev/bird/README.md` — reference README with all 5 install channels
- `~/dev/bird/completions/` — reference completions directory structure

### Institutional Learnings

- `~/dev/solutions-docs/architecture-patterns/release-pipeline-reusable-workflows-20260320.md` — 3-repo architecture
- `~/dev/solutions-docs/architecture-patterns/changelog-as-committed-artifact-20260319.md` — CHANGELOG is committed
  artifact, not auto-generated
- `~/dev/solutions-docs/integration-issues/homebrew-tap-automated-formula-updates-via-dispatch.md` — pre-seed formulas
  with `v0.0.0` placeholder, never the real first release version
- `~/dev/solutions-docs/workflow-issues/release-branch-pattern-for-guarded-docs-20260317.md` — release branch pattern
  for repos with `guard-main-docs.yml`

## Key Technical Decisions

- **Pre-seed Homebrew formula with v0.0.0**: Per learnings, using the real first release version causes `brew test-bot`
  to reject the dispatch update ("sha256 changed without url/version changing"). Pre-seeding with a dummy version avoids
  this.
- **Generate completions locally, commit to repo**: Per `rust-tool-release` standard, completions are
  platform-independent text files committed to `completions/`. Not generated in CI.
- **CHANGELOG via generate-changelog.sh**: Uses git-cliff + PR body `## Changelog` section expansion. This is the
  standard tooling, not manual editing.
- **Release branch from main, not dev**: Per the release branch pattern, `guard-main-docs.yml` blocks docs paths. The
  release branch cherry-picks non-docs commits from dev.

## Open Questions

### Resolved During Planning

- **Does CI_RELEASE_TOKEN exist?**: Yes — shared across all brettdavies repos, stored in 1Password.
- **Should we create SECURITY.md?**: No — not required by `rust-tool-release` standard. Can add later.

### Deferred to Implementation

- **Exact CHANGELOG content**: `generate-changelog.sh` will produce the content from PR bodies. The quality depends on
  how well the PR `## Changelog` sections were written.

## Implementation Units

- [ ] **Unit 1: Generate and commit shell completions**

**Goal:** Create `completions/` directory with pre-built completions for bash, zsh, fish, elvish, and PowerShell.
Unblocks the release archive step.

**Requirements:** R1

**Dependencies:** None

**Files:**

- Create: `completions/agentnative.bash`
- Create: `completions/agentnative.zsh`
- Create: `completions/agentnative.fish`
- Create: `completions/agentnative.elv`
- Create: `completions/_agentnative.ps1`

**Approach:**

- Run `~/.claude/skills/rust-tool-release/scripts/generate-completions.sh` from the repo root
- The script uses the `completions` subcommand already in `cli.rs`
- Verify `completions/` is NOT in `Cargo.toml` `exclude` (it should ship with `cargo install`)
- Also generate for the `anc` alias binary

**Patterns to follow:**

- `~/dev/bird/completions/` — same directory structure and naming

**Test scenarios:**

- Happy path: `generate-completions.sh` produces 5 files, one per shell
- Happy path: `generate-completions.sh --check` passes after generation (freshness check)
- Edge case: `anc` alias completions also generated if the script supports multiple binaries

**Verification:**

- `completions/` directory exists with non-empty files for each shell
- `generate-completions.sh --check` passes
- `cargo package --list` includes `completions/` files

---

- [ ] **Unit 2: Create RELEASING.md**

**Goal:** Document the release process per `rust-tool-release` standard.

**Requirements:** R2

**Dependencies:** None

**Files:**

- Create: `RELEASING.md`

**Approach:**

- Copy structure from `~/dev/bird/RELEASING.md`
- Adapt for agentnative (crate name, binary names including `anc` alias)
- Include the release branch pattern procedure (branch from main, cherry-pick, generate-changelog, PR)
- Note first-publish chicken-and-egg (manual `cargo publish` then Trusted Publishing setup)

**Patterns to follow:**

- `~/dev/bird/RELEASING.md`

**Test expectation:** None — documentation file.

**Verification:**

- File exists and covers: version bump, changelog generation, release branch, tagging, first-publish note

---

- [ ] **Unit 3: Update README with all 5 install channels**

**Goal:** README install section documents Homebrew, pre-built binary, cargo install, cargo-binstall, and source build.

**Requirements:** R4

**Dependencies:** None

**Files:**

- Modify: `README.md`

**Approach:**

- Expand the Install section to match `~/dev/bird/README.md` pattern
- Add: Homebrew (`brew tap brettdavies/tap && brew install agentnative`)
- Add: Pre-built binary (link to GitHub Releases)
- Keep: `cargo install agentnative`
- Keep: `cargo binstall agentnative`
- Add: From source (`git clone && cargo build --release`)

**Patterns to follow:**

- `~/dev/bird/README.md` install section

**Test expectation:** None — documentation file.

**Verification:**

- README contains all 5 install methods
- Homebrew command uses correct tap name

---

- [ ] **Unit 4: Pre-seed Homebrew formula in brettdavies/homebrew-tap**

**Goal:** Create `agentnative.rb` formula in the tap repo so the release dispatch has a target.

**Requirements:** R5

**Dependencies:** None (separate repo)

**Files:**

- Create: `Formula/agentnative.rb` (in `brettdavies/homebrew-tap`)
- Modify: `.github/workflows/update-formula.yml` allowlist (in `brettdavies/homebrew-tap`)

**Approach:**

- Use `v0.0.0` placeholder URL and zeroed sha256 — NEVER use the real first release version
- Source-build formula with `depends_on "rust" => :build`
- Use `generate_completions_from_executable` in `install` method for shell completions
- Install both `agentnative` and `anc` binaries
- Add `agentnative` to the dispatch allowlist in `update-formula.yml`
- Follow the formula template from `~/.claude/skills/homebrew-tap-publish/references/conventions.md` if available,
  otherwise mirror `bird.rb`

**Patterns to follow:**

- `~/dev/homebrew-tap/Formula/bird.rb` — existing formula in the tap

**Test scenarios:**

- Happy path: `brew audit --formula Formula/agentnative.rb` passes (use formula name, not file path)
- Edge case: formula installs both `agentnative` and `anc` binaries

**Verification:**

- Formula file exists in tap repo
- `update-formula.yml` allowlist includes `agentnative`
- Formula compiles and installs both binary names

---

- [ ] **Unit 5: Clean up stale remote branches**

**Goal:** Remove merged feature branches from origin.

**Requirements:** R6

**Dependencies:** None

**Files:** None

**Approach:**

- Delete `origin/feat/fix-r8-quiet` and `origin/fix/fork-bomb-depth-guard` — both PRs are merged
- Use `git push origin --delete <branch>`

**Test expectation:** None — git housekeeping.

**Verification:**

- `git branch -r` shows only `origin/dev`, `origin/main`, `origin/HEAD`

---

- [ ] **Unit 6: Generate CHANGELOG.md for v0.1.0 (on release branch)**

**Goal:** Populate CHANGELOG.md with v0.1.0 release notes using `generate-changelog.sh`.

**Requirements:** R3, R7

**Dependencies:** Units 1-4 (all other changes should be committed first so they're included in the release)

**Files:**

- Modify: `CHANGELOG.md`

**Approach:**

- This happens on the release branch during release prep, NOT on dev
- Create `release/v0.1.0` from `origin/main`
- Cherry-pick non-docs commits from dev
- Run `~/.claude/skills/rust-tool-release/scripts/generate-changelog.sh`
- The script auto-detects version from branch name
- Commit CHANGELOG.md as part of the release PR to main
- After PR merges: `git tag v0.1.0 && git push origin main --tags`

**Patterns to follow:**

- `~/dev/solutions-docs/workflow-issues/deterministic-release-workflow-pr-provenance-generated-changelogs-20260325.md`

**Test scenarios:**

- Happy path: `generate-changelog.sh` produces a CHANGELOG.md with a `## [0.1.0]` section
- Happy path: CHANGELOG.md content includes PR links and author attribution
- Error path: script fails if `cliff.toml` is misconfigured — verify `[remote.github]` section exists

**Verification:**

- CHANGELOG.md has a populated `## [0.1.0]` section
- `awk '/^## \[/{if(n++)exit}n' CHANGELOG.md` extracts non-empty release notes

## System-Wide Impact

- **Interaction graph:** The Homebrew formula in the tap repo receives `repository_dispatch` from `release.yml`. The
  `update-formula.yml` allowlist gates which repos can trigger updates. Adding `agentnative` to the allowlist is
  required.
- **Error propagation:** If `completions/` is missing at release time, the archive step fails and no release is
  published. This is the most critical fix.
- **API surface parity:** The `anc` alias binary also needs completions generated. The `generate-completions.sh` script
  may need to handle multiple binary names.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| `generate-completions.sh` doesn't support multiple binaries | Inspect the script; run it twice if needed (once for `agentnative`, once for `anc`) |
| `generate-changelog.sh` produces empty output (squash-merge history) | The script uses git-cliff on cherry-picked commits (which are individual conventional commits), not squash-merged ones |
| Homebrew formula pre-seed conflicts with first real release | Use `v0.0.0` placeholder per documented pattern |
| `cargo publish` fails on first run (Trusted Publishing not configured) | First publish must be manual with `CARGO_REGISTRY_TOKEN` |

## Sources & References

- **Skill:** `~/.claude/skills/rust-tool-release/SKILL.md`
- **Reference repo:** `~/dev/bird/` (existing tool following same standard)
- **Homebrew tap:** `brettdavies/homebrew-tap`
- **Solution:** `~/dev/solutions-docs/architecture-patterns/release-pipeline-reusable-workflows-20260320.md`
- **Solution:** `~/dev/solutions-docs/integration-issues/homebrew-tap-automated-formula-updates-via-dispatch.md`
