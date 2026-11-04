//! The `anc web` report contract: what a run prints, in what order, and
//! what the published exit-code table says it means. The failure paths
//! live in `web_audit_failures.rs` and the flag surface in
//! `web_audit_cli.rs`.

#![cfg(feature = "web")]

mod common;

use std::sync::Arc;

use agentnative::web_audit::engine::RunOutcome;
use agentnative::web_audit::handlers::default_handlers;
use agentnative::web_audit::registry::{
    CHECKS, LANE_CATEGORY, MCP_LANES, REGISTRY_VERSION, SITE_SHA, category_name, lane_of,
};
use agentnative::web_audit::render::{RenderOptions, advisory_note, format_text, prefix_for};
use agentnative::web_audit::score::is_scored;
use agentnative::web_audit::scorecard::{ResultRow, ScorecardStatus, WebScorecard};
use agentnative::web_audit::signal::{ContractCheck, NotChecked};
use common::corpus::{Golden, load_cases, replay};
use common::web_cli::{
    anc, fixture_site, full_run_code, host_of, stderr_of, stdout_of, verdict_is_pinned,
};

#[test]
fn a_reachable_site_reports_the_verdict_then_its_misses_with_fixes() {
    let server = fixture_site();
    let assert = anc()
        .args(["web", &host_of(&server)])
        .assert()
        .code(full_run_code(1));
    let out = stdout_of(assert.get_output());

    assert!(out.starts_with("anc web https://"), "{out}");
    let registry = format!("registry v{REGISTRY_VERSION} (anc.dev {}", &SITE_SHA[..7]);
    assert!(out.contains(&registry), "{out}");
    assert!(out.contains("spec "), "{out}");
    assert!(out.contains("mcp endpoint: none"), "{out}");
    // The verdict leads, before any row.
    let verdict = out.find("Verdict: ").expect("a verdict line");
    let first_row = out.find("[WARN]").expect("a warning row");
    assert!(
        verdict < first_row,
        "the verdict must precede the rows:\n{out}"
    );
    if verdict_is_pinned() {
        assert!(out.contains("Verdict: WARN"), "{out}");
        assert!(out.contains("warnings only ("), "{out}");
        let closing = out.trim_end().lines().last().unwrap_or_default();
        assert!(
            closing.starts_with("exit 1: warnings only ("),
            "the closing line names the code and what earned it: {closing:?}"
        );
        assert!(closing.ends_with("misses)"), "{closing:?}");
    }
    assert!(out.contains("Score: "), "{out}");
    // A failing row carries its fix and its docs.
    assert!(out.contains("Warnings ("), "{out}");
    assert!(out.contains("         Goal: "), "{out}");
    assert!(out.contains("         Fix:  "), "{out}");
    assert!(out.contains("         Docs: "), "{out}");
    // Passing and inapplicable rows are counted, not listed.
    assert!(out.contains("(run with --verbose to list them)"), "{out}");
    assert!(!out.contains("[PASS]"), "{out}");
    assert!(!out.contains("[N/A "), "{out}");
    // The probes reached the target and nothing else could have: every
    // request the server saw came from this run.
    let hits = server.hits();
    assert!(hits.len() > 5, "{hits:?}");
    // Every probe identifies itself, and the only User-Agents other than
    // the audit's own are the vendored tokens the UA-negotiation rows
    // deliberately impersonate.
    let vendored: Vec<&str> = agentnative::web_audit::registry::PROBE_UA_TOKENS
        .iter()
        .map(|(_, ua)| *ua)
        .collect();
    for hit in &hits {
        let ua = hit.header("user-agent").unwrap_or_default();
        assert!(
            ua.starts_with("anc-web-audit/") || vendored.contains(&ua),
            "unexpected probe User-Agent {ua:?} on {}",
            hit.target
        );
    }
    assert!(
        hits.iter().any(|h| h
            .header("user-agent")
            .is_some_and(|ua| vendored.contains(&ua))),
        "the UA-negotiation rows send the client they test"
    );
}

#[test]
fn verbose_lists_the_passing_and_inapplicable_rows_and_quiet_drops_them() {
    let server = fixture_site();
    let host = host_of(&server);
    let verbose = anc()
        .args(["web", &host, "--verbose"])
        .assert()
        .code(full_run_code(1));
    let out = stdout_of(verbose.get_output());
    assert!(out.contains("Passed ("), "{out}");
    assert!(out.contains("[PASS]"), "{out}");
    assert!(out.contains("Not applicable ("), "{out}");
    assert!(out.contains("[N/A "), "{out}");

    let quiet = anc()
        .args(["web", &host, "--quiet"])
        .assert()
        .code(full_run_code(1));
    let out = stdout_of(quiet.get_output());
    assert!(out.contains("Verdict: "), "{out}");
    assert!(out.contains("[WARN]"), "{out}");
    assert!(!out.contains("Passed:"), "{out}");
    assert!(!out.contains("[PASS]"), "{out}");
}

#[test]
fn json_mode_puts_nothing_but_the_scorecard_on_stdout() {
    let server = fixture_site();
    let host = host_of(&server);
    for flags in [vec!["--output", "json"], vec!["--json"]] {
        let mut args = vec!["web", host.as_str()];
        args.extend(flags.iter().copied());
        let assert = anc().args(&args).assert().code(full_run_code(1));
        let out = stdout_of(assert.get_output());
        let scorecard: WebScorecard = serde_json::from_str(&out)
            .unwrap_or_else(|e| panic!("stdout is a scorecard: {e}\n{out}"));
        assert_eq!(scorecard.results.len(), CHECKS.len());
        assert_eq!(scorecard.schema_version, "0.5");
        assert!(scorecard.target_url.starts_with("https://127.0.0.1:"));
        assert_eq!(scorecard.tool.url, scorecard.target_url);
        // Byte-identical to the mirror's own rendering, so the CLI adds
        // nothing to the wire shape.
        assert_eq!(
            out,
            agentnative::web_audit::scorecard::to_wire_json(&scorecard)
        );
        assert!(!out.contains("\u{1b}["), "no ANSI in JSON mode");
    }
}

/// anc.dev's wording under the score on every result page.
const SCORE_NOTE: &str = "relative to the checks that apply to this site; global measures it against a maximally agent-ready site.";

#[test]
fn the_score_line_says_what_relative_and_global_measure_unless_quiet() {
    let server = fixture_site();
    let host = host_of(&server);
    let assert = anc().args(["web", &host]).assert().code(full_run_code(1));
    let out = stdout_of(assert.get_output());
    let mut lines = out.lines().skip_while(|l| !l.starts_with("Score: "));
    assert!(lines.next().is_some(), "a score line:\n{out}");
    assert_eq!(
        lines.next(),
        Some(format!("       {SCORE_NOTE}").as_str()),
        "{out}"
    );

    let quiet = anc()
        .args(["web", &host, "--quiet"])
        .assert()
        .code(full_run_code(1));
    let out = stdout_of(quiet.get_output());
    assert!(out.contains("Score: "), "{out}");
    assert!(!out.contains("maximally agent-ready"), "{out}");
}

#[test]
fn web_help_says_what_the_two_scores_measure() {
    let help = anc().args(["web", "--help"]).assert().code(0);
    let out = stdout_of(help.get_output());
    let prose = out.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        prose.contains(&format!(
            "two scores: relative scores the site {SCORE_NOTE}"
        )),
        "{out}"
    );
}

#[test]
fn no_color_strips_styling_and_color_always_adds_it() {
    let server = fixture_site();
    let host = host_of(&server);
    // `NO_COLOR` decides in `auto`, the default, exactly as it does on the
    // audit path; an explicit `--color always` still wins over it.
    let plain = anc()
        .args(["web", &host])
        .env("NO_COLOR", "1")
        .assert()
        .code(full_run_code(1));
    assert!(
        !stdout_of(plain.get_output()).contains("\u{1b}["),
        "NO_COLOR must strip styling"
    );
    let never = anc()
        .args(["web", &host, "--color", "never"])
        .env_remove("NO_COLOR")
        .assert()
        .code(full_run_code(1));
    assert!(
        !stdout_of(never.get_output()).contains("\u{1b}["),
        "--color never must strip styling"
    );
    let styled = anc()
        .args(["web", &host, "--color", "always"])
        .env_remove("NO_COLOR")
        .assert()
        .code(full_run_code(1));
    assert!(
        stdout_of(styled.get_output()).contains("\u{1b}["),
        "--color always must style"
    );
}

#[test]
fn progress_stays_off_when_stdout_is_not_a_terminal() {
    let server = fixture_site();
    let assert = anc()
        .args(["web", &host_of(&server)])
        .assert()
        .code(full_run_code(1));
    let err = stderr_of(assert.get_output());
    assert!(
        err.is_empty(),
        "a piped run narrates nothing on stderr: {err}"
    );
}

/// The README's exit-code table is the published contract for the codes the
/// binary returns, so it is asserted against the constants rather than read
/// by eye. Every code in the table appears with the phrase the renderer's
/// own table uses, and `--help` carries the same four.
#[test]
fn the_readme_exit_table_matches_the_codes_the_binary_returns() {
    use agentnative::web_audit::render::EXIT_TABLE;

    let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))
        .expect("the README ships with the crate");
    let phrases = [
        (0, "clean"),
        (1, "warnings only"),
        (2, "failures present"),
        (3, "could not check"),
    ];
    for (code, phrase) in phrases {
        // The renderer's table is the source; the README and `--help` quote it.
        assert!(
            EXIT_TABLE
                .lines()
                .any(|l| l.trim_start().starts_with(&code.to_string()) && l.contains(phrase)),
            "the renderer's table lost `{code}  {phrase}`:\n{EXIT_TABLE}"
        );
        let row = readme
            .lines()
            .find(|l| l.starts_with(&format!("| {code}    |")))
            .unwrap_or_else(|| panic!("the README's exit table has no row for {code}"));
        assert!(
            row.to_lowercase().contains(phrase),
            "the README's row for {code} does not say {phrase:?}: {row}"
        );
    }
    let help = anc().args(["web", "--help"]).assert().code(0);
    let out = stdout_of(help.get_output()).to_lowercase();
    for (code, phrase) in phrases {
        assert!(
            out.contains(&format!("{code}  {phrase}")),
            "`anc web --help` lost `{code}  {phrase}`"
        );
    }
}

/// Each scored corpus scenario's replay: its name, its text report as `anc web`
/// prints it, and the scorecard the report renders. A scored scenario that replays as
/// unreachable has no report to read; the ledger holds that outcome.
fn corpus_reports(verbose: bool) -> Vec<(String, String, WebScorecard)> {
    corpus_reports_with(verbose, None)
}

fn corpus_reports_with(
    verbose: bool,
    contract: Option<ContractCheck>,
) -> Vec<(String, String, WebScorecard)> {
    load_cases()
        .into_iter()
        .filter(|case| matches!(case.golden, Golden::Scorecard(_)))
        .filter_map(|case| {
            let RunOutcome::Complete(report) = replay(&case.scenario, Arc::new(default_handlers()))
            else {
                return None;
            };
            let out = format_text(
                &report,
                RenderOptions {
                    color: false,
                    quiet: false,
                    verbose,
                    contract,
                },
            );
            Some((case.name, out, report.scorecard))
        })
        .collect()
}

/// A section's outline: each row as its indent and check id, each lane
/// heading and lane note as written, the evidence and fix lines dropped.
fn outline(out: &str, heading: &str) -> Vec<String> {
    let section = out
        .split("\n\n")
        .find(|block| block.starts_with(heading))
        .unwrap_or_else(|| panic!("no {heading:?} section in:\n{out}"));
    section
        .lines()
        .skip(1)
        .filter_map(|line| {
            let indent = line.len() - line.trim_start().len();
            let text = line.trim_start();
            if indent <= 4 && text.starts_with('[') {
                let id = text
                    .rsplit(") (")
                    .nth(1)
                    .and_then(|tail| tail.rsplit(" (").next())
                    .unwrap_or_else(|| panic!("no check id in {line:?}"));
                Some(format!("{}{id}", " ".repeat(indent)))
            } else if indent <= 4 {
                Some(line.to_string())
            } else {
                None
            }
        })
        .collect()
}

/// The outline a section must have, from the run's rows alone: rows outside any
/// lane in the order given, then each lane with a row here, in display order, under
/// its heading and note, its rows in registry order. A lane's tally reads every row
/// in the report, so a lane split across sections rolls up the same in each.
fn expected_outline(rows: &[&ResultRow], all: &[ResultRow]) -> Vec<String> {
    let mut lines: Vec<String> = rows
        .iter()
        .filter(|row| lane_of(&row.id).is_none())
        .map(|row| format!("  {}", row.id))
        .collect();
    let category = category_name(LANE_CATEGORY).expect("the lane category has a name");
    for lane in MCP_LANES {
        let mut ids: Vec<&str> = rows
            .iter()
            .filter(|row| lane_of(&row.id) == Some(lane))
            .map(|row| row.id.as_str())
            .collect();
        if ids.is_empty() {
            continue;
        }
        ids.sort_by_key(|id| CHECKS.iter().position(|check| check.id == *id));
        let counted: Vec<&ResultRow> = all
            .iter()
            .filter(|row| lane_of(&row.id) == Some(lane) && is_scored(row.status))
            .collect();
        let tally = if counted.is_empty() {
            String::new()
        } else {
            let passed = counted
                .iter()
                .filter(|row| row.status == ScorecardStatus::Pass)
                .count();
            format!(" ({passed}/{} pass)", counted.len())
        };
        lines.push(format!("  {category} · {}{tally}", lane.label));
        lines.push(format!("    {}.", lane.note));
        lines.extend(ids.iter().map(|id| format!("    {id}")));
    }
    lines
}

/// A section holds exactly what the rule beside it selects. An empty one may not
/// print at all, and `--verbose` prints its sections regardless.
fn assert_section(
    out: &str,
    heading: &str,
    card: &WebScorecard,
    keep: impl Fn(&ResultRow) -> bool,
) {
    let rows: Vec<&ResultRow> = card.results.iter().filter(|row| keep(row)).collect();
    let printed = out.split("\n\n").any(|block| block.starts_with(heading));
    if rows.is_empty() {
        assert!(
            !printed || outline(out, heading).is_empty(),
            "nothing belongs in {heading:?}:\n{out}"
        );
    } else {
        assert_eq!(
            outline(out, heading),
            expected_outline(&rows, &card.results),
            "{heading:?}:\n{out}"
        );
        let count = format!("{heading}{}", rows.len());
        assert!(
            out.split("\n\n").any(|block| {
                block
                    .strip_prefix(&count)
                    .is_some_and(|rest| rest.starts_with([' ', ')']))
            }),
            "{count:?} heads the section:\n{out}"
        );
    }
}

fn is_prefixed(row: &ResultRow, prefix: &str) -> bool {
    prefix_for(row) == prefix
}

#[test]
fn mcp_rows_group_under_their_protocol_lanes_within_each_section() {
    let (mut lane_rows, mut notes) = (0, 0);
    for (name, out, card) in corpus_reports(false) {
        let endpoint = card.mcp_endpoint.as_deref();
        assert_section(&out, "Failures (", &card, |row| is_prefixed(row, "FAIL"));
        assert_section(&out, "Warnings (", &card, |row| is_prefixed(row, "WARN"));
        assert_section(&out, "Could not check (", &card, |row| {
            matches!(row.status, ScorecardStatus::Error | ScorecardStatus::Skip)
        });
        assert_section(&out, "Advisories (", &card, |row| {
            row.status == ScorecardStatus::Pass && advisory_note(row, endpoint).is_some()
        });
        // A lane's rows keep the row layout, two columns deeper.
        for row in card.results.iter().filter(|row| lane_of(&row.id).is_some()) {
            if is_prefixed(row, "FAIL") || is_prefixed(row, "WARN") {
                lane_rows += 1;
                let heading = format!(
                    "    [{}] {} ({}) ({})",
                    prefix_for(row),
                    row.label,
                    row.id,
                    row.keyword.as_str()
                );
                assert!(out.contains(&heading), "{name}: {heading:?}:\n{out}");
                if let Some(evidence) = row.evidence.as_deref().and_then(|e| e.lines().next()) {
                    assert!(
                        out.contains(&format!("{heading}\n           {evidence}")),
                        "{name}: {heading:?}:\n{out}"
                    );
                }
            }
            if row.status == ScorecardStatus::Pass
                && let Some(note) = advisory_note(row, endpoint)
            {
                notes += 1;
                assert!(
                    out.contains(&format!("\n           Note: {note}")),
                    "{name}: {note:?}:\n{out}"
                );
            }
        }
    }
    assert!(lane_rows > 0, "no corpus scenario misses a lane row");
    assert!(notes > 0, "no corpus scenario carries an advisory");
}

#[test]
fn verbose_groups_the_passing_and_inapplicable_mcp_rows_by_lane_too() {
    for (_, out, card) in corpus_reports(true) {
        assert_section(&out, "Passed (", &card, |row| {
            row.status == ScorecardStatus::Pass
        });
        assert_section(&out, "Not applicable (", &card, |row| {
            row.status == ScorecardStatus::NA
        });
    }
}

/// One scored scenario's report: the contract lines read the same on any of them.
fn one_report(verbose: bool, contract: Option<ContractCheck>) -> String {
    corpus_reports_with(verbose, contract)
        .into_iter()
        .next()
        .expect("a scored corpus scenario")
        .1
}

#[test]
fn a_contract_that_differs_is_a_note_naming_the_pin_and_no_upgrade() {
    let out = one_report(false, Some(ContractCheck::Differs));
    let note = out
        .lines()
        .find(|l| l.starts_with("Note: this anc's web-audit contract"))
        .unwrap_or_else(|| panic!("no contract note:\n{out}"));
    assert!(
        note.contains("differs from the one anc.dev serves"),
        "{note}"
    );
    assert!(note.contains(&SITE_SHA[..7]), "{note}");
    assert!(!note.contains("behind"), "{note}");
    assert!(!note.to_lowercase().contains("upgrade"), "{note}");
}

#[test]
fn a_contract_that_matches_or_went_unchecked_is_no_note() {
    for contract in [
        None,
        Some(ContractCheck::Same),
        Some(ContractCheck::NotChecked(NotChecked::Unreachable)),
    ] {
        let out = one_report(false, contract);
        assert!(!out.contains("web-audit contract"), "{contract:?}:\n{out}");
        assert!(!out.contains("anc.dev contract:"), "{contract:?}:\n{out}");
    }
}

#[test]
fn verbose_states_the_contract_result_every_time() {
    for (contract, line) in [
        (ContractCheck::Same, "  anc.dev contract: same"),
        (ContractCheck::Differs, "  anc.dev contract: differs"),
        (
            ContractCheck::NotChecked(NotChecked::Status(404)),
            "  anc.dev contract: not checked (anc.dev answered 404)",
        ),
        (
            ContractCheck::NotChecked(NotChecked::NotPublic),
            "  anc.dev contract: not checked (not a public target)",
        ),
    ] {
        let out = one_report(true, Some(contract));
        assert!(out.lines().any(|l| l == line), "no {line:?} in:\n{out}");
    }
}
