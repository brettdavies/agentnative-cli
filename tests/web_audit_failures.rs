//! Every `anc web` path that ends without a scorecard: what the run says
//! went wrong, what it says caused it, and what it tells the caller to do
//! next, in text and in the structured envelope.

#![cfg(feature = "web")]

mod common;

use common::web_cli::{anc, fixture_site, host_of, plaintext_site, stderr_of, stdout_of};
use serde_json::Value;

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
        err.contains("no HTTP response from https://127.0.0.1:9/"),
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
    assert!(err.contains("sends no plaintext request"), "{err}");
    assert!(!err.contains("http://"), "no hint may suggest http: {err}");

    // An explicit scheme carries no such hint.
    let assert = anc().args(["web", "https://127.0.0.1:9"]).assert().code(3);
    assert!(
        !stderr_of(assert.get_output()).contains("scheme defaulted"),
        "{}",
        stderr_of(assert.get_output())
    );
}

#[test]
fn an_http_target_ends_unreachable_before_any_request() {
    let server = plaintext_site();
    let target = server.url("/");
    let assert = anc().args(["web", &target]).assert().code(3);
    let err = stderr_of(assert.get_output());
    assert!(
        err.contains(&format!(
            "error: {target} is not https, and anc sends no plaintext request."
        )),
        "{err}"
    );
    assert!(err.trim_end().ends_with("exit 3: could not check"), "{err}");

    let assert = anc()
        .args(["web", &target, "--output", "json"])
        .assert()
        .code(3);
    let envelope: Value = serde_json::from_str(stderr_of(assert.get_output()).trim()).unwrap();
    assert_eq!(envelope["kind"], Value::from("runtime"));
    assert_eq!(envelope["error"], Value::from("insecure-scheme"));
    assert_eq!(envelope["value"], Value::from(target.as_str()));
    assert_eq!(envelope["next_step"]["action"], Value::from("fix-target"));
    assert!(server.hits().is_empty(), "no plaintext request may go out");
}

#[test]
fn a_root_redirecting_to_http_ends_unreachable_without_following() {
    let plaintext = plaintext_site();
    let location = plaintext.url("/");
    let server = common::spawn_https(move |_| {
        common::RawResponse::new(302, &[("location", location.as_str())], b"")
    });
    let target = server.url("/");
    let assert = anc().args(["web", &target]).assert().code(3);
    let err = stderr_of(assert.get_output());
    assert!(
        err.contains(&format!(
            "error: {target} redirects to http, and anc sends no plaintext request."
        )),
        "{err}"
    );
    assert_eq!(server.hits().len(), 1, "the root request alone");
    assert!(plaintext.hits().is_empty(), "the http hop is never taken");
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
    assert!(err.trim_end().ends_with("exit 2: usage error"), "{err}");
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

/// The `next_step` keys, in order, of a failing `--output json` run.
fn next_step_keys(args: &[&str]) -> Vec<String> {
    let output = anc().args(args).output().expect("spawn anc");
    assert!(!output.status.success(), "{output:?}");
    let envelope: Value = serde_json::from_str(stderr_of(&output).trim()).unwrap();
    envelope["next_step"]
        .as_object()
        .unwrap_or_else(|| panic!("no next_step object: {envelope}"))
        .keys()
        .cloned()
        .collect()
}

#[test]
fn anc_web_and_anc_audit_errors_carry_the_same_next_step_keys() {
    let audit = next_step_keys(&["audit", "/nonexistent/anc-target", "--output", "json"]);
    assert_eq!(audit, ["action", "command", "docs"]);
    for web in [
        &["web", "127.0.0.1:9", "--output", "json"][..],
        &["web", "ftp://example.test/x", "--output", "json"][..],
        &[
            "web",
            "127.0.0.1:9",
            "--check",
            "no-such-check",
            "--output",
            "json",
        ][..],
    ] {
        assert_eq!(next_step_keys(web), audit, "{web:?}");
    }
}

#[test]
fn the_web_failure_envelopes_pin_their_bytes() {
    let stderr = |args: &[&str]| stderr_of(&anc().args(args).output().expect("spawn anc"));
    let docs = "https://github.com/brettdavies/agentnative-cli#auditing-a-website";
    assert_eq!(
        stderr(&["web", "127.0.0.1:9", "--output", "json"]),
        format!(
            "{}{docs}{}\n",
            r#"{"kind":"runtime","error":"target-unreachable","message":"https://127.0.0.1:9/ did not answer any probe","exit_code":3,"next_step":{"action":"retry","command":"anc web 127.0.0.1:9 --verbose","docs":""#,
            r#""},"argument":"<TARGET>","value":"https://127.0.0.1:9/"}"#,
        )
    );
    assert_eq!(
        stderr(&["web", "ftp://example.test/x", "--output", "json"]),
        format!(
            "{}{docs}{}\n",
            r#"{"kind":"runtime","error":"invalid-target","message":"cannot audit ftp://example.test/x: unsupported scheme: only http and https targets can be audited","exit_code":3,"next_step":{"action":"fix-target","command":"anc web --help","docs":""#,
            r#""},"argument":"<TARGET>","value":"ftp://example.test/x"}"#,
        )
    );
    assert_eq!(
        stderr(&[
            "web",
            "127.0.0.1:9",
            "--check",
            "no-such-check",
            "--output",
            "json"
        ]),
        format!(
            "{}{docs}{}\n",
            r#"{"kind":"usage","error":"unknown-check","message":"no such check: no-such-check","exit_code":2,"next_step":{"action":"list-checks","command":"anc emit web-checks","docs":""#,
            r#""},"argument":"--check <ID>","value":"no-such-check"}"#,
        )
    );
}

#[test]
fn a_retry_command_quotes_a_target_a_shell_would_split() {
    let assert = anc()
        .args(["web", "https://127.0.0.1:9/?a=1&b=2", "--output", "json"])
        .assert()
        .code(3);
    let envelope: Value = serde_json::from_str(stderr_of(assert.get_output()).trim()).unwrap();
    assert_eq!(
        envelope["next_step"]["command"],
        Value::from("anc web 'https://127.0.0.1:9/?a=1&b=2' --verbose")
    );
}
