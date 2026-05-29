---
title: "Scoring — leaderboard formula and badge eligibility"
last-revised: 2026-05-28
---

# Scoring — leaderboard formula and badge eligibility

This document defines how a tool's per-requirement scorecard collapses into the single `score_pct` that drives the
`anc.dev` leaderboard, the badge color, and badge eligibility. The taxonomy of per-requirement statuses and how they are
produced is defined in [`AGENTS.md`](AGENTS.md); this document is the layer above it: how those rows aggregate into one
number.

The formula is the spec-side contract. The `anc` CLI computes `badge.score_pct` to match it, and the
[`agentnative-site`](https://github.com/brettdavies/agentnative-site) renderer reads the eligibility floor and color
bands defined here.

## Scope: shipped-binary behavior only

A public score reflects how the **shipped binary behaves**, observed by running it (`anc audit --command <binary>`).
Source-code and repository audits (static analysis of a project's source tree, manifest inspection, bundle presence) do
**not** contribute to the public score. What a tool's source code looks like does not change how an agent experiences
the installed binary. The binary is what agents run.

Concretely, only behavioral-layer requirement rows enter the formula. Source-layer and project-layer audits are out of
scope for the leaderboard; they belong to a future advisory mode (`anc` run against a source tree to help authors
improve before release), not to the published score. This holds uniformly: every tool on the leaderboard, including
`anc` itself, is scored from binary behavior alone, so the comparison is like-for-like.

## Inputs: the seven statuses

Each behavioral requirement row carries a `status` and a `tier` (`must`, `should`, or `may`). The formula treats the
seven statuses in three groups:

| Status    | In denominator? | Execution credit | Meaning                                          |
| --------- | --------------- | ---------------- | ------------------------------------------------ |
| `pass`    | yes             | 1.0              | Behavior present and correct.                    |
| `warn`    | yes             | 0.5              | Behavior present, partially correct.             |
| `fail`    | yes             | 0.0              | Behavior expected, absent or broken.             |
| `opt_out` | yes             | 0.0              | Behavior deliberately declined (counts against). |
| `n_a`     | no              | —                | Inapplicable: a conditional antecedent is unmet. |
| `skip`    | no              | —                | Unmeasurable: the probe could not determine.     |
| `error`   | no              | —                | The probe raised an exception.                   |

`opt_out` is in the denominator on purpose: a deliberate decision not to adopt a behavior is a real signal worth
reflecting, distinct from a behavior that simply does not apply (`n_a`) or could not be measured (`skip`). The
distinction is the whole point of the seven-status taxonomy: it lets the score exclude genuinely inapplicable audits
without letting deliberate non-adoption hide.

## Formula

For a tool's set of behavioral rows, let `D` be the rows whose status is in `{pass, warn, fail, opt_out}` (the
denominator set). With per-tier weights `w(must)`, `w(should)`, `w(may)`:

```text
score_pct = round( 100 × Σ_{i∈D} w(tier_i) · credit(status_i)
                       ─────────────────────────────────────── )
                          Σ_{i∈D} w(tier_i)
```

where `credit(pass) = 1.0`, `credit(warn) = 0.5`, `credit(fail) = credit(opt_out) = 0.0`. A tool with an empty
denominator set scores 0.

### Tier weights (tunable)

The tier weights are a parameter, not a constant baked into the definition, so the standard can re-tune the balance
between MUST/SHOULD/MAY without redefining the formula. The current published weights are **flat**:

```text
w(must) = w(should) = w(may) = 1
```

Flat weights keep the score legible (every behavioral audit counts the same) and avoid over- or under-rewarding
optional-tier adoption relative to the mandatory baseline. Under flat weights the formula reduces to a simple
credit-weighted ratio:

```text
score_pct = round( 100 × (n_pass + 0.5 · n_warn) / (n_pass + n_warn + n_fail + n_opt_out) )
```

A future revision can move to non-flat weights (for example, weighting failures at the MUST tier more heavily). Such a
change is a re-tuning of a published parameter and ships through the normal versioned-release path, subject to the
stability commitment below.

### Worked example

A narrow filter that ships no structured output: 20 `pass`, 7 `warn`, 0 `fail`, 1 `opt_out`, 1 `n_a`, 14 `skip`.

- Denominator set `D` = the 20 + 7 + 0 + 1 = 28 rows that are pass/warn/fail/opt_out. The `n_a` and `skip` rows are
  excluded.
- Numerator (flat weights) = 20 × 1.0 + 7 × 0.5 + 0 + 1 × 0.0 = 23.5.
- `score_pct = round(100 × 23.5 / 28) = 84` → **Strong** band.

## Eligibility floor

**A tool is badge-eligible at `score_pct ≥ 70`.**

The floor is deliberately low. The badge's job is to spread the standard: a tool that clears a reasonable bar can
display it and point readers at its scorecard. Exclusivity is carried by the cohort bands below and by the score shown
on the badge itself, not by a high gate. A tool below the floor still gets a rendered badge and scorecard. The color
reflects the lower score, and the scorecard page shifts to improvement hints rather than the embed snippet (see
[`docs/badge.md`](../docs/badge.md#regression-behavior)).

## Cohort bands

The score maps to one of four cohort bands above the floor, plus a below-floor state. The bands are the exclusivity
signal. The top band is intentionally rare.

| Band          | Score   | Meaning                                               |
| ------------- | ------- | ----------------------------------------------------- |
| **Exemplary** | `≥ 85`  | Near-complete binary-behavior conformance. Rare.      |
| **Strong**    | `80–84` | Broad conformance with minor gaps.                    |
| **Solid**     | `75–79` | Solidly above the floor.                              |
| **Qualified** | `70–74` | Meets the eligibility floor.                          |
| _below floor_ | `< 70`  | Not yet eligible; badge renders muted improvement UX. |

The band thresholds are the spec-side contract; [`docs/badge.md`](../docs/badge.md) maps each band to a rendered color.
Exact colors are a rendering detail owned by the site.

## Stability commitment

The formula, tier weights, eligibility floor, and band thresholds are held stable for at least six months from
publication so that authors who embed the badge can rely on it. Any change ships through a versioned spec release with
the rationale recorded, never as a silent re-tuning.

## Cross-references

- [`AGENTS.md`](AGENTS.md): the seven-status taxonomy, per-row scorecard model, and antecedent propagation.
- [`docs/badge.md`](../docs/badge.md): badge claim, embed shapes, color rendering per band, regression behavior.
- RFC 2119 / RFC 8174: MUST / SHOULD / MAY tier semantics.
