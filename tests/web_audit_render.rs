//! The `anc web` report contract: what a run prints, in what order, and
//! what the published exit-code table says it means. The failure paths
//! live in `web_audit_failures.rs` and the flag surface in
//! `web_audit_cli.rs`.

#![cfg(feature = "web")]

mod common;

use std::sync::Arc;

use agentnative::web_audit::engine::RunOutcome;
use agentnative::web_audit::handlers::default_handlers;
use agentnative::web_audit::render::{RenderOptions, format_text};
use agentnative::web_audit::scorecard::WebScorecard;
use common::corpus::{load_cases, replay};
use common::web_cli::{anc, fixture_site, host_of, stderr_of, stdout_of};

#[test]
fn a_reachable_site_reports_the_verdict_then_its_misses_with_fixes() {
    let server = fixture_site();
    let assert = anc().args(["web", &host_of(&server)]).assert().code(1);
    let out = stdout_of(assert.get_output());

    assert!(out.starts_with("anc web https://"), "{out}");
    assert!(out.contains("registry v1 (anc.dev "), "{out}");
    assert!(out.contains("spec "), "{out}");
    assert!(out.contains("mcp endpoint: none"), "{out}");
    // The verdict leads, before any row.
    let verdict = out.find("Verdict: WARN").expect("a verdict line");
    let first_row = out.find("[WARN]").expect("a warning row");
    assert!(
        verdict < first_row,
        "the verdict must precede the rows:\n{out}"
    );
    assert!(out.contains("warnings only ("), "{out}");
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
    let closing = out.trim_end().lines().last().unwrap_or_default();
    assert!(
        closing.starts_with("exit 1: warnings only ("),
        "the closing line names the code and what earned it: {closing:?}"
    );
    assert!(closing.ends_with("misses)"), "{closing:?}");
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
    let verbose = anc().args(["web", &host, "--verbose"]).assert().code(1);
    let out = stdout_of(verbose.get_output());
    assert!(out.contains("Passed ("), "{out}");
    assert!(out.contains("[PASS]"), "{out}");
    assert!(out.contains("Not applicable ("), "{out}");
    assert!(out.contains("[N/A "), "{out}");

    let quiet = anc().args(["web", &host, "--quiet"]).assert().code(1);
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
        let assert = anc().args(&args).assert().code(1);
        let out = stdout_of(assert.get_output());
        let scorecard: WebScorecard = serde_json::from_str(&out)
            .unwrap_or_else(|e| panic!("stdout is a scorecard: {e}\n{out}"));
        assert_eq!(scorecard.results.len(), 68);
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
        .code(1);
    assert!(
        !stdout_of(plain.get_output()).contains("\u{1b}["),
        "NO_COLOR must strip styling"
    );
    let never = anc()
        .args(["web", &host, "--color", "never"])
        .env_remove("NO_COLOR")
        .assert()
        .code(1);
    assert!(
        !stdout_of(never.get_output()).contains("\u{1b}["),
        "--color never must strip styling"
    );
    let styled = anc()
        .args(["web", &host, "--color", "always"])
        .env_remove("NO_COLOR")
        .assert()
        .code(1);
    assert!(
        stdout_of(styled.get_output()).contains("\u{1b}["),
        "--color always must style"
    );
}

#[test]
fn progress_stays_off_when_stdout_is_not_a_terminal() {
    let server = fixture_site();
    let assert = anc().args(["web", &host_of(&server)]).assert().code(1);
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

/// The text report of a corpus scenario's replay, as `anc web` prints it.
fn corpus_report(name: &str, verbose: bool) -> String {
    let case = load_cases()
        .into_iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no corpus scenario {name}"));
    match replay(&case.scenario, Arc::new(default_handlers())) {
        RunOutcome::Complete(report) => format_text(
            &report,
            RenderOptions {
                color: false,
                quiet: false,
                verbose,
            },
        ),
        RunOutcome::Unreachable { reason, .. } => panic!("{name} reported unreachable: {reason}"),
    }
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
            if text.starts_with('[') {
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

const SHARED: [&str; 2] = [
    "  MCP · Every MCP server (4/4 pass)",
    "    transport, discovery, and card checks that apply whichever protocol the server speaks.",
];
const LEGACY: [&str; 2] = [
    "  MCP · Legacy lane · 2025-06-18 (8/10 pass)",
    "    initialize, then a session.",
];
const MODERN: [&str; 2] = [
    "  MCP · Modern lane · 2026-07-28 (0/7 pass)",
    "    header-routed and stateless; no initialize or session.",
];
const BROWSER: [&str; 2] = [
    "  MCP · In-page tools · WebMCP (1/1 pass)",
    "    browser tools exposed by the site's HTML, not by the MCP server.",
];

fn lines(parts: &[&[&str]]) -> Vec<String> {
    parts.concat().into_iter().map(str::to_string).collect()
}

/// The position of the first lane heading in a section outline.
fn first_lane(outline: &[String]) -> usize {
    outline
        .iter()
        .position(|line| line.starts_with("  MCP · "))
        .unwrap_or_else(|| panic!("no lane heading in {outline:#?}"))
}

#[test]
fn mcp_rows_group_under_their_protocol_lanes_within_each_section() {
    let out = corpus_report("mcp-negotiation-defects", false);
    assert_eq!(
        outline(&out, "Failures ("),
        lines(&[&MODERN, &["    mcp-modern-tools-list"]]),
        "{out}"
    );
    assert_eq!(
        outline(&out, "Warnings ("),
        lines(&[
            &[
                "  json-errors",
                "  rate-limit-headers",
                "  llms-txt",
                "  accept-markdown",
                "  robots",
                "  agent-friendly-404-md",
                "  link-headers",
            ],
            &LEGACY,
            &["    mcp-accept-json", "    mcp-accept-unsatisfiable"],
            &MODERN,
            &[
                "    mcp-server-discover",
                "    mcp-modern-unknown-method",
                "    mcp-modern-clientcaps",
                "    mcp-modern-header-mismatch",
                "    mcp-modern-version-reject",
                "    mcp-modern-resources-miss",
            ],
        ]),
        "{out}"
    );
    assert_eq!(
        outline(&out, "Advisories ("),
        lines(&[&SHARED, &["    mcp-server-card"]]),
        "{out}"
    );
    // A lane's rows keep the row layout, two columns deeper.
    assert!(
        out.contains(
            "    [WARN] a JSON-only Accept is answered without SSE framing (mcp-accept-json) (should)\n           https://example.com/mcp -> 200"
        ),
        "{out}"
    );
    assert!(
        out.contains("\n           Note: Superseded shape (SEP-1649)."),
        "{out}"
    );
}

#[test]
fn verbose_groups_the_passing_and_inapplicable_mcp_rows_by_lane_too() {
    let out = corpus_report("mcp-negotiation-defects", true);
    let passed = outline(&out, "Passed (");
    let lane_at = first_lane(&passed);
    assert!(
        passed[..lane_at]
            .iter()
            .all(|line| line.starts_with("  ") && !line.starts_with("   ")),
        "the rows outside any lane come first:\n{out}"
    );
    assert_eq!(
        passed[lane_at..],
        lines(&[
            &SHARED,
            &[
                "    mcp-get-fast-fail",
                "    mcp-server-card",
                "    mcp-card-legacy-aliases",
                "    mcp-usage-doc",
            ],
            &LEGACY,
            &[
                "    mcp-initialize",
                "    mcp-capabilities",
                "    mcp-tools-list",
                "    mcp-resources-list",
                "    mcp-unknown-method",
                "    mcp-malformed-body",
                "    mcp-batch-reject",
                "    mcp-unknown-tool",
            ],
            &BROWSER,
            &["    webmcp"],
        ])[..],
        "{out}"
    );
    let inapplicable = outline(&out, "Not applicable (");
    assert_eq!(
        inapplicable[first_lane(&inapplicable)..],
        lines(&[
            &SHARED,
            &[
                "    mcp-cors-preflight",
                "    mcp-cors-actual",
                "    mcp-auth-challenge",
                "    mcp-auth-servers",
                "    mcp-auth-enforced",
            ],
        ])[..],
        "{out}"
    );
}
