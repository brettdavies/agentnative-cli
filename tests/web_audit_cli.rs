//! `anc web` end to end: the real binary against an in-test server, the
//! exit table on every code, the report contract, the single-check form,
//! the offline readers, and every dead end's envelope and next action.

mod common;

use std::process::Command as StdCommand;

use agentnative::web_audit::scorecard::WebScorecard;
use assert_cmd::Command;
use common::{RawRequest, RawResponse, Server};
use serde_json::Value;

fn anc() -> Command {
    Command::cargo_bin("anc").expect("binary should exist")
}

const HTML_HEAD: &str = "<!doctype html><html><head>\
<meta name=\"description\" content=\"A fixture site for the web-audit CLI tests.\">\
<link rel=\"alternate\" type=\"text/markdown\" href=\"/index.md\">\
</head><body><main><h1>Fixture</h1><p>";
const HTML_TAIL: &str =
    "</p></main><noscript><a href=\"/llms.txt\">llms.txt</a></noscript></body></html>";

fn html_body() -> String {
    let prose = "Readable prose about the fixture service and its agent surfaces. ".repeat(6);
    format!("{HTML_HEAD}{prose}{HTML_TAIL}")
}

fn ok(content_type: &str, body: &str) -> RawResponse {
    RawResponse::new(200, &[("content-type", content_type)], body.as_bytes())
}

fn not_found() -> RawResponse {
    RawResponse::new(
        404,
        &[("content-type", "text/html; charset=utf-8")],
        b"<html><body><h1>Not found</h1></body></html>",
    )
}

/// A site that publishes llms.txt and robots.txt and serves HTML at the
/// root: every MUST applicable to it passes, and a handful of SHOULDs miss.
fn fixture_site() -> Server {
    common::spawn(|req: &RawRequest| {
        let path = req.target.split('?').next().unwrap_or("/");
        match path {
            "/" => ok("text/html; charset=utf-8", &html_body()),
            "/llms.txt" => ok(
                "text/plain; charset=utf-8",
                "# Fixture\n\n> A fixture site for the audit.\n\n## When to use\n\n- [Home](/)\n",
            ),
            "/robots.txt" => ok("text/plain", "User-agent: *\nAllow: /\n"),
            _ => not_found(),
        }
    })
}

/// A site whose root is a MUST-bearing API surface that answers wrongly,
/// so the run has a real failure to report.
fn api_site() -> Server {
    common::spawn(|req: &RawRequest| {
        let path = req.target.split('?').next().unwrap_or("/");
        match path {
            "/" => ok("text/html; charset=utf-8", &html_body()),
            // A declared API with no OpenAPI document: openapi is a MUST.
            "/llms.txt" => ok(
                "text/plain; charset=utf-8",
                "# API fixture\n\n> An API.\n\n- [API](/api/v1/things)\n",
            ),
            _ => not_found(),
        }
    })
}

fn host_of(server: &Server) -> String {
    server.addr.to_string()
}

fn stdout_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

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
fn a_must_miss_is_a_failure_and_a_clean_run_is_zero() {
    let server = api_site();
    let assert = anc()
        .args(["web", &host_of(&server), "--site-type", "api"])
        .assert()
        .code(2);
    let out = stdout_of(assert.get_output());
    assert!(out.contains("Verdict: FAIL"), "{out}");
    assert!(out.contains("Failures ("), "{out}");
    assert!(out.contains("MUST miss"), "{out}");
    assert!(out.contains("(openapi)"), "{out}");
    assert!(out.trim_end().ends_with("MUST miss)"), "{out}");

    // One passing check, scoped to itself, is a clean run.
    let server = fixture_site();
    anc()
        .args(["web", &host_of(&server), "--check", "llms-txt"])
        .assert()
        .code(0)
        .stdout(predicates_str("llms-txt\tpass\t"));
}

fn predicates_str(needle: &'static str) -> impl predicates::Predicate<str> {
    predicates::str::starts_with(needle)
}

#[test]
fn the_single_check_form_returns_that_rows_code() {
    let server = fixture_site();
    let host = host_of(&server);
    let cases: [(&str, i32, &str); 4] = [
        ("llms-txt", 0, "pass"),
        ("accept-markdown", 1, "absent"),
        ("openapi", 3, "n_a"),
        ("dns-aid", 3, "n_a"),
    ];
    for (id, code, status) in cases {
        let assert = anc()
            .args(["web", &host, "--check", id])
            .assert()
            .code(code);
        let out = stdout_of(assert.get_output());
        let fields: Vec<&str> = out.trim_end().split('\t').collect();
        assert_eq!(fields[0], id, "{out}");
        assert_eq!(fields[1], status, "{out}");
        assert_eq!(fields.len(), 3, "{out}");
    }
    // The withheld DNS row states why rather than failing.
    let assert = anc().args(["web", &host, "--check", "dns-aid"]).assert();
    assert!(
        stdout_of(assert.get_output()).contains("needs public DNS: verified on anc.dev"),
        "{}",
        stdout_of(assert.get_output())
    );
    // JSON keeps the one scorecard shape; the code is what narrows.
    let assert = anc()
        .args(["web", &host, "--check", "openapi", "--output", "json"])
        .assert()
        .code(3);
    let scorecard: WebScorecard = serde_json::from_str(&stdout_of(assert.get_output())).unwrap();
    assert_eq!(scorecard.results.len(), 65);
}

/// Declaring a type both narrows and widens: rows typed for the other
/// family gate out, and rows whose antecedent is "this site has an API"
/// gate in, because the declaration is what satisfies it. So this pins
/// which rows carry the declared-type line rather than counting them.
#[test]
fn the_declared_site_type_scopes_the_run() {
    use agentnative::web_audit::scorecard::{DeclaredSiteType, ScorecardStatus as S};

    let server = fixture_site();
    let host = host_of(&server);
    let run = |args: &[&str]| -> WebScorecard {
        let mut all = vec!["web", host.as_str(), "--output", "json"];
        all.extend(args.iter().copied());
        let assert = anc().args(&all).assert();
        serde_json::from_str(&stdout_of(assert.get_output())).unwrap()
    };
    let type_gated = |card: &WebScorecard| -> Vec<String> {
        card.results
            .iter()
            .filter(|r| r.evidence.as_deref() == Some("not applicable to the declared site type"))
            .map(|r| r.id.clone())
            .collect()
    };
    let status_of = |card: &WebScorecard, id: &str| -> S {
        card.results
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.status)
            .unwrap_or_else(|| panic!("no row {id}"))
    };

    let any = run(&[]);
    assert_eq!(any.site_type, None);
    assert!(
        type_gated(&any).is_empty(),
        "with no declaration nothing gates on type: {:?}",
        type_gated(&any)
    );

    let content = run(&["--site-type", "content"]);
    assert_eq!(content.site_type, Some(DeclaredSiteType::Content));
    let gated = type_gated(&content);
    assert!(gated.contains(&"openapi".to_string()), "{gated:?}");
    assert!(gated.contains(&"json-errors".to_string()), "{gated:?}");
    assert!(!gated.contains(&"llms-full-txt".to_string()), "{gated:?}");
    assert!(!gated.contains(&"robots".to_string()), "{gated:?}");

    let api = run(&["--site-type", "api"]);
    assert_eq!(api.site_type, Some(DeclaredSiteType::Api));
    let gated = type_gated(&api);
    assert!(gated.contains(&"llms-full-txt".to_string()), "{gated:?}");
    assert!(gated.contains(&"llms-txt-scoped".to_string()), "{gated:?}");
    assert!(!gated.contains(&"openapi".to_string()), "{gated:?}");
    assert!(!gated.contains(&"robots".to_string()), "{gated:?}");

    // Declaring an API is what makes the API rows applicable at all: this
    // fixture publishes no OpenAPI document, so the row is inapplicable
    // until the caller says the site has an API, and then it is a miss.
    assert_eq!(status_of(&any, "openapi"), S::NA);
    assert_eq!(status_of(&api, "openapi"), S::Absent);
    // A row every site answers for is untouched by the declaration.
    for card in [&any, &content, &api] {
        assert_eq!(status_of(card, "robots"), S::Pass);
    }
}

#[test]
fn an_unreachable_target_names_the_problem_the_cause_and_the_next_command() {
    // A port nothing listens on: the connection is refused at once.
    let dead = "127.0.0.1:9";
    let assert = anc().args(["web", dead]).assert().code(3);
    let err = stderr_of(assert.get_output());
    assert!(
        stdout_of(assert.get_output()).is_empty(),
        "stdout must stay empty"
    );
    assert!(err.contains("did not answer any probe"), "{err}");
    assert!(
        err.contains("no HTTP response from http://127.0.0.1:9/"),
        "{err}"
    );
    assert!(err.contains("check that the server is running"), "{err}");
    assert!(err.contains("next: anc web 127.0.0.1:9 --verbose"), "{err}");
    assert!(err.trim_end().ends_with("exit 3: could not check"), "{err}");

    let assert = anc()
        .args(["web", dead, "--output", "json"])
        .assert()
        .code(3);
    assert!(
        stdout_of(assert.get_output()).is_empty(),
        "stdout must stay empty"
    );
    let envelope: Value = serde_json::from_str(stderr_of(assert.get_output()).trim()).unwrap();
    assert_eq!(envelope["kind"], Value::from("runtime"));
    assert_eq!(envelope["error"], Value::from("target-unreachable"));
    assert_eq!(envelope["exit_code"], Value::from(3));
    assert_eq!(envelope["next_step"]["action"], Value::from("retry"));
    assert_eq!(
        envelope["next_step"]["command"],
        Value::from("anc web 127.0.0.1:9 --verbose")
    );
    assert!(envelope["next_step"]["docs"].as_str().is_some());
    assert!(
        envelope["message"]
            .as_str()
            .unwrap()
            .contains("127.0.0.1:9")
    );
}

#[test]
fn a_scheme_less_dot_bearing_target_says_the_scheme_defaulted() {
    // `.invalid` never resolves (RFC 2606), so the run fails without
    // reaching anything, and the hint names the https default.
    let assert = anc()
        .args(["web", "anc-web-audit-nonexistent.invalid"])
        .assert()
        .code(3);
    let err = stderr_of(assert.get_output());
    assert!(err.contains("the scheme defaulted to https"), "{err}");
    assert!(
        err.contains("anc web http://anc-web-audit-nonexistent.invalid"),
        "{err}"
    );

    // An explicit scheme carries no such hint.
    let assert = anc().args(["web", "http://127.0.0.1:9"]).assert().code(3);
    assert!(
        !stderr_of(assert.get_output()).contains("scheme defaulted"),
        "{}",
        stderr_of(assert.get_output())
    );
}

#[test]
fn an_invalid_target_and_an_unknown_check_fail_before_any_connection() {
    let server = fixture_site();
    let assert = anc().args(["web", "ftp://example.test/x"]).assert().code(3);
    let err = stderr_of(assert.get_output());
    assert!(
        err.contains("only http and https targets can be audited"),
        "{err}"
    );
    assert!(err.contains("next: anc web --help"), "{err}");

    let assert = anc()
        .args(["web", &host_of(&server), "--check", "no-such-check"])
        .assert()
        .code(2);
    let err = stderr_of(assert.get_output());
    assert!(err.contains("no such check: no-such-check"), "{err}");
    assert!(err.contains("anc emit web-checks"), "{err}");
    assert!(server.hits().is_empty(), "an unknown id must probe nothing");

    let assert = anc()
        .args([
            "web",
            &host_of(&server),
            "--check",
            "no-such-check",
            "--output",
            "json",
        ])
        .assert()
        .code(2);
    let envelope: Value = serde_json::from_str(stderr_of(assert.get_output()).trim()).unwrap();
    assert_eq!(envelope["error"], Value::from("unknown-check"));
    assert_eq!(envelope["next_step"]["action"], Value::from("list-checks"));
    assert_eq!(
        envelope["next_step"]["command"],
        Value::from("anc emit web-checks")
    );
}

#[test]
fn a_bare_network_target_routes_to_web_and_a_filename_stays_offline() {
    let server = fixture_site();
    // The sniff routes a host:port token to `web`.
    anc()
        .args([&host_of(&server), "--check", "llms-txt"])
        .assert()
        .code(0)
        .stdout(predicates_str("llms-txt\tpass\t"));
    assert!(!server.hits().is_empty());

    // A dot-bearing filename that does not exist fails through the audit
    // path with no network activity at all.
    let probe = fixture_site();
    let assert = anc().args(["anc-web-audit-report.json"]).assert().code(2);
    assert!(
        stderr_of(assert.get_output()).contains("path does not exist"),
        "{}",
        stderr_of(assert.get_output())
    );
    assert!(probe.hits().is_empty());

    // An explicit path form routes to audit even when the name is
    // URL-shaped.
    anc()
        .args(["./anc-web-audit-nonexistent.dev"])
        .assert()
        .code(2)
        .stderr(predicates::str::contains("path does not exist"));
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

#[test]
fn the_offline_readers_serve_the_compiled_registry() {
    let checks = anc().args(["emit", "web-checks"]).assert().code(0);
    let doc: Value = serde_json::from_str(&stdout_of(checks.get_output())).unwrap();
    let ids: Vec<&str> = doc["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 65);
    // Every id the reader lists is an id `--check` accepts.
    let server = fixture_site();
    let host = host_of(&server);
    for id in [ids[0], ids[ids.len() / 2], ids[ids.len() - 1]] {
        let assert = anc().args(["web", &host, "--check", id]).assert();
        assert!(
            stdout_of(assert.get_output()).starts_with(&format!("{id}\t")),
            "{id}"
        );
    }

    let schema = anc().args(["emit", "web-schema"]).assert().code(0);
    let schema_doc: Value = serde_json::from_str(&stdout_of(schema.get_output())).unwrap();
    assert!(
        schema_doc["$id"]
            .as_str()
            .unwrap()
            .contains("web-scorecard")
    );
    // The schema describes what the verb emits.
    let run = anc()
        .args(["web", &host, "--output", "json"])
        .assert()
        .code(1);
    let scorecard: Value = serde_json::from_str(&stdout_of(run.get_output())).unwrap();
    let required = schema_doc["required"].as_array().unwrap();
    for key in required {
        assert!(
            scorecard.get(key.as_str().unwrap()).is_some(),
            "emitted scorecard lacks the schema's required key {key}"
        );
    }

    let fixes = anc().args(["emit", "web-remediation"]).assert().code(0);
    let catalog: Value = serde_json::from_str(&stdout_of(fixes.get_output())).unwrap();
    let entries = catalog["remediation"].as_array().unwrap();
    assert_eq!(entries.len(), 65);
    assert!(
        entries
            .iter()
            .all(|e| !e["fix"].as_str().unwrap().is_empty())
    );
}

#[test]
fn help_carries_the_exit_table_and_the_external_dns_disclosure() {
    let help = anc().args(["web", "--help"]).assert().code(0);
    let out = stdout_of(help.get_output());
    for line in [
        "0  clean",
        "1  warnings only",
        "2  failures present",
        "3  could not check",
    ] {
        assert!(
            out.contains(line),
            "`anc web --help` lacks `{line}`:\n{out}"
        );
    }
    assert!(out.contains("--external-dns"), "{out}");
    assert!(out.contains("public resolvers"), "{out}");
    assert!(out.contains("--site-type"), "{out}");
    assert!(out.contains("--check"), "{out}");

    let top = anc().args(["--help"]).assert().code(0);
    let out = stdout_of(top.get_output());
    assert!(out.contains("anc web"), "{out}");
    assert!(out.contains("3  could not check"), "{out}");
}

#[test]
fn the_web_verb_appears_in_shell_completions() {
    let completions = anc().args(["completions", "bash"]).assert().code(0);
    let out = stdout_of(completions.get_output());
    assert!(out.contains("web"), "completions must offer the web verb");
    assert!(out.contains("web-checks"), "{}", &out[..out.len().min(400)]);
}

/// The binary must not need a shell to run; `StdCommand` here proves the
/// verb works through a plain exec with no environment prepared for it.
#[test]
fn the_verb_runs_under_a_bare_exec() {
    let server = fixture_site();
    let exe = assert_cmd::cargo::cargo_bin("anc");
    let out = StdCommand::new(exe)
        .args(["web", &host_of(&server), "--check", "llms-txt"])
        .env_clear()
        .output()
        .expect("spawn anc");
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("llms-txt\tpass\t"));
}
