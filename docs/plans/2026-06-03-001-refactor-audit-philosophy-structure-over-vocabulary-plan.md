---
title: "refactor: Audit philosophy — structure over vocabulary; spec is canonical"
type: refactor
status: proposed
priority: P1
date: 2026-06-03
origin: "Cross-cutting umbrella surfaced by adversarial review of merged PRs #73, #76, #77, #79 — audits drifting from
  spec text via vocabulary-broadening rather than structural reasoning"
---

# refactor: Audit philosophy — structure over vocabulary; spec is canonical

## Summary

Three of the four most recent audit-touching PRs (#73, #76, #79) followed the same shape: an audit was failing on a real
CLI (`xurl-rs`, `bird`, or `anc` itself), the team broadened the audit's Pass conditions, and the audit's behavior
diverged from the vendored spec text it claims to verify. The fourth (#77) is a different failure mode but shares the
underlying mechanism — vocabulary-based pattern matching that mis-judges adjacent cases.

The root cause is consistent across all four: **audits encode vocabulary lookups (closed lists of known names) as if
they were quality checks, when the principle they claim to verify is structural.** `COMMON_VERBS`, `STANDARD_VERBS`,
`DISCRIMINANT_FIELD_NAMES`, `ERROR_VALUE_STRINGS`, `SUCCESS_VALUE_STRINGS` — each is a closed vocabulary that pretends
to encode quality. In practice each one encodes "what shapes the maintainer happened to think of when writing the
audit." When a real CLI lands outside that shape, the audit either over-fires (real bug, fix the audit) or under-fires
(the maintainer broadens the vocabulary, the audit gets less strict, downstream consumers can't tell the difference).

This plan does not redesign any specific audit. It establishes the direction that per-audit redesign plans will
reference, and it supersedes plans #002 (PR #73 patches) and #005 (PR #79 patches) at the design level — those are
tactical stopgaps; this is the long-term shape.

## Problem frame

### Spec is canonical

The vendored spec text in `src/principles/spec/principles/p{1..7,8}-*.md` is the published contract for what `anc`
audits. When an audit's behavior disagrees with the spec, the audit is wrong. Period. If the audit needs to evolve, the
PR sequence is:

1. PR upstream in `agentnative-spec` revising the principle text.
2. `anc` re-vendors (via the existing sync mechanism).
3. PR in `anc` updates the audit to match.

This sequence was bypassed by #73, #76, and #79 — each landed in `anc` only, with no `agentnative-spec` PR. The result
is a vendored spec text that no longer describes the audit it claims to bind to.

### Vocabulary lookups as quality proxies

A closed vocabulary list is a useful heuristic when the vocabulary is small and motivated (e.g., `--help` / `--version`
are canonical CLI flag names). It is a failed quality proxy when the vocabulary is open-ended (verb lists, error-field
names, identifier sets). The failure modes:

- **Over-fires on equivalent but differently-named patterns.** `expect()` is morally identical to `unwrap()`; the
  `code-unwrap` audit only catches one.
- **Under-fires when the vocabulary is broadened to fit a specific CLI.** Adding `dm`, `repost`, `unrepost` to
  `STANDARD_VERBS` (PR #76) makes the audit pass for X/Mastodon CLIs but doesn't make it more accurate — it has just
  encoded one platform's vocabulary as universal.
- **Becomes self-confirming.** When `is_discriminant` accepts any string-shaped value under specific field names, and
  the field names are chosen because they match the maintainer's CLI, the audit becomes a fit to the population it
  claims to score (PR #79).

### Structural reasoning is more durable

The principle behind `p6-consistent-naming` is "an agent can predict where the action lives." That is structural — it's
about depth of action position, hierarchy shape, schema stability — not about whether `audit` is a verb. The principle
behind `p2-must-json-errors` is "an agent can dispatch on stable typed error information from stderr." Also structural —
stable schema, typed identifier, parseable to an agent's match arm — not "is the field called `kind`."

Structural checks scale better:

- They don't require maintenance every time a new CLI ships novel vocabulary.
- They catch gaming cases (an envelope with three correctly-named fields where the values play wrong roles still fails a
  structural check).
- They produce stable verdicts as the spec evolves — the structure of "action position predictability" is durable in a
  way that "must be called `kubectl get`" is not.

## Audits to evaluate against this philosophy

Each entry below is a candidate for redesign. The redesign for each will land in its own per-audit plan, referencing
this one as the design-level authority.

| Audit ID                | File                                         | Vocabulary currently used                                                  |
| ----------------------- | -------------------------------------------- | -------------------------------------------------------------------------- |
| `p6-consistent-naming`  | `src/audits/behavioral/consistent_naming.rs` | `COMMON_VERBS` (39 entries)                                                |
| `p6-may-standard-names` | `src/audits/behavioral/standard_names.rs`    | `STANDARD_VERBS` + `.anc.toml domain_verbs` extension                      |
| `p2-must-json-errors`   | `src/audits/behavioral/json_errors.rs`       | `DISCRIMINANT_FIELD_NAMES`, `ERROR_VALUE_STRINGS`, `SUCCESS_VALUE_STRINGS` |
| `code-unwrap`           | `src/audits/source/rust/unwrap.rs`           | Literal `.unwrap()` call expressions                                       |
| `p1-must-env-var`       | (see registry)                               | Env var name vocabulary (`NO_COLOR`, etc.)                                 |
| `p6-must-global-flags`  | (see registry)                               | Flag name vocabulary (`--help`, `--version`, etc.)                         |
| `p7-naked-println`      | (source-layer)                               | Literal `println!` / `print(` call expressions                             |
| `p2-source-tightening`  | (per the existing plan)                      | Library-specific call patterns                                             |

Two of these — env vars and global flags — have unambiguous canonical vocabulary the spec itself names (`NO_COLOR` is
defined by <https://no-color.org>; `--help` / `--version` are POSIX-canonical). Those stay vocabulary-based. The rest
are candidates for structural redesign.

## Direction (not prescriptive yet)

The redesign for each audit will be authored in its own plan; this section captures the orientation those plans inherit.

1. **Source audits walk code structure, not call expressions.** Tree-sitter / ast-grep is already in the toolbox; use it
   for typed traversal (functions, impl blocks, derive macros, attribute scopes) rather than text-pattern grep for call
   names. PR #77's `cfg(test)` walk is the right direction; the polarity bug surfaced by the review is a hint that this
   layer needs more rigor, not less.

2. **Behavioral audits assert structural properties of output, not specific field names.** "The envelope has three
   semantically-distinct fields" is a structural claim. "The envelope has a field literally named `kind`" is a
   vocabulary claim. Prefer the former wherever the principle allows. When vocabulary is required (e.g., `--help` flag
   detection), keep the vocabulary small and document why.

3. **When vocabulary IS the principle, the vocabulary lives in the spec, not in the audit.** If the spec mandates
   `error/kind/message`, the audit enforces literal names. If the spec mandates "three roles," the audit enforces roles.
   The current PR #79 state has the audit checking roles while the spec text says names — that mismatch is the actual
   bug.

4. **"Behavioral-first, structural fallback" carries over from the existing role-based-audit-validators plan.** Where a
   behavioral check is possible (run the CLI, observe the output), prefer it. Source-layer is the cheap fallback for
   what behavioral can't reach.

5. **Scorecard transparency is a hard requirement for any audit that admits per-CLI mitigation.** PR #76's `.anc.toml
   domain_verbs` is acceptable only if a domain-verb-assisted Pass is visibly distinct in the scorecard JSON from a
   built-ins-only Pass (see plan #003). The general rule: any audit that takes an external signal as input (config,
   profile, suppression) must reflect that input in the output.

## Relationship to per-PR plans

| Plan | Tactical scope                                  | Long-term ownership                                                                         |
| ---- | ----------------------------------------------- | ------------------------------------------------------------------------------------------- |
| #002 | PR #73 — probe-failure, leaf-noun, `emit`       | Superseded by this plan's `p6-consistent-naming` redesign                                   |
| #003 | PR #76 — scorecard transparency, verb-list trim | Mostly self-contained; references this plan for the "no per-CLI vocab without spec PR" rule |
| #004 | PR #77 — `cfg(not(test))` polarity              | Bug fix; informs the source-audit direction here                                            |
| #005 | PR #79 — three worst bypasses                   | Superseded by this plan's `p2-must-json-errors` redesign                                    |

Plans #002 and #005 are stopgaps. They stop the audit from being actively misleading until this plan delivers the
redesign. They do NOT update the spec or introduce new spec-divergent behavior. Plans #003 and #004 are mostly
self-contained — #003 because transparency is a self-contained ergonomic fix, #004 because the polarity bug is a real
correctness regression independent of philosophy.

## Open questions

- **Source-layer language scaling.** ast-grep walks scale by adding per-language pattern files. Adding Go, JavaScript,
  Java multiplies the maintenance surface. Do we accept the Rust/Python pair as the supported core for the next few
  releases, or do we publish a "supported source-audit languages" matrix in `agentnative-spec`?
- **Closed-source CLIs.** Some agents will audit binaries whose source is unavailable. Behavioral audits keep working
  there; source audits don't. How does the scorecard surface "source audits N/A because source unavailable" — silent
  Skip, explicit `n_a` with reason, or a `audit_scope` block in the scorecard's `target` info?
- **Spec evolution cadence.** Should `agentnative-spec` ship a new top-level "Structural Language" companion document
  that explicitly defines structure-over-vocabulary as the authoritative principle, or does each principle absorb this
  guidance independently when its audit is redesigned?
- **Vocabulary that stays.** `--help`, `--version`, `NO_COLOR` — name these explicitly in the spec as canonical
  vocabulary, with the rationale "the convention exists outside `anc` and predates it." A short list under "vocabulary
  the spec mandates" makes it harder for future audits to backslide into novel vocabulary.
- **PR-template guardrail.** A new audit-change PR-body checkbox: "Does this introduce or expand a vocabulary lookup? If
  yes, where is the spec PR?" — should this live in `.github/pull_request_template.md`, or is it CLAUDE.md /
  CONTRIBUTING territory? The aim is to make the spec-first sequence enforceable at review time.

## Acceptance

- This document is committed to `docs/plans/` and referenced by plans #002 and #005.
- The next audit-change PR after this lands either (a) makes no spec-divergent change, or (b) carries an
  `agentnative-spec` PR link in its body.
- Within two minor releases, `p6-consistent-naming` and `p2-must-json-errors` ship a structural redesign that references
  this plan's direction.
- The vendored spec text in `src/principles/spec/principles/p{2,6}-*.md` is reconciled with the audits — either by
  reverting the audits to the spec, or by landing an upstream spec PR and re-vendoring.

## Out of scope

- Specific audit redesigns. Each lives in a follow-on plan.
- Source-layer language expansion (Go, JavaScript, etc.).
- A unified audit-test-framework refactor — the current per-audit structure is fine; only the underlying checks change.
