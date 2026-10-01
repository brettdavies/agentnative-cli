---
title: Release the code-unwrap cfg(test) Exemption - Plan
type: fix
date: 2026-09-18
status: implementation-ready
topic: release-the-code-unwrap-exemption
artifact_contract: ce-unified-plan/v1
artifact_readiness: implementation-ready
product_contract_source: ce-plan-bootstrap
execution: code
---

# Release the code-unwrap cfg(test) Exemption - Plan

## Goal Capsule

- **Objective:** Someone who installs `anc` and audits a Rust crate with ordinary inline test modules gets a truthful
  `code-unwrap` verdict. Today they get a failure listing every `.unwrap()` in their test code, and the fix for that has
  been merged and unreleased since June.
- **Means:** Cut the release that carries it, then add one cheap signal so the next merged-but-unreleased fix is visible
  without a consumer having to discover it.
- **Execution profile:** `code-unwrap` stops reporting `#[cfg(test)]`-gated calls. For a consumer on a released binary
  this reads as a fix; for anyone who added `--include-tests` to work around it, that flag keeps its meaning.

## Problem Frame

`code-unwrap` exempts `.unwrap()` inside `#[cfg(test)]`-gated items. That behavior landed in #77 (`817d6fa`) and was
corrected for `cfg(not(test))` polarity in #80 (`7ba92a5`), both merged 2026-06-02 and 2026-09-03. `git tag --contains`
is empty for both commits: `main` sits at `v0.5.0`, tagged 2026-06-01, and `dev` is 133 commits ahead of it spanning
2026-06-01 to 2026-09-17.

So every installed `anc` predates the fix. Audited against `brettdavies/xurl-rs`, the released 0.5.0 binary reports 32
`.unwrap()` hits and all 32 sit after a `#[cfg(test)]` marker: a 100% false-positive rate on the audit's headline Rust
finding. Built from `dev` and run against the same tree, `code-unwrap` passes.

This already cost a downstream consumer real work. `xurl-rs` carried a task in its adoption-grade plan to report this as
an upstream false positive; there was nothing to report, because `dev` already carries the fix. Its companion task, an
`.anc.toml` that no audit applies, is a real gap that `docs/plans/2026-10-01-0015-feat-config-follows-the-repo-plan.md`
owns, and it reaches users only through a release as well: v0.5.0 predates `.anc.toml` support (#76, #83).

The plan that corrected the polarity bug,
`docs/plans/2026-06-03-004-fix-pr77-code-unwrap-cfg-not-test-polarity-plan.md`, is marked `status: completed` and lists
"`anc audit` on the consumer repo's `code-unwrap` continues to Pass" as an acceptance criterion. That criterion holds on
`dev` and has never held for any binary a user can install. Completed meant merged, and nothing downstream of merge was
checked.

## Scope

**In scope**

- Cut a release from `dev` so the exemption reaches installed binaries.
- One recurrence signal, chosen at implementation time from U2's options.
- A note in `RELEASES.md` that merged is not shipped.

**Out of scope**

- Changing `code-unwrap` itself. The audit is correct on `dev`; this is a distribution problem.
- The open `anc web` stack (#97, #99, #101-#106). Whether it lands before the cut is U1's one decision, not a scope
  expansion.
- Re-auditing the other 131 commits on `dev`. They passed CI when they merged.
- Anything in the consumer repo. Its false-positive task is closed; its `.anc.toml` task waits on the config plan.

## Implementation Units

### U1. Cut the release

**Goal.** A published `anc` whose `code-unwrap` exempts `#[cfg(test)]`-gated calls.

**Approach.** Follow `RELEASES.md` § Releasing dev to main as written: overlay `dev`'s tree onto a `release/*` branch
cut from `main`, PR to `main`, tag. Work the go/no-go list in `RELEASES-PREFLIGHT.md` and the post-tag steps in
`RELEASES-POSTFLIGHT.md`; neither needs changing for this release.

**The one decision.** Whether the open `anc web` stack lands on `dev` first. Releasing without it is the shorter path
and unblocks the consumer sooner; waiting folds three months of backlog and a new command into one cut. Either is
defensible, and the version bump follows from which is chosen. Settle it before cutting, not during.

**Verification.**

- `git tag --contains 817d6fa` and `git tag --contains 7ba92a5` each name the new tag.
- Install the published artifact and audit a crate whose only `.unwrap()` calls are inside `#[cfg(test)]`: `code-unwrap`
  passes. The minimal case is a crate whose `src/main.rs` is `fn main() {}` plus a `#[cfg(test)] mod tests` containing
  one `.unwrap()`. That input fails on 0.5.0 and passes on `dev`, so it distinguishes the two binaries in one run.
- `CHANGELOG.md` names the `code-unwrap` change under the new version, worded for someone who saw the false positives.

### U2. Make the gap visible next time

**Goal.** A merged user-visible fix that has not shipped is noticeable without a consumer discovering it.

**Approach.** Pick one, smallest first. This is a signal, not a release gate, and it should not be able to block a PR.

- A scheduled job that reports how far `dev` is ahead of `main`, in commits and in days. Today that reads 133 and 108.
  Cheapest to write, and it puts the number somewhere it gets seen.
- `anc --version` and the JSON envelope reporting the commit the binary was built from. This one also answers the
  question a confused consumer actually has, which is whether their binary predates a fix.

Either is sufficient. Do not build both, and do not build a release gate: the failure here was that nobody was looking,
not that a check was missing from CI.

**Verification.** Run it against today's `dev` and `main` before the cut, when the gap is 133 commits, and confirm it
reports the gap. A signal that only works on a clean tree proves nothing.

### U3. Say that merged is not shipped

**Goal.** The next plan's acceptance criteria do not treat a merge as delivery.

**Approach.** One short passage in `RELEASES.md` stating that a fix reaches users at a tag, not at a merge, and that a
plan claiming a consumer-visible outcome is not complete until a release carries it. Correct the completed-plan record
for `docs/plans/2026-06-03-004-fix-pr77-code-unwrap-cfg-not-test-polarity-plan.md` in the same change, since its
acceptance criterion is the worked example of the mistake.

**Verification.** Someone reading `RELEASES.md` can answer "is this fix in front of users yet" without reading git
history.

## Sequencing

U1 first; it is the only unit a user feels. U2 and U3 are independent of it and of each other, and either can follow.

## Definition of Done

- A tag contains `817d6fa` and `7ba92a5`, and the published binary passes `code-unwrap` on the minimal `#[cfg(test)]`
  crate above.
- `CHANGELOG.md` names the change.
- One recurrence signal from U2 exists and was observed reporting a real gap.
- `RELEASES.md` states that merged is not shipped.

## Sources

- `src/audits/source/rust/unwrap.rs` — the audit and its walk; correct as of `dev`.
- `docs/plans/2026-06-03-004-fix-pr77-code-unwrap-cfg-not-test-polarity-plan.md` — the completed plan whose acceptance
  criterion never held for a released binary.
- `RELEASES.md`, `RELEASES-PREFLIGHT.md`, `RELEASES-POSTFLIGHT.md` — the release flow this plan follows unchanged.
- PRs #77 and #80 — the merged, unreleased fixes.
