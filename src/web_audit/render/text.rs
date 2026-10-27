//! The text report: the verdict, the failing rows with their fixes, then
//! what could not be checked, advisories, and the passing rows; and the
//! one-line `--check` form.

use std::fmt::Write as _;

use super::exit::{
    EXIT_CLEAN, EXIT_FAILURES, EXIT_WARNINGS, exit_code, exit_reason, is_miss, plural,
};
use crate::color::{paint, status_style};
use crate::web_audit::engine::discovery_documents::card_suffix_url;
use crate::web_audit::engine::{RunReport, Unported};
use crate::web_audit::registry::{
    MCP_DISCOVERY, REGISTRY_VERSION, SITE_SHA, WebCheckKeyword, remediation_for,
};
use crate::web_audit::scorecard::{ResultRow, RowAdvisory, ScorecardStatus, WebScorecard};

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

/// How the text report renders.
#[derive(Clone, Copy, Debug, Default)]
pub struct RenderOptions {
    /// Apply ANSI styling to status prefixes.
    pub color: bool,
    /// Drop the passing and inapplicable sections entirely.
    pub quiet: bool,
    /// List passing and inapplicable rows instead of counting them.
    pub verbose: bool,
}

fn write_row(out: &mut String, row: &ResultRow, color: bool, with_fix: bool) {
    let prefix = prefix_for(row);
    let painted = paint(status_style(prefix, color), prefix);
    let _ = writeln!(
        out,
        "  [{painted}] {} ({}) ({})",
        row.label,
        row.id,
        row.keyword.as_str()
    );
    if let Some(evidence) = row.evidence.as_deref().filter(|e| !e.is_empty()) {
        for line in evidence.lines() {
            let _ = writeln!(out, "         {line}");
        }
    }
    if with_fix && let Some(fix) = remediation_for(&row.id) {
        let _ = writeln!(out, "         Goal: {}", fix.goal);
        let mut lines = fix.fix.trim_end().lines();
        if let Some(first) = lines.next() {
            let _ = writeln!(out, "         Fix:  {first}");
        }
        for line in lines {
            let _ = writeln!(out, "               {line}");
        }
        for resource in fix.resources {
            let _ = writeln!(out, "         Docs: {} <{}>", resource.label, resource.url);
        }
    }
}

/// The note a passing row carries about how it passed, the site's
/// `supersededNote`: a SEP-1649 card names where SEP-2127 puts the card,
/// from the scorecard's endpoint when it recorded one.
pub fn advisory_note(row: &ResultRow, endpoint: Option<&str>) -> Option<String> {
    match row.advisory? {
        RowAdvisory::Superseded => {
            let suffix = MCP_DISCOVERY.card_suffix;
            let card = endpoint.filter(|e| url::Url::parse(e).is_ok()).map_or_else(
                || format!("<endpoint>{suffix}"),
                |e| card_suffix_url(e, suffix),
            );
            Some(format!(
                "Superseded shape (SEP-1649). SEP-2127 moves the card to {card}, listed in {}.",
                MCP_DISCOVERY.ai_catalog
            ))
        }
    }
}

/// The note that a score is not comparable with anc.dev, when any handler
/// or rule the run needed is unported.
pub fn non_comparable_note(unported: &[Unported]) -> Option<String> {
    if unported.is_empty() {
        return None;
    }
    let mut kinds: Vec<&str> = unported.iter().map(|u| u.kind).collect();
    kinds.sort_unstable();
    kinds.dedup();
    let ids: Vec<&str> = unported.iter().map(|u| u.check_id).collect();
    Some(format!(
        "score not comparable with anc.dev: unported handler {} `{}`; skipped {}",
        plural(kinds.len(), "kind", "kinds"),
        kinds.join("`, `"),
        ids.join(", ")
    ))
}

/// The header naming the target and the registry the run was scored with.
fn write_header(out: &mut String, scorecard: &WebScorecard) {
    let _ = writeln!(out, "anc web {}", scorecard.target_url);
    let _ = writeln!(
        out,
        "  registry v{REGISTRY_VERSION} (anc.dev {}) · spec {} · site type: {} · mcp endpoint: {}",
        &SITE_SHA[..7],
        scorecard.spec_version,
        scorecard.site_type.map_or("any", |t| match t {
            crate::web_audit::scorecard::DeclaredSiteType::Content => "content",
            crate::web_audit::scorecard::DeclaredSiteType::Api => "api",
        }),
        scorecard.mcp_endpoint.as_deref().unwrap_or("none")
    );
}

/// Render the full text report: verdict first, then failing rows with
/// their fixes, then warnings, then what could not be checked, then the
/// passing and inapplicable rows as counts.
pub fn format_text(report: &RunReport, opts: RenderOptions) -> String {
    let scorecard = &report.scorecard;
    let rows = &scorecard.results;
    let code = exit_code(rows);
    let mut out = String::new();
    write_header(&mut out, scorecard);
    out.push('\n');

    let verdict = match code {
        EXIT_CLEAN => "PASS",
        EXIT_WARNINGS => "WARN",
        EXIT_FAILURES => "FAIL",
        _ => "ERR ",
    };
    let painted = paint(status_style(verdict, opts.color), verdict.trim());
    let _ = writeln!(out, "Verdict: {painted}: {}", exit_reason(code, rows));
    let _ = writeln!(
        out,
        "Score: {} relative · {} global · {} of {} applicable checks passed",
        scorecard.score.relative,
        scorecard.score.global,
        scorecard.summary.pass,
        rows.iter()
            .filter(|r| !matches!(r.status, ScorecardStatus::NA | ScorecardStatus::Skip))
            .count()
    );
    if let Some(note) = non_comparable_note(&report.unported) {
        let _ = writeln!(out, "Note: {note}");
    }
    if !report.complete {
        let _ = writeln!(
            out,
            "Note: the run hit its deadline; rows marked SKIP were not probed"
        );
    }

    let failures: Vec<&ResultRow> = rows
        .iter()
        .filter(|r| is_miss(r) && r.keyword == WebCheckKeyword::Must)
        .collect();
    let warnings: Vec<&ResultRow> = rows
        .iter()
        .filter(|r| is_miss(r) && r.keyword != WebCheckKeyword::Must)
        .collect();
    let unchecked: Vec<&ResultRow> = rows
        .iter()
        .filter(|r| matches!(r.status, ScorecardStatus::Error | ScorecardStatus::Skip))
        .collect();
    let passed: Vec<&ResultRow> = rows
        .iter()
        .filter(|r| r.status == ScorecardStatus::Pass)
        .collect();
    let inapplicable: Vec<&ResultRow> = rows
        .iter()
        .filter(|r| r.status == ScorecardStatus::NA)
        .collect();

    if !failures.is_empty() {
        let _ = writeln!(out, "\nFailures ({} MUST):", failures.len());
        for row in &failures {
            write_row(&mut out, row, opts.color, true);
        }
    }
    if !warnings.is_empty() {
        let _ = writeln!(out, "\nWarnings ({} SHOULD or MAY):", warnings.len());
        for row in &warnings {
            write_row(&mut out, row, opts.color, true);
        }
    }
    if !unchecked.is_empty() {
        let _ = writeln!(out, "\nCould not check ({}):", unchecked.len());
        for row in &unchecked {
            write_row(&mut out, row, opts.color, false);
        }
    }
    if !opts.quiet {
        let endpoint = scorecard.mcp_endpoint.as_deref();
        let advised: Vec<(&ResultRow, String)> = passed
            .iter()
            .filter_map(|row| advisory_note(row, endpoint).map(|note| (*row, note)))
            .collect();
        if !advised.is_empty() {
            let _ = writeln!(out, "\nAdvisories ({}):", advised.len());
            for (row, note) in &advised {
                write_row(&mut out, row, opts.color, false);
                let _ = writeln!(out, "         Note: {note}");
            }
        }
        if opts.verbose {
            let _ = writeln!(out, "\nPassed ({}):", passed.len());
            for row in &passed {
                write_row(&mut out, row, opts.color, false);
            }
            let _ = writeln!(out, "\nNot applicable ({}):", inapplicable.len());
            for row in &inapplicable {
                write_row(&mut out, row, opts.color, false);
            }
        } else {
            let _ = writeln!(
                out,
                "\nPassed: {} checks · not applicable: {} (run with --verbose to list them)",
                passed.len(),
                inapplicable.len()
            );
        }
    }
    let _ = writeln!(out, "\nexit {code}: {}", exit_reason(code, rows));
    out
}

/// The one-line `--check` form: id, status and the evidence line, tab
/// separated, as the site runner prints it.
pub fn format_check(row: &ResultRow) -> String {
    format!(
        "{}\t{}\t{}\n",
        row.id,
        row.status.as_str(),
        row.evidence.as_deref().unwrap_or("")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web_audit::render::test_rows::row;

    #[test]
    fn a_superseded_card_names_where_sep_2127_puts_the_card() {
        let plain = row(
            "mcp-server-card",
            WebCheckKeyword::Should,
            ScorecardStatus::Pass,
        );
        assert_eq!(advisory_note(&plain, Some("https://x.test/mcp")), None);
        let advised = ResultRow {
            advisory: Some(RowAdvisory::Superseded),
            ..plain
        };
        assert_eq!(
            advisory_note(&advised, Some("https://x.test/mcp/?a=1")).as_deref(),
            Some(
                "Superseded shape (SEP-1649). SEP-2127 moves the card to https://x.test/mcp/server-card, listed in /.well-known/ai-catalog.json."
            )
        );
        assert_eq!(
            advisory_note(&advised, None).as_deref(),
            Some(
                "Superseded shape (SEP-1649). SEP-2127 moves the card to <endpoint>/server-card, listed in /.well-known/ai-catalog.json."
            )
        );
    }

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

    #[test]
    fn the_check_line_is_tab_separated() {
        let mut r = row("robots", WebCheckKeyword::Should, ScorecardStatus::Pass);
        assert_eq!(format_check(&r), "robots\tpass\tevidence for robots\n");
        r.evidence = None;
        assert_eq!(format_check(&r), "robots\tpass\t\n");
    }

    #[test]
    fn the_non_comparable_note_names_kinds_and_ids() {
        assert_eq!(non_comparable_note(&[]), None);
        let note = non_comparable_note(&[
            Unported {
                check_id: "robots",
                kind: "http",
            },
            Unported {
                check_id: "llms-txt",
                kind: "http",
            },
        ])
        .unwrap();
        assert_eq!(
            note,
            "score not comparable with anc.dev: unported handler kind `http`; skipped robots, llms-txt"
        );
    }
}
