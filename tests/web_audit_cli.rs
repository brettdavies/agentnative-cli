//! The `anc web` surface: the flags, the single-check form, the routing of
//! a bare target, the offline readers and the help text. The report itself
//! is pinned in `web_audit_render.rs`, the dead ends in
//! `web_audit_failures.rs`.

mod common;

use std::process::Command as StdCommand;

use agentnative::web_audit::scorecard::WebScorecard;
use common::web_cli::{anc, api_site, fixture_site, host_of, stderr_of, stdout_of};
use serde_json::Value;

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
