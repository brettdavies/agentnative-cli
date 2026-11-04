//! The contract check: what a run compares with anc.dev, and who it contacts.

#![cfg(feature = "web")]

use std::net::IpAddr;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use agentnative::web_audit::fetch::trust::TrustClass;
use agentnative::web_audit::locality::Resolver;
use agentnative::web_audit::mock::{MockResponse, MockRule, MockTransport};
use agentnative::web_audit::registry::CONTRACT_FINGERPRINT;
use agentnative::web_audit::signal::{
    ContractCheck, NotChecked, PendingCheck, VERSION_SIGNAL_URL, check_contract,
};
use agentnative::web_audit::transport::{Request, Response, Transport, TransportError};

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
fn a_public_target_compares_the_trimmed_vendored_fingerprint() {
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

#[test]
fn a_stalled_signal_adds_nothing_to_the_run() {
    let (release, held) = mpsc::channel();
    let started = Instant::now();
    let pending = PendingCheck::start(
        "https://example.com/",
        Arc::new(Stalled(Mutex::new(held))),
        Arc::new(Answers("93.184.216.34")),
    );
    assert_eq!(
        pending.result(),
        ContractCheck::NotChecked(NotChecked::NoAnswerYet)
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    drop(release);
}

#[test]
fn an_answer_that_arrived_in_time_is_the_result() {
    let pending = PendingCheck::start(
        "https://example.com/",
        Arc::new(signal(body(CONTRACT_FINGERPRINT))),
        Arc::new(Answers("93.184.216.34")),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let result = pending.result();
        if result != ContractCheck::NotChecked(NotChecked::NoAnswerYet) {
            assert_eq!(result, ContractCheck::Same);
            return;
        }
        assert!(Instant::now() < deadline, "the check never answered");
        std::thread::sleep(Duration::from_millis(5));
    }
}
