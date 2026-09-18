//! The `anc web` report contract: what a run prints, in what order, and
//! what the published exit-code table says it means. The failure paths
//! live in `web_audit_failures.rs` and the flag surface in
//! `web_audit_cli.rs`.

mod common;

use agentnative::web_audit::scorecard::WebScorecard;
use common::web_cli::{anc, fixture_site, host_of, stderr_of, stdout_of};

#[test]
fn a_reachable_site_reports_the_verdict_then_its_misses_with_fixes() {
    let server = fixture_site();
    let assert = anc().args(["web", &host_of(&server)]).assert().code(1);
    let out = stdout_of(assert.get_output());

    assert!(out.starts_with("anc web http://"), "{out}");
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
        assert_eq!(scorecard.results.len(), 65);
        assert_eq!(scorecard.schema_version, "0.4");
        assert!(scorecard.target_url.starts_with("http://127.0.0.1:"));
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
