//! One audit's names resolve once: the address the locality rules classified
//! is the address the connection uses, whatever a later lookup would answer.

#![cfg(feature = "web")]

mod common;

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use agentnative::web_audit::audit_resolver::AuditResolver;
use agentnative::web_audit::engine::FetchHandle;
use agentnative::web_audit::fetch::redirect::{validate_hop, validate_target};
use agentnative::web_audit::fetch::trust::{TrustClass, TrustConfig};
use agentnative::web_audit::fetch::{FetchInit, FetchOptions, Fetcher};
use agentnative::web_audit::locality::{Locality, Resolver, classify_host};
use agentnative::web_audit::mock::{MockResponse, MockRule, MockTransport};
use agentnative::web_audit::transport::{Request, Transport, TransportError, UreqTransport};
use common::RawResponse;

const UA: &str = "anc-web-audit-test/0 (+https://anc.dev/audit)";
const PUBLIC: &str = "203.0.113.7";
const METADATA: &str = "169.254.169.254";
const LOOPBACK: &str = "127.0.0.1";

/// A name's answers in lookup order; the last repeats. An unknown name fails.
#[derive(Default)]
struct Script {
    answers: HashMap<&'static str, Vec<Vec<IpAddr>>>,
    lookups: Mutex<HashMap<String, usize>>,
}

impl Script {
    fn answering(mut self, name: &'static str, answers: &[&[&str]]) -> Self {
        let parsed = answers
            .iter()
            .map(|addrs| addrs.iter().map(|a| a.parse().unwrap()).collect())
            .collect();
        self.answers.insert(name, parsed);
        self
    }

    fn lookups(&self, name: &str) -> usize {
        self.lookups.lock().unwrap().get(name).copied().unwrap_or(0)
    }
}

impl Resolver for Script {
    fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, String> {
        let seen = {
            let mut lookups = self.lookups.lock().unwrap();
            let count = lookups.entry(host.to_string()).or_insert(0);
            *count += 1;
            *count - 1
        };
        match self.answers.get(host) {
            Some(answers) => Ok(answers[seen.min(answers.len() - 1)].clone()),
            None => Err(format!("no such host: {host}")),
        }
    }
}

/// The script, and the one resolver a transport and its classifier share.
fn audit(script: Script) -> (Arc<Script>, Arc<AuditResolver>) {
    let script = Arc::new(script);
    let resolver = Arc::new(AuditResolver::new(script.clone()));
    (script, resolver)
}

fn transport(resolver: &Arc<AuditResolver>) -> UreqTransport {
    UreqTransport::with_resolver(UA, None, TrustConfig::default(), Arc::clone(resolver))
}

fn plain_get(url: String, timeout: Duration) -> Request {
    Request {
        method: "GET".to_string(),
        url,
        headers: Vec::new(),
        body: None,
        timeout,
        via_proxy: false,
        trust: TrustClass::Public,
    }
}

#[test]
fn a_second_lookup_cannot_change_a_names_class() {
    let (script, resolver) =
        audit(Script::default().answering("rebind.example", &[&[PUBLIC], &[METADATA]]));
    for _ in 0..3 {
        assert_eq!(
            classify_host("rebind.example", &*resolver).unwrap(),
            Locality::Public
        );
    }
    assert_eq!(script.lookups("rebind.example"), 1);
}

#[test]
fn a_connection_goes_to_the_address_its_name_was_classified_by() {
    let origin = common::spawn(|_| RawResponse::new(200, &[], b"classified"));
    let (script, resolver) =
        audit(Script::default().answering("pinned.example", &[&[LOOPBACK], &[PUBLIC]]));
    assert_eq!(
        classify_host("pinned.example", &*resolver).unwrap(),
        Locality::Local
    );
    let url = format!("http://pinned.example:{}/", origin.addr.port());
    let response = transport(&resolver)
        .send(&plain_get(url, Duration::from_secs(5)))
        .expect("the classified address answers");
    assert_eq!(response.status, 200);
    assert_eq!(origin.hits().len(), 1);
    assert_eq!(script.lookups("pinned.example"), 1);
}

#[test]
fn a_classification_and_a_connection_share_one_lookup_however_the_name_is_spelled() {
    let origin = common::spawn(|_| RawResponse::new(200, &[], b"classified"));
    let (script, resolver) =
        audit(Script::default().answering("pinned.example", &[&[LOOPBACK], &[PUBLIC]]));
    assert_eq!(
        classify_host("Pinned.EXAMPLE.", &*resolver).unwrap(),
        Locality::Local
    );
    let url = format!("http://pinned.example.:{}/", origin.addr.port());
    let response = transport(&resolver)
        .send(&plain_get(url, Duration::from_secs(5)))
        .expect("the classified address answers");
    assert_eq!(response.status, 200);
    assert_eq!(script.lookups("pinned.example"), 1);
}

#[test]
fn a_name_classified_public_never_connects_to_a_private_second_answer() {
    let origin = common::spawn(|_| RawResponse::new(200, &[], b"private"));
    let (script, resolver) =
        audit(Script::default().answering("rebind.example", &[&[PUBLIC], &[LOOPBACK]]));
    assert_eq!(
        classify_host("rebind.example", &*resolver).unwrap(),
        Locality::Public
    );
    let url = format!("http://rebind.example:{}/", origin.addr.port());
    let result = transport(&resolver).send(&plain_get(url, Duration::from_millis(300)));
    assert!(
        result.is_err(),
        "the classified public address is unroutable here"
    );
    assert!(
        origin.hits().is_empty(),
        "the private second answer must never be contacted"
    );
    assert_eq!(script.lookups("rebind.example"), 1);
}

#[test]
fn a_redirect_within_a_name_keeps_the_class_that_name_was_classified_as() {
    let (script, resolver) =
        audit(Script::default().answering("rebind.example", &[&[PUBLIC], &[METADATA]]));
    let mock = MockTransport::new(
        vec![
            MockRule::new("GET", "https://rebind.example/").response(MockResponse::ok(
                302,
                &[("location", "/next")],
                b"",
            )),
            MockRule::new("GET", "https://rebind.example/next").response(MockResponse::ok(
                200,
                &[],
                b"landed",
            )),
        ],
        MockResponse::error("connection refused"),
    );
    let response = Fetcher::new(&mock, &*resolver, UA).fetch(
        "https://rebind.example/",
        &FetchInit::default(),
        &FetchOptions::default(),
    );
    assert_eq!(response.error, None);
    assert_eq!(response.status, Some(200));
    assert_eq!(response.body, "landed");
    assert_eq!(script.lookups("rebind.example"), 1);
}

#[test]
fn a_first_answer_in_the_metadata_range_is_still_refused() {
    let (_, resolver) =
        audit(Script::default().answering("metadata.example", &[&[METADATA], &[PUBLIC]]));
    for _ in 0..2 {
        let refusal = validate_target("https://metadata.example/", &*resolver).unwrap_err();
        assert_eq!(
            refusal,
            "blocked: metadata.example is a cloud metadata endpoint"
        );
    }
}

#[test]
fn a_redirect_to_a_name_of_another_class_is_still_refused() {
    let (_, resolver) = audit(
        Script::default()
            .answering("public.example", &[&[PUBLIC]])
            .answering("private.example", &[&[LOOPBACK], &[PUBLIC]]),
    );
    let target = validate_target("https://public.example/", &*resolver).unwrap();
    assert_eq!(target.class, Locality::Public);
    for _ in 0..2 {
        let refusal = validate_hop(
            "https://private.example/",
            &target.url,
            target.class,
            1,
            &*resolver,
        )
        .unwrap_err();
        assert_eq!(
            refusal,
            "blocked: redirect leaves the public target for a local host private.example (redirect hop 1)"
        );
    }
}

#[test]
fn a_handler_refuses_a_nested_host_while_the_targets_class_is_unknown() {
    let transport: Arc<dyn Transport + Send + Sync> = Arc::new(MockTransport::new(
        Vec::new(),
        MockResponse::error("connection refused"),
    ));
    let mut ctx = common::handler_context::context(transport);
    assert_eq!(ctx.validate_nested("https://api.example.net/"), Ok(()));
    ctx.target_locality = None;
    let refusal = ctx.validate_nested("https://api.example.net/").unwrap_err();
    assert!(refusal.starts_with("blocked: "), "{refusal}");
    assert!(refusal.contains("unknown"), "{refusal}");
}

#[test]
fn a_handle_over_a_transport_classifies_through_that_transports_resolver() {
    let origin = common::spawn(|_| RawResponse::new(200, &[], b"classified"));
    let (script, resolver) =
        audit(Script::default().answering("pinned.example", &[&[LOOPBACK], &[PUBLIC]]));
    let handle = FetchHandle::over_ureq(transport(&resolver), UA);
    assert_eq!(handle.classify("pinned.example").unwrap(), Locality::Local);
    // Plain HTTP on the fixture, so the handshake fails after the connection; the
    // connection is what asks the resolver.
    let url = format!("https://pinned.example:{}/", origin.addr.port());
    let response = handle.fetch(&url, &FetchInit::default(), &FetchOptions::default());
    assert!(response.status.is_none());
    assert!(
        response
            .error
            .as_deref()
            .is_some_and(|e| e.contains("TlsError") || e.contains("NetworkError")),
        "{response:?}"
    );
    assert_eq!(script.lookups("pinned.example"), 1);
}

#[test]
fn a_proxied_request_resolves_the_proxy_once_and_never_its_target() {
    let proxy = common::spawn_connect_proxy(|_| RawResponse::new(200, &[], b"via proxy"));
    let (script, resolver) = audit(Script::default().answering("proxy.example", &[&[LOOPBACK]]));
    let named = ureq::Proxy::new(&format!("http://proxy.example:{}", proxy.addr.port()))
        .expect("proxy url");
    let transport = UreqTransport::with_resolver(
        UA,
        Some(named),
        TrustConfig::default(),
        Arc::clone(&resolver),
    );
    for _ in 0..2 {
        let request = Request {
            via_proxy: true,
            ..plain_get("http://target.example/".to_string(), Duration::from_secs(5))
        };
        assert_eq!(transport.send(&request).expect("via the proxy").status, 200);
    }
    assert_eq!(script.lookups("proxy.example"), 1);
    assert_eq!(script.lookups("target.example"), 0);
}

/// A lookup that holds until the test lets it go, or until a request that
/// ignored its deadline would have failed the test's time check.
struct Held(Mutex<mpsc::Receiver<()>>);

impl Resolver for Held {
    fn resolve(&self, _: &str) -> Result<Vec<IpAddr>, String> {
        let _ = self.0.lock().unwrap().recv_timeout(Duration::from_secs(5));
        Err("released".to_string())
    }
}

#[test]
fn a_lookup_that_outlasts_the_request_ends_with_the_requests_deadline() {
    let (release, held) = mpsc::channel();
    let resolver = Arc::new(AuditResolver::new(Arc::new(Held(Mutex::new(held)))));
    let started = Instant::now();
    let error = transport(&resolver)
        .send(&plain_get(
            "http://slow.example/".to_string(),
            Duration::from_millis(200),
        ))
        .expect_err("no answer inside the deadline");
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "{:?}",
        started.elapsed()
    );
    assert!(
        matches!(
            error,
            TransportError::Timeout | TransportError::ConnectTimeout
        ),
        "{error:?}"
    );
    let _ = release.send(());
}

#[test]
fn a_lookup_failure_reads_as_the_resolvers_own_account_at_connect() {
    let (_, resolver) = audit(Script::default());
    let error = transport(&resolver)
        .send(&plain_get(
            "http://missing.example/".to_string(),
            Duration::from_secs(5),
        ))
        .expect_err("the name resolves to nothing");
    assert_eq!(
        error.to_string(),
        "NetworkError: no such host: missing.example"
    );
}
