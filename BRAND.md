# BRAND.md: agentnative voice and identity

Source of truth for the voice and identity of the agentnative standard. Shared across the spec, the website, the linter,
the skill bundle, and any future channel. Each channel inherits from this document and adds channel-specific register
and artifacts in its own `PRODUCT.md`.

> **Source of truth: `agentnative-spec/BRAND.md`.** This file is vendored into each channel repo (`agentnative-site`,
> `agentnative-cli`, `agentnative-skill`) via `scripts/sync-prose-tooling.sh`. Edits in
> a consumer repo will be overwritten on the next sync. File issues and PRs against this repo.

## Brand identity

**Three words: opinionated, precise, inviting.**

- **Opinionated.** The standard has a point of view. It does not enumerate tradeoffs and shrug; it states "MUST do X,
  here is the failure mode if you don't, here is the canonical fix." The point of view is what makes the standard worth
  citing.
- **Precise.** RFC 2119 language. Anchors stable and citable. Numbers measured, not asserted. Where a contract has a
  canonical realization (a flag spelling, an exit code, a path), it is named explicitly.
- **Inviting.** The reader (or agent handler) keeps reading by design. That comes from details: typography that rewards
  a slow read, prose that rewards a fast scan, code blocks that read like reference material a reader can trust.
  Inviting is not "friendly" and it is not "marketing." It rewards engagement.

## Voice anchor

Concrete before abstract. Show then tell. No filler adjectives. The standard speaks as a standard, not a person (
first-person singular is out), but every channel inherits the same sequence: state the contract, show the failure mode,
name the canonical fix.

## Audiences

Two first-class consumers across all channels:

- **Humans** evaluating, adopting, implementing, or extending the standard. Spec-channel readers are technically deep
  and arrive with skepticism; site-channel readers are time-pressured and decide in 60 seconds whether to take the
  standard seriously; linter users invoke at the terminal. Each channel narrows further in its own `PRODUCT.md`.
- **AI agents** consuming the standard programmatically: markdown via `Accept: text/markdown`, requirement IDs via
  frontmatter parsing, skill bundles via `SKILL.md`/`AGENTS.md` discovery, linter findings via JSON. Their UX is "do
  anchors stay stable, do IDs survive reorganizations, does the channel render cleanly across versions." This is not a
  nice-to-have. The agent audience is first-class. Decisions that improve a channel for humans at the cost of agent
  legibility are regressions.

## Universal anti-patterns

These bans apply across every channel. The narrative below is authoritative for both the *why* and the *what*; a Vale
rule pack on the `dev` branch encodes the literal phrases for enforcement on contributor PRs (dev-only tooling).

- **No marketing register.** First-person belief and recommendation framings are out. The standard speaks in the third
  person about contracts, not in the first person about beliefs.
- **No hedge words.** Probabilistic softeners undercut MUST and SHOULD. The contract is the contract.
- **No filler adjectives.** Marketing modifiers do no work. Concrete before abstract; the noun carries the meaning.
- **No verbatim quotation from any single source.** Where multiple sources converge on a claim, the standard's wording
  sounds like triangulation, not citation. Lineage belongs in the README's `Acknowledgements` section, not in the
  contract.

## Voice anchors: concrete examples

The ✓ column shows the contract voice. The ✗ column names the category of failure rather than reproducing literal banned
phrases. The category labels describe the shape of the failure each ✓ phrasing replaces.

| ✓                                                                                                                         | ✗                                                                                      |
| ------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| "CLI tools MUST run without human input."                                                                                 | First-person belief framing with lowercase RFC keyword and audience speculation.       |
| "Authentication failed: token expired (`expires_at: 2026-03-25T00:00:00Z`). Run `tool auth refresh` or set `TOOL_TOKEN`." | Apologetic register with vague remediation and no actionable diagnostic.               |
| "Numeric output is locale-independent: `.` decimal, no thousands grouping, regardless of `LC_NUMERIC`."                   | First-person recommendation with hedge word and unspecific "potential issues" framing. |

## Channels

The shared identity above applies to every channel. Each channel adds register and artifacts in its own `PRODUCT.md`:

- **Spec** (`agentnative-spec/PRODUCT.md`): RFC 2119 register, third-person standards voice, present tense, no
  first-person plural, no implementation leakage in MUSTs.
- **Site** (`agentnative-site/PRODUCT.md`): visual system (palette, typography, code-block treatment, OG image),
  tech-stack decisions (SSG, Worker, content negotiation), JS budget, dark-mode design.
- **Skill bundle**: instructional voice, second-person imperative is allowed, agent-loadable.
- **Linter (`anc`)**: terse error messages, ≤80-column help text, four-part error rubric (offending value, constraint,
  valid example, remediation).

## Channel artifacts

Each channel's repo carries its own narrow stack on top of this universal `BRAND.md`. The canonical layout:

| Channel      | `PRODUCT.md` location          | Deep tier-3                                            | Vale rule pack            | How `BRAND.md` arrives          |
| ------------ | ------------------------------ | ------------------------------------------------------ | ------------------------- | ------------------------------- |
| Spec         | `agentnative-spec/PRODUCT.md`  | `principles/`, `docs/architecture/`, `docs/decisions/` | `styles/spec/` (dev-only) | (origin — this repo)            |
| Site         | `agentnative-site/PRODUCT.md`  | `DESIGN.md` (root)                                     | (none yet)                | `scripts/sync-prose-tooling.sh` |
| CLI (`anc`)  | `agentnative-cli/PRODUCT.md`   | `src/` (Rust source IS the artifact)                   | (planned)                 | `scripts/sync-prose-tooling.sh` |
| Skill bundle | `agentnative-skill/PRODUCT.md` | `bundle/`                                              | (planned)                 | `scripts/sync-prose-tooling.sh` |

Every channel has earned a `PRODUCT.md` today: each holds channel-specific decisions (visual system, error rubric,
instructional voice, etc.) that the universal `BRAND.md` cannot carry.

**Convention: deep tier-3 artifacts live at the repo root, not in `docs/`.** The site channel's `DESIGN.md` sits at
`agentnative-site/DESIGN.md` (not `docs/DESIGN.md`) so the `/impeccable` skill loader and human readers find it without
traversal. Future deep companions (e.g., a hypothetical `GOVERNANCE.md`) follow the same pattern. Only research
artifacts and historical plans live under `docs/`.

## Sync

This document is the source of truth. Each consumer channel (`agentnative-site`, `agentnative-cli`, `agentnative-skill`)
vendors a copy via its own `scripts/sync-prose-tooling.sh`, decoupled from `scripts/sync-spec.sh` (which vendors
`principles/p*-*.md`, `VERSION`, and `CHANGELOG.md` into each consumer's `spec/`). The split exists because brand prose
and the principle contract release on different cadences.

`sync-prose-tooling.sh` tracks this repo's `main` HEAD, not tagged releases. Prose tooling is shared infrastructure
across channels, not pinned per-version. Edits land here first; consumer repos pick up the change in a follow-on commit
(batched into a release branch or maintenance PR).
