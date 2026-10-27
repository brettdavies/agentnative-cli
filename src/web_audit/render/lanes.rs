//! The MCP category's protocol lanes in the text report: inside each
//! section, the rows a lane claims gather under that lane's heading, the
//! way the site's result page groups the MCP category.

use std::fmt::Write as _;

use crate::web_audit::registry::{
    CHECKS, LANE_CATEGORY, MCP_LANES, McpLane, category_name, lane_of,
};
use crate::web_audit::score::is_scored;
use crate::web_audit::scorecard::{ResultRow, ScorecardStatus};

/// One lane's rows within a section.
pub(super) struct LaneBlock<'a> {
    pub(super) lane: &'static McpLane,
    pub(super) rows: Vec<&'a ResultRow>,
}

/// Split a section's rows into those no lane claims, in the order given,
/// and one block per lane in `MCP_LANES` order, each in registry order. A
/// lane with no row in the section gets no block.
pub(super) fn split<'a>(rows: &[&'a ResultRow]) -> (Vec<&'a ResultRow>, Vec<LaneBlock<'a>>) {
    let mut loose = Vec::new();
    let mut blocks: Vec<LaneBlock<'a>> = MCP_LANES
        .iter()
        .map(|lane| LaneBlock {
            lane,
            rows: Vec::new(),
        })
        .collect();
    for &row in rows {
        let block = lane_of(&row.id).and_then(|lane| blocks.iter_mut().find(|b| b.lane == lane));
        match block {
            Some(block) => block.rows.push(row),
            None => loose.push(row),
        }
    }
    for block in &mut blocks {
        block.rows.sort_by_key(|row| registry_index(&row.id));
    }
    blocks.retain(|block| !block.rows.is_empty());
    (loose, blocks)
}

fn registry_index(id: &str) -> usize {
    CHECKS.iter().position(|c| c.id == id).unwrap_or(usize::MAX)
}

/// A lane's heading, then the line saying what it covers, at `indent` and
/// one level deeper. The pass count is the lane's own over every row in the
/// report, so a lane split across sections reads the same rollup in each.
/// A lane with nothing counted shows no count, since "0/0" would read as a
/// lane that failed.
pub(super) fn write_heading(out: &mut String, lane: &McpLane, all: &[ResultRow], indent: &str) {
    let category = category_name(LANE_CATEGORY).unwrap_or(LANE_CATEGORY);
    let counted: Vec<&ResultRow> = all
        .iter()
        .filter(|row| lane_of(&row.id) == Some(lane) && is_scored(row.status))
        .collect();
    let count = if counted.is_empty() {
        String::new()
    } else {
        let passed = counted
            .iter()
            .filter(|row| row.status == ScorecardStatus::Pass)
            .count();
        format!(" ({passed}/{} pass)", counted.len())
    };
    let _ = writeln!(out, "{indent}{category} · {}{count}", lane.label);
    let _ = writeln!(out, "{indent}  {}.", lane.note);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web_audit::registry::WebCheckKeyword;
    use crate::web_audit::render::test_rows::row;

    fn ids(rows: &[&ResultRow]) -> Vec<String> {
        rows.iter().map(|r| r.id.clone()).collect()
    }

    #[test]
    fn rows_split_into_loose_rows_then_lanes_in_lane_map_order() {
        use ScorecardStatus as S;
        let given = [
            row("webmcp", WebCheckKeyword::May, S::Pass),
            row("robots", WebCheckKeyword::Should, S::Absent),
            row("mcp-server-discover", WebCheckKeyword::Should, S::Absent),
            row("mcp-cors-actual", WebCheckKeyword::Should, S::Pass),
            row("mcp-accept-json", WebCheckKeyword::Should, S::Broken),
            row("llms-txt", WebCheckKeyword::Should, S::Absent),
            row("mcp-initialize", WebCheckKeyword::Must, S::Pass),
            row("mcp-get-fast-fail", WebCheckKeyword::Should, S::Pass),
        ];
        let refs: Vec<&ResultRow> = given.iter().collect();
        let (loose, blocks) = split(&refs);
        assert_eq!(ids(&loose), ["robots", "llms-txt"]);
        let grouped: Vec<(&str, Vec<String>)> =
            blocks.iter().map(|b| (b.lane.id, ids(&b.rows))).collect();
        assert_eq!(
            grouped,
            [
                (
                    "shared",
                    vec!["mcp-get-fast-fail".into(), "mcp-cors-actual".into()]
                ),
                (
                    "legacy",
                    vec!["mcp-initialize".into(), "mcp-accept-json".into()]
                ),
                ("modern", vec!["mcp-server-discover".into()]),
                ("browser", vec!["webmcp".into()]),
            ]
        );
    }

    #[test]
    fn a_section_without_mcp_rows_gets_no_lane() {
        let given = [row(
            "robots",
            WebCheckKeyword::Should,
            ScorecardStatus::Pass,
        )];
        let refs: Vec<&ResultRow> = given.iter().collect();
        let (loose, blocks) = split(&refs);
        assert_eq!(ids(&loose), ["robots"]);
        assert!(blocks.is_empty());
    }

    #[test]
    fn the_heading_counts_the_lane_over_the_whole_report() {
        use ScorecardStatus as S;
        let all = [
            row("mcp-initialize", WebCheckKeyword::Must, S::Pass),
            row("mcp-tools-list", WebCheckKeyword::Must, S::Noncompliant),
            row("mcp-capabilities", WebCheckKeyword::Should, S::NA),
            row("mcp-resources-list", WebCheckKeyword::Should, S::Error),
            row("mcp-get-fast-fail", WebCheckKeyword::Should, S::Pass),
        ];
        let legacy = MCP_LANES.iter().find(|l| l.id == "legacy").unwrap();
        let mut out = String::new();
        write_heading(&mut out, legacy, &all, "  ");
        assert_eq!(
            out,
            format!(
                "  MCP · {} (1/2 pass)\n    {}.\n",
                legacy.label, legacy.note
            )
        );
    }

    #[test]
    fn a_lane_with_nothing_counted_shows_no_count() {
        let all = [
            row(
                "mcp-cors-preflight",
                WebCheckKeyword::Should,
                ScorecardStatus::NA,
            ),
            row(
                "mcp-auth-enforced",
                WebCheckKeyword::May,
                ScorecardStatus::Skip,
            ),
        ];
        let shared = MCP_LANES.iter().find(|l| l.id == "shared").unwrap();
        let mut out = String::new();
        write_heading(&mut out, shared, &all, "  ");
        assert_eq!(
            out,
            format!("  MCP · {}\n    {}.\n", shared.label, shared.note)
        );
    }
}
