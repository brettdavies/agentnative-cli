//! Flat `&'static [Requirement]` registry covering every MUST, SHOULD, and
//! MAY across P1–P7. The registry is the single source of truth linking
//! spec requirements to the checks that verify them via `Check::covers()`.
//!
//! IDs follow the pattern `p{N}-{level}-{key}`. They are stable and must
//! not change once published — scorecards and the coverage matrix pin
//! against them.

use serde::Serialize;

/// Severity level of a spec requirement.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Must,
    Should,
    May,
}

/// Whether a requirement applies to every CLI or only when a condition holds.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "condition", rename_all = "lowercase")]
pub enum Applicability {
    Universal,
    Conditional(&'static str),
}

/// Categories under which a tool may be exempt from specific requirements.
/// Referenced by scorecard `audit_profile`.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ExceptionCategory {
    /// TUI-by-design tools (lazygit, k9s, btop). Interactive-prompt MUSTs
    /// suppressed; TTY-driving-agent access is out-of-scope for verification.
    HumanTui,
    /// File-traversal utilities (fd, find). Subcommand-structure SHOULDs
    /// relaxed; these tools have no subcommands by design.
    FileTraversal,
    /// POSIX utilities (cat, sed, awk). Stdin-as-primary-input is their
    /// contract; P1 interactive-prompt MUSTs satisfied vacuously.
    PosixUtility,
    /// Diagnostic tools (nvidia-smi, vmstat). No write operations, so P5
    /// MUSTs do not apply.
    DiagnosticOnly,
}

impl ExceptionCategory {
    /// Kebab-case identifier that matches the serde representation used by
    /// both the CLI (`--audit-profile human-tui`) and the scorecard JSON
    /// (`"audit_profile": "human-tui"`). Kept as a dedicated method so
    /// callers don't have to round-trip through `serde_json` to stringify.
    pub fn as_kebab_case(&self) -> &'static str {
        match self {
            ExceptionCategory::HumanTui => "human-tui",
            ExceptionCategory::FileTraversal => "file-traversal",
            ExceptionCategory::PosixUtility => "posix-utility",
            ExceptionCategory::DiagnosticOnly => "diagnostic-only",
        }
    }

    /// One-line human description. Surfaces in `coverage/matrix.json`
    /// under the `audit_profiles` section so agents + site renderers can
    /// explain each category without re-deriving semantics from the
    /// kebab-case name.
    pub fn description(&self) -> &'static str {
        match self {
            ExceptionCategory::HumanTui => {
                "TUI-by-design tools (lazygit, k9s, btop). Interactive-prompt MUSTs \
                 suppressed; the TTY-driving contract is out of scope for verification."
            }
            ExceptionCategory::FileTraversal => {
                "File-traversal utilities (fd, find). Subcommand-structure SHOULDs \
                 relaxed; these tools have no subcommands by design."
            }
            ExceptionCategory::PosixUtility => {
                "POSIX utilities (cat, sed, awk). Stdin-as-primary-input is their \
                 contract; P1 interactive-prompt MUSTs satisfied vacuously."
            }
            ExceptionCategory::DiagnosticOnly => {
                "Diagnostic tools (nvidia-smi, vmstat). No write operations, so the \
                 P5 mutation-boundary MUSTs do not apply."
            }
        }
    }
}

/// Every `ExceptionCategory` variant in order. Anchor for parity drift
/// tests (CLI `AuditProfile` must stay isomorphic) and for callers that
/// need to iterate the full set (suppression-table drift check,
/// `coverage/matrix.json` audit_profile section).
///
/// A new variant on the enum is a breaking plan change — land it in
/// `docs/plans/`, update this slice, update `SUPPRESSION_TABLE`, update
/// `AuditProfile`, and regenerate completions. The drift tests below and
/// in `src/cli.rs` tie all four sites together.
pub const ALL_EXCEPTION_CATEGORIES: &[ExceptionCategory] = &[
    ExceptionCategory::HumanTui,
    ExceptionCategory::FileTraversal,
    ExceptionCategory::PosixUtility,
    ExceptionCategory::DiagnosticOnly,
];

// Compile-time guard that the slice above covers every variant. If a
// new variant is added without updating ALL_EXCEPTION_CATEGORIES the
// match is non-exhaustive and the build breaks — making this drift
// impossible to merge rather than "test should catch it."
#[allow(dead_code)]
const fn _all_categories_covers_every_variant(c: ExceptionCategory) -> bool {
    match c {
        ExceptionCategory::HumanTui
        | ExceptionCategory::FileTraversal
        | ExceptionCategory::PosixUtility
        | ExceptionCategory::DiagnosticOnly => true,
    }
}

/// Prefix of the structured evidence string emitted for any check suppressed
/// by `--audit-profile`. The full evidence takes the shape
/// `"suppressed by audit_profile: <kebab-case-category>"`. This is the single
/// source of truth — `main.rs` (producer), `scorecard::audience` (consumer
/// sniffer), and the `scorecard::build_coverage_summary` filter all reference
/// this constant so a rename can't silently desync the three sites.
///
/// Consumers outside this crate (the integration test asserting the literal,
/// downstream site renderers) pin against the stable string shape — treat any
/// edit here as a consumer-contract change.
pub const SUPPRESSION_EVIDENCE_PREFIX: &str = "suppressed by audit_profile: ";

/// Which check IDs each exception category suppresses. When a category
/// applies, the listed checks emit `CheckStatus::Skip` with structured
/// evidence (`"suppressed by audit_profile: <category>"`) instead of
/// running — they appear in `results[]` so readers see what was excluded.
///
/// Entries map to *check* IDs, not requirement IDs, because the runtime
/// suppression point has `check.id()` in hand. The conceptual exemption is
/// a requirement — e.g., TUI apps are exempt from
/// `p1-must-no-interactive` — but because each requirement may be covered
/// by multiple checks across layers, the table enumerates every covering
/// check explicitly so the suppression behavior is deterministic.
///
/// **Every `ExceptionCategory` variant appears here**, even with an empty
/// slice. A missing category would silently no-op at the call site and
/// degrade to running every check — the drift test below catches the gap.
///
/// Every listed check ID is validated against the behavioral/source/project
/// catalog at test time; a typo or rename breaks the build.
///
/// # Trust boundary
///
/// The CLI accepts `--audit-profile <category>` from the caller without
/// validating that the target tool actually fits the declared category.
/// A broken CLI can self-declare `--audit-profile human-tui` and silently
/// mask the P1 interactive-prompt MUSTs + `p6-sigpipe` that would
/// otherwise Fail. This is intentional: the CLI only knows what it was
/// told, and hard-coding per-tool category detection would entangle the
/// repo-agnostic CLI with a tool registry it deliberately doesn't own.
/// Guarding against caller-chosen miscategorization is an upstream
/// concern (site's regen script looks up each tool's declared profile;
/// CI policy gates reviewer attention on registry changes). See also the
/// drift test in `src/cli.rs` pinning `AuditProfile` ↔ `ExceptionCategory`
/// parity and the `audit_profiles` section of `coverage/matrix.json`
/// publishing the full mapping.
///
/// # Drift test scope
///
/// The `suppression_table_check_ids_exist_in_catalog` test below verifies
/// that every listed check ID resolves to a real catalog entry — typos
/// surface at build time. It does *not* assert that each ID is
/// *semantically appropriate* for its category (e.g., a typo that
/// accidentally moves `p2-json-output` into `HumanTui` would still pass
/// because `p2-json-output` exists). At v0.1.3's 4 committed categories
/// the per-category slice is short enough for eyeball review; revisit a
/// per-category snapshot assertion if the table grows.
pub static SUPPRESSION_TABLE: &[(ExceptionCategory, &[&str])] = &[
    (
        ExceptionCategory::HumanTui,
        &[
            // p1-must-no-interactive — TUI apps intercept the TTY by design;
            // their whole contract is interactive. All three covering checks
            // suppress together for consistency.
            "p1-non-interactive",
            "p1-flag-existence",
            "p1-non-interactive-source",
            // p1-should-tty-detection — satisfied vacuously by the TUI
            // contract (the app's event loop is its TTY handler).
            "p1-tty-detection-source",
            // p6-must-sigpipe — TUIs routinely install their own signal
            // handlers to redraw or exit cleanly; the default-disposition
            // check doesn't match the category's execution model.
            "p6-sigpipe",
        ],
    ),
    (
        ExceptionCategory::FileTraversal,
        &[
            // No current check verifies subcommand-examples or
            // subcommand-operations for tools-without-subcommands. The
            // `If: CLI uses subcommands` applicability on existing checks
            // already produces the right Skip outcome for fd/find-style
            // tools. Kept as a table entry so future checks can be added
            // without a schema change.
        ],
    ),
    (
        ExceptionCategory::PosixUtility,
        &[
            // p1-must-no-interactive — POSIX utilities use stdin as the
            // primary input, so the interactive-prompt MUST is satisfied
            // vacuously rather than needing a --no-interactive flag.
            "p1-non-interactive",
            "p1-flag-existence",
            "p1-non-interactive-source",
        ],
    ),
    (
        ExceptionCategory::DiagnosticOnly,
        &[
            // p5-must-dry-run — diagnostic tools perform no writes, so the
            // write-safety MUSTs do not apply. Dry-run is the only P5 check
            // currently covered; read-write-distinction and force-yes are
            // still uncovered in v0.1.3.
            "p5-dry-run",
        ],
    ),
];

/// Whether `check_id` should be suppressed under the given `category`.
/// Returns `false` for unknown check IDs and for categories whose table
/// entry is empty. O(n) in the per-category slice — the table is small
/// and the call site runs once per check per invocation.
pub fn suppresses(check_id: &str, category: ExceptionCategory) -> bool {
    SUPPRESSION_TABLE
        .iter()
        .find(|(cat, _)| *cat == category)
        .is_some_and(|(_, ids)| ids.contains(&check_id))
}

/// A single spec requirement. The flat registry below is iterated by the
/// matrix generator and cross-referenced against `Check::covers()`.
#[derive(Debug, Clone, Serialize)]
pub struct Requirement {
    pub id: &'static str,
    pub principle: u8,
    pub level: Level,
    pub summary: &'static str,
    pub applicability: Applicability,
}

// REQUIREMENTS and SPEC_VERSION are generated at build time from vendored
// frontmatter under `src/principles/spec/principles/`. See `build.rs` and
// `build_support/parser.rs` for the pipeline; the generated file carries
// its own doc comments for the sort contract and version source.
include!(concat!(env!("OUT_DIR"), "/generated_requirements.rs"));

/// Look up a requirement by ID. Returns `None` if the ID is not registered.
pub fn find(id: &str) -> Option<&'static Requirement> {
    REQUIREMENTS.iter().find(|r| r.id == id)
}

/// Count requirements at a given level. Test helper + doc convenience.
#[allow(dead_code)]
pub fn count_at_level(level: Level) -> usize {
    REQUIREMENTS.iter().filter(|r| r.level == level).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_are_unique() {
        let mut seen = HashSet::new();
        for r in REQUIREMENTS {
            assert!(seen.insert(r.id), "duplicate requirement ID: {}", r.id);
        }
    }

    #[test]
    fn ids_follow_naming_convention() {
        for r in REQUIREMENTS {
            let prefix = format!("p{}-", r.principle);
            assert!(
                r.id.starts_with(&prefix),
                "requirement {} does not start with {}",
                r.id,
                prefix
            );
            let level_token = match r.level {
                Level::Must => "-must-",
                Level::Should => "-should-",
                Level::May => "-may-",
            };
            assert!(
                r.id.contains(level_token),
                "requirement {} level token {} missing",
                r.id,
                level_token
            );
        }
    }

    #[test]
    fn principle_range_is_valid() {
        for r in REQUIREMENTS {
            assert!(
                (1..=7).contains(&r.principle),
                "requirement {} has invalid principle {}",
                r.id,
                r.principle
            );
        }
    }

    #[test]
    fn summary_is_non_empty() {
        for r in REQUIREMENTS {
            assert!(
                !r.summary.trim().is_empty(),
                "requirement {} has empty summary",
                r.id
            );
        }
    }

    #[test]
    fn find_returns_registered_ids() {
        assert!(find("p1-must-no-interactive").is_some());
        assert!(find("p6-must-sigpipe").is_some());
        assert!(find("nonexistent-id").is_none());
    }

    #[test]
    fn registry_size_matches_spec() {
        // Spec snapshot 2026-04-20: 46 requirements across P1-P7.
        // Bumping this counter is a deliberate act; it means the spec grew.
        assert_eq!(REQUIREMENTS.len(), 46);
    }

    #[test]
    fn level_counts_match_spec() {
        assert_eq!(count_at_level(Level::Must), 23);
        assert_eq!(count_at_level(Level::Should), 16);
        assert_eq!(count_at_level(Level::May), 7);
    }

    #[test]
    fn exception_category_as_kebab_case_matches_serde() {
        // as_kebab_case must agree with serde_json's rendering — the two
        // are both user-visible surfaces and drifting between them would
        // produce inconsistent scorecard JSON.
        for cat in [
            ExceptionCategory::HumanTui,
            ExceptionCategory::FileTraversal,
            ExceptionCategory::PosixUtility,
            ExceptionCategory::DiagnosticOnly,
        ] {
            let via_serde = serde_json::to_value(cat)
                .ok()
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .expect("serde renders category as string");
            assert_eq!(via_serde, cat.as_kebab_case(), "mismatch for {cat:?}");
        }
    }

    #[test]
    fn suppresses_positive_cases() {
        assert!(suppresses(
            "p1-non-interactive",
            ExceptionCategory::HumanTui
        ));
        assert!(suppresses("p6-sigpipe", ExceptionCategory::HumanTui));
        assert!(suppresses(
            "p1-non-interactive",
            ExceptionCategory::PosixUtility
        ));
        assert!(suppresses("p5-dry-run", ExceptionCategory::DiagnosticOnly));
    }

    #[test]
    fn suppresses_negative_cases() {
        // Checks not in the HumanTui list must not be suppressed by it.
        assert!(!suppresses("p2-json-output", ExceptionCategory::HumanTui));
        // p6-sigpipe is only suppressed under HumanTui, not the others.
        assert!(!suppresses("p6-sigpipe", ExceptionCategory::PosixUtility));
        assert!(!suppresses("p6-sigpipe", ExceptionCategory::DiagnosticOnly));
        // Unknown check ID is never suppressed.
        assert!(!suppresses(
            "totally-fake-check-id",
            ExceptionCategory::HumanTui
        ));
        assert!(!suppresses(
            "totally-fake-check-id",
            ExceptionCategory::DiagnosticOnly
        ));
    }

    #[test]
    fn suppression_table_covers_every_category() {
        // Every `ExceptionCategory` variant must have a row in the table
        // (even if empty) — otherwise a category silently becomes a no-op
        // at the call site and the `suppresses()` helper always returns
        // false for it, which is never what the operator intended.
        for cat in [
            ExceptionCategory::HumanTui,
            ExceptionCategory::FileTraversal,
            ExceptionCategory::PosixUtility,
            ExceptionCategory::DiagnosticOnly,
        ] {
            assert!(
                SUPPRESSION_TABLE.iter().any(|(c, _)| *c == cat),
                "SUPPRESSION_TABLE missing category {cat:?} — a variant was \
                 added to ExceptionCategory without a corresponding table \
                 entry. Add a row (empty slice is fine) and document why.",
            );
        }
    }

    #[test]
    fn suppression_table_check_ids_exist_in_catalog() {
        use crate::check::Check;
        use crate::checks::all_checks_catalog;

        let catalog: Vec<Box<dyn Check>> = all_checks_catalog();
        let catalog_ids: Vec<&str> = catalog.iter().map(|c| c.id()).collect();

        for (cat, ids) in SUPPRESSION_TABLE {
            for id in *ids {
                assert!(
                    catalog_ids.contains(id),
                    "SUPPRESSION_TABLE entry for {cat:?} references unknown \
                     check ID `{id}` — either the check was renamed/removed \
                     or the table has a typo. Fix the table, not the \
                     catalog.",
                );
            }
        }
    }
}
