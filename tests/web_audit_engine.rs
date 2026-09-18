//! The wave scheduler end to end: discovery, the reachability gate, the two
//! waves and their antecedent gating, the wave-2 context, the optional
//! re-tag, the locality DNS gate, unported handlers, and the deadline
//! against a target that tarpits or trickles.
//!
//! The handlers here are stand-ins: each fetches `__probe/<check id>` on
//! the target and turns the answer into an outcome, with a 200 JSON body
//! taken verbatim as the evidence row, so a scenario shapes wave-1
//! evidence through mock rules alone.

use std::io::{self, Cursor, Read};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use agentnative::web_audit::engine::{
    DEGRADED_PER_CHECK_TIMEOUT, EXTERNAL_DNS_WITHHELD, FetchHandle, HandlerContext, HandlerSet,
    ProbeOutcome, ProbeStatus, ProgressSink, RunInput, RunOutcome, RunReport, Unported,
    run_web_audit,
};
use agentnative::web_audit::fetch::FetchInit;
use agentnative::web_audit::headers::Headers;
use agentnative::web_audit::locality::{Locality, Resolver};
use agentnative::web_audit::mock::{MockResponse, MockRule, MockTransport};
use agentnative::web_audit::registry::{AntecedentToken, CHECKS, HANDLER_KINDS, WebCheck};
use agentnative::web_audit::scorecard::{
    DeclaredSiteType, EngineResult, EvidenceItem, NaReason, ScorecardStatus,
};
use agentnative::web_audit::transport::{Request, Response, Transport, TransportError};
use serde_json::{Value, json};

const BASE: &str = "https://example.com/";
const UA: &str = "anc-web-audit-test/0 (+https://anc.dev/audit)";
const HTML: &str = "<!doctype html><html><head><meta name=\"description\" content=\"x\"></head><body>hi</body></html>";

struct PublicResolver;

impl Resolver for PublicResolver {
    fn resolve(&self, _host: &str) -> Result<Vec<IpAddr>, String> {
        Ok(vec!["93.184.216.34".parse().unwrap()])
    }
}

fn handle(transport: Arc<dyn Transport + Send + Sync>) -> FetchHandle {
    FetchHandle::new(transport, Arc::new(PublicResolver), UA)
}

fn html(status: u16) -> MockResponse {
    MockResponse::ok(
        status,
        &[("content-type", "text/html; charset=utf-8")],
        HTML.as_bytes(),
    )
}

fn json_body(value: Value) -> MockResponse {
    MockResponse::ok(
        200,
        &[("content-type", "application/json")],
        value.to_string().as_bytes(),
    )
}

fn probe_rule(id: &str) -> MockRule {
    MockRule::new("GET", &format!("{BASE}__probe/{id}"))
}

// Stand-in handlers.

fn probe(check: &WebCheck, ctx: &HandlerContext) -> ProbeOutcome {
    let url = format!("{}__probe/{}", ctx.base, check.id);
    let resp = ctx
        .fetch
        .fetch(&url, &FetchInit::default(), &ctx.fetch_options(None));
    match resp.status {
        None => ProbeOutcome::error(resp.error.as_deref().unwrap_or("fetch failed")),
        Some(200) => {
            let mut item: EvidenceItem = serde_json::from_str(&resp.body).unwrap_or_default();
            item.entry("status").or_insert(json!(200));
            item.entry("url").or_insert(json!(url));
            ProbeOutcome::new(ProbeStatus::Pass, vec![item])
        }
        Some(status) => {
            let mut item = EvidenceItem::new();
            item.insert("status".to_string(), json!(status));
            item.insert("url".to_string(), json!(url));
            ProbeOutcome::new(ProbeStatus::Absent, vec![item])
        }
    }
}

fn probe_with(
    check: &WebCheck,
    ctx: &HandlerContext,
    extra: impl FnOnce(&HandlerContext, &mut EvidenceItem),
) -> ProbeOutcome {
    let mut outcome = probe(check, ctx);
    if let Some(item) = outcome.evidence.first_mut() {
        extra(ctx, item);
    }
    outcome
}

/// Every registry kind and the eval rule, each recording the slice of
/// context it would consume.
fn stand_in_handlers() -> Arc<HandlerSet> {
    let mut set = HandlerSet::new();
    for kind in HANDLER_KINDS {
        set.register(kind, probe);
    }
    set.register("scoped-llms", |check, ctx| {
        probe_with(check, ctx, |ctx, item| {
            item.insert("scoped_dirs".to_string(), json!(*ctx.scoped_dirs));
        })
    });
    set.register("llms-txt-quality", |check, ctx| {
        probe_with(check, ctx, |ctx, item| {
            item.insert(
                "retained".to_string(),
                json!(ctx.retained_bodies.get("llms-txt")),
            );
        })
    });
    set.register("api-hygiene", |check, ctx| {
        probe_with(check, ctx, |ctx, item| {
            item.insert(
                "retained".to_string(),
                json!(ctx.retained_bodies.get("openapi")),
            );
        })
    });
    set.register("mcp", |check, ctx| {
        probe_with(check, ctx, |ctx, item| {
            item.insert("session".to_string(), json!(ctx.mcp_session_id));
            item.insert("endpoint".to_string(), json!(ctx.mcp_endpoint));
            item.insert(
                "lane".to_string(),
                json!(format!("{:?}", ctx.mcp_lanes.modern)),
            );
            item.insert(
                "legacy_advertised".to_string(),
                json!(ctx.mcp_lanes.legacy_advertised),
            );
        })
    });
    set.register("dns-doh", |check, ctx| {
        probe_with(check, ctx, |ctx, item| {
            item.insert("external_dns".to_string(), json!(ctx.external_dns));
        })
    });
    set.register_eval("legacy-alias-redirects", probe);
    Arc::new(set)
}

// Scenarios.

/// Root HTML, an MCP card, and JSON-shaped wave-1 sources.
fn healthy_rules() -> Vec<MockRule> {
    vec![
        MockRule::new("GET", BASE).response(html(200)),
        MockRule::new("GET", &format!("{BASE}.well-known/mcp.json"))
            .response(json_body(json!({ "mcp_endpoint": "/mcp" }))),
        MockRule::new("POST", &format!("{BASE}mcp"))
            .body_contains("notifications/initialized")
            .response(MockResponse::ok(202, &[], b"")),
        probe_rule("llms-txt").response(json_body(json!({
            "body": "# X\n- [Guide](/docs/guide.md)\n- [Ref](/api/v1/ref)\n"
        }))),
        probe_rule("sitemap").response(json_body(json!({
            "body": "<urlset><url><loc>https://example.com/blog/post</loc></url></urlset>"
        }))),
        probe_rule("openapi").response(json_body(json!({ "body": "{\"openapi\":\"3.1.0\"}" }))),
        probe_rule("mcp-initialize").response(json_body(json!({
            "session_id": "sess-1",
            "capabilities": ["tools", "resources"]
        }))),
        probe_rule("mcp-server-discover")
            .response(json_body(json!({ "supported_versions": ["2026-07-28"] }))),
    ]
}

fn healthy_transport() -> Arc<MockTransport> {
    Arc::new(MockTransport::new(healthy_rules(), html(200)))
}

fn input<'a>(
    transport: Arc<dyn Transport + Send + Sync>,
    handlers: Arc<HandlerSet>,
) -> RunInput<'a> {
    let mut input = RunInput::new(BASE, handle(transport), handlers);
    input.per_check_timeout = Duration::from_secs(2);
    input.per_audit_deadline = Duration::from_secs(20);
    input.spec_version = "test".to_string();
    input
}

fn complete(outcome: RunOutcome) -> Box<RunReport> {
    match outcome {
        RunOutcome::Complete(report) => report,
        RunOutcome::Unreachable { reason, .. } => panic!("unreachable: {reason}"),
    }
}

fn row<'a>(report: &'a RunReport, id: &str) -> &'a EngineResult {
    report
        .results
        .iter()
        .find(|r| r.check.id == id)
        .unwrap_or_else(|| panic!("no row {id}"))
}

fn raw<'a>(report: &'a RunReport, id: &str, key: &str) -> Option<&'a Value> {
    row(report, id).raw_evidence.first()?.get(key)
}

#[derive(Default)]
struct Collected {
    discovery: Vec<Option<String>>,
    ids: Vec<&'static str>,
}

impl ProgressSink for Collected {
    fn discovery(&mut self, endpoint: Option<&str>) {
        self.discovery.push(endpoint.map(str::to_string));
    }
    fn result(&mut self, result: &EngineResult) {
        self.ids.push(result.check.id);
    }
}

#[test]
fn a_healthy_target_runs_both_waves_into_a_full_scorecard() {
    let transport = healthy_transport();
    let mut progress = Collected::default();
    let mut input = input(transport.clone(), stand_in_handlers());
    input.progress = Some(&mut progress);
    let report = complete(run_web_audit(input));

    let ids: Vec<&str> = report.results.iter().map(|r| r.check.id).collect();
    let registry: Vec<&str> = CHECKS.iter().map(|c| c.id).collect();
    assert_eq!(ids, registry, "results follow registry order");
    assert_eq!(report.scorecard.results.len(), 65);
    assert!(report.complete);
    assert!(report.unported.is_empty());
    assert_eq!(report.target_locality, Some(Locality::Public));
    assert_eq!(
        report.scorecard.mcp_endpoint.as_deref(),
        Some("https://example.com/mcp")
    );
    assert_eq!(report.scorecard.summary.pass, 64);
    assert_eq!(report.scorecard.summary.n_a, 1);
    assert_eq!(report.scorecard.score.relative, 100);
    let auth = row(&report, "oauth-protected-resource");
    assert_eq!(auth.status, ScorecardStatus::NA);
    assert_eq!(auth.na_reason, Some(NaReason::AntecedentUnmet));
    assert_eq!(auth.evidence, "MCP endpoint does not challenge for auth");

    // Wave 2 saw the wave-1 derivations.
    assert_eq!(
        raw(&report, "llms-txt-scoped", "scoped_dirs"),
        Some(&json!(["/docs", "/api", "/blog"]))
    );
    assert_eq!(
        raw(&report, "llms-txt-format", "retained"),
        Some(&json!(
            "# X\n- [Guide](/docs/guide.md)\n- [Ref](/api/v1/ref)\n"
        ))
    );
    assert_eq!(
        raw(&report, "json-errors", "retained"),
        Some(&json!("{\"openapi\":\"3.1.0\"}"))
    );
    assert_eq!(
        raw(&report, "mcp-tools-list", "session"),
        Some(&json!("sess-1"))
    );
    assert_eq!(
        raw(&report, "mcp-tools-list", "endpoint"),
        Some(&json!("https://example.com/mcp"))
    );
    assert_eq!(
        raw(&report, "mcp-tools-list", "lane"),
        Some(&json!("Present"))
    );
    assert_eq!(
        raw(&report, "mcp-tools-list", "legacy_advertised"),
        Some(&json!(["tools", "resources"]))
    );
    // Wave 1 ran before any derivation existed.
    assert_eq!(
        raw(&report, "mcp-initialize", "session"),
        Some(&Value::Null)
    );

    // The session handshake went out once, between the waves.
    let notify: Vec<Request> = transport
        .requests()
        .into_iter()
        .filter(|r| {
            r.method == "POST"
                && r.url == "https://example.com/mcp"
                && String::from_utf8_lossy(r.body.as_deref().unwrap_or(&[]))
                    .contains("notifications/initialized")
        })
        .collect();
    assert_eq!(notify.len(), 1);
    assert!(
        notify[0]
            .headers
            .iter()
            .any(|(n, v)| n.eq_ignore_ascii_case("mcp-session-id") && v == "sess-1"),
        "{:?}",
        notify[0].headers
    );

    assert_eq!(
        progress.discovery,
        vec![Some("https://example.com/mcp".to_string())]
    );
    assert_eq!(progress.ids.len(), 65);
}

#[test]
fn unmet_antecedents_cascade_and_absent_mays_become_optional() {
    let rules = vec![
        MockRule::new("GET", BASE).response(html(200)),
        MockRule::new("GET", &format!("{BASE}.well-known/mcp.json")).response(MockResponse::ok(
            404,
            &[],
            b"",
        )),
        MockRule::new("GET", &format!("{BASE}.well-known/mcp/server-card.json"))
            .response(MockResponse::ok(404, &[], b"")),
        MockRule::new("POST", &format!("{BASE}mcp")).response(MockResponse::ok(404, &[], b"")),
        MockRule::new("POST", &format!("{BASE}sse")).response(MockResponse::ok(404, &[], b"")),
        MockRule::new("POST", &format!("{BASE}message")).response(MockResponse::ok(404, &[], b"")),
        probe_rule("robots").response(MockResponse::ok(404, &[], b"")),
        probe_rule("llms-txt").response(MockResponse::ok(404, &[], b"")),
        probe_rule("openapi").response(MockResponse::ok(404, &[], b"")),
        probe_rule("oauth-discovery").response(MockResponse::ok(404, &[], b"")),
        probe_rule("security-txt").response(MockResponse::ok(404, &[], b"")),
    ];
    let transport = Arc::new(MockTransport::new(rules, html(200)));
    let report = complete(run_web_audit(input(transport, stand_in_handlers())));

    assert_eq!(report.scorecard.mcp_endpoint, None);
    assert_eq!(report.scorecard.mcp_discovery.len(), 6);
    let expect_unmet = |id: &str, evidence: &str| {
        let r = row(&report, id);
        assert_eq!(r.status, ScorecardStatus::NA, "{id}");
        assert_eq!(r.na_reason, Some(NaReason::AntecedentUnmet), "{id}");
        assert_eq!(r.evidence, evidence, "{id}");
    };
    // A SHOULD that is absent stays absent; its dependents are gated.
    assert_eq!(row(&report, "robots").status, ScorecardStatus::Absent);
    expect_unmet("robots-ai-rules", "robots.txt not present");
    expect_unmet("content-signals", "robots.txt not present");
    assert_eq!(row(&report, "llms-txt").status, ScorecardStatus::Absent);
    expect_unmet("llms-txt-format", "root llms.txt not present");
    expect_unmet("llms-txt-scoped", "root llms.txt not present");
    expect_unmet("llms-full-txt", "not a docs/content site");
    // A wave-1 source gated by its own antecedent takes the gate's verdict.
    expect_unmet("openapi", "no API surface detected");
    expect_unmet("json-errors", "no API surface detected");
    expect_unmet("json-schemas", "no JSON Schema references detected");
    expect_unmet("auth-md", "no auth surface detected");
    // Every MCP-gated row carries its own token's line, as on the site.
    let mcp_gated = |c: &&WebCheck| {
        matches!(
            c.antecedent,
            AntecedentToken::McpPresent | AntecedentToken::McpResources | AntecedentToken::McpAuth
        )
    };
    for check in CHECKS.iter().filter(mcp_gated) {
        let line = match check.antecedent {
            AntecedentToken::McpResources => {
                "neither initialize nor server/discover advertises capabilities.resources"
            }
            AntecedentToken::McpAuth => "MCP endpoint does not challenge for auth",
            _ => "no MCP endpoint discovered",
        };
        expect_unmet(check.id, line);
    }
    // An applicable MAY that is absent is optional, not a miss.
    let security = row(&report, "security-txt");
    assert_eq!(security.status, ScorecardStatus::NA);
    assert_eq!(security.na_reason, Some(NaReason::OptionalAbsent));
    // An applicable SHOULD that passed is untouched.
    assert_eq!(row(&report, "sitemap").status, ScorecardStatus::Pass);
    assert!(report.complete);
}

#[test]
fn the_declared_site_type_gates_before_antecedents_and_a_discovered_mcp_overrides_it() {
    let transport = healthy_transport();
    let mut content = input(transport, stand_in_handlers());
    content.site_type = Some(DeclaredSiteType::Content);
    let report = complete(run_web_audit(content));
    let openapi = row(&report, "openapi");
    assert_eq!(openapi.status, ScorecardStatus::NA);
    assert_eq!(openapi.na_reason, Some(NaReason::AntecedentUnmet));
    assert_eq!(openapi.evidence, "not applicable to the declared site type");
    assert_eq!(
        row(&report, "json-schemas").evidence,
        "not applicable to the declared site type"
    );
    // A check typed for MCP applies once an endpoint is discovered.
    assert_eq!(
        row(&report, "oauth-discovery").status,
        ScorecardStatus::Pass
    );
    assert_eq!(row(&report, "mcp-initialize").status, ScorecardStatus::Pass);
    assert_eq!(row(&report, "llms-full-txt").status, ScorecardStatus::Pass);
    assert_eq!(report.scorecard.site_type, Some(DeclaredSiteType::Content));

    let transport = healthy_transport();
    let mut api = input(transport, stand_in_handlers());
    api.site_type = Some(DeclaredSiteType::Api);
    let report = complete(run_web_audit(api));
    assert_eq!(row(&report, "openapi").status, ScorecardStatus::Pass);
    assert_eq!(
        row(&report, "llms-full-txt").evidence,
        "not applicable to the declared site type"
    );
    assert_eq!(
        row(&report, "llms-txt-scoped").evidence,
        "not applicable to the declared site type"
    );
}

#[test]
fn silence_and_edge_errors_are_unreachable_but_any_real_status_audits() {
    let silent = Arc::new(MockTransport::new(
        vec![],
        MockResponse::error("connection refused"),
    ));
    match run_web_audit(input(silent, stand_in_handlers())) {
        RunOutcome::Unreachable { reason, discovery } => {
            assert!(reason.starts_with("https://example.com/ did not answer any probe (no HTTP response from the root fetch or MCP discovery)."), "{reason}");
            assert_eq!(discovery.len(), 6);
            assert_eq!(discovery[0].get("status"), Some(&Value::Null));
        }
        RunOutcome::Complete(_) => panic!("a silent target scored"),
    }

    let edge = Arc::new(MockTransport::new(vec![], MockResponse::ok(530, &[], b"")));
    match run_web_audit(input(edge, stand_in_handlers())) {
        RunOutcome::Unreachable { reason, .. } => {
            assert!(
                reason.contains("every response was a Cloudflare edge error"),
                "{reason}"
            );
        }
        RunOutcome::Complete(_) => panic!("an edge-only target scored"),
    }

    let not_found = Arc::new(MockTransport::new(vec![], MockResponse::ok(404, &[], b"")));
    let report = complete(run_web_audit(input(not_found, stand_in_handlers())));
    assert_eq!(report.results.len(), 65);
    assert_eq!(
        row(&report, "agent-friendly-404").status,
        ScorecardStatus::Absent
    );
    let webmcp = row(&report, "webmcp");
    assert_eq!(webmcp.status, ScorecardStatus::NA);
    assert_eq!(webmcp.evidence, "root is not an HTML document");
}

#[test]
fn a_dead_root_degrades_the_per_check_timeout_and_errors_root_dependents() {
    let rules = vec![
        MockRule::new("GET", BASE).response(MockResponse::error("connection refused")),
        MockRule::new("POST", &format!("{BASE}mcp"))
            .body_json_method("initialize")
            .response(json_body(json!({
                "jsonrpc": "2.0", "id": 1, "result": { "serverInfo": { "name": "x" } }
            }))),
        probe_rule("mcp-tools-list").response(json_body(json!({}))),
    ];
    let transport = Arc::new(MockTransport::new(rules, MockResponse::ok(404, &[], b"")));
    let mut input = input(transport.clone(), stand_in_handlers());
    input.per_check_timeout = Duration::from_secs(5);
    let report = complete(run_web_audit(input));
    assert_eq!(
        report.scorecard.mcp_endpoint.as_deref(),
        Some("https://example.com/mcp")
    );

    let requests = transport.requests();
    assert_eq!(requests[0].url, BASE);
    assert_eq!(requests[0].timeout, Duration::from_secs(5));
    assert!(
        requests[1..]
            .iter()
            .all(|r| r.timeout <= DEGRADED_PER_CHECK_TIMEOUT),
        "{:?}",
        requests[1..].iter().map(|r| r.timeout).collect::<Vec<_>>()
    );
    for id in ["agent-friendly-404", "webmcp", "markdown-vary"] {
        let r = row(&report, id);
        assert_eq!(r.status, ScorecardStatus::Error, "{id}");
        assert_eq!(
            r.evidence, "antecedent unresolvable: root fetch failed",
            "{id}"
        );
    }
    assert_eq!(row(&report, "mcp-tools-list").status, ScorecardStatus::Pass);
}

#[test]
fn an_unported_handler_skips_its_rows_by_name_without_failing_the_run() {
    let transport = healthy_transport();
    let report = complete(run_web_audit(input(transport, Arc::new(HandlerSet::new()))));
    assert!(report.complete);
    let robots = row(&report, "robots");
    assert_eq!(robots.status, ScorecardStatus::Skip);
    assert_eq!(
        robots.evidence,
        "handler `http` not yet ported to the local engine"
    );
    assert_eq!(
        row(&report, "mcp-card-legacy-aliases").evidence,
        "handler `legacy-alias-redirects` not yet ported to the local engine"
    );
    assert!(report.unported.contains(&Unported {
        check_id: "robots",
        kind: "http",
    }));
    let skipped = report
        .results
        .iter()
        .filter(|r| r.status == ScorecardStatus::Skip)
        .count();
    assert_eq!(report.unported.len(), skipped);
    // A source that was skipped does not satisfy its dependents.
    assert_eq!(row(&report, "robots-ai-rules").status, ScorecardStatus::NA);
    assert_eq!(report.scorecard.summary.skip as usize, skipped);
}

#[test]
fn the_dns_gate_withholds_external_resolvers_for_a_local_target_unless_overridden() {
    let local = "http://127.0.0.1:8080/";
    let run = |external_dns: bool| {
        let transport = Arc::new(MockTransport::new(vec![], html(200)));
        let mut input = RunInput::new(
            local,
            handle(Arc::clone(&transport) as Arc<_>),
            stand_in_handlers(),
        );
        input.per_check_timeout = Duration::from_secs(2);
        input.per_audit_deadline = Duration::from_secs(20);
        input.external_dns = external_dns;
        (complete(run_web_audit(input)), transport)
    };
    let (withheld, transport) = run(false);
    // A local run reaches the target and nothing else: no DNS resolver, no
    // anc.dev, nothing a person auditing their own network did not ask for.
    for request in transport.requests() {
        assert!(
            request.url.starts_with(local),
            "a local run contacted {}",
            request.url
        );
    }
    assert_eq!(withheld.target_locality, Some(Locality::Local));
    assert_eq!(withheld.scorecard.target_url, local);
    let dns = row(&withheld, "dns-aid");
    assert_eq!(dns.status, ScorecardStatus::NA);
    assert_eq!(dns.na_reason, None);
    assert_eq!(dns.evidence, EXTERNAL_DNS_WITHHELD);
    assert_eq!(row(&withheld, "robots").status, ScorecardStatus::Pass);

    let (overridden, transport) = run(true);
    assert_eq!(row(&overridden, "dns-aid").status, ScorecardStatus::Pass);
    // The opposite, so the assertion above is a fact about the gate rather
    // than about a handler that never probes: with the flag the row's
    // handler runs and issues its request. Where that request goes with the
    // real handler is `web_audit_handlers.rs`'s to pin.
    assert!(
        transport
            .requests()
            .iter()
            .any(|r| r.url.ends_with("__probe/dns-aid")),
        "the override must let the DNS handler run"
    );
    assert_eq!(
        raw(&overridden, "dns-aid", "external_dns"),
        Some(&json!(true))
    );

    let public = complete(run_web_audit(input(
        Arc::new(MockTransport::new(vec![], html(200))),
        stand_in_handlers(),
    )));
    assert_eq!(row(&public, "dns-aid").status, ScorecardStatus::Pass);
}

// Deadline scenarios: a target that answers the root and discovery, then
// never finishes another request.

enum Stall {
    /// Hold the request for its whole timeout, then report the timeout.
    HonorTimeout,
    /// Never return; the pool must abandon the worker.
    Forever,
    /// Answer at once and trickle body bytes until the timeout.
    Trickle,
}

struct StallTransport {
    mode: Stall,
    seen: Mutex<Vec<(String, Duration)>>,
}

struct TrickleBody {
    until: Instant,
}

impl Read for TrickleBody {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let now = Instant::now();
        if now >= self.until || buf.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "body read timed out",
            ));
        }
        thread::sleep(Duration::from_millis(40).min(self.until - now));
        buf[0] = b'x';
        Ok(1)
    }
}

impl Transport for StallTransport {
    fn send(&self, request: &Request) -> Result<Response, TransportError> {
        self.seen
            .lock()
            .unwrap()
            .push((request.url.clone(), request.timeout));
        let path = url::Url::parse(&request.url).unwrap().path().to_string();
        if path == "/" {
            return Ok(Response {
                status: 200,
                headers: Headers::from_pairs([("content-type", "text/html")]),
                body: Box::new(Cursor::new(HTML.as_bytes().to_vec())),
            });
        }
        if path.starts_with("/.well-known/")
            || matches!(path.as_str(), "/mcp" | "/sse" | "/message")
        {
            return Ok(Response {
                status: 404,
                headers: Headers::new(),
                body: Box::new(Cursor::new(Vec::new())),
            });
        }
        match self.mode {
            Stall::HonorTimeout => {
                thread::sleep(request.timeout);
                Err(TransportError::Timeout)
            }
            Stall::Forever => loop {
                thread::sleep(Duration::from_secs(3600));
            },
            Stall::Trickle => Ok(Response {
                status: 200,
                headers: Headers::from_pairs([("content-type", "text/html")]),
                body: Box::new(TrickleBody {
                    until: Instant::now() + request.timeout,
                }),
            }),
        }
    }
}

fn run_stalled(mode: Stall) -> (Box<RunReport>, Duration, Vec<(String, Duration)>) {
    let transport = Arc::new(StallTransport {
        mode,
        seen: Mutex::new(Vec::new()),
    });
    let mut input = input(transport.clone(), stand_in_handlers());
    input.per_check_timeout = Duration::from_millis(200);
    input.per_audit_deadline = Duration::from_millis(700);
    let started = Instant::now();
    let report = complete(run_web_audit(input));
    let elapsed = started.elapsed();
    let seen = transport.seen.lock().unwrap().clone();
    (report, elapsed, seen)
}

fn deadline_skips(report: &RunReport) -> usize {
    report
        .results
        .iter()
        .filter(|r| {
            r.status == ScorecardStatus::Skip
                && r.evidence == "skipped: per-audit deadline exceeded"
        })
        .count()
}

#[test]
fn a_tarpit_that_honors_timeouts_ends_inside_the_deadline_with_shortened_late_requests() {
    let (report, elapsed, seen) = run_stalled(Stall::HonorTimeout);
    assert!(elapsed < Duration::from_millis(700 + 600), "{elapsed:?}");
    assert_eq!(report.results.len(), 65);
    assert!(!report.complete);
    assert!(deadline_skips(&report) > 0);
    let probes: Vec<&(String, Duration)> = seen
        .iter()
        .filter(|(u, _)| u.contains("__probe/"))
        .collect();
    assert!(probes.iter().all(|(_, t)| *t <= Duration::from_millis(200)));
    assert!(
        probes.iter().any(|(_, t)| *t < Duration::from_millis(200)),
        "no request carried a shortened timeout: {:?}",
        probes.iter().map(|(_, t)| t).collect::<Vec<_>>()
    );
    assert_eq!(row(&report, "robots").status, ScorecardStatus::Error);
}

#[test]
fn a_tarpit_that_never_returns_is_abandoned_at_the_deadline() {
    let (report, elapsed, _) = run_stalled(Stall::Forever);
    assert!(elapsed < Duration::from_millis(700 + 600), "{elapsed:?}");
    assert_eq!(report.results.len(), 65);
    assert!(!report.complete);
    // Nothing past the root and discovery ever returned.
    assert!(deadline_skips(&report) >= 9, "{}", deadline_skips(&report));
    assert_eq!(row(&report, "robots").status, ScorecardStatus::Skip);
    assert_eq!(
        row(&report, "robots").raw_evidence[0].get("why"),
        Some(&json!(["per-audit deadline exceeded"]))
    );
}

#[test]
fn a_slow_loris_body_ends_inside_the_deadline() {
    let (report, elapsed, seen) = run_stalled(Stall::Trickle);
    assert!(elapsed < Duration::from_millis(700 + 600), "{elapsed:?}");
    assert_eq!(report.results.len(), 65);
    assert!(!report.complete);
    assert!(deadline_skips(&report) > 0);
    let probes: Vec<&(String, Duration)> = seen
        .iter()
        .filter(|(u, _)| u.contains("__probe/"))
        .collect();
    assert!(probes.iter().any(|(_, t)| *t < Duration::from_millis(200)));
    assert_eq!(row(&report, "robots").status, ScorecardStatus::Error);
}

#[test]
fn an_unparseable_target_is_reported_not_panicked() {
    let transport = healthy_transport();
    let mut input = input(transport, stand_in_handlers());
    input.url = "not a url".to_string();
    match run_web_audit(input) {
        RunOutcome::Unreachable { reason, discovery } => {
            assert_eq!(reason, "blocked: unparseable url: not a url");
            assert!(discovery.is_empty());
        }
        RunOutcome::Complete(_) => panic!("scored an unparseable url"),
    }
}
