---
title: Hand-Written Help Subcommand Parsing - Plan
type: fix
date: 2026-09-16
status: completed
topic: hand-written-help-subcommand-parsing
artifact_contract: ce-unified-plan/v1
artifact_readiness: implementation-ready
product_contract_source: ce-plan-bootstrap
execution: code
---

# Hand-Written Help Subcommand Parsing - Plan

## Goal Capsule

- **Objective:** A CLI whose help lists its commands under a hand-written `... commands:` heading gets graded on what it
  actually does. Today anc reports that such a tool has no subcommands, which is false, and fifteen audits either leave
  the score or answer on no evidence.
- **Means:** Recognize hand-written command headers, strip the binary-name prefix that begins each entry, and add one
  SHOULD check that reports the prefix as a legibility defect (KTD1, KTD2, KTD6).
- **Authority hierarchy:** Requirements (R-IDs) win on behavior. Key Technical Decisions (KTD-IDs) win on mechanism
  within their cited R constraints. Units carry only local deltas.
- **Execution profile:** Four units in two PRs to `dev`. PR #94 carries U3, U1 and U2 as three commits in that order, U3
  first because U1 amplifies the defect U3 guards. PR #95 carries U4 alone, stacked on #94, and spans two repos: the
  requirement lands upstream in spec PR brettdavies/agentnative#53, is vendored here at a recorded SHA, and only then
  can its audit compile.
- **Stop conditions:** Stop and report if the new parser returns anything other than single-token top-level names, since
  three MUST audits turn a multi-word name into a false failure (KTD2). Stop if `anc audit .` on this repo changes any
  P2 or P5 row, since the dogfood suite asserts neither fails.
- **Tail ownership:** The implementer owns each branch, the failing-first proof for every new test, and the PRs. Brett
  owns the spec decision in U4 and the merges.

---

## Product Contract

### Summary

Teach the help parser the hand-written command-block shape, strip the binary name from each entry, and keep the parser's
existing output contract of single-token top-level names. Collapse a duplicate copy of the parser onto the shared one.
Guard a substring matcher that the fix would otherwise turn into false failures. Add a SHOULD requirement that reports
binary-prefixed command lists.

### Problem Frame

`parse_subcommands` recognizes three exact headers and takes the first whitespace-separated token on each indented line.
A hand-written block defeats both halves. herdr heads its block `Common commands:`, and every entry begins with the
binary name, so even a recognized header would yield `herdr` repeated rather than `status`, `server` and `machine`.

The result is not a gap, it is a false statement. Fifteen registered behavioral audits depend on subcommand parsing,
eleven directly and four through shared helpers. When the list comes back empty ten degrade to `Skip`, which is excluded
from the score denominator, so they leave the calculation rather than counting against the tool. The other five stay in
it on no evidence: one fails a MUST, two pass vacuously, and two warn. One audit then emits "binary has no subcommands"
as a fact about the target while another says "no subcommands parsed from --help" about itself. For a tool whose product
is telling people true things about their CLI, the false assertion is the defect and the wrong score is downstream of
it.

This is the third instance of one pathology in this codebase: a heuristic tuned to one vendor's output shape, applied to
arbitrary third-party text, failing silently with misleading evidence. The recorded sibling is
`docs/solutions/workflow-issues/anc-pager-substring-false-positive-2026-06-02.md`, where bare substring matching on
`less`, `more` and `pager` flips an audit on any CLI whose help says "headless mode". Its filed P0 is still open and the
matcher is still naive, so prose alone has not held here either.

### Key Decisions

- KD1. **Binary-prefixed command lists are a SHOULD-level legibility defect, not a MUST** (session-settled:
  user-directed — chosen over MUST-fails and over optional-advisory: the behavioral layer already grades with warns
  rather than fails, and the repo's own pager learning records that a strict rule penalizes the population anc exists to
  grade). Governs R7, R8.
- KD2. **The new check fires on the binary-name prefix specifically, not on unextractable command blocks generally**
  (session-settled: user-directed — chosen over a general rule and over shipping both: a general rule grades the target
  on anc's own parser strength, which is exactly how the pager defect went wrong). Governs R7.

### Requirements

**Parser behavior**

- R1. The parser recognizes a command-block header whose final word is `commands:` or `subcommands:`,
  case-insensitively, so `Common commands:`, `Advanced commands:` and cobra's `Available Commands:` open a block
  alongside clap's `Commands:`. Source: `src/runner/help_probe/mod.rs`.
- R2. When every entry in a block begins with the tool's own name, taken from the `Usage:` line or from the binary's
  file stem, the parser strips it and reads the command from what follows. The test is per block and has no minimum
  block size, so a one-entry block strips on its own terms, a block with a single unprefixed entry is left alone, and a
  one-entry clap block is never read as prefixed because its only command is not the tool's name. Source:
  `src/runner/help_probe/mod.rs`.
- R3. The parser returns single-token top-level names and nothing else. A nested entry contributes its top-level name
  only, duplicates collapse, and an entry that yields no name after stripping is dropped. The bare-invocation entry,
  which carries the binary name and no command, is one such entry. A line indented more deeply than the block's first
  entry continues the previous entry, a wrapped description or a nested subcommand, and contributes nothing.
- R4. A token that opens a placeholder, beginning with `<` or `[`, terminates the name rather than becoming one.
- R5. `.subcommands()` keeps its current type and meaning, so no consumer changes.
- R5a. A separate accessor exposes what the name list cannot carry: whether a command block was found, its raw entry
  lines, and the binary-name prefix when one was stripped. R7's evidence and R10's wording both read it.

**One parser**

- R6. One implementation parses command blocks. The second copy behind the JSON-output audit is replaced by a call to
  it. Source: `src/audits/behavioral/json_output.rs:110`.

**The new check**

- R7. A new SHOULD requirement reports a command block whose entries repeat the binary name, with evidence naming the
  offending lines and remediation saying to drop the prefix. The bare-invocation entry is not an offender: documenting
  what the tool does with no arguments is legitimate, and only prefixed commands are the defect. The remediation states
  the benefit to the graded tool: a reader or agent scanning the block gets the command token directly instead of
  reconstructing it from a repeated binary name.
- R8. The check warns. It never fails, and a CLI with no command block is not applicable rather than failing.

**Not amplifying a latent defect**

- R9. A destructive verb matches only at the start of the name or the start of a `-` or `_` delimited segment, so
  `dropdb`, `rmdir`, `cleanup`, `delete-all` and `force-push` stay destructive while `format`, `transform`, `perform`,
  `confirm` and `firmware` do not. Source: `src/audits/behavioral/destructive_ops.rs:36-39`.

**Evidence honesty**

- R10. An audit that finds no subcommands says whether it parsed none or the tool has none, and never asserts the latter
  when it means the former.

### Success Criteria

- Auditing herdr reports its real top-level commands, evaluates the fifteen dependent audits on real names, and raises
  one warn for the binary-name prefix.
- `anc audit .` against this repo changes no P2 or P5 row, which the dogfood suite already asserts.
- No CLI gains a MUST failure that is not a true statement about it. A MUST that fails for the first time because the
  audit finally has real names to judge is a correct verdict, not a regression.
- Every currently published tool is re-audited before release, each badge-eligibility crossing is recorded, and PRs #94
  and #95 describe the score movement under the changelog's Changed heading.

### Scope Boundaries

- No change to what any of the fifteen audits assert, beyond the destructive-verb correction in R9 and the evidence
  wording in R10.
- Second-level verb discovery stays as it is. Four gates would read better from nested verbs, and they read the flat
  list today; changing that is its own decision.
- The UTF-8 evidence panic is planned separately in
  `docs/plans/2026-09-16-1501-fix-utf8-safe-evidence-previews-plan.md`.

#### Deferred to Follow-Up Work

- The pager substring matcher, the recorded sibling of this defect, still matching `less`, `more` and `pager` without
  word boundaries. R9 fixes the same class in one place; the pager check is the other and is not amplified by this work.
- Teaching the four flat-list gates to read nested verbs, so a persistent-session tool matches on `server start` rather
  than missing because it read `server`.
- A broader non-clap help fixture set. `tests/fixtures/handwritten-help/tally` is the one hand-written fixture the suite
  carries; the absence of any such fixture is why the defect shipped.
- Tools that enumerate commands only in a `Usage:` block, with no `commands:` heading at all. herdr carries both shapes,
  so the alternative source sits in the motivating example; this plan does not read it.
- Fused destructive verbs such as `autoremove` and `autoclean`, which substring matching caught and segment-prefix
  matching does not. Adding them to the verb list with a failing-first test is the fix if recall matters.
- Cobra group headers with a parenthetical before the colon (`Basic Commands (Beginner):`, as kubectl prints), which the
  last-word rule does not recognize, so such tools parse a partial name set.
- The release-time re-audit of every published tool, recording each badge-eligibility crossing, which the next release's
  preflight owns.
- A spec release that includes `p3-should-unprefixed-command-list`, followed by the default tag sync, before the next
  CLI release: the vendored `VERSION` still reads 0.5.0, which the scorecard reports as `spec_version`, while the
  published v0.5.0 spec has 59 requirements.

### Sources

- `docs/solutions/workflow-issues/anc-pager-substring-false-positive-2026-06-02.md`: the recorded sibling defect, its
  silent-evidence failure mode, and the finding that it penalizes anc's own target population.
- `src/scorecard/mod.rs:232`: `Skip`, `NotApplicable` and `Error` are excluded from the score denominator, which is why
  ten degraded audits vanish rather than score zero. `OptOut` is not excluded and scores zero.
- `tests/standard_names_integration.rs:42-60`: the end-to-end fixture template that stages a binary and runs real `anc`.
- `RELEASES-PREFLIGHT.md:163`: `anc emit coverage-matrix --check` is a drift guard, so U4 must regenerate the matrix.
- Consumer review, completed during planning: all fifteen consumers were read for shape assumptions. Every name
  comparison is exact or case-insensitive equality except the destructive-verb predicate R9 corrects, and three call
  sites pass names back as a single argv element, which is what forecloses multi-word names. No second latent assumption
  was found.

---

## Planning Contract

### Key Technical Decisions

- KTD1. **Recognize the header by its last word, not by an allowlist.** A block header ending in `commands:` covers
  `Common commands:`, `Advanced commands:` and the three known forms without enumerating vendors. Instantiates R1.
- KTD2. **Strip the binary name, then take the first remaining token.** This is the whole extraction rule. It yields
  `status` from `herdr status [server|client]`, `server` from `herdr server stop`, `machine` from `herdr machine
  <subcommand>`, and nothing from the bare `herdr` line. The invocation is the text before the two-space description
  gap; in a prefixed block, a bare entry whose description follows a single space (`tool Launch the app`) is read as
  description only, since commands are lowercase by convention and sentences are capitalized. Dropping the bare line
  costs no coverage: bare-invocation behavior is probed by executing the binary with no arguments in
  `src/audits/behavioral/flag_existence.rs:71` and `src/audits/behavioral/non_interactive.rs:73`, never by reading it
  out of the help text. The rule preserves the existing output contract exactly, so none of the fifteen consumers
  changes. Rejected: returning `server stop` as one name, which three MUST audits turn into a false failure because they
  compare names by equality and pass them back as a single argv element; and splitting into `server` plus `stop`, which
  invents a phantom top-level command that two audits would then probe and score. Instantiates R2, R3, R5.
- KTD3. **Prefix agreement is unanimous within a block, not a majority across the help text, and the agreed token must
  be the tool's own name.** Every entry in the block must lead with the `Usage:` line's tool name or the binary's file
  stem before anything is stripped, and each block is judged on its own. Unanimity keeps a genuine command that shares
  the tool's name from being eaten: a two-entry block where only one entry leads with the binary name fails the test and
  is left intact. Judging per block lets a one-entry block strip correctly, which herdr needs for its second,
  single-entry block, and requiring the tool's name keeps a one-entry clap block from reading its only command as a
  prefix. Both candidates are needed because a usage line can lead with a launcher (`npx tool`) or a suffixed file name
  (`tool.exe`) while the entries lead with the bare name. Rejected: a majority threshold, which strips that two-entry
  block wrongly and forces an arbitrary minimum size; and agreement on any shared token, which mis-strips the one-entry
  clap block. Instantiates R2.
- KTD4. **Keep `is_subcommand_name` as the validity gate.** It already rejects angle brackets, so no consumer has ever
  seen a placeholder and none sanitizes one. Any relaxation would put `<subcommand>` into Warn and Fail evidence and
  into three argv spawns. Instantiates R4.
- KTD5. **The JSON-output audit calls the shared parser.** Its private copy carries both defects and no name validation,
  and it drives an opt-out that collapses two P2 requirements to not-applicable, so the divergence has a scorecard-level
  consequence rather than a local one. It also carries a third divergence the swap must preserve: it drops the `help`
  entry while the shared parser keeps it. Left unhandled, the audit would probe `<bin> help --help` and can move a MUST
  row on any tool whose `help` subcommand echoes top-level help, so the swap filters `help` at the call site using the
  skip list the subcommand-help helper already defines. Instantiates R6.
- KTD6. **The new check reads the same parsed block the parser produces, and reports lines rather than counts.**
  Remediation names the prefix and the fix. Instantiates R7, R8, and KD1 and KD2 (R7, R8).
- KTD8. **The requirement is authored upstream and vendored, never edited in place** (session-settled: user-directed —
  chosen over splitting U4 into its own plan and over deferring it: the plan stays one artifact and the cross-repo
  sequencing is stated rather than discovered). The requirements table is generated at build time from vendored spec
  frontmatter, and the build fails when an audit's `covers()` id does not resolve, so the upstream merge gates the
  audit. Instantiates R7.
- KTD7. **Segment-prefix destructive matching lands before the parser fix.** `is_destructive` substring-matches `rm`, so
  `format`, `transform`, `perform` and `confirm` classify as destructive today. That cannot fire on a hand-written-help
  CLI because no names are parsed. U1 makes it fire, turning a MUST audit into a false failure on real tools. The guard
  is a correction this work forces, not an opportunistic cleanup. Instantiates R9.

### System-Wide Impact

- Scores move for every already-audited CLI with a hand-written command block. Ten audits re-enter the denominator and
  five that were answering on no evidence start answering on real names, so the direction is not predictable in advance
  and is not a regression when it falls.
- U4's new SHOULD row enters the denominator for every CLI that has a command block, not only prefixed ones: a
  full-credit pass for clap-shaped tools, a half-credit warn for prefixed ones. Because the badge floor is a percentage,
  a published third party sitting near it can change badge eligibility with no change to its own behavior.
- The JSON-output opt-out currently collapses two P2 requirements to not-applicable for these tools. After U2 those
  requirements are evaluated.
- U4 adds a requirement and an audit, so `docs/coverage-matrix.md` and `coverage/matrix.json` must be regenerated or the
  release preflight fails.

### Risks & Dependencies

- **The fix amplifies a live defect.** Shipping U1 before U3 introduces false MUST failures wherever a command name
  contains `rm` as a substring. The sequencing is the mitigation and the stop condition names it.
- **A block mixing prefixed and unprefixed entries is left unstripped.** Unanimity is deliberate, so a tool whose block
  carries one prose line among its commands parses as it does today rather than partially. The fixture set should carry
  that shape.
- **Stale attributes mislead the next reader.** Four `#[allow(dead_code)]` sites in the help probe are stale, verified
  by forcing the lint, and two doc comments state that no behavioral audit consumes the parser when fifteen do.

### Sequencing

U3 first, because U1 amplifies the defect it guards. Then U1, the parser itself. Then U2, which can only collapse onto a
parser that already works. U4 last, since the new check reads U1's output and its requirement must merge upstream and be
vendored before the audit compiles.

---

## Implementation Units

### U3. Segment-prefix destructive verbs

- **Goal:** A command whose name merely contains a destructive verb mid-word is not classified destructive, while
  compound names that begin with one still are.
- **Requirements:** R9; KTD7.
- **Dependencies:** none.
- **Files:** `src/audits/behavioral/destructive_ops.rs`.
- **Approach:**
  1. Replace the substring test in `is_destructive` with a segment-prefix test: split the lowercased name on `-` and
     `_`, and match a verb only where it begins the name or begins a segment.
  2. Keep the `force-` prefix entry working, which already encodes a prefix.
  3. Leave the existing `substring_match_for_compound_names` assertion green rather than deleting it; segment-prefix
     satisfies both `delete-all` and `dropdb`, which is why this rule was chosen over whole-word matching.
- **Execution note:** Write the failing cases first against the current matcher and quote the output; `format` and
  `confirm` classify as destructive today.
- **Patterns to follow:** the recorded pager-substring learning in `docs/solutions/workflow-issues/`, which is the same
  defect in the sibling matcher.
- **Test scenarios:**
  - `format`, `transform`, `perform`, `confirm` and `firmware` are not destructive.
  - `delete`, `remove`, `rm`, `purge`, `reset-keys` and `force-push` remain destructive.
  - `dropdb`, `rmdir`, `cleanup` and `purgeall` remain destructive, which whole-word matching would have lost.
  - `config reset-keys` classifies on the `reset` segment, not on a substring accident.
- **Verification:** the destructive-ops, force-yes and read-write-distinction suites pass, the last because the
  write-verb predicate delegates to this function; `anc audit .` shows no P5 row change.

### U1. Hand-written command blocks

- **Goal:** The parser reads a hand-written command block and returns the same shape it returns for clap.
- **Requirements:** R1, R2, R3, R4, R5, R10; KTD1, KTD2, KTD3, KTD4.
- **Dependencies:** U3.
- **Files:** `src/runner/help_probe/mod.rs`, `src/runner/mod.rs`, `src/audits/behavioral/subcommand_examples.rs`,
  `src/audits/behavioral/standard_names.rs`, `tests/integration.rs`, `tests/fixtures/handwritten-help/tally` (new).
- **Approach:**
  1. Recognize a header whose final word is `commands:`, case-insensitively, keeping the three existing forms.
  2. Detect a binary-name prefix by agreement across the block's entries, then strip it per entry.
  3. Take the first remaining token, stopping at a token that opens a placeholder, and validate it with the existing
     name gate. Collapse duplicates and drop entries that yield nothing.
  4. Remove the four stale `#[allow(dead_code)]` attributes and correct the two doc comments that claim no audit
     consumes the parser.
  5. Add the block accessor of R5a: `command_blocks()` returns each block's header, entry lines and stripped prefix,
     `command_text()` gives an entry's invocation minus the prefix, and `missing_subcommands_reason()` words an empty
     parse; `.subcommands()` is untouched.
  6. Correct the evidence wording so an audit distinguishes an unparsed block from an absent one, reading the accessor
     rather than re-deriving it from raw help text (R10).
- **Execution note:** Start from a failing test using herdr's real block shape; observe the empty result first.
- **Patterns to follow:** the inline `const *_HELP` fixture style in this module's test mod; the end-to-end fixture
  script in `tests/standard_names_integration.rs:42-60` for the integration case.
- **Test scenarios:**
  - A herdr-shaped block returns `status`, `update`, `completion`, `server`, `channel`, `config`, `machine`, `api` and
    the rest, once each, with no `herdr` entry.
  - `herdr server stop` contributes `server` only; `herdr channel set <stable|preview>` contributes `channel` only.
  - `herdr machine <subcommand>` contributes `machine`; no name contains a bracket.
  - The bare binary-name line contributes nothing, and the two audits that probe bare invocation still observe it by
    execution.
  - A clap block is parsed exactly as it is today, proving no regression.
  - A block whose entries do not share a prefix is read without stripping.
  - A two-entry block where one command equals the binary name does not read as prefixed.
  - A localized help block still degrades to empty rather than inventing names.
  - An audit that parsed nothing reports that it parsed nothing, rather than asserting the tool has no subcommands.
- **Verification:** the help-probe and behavioral suites pass; auditing the new fixture reports its commands; `anc audit
  .` changes no row on this repo.

### U2. One command-block parser

- **Goal:** The JSON-output audit uses the shared parser instead of its own copy.
- **Requirements:** R6; KTD5.
- **Dependencies:** U1.
- **Files:** `src/audits/behavioral/json_output.rs`, `src/audits/behavioral/subcommand_help.rs` (its skip list shared
  with the JSON-output audit).
- **Approach:**
  1. Delete the private `parse_subcommand_names` and call the shared parser.
  2. Confirm the opt-out path and its antecedent propagation behave unchanged when the list is genuinely empty.
- **Patterns to follow:** the shared-helper rule in `CLAUDE.md` § Source Audit Convention.
- **Test scenarios:**
  - A hand-written block now yields names here too, so the audit evaluates rather than opting out.
  - A tool with no command block still opts out, and the two dependent P2 requirements still collapse to not-applicable.
  - A `Commands:` block yields the same names after the swap once `help` is filtered at the call site.
  - A tool whose `help` subcommand echoes top-level help is not probed at `help`, so its output-flag row is unchanged.
- **Verification:** the JSON-output suite passes; no scorecard row changes for a clap-shaped fixture.

### U4. The binary-prefix legibility check

- **Goal:** A command block that repeats the binary name is reported once, at SHOULD, with actionable remediation.
- **Requirements:** R7, R8; KTD6, KTD8, and KD1 and KD2.
- **Dependencies:** U1, and the upstream requirement in the `brettdavies/agentnative` spec repo, merged there through
  brettdavies/agentnative#53 as `8b84077`; the vendored copy here is byte-identical to that commit.
- **Files:** `src/principles/spec/principles/p3-progressive-help-discovery.md` (vendored),
  `src/audits/behavioral/mod.rs`, `src/audits/behavioral/unprefixed_command_list.rs` (new), `src/runner/mod.rs`
  (`CommandBlock` re-exported), `src/principles/registry.rs` (counter assertions only), `tests/build_parser.rs`
  (vendored-spec count), `tests/integration.rs`, `docs/coverage-matrix.md`, `coverage/matrix.json`.
  `src/principles/spec/VERSION` stays at 0.5.0 until the next spec release.
- **Approach:**
  1. Land the SHOULD requirement in the spec repo's P3 principles frontmatter, following the id shape of the existing P3
     entries, then vendor it here with the sync script at a pinned ref and record the resolved SHA in the PR body. The
     requirements table is generated from that vendored frontmatter at build time, so editing the registry directly is
     overwritten by the next sync.
  2. Add the behavioral audit `p3-unprefixed-command-list`, covering `p3-should-unprefixed-command-list`, warning when a
     block's entries repeat the binary name and resolving not-applicable when no subcommand names were parsed, the gate
     `p3-subcommand-examples` and `p6-standard-names` share. An examples block (`Example commands:`) parses like any
     other but is not graded, since examples are where the spec expects full invocations.
  3. Register the audit, bump the two registry counter assertions (total requirements and the SHOULD count), and update
     the matching requirement-count prose in the coverage matrix, which the repo's instructions treat as one deliberate
     paired act.
  4. Regenerate the coverage matrix.
- **Execution note:** This is the only unit that changes the spec surface, so it lands alone and its PR body carries the
  requirement text.
- **Patterns to follow:** the P3 audits already in `src/audits/behavioral/`, particularly the id and tier shape of
  `p3-should-paired-examples`.
- **Test scenarios:**
  - A herdr-shaped block warns, and the evidence names the offending lines.
  - A clap-shaped block passes.
  - A tool with no command block resolves not-applicable rather than warning.
  - A block where one entry coincidentally starts with the binary name does not warn.
  - A block whose only binary-name-only entry is the bare-invocation line does not warn, and that line is absent from
    the evidence when other entries do offend.
  - `anc emit coverage-matrix --check` exits zero after regeneration.
- **Verification:** the new suite passes; the coverage matrix is regenerated and clean; `anc audit .` on this repo does
  not warn on its own help.

---

## Verification Contract

| Gate                 | Command                                           | Proves                                                                                                                              |
| -------------------- | ------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| Format               | `cargo fmt --check`                               | Formatting matches the repo.                                                                                                        |
| Lint                 | `RUSTFLAGS=-Dwarnings cargo clippy --all-targets` | No warnings, including after the stale allows are removed.                                                                          |
| Unit and integration | `cargo test`                                      | Parser, audit and fixture suites pass.                                                                                              |
| Dogfood              | `cargo test --test dogfood`                       | No P2 or P5 failure when anc audits itself.                                                                                         |
| Fixture audit        | `anc audit tests/fixtures/handwritten-help/tally` | Real commands reported, one prefix warning, and any MUST failure is a correct verdict on the fixture rather than a parser artifact. |
| Coverage drift       | `anc emit coverage-matrix --check`                | The matrix matches the registry after U4.                                                                                           |
| Supply chain         | `cargo deny check`                                | Unchanged dependency posture.                                                                                                       |

---

## Definition of Done

Global:

- Each new test was observed failing against the unfixed code, and the failure output is quoted in the PR.
- No comment in the diff narrates the change; comments state present behavior.
- No abandoned-approach code remains in the diff.
- Each PR fills `.github/pull_request_template.md`, with user-visible effects under the Changelog block's `### Fixed`
  and, for U4, `### Added`.

| U-ID | Done when                                                                                                                                                                     |
| ---- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| U3   | Words containing a destructive verb as a substring are not destructive; the verbs themselves still are.                                                                       |
| U1   | A hand-written block yields correct single-token top-level names; a clap block is unchanged; the stale attributes and wrong doc comments are gone; an unparsed block says so. |
| U2   | One parser serves both call sites; the opt-out path is unchanged for tools with no block.                                                                                     |
| U4   | The SHOULD requirement and its audit exist and are registered; the matrix is regenerated; herdr warns once and anc does not.                                                  |
