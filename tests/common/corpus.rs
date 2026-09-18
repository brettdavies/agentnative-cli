//! The vendored conformance corpus, replayed through the engine: each
//! scenario's exchanges become mock-transport rules, the engine runs over
//! them, and the rows it finalizes are compared with the site's golden
//! scorecard. The format is documented in
//! `tests/fixtures/web-audit-conformance/README.md`.

use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use agentnative::web_audit::engine::{FetchHandle, HandlerSet, RunInput, RunOutcome, RunReport};
use agentnative::web_audit::locality::Resolver;
use agentnative::web_audit::mock::{MockResponse, MockRule, MockTransport};
use agentnative::web_audit::registry::AUDIT_USER_AGENT;
use agentnative::web_audit::scorecard::{DeclaredSiteType, ResultRow, WebScorecard};
use serde::Deserialize;
use serde_json::{Map, Value};

/// Every name resolves to a public address, as the corpus targets do.
pub struct PublicResolver;

impl Resolver for PublicResolver {
    fn resolve(&self, _host: &str) -> Result<Vec<IpAddr>, String> {
        Ok(vec!["93.184.216.34".parse().unwrap()])
    }
}

/// A recorded response, or a transport failure.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum Recorded {
    /// A transport failure surfaced with this message.
    Failure {
        /// The `ProbeResponse.error` string.
        error: String,
    },
    /// A full response.
    Response {
        /// HTTP status.
        status: u16,
        /// Lowercase header names, single values.
        #[serde(default)]
        headers: Map<String, Value>,
        /// UTF-8 body.
        #[serde(default)]
        body: String,
    },
}

impl Recorded {
    fn to_mock(&self) -> MockResponse {
        match self {
            Recorded::Failure { error } => MockResponse::error(error),
            Recorded::Response {
                status,
                headers,
                body,
            } => {
                let pairs: Vec<(String, String)> = headers
                    .iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect();
                let borrowed: Vec<(&str, &str)> = pairs
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect();
                MockResponse::ok(*status, &borrowed, body.as_bytes())
            }
        }
    }
}

/// The request side of an exchange.
#[derive(Clone, Debug, Deserialize)]
pub struct RecordedRequest {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: Map<String, Value>,
    #[serde(default)]
    pub body_json_method: Option<String>,
    #[serde(default)]
    pub body_contains: Option<String>,
}

/// One ordered rule.
#[derive(Clone, Debug, Deserialize)]
pub struct Exchange {
    pub request: RecordedRequest,
    pub response: Recorded,
}

/// A `scenario.json`.
#[derive(Clone, Debug, Deserialize)]
pub struct Scenario {
    pub description: String,
    pub covers: Vec<String>,
    pub target: String,
    pub site_type: Option<String>,
    pub spec_version: String,
    pub unmatched: Recorded,
    pub allow_unmatched: bool,
    pub exchanges: Vec<Exchange>,
}

/// A `scorecard.json`: the site's scorecard, or the unreachable reason.
#[derive(Clone, Debug)]
pub enum Golden {
    Scorecard(Box<WebScorecard>),
    Unreachable(String),
}

/// One scenario directory, loaded.
#[derive(Clone, Debug)]
pub struct Case {
    pub name: String,
    pub scenario: Scenario,
    pub golden: Golden,
}

/// The corpus root.
pub fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/web-audit-conformance")
}

/// Every scenario, in directory order.
pub fn load_cases() -> Vec<Case> {
    let root = corpus_dir().join("scenarios");
    let mut names: Vec<String> = fs::read_dir(&root)
        .expect("corpus scenarios directory")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let dir = root.join(&name);
            let scenario: Scenario = serde_json::from_str(
                &fs::read_to_string(dir.join("scenario.json")).expect("scenario.json"),
            )
            .unwrap_or_else(|e| panic!("{name}/scenario.json: {e}"));
            let raw = fs::read_to_string(dir.join("scorecard.json")).expect("scorecard.json");
            let golden = match serde_json::from_str::<Value>(&raw) {
                Ok(Value::Object(map)) if map.contains_key("unreachable") => {
                    Golden::Unreachable(map["unreachable"].as_str().unwrap_or_default().to_string())
                }
                _ => Golden::Scorecard(Box::new(
                    serde_json::from_str(&raw)
                        .unwrap_or_else(|e| panic!("{name}/scorecard.json: {e}")),
                )),
            };
            Case {
                name,
                scenario,
                golden,
            }
        })
        .collect()
}

/// The scenario's exchanges as a mock transport.
pub fn transport_for(scenario: &Scenario) -> Arc<MockTransport> {
    let rules: Vec<MockRule> = scenario
        .exchanges
        .iter()
        .map(|x| {
            let mut rule = MockRule::new(&x.request.method, &x.request.url);
            for (name, value) in &x.request.headers {
                if let Some(v) = value.as_str() {
                    rule = rule.header(name, v);
                }
            }
            if let Some(method) = &x.request.body_json_method {
                rule = rule.body_json_method(method);
            }
            if let Some(needle) = &x.request.body_contains {
                rule = rule.body_contains(needle);
            }
            rule.response(x.response.to_mock())
        })
        .collect();
    Arc::new(MockTransport::new(rules, scenario.unmatched.to_mock()))
}

/// Run the engine over a scenario with the given handlers.
pub fn replay(scenario: &Scenario, handlers: Arc<HandlerSet>) -> RunOutcome {
    let transport = transport_for(scenario);
    let fetch = FetchHandle::new(transport, Arc::new(PublicResolver), AUDIT_USER_AGENT);
    let mut input = RunInput::new(&scenario.target, fetch, handlers);
    input.site_type = match scenario.site_type.as_deref() {
        Some("content") => Some(DeclaredSiteType::Content),
        Some("api") => Some(DeclaredSiteType::Api),
        _ => None,
    };
    input.spec_version = scenario.spec_version.clone();
    input.per_check_timeout = Duration::from_secs(2);
    input.per_audit_deadline = Duration::from_secs(20);
    agentnative::web_audit::engine::run_web_audit(input)
}

/// A row-level difference between the engine and the golden.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowDiff {
    pub id: String,
    pub expected: Option<ResultRow>,
    pub actual: Option<ResultRow>,
}

/// Compare the rows named by `ids` between a report and a golden scorecard.
pub fn diff_rows(report: &RunReport, golden: &WebScorecard, ids: &[String]) -> Vec<RowDiff> {
    let mut out = Vec::new();
    for id in ids {
        let expected = golden.results.iter().find(|r| &r.id == id).cloned();
        let actual = report
            .scorecard
            .results
            .iter()
            .find(|r| &r.id == id)
            .cloned();
        if expected != actual {
            out.push(RowDiff {
                id: id.clone(),
                expected,
                actual,
            });
        }
    }
    out
}

/// Render a diff list for an assertion message.
pub fn describe(diffs: &[RowDiff]) -> String {
    diffs
        .iter()
        .map(|d| {
            let show = |row: &Option<ResultRow>| match row {
                Some(r) => format!(
                    "{:?} na_reason={:?} unprobed={} evidence={:?}",
                    r.status, r.na_reason, r.unprobed, r.evidence
                ),
                None => "<missing>".to_string(),
            };
            format!(
                "  {}:\n    expected {}\n    actual   {}",
                d.id,
                show(&d.expected),
                show(&d.actual)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
