# Contributing to `agentnative-cli`

`anc` is the reference linter for the [agent-native CLI spec](https://github.com/brettdavies/agentnative). This repo
holds the Rust source, the scoring engine, the language auditors, and the registry. Principle-level discussion (the
spec's MUST/SHOULD/MAY tiers, new principles, applicability clauses) belongs in the spec repo, not here. For
visitor-facing cross-repo navigation, see [`anc.dev/contribute`](https://anc.dev/contribute).

## Contribution tiers

`anc` accepts three shapes of contribution. All three are welcome; none is required. CLI work skews toward Tier 3
because the linter is the implementation surface. Most improvements are concrete code.

| Tier            | Shape                                                                                                                                                                                                                                                                     | Intake                                                                                                                                                                                                                                                                                                                        | Effort   |
| --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| **1. Signal**   | False-positive report, scoring bug, feature request, install or platform issue, registry omission                                                                                                                                                                         | [`false-positive`](https://github.com/brettdavies/agentnative-cli/issues/new?template=false-positive.yml) / [`scoring-bug`](https://github.com/brettdavies/agentnative-cli/issues/new?template=scoring-bug.yml) / [`feature-request`](https://github.com/brettdavies/agentnative-cli/issues/new?template=feature-request.yml) | ~5 min   |
| **2. Proposal** | A new language auditor design, a scoring-engine rework, a new audience or audit profile, a registry expansion                                                                                                                                                             | Issue with the design before opening a PR; maintainer agreement is the gate                                                                                                                                                                                                                                                   | ~1-2 hrs |
| **3. Code**     | Language auditor implementations, false-positive fixes, scoring-engine improvements, registry submissions ([`add-tool-to-registry`](https://github.com/brettdavies/agentnative-cli/issues/new?template=add-tool-to-registry.yml) template), platform / install-path fixes | PR against `dev` (per branch discipline)                                                                                                                                                                                                                                                                                      | Variable |

For principle-level discussion (MUST/SHOULD/MAY tier changes, new principles, applicability-clause critiques), file a
`pressure-test` issue in the
[spec repo](https://github.com/brettdavies/agentnative/issues/new?template=pressure-test.yml). Those discussions don't
belong here.

**Response expectations:** Tier 1 and Tier 2 are welcome and get a substantive reply when time allows. Tier 3 PRs are
reviewed when scope and time permit. A solo maintainer cannot promise merge windows; real PRs land.

## Branch model

```text
feat/* → PR to dev (squash merge)
       → cherry-pick non-docs commits to release/<YYYY-MM-DD>-<slug>
       → PR release/* to main (squash merge)
       → tag v<X.Y.Z> + GitHub Release + crates.io + Homebrew tap
```

`dev` is the integration branch. `main` is what consumers install. Engineering docs (`docs/plans/`, `docs/solutions/`,
`docs/brainstorms/`, `docs/reviews/`) live on `dev` only and are blocked from `main` by `guard-main-docs.yml`. Full
procedure in [`RELEASES.md`](./RELEASES.md).

## Dev setup

```bash
git clone https://github.com/brettdavies/agentnative-cli && cd agentnative-cli
cargo build                   # builds the `anc` binary
cargo test                    # unit + integration tests
cargo deny audit              # supply-chain audit (licenses, advisories)
git config core.hooksPath scripts/hooks   # activate pre-push hook (next section)
```

The `anc` binary lives at `target/debug/anc` after build; `cargo run -- audit .` against any CLI's source tree runs the
full scoring pipeline locally. For the registry source of truth (the list of tools rendered on `anc.dev/scorecards`),
see [`src/principles/registry.rs`](src/principles/registry.rs).

## Pre-push hook battery

The repo ships a pre-push hook that mirrors CI: `cargo fmt --check`, `cargo clippy -Dwarnings`, `cargo test`,
`cargo-deny audit`, and a Windows compatibility scan. Activate the hook once after clone:

```bash
git config core.hooksPath scripts/hooks
```

PRs that pass the hook locally also pass CI. Fix locally before pushing; the hook is the gate.

## Pull requests

- **Title format:** [Conventional Commits](https://www.conventionalcommits.org/) (`type(scope): description`). The PR
  title becomes the squash-merge commit subject and lands in `CHANGELOG.md` via `cliff.toml` at release time, so write
  it as the commit message you want in history.
- **Body:** follow [`.github/pull_request_template.md`](.github/pull_request_template.md). The `## Changelog` section is
  the source of truth for `CHANGELOG.md` entries. Write for users, not implementers. Never hand-edit `CHANGELOG.md`.
- **Scope:** keep PRs small and single-purpose where possible. False-positive fixes and new language auditors each
  warrant their own PR.
- **Tests:** new auditors ship with a unit test and a regression fixture; scoring-engine changes ship with before/after
  scorecards for at least three tools from the registry.

## Registry submissions

Adding a tool to the leaderboard ([`anc.dev/scorecards`](https://anc.dev/scorecards)) goes through this repo's
[`add-tool-to-registry`](https://github.com/brettdavies/agentnative-cli/issues/new?template=add-tool-to-registry.yml)
template, which proposes the tool. After triage, the entry lands via a PR against
[`src/principles/registry.rs`](src/principles/registry.rs) and the scoring pipeline generates the scorecard. A direct PR
is also welcome.

The spec repo's [`grading-finding`](https://github.com/brettdavies/agentnative/issues/new?template=grading-finding.yml)
template is a different path: that's for spec-feedback derived from scoring a real CLI (Tier 1/2 pressure-test variant),
not for adding a tool to the registry.

## AI disclosure

Inherits from the spec's AI disclosure policy. See
[agentnative/CONTRIBUTING.md § AI disclosure policy](https://github.com/brettdavies/agentnative/blob/main/CONTRIBUTING.md#ai-disclosure-policy).

## Security

Do not file security issues in the public tracker. Use the
[GitHub private security advisories channel](https://github.com/brettdavies/agentnative-cli/security/advisories/new).

## License

Dual-licensed: MIT or Apache-2.0 at the consumer's option. No CLA. The Apache-2.0 side carries the standard contributor
patent grant under §3 of the license. See [`LICENSE-MIT`](./LICENSE-MIT) and [`LICENSE-APACHE`](./LICENSE-APACHE).

## Cross-repo navigation

The full visitor-facing menu lives at [`anc.dev/contribute`](https://anc.dev/contribute). Per-repo intakes:

- [Spec](https://github.com/brettdavies/agentnative): principle text, pressure-tests, versioning policy
- This repo: the linter, the scoring engine, the registry
- [Site](https://github.com/brettdavies/agentnative-site): anc.dev source, leaderboard renderer, live-scoring
- [Skill bundle](https://github.com/brettdavies/agentnative-skill): agent-facing bundle, install paths
