//! How the text report prints rows: one row with its evidence and fix, and
//! one section's rows with the MCP rows under their protocol lanes.

use std::fmt::Write as _;

use super::lanes;
use crate::color::{paint, status_style};
use crate::web_audit::registry::{WebCheckKeyword, remediation_for};
use crate::web_audit::scorecard::{ResultRow, ScorecardStatus};

/// The indent of a section's own rows and of its lane headings.
const SECTION_INDENT: &str = "  ";
/// The indent of the rows under a lane heading.
const LANE_INDENT: &str = "    ";

/// The status prefix a row renders with, mapped through the tier: a miss
/// on a MUST is a failure, on a SHOULD or MAY a warning.
pub fn prefix_for(row: &ResultRow) -> &'static str {
    match row.status {
        ScorecardStatus::Pass => "PASS",
        ScorecardStatus::Noncompliant | ScorecardStatus::Broken | ScorecardStatus::Absent => {
            if row.keyword == WebCheckKeyword::Must {
                "FAIL"
            } else {
                "WARN"
            }
        }
        ScorecardStatus::NA => "N/A ",
        ScorecardStatus::Skip => "SKIP",
        ScorecardStatus::Error => "ERR ",
    }
}

/// A row's status line at `indent`, then its evidence and, when asked, its
/// fix, aligned past the status column.
pub(super) fn write_row(
    out: &mut String,
    row: &ResultRow,
    indent: &str,
    color: bool,
    with_fix: bool,
) {
    let prefix = prefix_for(row);
    let painted = paint(status_style(prefix, color), prefix);
    let _ = writeln!(
        out,
        "{indent}[{painted}] {} ({}) ({})",
        row.label,
        row.id,
        row.keyword.as_str()
    );
    if let Some(evidence) = row.evidence.as_deref().filter(|e| !e.is_empty()) {
        for line in evidence.lines() {
            let _ = writeln!(out, "{indent}       {line}");
        }
    }
    if with_fix && let Some(fix) = remediation_for(&row.id) {
        let _ = writeln!(out, "{indent}       Goal: {}", fix.goal);
        let mut lines = fix.fix.trim_end().lines();
        if let Some(first) = lines.next() {
            let _ = writeln!(out, "{indent}       Fix:  {first}");
        }
        for line in lines {
            let _ = writeln!(out, "{indent}             {line}");
        }
        for resource in fix.resources {
            let _ = writeln!(
                out,
                "{indent}       Docs: {} <{}>",
                resource.label, resource.url
            );
        }
    }
}

/// A section's rows: those outside any lane first, in report order, then
/// each MCP lane's heading over its rows. The lanes follow rather than sit
/// where their first row fell because a lane's rows are indented under its
/// heading, so a section row printed after them would read as one of them.
/// `all` is every row in the report, which a lane's pass count reads.
pub(super) fn write_section(
    out: &mut String,
    rows: &[&ResultRow],
    all: &[ResultRow],
    mut write: impl FnMut(&mut String, &ResultRow, &str),
) {
    let (loose, blocks) = lanes::split(rows);
    for row in loose {
        write(out, row, SECTION_INDENT);
    }
    for block in blocks {
        lanes::write_heading(out, block.lane, all, SECTION_INDENT);
        for row in block.rows {
            write(out, row, LANE_INDENT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web_audit::render::test_rows::row;

    #[test]
    fn prefixes_follow_the_tier_mapping() {
        use ScorecardStatus as S;
        assert_eq!(
            prefix_for(&row("a", WebCheckKeyword::Must, S::Absent)),
            "FAIL"
        );
        assert_eq!(
            prefix_for(&row("a", WebCheckKeyword::Should, S::Absent)),
            "WARN"
        );
        assert_eq!(
            prefix_for(&row("a", WebCheckKeyword::May, S::Broken)),
            "WARN"
        );
        assert_eq!(prefix_for(&row("a", WebCheckKeyword::May, S::Pass)), "PASS");
        assert_eq!(prefix_for(&row("a", WebCheckKeyword::May, S::NA)), "N/A ");
    }
}
