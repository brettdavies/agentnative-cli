//! The MCP handler's wire shapes and gates, asserted against the mock
//! transport: what each op sends, when a session rides along, the one
//! conditional re-ask, the modern-lane gate, and the shared body cap.
//! Verdict parity with the site is the corpus replay's job.

mod common;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use agentnative::web_audit::engine::mcp_wire::{MODERN_PROTOCOL_VERSION, modern_probe_body};
use agentnative::web_audit::engine::{
    FetchHandle, HandlerContext, McpLaneEvidence, McpModernLane, ProbeOutcome, ProbeStatus,
};
use agentnative::web_audit::handlers::mcp::run_mcp;
use agentnative::web_audit::locality::Locality;
use agentnative::web_audit::mock::{MockResponse, MockRule, MockTransport};
use agentnative::web_audit::registry::{CHECKS, HandlerBinding, WebCheck, check_by_id};
use agentnative::web_audit::transport::{Request, Transport};
use common::corpus::PublicResolver;
use serde_json::{Value, json};

const BASE: &str = "https://example.com/";
const MCP: &str = "https://example.com/mcp";

fn check(id: &str) -> &'static WebCheck {
    check_by_id(id).unwrap_or_else(|| panic!("no registry check {id}"))
}

fn json_ok(body: Value) -> MockResponse {
    MockResponse::ok(
        200,
        &[("content-type", "application/json")],
        body.to_string().as_bytes(),
    )
}

fn rpc_error(code: i64) -> MockResponse {
    json_ok(json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": code, "message": "nope" } }))
}

fn context(transport: Arc<dyn Transport + Send + Sync>) -> HandlerContext {
    HandlerContext {
        base: BASE.to_string(),
        host: "example.com".to_string(),
        mcp_endpoint: Some(MCP.to_string()),
        protocol_version: "2025-06-18",
        default_timeout: Duration::from_secs(2),
        root: None,
        scoped_dirs: Arc::new(Vec::new()),
        retained_bodies: Arc::new(HashMap::new()),
        fetch: FetchHandle::new(transport, Arc::new(PublicResolver), "test-ua"),
        mcp_session_id: None,
        mcp_lanes: McpLaneEvidence::default(),
        deadline: Instant::now() + Duration::from_secs(20),
        external_dns: false,
        target_locality: Some(Locality::Public),
    }
}

fn header<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request
        .headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn body_of(request: &Request) -> String {
    String::from_utf8_lossy(request.body.as_deref().unwrap_or(&[])).into_owned()
}

fn body_json(request: &Request) -> Value {
    serde_json::from_str(&body_of(request)).unwrap_or(Value::Null)
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

/// Run one check against a transport that answers everything the same way.
fn probe(id: &str, answer: MockResponse, session: Option<&str>) -> (ProbeOutcome, Vec<Request>) {
    let transport = Arc::new(MockTransport::new(vec![], answer));
    let mut ctx = context(transport.clone());
    ctx.mcp_session_id = session.map(str::to_string);
    let outcome = run_mcp(check(id), &ctx);
    (outcome, transport.requests())
}

#[test]
fn every_mcp_row_posts_one_json_rpc_request_to_the_endpoint() {
    for c in CHECKS.iter().filter(|c| c.handler == HandlerBinding::Mcp) {
        let (_, requests) = probe(c.id, rpc_error(-32601), None);
        assert_eq!(requests.len(), 1, "{}", c.id);
        assert_eq!(requests[0].method, "POST", "{}", c.id);
        assert_eq!(requests[0].url, MCP, "{}", c.id);
        assert!(header(&requests[0], "mcp-session-id").is_none(), "{}", c.id);
    }
}

#[test]
fn legacy_era_rows_send_the_pinned_initialize_and_attach_the_session_after_it() {
    let (_, init) = probe("mcp-initialize", json_ok(json!({})), Some("sess-1"));
    assert_eq!(
        body_of(&init[0]),
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"agent-web-audit","version":"1.0"}}}"#
    );
    assert_eq!(header(&init[0], "content-type"), Some("application/json"));
    assert_eq!(
        header(&init[0], "accept"),
        Some("application/json, text/event-stream")
    );
    assert!(
        header(&init[0], "mcp-session-id").is_none(),
        "initialize never carries a session"
    );

    for id in ["mcp-tools-list", "mcp-resources-list", "mcp-unknown-method"] {
        let (_, requests) = probe(id, json_ok(json!({})), Some("sess-1"));
        assert_eq!(
            header(&requests[0], "mcp-session-id"),
            Some("sess-1"),
            "{id}"
        );
    }
    let (_, tools) = probe("mcp-tools-list", json_ok(json!({})), None);
    assert_eq!(
        body_of(&tools[0]),
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#
    );
    let (_, unknown) = probe("mcp-unknown-method", json_ok(json!({})), None);
    assert_eq!(
        body_of(&unknown[0]),
        r#"{"jsonrpc":"2.0","id":1,"method":"nonexistent/method","params":{}}"#
    );
}

#[test]
fn modern_rows_are_header_routed_sessionless_and_carry_the_meta_envelope() {
    let (_, discover) = probe("mcp-server-discover", json_ok(json!({})), Some("sess-1"));
    assert_eq!(header(&discover[0], "mcp-method"), Some("server/discover"));
    assert_eq!(
        header(&discover[0], "mcp-protocol-version"),
        Some(MODERN_PROTOCOL_VERSION)
    );
    assert!(header(&discover[0], "mcp-name").is_none());
    assert!(header(&discover[0], "mcp-session-id").is_none());
    assert_eq!(body_of(&discover[0]), modern_probe_body("server/discover"));

    let (_, tools) = probe("mcp-modern-tools-list", json_ok(json!({})), Some("sess-1"));
    assert_eq!(header(&tools[0], "mcp-method"), Some("tools/list"));
    assert!(header(&tools[0], "mcp-session-id").is_none());
    let meta = &body_json(&tools[0])["params"]["_meta"];
    assert_eq!(
        meta["io.modelcontextprotocol/protocolVersion"],
        json!(MODERN_PROTOCOL_VERSION)
    );
    assert_eq!(
        meta["io.modelcontextprotocol/clientCapabilities"],
        json!({})
    );

    let (_, clientcaps) = probe("mcp-modern-clientcaps", json_ok(json!({})), None);
    let meta = &body_json(&clientcaps[0])["params"]["_meta"];
    assert!(
        meta.get("io.modelcontextprotocol/clientCapabilities")
            .is_none()
    );
    assert!(meta.get("io.modelcontextprotocol/clientInfo").is_some());

    let (_, mismatch) = probe("mcp-modern-header-mismatch", json_ok(json!({})), None);
    assert_eq!(header(&mismatch[0], "mcp-method"), Some("resources/list"));
    assert_eq!(body_json(&mismatch[0])["method"], json!("tools/list"));

    let (_, version) = probe("mcp-modern-version-reject", json_ok(json!({})), None);
    assert_eq!(
        header(&version[0], "mcp-protocol-version"),
        Some("2025-03-26")
    );
    assert_eq!(
        body_json(&version[0])["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"],
        json!("2025-03-26")
    );

    let (_, miss) = probe("mcp-modern-resources-miss", json_ok(json!({})), None);
    let uri = "resource://agent-web-audit/nonexistent";
    assert_eq!(header(&miss[0], "mcp-name"), Some(uri));
    assert_eq!(header(&miss[0], "mcp-method"), Some("resources/read"));
    assert_eq!(body_json(&miss[0])["params"]["uri"], json!(uri));
}

#[test]
fn legacy_conformance_probes_are_the_site_bytes_with_no_session_attached() {
    let (_, malformed) = probe("mcp-malformed-body", json_ok(json!({})), Some("sess-1"));
    assert_eq!(body_of(&malformed[0]), "not-json{{");
    assert_eq!(
        header(&malformed[0], "content-type"),
        Some("application/json")
    );
    assert!(header(&malformed[0], "mcp-session-id").is_none());

    let (_, batch) = probe("mcp-batch-reject", json_ok(json!({})), None);
    let body = body_json(&batch[0]);
    assert_eq!(body.as_array().map(Vec::len), Some(1));
    assert!(body[0]["params"]["_meta"].is_object());

    let (_, tool) = probe("mcp-unknown-tool", json_ok(json!({})), None);
    let body = body_json(&tool[0]);
    assert_eq!(body["method"], json!("tools/call"));
    assert_eq!(body["params"]["name"], json!("not_a_real_tool"));
}

#[test]
fn negotiation_rows_vary_only_the_accept_of_the_proven_tools_list() {
    let (_, json) = probe("mcp-accept-json", json_ok(json!({})), Some("sess-1"));
    assert_eq!(header(&json[0], "accept"), Some("application/json"));
    assert_eq!(
        body_of(&json[0]),
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#
    );
    assert!(header(&json[0], "mcp-session-id").is_none());
    let (_, xml) = probe("mcp-accept-unsatisfiable", json_ok(json!({})), None);
    assert_eq!(header(&xml[0], "accept"), Some("application/xml"));
    assert_eq!(body_of(&xml[0]), body_of(&json[0]));
}

#[test]
fn a_session_refusal_re_asks_only_the_legacy_conformance_rows_with_identical_bytes() {
    let rules = vec![
        MockRule::new("POST", MCP)
            .header("Mcp-Session-Id", "sess-1")
            .response(rpc_error(-32700)),
    ];
    let transport = Arc::new(MockTransport::new(rules, rpc_error(-32000)));
    let mut ctx = context(transport.clone());
    ctx.mcp_session_id = Some("sess-1".to_string());
    let outcome = run_mcp(check("mcp-malformed-body"), &ctx);
    assert_eq!(outcome.status, ProbeStatus::Pass, "{:?}", outcome.evidence);
    let requests = transport.requests();
    assert_eq!(requests.len(), 2);
    assert!(header(&requests[0], "mcp-session-id").is_none());
    assert_eq!(header(&requests[1], "mcp-session-id"), Some("sess-1"));
    assert_eq!(body_of(&requests[0]), body_of(&requests[1]));

    // A stateless target never issues a session, so the row stays one request.
    let (outcome, requests) = probe("mcp-malformed-body", rpc_error(-32000), None);
    assert_eq!(requests.len(), 1);
    assert_eq!(outcome.status, ProbeStatus::Noncompliant);
    assert_eq!(why(&outcome), ["expected error code -32700, got -32000"]);

    // A negotiation row already read the framing of the refusal.
    let (outcome, requests) = probe("mcp-accept-json", rpc_error(-32000), Some("sess-1"));
    assert_eq!(requests.len(), 1);
    assert_eq!(outcome.status, ProbeStatus::Pass);
}

#[test]
fn the_modern_lane_gate_settles_dependent_rows_without_a_request() {
    let transport = Arc::new(MockTransport::new(vec![], json_ok(json!({}))));
    let mut ctx = context(transport.clone());
    ctx.mcp_lanes.modern = McpModernLane::Unevidenced;
    for id in [
        "mcp-modern-tools-list",
        "mcp-modern-unknown-method",
        "mcp-modern-clientcaps",
        "mcp-modern-header-mismatch",
        "mcp-modern-version-reject",
        "mcp-modern-resources-miss",
    ] {
        let outcome = run_mcp(check(id), &ctx);
        assert_eq!(outcome.status, ProbeStatus::Absent, "{id}");
        assert!(outcome.unprobed, "{id}");
        assert_eq!(
            why(&outcome),
            ["no modern lane: server/discover returned no result"],
            "{id}"
        );
    }
    assert!(transport.requests().is_empty());

    // The discriminating row is never gated on its own answer, and the
    // legacy rows are untouched.
    let discover = run_mcp(check("mcp-server-discover"), &ctx);
    assert!(!discover.unprobed);
    let tools = run_mcp(check("mcp-tools-list"), &ctx);
    assert!(!tools.unprobed);
    assert_eq!(transport.requests().len(), 2);

    for lane in [McpModernLane::Present, McpModernLane::Unknown] {
        let transport = Arc::new(MockTransport::new(vec![], json_ok(json!({}))));
        let mut ctx = context(transport.clone());
        ctx.mcp_lanes.modern = lane;
        let outcome = run_mcp(check("mcp-modern-tools-list"), &ctx);
        assert!(!outcome.unprobed, "{lane:?}");
        assert_eq!(transport.requests().len(), 1, "{lane:?}");
    }
}

#[test]
fn without_an_endpoint_every_row_is_a_reasonless_na_that_probes_nothing() {
    let transport = Arc::new(MockTransport::new(vec![], json_ok(json!({}))));
    let mut ctx = context(transport.clone());
    ctx.mcp_endpoint = None;
    for c in CHECKS.iter().filter(|c| c.handler == HandlerBinding::Mcp) {
        let outcome = run_mcp(c, &ctx);
        assert_eq!(outcome.status, ProbeStatus::Na, "{}", c.id);
        assert_eq!(outcome.na_reason, None, "{}", c.id);
        assert_eq!(why(&outcome), ["no MCP endpoint discovered"], "{}", c.id);
    }
    assert!(transport.requests().is_empty());
}

#[test]
fn an_mcp_probe_stops_reading_at_the_shared_body_cap() {
    // A stream whose first data line sits past the 64 KiB cap never
    // reaches the parser, so the row reads as a broken surface rather
    // than buffering the whole body.
    let padding = ": keep-alive\n".repeat(6000);
    let body = format!(
        "{padding}data: {}\n\n",
        json!({ "jsonrpc": "2.0", "id": 1, "result": { "tools": [] } })
    );
    assert!(padding.len() > 64 * 1024);
    let stream = MockResponse::ok(
        200,
        &[("content-type", "text/event-stream")],
        body.as_bytes(),
    );
    let (outcome, _) = probe("mcp-tools-list", stream, None);
    assert_eq!(outcome.status, ProbeStatus::Broken);
    assert_eq!(why(&outcome), ["no parseable JSON-RPC response"]);

    let short = format!(
        "data: {}\n\n",
        json!({ "jsonrpc": "2.0", "id": 1, "result": { "tools": [] } })
    );
    let stream = MockResponse::ok(
        200,
        &[("content-type", "text/event-stream")],
        short.as_bytes(),
    );
    let (outcome, _) = probe("mcp-tools-list", stream, None);
    assert_eq!(outcome.status, ProbeStatus::Pass);
}

#[test]
fn initialize_records_the_handshake_facts_the_antecedents_read() {
    let answer = MockResponse::ok(
        200,
        &[
            ("content-type", "application/json"),
            ("mcp-session-id", "abc"),
        ],
        json!({
            "jsonrpc": "2.0", "id": 1,
            "result": {
                "serverInfo": { "name": "srv", "version": "1" },
                "protocolVersion": "2025-06-18",
                "capabilities": { "tools": {}, "resources": {} }
            }
        })
        .to_string()
        .as_bytes(),
    );
    let (outcome, _) = probe("mcp-initialize", answer, None);
    assert_eq!(outcome.status, ProbeStatus::Pass);
    let ev = &outcome.evidence[0];
    assert_eq!(ev.get("session_id"), Some(&json!("abc")));
    assert_eq!(ev.get("capabilities"), Some(&json!(["tools", "resources"])));
    assert_eq!(
        ev.get("serverInfo"),
        Some(&json!({ "name": "srv", "version": "1" }))
    );
    assert_eq!(ev.get("protocolVersion"), Some(&json!("2025-06-18")));

    let challenge = MockResponse::ok(401, &[("www-authenticate", "Bearer realm=\"x\"")], b"");
    let (outcome, _) = probe("mcp-initialize", challenge, None);
    assert_eq!(outcome.status, ProbeStatus::Broken);
    assert_eq!(
        outcome.evidence[0].get("www_authenticate"),
        Some(&json!("Bearer realm=\"x\""))
    );
}
