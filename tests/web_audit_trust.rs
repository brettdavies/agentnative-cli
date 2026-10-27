//! Which certificate chains `anc web` trusts: a private CA is rejected with
//! its own evidence until `--ca-cert` or the OS trust store vouches for it,
//! and a `--ca-cert` file that holds no usable root is a usage error raised
//! before any request.

#![cfg(feature = "web")]

mod common;

use common::web_cli::{anc, fixture_site, stderr_of, stdout_of};
use common::{empty_home_config, test_ca_path};
use serde_json::Value;

fn envelope(output: &std::process::Output) -> Value {
    serde_json::from_str(stderr_of(output).trim())
        .unwrap_or_else(|e| panic!("stderr is not one JSON envelope ({e}): {output:?}"))
}

/// The registry's row count, the length of every complete scorecard.
fn registry_rows() -> usize {
    let output = anc().args(["emit", "web-checks"]).output().unwrap();
    let checks: Value = serde_json::from_str(&stdout_of(&output)).unwrap();
    checks["checks"].as_array().unwrap().len()
}

#[test]
fn a_private_ca_ends_the_run_naming_the_rejection_and_the_ca_cert_fix() {
    let server = fixture_site();
    let target = server.url("/");
    let output = anc()
        .env_remove("SSL_CERT_FILE")
        .args(["web", &target])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(stdout_of(&output).is_empty(), "stdout must stay empty");
    let err = stderr_of(&output);
    assert!(
        err.contains("presented a certificate chain this run does not trust"),
        "{err}"
    );
    assert!(
        err.contains("TlsError: chain rejected by bundled roots and the OS trust store ("),
        "{err}"
    );
    assert!(err.contains("--ca-cert <FILE>"), "{err}");
    assert!(err.trim_end().ends_with("exit 3: could not check"), "{err}");
    assert!(
        server.hits().is_empty(),
        "nothing may cross a rejected handshake"
    );

    let output = anc()
        .env_remove("SSL_CERT_FILE")
        .args(["web", &target, "--output", "json"])
        .output()
        .unwrap();
    let envelope = envelope(&output);
    assert_eq!(envelope["kind"], "runtime");
    assert_eq!(envelope["error"], "untrusted-certificate");
    assert_eq!(envelope["exit_code"], 3);
    assert_eq!(envelope["argument"], "<TARGET>");
    assert_eq!(envelope["value"], Value::from(target.as_str()));
    assert_eq!(envelope["next_step"]["action"], "add-ca-cert");
    assert_eq!(
        envelope["next_step"]["template"],
        Value::from(format!("anc web {target} --ca-cert <FILE>"))
    );
    assert!(envelope["next_step"].get("command").is_none());
}

#[test]
fn a_ca_cert_root_lets_the_handshake_through_and_every_check_runs() {
    let server = fixture_site();
    let output = anc()
        .env_remove("SSL_CERT_FILE")
        .args([
            "web",
            &server.url("/"),
            "--ca-cert",
            test_ca_path(),
            "--output",
            "json",
        ])
        .output()
        .unwrap();
    let code = output.status.code();
    assert!(
        matches!(code, Some(0..=2)),
        "expected a verdict: {output:?}"
    );
    let scorecard: Value = serde_json::from_str(&stdout_of(&output)).unwrap();
    assert_eq!(
        scorecard["results"].as_array().unwrap().len(),
        registry_rows()
    );
    assert!(scorecard["summary"]["pass"].as_u64().unwrap() > 0);
    assert!(
        server.hits().iter().any(|hit| hit.target == "/"),
        "the root fetch reached the server"
    );
}

#[test]
fn a_local_target_trusts_a_ca_in_the_os_trust_store() {
    // rustls-native-certs reads SSL_CERT_FILE in place of the platform
    // store, which stands in for a CA installed on the machine.
    let server = fixture_site();
    let output = anc()
        .env("SSL_CERT_FILE", test_ca_path())
        .env_remove("SSL_CERT_DIR")
        .args(["web", &server.url("/"), "--output", "json"])
        .output()
        .unwrap();
    assert!(
        matches!(output.status.code(), Some(0..=2)),
        "expected a verdict: {output:?}"
    );
}

#[test]
fn a_bad_ca_cert_file_is_a_usage_error_before_any_request() {
    let server = fixture_site();
    let target = server.url("/");
    let not_pem = empty_home_config();
    let missing = "/nonexistent/anc-test-ca.pem";
    for (path, reason) in [
        (not_pem.to_str().unwrap(), "holds no PEM certificate"),
        (missing, "cannot read the file"),
    ] {
        let output = anc()
            .args(["web", &target, "--ca-cert", path])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        let err = stderr_of(&output);
        assert!(
            err.contains(&format!("cannot trust --ca-cert {path}")),
            "{err}"
        );
        assert!(err.contains(reason), "{err}");
        assert!(err.trim_end().ends_with("exit 2: usage error"), "{err}");

        let output = anc()
            .args(["web", &target, "--ca-cert", path, "--output", "json"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        let envelope = envelope(&output);
        assert_eq!(envelope["kind"], "usage");
        assert_eq!(envelope["error"], "invalid-ca-cert");
        assert_eq!(envelope["exit_code"], 2);
        assert_eq!(envelope["argument"], "--ca-cert <FILE>");
        assert_eq!(envelope["value"], path);
        assert_eq!(envelope["next_step"]["action"], "show-help");
        assert_eq!(envelope["next_step"]["command"], "anc web --help");
    }
    assert!(
        server.hits().is_empty(),
        "a bad --ca-cert must probe nothing"
    );
}
