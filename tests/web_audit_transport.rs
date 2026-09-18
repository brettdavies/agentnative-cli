//! Transport and guarded-fetch behavior for the local web audit: the
//! single-hop transport contract, the redirect loop with its two refusal
//! exits, the three body-cap regimes, per-request proxy routing, and the
//! TLS evidence classes.

mod common;

use std::io::Write;
use std::net::IpAddr;
use std::time::Duration;

use agentnative::web_audit::fetch::body::{
    AUDIT_PROBE_MAX_BODY_BYTES, AUDIT_ROOT_MAX_BODY_BYTES, STATUS_ONLY_BODY_BYTES,
};
use agentnative::web_audit::fetch::{FetchInit, FetchOptions, Fetcher, ProbeResponse};
use agentnative::web_audit::locality::Resolver;
use agentnative::web_audit::mock::{MockResponse, MockRule, MockTransport};
use agentnative::web_audit::transport::{Request, Transport, TransportError, UreqTransport};
use common::RawResponse;

const UA: &str = "anc-web-audit-test/0 (+https://anc.dev/audit)";

/// Names resolve to a public address; literals never reach the resolver.
struct PublicResolver;

impl Resolver for PublicResolver {
    fn resolve(&self, _host: &str) -> Result<Vec<IpAddr>, String> {
        Ok(vec!["93.184.216.34".parse().unwrap()])
    }
}

fn fetcher<'a>(transport: &'a dyn Transport) -> Fetcher<'a> {
    Fetcher::new(transport, &PublicResolver, UA)
}

fn ok(status: u16, headers: &[(&str, &str)], body: &str) -> MockResponse {
    MockResponse::ok(status, headers, body.as_bytes())
}

fn get(url: &str) -> MockRule {
    MockRule::new("GET", url)
}

fn unmatched() -> MockResponse {
    MockResponse::error("connection refused")
}

fn fetch(transport: &dyn Transport, url: &str, opts: &FetchOptions) -> ProbeResponse {
    fetcher(transport).fetch(url, &FetchInit::default(), opts)
}

fn request(url: &str, timeout_ms: u64, via_proxy: bool) -> Request {
    Request {
        method: "GET".to_string(),
        url: url.to_string(),
        headers: Vec::new(),
        body: None,
        timeout: Duration::from_millis(timeout_ms),
        via_proxy,
    }
}

// ---------------------------------------------------------------------------
// Transport trait through the mock
// ---------------------------------------------------------------------------

#[test]
fn mock_get_returns_status_headers_and_body_through_the_trait() {
    let transport = MockTransport::new(
        vec![get("https://example.com/").response(ok(
            200,
            &[("Content-Type", "text/plain"), ("X-Probe", "yes")],
            "hello",
        ))],
        unmatched(),
    );
    let resp = fetch(&transport, "https://example.com/", &FetchOptions::default());
    assert_eq!(resp.status, Some(200));
    assert_eq!(resp.body, "hello");
    assert_eq!(resp.headers.get("content-type"), Some("text/plain"));
    assert_eq!(resp.headers.get("x-probe"), Some("yes"));
    assert_eq!(resp.error, None);
    assert!(!resp.truncated);
    let seen = transport.requests();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].url, "https://example.com/");
}

#[test]
fn mock_matches_on_header_subset_and_json_rpc_method() {
    let transport = MockTransport::new(
        vec![
            MockRule::new("POST", "https://example.com/mcp")
                .body_json_method("initialize")
                .response(ok(200, &[], "init")),
            MockRule::new("POST", "https://example.com/mcp")
                .header("mcp-session-id", "abc")
                .body_contains("tools/list")
                .response(ok(200, &[], "tools")),
            MockRule::new("POST", "https://example.com/mcp").response(ok(404, &[], "fallback")),
        ],
        unmatched(),
    );
    let init = FetchInit {
        method: "POST".to_string(),
        headers: vec![],
        body: Some(br#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#.to_vec()),
    };
    let resp =
        fetcher(&transport).fetch("https://example.com/mcp", &init, &FetchOptions::default());
    assert_eq!(resp.body, "init");
    let tools = FetchInit {
        method: "POST".to_string(),
        headers: vec![("Mcp-Session-Id".to_string(), "abc".to_string())],
        body: Some(br#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#.to_vec()),
    };
    let resp =
        fetcher(&transport).fetch("https://example.com/mcp", &tools, &FetchOptions::default());
    assert_eq!(resp.body, "tools");
    let other = FetchInit {
        method: "POST".to_string(),
        headers: vec![],
        body: Some(br#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#.to_vec()),
    };
    let resp =
        fetcher(&transport).fetch("https://example.com/mcp", &other, &FetchOptions::default());
    assert_eq!(resp.status, Some(404));
    assert_eq!(resp.body, "fallback");
}

#[test]
fn mock_unmatched_policy_is_a_transport_failure_with_the_recorded_text() {
    let transport = MockTransport::new(
        vec![],
        MockResponse::error("TimeoutError: deadline exceeded"),
    );
    let resp = fetch(&transport, "https://example.com/", &FetchOptions::default());
    assert_eq!(resp.status, None);
    assert_eq!(
        resp.error.as_deref(),
        Some("TimeoutError: deadline exceeded")
    );
    assert_eq!(resp.body, "");
}

// ---------------------------------------------------------------------------
// Redirect loop
// ---------------------------------------------------------------------------

#[test]
fn fetch_follows_a_two_hop_public_redirect_chain() {
    let transport = MockTransport::new(
        vec![
            get("https://example.com/a").response(ok(301, &[("Location", "/b")], "")),
            get("https://example.com/b").response(ok(
                302,
                &[("location", "https://example.com/c")],
                "",
            )),
            get("https://example.com/c").response(ok(200, &[], "final")),
        ],
        unmatched(),
    );
    let resp = fetch(
        &transport,
        "https://example.com/a",
        &FetchOptions::default(),
    );
    assert_eq!(resp.status, Some(200));
    assert_eq!(resp.body, "final");
    let urls: Vec<String> = transport.requests().into_iter().map(|r| r.url).collect();
    assert_eq!(
        urls,
        [
            "https://example.com/a",
            "https://example.com/b",
            "https://example.com/c"
        ]
    );
}

#[test]
fn fetch_returns_the_redirect_itself_when_not_following() {
    let transport = MockTransport::new(
        vec![get("https://example.com/old").response(ok(301, &[("location", "/new")], ""))],
        unmatched(),
    );
    let opts = FetchOptions {
        follow_redirects: false,
        ..FetchOptions::default()
    };
    let resp = fetch(&transport, "https://example.com/old", &opts);
    assert_eq!(resp.status, Some(301));
    assert_eq!(resp.headers.get("location"), Some("/new"));
    assert_eq!(transport.requests().len(), 1);
}

#[test]
fn fetch_stops_at_the_hop_cap() {
    let rules = (0..8)
        .map(|n| {
            get(&format!("https://example.com/{n}")).response(ok(
                302,
                &[("location", &format!("https://example.com/{}", n + 1))],
                "",
            ))
        })
        .collect();
    let transport = MockTransport::new(rules, unmatched());
    let resp = fetch(
        &transport,
        "https://example.com/0",
        &FetchOptions::default(),
    );
    assert_eq!(resp.status, None);
    assert_eq!(
        resp.error.as_deref(),
        Some("redirect limit exceeded (4 hops)")
    );
    assert_eq!(transport.requests().len(), 5);
}

#[test]
fn fetch_returns_a_redirect_without_location_as_is() {
    let transport = MockTransport::new(
        vec![get("https://example.com/").response(ok(302, &[], "odd"))],
        unmatched(),
    );
    let resp = fetch(&transport, "https://example.com/", &FetchOptions::default());
    assert_eq!(resp.status, Some(302));
    assert_eq!(resp.body, "odd");
}

#[test]
fn fetch_refuses_a_redirect_into_ipv4_metadata() {
    let transport = MockTransport::new(
        vec![get("https://example.com/").response(ok(
            302,
            &[("location", "http://169.254.169.254/latest/meta-data/")],
            "",
        ))],
        MockResponse::ok(200, &[], b"secret"),
    );
    let resp = fetch(&transport, "https://example.com/", &FetchOptions::default());
    assert_eq!(resp.status, None);
    let err = resp.error.expect("refused");
    assert!(err.starts_with("blocked: "), "{err}");
    assert!(err.ends_with("(redirect hop 1)"), "{err}");
    assert!(err.contains("169.254.169.254"), "{err}");
    assert_eq!(
        transport.requests().len(),
        1,
        "the metadata hop was never requested"
    );
}

#[test]
fn fetch_refuses_a_redirect_into_ipv6_metadata() {
    let transport = MockTransport::new(
        vec![get("https://example.com/").response(ok(
            302,
            &[("location", "http://[fd00:ec2::254]/latest/meta-data/")],
            "",
        ))],
        MockResponse::ok(200, &[], b"secret"),
    );
    let resp = fetch(&transport, "https://example.com/", &FetchOptions::default());
    assert_eq!(resp.status, None);
    let err = resp.error.expect("refused");
    assert!(
        err.starts_with("blocked: ") && err.ends_with("(redirect hop 1)"),
        "{err}"
    );
    assert_eq!(transport.requests().len(), 1);
}

#[test]
fn fetch_refuses_a_local_target_redirecting_to_a_public_host() {
    let transport = MockTransport::new(
        vec![get("http://localhost:8787/").response(ok(
            302,
            &[("location", "https://example.com/")],
            "",
        ))],
        MockResponse::ok(200, &[], b"leaked"),
    );
    let resp = fetch(
        &transport,
        "http://localhost:8787/",
        &FetchOptions::default(),
    );
    assert_eq!(resp.status, None);
    let err = resp.error.expect("refused");
    assert!(err.starts_with("blocked: "), "{err}");
    assert!(err.contains("local") && err.contains("public"), "{err}");
    assert!(err.ends_with("(redirect hop 1)"), "{err}");
    assert_eq!(transport.requests().len(), 1);
}

#[test]
fn fetch_refuses_a_public_target_redirecting_into_a_private_range() {
    let transport = MockTransport::new(
        vec![get("https://example.com/").response(ok(
            302,
            &[("location", "http://10.0.0.5/admin")],
            "",
        ))],
        MockResponse::ok(200, &[], b"internal"),
    );
    let resp = fetch(&transport, "https://example.com/", &FetchOptions::default());
    assert_eq!(resp.status, None);
    assert!(resp.error.unwrap().starts_with("blocked: "));
    assert_eq!(transport.requests().len(), 1);
}

#[test]
fn fetch_refuses_a_metadata_target_before_any_request() {
    let transport = MockTransport::new(vec![], MockResponse::ok(200, &[], b"secret"));
    let resp = fetch(
        &transport,
        "http://169.254.169.254/",
        &FetchOptions::default(),
    );
    assert_eq!(resp.status, None);
    assert!(resp.error.unwrap().starts_with("blocked: "));
    assert!(transport.requests().is_empty());
    let resp = fetch(&transport, "ftp://example.com/", &FetchOptions::default());
    assert_eq!(
        resp.error.as_deref(),
        Some("blocked: scheme ftp: is not http(s)")
    );
    let resp = fetch(&transport, "not a url", &FetchOptions::default());
    assert_eq!(
        resp.error.as_deref(),
        Some("blocked: unparseable url: not a url")
    );
    assert!(transport.requests().is_empty());
}

#[test]
fn fetch_serves_a_local_target_and_its_same_class_redirect() {
    let transport = MockTransport::new(
        vec![
            get("http://localhost:8787/").response(ok(
                301,
                &[("location", "http://127.0.0.1:8787/")],
                "",
            )),
            get("http://127.0.0.1:8787/").response(ok(200, &[], "home")),
        ],
        unmatched(),
    );
    let resp = fetch(
        &transport,
        "http://localhost:8787/",
        &FetchOptions::default(),
    );
    assert_eq!(resp.status, Some(200));
    assert_eq!(resp.body, "home");
}

// ---------------------------------------------------------------------------
// User-Agent
// ---------------------------------------------------------------------------

#[test]
fn fetch_sends_the_audit_user_agent_when_the_caller_sets_none() {
    let transport = MockTransport::new(vec![], MockResponse::ok(200, &[], b""));
    fetch(&transport, "https://example.com/", &FetchOptions::default());
    let seen = &transport.requests()[0];
    let ua: Vec<&(String, String)> = seen
        .headers
        .iter()
        .filter(|(n, _)| n.eq_ignore_ascii_case("user-agent"))
        .collect();
    assert_eq!(ua.len(), 1);
    assert_eq!(ua[0].1, UA);
}

#[test]
fn fetch_keeps_a_caller_supplied_user_agent_regardless_of_casing() {
    let transport = MockTransport::new(vec![], MockResponse::ok(200, &[], b""));
    let init = FetchInit {
        method: "GET".to_string(),
        headers: vec![("User-Agent".to_string(), "custom-probe/1".to_string())],
        body: None,
    };
    fetcher(&transport).fetch("https://example.com/", &init, &FetchOptions::default());
    let seen = &transport.requests()[0];
    let ua: Vec<&(String, String)> = seen
        .headers
        .iter()
        .filter(|(n, _)| n.eq_ignore_ascii_case("user-agent"))
        .collect();
    assert_eq!(ua.len(), 1);
    assert_eq!(ua[0].0, "User-Agent");
    assert_eq!(ua[0].1, "custom-probe/1");
}

// ---------------------------------------------------------------------------
// Body regimes through the fetch layer
// ---------------------------------------------------------------------------

fn huge_body_transport(len: usize) -> MockTransport {
    MockTransport::new(
        vec![],
        MockResponse::ok(200, &[("content-type", "text/plain")], &vec![b'x'; len]),
    )
}

#[test]
fn body_cap_zero_skips_the_body() {
    let transport = huge_body_transport(AUDIT_PROBE_MAX_BODY_BYTES + 2048);
    let opts = FetchOptions {
        max_body_bytes: Some(STATUS_ONLY_BODY_BYTES),
        ..FetchOptions::default()
    };
    let resp = fetch(&transport, "https://example.com/", &opts);
    assert_eq!(resp.status, Some(200));
    assert_eq!(resp.body, "");
    assert!(!resp.truncated);
}

#[test]
fn body_over_the_probe_cap_truncates_and_flags_without_error() {
    let transport = huge_body_transport(AUDIT_PROBE_MAX_BODY_BYTES + 2048);
    let opts = FetchOptions {
        max_body_bytes: Some(AUDIT_PROBE_MAX_BODY_BYTES),
        ..FetchOptions::default()
    };
    let resp = fetch(&transport, "https://example.com/", &opts);
    assert_eq!(resp.status, Some(200));
    assert_eq!(resp.error, None);
    assert_eq!(resp.body.len(), AUDIT_PROBE_MAX_BODY_BYTES);
    assert!(resp.truncated);
}

#[test]
fn body_exactly_at_the_probe_cap_is_not_flagged() {
    let transport = huge_body_transport(AUDIT_PROBE_MAX_BODY_BYTES);
    let opts = FetchOptions {
        max_body_bytes: Some(AUDIT_PROBE_MAX_BODY_BYTES),
        ..FetchOptions::default()
    };
    let resp = fetch(&transport, "https://example.com/", &opts);
    assert_eq!(resp.body.len(), AUDIT_PROBE_MAX_BODY_BYTES);
    assert!(!resp.truncated);
}

#[test]
fn uncapped_read_takes_the_whole_body_under_the_root_ceiling() {
    let transport = huge_body_transport(AUDIT_PROBE_MAX_BODY_BYTES * 3);
    let resp = fetch(&transport, "https://example.com/", &FetchOptions::default());
    assert_eq!(resp.body.len(), AUDIT_PROBE_MAX_BODY_BYTES * 3);
    assert!(!resp.truncated);
}

#[test]
fn uncapped_read_stops_at_the_root_ceiling_with_the_flag() {
    let transport = huge_body_transport(AUDIT_ROOT_MAX_BODY_BYTES + 1);
    let resp = fetch(&transport, "https://example.com/", &FetchOptions::default());
    assert_eq!(resp.body.len(), AUDIT_ROOT_MAX_BODY_BYTES);
    assert!(resp.truncated);
    assert_eq!(resp.error, None);
}

// ---------------------------------------------------------------------------
// Real transport against loopback servers
// ---------------------------------------------------------------------------

#[test]
fn ureq_transport_returns_a_redirect_instead_of_following_it() {
    let server = common::spawn(|req| match req.target.as_str() {
        "/old" => RawResponse::new(301, &[("location", "/new")], b""),
        _ => RawResponse::new(200, &[], b"new"),
    });
    let transport = UreqTransport::new(UA);
    let resp = transport
        .send(&request(&server.url("/old"), 5000, false))
        .expect("response");
    assert_eq!(resp.status, 301);
    assert_eq!(resp.headers.get("location"), Some("/new"));
    assert_eq!(server.hits().len(), 1);
}

#[test]
fn ureq_transport_sends_the_configured_user_agent_and_lowercases_response_headers() {
    let server = common::spawn(|_| RawResponse::new(200, &[("X-Mixed-Case", "v")], b"ok"));
    let transport = UreqTransport::new(UA);
    let resp = transport
        .send(&request(&server.url("/"), 5000, false))
        .expect("response");
    assert_eq!(resp.headers.get("x-mixed-case"), Some("v"));
    assert_eq!(server.hits()[0].header("user-agent"), Some(UA));
}

fn gzip_of_zeros(len: usize) -> Vec<u8> {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    let chunk = vec![0u8; 1 << 20];
    let mut written = 0;
    while written < len {
        let n = (len - written).min(chunk.len());
        enc.write_all(&chunk[..n]).unwrap();
        written += n;
    }
    enc.finish().unwrap()
}

#[test]
fn ureq_transport_decodes_gzip_bodies() {
    let body = {
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(b"compressed hello").unwrap();
        enc.finish().unwrap()
    };
    let server =
        common::spawn(move |_| RawResponse::new(200, &[("content-encoding", "gzip")], &body));
    let transport = UreqTransport::new(UA);
    let resp = fetch(&transport, &server.url("/"), &FetchOptions::default());
    assert_eq!(resp.body, "compressed hello");
}

#[test]
fn gzip_body_inflating_past_the_root_ceiling_truncates_with_the_flag() {
    let bomb = gzip_of_zeros(AUDIT_ROOT_MAX_BODY_BYTES + (1 << 20));
    assert!(bomb.len() < 64 * 1024, "the bomb must be small on the wire");
    let server =
        common::spawn(move |_| RawResponse::new(200, &[("content-encoding", "gzip")], &bomb));
    let transport = UreqTransport::new(UA);
    let resp = fetch(&transport, &server.url("/"), &FetchOptions::default());
    assert_eq!(resp.status, Some(200));
    assert_eq!(resp.body.len(), AUDIT_ROOT_MAX_BODY_BYTES);
    assert!(resp.truncated);
}

#[test]
fn ureq_transport_reports_a_silent_server_as_the_deadline_error() {
    let addr = common::spawn_silent();
    let transport = UreqTransport::new(UA);
    let err = transport
        .send(&request(&format!("http://{addr}/"), 300, false))
        .expect_err("no response");
    assert!(matches!(err, TransportError::Timeout), "{err:?}");
    assert_eq!(err.to_string(), "TimeoutError: deadline exceeded");
}

#[test]
fn connect_timeout_and_deadline_are_distinct_errors() {
    let connect = TransportError::from(ureq::Error::Timeout(ureq::Timeout::Connect));
    let global = TransportError::from(ureq::Error::Timeout(ureq::Timeout::Global));
    assert!(matches!(connect, TransportError::ConnectTimeout));
    assert!(matches!(global, TransportError::Timeout));
    assert_ne!(connect.to_string(), global.to_string());
    let refused = TransportError::from(ureq::Error::ConnectionFailed);
    assert!(matches!(refused, TransportError::Network(_)));
}

#[test]
fn ureq_transport_reports_connection_refused_as_a_network_error() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let transport = UreqTransport::new(UA);
    let err = transport
        .send(&request(&format!("http://{addr}/"), 2000, false))
        .expect_err("refused");
    assert!(matches!(err, TransportError::Network(_)), "{err:?}");
}

// ---------------------------------------------------------------------------
// Proxy routing
// ---------------------------------------------------------------------------

fn proxy_to(server: &common::Server) -> ureq::Proxy {
    ureq::Proxy::new(&format!("http://{}", server.addr)).expect("proxy url")
}

#[test]
fn public_target_routes_through_the_proxy() {
    let proxy = common::spawn_connect_proxy(|_| RawResponse::new(200, &[], b"via proxy"));
    let transport = UreqTransport::with_proxy(UA, Some(proxy_to(&proxy)));
    let resp = transport
        .send(&request("http://198.51.100.1/probe", 5000, true))
        .expect("response through proxy");
    assert_eq!(resp.status, 200);
    let hits = proxy.hits();
    assert_eq!(hits.len(), 2, "a CONNECT and the tunneled request");
    assert_eq!(hits[0].method, "CONNECT");
    assert_eq!(hits[0].target, "198.51.100.1:80");
    assert_eq!(hits[1].target, "/probe");
}

#[test]
fn loopback_target_connects_directly_when_a_proxy_is_configured() {
    let proxy = common::spawn_connect_proxy(|_| RawResponse::new(200, &[], b"via proxy"));
    let origin = common::spawn(|_| RawResponse::new(200, &[], b"direct"));
    let transport = UreqTransport::with_proxy(UA, Some(proxy_to(&proxy)));
    let resp = fetch(&transport, &origin.url("/"), &FetchOptions::default());
    assert_eq!(resp.body, "direct");
    assert!(proxy.hits().is_empty());
    assert_eq!(origin.hits().len(), 1);
}

#[test]
fn fetcher_routes_a_public_name_through_the_proxy_and_a_loopback_literal_direct() {
    let proxy = common::spawn_connect_proxy(|_| RawResponse::new(200, &[], b"via proxy"));
    let origin = common::spawn(|_| RawResponse::new(200, &[], b"direct"));
    let transport = UreqTransport::with_proxy(UA, Some(proxy_to(&proxy)));
    let resp = fetch(&transport, "http://public.test/x", &FetchOptions::default());
    assert_eq!(resp.body, "via proxy");
    assert_eq!(proxy.hits()[0].target, "public.test:80");
    let resp = fetch(&transport, &origin.url("/"), &FetchOptions::default());
    assert_eq!(resp.body, "direct");
    assert_eq!(proxy.hits().len(), 2, "no third hit for the loopback fetch");
}

#[test]
fn no_proxy_entries_are_honored() {
    let proxy = common::spawn(|_| RawResponse::new(200, &[], b"via proxy"));
    let with_exception = ureq::Proxy::builder(ureq::ProxyProtocol::Http)
        .host(&proxy.addr.ip().to_string())
        .port(proxy.addr.port())
        .no_proxy("198.51.100.1")
        .build()
        .expect("proxy with NO_PROXY entry");
    let transport = UreqTransport::with_proxy(UA, Some(with_exception));
    let result = transport.send(&request("http://198.51.100.1:9/", 300, true));
    assert!(
        result.is_err(),
        "an unroutable direct connect cannot succeed"
    );
    assert!(
        proxy.hits().is_empty(),
        "the excepted host must bypass the proxy"
    );
}

// ---------------------------------------------------------------------------
// TLS evidence
// ---------------------------------------------------------------------------

#[test]
fn self_signed_server_yields_the_bundled_root_rejection_class() {
    let cert = include_bytes!("fixtures/tls/localhost.cert.pem");
    let key = include_bytes!("fixtures/tls/localhost.key.pem");
    let addr = common::spawn_tls(cert, key);
    let transport = UreqTransport::new(UA);
    let err = transport
        .send(&request(
            &format!("https://localhost:{}/", addr.port()),
            5000,
            false,
        ))
        .expect_err("the bundled roots must reject a self-signed chain");
    match &err {
        TransportError::Tls(tls) => {
            assert!(tls.bundled_root_rejection, "{tls:?}");
            assert!(
                err.to_string().contains("chain rejected by bundled roots"),
                "{err}"
            );
        }
        other => panic!("expected a TLS failure, got {other:?}"),
    }
    let resp = fetch(
        &transport,
        &format!("https://localhost:{}/", addr.port()),
        &FetchOptions::default(),
    );
    assert_eq!(resp.status, None);
    assert!(
        resp.error
            .unwrap()
            .contains("chain rejected by bundled roots")
    );
}

/// Needs network access; run with `cargo test -- --ignored`.
#[test]
#[ignore = "reaches a public HTTPS endpoint"]
fn bundled_roots_verify_a_public_https_endpoint() {
    let transport = UreqTransport::new(UA);
    let resp = fetch(&transport, "https://anc.dev/", &FetchOptions::default());
    assert_eq!(resp.error, None, "bundled roots must verify a public chain");
    assert!(resp.status.is_some());
}

/// A connection the pool kept but the server had already closed fails
/// before the request reaches anything, so it is retried on a fresh one.
/// A server that closes every connection instead ends in a network error
/// after a bounded number of attempts, never a loop.
#[test]
fn a_connection_closed_before_any_response_is_retried_once_and_bounded() {
    // Two dead connections, then a real answer: the probe succeeds.
    let (addr, accepted) = common::spawn_closing(2, "# recovered\n");
    let transport = UreqTransport::new(UA);
    let resp = fetch(
        &transport,
        &format!("http://{addr}/llms.txt"),
        &FetchOptions::default(),
    );
    assert_eq!(resp.status, Some(200), "{:?}", resp.error);
    assert_eq!(resp.body, "# recovered\n");
    assert_eq!(
        *accepted.lock().unwrap(),
        3,
        "the probe walked past both dead connections and stopped"
    );

    // A server that never answers: the error surfaces, and the attempts
    // are bounded rather than retried forever.
    let (addr, accepted) = common::spawn_closing(usize::MAX, "");
    let resp = fetch(
        &transport,
        &format!("http://{addr}/llms.txt"),
        &FetchOptions::default(),
    );
    assert_eq!(resp.status, None);
    let error = resp.error.expect("a network error");
    assert!(error.starts_with("NetworkError: "), "{error}");
    let attempts = *accepted.lock().unwrap();
    assert!(
        (1..=3).contains(&attempts),
        "attempts must be bounded, saw {attempts}"
    );
}
