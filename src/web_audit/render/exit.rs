//! The exit table and the code a set of rows earns.

use crate::web_audit::registry::WebCheckKeyword;
use crate::web_audit::scorecard::{ResultRow, ScorecardStatus};

/// Every applicable check passed.
pub const EXIT_CLEAN: i32 = 0;
/// Warnings only: a SHOULD or MAY miss.
pub const EXIT_WARNINGS: i32 = 1;
/// Failures present: a MUST miss.
pub const EXIT_FAILURES: i32 = 2;
/// Could not check: an unreachable target, a probe that errored or was cut
/// short, or every selected check inapplicable.
pub const EXIT_COULD_NOT_CHECK: i32 = 3;

/// The exit table, as `--help` and the README print it.
pub const EXIT_TABLE: &str = "\
Exit codes (shared with `anc audit`):
  0  clean: every applicable check passed
  1  warnings only: a SHOULD or MAY check missed
  2  failures present: a MUST check missed, or a usage error
  3  could not check: the target was unreachable, a probe errored or was cut
     short by the deadline, or every selected check was inapplicable";

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

pub(super) fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        one.to_string()
    } else {
        many.to_string()
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
