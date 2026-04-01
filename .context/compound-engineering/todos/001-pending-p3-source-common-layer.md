---
status: pending
priority: p3
issue_id: "001"
tags: [architecture, source-checks]
dependencies: []
---

# Add source/common/ layer for cross-language source checks

## Problem Statement

`dry_run.rs` currently lives in `project/` but reads parsed source files (`project.parsed_files()`) to search for
write/mutate keywords and `--dry-run` flag definitions. This is source analysis, not project structure inspection.
However, the keywords it searches for (`--delete`, `--deploy`, `"dry-run"`) are language-agnostic -- they'd work on
Python, Go, or Node source too.

There's no `source/common/` layer for cross-language source checks. Today `source/` is organized as `source/rust/` and
`source/python/` (empty). When a second cross-language source check emerges, extract `source/common/` and move `dry_run`
there.

## Findings

- `dry_run.rs` uses `project.parsed_files()` for plain text search (no ast-grep, no language-specific patterns)
- It's the only check that straddles the project/source boundary
- Current placement in `project/` works but is technically wrong per the layer taxonomy
- No other cross-language source checks exist yet -- YAGNI until there's a second one

## Proposed Solutions

1. **Create `source/common/` module** with its own `all_common_checks()` registry, collected for all languages
2. Move `dry_run.rs` from `project/` to `source/common/`
3. Update `all_source_checks()` in `source/mod.rs` to always include common checks regardless of language

## Recommended Action

_To be filled during triage._

## Acceptance Criteria

- [ ] `source/common/` module exists with `mod.rs` and `all_common_checks()`
- [ ] `dry_run.rs` moved from `project/` to `source/common/`
- [ ] `dry_run` layer metadata updated from `CheckLayer::Project` to `CheckLayer::Source`
- [ ] Common checks collected in orchestration loop for all detected languages
- [ ] All existing tests pass
- [ ] Triggered by: a second cross-language source check being needed

## Work Log

| Date | Session | Notes |
|------|---------|-------|
| 2026-04-01 | v0.1 completion | Identified during layer audit. Deferred per YAGNI -- only one cross-language source check exists. |
