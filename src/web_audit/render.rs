//! The `anc web` report: the exit table, the text rendering that leads with
//! the verdict and the failing rows with their fixes, and the one-line
//! `--check` form. JSON mode prints the scorecard itself and lives with the
//! scorecard mirror.

use std::fmt::Write as _;
use std::io::IsTerminal;

use crate::color::{paint, status_style};
use crate::json_error::NextStep;
use crate::web_audit::engine::{ProgressSink, RunReport, Unported};
use crate::web_audit::registry::{
    CHECKS, REGISTRY_VERSION, SITE_SHA, WebCheckKeyword, remediation_for,
};
use crate::web_audit::scorecard::{EngineResult, ResultRow, ScorecardStatus, WebScorecard};

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
fn is_miss(row: &ResultRow) -> bool {
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

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        one.to_string()
    } else {
        many.to_string()
    }
}

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

/// Whether a run narrates its progress: only on a terminal, never when
/// quieted and never when `NO_COLOR` asks for a plain stream. A piped or
/// redirected run stays silent, so a twenty-five second audit reads as
/// work in progress to a person and as nothing at all to a program.
pub fn progress_enabled(quiet: bool) -> bool {
    !quiet
        && std::io::stdout().is_terminal()
        && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
}

/// Narrates a run on stderr: the discovered endpoint, then a counter that
/// names the check it just finished.
#[derive(Debug)]
pub struct StderrProgress {
    done: usize,
    total: usize,
}

impl StderrProgress {
    /// A sink counting toward the registry's check total.
    pub fn new() -> Self {
        StderrProgress {
            done: 0,
            total: CHECKS.len(),
        }
    }
}

impl Default for StderrProgress {
    fn default() -> Self {
        Self::new()
    }
}

impl ProgressSink for StderrProgress {
    fn discovery(&mut self, endpoint: Option<&str>) {
        match endpoint {
            Some(endpoint) => eprintln!("  mcp endpoint: {endpoint}"),
            None => eprintln!("  mcp endpoint: none discovered"),
        }
    }

    fn result(&mut self, result: &EngineResult) {
        self.done += 1;
        eprint!("\r  {}/{} {:<40}", self.done, self.total, result.check.id);
        if self.done == self.total {
            eprintln!();
        }
    }
}

/// A run that ended without a scorecard, and what to do about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    /// The stable, kebab-case reason an agent branches on.
    pub slug: &'static str,
    /// What went wrong, in one sentence.
    pub message: String,
    /// The cause and what to try, one line each.
    pub hints: Vec<String>,
    /// The action, command and docs an agent takes next.
    pub next_step: NextStep,
    /// The exit code this failure returns.
    pub exit_code: i32,
}

/// Where the local web audit is documented.
const WEB_DOCS: &str = "https://anc.dev/audit";

/// The target answered nothing. The engine's reason is the hosted
/// auditor's wording, which speaks for its own vantage point, so the
/// local run says what is true here instead: nothing on this machine's
/// network path answered.
pub fn unreachable_failure(token: &str, normalized: &str, defaulted_https: bool) -> Failure {
    let mut hints = vec![format!(
        "no HTTP response from {normalized}, on the root fetch or on any MCP discovery probe"
    )];
    if defaulted_https {
        hints.push(format!(
            "the scheme defaulted to https; if the server speaks plain HTTP, try `anc web http://{token}`"
        ));
    }
    hints.push("check that the server is running and that this machine can reach it".to_string());
    Failure {
        slug: "target-unreachable",
        message: format!("{normalized} did not answer any probe"),
        hints,
        next_step: NextStep {
            action: "retry",
            command: format!("anc web {token} --verbose"),
            docs: Some(WEB_DOCS),
        },
        exit_code: EXIT_COULD_NOT_CHECK,
    }
}

/// The target could not be read as an http(s) URL at all.
pub fn invalid_target_failure(token: &str, reason: &str) -> Failure {
    Failure {
        slug: "invalid-target",
        message: format!("cannot audit {token}: {reason}"),
        hints: vec![
            "pass a URL or a host, such as `anc web localhost:8787` or `anc web https://anc.dev`"
                .to_string(),
        ],
        next_step: NextStep {
            action: "fix-target",
            command: "anc web --help".to_string(),
            docs: Some(WEB_DOCS),
        },
        exit_code: EXIT_COULD_NOT_CHECK,
    }
}

/// `--check` named an id the compiled registry does not carry, which is a
/// usage error rather than an unreachable surface.
pub fn unknown_check_failure(id: &str) -> Failure {
    Failure {
        slug: "unknown-check",
        message: format!("no such check: {id}"),
        hints: vec!["run `anc emit web-checks` for every id this build scores".to_string()],
        next_step: NextStep {
            action: "list-checks",
            command: "anc emit web-checks".to_string(),
            docs: Some(WEB_DOCS),
        },
        exit_code: EXIT_FAILURES,
    }
}

/// The text rendering of a failure: the problem, then the cause and the
/// next thing to try, then the code and what earned it.
pub fn format_failure(failure: &Failure) -> String {
    let mut out = format!("error: {}\n", failure.message);
    for hint in &failure.hints {
        let _ = writeln!(out, "  {hint}");
    }
    let _ = writeln!(
        out,
        "  next: {}\n\nexit {}: could not check",
        failure.next_step.command, failure.exit_code
    );
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
    use crate::web_audit::registry::WebCheckTier;
    use crate::web_audit::scorecard::Layer;

    fn row(id: &str, keyword: WebCheckKeyword, status: ScorecardStatus) -> ResultRow {
        ResultRow {
            id: id.to_string(),
            label: format!("Row {id}"),
            category: "api".to_string(),
            group: "api".to_string(),
            layer: Layer::Web,
            keyword,
            tier: WebCheckTier::Required,
            principle: "P1".to_string(),
            status,
            na_reason: None,
            unprobed: false,
            evidence: Some(format!("evidence for {id}")),
        }
    }

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
