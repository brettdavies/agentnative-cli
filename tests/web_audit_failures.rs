//! Every `anc web` path that ends without a scorecard: what the run says
//! went wrong, what it says caused it, and what it tells the caller to do
//! next, in text and in the structured envelope.

mod common;

use common::web_cli::{anc, fixture_site, host_of, stderr_of, stdout_of};
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
