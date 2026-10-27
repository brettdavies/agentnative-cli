//! The API category at the API catalog's anchor hosts, end to end over the
//! mock transport with the real handlers: the OpenAPI row scores each
//! description where the catalog declares it, the hygiene rows probe each
//! anchor host once, the follow's host slots and refusals settle what the
//! declared-hosts trail records, the follow's clock starts where the site
//! starts it, and a description larger than the read cap counts as present
//! only when it is shaped like one.

#![cfg(feature = "web")]

use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use agentnative::web_audit::engine::{FetchHandle, RunInput, RunOutcome, RunReport, run_web_audit};
use agentnative::web_audit::handlers::default_handlers;
use agentnative::web_audit::locality::Resolver;
use agentnative::web_audit::mock::{MockResponse, MockRule, MockTransport};
use agentnative::web_audit::registry::MCP_DISCOVERY;
use agentnative::web_audit::scorecard::{EngineResult, NaReason, ResultRow, ScorecardStatus};
use agentnative::web_audit::transport::{Request, Response, Transport, TransportError};
use serde_json::{Value, json};

const BASE: &str = "https://example.com/";
const CATALOG: &str = "https://example.com/.well-known/api-catalog";

struct PublicResolver;

impl Resolver for PublicResolver {
    fn resolve(&self, _host: &str) -> Result<Vec<IpAddr>, String> {
        Ok(vec!["93.184.216.34".parse().unwrap()])
    }
}

fn ok(content_type: &str, body: &str) -> MockResponse {
    MockResponse::ok(200, &[("content-type", content_type)], body.as_bytes())
}

fn get(url: &str, response: MockResponse) -> MockRule {
    MockRule::new("GET", url).response(response)
}

fn json_error() -> MockResponse {
    MockResponse::ok(
        404,
        &[("content-type", "application/json")],
        br#"{"error":{"message":"no such route"}}"#,
    )
}

/// A catalog whose linkset holds one context per `(anchor, service-desc)`.
fn catalog(contexts: &[(&str, &str)]) -> MockRule {
    let linkset: Vec<Value> = contexts
        .iter()
        .map(|(anchor, desc)| json!({"anchor": anchor, "service-desc": [{"href": desc}]}))
        .collect();
    get(
        CATALOG,
        ok(
            "application/linkset+json",
            &json!({ "linkset": linkset }).to_string(),
        ),
    )
}

fn mock(mut rules: Vec<MockRule>) -> MockTransport {
    rules.insert(
        0,
        get(
            BASE,
            ok("text/html; charset=utf-8", "<html><body>hi</body></html>"),
        ),
    );
    MockTransport::new(
        rules,
        MockResponse::ok(404, &[("content-type", "text/html")], b"<html>nope</html>"),
    )
}

fn run_on(transport: Arc<dyn Transport + Send + Sync>) -> Box<RunReport> {
    let fetch = FetchHandle::new(transport, Arc::new(PublicResolver), "anc-test/0");
    let mut input = RunInput::new(BASE, fetch, Arc::new(default_handlers()));
    input.per_check_timeout = Duration::from_secs(2);
    input.per_audit_deadline = Duration::from_secs(20);
    match run_web_audit(input) {
        RunOutcome::Complete(report) => report,
        RunOutcome::Unreachable { reason, .. } => panic!("unreachable: {reason}"),
    }
}

fn run(rules: Vec<MockRule>) -> (Box<RunReport>, Arc<MockTransport>) {
    let transport = Arc::new(mock(rules));
    (run_on(transport.clone()), transport)
}

/// A mock that holds each POST and records when every request was sent and
/// when its answer came back.
struct Timed {
    inner: MockTransport,
    post_delay: Duration,
    log: Mutex<Vec<(String, String, Instant, Instant)>>,
}

impl Transport for Timed {
    fn send(&self, request: &Request) -> Result<Response, TransportError> {
        let sent = Instant::now();
        if request.method == "POST" {
            std::thread::sleep(self.post_delay);
        }
        let answer = self.inner.send(request);
        let entry = (
            request.method.clone(),
            request.url.clone(),
            sent,
            Instant::now(),
        );
        self.log.lock().unwrap().push(entry);
        answer
    }
}

fn result<'a>(report: &'a RunReport, id: &str) -> &'a EngineResult {
    report.results.iter().find(|r| r.check.id == id).unwrap()
}

fn wire<'a>(report: &'a RunReport, id: &str) -> &'a ResultRow {
    report
        .scorecard
        .results
        .iter()
        .find(|r| r.id == id)
        .unwrap()
}

fn hosts(report: &RunReport, id: &str) -> Value {
    serde_json::to_value(&wire(report, id).hosts).unwrap()
}

fn requested(transport: &MockTransport, url: &str) -> usize {
    transport.requests().iter().filter(|r| r.url == url).count()
}

#[test]
fn the_api_rows_evaluate_at_the_catalog_anchor_hosts() {
    let spec = r#"{"openapi":"3.1.0","paths":{"/v1/items/{id}":{"get":{"responses":{"404":{}}}}}}"#;
    let (report, transport) = run(vec![
        catalog(&[
            ("https://api.test/", "https://api.test/openapi.json"),
            ("https://files.test/", "https://files.test/openapi.yaml"),
            (
                "https://mcp.test/mcp",
                "https://mcp.test/.well-known/mcp/server-card.json",
            ),
        ]),
        get(
            "https://api.test/openapi.json",
            ok("application/json", spec),
        ),
        get(
            "https://api.test/v1/items/anc-web-audit-no-such",
            json_error(),
        ),
        get(
            "https://files.test/openapi.yaml",
            ok("application/yaml", "openapi: 3.1.0\npaths: {}\n"),
        ),
        get("https://files.test/anc-web-audit-no-such-api", json_error()),
    ]);

    assert_eq!(result(&report, "openapi").status, ScorecardStatus::Pass);
    let both = |status: &str| {
        json!([
            {"host": "api.test", "status": status},
            {"host": "files.test", "status": status},
        ])
    };
    assert_eq!(hosts(&report, "openapi"), both("pass"));
    assert_eq!(wire(&report, "openapi").host, None);
    assert_eq!(result(&report, "json-errors").status, ScorecardStatus::Pass);
    assert_eq!(
        result(&report, "json-errors").evidence,
        "https://api.test/v1/items/anc-web-audit-no-such -> 404"
    );
    assert_eq!(hosts(&report, "json-errors"), both("pass"));
    assert_eq!(
        result(&report, "rate-limit-headers").status,
        ScorecardStatus::Absent
    );
    assert_eq!(hosts(&report, "rate-limit-headers"), both("absent"));

    // Both hygiene rows share one GET per anchor host, and none reaches the
    // audited site or the anchor that declares no API description.
    assert_eq!(
        requested(
            &transport,
            "https://api.test/v1/items/anc-web-audit-no-such"
        ),
        1
    );
    assert_eq!(
        requested(&transport, "https://files.test/anc-web-audit-no-such-api"),
        1
    );
    assert_eq!(
        requested(&transport, "https://example.com/anc-web-audit-no-such-api"),
        0
    );
    assert!(
        transport
            .requests()
            .iter()
            .all(|r| !r.url.contains("mcp.test"))
    );
    let trail: Vec<(String, String, Option<String>)> = report
        .scorecard
        .declared_hosts
        .iter()
        .map(|e| {
            (
                e["kind"].as_str().unwrap().to_string(),
                e["outcome"].as_str().unwrap().to_string(),
                e.get("reason").and_then(Value::as_str).map(str::to_string),
            )
        })
        .collect();
    let entry = |kind: &str, outcome: &str, reason: Option<&str>| {
        (
            kind.to_string(),
            outcome.to_string(),
            reason.map(str::to_string),
        )
    };
    assert_eq!(
        trail,
        [
            entry("api-anchor", "followed", None),
            entry("api-anchor", "followed", None),
            entry("api-anchor", "not-followed", Some("no-service-desc")),
            entry("api-description", "followed", None),
            entry("api-description", "followed", None),
        ]
    );
    assert_eq!(
        report.scorecard.declared_hosts[2]["surface"],
        json!("/.well-known/api-catalog#/linkset/2")
    );
}

#[test]
fn a_description_larger_than_the_read_cap_counts_present_only_when_shaped_like_one() {
    let padding = format!("\"{}\"", "x".repeat(600 * 1024));
    let over_cap = format!(r#"{{"components":{{"pad":{padding}}},"openapi":"3.1.0"}}"#);
    let page = format!(
        "<html><body>{}openapi</body></html>",
        "x".repeat(600 * 1024)
    );
    let anchored = |body: &str, content_type: &str| {
        run(vec![
            catalog(&[("https://api.test/", "https://api.test/spec3.json")]),
            get("https://api.test/spec3.json", ok(content_type, body)),
        ])
        .0
    };

    let report = anchored(&over_cap, "application/json");
    let openapi = result(&report, "openapi");
    assert_eq!(openapi.status, ScorecardStatus::Pass);
    let item = &openapi.raw_evidence[0];
    assert_eq!(item["truncated"], json!(true));
    assert_eq!(
        item["why"],
        json!([
            "status 200 in [200]",
            "description larger than 512 KiB; read in part, presence counted"
        ])
    );
    // A truncated body never parses, so the probe falls back to the nonsense path.
    assert_eq!(
        result(&report, "json-errors").raw_evidence[0]["url"],
        json!("https://api.test/anc-web-audit-no-such-api")
    );

    let report = anchored(&page, "text/html");
    assert_eq!(result(&report, "openapi").status, ScorecardStatus::Broken);
}

#[test]
fn a_description_host_that_never_answers_or_redirects_twice_reads_unreachable() {
    let report = run(vec![
        catalog(&[("https://api.test/", "https://api.test/openapi.json")]),
        get(
            "https://api.test/openapi.json",
            MockResponse::error("connection refused"),
        ),
    ])
    .0;
    let openapi = result(&report, "openapi");
    assert_eq!(openapi.status, ScorecardStatus::NA);
    assert_eq!(openapi.na_reason, Some(NaReason::DeclaredHostUnreachable));
    assert_eq!(openapi.evidence, "https://api.test/openapi.json");
    assert_eq!(wire(&report, "openapi").host.as_deref(), Some("api.test"));

    let hop = |to: &str| MockResponse::ok(302, &[("location", to)], b"");
    let report = run(vec![
        catalog(&[("https://api.test/", "https://api.test/openapi.json")]),
        get(
            "https://api.test/openapi.json",
            hop("https://spec.test/v1.json"),
        ),
        get(
            "https://spec.test/v1.json",
            hop("https://spec.test/v2.json"),
        ),
    ])
    .0;
    let openapi = result(&report, "openapi");
    assert_eq!(openapi.na_reason, Some(NaReason::DeclaredHostUnreachable));
    assert_eq!(openapi.evidence, "https://spec.test/v1.json");
    let description = &report.scorecard.declared_hosts[1];
    assert_eq!(description["final_url"], json!("https://spec.test/v1.json"));
    assert_eq!(description["outcome"], json!("unreachable"));
}

#[test]
fn the_follow_admits_four_hosts_and_refuses_what_it_may_not_request() {
    let contexts: Vec<(String, String)> = (1..=5)
        .map(|n| {
            (
                format!("https://h{n}.test/"),
                format!("https://h{n}.test/o.json"),
            )
        })
        .collect();
    let borrowed: Vec<(&str, &str)> = contexts
        .iter()
        .map(|(a, d)| (a.as_str(), d.as_str()))
        .collect();
    let report = run(vec![catalog(&borrowed)]).0;
    let outcomes: Vec<(&str, Option<&str>)> = report
        .scorecard
        .declared_hosts
        .iter()
        .map(|e| {
            (
                e["outcome"].as_str().unwrap(),
                e.get("cause").and_then(Value::as_str),
            )
        })
        .collect();
    let mut want = vec![("followed", None); 4];
    want.push(("budget-exceeded", Some("per-audit-cap")));
    want.extend(vec![("followed", None); 4]);
    want.push(("budget-exceeded", Some("per-audit-cap")));
    assert_eq!(outcomes, want);
    let json_errors = result(&report, "json-errors");
    assert_eq!(
        json_errors.raw_evidence.last().unwrap()["na_reason"],
        json!("declared-host-budget-exceeded")
    );

    let report = run(vec![catalog(&[
        ("http://plain.test/", "http://plain.test/o.json"),
        ("https://93.184.216.34/", "https://93.184.216.34/o.json"),
        ("https://docs.anc.dev/", "https://docs.anc.dev/o.json"),
    ])])
    .0;
    let settled: Vec<(&str, Option<&str>)> = report.scorecard.declared_hosts[..3]
        .iter()
        .map(|e| {
            (
                e["outcome"].as_str().unwrap(),
                e.get("reason").and_then(Value::as_str),
            )
        })
        .collect();
    assert_eq!(
        settled,
        [
            ("not-followed", Some("insecure-scheme")),
            ("blocked", None),
            ("not-followed", Some("self-path")),
        ]
    );
    // A description declared over plaintext stands aside for the blocked one.
    let openapi = result(&report, "openapi");
    assert_eq!(openapi.status, ScorecardStatus::NA);
    assert_eq!(openapi.na_reason, Some(NaReason::DeclaredHostBlocked));
}

#[test]
fn the_follow_clock_starts_after_the_document_reads_and_before_the_posts() {
    let transport = Arc::new(Timed {
        inner: mock(vec![catalog(&[(
            "https://api.test/",
            "https://api.test/openapi.json",
        )])]),
        post_delay: Duration::from_millis(300),
        log: Mutex::new(Vec::new()),
    });
    let report = run_on(transport.clone());
    let opened = report
        .discovery
        .follow_opened
        .expect("discovery opened the follow");
    let log = transport.log.lock().unwrap();
    let mut documents: Vec<String> = MCP_DISCOVERY
        .well_known
        .iter()
        .chain([&MCP_DISCOVERY.ai_catalog])
        .map(|path| format!("https://example.com{path}"))
        .collect();
    documents.push(CATALOG.to_string());
    let read = log
        .iter()
        .filter(|(method, url, ..)| method == "GET" && documents.contains(url))
        .map(|(.., answered)| *answered)
        .max()
        .expect("the document reads");
    let first_post = log
        .iter()
        .filter(|(method, ..)| method == "POST")
        .map(|(_, _, sent, _)| *sent)
        .min()
        .expect("discovery POSTed to the common paths");
    assert!(
        read <= opened,
        "the clock started before a document read answered"
    );
    assert!(
        opened <= first_post,
        "the clock started after the first POST went out"
    );
    let described = log
        .iter()
        .find(|(_, url, ..)| url == "https://api.test/openapi.json")
        .map(|(_, _, sent, _)| *sent)
        .expect("the description read");
    assert!(
        described >= first_post + Duration::from_millis(300),
        "the API batch settles after the POSTs"
    );
}
