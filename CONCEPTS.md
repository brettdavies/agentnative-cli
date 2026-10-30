# Concepts

> Shared domain vocabulary for this project: entities, named processes, and status concepts with project-specific
> meaning. Seeded with core domain vocabulary, then accretes as ce-compound and ce-compound-refresh process learnings;
> direct edits are fine. Glossary only, not a spec or catch-all.

## Audits

### Requirement

One MUST, SHOULD, or MAY statement from the agent-native spec's principles, identified by a stable id. Audits gather
evidence for requirements, and the scorecard reports a status for each requirement.

### Applicability

Whether a Requirement binds a given target. A universal requirement applies to every CLI; a conditional one applies only
under a stated condition, such as the CLI having subcommands. anc never grades on the condition's wording: the audits
that cover the requirement check the condition themselves, and the wording is there for readers.

A conditional requirement may also name an antecedent audit. When the antecedent finds the behavior absent or declined,
the requirement's row is not applicable; when the antecedent could not be measured or errored, the row takes that status
too; otherwise the row keeps the status its own audit gave it.

### Coverage matrix

The published table of every Requirement with its level, its Applicability, and the audits that evidence it, so a reader
can see which requirements anc verifies and which no audit covers. It is generated from the spec's text and each audit's
declared coverage, and a drift check fails when the published copy disagrees with either.

### Scorecard

The structured report one anc run produces for a target: a result row per Requirement with its status and evidence,
totals, and a description of the tool, the run, and the target. Its shape evolves additively, and consumers detect newer
fields rather than assume them.

The run description records when and how the scorecard was produced, so two scorecards of the same target always differ
in those fields even when every result row is identical.

### Audit

One check anc runs against a target CLI that reports a status with evidence as proof of one or more Requirements. Every
audit belongs to exactly one layer: a Behavioral audit, a Source audit, or a Project audit.

### Behavioral audit

An audit that runs the target's built binary and judges what it observably does, such as its help text, exit codes, and
flags, so it applies to a CLI written in any language.

### Option definition

A line in a tool's `--help` that declares one or more flag names, such as `-q, --quiet  Suppress output`,
`-V/--version`, or `--version (-V)`. A behavioral audit counts a flag only when an option definition declares it; a flag
named only in a usage line or in prose does not count, and the row's evidence says so. A single letter counts for a
meaning, such as `-q` for quiet, only when its definition also declares the long name or its description names that
meaning. The README's "How behavioral audits read `--help`" section lists the layouts anc reads.

### Source audit

An audit that reads a project's source files for code patterns, without running the binary, for the languages anc can
parse.

Each source file is parsed once per run, and every source audit that applies reads that one parse. Evidence an audit
gathers across several files is listed in file path order, so the same input produces the same output on every run and
at every thread count.

### Project audit

An audit that inspects a project's files and manifests, such as whether a file exists or what a manifest declares,
rather than its source code or its binary.

### Audit profile

A declared kind of tool, such as a TUI, a file-traversal utility, a POSIX utility, or a diagnostic tool, under which
some Requirements do not apply. The audits for those requirements still appear in the scorecard, marked skipped with the
profile named, and do no work. *Avoid:* exception category
