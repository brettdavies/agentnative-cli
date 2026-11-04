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

    /// The ids of a lane's checks, in registry order.
    fn checks_in(lane: &McpLane) -> Vec<&'static str> {
        CHECKS
            .iter()
            .filter(|c| lane_of(c.id) == Some(lane))
            .map(|c| c.id)
            .collect()
    }

    fn loose_checks() -> Vec<&'static str> {
        CHECKS
            .iter()
            .filter(|c| lane_of(c.id).is_none())
            .map(|c| c.id)
            .collect()
    }

    /// A lane with `at_least` checks. The lane map is display data the site owns,
    /// so these tests name no lane or check of their own.
    fn lane_with(at_least: usize) -> &'static McpLane {
        MCP_LANES
            .iter()
            .find(|lane| checks_in(lane).len() >= at_least)
            .expect("a lane this large")
    }

    fn heading(lane: &McpLane, count: &str) -> String {
        let category = category_name(LANE_CATEGORY).unwrap();
        format!("  {category} · {}{count}\n    {}.\n", lane.label, lane.note)
    }

    #[test]
    fn rows_split_into_loose_rows_then_lanes_in_lane_map_order() {
        let loose = loose_checks();
        // Each lane's rows reversed and the lanes reversed, so only `split` can put them
        // back, and the loose rows on either side out of registry order, which it keeps.
        let mut given = vec![row(
            loose[1],
            WebCheckKeyword::Should,
            ScorecardStatus::Absent,
        )];
        for lane in MCP_LANES.iter().rev() {
            for id in checks_in(lane).iter().rev() {
                given.push(row(id, WebCheckKeyword::Should, ScorecardStatus::Pass));
            }
        }
        given.push(row(
            loose[0],
            WebCheckKeyword::Should,
            ScorecardStatus::Absent,
        ));
        let refs: Vec<&ResultRow> = given.iter().collect();
        let (loose_rows, blocks) = split(&refs);
        assert_eq!(ids(&loose_rows), [loose[1], loose[0]], "in the order given");
        let grouped: Vec<(&str, Vec<String>)> =
            blocks.iter().map(|b| (b.lane.id, ids(&b.rows))).collect();
        let expected: Vec<(&str, Vec<String>)> = MCP_LANES
            .iter()
            .filter(|lane| !checks_in(lane).is_empty())
            .map(|lane| {
                let rows = checks_in(lane).iter().map(|id| id.to_string()).collect();
                (lane.id, rows)
            })
            .collect();
        assert_eq!(grouped, expected);
    }

    #[test]
    fn a_section_without_mcp_rows_gets_no_lane() {
        let id = loose_checks()[0];
        let given = [row(id, WebCheckKeyword::Should, ScorecardStatus::Pass)];
        let refs: Vec<&ResultRow> = given.iter().collect();
        let (loose, blocks) = split(&refs);
        assert_eq!(ids(&loose), [id]);
        assert!(blocks.is_empty());
    }

    #[test]
    fn the_heading_counts_the_lane_over_the_whole_report() {
        use ScorecardStatus as S;
        let lane = lane_with(4);
        let mine = checks_in(lane);
        let other = MCP_LANES
            .iter()
            .find(|other| *other != lane && !checks_in(other).is_empty())
            .map(|other| checks_in(other)[0])
            .expect("a second lane");
        let all = [
            row(mine[0], WebCheckKeyword::Must, S::Pass),
            row(mine[1], WebCheckKeyword::Must, S::Noncompliant),
            row(mine[2], WebCheckKeyword::Should, S::NA),
            row(mine[3], WebCheckKeyword::Should, S::Error),
            row(other, WebCheckKeyword::Should, S::Pass),
        ];
        let mut out = String::new();
        write_heading(&mut out, lane, &all, "  ");
        assert_eq!(out, heading(lane, " (1/2 pass)"));
    }

    #[test]
    fn a_lane_with_nothing_counted_shows_no_count() {
        let lane = lane_with(2);
        let mine = checks_in(lane);
        let all = [
            row(mine[0], WebCheckKeyword::Should, ScorecardStatus::NA),
            row(mine[1], WebCheckKeyword::May, ScorecardStatus::Skip),
        ];
        let mut out = String::new();
        write_heading(&mut out, lane, &all, "  ");
        assert_eq!(out, heading(lane, ""));
    }
}
