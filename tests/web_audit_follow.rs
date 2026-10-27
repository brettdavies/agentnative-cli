//! Declared hosts end to end, over the corpus's recorded exchanges and the
//! real handlers: a POST is never replayed across origins, an endpoint on a
//! declared host receives a wire probe only after its own host confirms it,
//! and a probe of a confirmed endpoint takes no redirect.

#![cfg(feature = "web")]

mod common;

use std::sync::Arc;

use agentnative::web_audit::engine::{RunOutcome, RunReport};
use agentnative::web_audit::handlers::default_handlers;
use agentnative::web_audit::scorecard::{NaReason, ScorecardStatus};
use common::corpus::{
    Case, Exchange, Recorded, RecordedRequest, load_cases, replay_on, transport_for,
};
use serde_json::{Map, Value};

const DECLARED: &str = "https://mcp.example.net/mcp";
const ELSEWHERE: &str = "https://elsewhere.example.org/mcp";

fn case(name: &str) -> Case {
    load_cases()
        .into_iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("the corpus has {name}"))
}

fn complete(outcome: RunOutcome) -> RunReport {
    match outcome {
        RunOutcome::Complete(report) => *report,
        RunOutcome::Unreachable { reason, .. } => panic!("unreachable: {reason}"),
    }
}

fn sent_to(requests: &[agentnative::web_audit::transport::Request], host: &str) -> Vec<String> {
    requests
        .iter()
        .filter(|r| url::Url::parse(&r.url).is_ok_and(|u| u.host_str() == Some(host)))
        .map(|r| format!("{} {}", r.method, r.url))
        .collect()
}

#[test]
fn a_post_redirected_off_the_audited_origin_is_not_replayed_to_an_unconfirmed_host() {
    let case = case("follow-own-redirect-refused");
    let transport = transport_for(&case.scenario);
    let report = complete(replay_on(
        &case.scenario,
        Arc::new(default_handlers()),
        transport.clone(),
    ));
    let reached = sent_to(&transport.requests(), "mcp.example.net");
    assert!(
        reached
            .iter()
            .all(|r| !r.starts_with("POST") && !r.starts_with("OPTIONS")),
        "no POST or OPTIONS reaches the host that confirmed nothing: {reached:?}"
    );
    let initialize = report
        .results
        .iter()
        .find(|r| r.check.id == "mcp-initialize")
        .expect("the initialize row");
    assert_eq!(initialize.na_reason, Some(NaReason::ReciprocityRefused));
    assert_eq!(initialize.evidence, DECLARED);
}

#[test]
fn a_probe_of_a_confirmed_endpoint_takes_no_redirect() {
    let mut case = case("follow-card-admit");
    let mut headers = Map::new();
    headers.insert("location".into(), Value::String(ELSEWHERE.into()));
    case.scenario.exchanges.insert(
        0,
        Exchange {
            request: RecordedRequest {
                method: "POST".into(),
                url: DECLARED.into(),
                headers: Map::new(),
                body_json_method: None,
                body_contains: None,
            },
            response: Recorded::Response {
                status: 307,
                headers,
                body: String::new(),
            },
        },
    );
    let transport = transport_for(&case.scenario);
    let report = complete(replay_on(
        &case.scenario,
        Arc::new(default_handlers()),
        transport.clone(),
    ));
    assert_eq!(report.scorecard.mcp_endpoint.as_deref(), Some(DECLARED));
    assert_eq!(
        sent_to(&transport.requests(), "elsewhere.example.org"),
        Vec::<String>::new()
    );
    let initialize = report
        .results
        .iter()
        .find(|r| r.check.id == "mcp-initialize")
        .expect("the initialize row");
    assert_ne!(initialize.status, ScorecardStatus::Pass);
    let refused = format!("redirect refused: 307 to {ELSEWHERE}");
    assert!(
        initialize
            .raw_evidence
            .iter()
            .any(|item| item.get("error").and_then(Value::as_str) == Some(refused.as_str())),
        "the probe reads the redirect as refused: {:?}",
        initialize.raw_evidence
    );
}
