//! The stateless handlers against the site: every scenario in the vendored
//! conformance corpus replays through the engine with the stateless
//! handler set, and each row the scenario is the subject of must match the
//! site's golden row. The registry's patterns are held to the site's
//! `RegExp` results over the fixed probe table. A handful of behaviors the
//! corpus cannot observe (which requests went out, and what never did)
//! are asserted against the mock transport directly.

#![cfg(feature = "web")]

mod common;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use agentnative::web_audit::engine::discovery_documents::{API_CATALOG_DOCUMENT, RetainedDocument};
use agentnative::web_audit::engine::{ProbeOutcome, ProbeStatus, RunOutcome};
use agentnative::web_audit::fetch::ProbeResponse;
use agentnative::web_audit::handlers::api_hygiene::derive_api_probe_url;
use agentnative::web_audit::handlers::assert::compiled;
use agentnative::web_audit::handlers::cors_preflight::run_cors_preflight;
use agentnative::web_audit::handlers::default_handlers;
use agentnative::web_audit::handlers::dns_doh::run_dns_doh;
use agentnative::web_audit::handlers::http::{run_http, run_retained_document};
use agentnative::web_audit::handlers::llms_txt_quality::run_llms_txt_quality;
use agentnative::web_audit::headers::Headers;
use agentnative::web_audit::locality::Locality;
use agentnative::web_audit::mock::{MockResponse, MockRule, MockTransport};
use agentnative::web_audit::registry::check_by_id;
use agentnative::web_audit::scorecard::NaReason;
use agentnative::web_audit::transport::{Request, Response, Transport, TransportError};
use common::corpus::{Golden, corpus_dir, describe, diff_rows, load_cases, replay};
use common::handler_context::{BASE, check, context};
use common::ledger::{ROW, UNREACHABLE, compare_with_ledger, load_ledger};
use serde::Deserialize;
use serde_json::{Value, json};

const MCP: &str = "https://example.com/mcp";

// Corpus replay.

/// Each scenario's subject rows (its `covers`) diverge from the golden
/// exactly where the divergence ledger records a `row:` entry, and an
/// unreachable outcome exactly where it records `unreachable`.
#[test]
fn every_corpus_scenario_reproduces_its_subject_rows_or_is_ledgered() {
    let handlers = Arc::new(default_handlers());
    let ledger = load_ledger();
    let mut actual: BTreeSet<(String, String)> = BTreeSet::new();
    let mut in_scope: BTreeSet<(String, String)> = BTreeSet::new();
    let mut detail: Vec<String> = Vec::new();
    let mut compared = 0usize;
    for case in load_cases() {
        in_scope.insert((case.name.clone(), UNREACHABLE.to_string()));
        let outcome = replay(&case.scenario, Arc::clone(&handlers));
        match (&case.golden, outcome) {
            (Golden::Unreachable(reason), RunOutcome::Unreachable { reason: got, .. }) => {
                compared += 1;
                if reason != &got {
                    actual.insert((case.name.clone(), UNREACHABLE.to_string()));
                }
            }
            (Golden::Unreachable(_), RunOutcome::Complete(_))
            | (Golden::Scorecard(_), RunOutcome::Unreachable { .. }) => {
                actual.insert((case.name.clone(), UNREACHABLE.to_string()));
            }
            (Golden::Scorecard(golden), RunOutcome::Complete(report)) => {
                let ids: Vec<String> = case.scenario.covers.clone();
                compared += ids.len();
                for id in &ids {
                    in_scope.insert((case.name.clone(), format!("{ROW}{id}")));
                }
                let diffs = diff_rows(&report, golden, &ids);
                for d in &diffs {
                    let element = (case.name.clone(), format!("{ROW}{}", d.id));
                    if !ledger.contains_key(&element) {
                        detail.push(format!(
                            "{}:\n{}",
                            case.name,
                            describe(std::slice::from_ref(d))
                        ));
                    }
                    actual.insert(element);
                }
            }
        }
    }
    assert!(compared > 100, "compared only {compared} rows");
    let problems = compare_with_ledger(&actual, &ledger, |e| in_scope.contains(e));
    assert!(
        problems.is_empty(),
        "{} subject-row ledger mismatch(es):\n{}\n{}",
        problems.len(),
        problems.join("\n"),
        detail.join("\n")
    );
}

// Regex parity.

#[derive(Deserialize)]
struct ParityPattern {
    check_id: String,
    field: String,
    pattern: String,
    flags: String,
    results: Vec<bool>,
}

#[derive(Deserialize)]
struct ParityFixture {
    probes: Vec<String>,
    patterns: Vec<ParityPattern>,
}

/// Divergences the translation cannot close, keyed by check id and field
/// and valued by the probes affected. Rust's case-insensitive matching
/// uses Unicode simple folding where a JavaScript `RegExp` without the `u`
/// flag folds ASCII only; the fixture's probe table measures whether any
/// registry pattern reaches that gap.
fn documented_residuals() -> BTreeMap<(String, String), Vec<String>> {
    BTreeMap::new()
}

#[test]
fn every_registry_pattern_agrees_with_the_site_over_the_probe_table() {
    let raw = fs::read_to_string(corpus_dir().join("regex-parity.json")).unwrap();
    let fixture: ParityFixture = serde_json::from_str(&raw).unwrap();
    assert_eq!(fixture.probes.len(), 138);
    let site: BTreeSet<(String, String)> = fixture
        .patterns
        .iter()
        .map(|p| (p.check_id.clone(), p.field.clone()))
        .collect();
    let ours: BTreeSet<(String, String)> = agentnative::web_audit::registry::CHECKS
        .iter()
        .flat_map(|c| {
            c.patterns
                .iter()
                .map(|p| (c.id.to_string(), p.field.to_string()))
        })
        .collect();
    assert_eq!(
        site, ours,
        "the fixture covers exactly the registry's compiled patterns"
    );
    assert_eq!(site.len(), 31, "pattern count (bump deliberately)");
    let residuals = documented_residuals();
    let mut mismatches: Vec<String> = Vec::new();
    for pattern in &fixture.patterns {
        let check = check_by_id(&pattern.check_id)
            .unwrap_or_else(|| panic!("fixture names unknown check {}", pattern.check_id));
        let regex = compiled(check, &pattern.field).unwrap_or_else(|| {
            panic!(
                "no compiled pattern for {}.{}",
                pattern.check_id, pattern.field
            )
        });
        assert_eq!(pattern.results.len(), fixture.probes.len());
        let allowed = residuals
            .get(&(pattern.check_id.clone(), pattern.field.clone()))
            .cloned()
            .unwrap_or_default();
        for (probe, expected) in fixture.probes.iter().zip(&pattern.results) {
            let actual = regex.is_match(probe);
            if actual != *expected && !allowed.contains(probe) {
                mismatches.push(format!(
                    "{}.{} /{}/{}: probe {probe:?} site={expected} rust={actual}",
                    pattern.check_id, pattern.field, pattern.pattern, pattern.flags
                ));
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} pattern/probe pair(s) diverge:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

// Behaviors the corpus cannot observe.

fn html(status: u16, body: &str) -> MockResponse {
    MockResponse::ok(
        status,
        &[("content-type", "text/html; charset=utf-8")],
        body.as_bytes(),
    )
}

fn json_ok(body: Value) -> MockResponse {
    MockResponse::ok(
        200,
        &[("content-type", "application/json")],
        body.to_string().as_bytes(),
    )
}

fn root(body: &str) -> Arc<ProbeResponse> {
    Arc::new(ProbeResponse {
        status: Some(200),
        headers: Headers::from_pairs([("content-type", "text/html; charset=utf-8")]),
        body: body.to_string(),
        error: None,
        elapsed_ms: 0,
        truncated: false,
        refused: None,
    })
}

fn header<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request
        .headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn why(outcome: &ProbeOutcome) -> Vec<String> {
    outcome.evidence[0]
        .get("why")
        .and_then(Value::as_array)
        .map(|w| {
            w.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn retained_document_scores_the_kept_api_catalog_without_a_request() {
    let check = check_by_id("api-catalog").unwrap();
    let transport = Arc::new(MockTransport::new(
        vec![],
        MockResponse::error("unexpected"),
    ));
    let mut ctx = context(transport.clone());
    let none = run_retained_document(check, &ctx);
    assert_eq!(none.status, ProbeStatus::Absent);
    assert_eq!(why(&none), ["discovery kept no api-catalog document"]);

    let kept = |status: u16, body: &str| RetainedDocument {
        url: "https://example.com/.well-known/api-catalog".to_string(),
        response: ProbeResponse {
            status: Some(status),
            headers: Headers::from_pairs([("content-type", "application/linkset+json")]),
            body: body.to_string(),
            error: None,
            elapsed_ms: 0,
            truncated: false,
            refused: None,
        },
        shape: None,
    };
    ctx.retained_documents = Arc::new(HashMap::from([(
        API_CATALOG_DOCUMENT,
        kept(200, r#"{"linkset":[]}"#),
    )]));
    let pass = run_retained_document(check, &ctx);
    assert_eq!(pass.status, ProbeStatus::Pass);
    assert_eq!(
        pass.evidence[0].get("retained"),
        Some(&Value::from("api-catalog"))
    );
    ctx.retained_documents = Arc::new(HashMap::from([(API_CATALOG_DOCUMENT, kept(200, "{}"))]));
    assert_eq!(
        run_retained_document(check, &ctx).status,
        ProbeStatus::Broken
    );
    ctx.retained_documents = Arc::new(HashMap::from([(API_CATALOG_DOCUMENT, kept(404, ""))]));
    assert_eq!(
        run_retained_document(check, &ctx).status,
        ProbeStatus::Absent
    );
    assert!(transport.requests().is_empty());
}

#[test]
fn http_reuses_the_root_fetch_only_for_a_plain_get_of_the_base() {
    let transport = Arc::new(MockTransport::new(
        vec![],
        MockResponse::ok(200, &[("content-type", "text/markdown")], b"# md"),
    ));
    let mut ctx = context(transport.clone());
    ctx.root = Some(root(
        "<html><head><meta name=\"description\" content=\"A site\"></head></html>",
    ));
    let plain = run_http(check("root-meta-description"), &ctx);
    assert_eq!(plain.status, ProbeStatus::Pass, "{:?}", plain.evidence);
    assert!(transport.requests().is_empty(), "the root row re-fetched /");

    let negotiated = run_http(check("accept-markdown"), &ctx);
    assert_eq!(negotiated.status, ProbeStatus::Pass);
    let requests = transport.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].url, BASE);
    assert_eq!(header(&requests[0], "accept"), Some("text/markdown"));
}

#[test]
fn http_timeouts_are_broken_only_under_an_explicit_hang_budget() {
    let transport = Arc::new(MockTransport::new(
        vec![],
        MockResponse::error("TimeoutError: deadline exceeded"),
    ));
    let mut ctx = context(transport);
    ctx.mcp_endpoint = Some(MCP.to_string());
    let hang = run_http(check("mcp-get-fast-fail"), &ctx);
    assert_eq!(hang.status, ProbeStatus::Broken);
    let plain = run_http(check("llms-txt"), &ctx);
    assert_eq!(plain.status, ProbeStatus::Error);
    assert_eq!(
        why(&plain),
        ["request failed: TimeoutError: deadline exceeded"]
    );
}

#[test]
fn http_substitutes_the_endpoint_and_settles_na_without_one() {
    let transport = Arc::new(MockTransport::new(vec![], MockResponse::ok(405, &[], b"")));
    let mut ctx = context(transport.clone());
    let without = run_http(check("mcp-get-fast-fail"), &ctx);
    assert_eq!(without.status, ProbeStatus::Na);
    assert_eq!(without.na_reason, None);
    assert_eq!(why(&without), ["no resolvable probe URL"]);
    assert!(transport.requests().is_empty());

    ctx.mcp_endpoint = Some(MCP.to_string());
    let with = run_http(check("mcp-get-fast-fail"), &ctx);
    assert_eq!(with.status, ProbeStatus::Pass);
    assert_eq!(transport.requests()[0].url, MCP);
    assert_eq!(transport.requests()[0].method, "GET");
}

#[test]
fn mcp_get_fast_fail_asks_for_an_event_stream_and_breaks_on_a_document() {
    for (status, content_type, want) in [
        (200, "text/event-stream", ProbeStatus::Pass),
        (200, "text/html; charset=utf-8", ProbeStatus::Broken),
        (200, "text/markdown; charset=utf-8", ProbeStatus::Broken),
        (405, "text/plain", ProbeStatus::Pass),
    ] {
        let transport = Arc::new(MockTransport::new(
            vec![],
            MockResponse::ok(status, &[("content-type", content_type)], b""),
        ));
        let mut ctx = context(transport.clone());
        ctx.mcp_endpoint = Some(MCP.to_string());
        let outcome = run_http(check("mcp-get-fast-fail"), &ctx);
        assert_eq!(
            outcome.status, want,
            "{status} {content_type}: {:?}",
            outcome.evidence
        );
        assert_eq!(
            header(&transport.requests()[0], "accept"),
            Some("text/event-stream")
        );
    }
}

#[test]
fn cors_issues_exactly_the_preflight_and_the_origin_bearing_post_with_the_session() {
    let transport = Arc::new(MockTransport::new(vec![], html(200, "")));
    let mut ctx = context(transport.clone());
    ctx.mcp_endpoint = Some(MCP.to_string());
    ctx.mcp_session_id = Some("sess-1".to_string());
    let outcome = run_cors_preflight(check("mcp-cors-preflight"), &ctx);
    assert_eq!(outcome.status, ProbeStatus::Na);
    assert_eq!(outcome.na_reason, Some(NaReason::PostureConsistent));
    assert_eq!(outcome.evidence[0].get("probe"), Some(&json!("preflight")));

    let requests = transport.requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    let preflight = requests
        .iter()
        .find(|r| r.method == "OPTIONS")
        .expect("OPTIONS");
    let post = requests.iter().find(|r| r.method == "POST").expect("POST");
    assert_eq!(preflight.url, MCP);
    assert!(header(preflight, "origin").is_some());
    assert!(header(preflight, "access-control-request-method").is_some());
    assert!(header(preflight, "access-control-request-headers").is_some());
    assert_eq!(header(post, "origin"), header(preflight, "origin"));
    assert_eq!(header(post, "mcp-session-id"), Some("sess-1"));
    assert!(String::from_utf8_lossy(post.body.as_deref().unwrap_or(&[])).contains("tools/list"));

    let actual = run_cors_preflight(check("mcp-cors-actual"), &ctx);
    assert_eq!(actual.evidence[0].get("probe"), Some(&json!("post")));
    assert_eq!(actual.na_reason, Some(NaReason::PostureConsistent));
}

#[test]
fn cors_without_an_endpoint_is_a_reasonless_na_that_probes_nothing() {
    let transport = Arc::new(MockTransport::new(vec![], html(200, "")));
    let ctx = context(transport.clone());
    let outcome = run_cors_preflight(check("mcp-cors-preflight"), &ctx);
    assert_eq!(outcome.status, ProbeStatus::Na);
    assert_eq!(outcome.na_reason, None);
    assert_eq!(why(&outcome), ["no endpoint to preflight"]);
    assert!(transport.requests().is_empty());
}

const RETRY_SHAPED: [(u16, &str); 2] = [(408, "Request Timeout"), (429, "Too Many Requests")];

fn busy(status: u16, headers: &[(&str, &str)]) -> MockResponse {
    let mut all = vec![("retry-after", "30")];
    all.extend_from_slice(headers);
    MockResponse::ok(status, &all, b"slow down")
}

const ACAO: [(&str, &str); 1] = [("access-control-allow-origin", "*")];

fn preflight_acao(status: u16) -> MockResponse {
    MockResponse::ok(status, &ACAO, b"")
}

fn bare() -> MockResponse {
    MockResponse::ok(200, &[], b"")
}

/// Classify one surface of the pair from these two answers.
fn classify(surface: &str, preflight: MockResponse, post: MockResponse) -> ProbeOutcome {
    let transport = Arc::new(MockTransport::new(
        vec![
            MockRule::new("OPTIONS", MCP).response(preflight),
            MockRule::new("POST", MCP).response(post),
        ],
        MockResponse::error("unmatched"),
    ));
    let mut ctx = context(transport);
    ctx.mcp_endpoint = Some(MCP.to_string());
    let id = if surface == "preflight" {
        "mcp-cors-preflight"
    } else {
        "mcp-cors-actual"
    };
    run_cors_preflight(check(id), &ctx)
}

#[test]
fn http_reads_a_retry_shaped_answer_on_the_mcp_endpoint_as_an_operational_error() {
    for (status, reason) in RETRY_SHAPED {
        let transport = Arc::new(MockTransport::new(vec![], busy(status, &[])));
        let mut ctx = context(transport);
        ctx.mcp_endpoint = Some(MCP.to_string());
        let outcome = run_http(check("mcp-get-fast-fail"), &ctx);
        assert_eq!(
            format!("{status}:{:?}", outcome.status),
            format!("{status}:{:?}", ProbeStatus::Error)
        );
        assert_eq!(
            why(&outcome),
            [format!(
                "the target answered HTTP {status} {reason}; not scored"
            )]
        );
        assert_eq!(outcome.evidence[0].get("ok"), Some(&json!(false)));

        // A document off the MCP endpoint keeps its own classification.
        let off = run_http(check("llms-txt"), &ctx);
        assert_eq!(
            format!("{status}:{:?}", off.status),
            format!("{status}:{:?}", ProbeStatus::Broken)
        );
    }
}

#[test]
fn cors_leaves_a_row_unscored_when_its_own_probe_is_retry_shaped_whatever_headers_ride_it() {
    for (status, reason) in RETRY_SHAPED {
        let line = [format!(
            "the target answered HTTP {status} {reason}; not scored"
        )];
        for preflight in [busy(status, &[]), busy(status, &ACAO)] {
            let pre = classify("preflight", preflight, MockResponse::ok(200, &ACAO, b""));
            assert_eq!(
                format!("{status}:{:?}", pre.status),
                format!("{status}:{:?}", ProbeStatus::Error)
            );
            assert_eq!(why(&pre), line);
        }
        for post in [busy(status, &[]), busy(status, &ACAO)] {
            let act = classify("actual", preflight_acao(204), post);
            assert_eq!(
                format!("{status}:{:?}", act.status),
                format!("{status}:{:?}", ProbeStatus::Error)
            );
            assert_eq!(why(&act), line);
        }
    }
}

#[test]
fn cors_a_retry_shaped_sibling_cannot_settle_a_row_without_its_own_allow_origin() {
    for (status, reason) in RETRY_SHAPED {
        assert_eq!(
            classify("preflight", preflight_acao(204), busy(status, &[])).status,
            ProbeStatus::Pass
        );
        assert_eq!(
            classify("preflight", preflight_acao(500), busy(status, &[])).status,
            ProbeStatus::Broken
        );
        assert_eq!(
            classify(
                "actual",
                busy(status, &[]),
                MockResponse::ok(200, &ACAO, b"")
            )
            .status,
            ProbeStatus::Pass
        );

        // A busy answer's headers describe the load, not the posture.
        for sibling in [busy(status, &[]), busy(status, &ACAO)] {
            let pre = classify("preflight", bare(), sibling.clone());
            assert_eq!(
                format!("{status}:{:?}", pre.status),
                format!("{status}:{:?}", ProbeStatus::Error)
            );
            assert_eq!(pre.na_reason, None);
            assert_eq!(
                why(&pre),
                [format!(
                    "the POST probe answered HTTP {status} {reason}, so the no-CORS posture cannot be confirmed"
                )]
            );
            let act = classify("actual", sibling, bare());
            assert_eq!(
                format!("{status}:{:?}", act.status),
                format!("{status}:{:?}", ProbeStatus::Error)
            );
            assert_eq!(act.na_reason, None);
            assert_eq!(
                why(&act),
                [format!(
                    "the preflight probe answered HTTP {status} {reason}, so the no-CORS posture cannot be confirmed"
                )]
            );
        }
    }
}

#[test]
fn dns_queries_encode_the_record_name_and_fall_back_per_resolver() {
    let google = "https://dns.google/resolve?name=_index._agents.example.com&type=SVCB";
    let transport = Arc::new(MockTransport::new(
        vec![
            MockRule::new(
                "GET",
                "https://cloudflare-dns.com/dns-query?name=_index._agents.example.com&type=SVCB",
            )
            .response(MockResponse::error("TypeError: connection refused")),
            MockRule::new("GET", google).response(json_ok(json!({ "Status": 0, "Answer": [{}] }))),
        ],
        json_ok(json!({ "Status": 3 })),
    ));
    let ctx = context(transport.clone());
    let outcome = run_dns_doh(check("dns-aid"), &ctx);
    assert_eq!(outcome.status, ProbeStatus::Pass, "{:?}", outcome.evidence);
    assert_eq!(
        outcome.evidence[0],
        json!({
            "name": "_index._agents.example.com",
            "resolver": "https://dns.google/resolve",
            "dns_status": 0,
            "answers": 1
        })
        .as_object()
        .cloned()
        .unwrap()
    );
    let requests = transport.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].url, google);
    assert_eq!(header(&requests[0], "accept"), Some("application/dns-json"));
}

#[test]
fn nested_probes_never_cross_the_target_class() {
    let transport = Arc::new(MockTransport::new(vec![], html(200, "ok")));
    let mut local = context(transport.clone());
    local.base = "http://127.0.0.1:8080/".to_string();
    local.host = "127.0.0.1".to_string();
    local.target_locality = Some(Locality::Local);
    local.retained_bodies = Arc::new(HashMap::from([(
        "llms-txt".to_string(),
        "# X\n- [Docs](https://docs.example.com/guide)\n".to_string(),
    )]));
    let outcome = run_llms_txt_quality(check("llms-txt-links"), &local);
    assert_eq!(outcome.status, ProbeStatus::Absent);
    assert_eq!(
        outcome.evidence[0].get("blocked"),
        Some(&json!(
            "blocked: docs.example.com is a public host outside the local target"
        ))
    );
    assert!(transport.requests().is_empty(), "a local run egressed");

    let mut public = context(transport.clone());
    public.retained_bodies = Arc::new(HashMap::from([(
        "llms-txt".to_string(),
        "# X\n- [Admin](http://10.0.0.5/admin)\n".to_string(),
    )]));
    let outcome = run_llms_txt_quality(check("llms-txt-links"), &public);
    assert_eq!(
        outcome.evidence[0].get("blocked"),
        Some(&json!(
            "blocked: ipv4 10.0.0.5 is in blocked range 10.0.0.0/8"
        ))
    );
    assert!(
        transport.requests().is_empty(),
        "a public run probed a private host"
    );
}

/// A transport that takes a fixed time to answer.
struct Slow {
    inner: MockTransport,
    delay: Duration,
}

impl Transport for Slow {
    fn send(&self, request: &Request) -> Result<Response, TransportError> {
        thread::sleep(self.delay);
        self.inner.send(request)
    }
}

#[test]
fn a_nested_probe_budget_that_runs_out_marks_the_row_incomplete() {
    let transport = Arc::new(Slow {
        inner: MockTransport::new(vec![], html(200, "ok")),
        delay: Duration::from_millis(30),
    });
    let mut ctx = context(transport);
    ctx.default_timeout = Duration::from_millis(40);
    ctx.retained_bodies = Arc::new(HashMap::from([(
        "llms-txt".to_string(),
        "- [a](/a)\n- [b](/b)\n- [c](/c)\n".to_string(),
    )]));
    let outcome = run_llms_txt_quality(check("llms-txt-links"), &ctx);
    assert!(outcome.incomplete, "{:?}", outcome.evidence);
    assert_eq!(outcome.status, ProbeStatus::Error);
    let last = outcome.evidence.last().unwrap();
    assert_eq!(
        last.get("why"),
        Some(&json!(["nested-probe budget exhausted"]))
    );
    assert!(outcome.evidence.len() < 4);
}

#[test]
fn the_api_probe_url_prefers_a_documented_client_error_then_any_safe_get() {
    let ctx = context(Arc::new(MockTransport::new(vec![], html(404, ""))));
    let spec = json!({
        "paths": {
            "/users": { "post": { "responses": { "404": {} } }, "get": { "responses": { "200": {} } } },
            "/users/{id}": { "get": { "responses": { "200": {}, "404": {} } } }
        }
    })
    .to_string();
    assert_eq!(
        derive_api_probe_url(&spec, &ctx),
        (
            "https://example.com/users/anc-web-audit-no-such".to_string(),
            "openapi-4xx"
        )
    );
    let no_4xx = json!({ "paths": { "/things": { "get": {} } } }).to_string();
    assert_eq!(
        derive_api_probe_url(&no_4xx, &ctx),
        ("https://example.com/things".to_string(), "openapi-get")
    );
    let fallback = (
        "https://example.com/anc-web-audit-no-such-api".to_string(),
        "fallback",
    );
    assert_eq!(derive_api_probe_url("", &ctx), fallback);
    assert_eq!(derive_api_probe_url("not json", &ctx), fallback);
    assert_eq!(derive_api_probe_url("[1,2]", &ctx), fallback);
    let no_safe = json!({ "paths": { "/x": { "delete": {} } } }).to_string();
    assert_eq!(derive_api_probe_url(&no_safe, &ctx), fallback);
}
