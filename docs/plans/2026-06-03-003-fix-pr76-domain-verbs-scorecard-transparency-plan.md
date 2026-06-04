---
title: "fix: PR #76 follow-up — scorecard transparency for .anc.toml domain_verbs; trim platform verbs"
type: fix
status: completed
priority: P1
date: 2026-06-03
origin: "Adversarial review of merged PR #76 (commit 9263c5c7) surfaced 5 findings; this plan keeps the escape hatch
  but makes domain-verb-assisted Passes visibly distinct in the scorecard JSON and trims platform-specific verbs out of
  the built-in list back into a documented social-CLI example"
---

# fix: PR #76 follow-up — scorecard transparency for .anc.toml domain_verbs; trim platform verbs

## Summary

PR #76 added two coupled changes: (a) 18 new "standard" verbs to the built-in `STANDARD_VERBS` list, and (b) a
`.anc.toml` config file at the audit target root that lets any CLI declare additional `[p6] domain_verbs` to extend the
list per-project. The combined effect is that any CLI can now Pass `p6-may-standard-names` by declaring its own
vocabulary as standard, and the scorecard JSON gives no signal that the Pass was self-declared rather than earned.

The user has chosen to keep the `.anc.toml domain_verbs` mechanism — it preserves the team's collaboration outcome and
sidesteps the harder spec-authority debate. This plan adds the missing transparency so a domain-verb-assisted Pass is
visibly distinct in the scorecard, trims the platform-specific verbs out of the built-in list back into a documented
social-CLI example, and pins the abuse cases via adversarial tests.

The deeper concern (per-CLI vocabulary is not blessed by the P6 spec, and `.anc.toml` is not consumed by downstream
agents at runtime — only by the audit) is parked. Plan #001 (audit philosophy) owns the spec-alignment question.

PR reference: <https://github.com/brettdavies/agentnative-cli/pull/76> (merged 2026-06-02).

## Findings inventory

The adversarial review surfaced 5 findings against this PR.

| #   | Severity | Title                                                                  | Disposition             |
| --- | -------- | ---------------------------------------------------------------------- | ----------------------- |
| 1   | P1       | Audit-identity collapse — Pass is byte-identical in scorecard JSON     | **Fixed here (R1, R2)** |
| 2   | P2       | Spec authority gap — P6 doesn't bless per-CLI vocabulary               | Parked → plan #001      |
| 3   | P2       | Verb-list dilution — 7 of 18 new verbs are platform-specific           | **Fixed here (R3, R4)** |
| 4   | P2       | No adversarial test for abused domain_verbs                            | **Fixed here (R5)**     |
| 5   | P2       | Discoverability anti-pattern — `.anc.toml` invisible to runtime agents | Parked → plan #001      |

## Scope

### In scope

- Scorecard transparency: `using_domain_verbs: bool` and `domain_match_count: usize` fields on the
  `p6-may-standard-names` audit row when domain_verbs were consulted; evidence string showing the built-in vs domain
  ratio.
- Trim 7 platform-specific verbs (`post`, `repost`, `unrepost`, `quote`, `like`, `unlike`, `dm`) out of
  `STANDARD_VERBS`. The remaining 11 additions (cross-domain verbs) stay.
- Document an example `.anc.toml` for social-CLI vocabulary at
  `docs/solutions/architecture-patterns/anc-toml-domain-verbs-pattern-2026-06-03.md`.
- Five adversarial unit tests covering: nonsense `domain_verbs` still Passes with the transparency flag; case-sensitive
  mismatch (`Post` does NOT match `post`); empty file, missing file, malformed TOML semantics; transparency fields
  absent when no `.anc.toml` is consulted.

### Out of scope

- Spec PR in `agentnative-spec` — plan #001 owns this. Per-CLI vocabulary is unblessed by the spec text today; this plan
  does not change that.
- Removing `.anc.toml domain_verbs` entirely.
- Monorepo / per-binary `.anc.toml` (one file per repo root remains the contract).
- Surfacing the merged verb list to downstream agents at runtime (the discoverability anti-pattern in finding #5).
- Validation of `domain_verbs` entries' shape (length cap, character set restrictions). Treat `domain_verbs = ["yeet",
  "bork"]` as a self-declaration the user owns; the scorecard signal is the safeguard.

## Requirements

- **R1**: When the audit Passes and at least one subcommand was recognized via `domain_verbs` (not via built-in
  `STANDARD_VERBS`), the scorecard JSON row for `p6-may-standard-names` includes:
- `using_domain_verbs: true`
- `domain_match_count: <N>` where `N` is the count of subcommands matched via domain_verbs
- Evidence string of the form `"X/Y subcommands standard (Z via .anc.toml [p6].domain_verbs: [verb1, verb2, …])"`

- **R2**: When the audit Passes and `domain_verbs` was not consulted (no `.anc.toml`, or `domain_verbs` was empty, or
  the file existed but no entry matched), `using_domain_verbs` and `domain_match_count` are absent from the scorecard
  row. The Pass evidence string is the existing built-ins-only form.

- **R3**: The following entries are removed from `STANDARD_VERBS`: `post`, `repost`, `unrepost`, `quote`, `like`,
  `unlike`, `dm`. These remain available via `.anc.toml domain_verbs` opt-in.

- **R4**: The remaining 11 PR-#76 additions stay in `STANDARD_VERBS`: `archive`, `unarchive`, `subscribe`,
  `unsubscribe`, `block`, `unblock`, `follow`, `unfollow`, `bookmark`, `mute`, `unmute`, `reply`. (Note: that's 12 with
  `reply`; verify the original PR list — adjust if PR #76 listed 11 cross-domain verbs vs. 12.)

- **R5**: Five new tests:
- `nonsense_domain_verbs_pass_with_transparency_flag` — `.anc.toml` containing `domain_verbs = ["yeet", "bork",
  "blarg"]` against a CLI whose subcommands match. Asserts Pass, `using_domain_verbs: true`, and the evidence string
  names all three domain verbs.
- `case_mismatch_domain_verbs_does_not_match` — `.anc.toml` containing `domain_verbs = ["Post"]` against a CLI with
  subcommand `post`. Asserts no match (consistent with the existing lowercased-subcommand semantics).
- `pass_without_anc_toml_omits_transparency_fields` — absent `.anc.toml`. Asserts `using_domain_verbs` absent from JSON,
  evidence string is built-ins-only form.
- `empty_domain_verbs_omits_transparency_fields` — `.anc.toml` exists, `[p6]` section exists, `domain_verbs = []`.
  Asserts same as the previous case.
- `social_cli_documented_example_passes` — integration test using the documented example `.anc.toml` from
  `docs/solutions/`. Asserts the platform-specific verbs route through `domain_verbs` and the row carries the
  transparency fields.

- **R6**: The documentation file `docs/solutions/architecture-patterns/anc-toml-domain-verbs-pattern-2026-06-03.md`
  exists and is referenced from the audit's Warn evidence string when the audit fails on a CLI with social-CLI shape (so
  authors discover the `domain_verbs` opt-in).

## Implementation Units

### U1. Extend audit return shape

`audit_standard_names(help: &Help, domain_verbs: &[String])` currently returns `AuditStatus`. Either extend it to return
a tuple `(AuditStatus, AuditExtras)` where `AuditExtras` carries `using_domain_verbs: bool` and `domain_match_count:
usize`, or store the extras as side-channel state on a `Result` struct. Prefer the tuple — explicit flow, no hidden
state.

### U2. Wire extras into AuditResult / scorecard JSON

Plumb the new fields into `AuditResult` (or a per-audit metadata block) so `src/scorecard/mod.rs` can emit them. Bump
the scorecard `schema_version` to `0.6`. The fields are additive and `null`/absent when not applicable, per the existing
scorecard always-present-null contract.

### U3. Construct evidence string with built-in/domain ratio

In the audit's `run()`, when at least one match was via `domain_verbs`, format evidence as `"X/Y subcommands standard (Z
via .anc.toml [p6].domain_verbs: [verb1, verb2])"`. When the Pass is built-ins-only, keep the existing form. When the
audit Warns, append a pointer to the documentation file (R6).

### U4. Trim platform-specific verbs from STANDARD_VERBS

Single edit in `src/audits/behavioral/standard_names.rs`. Remove the 7 entries named in R3. Reorder remaining entries
into the existing alphabetical structure.

### U5. Write documentation file

Create `docs/solutions/architecture-patterns/anc-toml-domain-verbs-pattern-2026-06-03.md`. The doc explains: what
`.anc.toml domain_verbs` is for; how to declare social-CLI vocabulary (`post`, `repost`, `like`, `dm`, etc.); the
case-sensitivity contract; how the scorecard surfaces the use. Reference the example from at least one place where a CLI
author would land: the audit's Warn evidence (R3).

### U6. Add five tests

Three unit tests in `standard_names.rs`, one integration test in `tests/standard_names_integration.rs`, and the
documented-example integration test. Use the existing fixture helpers; mock `Help` in unit tests and use temporary
directories for the integration tests.

## Open questions

- **Schema version bump.** Bump to `0.6`? Or wait for a coordinated 0.x → 1.0 boundary? Default: `0.6`, since the fields
  are additive and pre-1.0 schemas evolve additively under the documented contract.

- **xurl-rs impact.** xurl-rs's committed `.anc.toml` currently declares X/Twitter vocabulary. After R3 removes
  `post`/`repost`/etc. from built-ins, xurl-rs's `domain_verbs` becomes load-bearing for its Pass. Confirm with the
  xurl-rs team that committing `.anc.toml` to their repo is acceptable, and that the scorecard transparency flag is an
  acceptable signal alongside their Pass. (Coordination, not code.)

- **`using_domain_verbs` weight in `score_pct`.** The badge formula in `principles/scoring.md` treats every Pass as a
  full-credit pass regardless of mitigation. Should a `using_domain_verbs: true` Pass count as `credit(pass) = 1.0`
  unchanged, or as a partial credit (e.g., `0.75`)? Default: leave at 1.0 for this plan. Re-evaluate in plan #001's
  philosophy discussion — partial-credit scoring is a larger design question that touches `--audit-profile` suppression
  too.

- **Evidence-string length budget.** The proposed evidence form (`X/Y subcommands standard (Z via .anc.toml ...)`) could
  grow when many subcommands match via domain_verbs. Cap at first N=5 domain verbs in the string and append `…` if more?
  Default: yes, cap at 5 to keep `text` mode rendering tidy.

- **Documentation discoverability.** Should `anc audit . --output text` print the documentation URL in the Warn case for
  `p6-may-standard-names`? Or is that noise? Default: print it, since the audit's Warn-to-Pass remediation path is the
  documented `.anc.toml` mechanism — putting the URL in the agent's face when the audit fires is the right affordance.

## Acceptance

- All five new tests pass.
- `cargo test` total passes the post-PR-#76 baseline (817+ passing).
- `cargo clippy --all-targets -- -Dwarnings` clean.
- Scorecard schema bumps to `0.6`; `coverage/matrix.json` and any other artifacts regenerate cleanly.
- `anc emit coverage-matrix --check` passes.
- xurl-rs `.anc.toml` continues to enable `p6-may-standard-names` Pass for that CLI (with the new transparency flag
  set). xurl-rs side: a separate coordination task to commit `.anc.toml` to the xurl-rs repo if not already there.
- Documentation file exists and is reachable from the audit's Warn evidence.
- The seven removed verbs (`post`, `repost`, `unrepost`, `quote`, `like`, `unlike`, `dm`) no longer appear in the
  built-in `STANDARD_VERBS` array (asserted via test or static analysis).

## Notes for the implementer

- Conventional commit shape: `fix(p6-standard-names): scorecard transparency for domain_verbs; trim platform verbs`.
- The audit's `Confidence::Low` marker (existing) stays unchanged. Whether `Low` should also apply to
  domain-verb-assisted Passes is a plan-#001 discussion.
- `src/anc_toml.rs::P6Config { domain_verbs: Vec<String> }` is the structural source — extending it later (length caps,
  character validation) is out of scope here.
- Do not modify `src/principles/spec/principles/p6-composable-predictable-command-structure.md` in this PR. The spec
  alignment is plan #001's responsibility.
- Open the PR against `dev`. Before merging: run `anc audit .` to dogfood, and verify the `p6-may-standard-names` row is
  built-ins-only Pass (since `anc` itself has no `.anc.toml`).
