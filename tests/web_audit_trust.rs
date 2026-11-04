//! Which certificate chains `anc web` trusts: a private CA is rejected with
//! its own evidence until `--ca-cert` or the OS trust store vouches for it,
//! and a `--ca-cert` file that holds no usable root is a usage error raised
//! before any request.

#![cfg(feature = "web")]

mod common;

use std::io::Write;

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

/// A certificate that fails a check no root waives, served by a server that
/// `--ca-cert` trusts: the file is the certificate itself, so the chain is never
/// the question.
fn rejected_certificate_run(cert: &[u8], fixture: &str, output: &[&str]) -> std::process::Output {
    let key = include_bytes!("fixtures/tls/rejected.key.pem");
    let addr = common::spawn_tls(cert, key);
    let root = format!(
        "{}/tests/fixtures/tls/{fixture}",
        env!("CARGO_MANIFEST_DIR")
    );
    let target = format!("https://localhost:{}/", addr.port());
    anc()
        .args(["web", &target, "--ca-cert", &root])
        .args(output)
        .output()
        .unwrap()
}

#[test]
fn a_certificate_no_root_can_save_ends_the_run_naming_the_certificate() {
    let cases: [(&str, &[u8], &str); 3] = [
        (
            "ca-flagged.cert.pem",
            include_bytes!("fixtures/tls/ca-flagged.cert.pem"),
            "CaUsedAsEndEntity",
        ),
        (
            "wrong-name.cert.pem",
            include_bytes!("fixtures/tls/wrong-name.cert.pem"),
            "not valid for name",
        ),
        (
            "expired.cert.pem",
            include_bytes!("fixtures/tls/expired.cert.pem"),
            "expired",
        ),
    ];
    for (fixture, cert, names_it) in cases {
        let output = rejected_certificate_run(cert, fixture, &["--output", "json"]);
        assert_eq!(output.status.code(), Some(3), "{fixture}: {output:?}");
        assert!(
            stdout_of(&output).is_empty(),
            "{fixture}: stdout must stay empty"
        );
        let envelope = envelope(&output);
        assert_eq!(envelope["kind"], "runtime", "{fixture}");
        assert_eq!(envelope["error"], "invalid-certificate", "{fixture}");
        assert_eq!(envelope["exit_code"], 3, "{fixture}");
        assert_eq!(envelope["argument"], "<TARGET>", "{fixture}");
        assert_eq!(envelope["next_step"]["action"], "retry", "{fixture}");
        let message = envelope["message"].as_str().unwrap();
        assert!(message.contains(names_it), "{fixture}: {message}");
        let command = envelope["next_step"]["command"].as_str().unwrap();
        assert!(command.contains(" --ca-cert "), "{fixture}: {command}");
        assert!(command.ends_with(" --verbose"), "{fixture}: {command}");

        let text = rejected_certificate_run(cert, fixture, &[]);
        assert_eq!(text.status.code(), Some(3), "{fixture}: {text:?}");
        let err = stderr_of(&text);
        assert!(
            err.contains("or a redirect from it presented a certificate anc web rejects"),
            "{fixture}: {err}"
        );
        assert!(
            err.contains("TlsError: certificate rejected ("),
            "{fixture}: {err}"
        );
        assert!(err.contains(names_it), "{fixture}: {err}");
        assert!(err.contains("`--ca-cert` adds a root"), "{fixture}: {err}");
    }
}

#[test]
fn a_root_with_the_right_name_and_the_wrong_key_is_a_root_problem() {
    let addr = common::spawn_tls(common::CA_SIGNED_CERT_PEM, common::CA_SIGNED_KEY_PEM);
    let target = format!("https://localhost:{}/", addr.port());
    let stale = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/tls/stale-root.cert.pem"
    );
    let output = anc()
        .env_remove("SSL_CERT_FILE")
        .args(["web", &target, "--ca-cert", stale, "--output", "json"])
        .output()
        .unwrap();
    let envelope = envelope(&output);
    assert_eq!(envelope["error"], "untrusted-certificate", "{envelope}");
    assert_eq!(envelope["next_step"]["action"], "add-ca-cert");
}

#[test]
fn the_template_to_add_a_root_keeps_the_runs_roots() {
    let server = fixture_site();
    let target = server.url("/");
    let other = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/tls/localhost.cert.pem"
    );
    let output = anc()
        .env_remove("SSL_CERT_FILE")
        .args(["web", &target, "--ca-cert", other, "--output", "json"])
        .output()
        .unwrap();
    let envelope = envelope(&output);
    assert_eq!(envelope["error"], "untrusted-certificate", "{envelope}");
    assert_eq!(
        envelope["next_step"]["template"],
        Value::from(format!(
            "anc web {target} --ca-cert {other} --ca-cert <FILE>"
        ))
    );
}

#[test]
fn a_failed_handshake_says_so_rather_than_calling_the_server_silent() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n");
        }
    });
    let target = format!("https://127.0.0.1:{port}/");
    let output = anc()
        .args(["web", &target, "--output", "json"])
        .output()
        .unwrap();
    let envelope = envelope(&output);
    assert_eq!(envelope["error"], "target-unreachable", "{envelope}");
    let message = envelope["message"].as_str().unwrap();
    assert!(
        message.contains("did not complete a TLS handshake"),
        "{message}"
    );
    assert!(message.contains("TlsError: "), "{message}");
}

#[test]
fn the_command_to_try_again_keeps_the_runs_roots() {
    let ca = test_ca_path();
    let output = anc()
        .args(["web", "127.0.0.1:9", "--ca-cert", ca, "--output", "json"])
        .output()
        .unwrap();
    let envelope = envelope(&output);
    assert_eq!(envelope["error"], "target-unreachable");
    let command = envelope["next_step"]["command"].as_str().unwrap();
    assert!(command.contains(&format!(" --ca-cert {ca}")), "{command}");
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
