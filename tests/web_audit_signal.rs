//! The contract check: what a run compares with anc.dev, and who it contacts.

#![cfg(feature = "web")]

mod common;

use std::io::ErrorKind;
use std::net::{IpAddr, TcpListener};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use agentnative::web_audit::fetch::trust::TrustClass;
use agentnative::web_audit::locality::Resolver;
use agentnative::web_audit::mock::{MockResponse, MockRule, MockTransport};
use agentnative::web_audit::registry::{AUDIT_USER_AGENT, CONTRACT_FINGERPRINT};
use agentnative::web_audit::signal::{
    ContractCheck, NotChecked, PendingCheck, VERSION_SIGNAL_URL, check_contract,
};
use agentnative::web_audit::transport::{Request, Response, Transport, TransportError};
use common::web_cli::{anc, fixture_site, host_of, stderr_of, stdout_of};

struct Answers(&'static str);

impl Resolver for Answers {
    fn resolve(&self, _host: &str) -> Result<Vec<IpAddr>, String> {
        Ok(vec![self.0.parse().expect("an address")])
    }
}

struct NoAnswer;

impl Resolver for NoAnswer {
    fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, String> {
        Err(format!("no such host: {host}"))
    }
}

fn signal(response: MockResponse) -> MockTransport {
    MockTransport::new(
        vec![MockRule::new("GET", VERSION_SIGNAL_URL).response(response)],
        MockResponse::error("not the signal"),
    )
}

fn body(fingerprint: &str) -> MockResponse {
    let json = format!(
        r#"{{"spec_version":"0.7.0","registry_fingerprint":"c06f1efd281b","web_schema_version":"0.5","contract_fingerprint":"{fingerprint}"}}"#
    );
    MockResponse::ok(
        200,
        &[("content-type", "application/json")],
        json.as_bytes(),
    )
}

const PUBLIC: Answers = Answers("93.184.216.34");

#[test]
fn a_public_target_compares_the_vendored_fingerprint() {
    let same = signal(body(CONTRACT_FINGERPRINT));
    assert_eq!(
        check_contract("example.com", &same, &PUBLIC),
        ContractCheck::Same
    );
    let other = signal(body("000000000000"));
    assert_eq!(
        check_contract("example.com", &other, &PUBLIC),
        ContractCheck::Differs
    );
}

#[test]
fn the_signal_goes_once_through_the_proxy_on_the_public_trust_set_with_no_credential() {
    let transport = signal(body(CONTRACT_FINGERPRINT));
    check_contract("example.com", &transport, &PUBLIC);
    let requests = transport.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    let request = &requests[0];
    assert_eq!(request.method, "GET");
    assert_eq!(request.url, VERSION_SIGNAL_URL);
    assert!(request.via_proxy);
    assert_eq!(request.trust, TrustClass::Public);
    assert!(request.body.is_none());
    assert!(
        !request
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("authorization")),
        "{:?}",
        request.headers
    );
}

#[test]
fn the_signal_sends_the_audit_user_agent() {
    let transport = signal(body(CONTRACT_FINGERPRINT));
    check_contract("example.com", &transport, &PUBLIC);
    let requests = transport.requests();
    assert!(
        requests[0]
            .headers
            .iter()
            .any(|(name, value)| name.eq_ignore_ascii_case("user-agent")
                && value == AUDIT_USER_AGENT),
        "{:?}",
        requests[0].headers
    );
}

#[test]
fn a_local_private_or_metadata_target_contacts_nobody() {
    for (host, resolver) in [
        ("localhost", Answers("127.0.0.1")),
        ("127.0.0.1", Answers("127.0.0.1")),
        ("[::1]", Answers("::1")),
        ("intranet.example", Answers("10.1.2.3")),
        ("metadata.google.internal", Answers("169.254.169.254")),
    ] {
        let transport = signal(body(CONTRACT_FINGERPRINT));
        assert_eq!(
            check_contract(host, &transport, &resolver),
            ContractCheck::NotChecked(NotChecked::NotPublic),
            "{host}"
        );
        assert!(transport.requests().is_empty(), "{host} sent a request");
    }
}

#[test]
fn a_target_of_unknown_locality_counts_as_not_public() {
    let transport = signal(body(CONTRACT_FINGERPRINT));
    assert_eq!(
        check_contract("example.com", &transport, &NoAnswer),
        ContractCheck::NotChecked(NotChecked::ClassUnknown)
    );
    assert!(transport.requests().is_empty());
}

#[test]
fn anything_but_a_readable_200_is_not_checked() {
    for (response, want) in [
        (MockResponse::ok(404, &[], b"nope"), NotChecked::Status(404)),
        (
            MockResponse::ok(503, &[("retry-after", "30")], b"down"),
            NotChecked::Status(503),
        ),
        (
            MockResponse::ok(301, &[("location", "https://elsewhere.example/")], b""),
            NotChecked::Status(301),
        ),
        (
            MockResponse::error("connection refused"),
            NotChecked::Unreachable,
        ),
        (
            MockResponse::ok(200, &[], b"<html>"),
            NotChecked::Unreadable,
        ),
        (
            MockResponse::ok(200, &[], br#"{"registry_fingerprint":"c06f1efd281b"}"#),
            NotChecked::NoFingerprint,
        ),
        (
            MockResponse::ok(200, &[], br#"{"contract_fingerprint":17}"#),
            NotChecked::NoFingerprint,
        ),
        (
            MockResponse::ok(200, &[], br#"{"contract_fingerprint":""}"#),
            NotChecked::NoFingerprint,
        ),
    ] {
        let transport = signal(response);
        assert_eq!(
            check_contract("example.com", &transport, &PUBLIC),
            ContractCheck::NotChecked(want),
            "{want:?}"
        );
        assert_eq!(transport.requests().len(), 1, "a redirect is not followed");
    }
}

#[test]
fn an_answer_past_the_read_bound_is_not_checked_rather_than_read_whole() {
    let mut oversized = br#"{"contract_fingerprint":""#.to_vec();
    oversized.extend(std::iter::repeat_n(b'a', 64 * 1024));
    oversized.extend_from_slice(br#""}"#);
    let transport = signal(MockResponse::ok(200, &[], &oversized));
    assert_eq!(
        check_contract("example.com", &transport, &PUBLIC),
        ContractCheck::NotChecked(NotChecked::Unreadable)
    );
}

/// A transport that holds every request until the test releases it.
struct Stalled(Mutex<Receiver<()>>);

impl Transport for Stalled {
    fn send(&self, _request: &Request) -> Result<Response, TransportError> {
        let _ = self.0.lock().expect("stall lock").recv();
        Err(TransportError::Network("released".to_string()))
    }
}

/// A pending check on a transport that never answers, and the handle whose
/// drop releases it.
fn stalled_check(target: &str, resolver: Answers) -> (PendingCheck, mpsc::Sender<()>) {
    let (release, held) = mpsc::channel();
    let pending = PendingCheck::start(
        target,
        Arc::new(Stalled(Mutex::new(held))),
        Arc::new(resolver),
    );
    (pending, release)
}

#[test]
fn a_stalled_signal_holds_a_short_run_no_longer_than_its_deadline() {
    let started = Instant::now();
    let (pending, release) = stalled_check("https://example.com/", PUBLIC);
    assert_eq!(
        pending.result(),
        ContractCheck::NotChecked(NotChecked::NoAnswer)
    );
    let elapsed = started.elapsed();
    assert!(elapsed >= Duration::from_secs(1), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(4), "{elapsed:?}");
    drop(release);
}

#[test]
fn a_run_past_the_deadline_pays_nothing_for_a_stalled_signal() {
    let (pending, release) = stalled_check("https://example.com/", PUBLIC);
    thread::sleep(Duration::from_millis(2100));
    let asked = Instant::now();
    assert_eq!(
        pending.result(),
        ContractCheck::NotChecked(NotChecked::NoAnswer)
    );
    assert!(asked.elapsed() < Duration::from_millis(500));
    drop(release);
}

#[test]
fn a_local_target_answers_at_once_without_waiting_out_the_deadline() {
    let started = Instant::now();
    let (pending, release) = stalled_check("https://localhost:8787/", Answers("127.0.0.1"));
    assert_eq!(
        pending.result(),
        ContractCheck::NotChecked(NotChecked::NotPublic)
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    drop(release);
}

#[test]
fn a_short_run_still_gets_an_answer_that_arrives_in_time() {
    let pending = PendingCheck::start(
        "https://example.com/",
        Arc::new(signal(body(CONTRACT_FINGERPRINT))),
        Arc::new(PUBLIC),
    );
    assert_eq!(pending.result(), ContractCheck::Same);
}

#[test]
fn a_plaintext_target_sends_nothing() {
    let transport = Arc::new(signal(body(CONTRACT_FINGERPRINT)));
    let pending = PendingCheck::start("http://example.com/", transport.clone(), Arc::new(PUBLIC));
    pending.result();
    assert!(transport.requests().is_empty());
}

const LOCAL_RESULT: &str = "anc.dev contract: not checked (not a public target)";

#[test]
fn json_mode_keeps_the_result_off_stdout_and_states_it_on_stderr() {
    let server = fixture_site();
    let host = host_of(&server);
    let assert = anc()
        .args(["web", &host, "--output", "json", "--verbose"])
        .assert();
    let output = assert.get_output();

    let stderr = stderr_of(output);
    assert!(
        stderr
            .lines()
            .any(|l| l == format!("verbose: {LOCAL_RESULT}")),
        "{stderr}"
    );
    assert!(!stderr.contains("contract-differs"), "{stderr}");

    let stdout = stdout_of(output);
    assert!(!stdout.contains("anc.dev contract"), "{stdout}");
    let scorecard: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout is the scorecard alone");
    assert!(scorecard.get("results").is_some(), "{stdout}");
}

#[test]
fn without_verbose_a_json_run_says_nothing_about_the_check() {
    let server = fixture_site();
    let assert = anc()
        .args(["web", &host_of(&server), "--output", "json"])
        .assert();
    let stderr = stderr_of(assert.get_output());
    assert!(!stderr.contains("anc.dev contract"), "{stderr}");
}

#[test]
fn a_check_run_states_the_result_on_stderr_and_leaves_its_row_alone() {
    let server = fixture_site();
    let assert = anc()
        .args(["web", &host_of(&server), "--check", "llms-txt", "--verbose"])
        .assert();
    let output = assert.get_output();
    let stderr = stderr_of(output);
    assert!(
        stderr
            .lines()
            .any(|l| l == format!("verbose: {LOCAL_RESULT}")),
        "{stderr}"
    );
    let stdout = stdout_of(output);
    assert!(!stdout.contains("anc.dev contract"), "{stdout}");
}

#[test]
fn a_text_report_states_the_result_under_its_header() {
    let server = fixture_site();
    let assert = anc().args(["web", &host_of(&server), "--verbose"]).assert();
    let stdout = stdout_of(assert.get_output());
    assert!(
        stdout.lines().any(|l| l == format!("  {LOCAL_RESULT}")),
        "{stdout}"
    );
}

/// A listener standing in for a proxy. Nothing accepts on it, so a connection a
/// run opened waits in its backlog for [`assert_no_connection`].
fn idle_proxy() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let url = format!("http://{}", listener.local_addr().expect("an address"));
    (listener, url)
}

fn assert_no_connection(listener: &TcpListener, what: &str) {
    listener.set_nonblocking(true).expect("non-blocking");
    match listener.accept() {
        Err(error) if error.kind() == ErrorKind::WouldBlock => {}
        other => panic!("{what} connected to the proxy: {other:?}"),
    }
}

fn anc_through(proxy_url: &str) -> assert_cmd::Command {
    let mut cmd = anc();
    for name in ["ALL_PROXY", "all_proxy", "HTTPS_PROXY", "https_proxy"] {
        cmd.env(name, proxy_url);
    }
    cmd.env_remove("NO_PROXY").env_remove("no_proxy");
    cmd
}

#[test]
fn the_help_and_version_forms_contact_nobody() {
    for args in [
        vec!["web", "--help"],
        vec!["web", "-h"],
        vec!["--version"],
        vec!["--help"],
    ] {
        let (proxy, url) = idle_proxy();
        anc_through(&url).args(&args).assert().success();
        assert_no_connection(&proxy, &args.join(" "));
    }
}

#[test]
fn a_local_target_run_never_reaches_for_anc_dev() {
    let server = fixture_site();
    let (proxy, url) = idle_proxy();
    let assert = anc_through(&url)
        .args(["web", &host_of(&server), "--verbose"])
        .assert();
    let stdout = stdout_of(assert.get_output());
    assert!(stdout.contains(LOCAL_RESULT), "{stdout}");
    assert_no_connection(&proxy, "a local target's run");
}

#[test]
fn the_help_discloses_the_fetch() {
    let assert = anc().args(["web", "--help"]).assert().success();
    let help = stdout_of(assert.get_output());
    assert!(help.contains(VERSION_SIGNAL_URL), "{help}");
}
