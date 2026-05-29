# Releases rationale

Companion to [`RELEASES.md`](./RELEASES.md). RELEASES.md is the runbook (commands, paths, decision tables). This file
holds the WHY behind those rules: branching model, PR conventions, release pipeline, CHANGELOG generation, prose-audit
pipeline, spec-vendor pipeline, branch-protection pitfalls.

Read this when:

- A rule in RELEASES.md doesn't make sense and you're tempted to change it.
- A new contributor asks "why do we do X this way".
- You're adding a new release-flow rule and need to know where it fits the existing model.

## Branching model

### Forever `dev`, ephemeral release branches

`dev` is never deleted, even after a release. The next release cycle reuses the same `dev`. The repo's
`deleteBranchOnMerge: true` setting doesn't touch `dev` as long as `dev` is never the head of a PR. Using a short-lived
`release/*` head is what keeps the setting compatible with a forever integration branch.

Engineering docs (`docs/plans/`, `docs/solutions/`, `docs/brainstorms/`, `docs/reviews/`) live on `dev` only. They never
reach `main`. `guard-main-docs.yml` blocks them from PRs targeting `main`, and `guard-release-branch.yml` rejects any PR
to main whose head isn't `release/*`.

### Why cherry-pick from `main`, not branch from `dev`

Branching from `dev` and then `gio trash`-ing the guarded paths seems simpler but produces `add/add` merge conflicts
whenever `dev` and `main` have diverged (which they always do after the first squash merge). The file appears as "added"
on both sides with different content. Always branch from `origin/main` and cherry-pick the dev commits onto it.

### CalVer + version branch naming

Branch naming `release/v<version>` or `release/v<version>-<slug>` (e.g. `release/v0.1.0`,
`release/v0.2.0-python-audits`) makes release branches sortable and unambiguous when multiple cuts are in flight. The
`v<version>` prefix is required: `scripts/generate-changelog.sh` extracts the version from the branch name. Slug is
kebab-case, short, descriptive.

## PR body conventions

### No explainer prose in the body

Every section of a PR body is user-facing substance only: what is changing for the consumer that was not already there —
the **net diff**, not the commit history or intermediate state that produced it. Workflow mechanics (cherry-pick,
regenerate, pre-push gate, CI behavior) is documented in RELEASES.md and `.github/`, NOT in the PR body. Triple-diff
output ("A: 12 files, B: none, C: clean"), leak-audit narration ("`guard-main-docs` runs clean", "no guarded paths
leaked"), patch-id cherry-audit counts, pre-push gate results, CI audit status, exclusion rationale, and other
verification artifacts stay local; anomalies get fixed before push, not audit-trailed in the body.

The PR body is read by humans reviewing what shipped. Workflow mechanics and tool-fix provenance are noise from that
perspective; they belong in this file, the script outputs, and the commit history respectively.

### Why `feat`/`fix` are preferred over `chore`

`cliff.toml` skips `^chore` (and `^style` / `^test` / `^ci` / `^build`) regardless of body content. Mistyping a
user-facing change as `chore` silently strips it from release notes. Prefer `feat` / `fix` when the change has any
user-observable effect (config defaults, env vars, default behaviors).

### Why required-when-empty sub-headers

`Related Issues/Stories` has four labels (`Story:` / `Issue:` / `Architecture:` / `Related PRs:`). `Files Modified` has
four sub-headers (`Modified` / `Created` / `Renamed` / `Deleted`). All four must appear in every PR, even when empty:
write `- None.` or `n/a` rather than deleting the label. Reason: scanners and humans both rely on a known section shape.
Conditionally-absent sections force every reader to mentally audit "did the author skip this or does it not apply?"

### Why no AI attribution

`Co-Authored-By: Claude …`, `🤖 Generated with [Claude Code]`, or any similar AI-attribution trailer is banned from
commit messages and PR bodies. Commits and PRs stand on their own technical content. Attribution trailers are noise and
they age poorly as tools shift.

### Why no hard line wraps

Author each paragraph and each bullet as one logical line, however long. GitHub soft-wraps for display. Hard wraps
within prose produce visible mid-sentence breaks in some renderers and interfere with the prose-audit pipeline: Vale's
line-anchored output reports findings against split lines, LanguageTool's input handling can choke on certain
control-char interactions. The auto-format hook skips `/tmp/` paths so the body keeps its authored shape, don't undo
that with manual wrapping during composition. Same rule applies to commit messages composed via heredoc.

### Why release-PR bodies repeat changelog entries from upstream PRs

The release PR carries the same `### Added` / `### Changed` / `### Fixed` / `### Documentation` bullets as the feature
PRs it cherry-picks. The repetition is intentional and harmless: `cliff.toml`'s `^release` skip prevents the release-PR
squash commit from being double-counted in any future regeneration.

### Why internal-tooling commits don't appear in `## Changelog`

`chore(cliff): ...`, `chore(prose-audit): ...`, and similar internal tooling commits don't appear in the PR body's `##
Changelog`. They are not user-facing. They belong in commit history and in the Files Modified / Key Details sections of
the PR body, not in the source-of-truth release notes.

## Triple-diff verification

The release-PR procedure runs three diffs (A: main→release, B: release→dev for non-doc paths, C: dev→main) plus a
patch-id cherry audit. This is belt-and-suspenders because missed cherry-picks have shipped to `main` on this and
sibling repos before, and the file-level diff in B alone doesn't catch the patch-id false-negative class.

### Why patch-id cherry-audit output is noisy

In a squash-merge workflow, `git cherry HEAD origin/dev` produces many `+` lines that need human triage. They do NOT
auto-block the release. Expected sources of false positives:

1. **Historical commits squash-merged in prior releases.** The squash commit on main has a different patch-id than the
   dev commits it consolidates, so old commits show as `+` forever. Anything older than the previous release tag is
   almost always this.
2. **Cherry-picks where conflict resolution stripped guarded paths** (`docs/plans/`, `docs/brainstorms/`, etc.) or
   otherwise altered the tree. Same source-code intent, different patch-id.
3. **Intentionally skipped commits** (docs-only commits, release-prep backports, revert-and-redo prep steps).

A real miss looks like: a recent feat/fix/chore commit on dev whose *file content* is not yet on main. To triage a `+`
line:

```bash
git show <sha> --stat                       # what did it touch?
git diff origin/main..HEAD -- <those-files> # already on release?
```

If every touched file is guarded (`docs/plans/`, `docs/brainstorms/`, etc.) OR the content is already on main via a
prior squash, it's a false positive (no action). Otherwise cherry-pick the commit and re-run the triple-diff.

## CHANGELOG generation

### Generated, never hand-written

`scripts/generate-changelog.sh` (with `cliff.toml`) is the only sanctioned way to update `CHANGELOG.md`. The script runs
`git-cliff` to prepend a versioned entry for commits since the last tag, then walks each squash-merged PR's body to
extract the `## Changelog → ### Added / Changed / Fixed / Documentation` subsections, replacing the auto-generated
bullets with the curated PR-body content (with author and PR-link attribution).

If a PR's `## Changelog` section is empty, that PR's entry is omitted from the changelog (empty section = no user-facing
change). To fix a wrong CHANGELOG entry, fix the input: edit the squash-merged PR body, then re-run the script. Do
**not** edit `CHANGELOG.md` directly.

### Why `cliff.toml` skips chore/style/test/ci/build

These commit types do not produce user-facing content. If a cherry-picked PR has user-facing `## Changelog` content but
its commit subject starts with one of those types, its bullets get silently dropped. After running the script,
cross-check the generated section against `gh pr view <num> --json body` for each cherry-picked PR; correct mistyped PR
titles (e.g. `chore` → `feat`) and re-amend the cherry-pick subject before re-running. See "Prefer `feat`/`fix` over
`chore`" in global CLAUDE.md for prevention.

## Release pipeline

### Annotated tags + Trusted Publishing

Always use annotated tags (`-a -m`). Bare `git tag <name>` silently fails with `fatal: no tag message?` on machines
where `tag.gpgsign=true` is set globally (a brettdavies dotfile default). See
[solutions: git tag fails with tag.gpgsign — use annotated tags](https://github.com/brettdavies/solutions-docs/blob/main/best-practices/git-tag-fails-with-tag-gpgsign-use-annotated-tags-2026-04-13.md).

Subsequent releases use the OIDC Trusted Publishing flow built into `release.yml`: no static token in CI. The initial
publish (`v0.1.0`) requires a regular crates.io API token because Trusted Publishing needs the crate to exist first.

### Why `make_latest: false` then `finalize-release`

The GitHub Release is created visible-but-not-latest (`make_latest: false`) so `cargo-binstall` and `/releases/latest`
don't 404 during the bottle-build window, but the release isn't yet promoted to "Latest" while bottles upload. After the
homebrew-tap workflow uploads bottles to this repo's release assets, it dispatches `finalize-release` back to this repo,
which idempotently flips `make_latest: true`. End result: crate on crates.io, GitHub Release marked latest, Homebrew
formula updated with bottles, all atomically advertised.

### Why backport `main` → `dev` after publish

Once `finalize-release.yml` has flipped the GitHub Release to `published`, `scripts/sync-dev-after-release.sh` backports
the release-bookkeeping files from `main` to `dev` so future builds from `dev` report the released version (and so `anc
audit`'s embedded badge URL points at the right slug, not stale `0.1.0`).

The script surgically updates only `Cargo.toml`'s `[package].version` line (other `Cargo.toml` lines on `dev`,
post-launch deps, rust-version bumps, are preserved), regenerates `Cargo.lock` via `cargo build --release`, and copies
`CHANGELOG.md` verbatim from `origin/main`. The single commit lands directly on `dev` (signed via your normal commit
signing, no PR), establishing release backport as a deliberate convention rather than the prior "never back-merged"
norm.

The backport is idempotent: re-running on a `dev` already in sync exits 0 with no commit.

### Why two musl rows are hard-blocking

The cross-compile matrix builds 7 targets; the two musl rows (`x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`)
are statically linked. `linux_musl_required: true` in the reusable workflow input makes their failures hard-block the
release, and `linux_musl_verify_alpine: true` runs the x86_64-musl binary inside `alpine:latest` after build as an
exec-compat sanity check. Alpine and other musl-libc hosts can run them without glibc, so a green musl row matters: the
dynamic targets fail loudly on missing-symbol mismatch, but a statically-linked binary that happens to compile could
still segfault on first exec without the verify step.

## Spec-vendor pipeline

The `agentnative-cli` repo vendors the spec via `scripts/sync-spec.sh` (preferred latest-tag fetch) and exposes the
vendored content through codegen-derived structures (e.g., principle metadata in `crates/anc-core/src/principles`). The
spec-fixture-drift CI workflow runs `--check` on every PR; pulling the latest content during the release flow catches
any spec changes since `dev` was branched and avoids tagging a release with the codegen-derived host map one revision
behind upstream.

The skill-fixture refresh (`scripts/sync-skill-fixture.sh`) is the analogous pipeline for skill metadata: the Rust map
(`SkillHost` / `KNOWN_HOSTS` / `resolve_host`) regenerates from the JSON automatically on the next `cargo build`. No
manual src edits needed.

## Prose scrubbing scope

Three release-flow artifacts live outside any automated prose audit and need a manual scrub before they ship:

- **PR bodies.** `gh pr create` and `gh pr edit` send body text directly to GitHub; no automated prose audit has reach
  there.
- **`CHANGELOG.md`.** A generated artifact built from upstream PR bodies; it inherits whatever prose those PR bodies
  carry, so scrubbing happens at generation time on the release branch.
- **Release-PR bodies.** The `release/v<version>` PR to `main` carries contributor-authored wrap-up text composed after
  `CHANGELOG.md` has been generated, and the same out-of-repo gap applies.

The canonical Vale + LanguageTool rule packs and orchestrator behavior live in the spec repo at
[`~/dev/agentnative-spec/docs/architecture/voice-enforcement.md`](../agentnative-spec/docs/architecture/voice-enforcement.md).
Until those packs are vendored into this repo (a deferred follow-up tracked in the spec plan; expected to extend
`scripts/sync-spec.sh`), point Vale at the spec checkout via `--config`.

Scrub-before-submit (author in `/tmp/`, scrub there, submit via `--body-file`) avoids the round-trip of "submit, scrub,
edit, scrub again". Every fix lands locally and the public PR sees only clean text. The auto-format hook skips `/tmp/`
paths so the body keeps its authored shape and no soft-wrapping is injected.

For a `CHANGELOG.md` finding, fix the upstream PR body (which `generate-changelog.sh` re-fetches every run) and
regenerate. Hand-editing `CHANGELOG.md` directly produces drift the next regeneration overwrites.

## Branch protection

### Status-audit context strings

The `required_status_audits[].context` strings in `protect-main.json` MUST match exactly what GitHub publishes for each
audit:

- **Inline job** (with `name:` field): published as just `<job-name>` (no workflow-name prefix).
- **Reusable-workflow caller** (`uses: .../foo.yml@ref`): published as `<caller-job-id> / <reusable-job-id-or-name>`.

Mixing these produces a stuck-but-green PR: all actual audits report green, but the ruleset waits forever on a context
that will never appear. Confirm the real contexts after a first CI run with:

```bash
gh api repos/brettdavies/agentnative-cli/commits/<sha>/audit-runs --jq '.audit_runs[].name'
```

### Why rulesets live in-repo

Committing the JSON alongside code means ruleset changes land via the same review process as workflow changes. A
`chore(ci): tighten protect-main` change goes through dev → release/* → main like anything else.

## Related docs

- [`RELEASES.md`](./RELEASES.md) (operational runbook: commands, paths, decision tables)
- [`AGENTS.md`](AGENTS.md) (running `anc`, project structure, adding new audits)
- [`README.md`](README.md) (install channels, principles, CLI reference)
- [`.github/pull_request_template.md`](.github/pull_request_template.md) (PR body structure with changelog sections)
