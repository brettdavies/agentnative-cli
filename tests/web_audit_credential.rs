//! A run that presents a bearer token for the audited MCP endpoint: the
//! token opens the session rows a sign-in would otherwise leave
//! unevaluated, it reaches the endpoint's handshake and session requests
//! and nothing else, and whether the endpoint requires sign-in is still
//! read from what a client without a token is answered.

#![cfg(feature = "web")]

mod common;

use std::sync::Arc;
use std::time::Duration;

use agentnative::web_audit::credential::{Credential, TokenSource};
use agentnative::web_audit::engine::{FetchHandle, RunInput, RunOutcome, RunReport, run_web_audit};
use agentnative::web_audit::fetch::{FetchInit, FetchOptions};
use agentnative::web_audit::handlers::default_handlers;
use agentnative::web_audit::mock::{MockResponse, MockRule, MockTransport};
use agentnative::web_audit::registry::AUDIT_USER_AGENT;
use agentnative::web_audit::scorecard::{NaReason, ScorecardStatus, WebScorecard, to_wire_json};
use agentnative::web_audit::transport::Request;
use common::corpus::{
    Exchange, Golden, PublicResolver, Recorded, RecordedRequest, Scenario, load_cases, replay_as,
    transport_for,
};
use serde_json::{Map, Value, json};

const TOKEN: &str = "corpus-token-7f3a";
const ENDPOINT: &str = "https://example.com/mcp";
const METADATA: &str = "https://example.com/.well-known/oauth-protected-resource/mcp";

/// Rows a client needs a session for, which sign-in blocks without a token.
const SESSION_ROWS: [&str; 6] = [
    "mcp-initialize",
    "mcp-capabilities",
    "mcp-tools-list",
    "mcp-server-discover",
    "mcp-modern-tools-list",
    "mcp-unknown-method",
];
const SIGN_IN_ROWS: [&str; 3] = [
    "mcp-auth-challenge",
    "mcp-auth-servers",
    "mcp-auth-enforced",
];

fn credential() -> Credential {
    Credential::bearer(TOKEN, TokenSource::Env).expect("a valid token")
}

fn bearer() -> String {
    format!("Bearer {TOKEN}")
}

fn authorization(request: &Request) -> Option<&str> {
    request
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        .map(|(_, value)| value.as_str())
}

fn scenario(name: &str) -> Scenario {
    load_cases()
        .into_iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no corpus scenario {name}"))
        .scenario
}

fn report_of(outcome: RunOutcome) -> RunReport {
    match outcome {
        RunOutcome::Complete(report) => *report,
        RunOutcome::Unreachable { reason, .. } => panic!("unreachable: {reason}"),
    }
}

fn response(status: u16, headers: Value, body: &str) -> Recorded {
    Recorded::Response {
        status,
        headers: headers.as_object().cloned().unwrap_or_default(),
        body: body.to_string(),
    }
}

/// `run-all-pass`, put behind sign-in: every MCP request the endpoint
/// serves needs the token, a request without one draws a 401 whose
/// challenge names RFC 9728 metadata on the endpoint's host, and the
/// cross-origin request, which no client signs, is refused the same way.
fn protected_server() -> Scenario {
    let mut site = scenario("run-all-pass");
    let challenge = format!("Bearer resource_metadata=\"{METADATA}\"");
    for exchange in &mut site.exchanges {
        let request = &mut exchange.request;
        if request.method != "POST" || request.url != ENDPOINT {
            continue;
        }
        if request.headers.contains_key("origin") {
            if let Recorded::Response {
                status,
                headers,
                body,
            } = &mut exchange.response
            {
                *status = 401;
                headers.insert("www-authenticate".into(), json!(challenge));
                *body = r#"{"error":"unauthorized"}"#.to_string();
            }
        } else {
            request
                .headers
                .insert("authorization".into(), json!(bearer()));
        }
    }
    site.exchanges.push(Exchange {
        request: RecordedRequest {
            method: "POST".into(),
            url: ENDPOINT.into(),
            headers: Map::new(),
            body_json_method: None,
            body_contains: None,
        },
        response: response(
            401,
            json!({ "content-type": "application/json", "www-authenticate": challenge }),
            r#"{"error":"unauthorized"}"#,
        ),
    });
    site.exchanges.push(Exchange {
        request: RecordedRequest {
            method: "GET".into(),
            url: METADATA.into(),
            headers: Map::new(),
            body_json_method: None,
            body_contains: None,
        },
        response: response(
            200,
            json!({ "content-type": "application/json" }),
            &json!({ "resource": ENDPOINT, "authorization_servers": ["https://auth.example.com"] })
                .to_string(),
        ),
    });
    site
}

fn run(site: &Scenario, credential: Option<Credential>) -> (RunReport, Vec<Request>) {
    let transport = transport_for(site);
    let report = report_of(replay_as(
        site,
        Arc::new(default_handlers()),
        transport.clone(),
        credential,
    ));
    (report, transport.requests())
}

fn verdict(card: &WebScorecard, id: &str) -> (ScorecardStatus, Option<NaReason>, String) {
    let row = card
        .results
        .iter()
        .find(|r| r.id == id)
        .unwrap_or_else(|| panic!("no row {id}"));
    (
        row.status,
        row.na_reason,
        row.evidence.clone().unwrap_or_default(),
    )
}

#[test]
fn a_signed_in_run_scores_a_protected_server_in_full() {
    let (report, sent) = run(&protected_server(), Some(credential()));
    let card = &report.scorecard;
    assert!(card.vantage.credentialed, "the token was presented");
    for id in SESSION_ROWS.iter().chain(&SIGN_IN_ROWS) {
        assert_eq!(
            verdict(card, id).0,
            ScorecardStatus::Pass,
            "{id}: {:?}",
            verdict(card, id)
        );
    }
    assert_eq!(
        verdict(card, "mcp-auth-enforced").2,
        "refused with 401",
        "the enforcement probe went without the token"
    );
    assert_eq!((card.score.relative, card.score.global), (100, 100));
    assert!(sent.iter().any(|r| authorization(r) == Some(&bearer())));
}

#[test]
fn without_a_token_the_same_server_reads_its_session_rows_auth_required() {
    let (report, sent) = run(&protected_server(), None);
    let card = &report.scorecard;
    assert!(!card.vantage.credentialed);
    for id in SESSION_ROWS {
        let (status, reason, _) = verdict(card, id);
        assert_eq!(
            (status, reason),
            (ScorecardStatus::NA, Some(NaReason::AuthRequired)),
            "{id}"
        );
    }
    for id in SIGN_IN_ROWS {
        assert_eq!(verdict(card, id).0, ScorecardStatus::Pass, "{id}");
    }
    assert!(sent.iter().all(|r| authorization(r).is_none()));
}

/// Over the whole corpus: a token reaches the endpoint of record's POSTs
/// and nothing else (no declared host, metadata document, sign-in server
/// or other check), and a server that ignores it scores exactly as the
/// token-less run the corpus pins.
#[test]
fn across_the_corpus_the_token_reaches_only_the_endpoint_of_record() {
    let mut signed = 0;
    for case in load_cases() {
        let Golden::Scorecard(golden) = &case.golden else {
            continue;
        };
        let (mut report, sent) = run(&case.scenario, Some(credential()));
        for request in sent.iter().filter(|r| authorization(r).is_some()) {
            assert_eq!(
                (
                    request.method.as_str(),
                    Some(request.url.as_str()),
                    authorization(request)
                ),
                (
                    "POST",
                    report.scorecard.mcp_endpoint.as_deref(),
                    Some(bearer().as_str())
                ),
                "{}: the token left the endpoint of record",
                case.name
            );
            signed += 1;
        }
        assert!(report.scorecard.vantage.credentialed, "{}", case.name);
        report.scorecard.vantage.credentialed = false;
        assert_eq!(
            to_wire_json(&report.scorecard),
            to_wire_json(golden),
            "{}: a token the server ignores changed the scorecard",
            case.name
        );
    }
    assert!(signed > 0, "no corpus scenario carried the token");
}

fn site_rules(endpoint_answer: MockResponse) -> Vec<MockRule> {
    vec![
        MockRule::new("GET", "https://example.com/").response(MockResponse::ok(
            200,
            &[("content-type", "text/html")],
            b"<html><head><title>t</title></head><body>hi</body></html>",
        )),
        MockRule::new(
            "GET",
            "https://example.com/.well-known/mcp/server-card.json",
        )
        .response(MockResponse::ok(
            200,
            &[("content-type", "application/json")],
            br#"{"name":"x","version":"1","mcp_endpoint":"https://example.com/mcp"}"#,
        )),
        MockRule::new("POST", ENDPOINT)
            .header("authorization", &bearer())
            .response(endpoint_answer),
    ]
}

/// A signed handshake answered with a redirect to another origin sends
/// nothing carrying the token there.
#[test]
fn a_cross_origin_redirect_never_carries_the_token() {
    for status in [307, 308] {
        let elsewhere = "https://other.example.net/mcp";
        let transport = Arc::new(MockTransport::new(
            site_rules(MockResponse::ok(status, &[("location", elsewhere)], b"")),
            MockResponse::ok(404, &[], b""),
        ));
        let fetch = FetchHandle::new(
            transport.clone(),
            Arc::new(PublicResolver),
            AUDIT_USER_AGENT,
        )
        .with_credential(credential());
        let mut input = RunInput::new("https://example.com/", fetch, Arc::new(default_handlers()));
        input.per_check_timeout = Duration::from_secs(2);
        input.per_audit_deadline = Duration::from_secs(20);
        report_of(run_web_audit(input));
        let sent = transport.requests();
        assert!(
            sent.iter()
                .any(|r| r.url == ENDPOINT && authorization(r).is_some()),
            "{status}: the endpoint was never signed in to"
        );
        let leaked: Vec<&Request> = sent
            .iter()
            .filter(|r| !r.url.starts_with("https://example.com/") && authorization(r).is_some())
            .collect();
        assert!(leaked.is_empty(), "{status}: {leaked:#?}");
    }
}

/// The guard below the MCP rows' own redirect policy: whatever a call
/// follows, a hop to another origin goes without the credential, and a hop
/// on the first origin keeps it.
#[test]
fn the_fetch_layer_drops_a_credential_at_an_origin_change() {
    let transport = Arc::new(MockTransport::new(
        vec![
            MockRule::new("GET", "https://example.com/a").response(MockResponse::ok(
                302,
                &[("location", "/b")],
                b"",
            )),
            MockRule::new("GET", "https://example.com/b").response(MockResponse::ok(
                307,
                &[("location", "https://other.example.net/c")],
                b"",
            )),
        ],
        MockResponse::ok(200, &[], b"done"),
    ));
    let fetch = FetchHandle::new(
        transport.clone(),
        Arc::new(PublicResolver),
        AUDIT_USER_AGENT,
    );
    let init = FetchInit {
        headers: vec![("Authorization".to_string(), bearer())],
        ..FetchInit::default()
    };
    let resp = fetch.fetch("https://example.com/a", &init, &FetchOptions::default());
    assert_eq!(resp.status, Some(200));
    let hops: Vec<(String, Option<String>)> = transport
        .requests()
        .iter()
        .map(|r| (r.url.clone(), authorization(r).map(str::to_string)))
        .collect();
    assert_eq!(
        hops,
        vec![
            ("https://example.com/a".to_string(), Some(bearer())),
            ("https://example.com/b".to_string(), Some(bearer())),
            ("https://other.example.net/c".to_string(), None),
        ]
    );
}

#[test]
fn a_request_prints_without_its_credential() {
    let (_, sent) = run(&protected_server(), Some(credential()));
    let signed = sent
        .iter()
        .find(|r| authorization(r).is_some())
        .expect("a signed request");
    let shown = format!("{signed:?}");
    assert!(!shown.contains(TOKEN), "{shown}");
}
