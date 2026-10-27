//! The `anc web` report: the exit table, the text rendering that leads with
//! the verdict and the failing rows with their fixes, and the one-line
//! `--check` form. JSON mode prints the scorecard itself and lives with the
//! scorecard mirror.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::io::IsTerminal;
use std::path::Path;

use crate::argv::format_invocation;
use crate::color::{paint, status_style};
use crate::json_error::NextStep;
use crate::web_audit::engine::discovery_documents::card_suffix_url;
use crate::web_audit::engine::{ProgressSink, RunReport, Unported};
use crate::web_audit::fetch::trust::CaCertError;
use crate::web_audit::registry::{
    CHECKS, MCP_DISCOVERY, REGISTRY_VERSION, SITE_SHA, WebCheckKeyword, remediation_for,
};
use crate::web_audit::scorecard::{
    EngineResult, ResultRow, RowAdvisory, ScorecardStatus, WebScorecard,
};

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
    /// The input the failure names, spelled as clap's own errors spell it:
    /// `<TARGET>`, `--check <ID>` or `--ca-cert <FILE>`.
    pub argument: &'static str,
    /// The value of [`Failure::argument`] the failure is about.
    pub value: String,
}

/// The positional `anc web` audits, as clap names it.
const TARGET_ARGUMENT: &str = "<TARGET>";
/// The `--check` flag, as clap names it.
const CHECK_ARGUMENT: &str = "--check <ID>";
/// The `--ca-cert` flag, as clap names it.
const CA_CERT_ARGUMENT: &str = "--ca-cert <FILE>";

/// `anc web` with `args`, shell-quoted so the line runs as printed.
fn web_command(args: &[&str]) -> String {
    let argv: Vec<OsString> = ["anc", "web"]
        .iter()
        .chain(args)
        .map(OsString::from)
        .collect();
    format_invocation(&argv)
}

/// The target answered nothing. The engine's reason is the hosted
/// auditor's wording, which speaks for its own vantage point, so the
/// local run says what is true here instead: nothing on this machine's
/// network path answered.
pub fn unreachable_failure(token: &str, normalized: &str, defaulted_https: bool) -> Failure {
    let mut hints = vec![format!(
        "no HTTP response from {normalized}, on the root fetch or on any MCP discovery probe"
    )];
    if defaulted_https {
        hints.push(
            "the scheme defaulted to https; anc web sends no plaintext request, so a server \
             that speaks only HTTP cannot be audited until it serves TLS"
                .to_string(),
        );
    }
    hints.push("check that the server is running and that this machine can reach it".to_string());
    Failure {
        slug: "target-unreachable",
        message: format!("{normalized} did not answer any probe"),
        hints,
        next_step: NextStep::retry(web_command(&[token, "--verbose"])),
        exit_code: EXIT_COULD_NOT_CHECK,
        argument: TARGET_ARGUMENT,
        value: normalized.to_string(),
    }
}

/// The target answered with a certificate chain no root in the trust set
/// vouches for. Nothing was sent past the handshake, and retrying changes
/// nothing; trusting the server's CA does.
pub fn untrusted_certificate_failure(token: &str, normalized: &str, evidence: &str) -> Failure {
    Failure {
        slug: "untrusted-certificate",
        message: format!("{normalized} presented a certificate chain this run does not trust"),
        hints: vec![
            evidence.to_string(),
            "if the server's certificate comes from a private or development CA, pass that \
             CA's PEM certificate with `--ca-cert <FILE>`, or install it in the OS trust store, \
             which a local or private target also trusts"
                .to_string(),
        ],
        next_step: NextStep::add_ca_cert(format!("{} --ca-cert <FILE>", web_command(&[token]))),
        exit_code: EXIT_COULD_NOT_CHECK,
        argument: TARGET_ARGUMENT,
        value: normalized.to_string(),
    }
}

/// The target is `http`, or its root redirects to `http`: the run ends
/// before any plaintext request, with the engine's no-plaintext reason as
/// the message.
pub fn insecure_scheme_failure(normalized: &str, reason: &str, redirected: bool) -> Failure {
    let cause = if redirected {
        "the root redirects https to http; serve the site over https end to end"
    } else {
        "anc web sends HTTPS only, local targets included: serve the site over TLS and audit \
         its https URL"
    };
    Failure {
        slug: "insecure-scheme",
        message: reason.to_string(),
        hints: vec![
            cause.to_string(),
            "a certificate from a private or development CA needs `--ca-cert <FILE>`, or that \
             CA in the OS trust store"
                .to_string(),
        ],
        next_step: NextStep::fix_target(web_command(&["--help"])),
        exit_code: EXIT_COULD_NOT_CHECK,
        argument: TARGET_ARGUMENT,
        value: normalized.to_string(),
    }
}

/// A `--ca-cert` file supplies no usable root, a usage error raised before
/// any request goes out.
pub fn ca_cert_failure(path: &Path, reason: &CaCertError) -> Failure {
    let shown = path.display().to_string();
    Failure {
        slug: "invalid-ca-cert",
        message: format!("cannot trust --ca-cert {shown}: {reason}"),
        hints: vec![
            "pass a PEM file holding the CA's root certificate, a block that begins \
             `-----BEGIN CERTIFICATE-----`"
                .to_string(),
        ],
        next_step: NextStep::show_help(web_command(&["--help"])),
        exit_code: EXIT_FAILURES,
        argument: CA_CERT_ARGUMENT,
        value: shown,
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
        next_step: NextStep::fix_target(web_command(&["--help"])),
        exit_code: EXIT_COULD_NOT_CHECK,
        argument: TARGET_ARGUMENT,
        value: token.to_string(),
    }
}

/// `--check` named an id the compiled registry does not carry, which is a
/// usage error rather than an unreachable surface.
pub fn unknown_check_failure(id: &str) -> Failure {
    Failure {
        slug: "unknown-check",
        message: format!("no such check: {id}"),
        hints: vec!["run `anc emit web-checks` for every id this build scores".to_string()],
        next_step: NextStep::list_checks("anc emit web-checks".to_string()),
        exit_code: EXIT_FAILURES,
        argument: CHECK_ARGUMENT,
        value: id.to_string(),
    }
}

impl Failure {
    /// The envelope's `kind`: `usage` for a failure that exits as a usage
    /// error, `runtime` for one that could not check.
    pub fn kind(&self) -> &'static str {
        if self.exit_code == EXIT_FAILURES {
            "usage"
        } else {
            "runtime"
        }
    }
}

/// The text rendering of a failure: the problem, then the cause and the
/// next thing to try, then the code and what earned it.
pub fn format_failure(failure: &Failure) -> String {
    let mut out = format!("error: {}\n", failure.message);
    for hint in &failure.hints {
        let _ = writeln!(out, "  {hint}");
    }
    if let Some(command) = &failure.next_step.command {
        let _ = writeln!(out, "  next: {command}");
    }
    let _ = writeln!(
        out,
        "\nexit {}: {}",
        failure.exit_code,
        failure_meaning(failure.exit_code)
    );
    out
}

/// What a failure's exit code means, as the exit table words it. A run
/// that ends without a scorecard exits 2 only for a usage error.
fn failure_meaning(exit_code: i32) -> &'static str {
    match exit_code {
        EXIT_FAILURES => "usage error",
        _ => "could not check",
    }
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
            advisory: None,
            evidence: Some(format!("evidence for {id}")),
            hosts: Vec::new(),
            host: None,
        }
    }

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
    fn a_retry_command_quotes_a_target_a_shell_would_split() {
        let failure = unreachable_failure(
            "http://127.0.0.1:9/?a=1&b=2",
            "http://127.0.0.1:9/?a=1&b=2",
            false,
        );
        assert_eq!(
            failure.next_step.command.as_deref(),
            Some("anc web 'http://127.0.0.1:9/?a=1&b=2' --verbose")
        );
        let plain = unreachable_failure("127.0.0.1:9", "http://127.0.0.1:9/", false);
        assert_eq!(
            plain.next_step.command.as_deref(),
            Some("anc web 127.0.0.1:9 --verbose")
        );
    }

    #[test]
    fn the_failure_trailer_names_what_its_exit_code_means() {
        let unknown = format_failure(&unknown_check_failure("no-such-check"));
        assert!(unknown.ends_with("\nexit 2: usage error\n"), "{unknown}");
        let unreachable = format_failure(&unreachable_failure(
            "127.0.0.1:9",
            "http://127.0.0.1:9/",
            false,
        ));
        assert!(
            unreachable.ends_with("\nexit 3: could not check\n"),
            "{unreachable}"
        );
        let invalid = format_failure(&invalid_target_failure("ftp://x", "unsupported scheme"));
        assert!(
            invalid.ends_with("\nexit 3: could not check\n"),
            "{invalid}"
        );
    }

    #[test]
    fn a_failure_names_its_argument_as_clap_does() {
        let mut cli = <crate::cli::Cli as clap::CommandFactory>::command();
        cli.build();
        let web = cli.find_subcommand("web").expect("the web subcommand");
        let spelled = |id: &str| {
            web.get_arguments()
                .find(|arg| arg.get_id() == id)
                .unwrap_or_else(|| panic!("no `{id}` argument"))
                .to_string()
        };
        assert_eq!(spelled("target"), TARGET_ARGUMENT);
        assert_eq!(spelled("check"), CHECK_ARGUMENT);
        assert_eq!(spelled("ca_cert"), CA_CERT_ARGUMENT);
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
