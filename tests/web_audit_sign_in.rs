//! An MCP endpoint that requires sign-in, scored by what an agent at the
//! audit's vantage can verify: a 401 its own RFC 9728 metadata backs makes
//! the endpoint present, the rows that need a session read `n_a`
//! `auth-required`, the three sign-in rows score what the 401 and the
//! metadata say, and the global score's maximum counts one MCP access
//! design. The corpus replay pins every row; these name the behaviors.

#![cfg(feature = "web")]

mod common;

use std::sync::Arc;
use std::time::Duration;

use agentnative::web_audit::engine::{FetchHandle, RunInput, RunOutcome, run_web_audit};
use agentnative::web_audit::handlers::default_handlers;
use agentnative::web_audit::mock::{MockResponse, MockRule, MockTransport};
use agentnative::web_audit::registry::AUDIT_USER_AGENT;
use agentnative::web_audit::scorecard::{NaReason, ResultRow, ScorecardStatus, WebScorecard};
use common::corpus::{Case, load_cases, replay};

fn scorecard_of(name: &str) -> WebScorecard {
    let cases: Vec<Case> = load_cases();
    let case = cases
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no corpus scenario {name}"));
    match replay(&case.scenario, Arc::new(default_handlers())) {
        RunOutcome::Complete(report) => report.scorecard,
        RunOutcome::Unreachable { reason, .. } => panic!("{name}: unreachable: {reason}"),
    }
}

fn row<'a>(card: &'a WebScorecard, id: &str) -> &'a ResultRow {
    card.results
        .iter()
        .find(|r| r.id == id)
        .unwrap_or_else(|| panic!("no row {id}"))
}

fn verdict(card: &WebScorecard, id: &str) -> (ScorecardStatus, Option<NaReason>, String) {
    let r = row(card, id);
    (
        r.status,
        r.na_reason,
        r.evidence.clone().unwrap_or_default(),
    )
}

#[test]
fn a_metadata_backed_401_makes_the_endpoint_present_and_its_session_rows_auth_required() {
    let card = scorecard_of("auth-declared-endpoint");
    for id in ["mcp-initialize", "mcp-tools-list", "mcp-server-discover"] {
        let (status, reason, _) = verdict(&card, id);
        assert_eq!(
            (status, reason),
            (ScorecardStatus::NA, Some(NaReason::AuthRequired)),
            "{id}"
        );
    }
    assert_eq!(
        verdict(&card, "mcp-auth-challenge").0,
        ScorecardStatus::Pass
    );
    assert_eq!(verdict(&card, "mcp-auth-servers").0, ScorecardStatus::Pass);
    let (status, _, evidence) = verdict(&card, "mcp-auth-enforced");
    assert_eq!(
        (status, evidence.as_str()),
        (ScorecardStatus::Pass, "refused with 401")
    );
}

#[test]
fn the_sign_in_rows_say_there_is_no_endpoint_when_discovery_found_none() {
    let card = scorecard_of("frontmatter-absent");
    for id in [
        "mcp-auth-challenge",
        "mcp-auth-servers",
        "mcp-auth-enforced",
    ] {
        assert_eq!(
            verdict(&card, id),
            (
                ScorecardStatus::NA,
                Some(NaReason::AntecedentUnmet),
                "no MCP endpoint discovered".to_string()
            ),
            "{id}"
        );
    }
}

#[test]
fn a_usable_authorization_server_beside_bad_entries_is_noncompliant() {
    let card = scorecard_of("auth-servers-mixed");
    let (status, _, evidence) = verdict(&card, "mcp-auth-servers");
    assert_eq!(status, ScorecardStatus::Noncompliant, "{evidence}");
}

#[test]
fn a_bare_bearer_challenge_names_no_metadata() {
    let card = scorecard_of("mcp-www-authenticate");
    assert_eq!(
        verdict(&card, "mcp-auth-challenge"),
        (
            ScorecardStatus::Noncompliant,
            None,
            "the Bearer challenge names no resource_metadata".to_string()
        )
    );
}

/// Open 155, protected 158, no MCP 158: an open server's global keeps its
/// value, one without an endpoint counts the largest design.
#[test]
fn the_global_maximum_counts_the_mcp_access_design_the_site_presents() {
    assert_eq!(scorecard_of("run-all-pass").score.global, 100);
    let no_mcp = scorecard_of("frontmatter-absent");
    assert_eq!((no_mcp.score.relative, no_mcp.score.global), (65, 19));
    let protected = scorecard_of("auth-declared-endpoint");
    assert_eq!((protected.score.relative, protected.score.global), (68, 26));
}

const SITE: &str = "https://example.com/";
const METADATA_JSON: &[u8] = br#"{"resource":"https://example.com/mcp","authorization_servers":["https://auth.example.com"]}"#;

fn html_root() -> MockRule {
    MockRule::new("GET", SITE).response(MockResponse::ok(
        200,
        &[("content-type", "text/html")],
        b"<html><head><title>t</title></head><body>hi</body></html>",
    ))
}

fn json(body: &[u8]) -> MockResponse {
    MockResponse::ok(200, &[("content-type", "application/json")], body)
}

/// A run over `rules` whose unmatched requests read 404, and every request
/// it sent.
fn run(rules: Vec<MockRule>) -> (WebScorecard, Vec<String>) {
    let transport = Arc::new(MockTransport::new(rules, MockResponse::ok(404, &[], b"")));
    let fetch = FetchHandle::new(
        transport.clone(),
        Arc::new(common::corpus::PublicResolver),
        AUDIT_USER_AGENT,
    );
    let mut input = RunInput::new(SITE, fetch, Arc::new(default_handlers()));
    input.per_check_timeout = Duration::from_secs(2);
    input.per_audit_deadline = Duration::from_secs(20);
    let RunOutcome::Complete(report) = run_web_audit(input) else {
        panic!("the site answers");
    };
    let sent = transport
        .requests()
        .iter()
        .map(|r| format!("{} {}", r.method, r.url))
        .collect();
    (report.scorecard, sent)
}

/// A common path's 401 whose challenge names metadata the auditor must not
/// read: another host, plaintext, or a cloud metadata address. None of it
/// is requested, and the path never becomes the endpoint.
#[test]
fn a_challenge_naming_metadata_off_the_endpoints_https_host_is_never_read() {
    for named in [
        "https://evil.example.net/.well-known/oauth-protected-resource",
        "http://example.com/.well-known/oauth-protected-resource",
        "https://169.254.169.254/.well-known/oauth-protected-resource",
    ] {
        let challenge = format!("Bearer resource_metadata=\"{named}\"");
        let (card, sent) = run(vec![
            html_root(),
            MockRule::new("POST", "https://example.com/mcp").response(MockResponse::ok(
                401,
                &[("www-authenticate", &challenge)],
                b"",
            )),
            MockRule::new("GET", named).response(json(METADATA_JSON)),
        ]);
        assert_eq!(card.mcp_endpoint, None, "{named}");
        assert!(
            !sent.iter().any(|r| r.ends_with(named)),
            "{named} was requested: {sent:#?}"
        );
    }
}

/// A common path that answers 401, metadata naming it as its resource, and
/// an echo control answered with `echo`.
fn protected_endpoint(echo: MockResponse, tools_list: MockResponse) -> WebScorecard {
    let challenge =
        "Bearer resource_metadata=\"https://example.com/.well-known/oauth-protected-resource/mcp\"";
    run(vec![
        html_root(),
        MockRule::new("POST", "https://example.com/mcp")
            .body_contains("tools/list")
            .response(tools_list),
        MockRule::new("POST", "https://example.com/mcp").response(MockResponse::ok(
            401,
            &[("www-authenticate", challenge)],
            b"",
        )),
        MockRule::new(
            "GET",
            "https://example.com/.well-known/oauth-protected-resource/mcp",
        )
        .response(json(METADATA_JSON)),
        MockRule::new(
            "GET",
            "https://example.com/.well-known/oauth-protected-resource/anc-web-audit-no-such-resource",
        )
        .response(echo),
    ])
    .0
}

fn unauthorized() -> MockResponse {
    MockResponse::ok(401, &[("www-authenticate", "Bearer")], b"")
}

fn not_found() -> MockResponse {
    MockResponse::ok(404, &[], b"")
}

#[test]
fn a_common_path_401_its_metadata_backs_is_the_endpoint_and_requires_sign_in() {
    let card = protected_endpoint(not_found(), unauthorized());
    assert_eq!(
        card.mcp_endpoint.as_deref(),
        Some("https://example.com/mcp")
    );
    assert_eq!(
        verdict(&card, "mcp-initialize").1,
        Some(NaReason::AuthRequired)
    );
    assert_eq!(verdict(&card, "mcp-auth-enforced").0, ScorecardStatus::Pass);
    let probed: Vec<&str> = card
        .mcp_discovery
        .iter()
        .filter_map(|item| item.get("probed").and_then(|p| p.as_str()))
        .collect();
    assert!(probed.contains(&"initialize (auth required)"), "{probed:?}");
}

/// Where the metadata is what finds the endpoint, an echo read that gets no
/// answer confirms nothing.
#[test]
fn an_unanswered_echo_read_finds_no_endpoint() {
    let card = protected_endpoint(MockResponse::ok(503, &[], b""), unauthorized());
    assert_eq!(card.mcp_endpoint, None);
}

#[test]
fn a_tools_list_served_without_a_token_is_noncompliant() {
    let served = json(br#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#);
    let card = protected_endpoint(not_found(), served);
    assert_eq!(
        verdict(&card, "mcp-auth-enforced"),
        (
            ScorecardStatus::Noncompliant,
            None,
            "a request without a token was served a result".to_string()
        )
    );
}
