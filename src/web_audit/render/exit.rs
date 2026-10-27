//! The exit table and the code a set of rows earns.

use crate::web_audit::registry::WebCheckKeyword;
use crate::web_audit::scorecard::{ResultRow, ScorecardStatus};

pub(super) use crate::exit_codes::plural;
pub use crate::exit_codes::{
    EXIT_CLEAN, EXIT_COULD_NOT_CHECK, EXIT_FAILURES, EXIT_TABLE, EXIT_WARNINGS,
};

/// A row that probed a surface and found it wanting.
pub(super) fn is_miss(row: &ResultRow) -> bool {
    matches!(
        row.status,
        ScorecardStatus::Noncompliant | ScorecardStatus::Broken | ScorecardStatus::Absent
    )
}

/// The exit code for a set of rows.
pub fn exit_code(rows: &[ResultRow]) -> i32 {
    if rows.is_empty() || rows.iter().all(|r| r.status == ScorecardStatus::NA) {
        return EXIT_COULD_NOT_CHECK;
    }
    if rows
        .iter()
        .any(|r| matches!(r.status, ScorecardStatus::Error | ScorecardStatus::Skip))
    {
        return EXIT_COULD_NOT_CHECK;
    }
    if rows
        .iter()
        .any(|r| is_miss(r) && r.keyword == WebCheckKeyword::Must)
    {
        return EXIT_FAILURES;
    }
    if rows.iter().any(is_miss) {
        return EXIT_WARNINGS;
    }
    EXIT_CLEAN
}

/// What earned an exit code, for the closing line.
pub fn exit_reason(code: i32, rows: &[ResultRow]) -> String {
    match code {
        EXIT_CLEAN => "every applicable check passed".to_string(),
        EXIT_WARNINGS => {
            let n = rows.iter().filter(|r| is_miss(r)).count();
            format!(
                "warnings only ({n} SHOULD or MAY {})",
                plural(n, "miss", "misses")
            )
        }
        EXIT_FAILURES => {
            let n = rows
                .iter()
                .filter(|r| is_miss(r) && r.keyword == WebCheckKeyword::Must)
                .count();
            format!(
                "failures present ({n} MUST {})",
                plural(n, "miss", "misses")
            )
        }
        _ => {
            if rows.is_empty() || rows.iter().all(|r| r.status == ScorecardStatus::NA) {
                "could not check: every selected check was inapplicable".to_string()
            } else {
                let errored = rows
                    .iter()
                    .filter(|r| r.status == ScorecardStatus::Error)
                    .count();
                let skipped = rows
                    .iter()
                    .filter(|r| r.status == ScorecardStatus::Skip)
                    .count();
                format!(
                    "could not check: {errored} {} errored, {skipped} skipped",
                    plural(errored, "probe", "probes")
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web_audit::render::test_rows::row;

    #[test]
    fn the_exit_table_pins_all_four_codes() {
        use ScorecardStatus as S;
        use WebCheckKeyword as K;
        assert_eq!(exit_code(&[row("a", K::Must, S::Pass)]), 0);
        assert_eq!(
            exit_code(&[row("a", K::Must, S::Pass), row("b", K::Should, S::Absent)]),
            1
        );
        assert_eq!(exit_code(&[row("a", K::May, S::Noncompliant)]), 1);
        assert_eq!(
            exit_code(&[row("a", K::Must, S::Broken), row("b", K::Should, S::Absent)]),
            2
        );
        assert_eq!(
            exit_code(&[row("a", K::Must, S::Pass), row("b", K::May, S::Error)]),
            3
        );
        assert_eq!(exit_code(&[row("a", K::Must, S::Skip)]), 3);
        assert_eq!(
            exit_code(&[row("a", K::Must, S::NA), row("b", K::May, S::NA)]),
            3
        );
        assert_eq!(exit_code(&[]), 3);
        assert_eq!(
            exit_code(&[row("a", K::Must, S::NA), row("b", K::May, S::Pass)]),
            0
        );
        assert_eq!(
            exit_reason(2, &[row("a", K::Must, S::Broken)]),
            "failures present (1 MUST miss)"
        );
        assert_eq!(
            exit_reason(3, &[row("a", K::Must, S::NA)]),
            "could not check: every selected check was inapplicable"
        );
    }
}
