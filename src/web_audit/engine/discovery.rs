//! MCP endpoint discovery, a mirror of the site's `discovery.ts`.
//!
//! Well-known cards are probed first; then a legacy `initialize` on the
//! common paths takes the first answer carrying `serverInfo`; then a modern
//! header-routed `tools/list` over the same paths takes the first `tools`
//! result, so a modern-only server is still found. Each pass probes its
//! candidates concurrently under one shared slice bounded by the per-audit
//! deadline and the discovery budget, so a target that never answers cannot
//! spend the whole budget on one path at a time. A card's endpoint is
//! target-controlled, so an off-origin declaration is recorded and dropped.

use std::time::{Duration, Instant};

use serde_json::{Value, json};
use url::Url;

use super::mcp_wire::{
    legacy_initialize_body, legacy_probe_headers, modern_probe_body, modern_probe_headers,
    parse_json_rpc,
};
use super::pool::run_bounded;
use super::types::FetchHandle;
use crate::web_audit::fetch::{FetchInit, FetchOptions, ProbeResponse};
use crate::web_audit::registry::McpDiscovery;
use crate::web_audit::scorecard::EvidenceItem;

/// Hard wall-clock cap on the whole discovery phase, so it never starves
/// the check waves.
pub const DISCOVERY_BUDGET: Duration = Duration::from_millis(12_000);

/// Slack past a pass's slice before an unreturned probe counts as failed.
const PASS_GRACE: Duration = Duration::from_millis(250);

/// Where discovery landed and how it got there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveryResult {
    /// The endpoint, when a pass found one.
    pub endpoint: Option<String>,
    /// One evidence row per probe, in path order.
    pub evidence: Vec<EvidenceItem>,
}

/// Join a path to the base, or pass an absolute URL through unchanged; empty
/// when the path cannot resolve. Mirrors the site's `resolveUrl`.
pub fn resolve_url(base: &str, path_or_url: &str) -> String {
    if path_or_url.is_empty() {
        return String::new();
    }
    if path_or_url.starts_with("http://") || path_or_url.starts_with("https://") {
        return path_or_url.to_string();
    }
    Url::parse(base)
        .and_then(|b| b.join(path_or_url))
        .map(|u| u.to_string())
        .unwrap_or_default()
}

fn same_origin(candidate: &str, base: &str) -> bool {
    match (Url::parse(candidate), Url::parse(base)) {
        (Ok(c), Ok(b)) => c.origin() == b.origin(),
        _ => false,
    }
}

fn item(value: Value) -> EvidenceItem {
    value.as_object().cloned().unwrap_or_default()
}

fn status_value(resp: &ProbeResponse) -> Value {
    match resp.status {
        Some(s) => Value::from(s),
        None => Value::Null,
    }
}

fn failed(error: &str) -> ProbeResponse {
    ProbeResponse::failed(error.to_string(), 0)
}

/// Probe every candidate in one concurrent pass; results stay in URL order.
fn probe_all(
    fetch: &FetchHandle,
    urls: &[String],
    init: FetchInit,
    timeout: Duration,
) -> Vec<ProbeResponse> {
    let opts = FetchOptions {
        timeout,
        ..FetchOptions::default()
    };
    let jobs: Vec<(FetchHandle, String, FetchInit, FetchOptions)> = urls
        .iter()
        .map(|url| (fetch.clone(), url.clone(), init.clone(), opts.clone()))
        .collect();
    let deadline = Instant::now() + timeout + PASS_GRACE;
    run_bounded(jobs, urls.len(), deadline, |(fetch, url, init, opts)| {
        fetch.fetch(&url, &init, &opts)
    })
    .into_iter()
    .map(|r| r.unwrap_or_else(|| failed("TimeoutError: deadline exceeded")))
    .collect()
}

/// Discover the MCP endpoint for `base` under the registry's configuration.
pub fn discover_mcp_endpoint(
    fetch: &FetchHandle,
    base: &str,
    cfg: &McpDiscovery,
    timeout: Duration,
    deadline: Instant,
) -> DiscoveryResult {
    let mut evidence: Vec<EvidenceItem> = Vec::new();
    let deadline_at = deadline.min(Instant::now() + DISCOVERY_BUDGET);
    let pass_budget = || -> Option<Duration> {
        let slice = timeout.min(deadline_at.saturating_duration_since(Instant::now()));
        (!slice.is_zero()).then_some(slice)
    };
    let exhausted = |mut evidence: Vec<EvidenceItem>| {
        evidence.push(item(
            json!({ "note": "per-audit deadline exceeded during discovery" }),
        ));
        DiscoveryResult {
            endpoint: None,
            evidence,
        }
    };

    // Pass 1: well-known cards; the first in configured order with a
    // same-origin endpoint wins.
    {
        let pairs: Vec<(&str, String)> = cfg
            .well_known
            .iter()
            .map(|wk| (*wk, resolve_url(base, wk)))
            .filter(|(_, url)| !url.is_empty())
            .collect();
        let Some(slice) = pass_budget() else {
            return exhausted(evidence);
        };
        let urls: Vec<String> = pairs.iter().map(|(_, url)| url.clone()).collect();
        let responses = probe_all(fetch, &urls, FetchInit::default(), slice);
        for ((wk, _), resp) in pairs.iter().zip(responses.iter()) {
            if resp.status != Some(200) {
                continue;
            }
            let card = parse_json_rpc(resp).unwrap_or_default();
            let endpoint = card
                .get("mcp_endpoint")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    card.get("url")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                })
                .or_else(|| {
                    card.get("transport")
                        .and_then(|t| t.get("endpoint"))
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                });
            if let Some(ep) = endpoint {
                let resolved = resolve_url(base, ep);
                if !same_origin(&resolved, base) {
                    evidence.push(item(json!({
                        "source": wk,
                        "endpoint": resolved,
                        "blocked": "off-origin endpoint declaration",
                    })));
                    continue;
                }
                let mut found = item(json!({ "source": wk, "endpoint": resolved }));
                if card.contains_key("authentication") || card.contains_key("auth") {
                    found.insert("authentication".to_string(), Value::Bool(true));
                }
                evidence.push(found);
                return DiscoveryResult {
                    endpoint: Some(resolved),
                    evidence,
                };
            }
            evidence.push(item(
                json!({ "source": wk, "note": "card present, no endpoint field" }),
            ));
        }
    }

    let paths: Vec<(&str, String)> = cfg
        .common_paths
        .iter()
        .map(|p| (*p, resolve_url(base, p)))
        .filter(|(_, url)| !url.is_empty())
        .collect();
    let urls: Vec<String> = paths.iter().map(|(_, url)| url.clone()).collect();

    // Pass 2: legacy initialize on the common paths.
    {
        let Some(slice) = pass_budget() else {
            return exhausted(evidence);
        };
        let init = FetchInit {
            method: "POST".to_string(),
            headers: legacy_probe_headers(),
            body: Some(legacy_initialize_body(cfg.protocol_version).into_bytes()),
        };
        let responses = probe_all(fetch, &urls, init, slice);
        for ((path, url), resp) in paths.iter().zip(responses.iter()) {
            let rpc = parse_json_rpc(resp);
            let has_server_info = rpc
                .as_ref()
                .and_then(|r| r.get("result"))
                .and_then(Value::as_object)
                .and_then(|result| result.get("serverInfo"))
                .is_some_and(truthy);
            if has_server_info {
                evidence.push(item(
                    json!({ "source": path, "endpoint": url, "probed": "initialize" }),
                ));
                return DiscoveryResult {
                    endpoint: Some(url.clone()),
                    evidence,
                };
            }
            evidence.push(item(json!({
                "source": path,
                "status": status_value(resp),
                "probed": "initialize (no serverInfo)",
            })));
        }
    }

    // Pass 3: modern header-routed tools/list on the same paths.
    {
        let Some(slice) = pass_budget() else {
            return exhausted(evidence);
        };
        let init = FetchInit {
            method: "POST".to_string(),
            headers: modern_probe_headers("tools/list"),
            body: Some(modern_probe_body("tools/list").into_bytes()),
        };
        let responses = probe_all(fetch, &urls, init, slice);
        for ((path, url), resp) in paths.iter().zip(responses.iter()) {
            let has_tools = parse_json_rpc(resp)
                .as_ref()
                .and_then(|r| r.get("result"))
                .and_then(|result| result.get("tools"))
                .is_some_and(Value::is_array);
            if has_tools {
                evidence.push(item(
                    json!({ "source": path, "endpoint": url, "probed": "modern-tools-list" }),
                ));
                return DiscoveryResult {
                    endpoint: Some(url.clone()),
                    evidence,
                };
            }
            evidence.push(item(json!({
                "source": path,
                "status": status_value(resp),
                "probed": "modern-tools-list (no tools)",
            })));
        }
    }

    DiscoveryResult {
        endpoint: None,
        evidence,
    }
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::web_audit::locality::Resolver;
    use crate::web_audit::mock::{MockResponse, MockRule, MockTransport};

    struct PublicResolver;
    impl Resolver for PublicResolver {
        fn resolve(&self, _: &str) -> Result<Vec<std::net::IpAddr>, String> {
            Ok(vec!["93.184.216.34".parse().unwrap()])
        }
    }

    const BASE: &str = "https://example.com/";

    fn cfg() -> McpDiscovery {
        McpDiscovery {
            well_known: &["/.well-known/mcp.json", "/.well-known/mcp/server-card.json"],
            common_paths: &["/mcp", "/sse", "/message"],
            protocol_version: "2025-06-18",
        }
    }

    fn handle(transport: MockTransport) -> FetchHandle {
        FetchHandle::new(Arc::new(transport), Arc::new(PublicResolver), "test-ua")
    }

    fn json_ok(body: &str) -> MockResponse {
        MockResponse::ok(
            200,
            &[("content-type", "application/json")],
            body.as_bytes(),
        )
    }

    fn discover(transport: MockTransport) -> DiscoveryResult {
        discover_mcp_endpoint(
            &handle(transport),
            BASE,
            &cfg(),
            Duration::from_secs(2),
            Instant::now() + Duration::from_secs(10),
        )
    }

    #[test]
    fn a_well_known_card_with_an_endpoint_wins_and_records_auth() {
        let transport = MockTransport::new(
            vec![
                MockRule::new("GET", "https://example.com/.well-known/mcp.json").response(json_ok(
                    r#"{"mcp_endpoint":"/mcp","authentication":{"type":"oauth2"}}"#,
                )),
            ],
            MockResponse::ok(404, &[], b""),
        );
        let out = discover(transport);
        assert_eq!(out.endpoint.as_deref(), Some("https://example.com/mcp"));
        assert_eq!(
            serde_json::to_string(&out.evidence).unwrap(),
            r#"[{"source":"/.well-known/mcp.json","endpoint":"https://example.com/mcp","authentication":true}]"#
        );
    }

    #[test]
    fn a_nested_transport_endpoint_is_read_and_an_off_origin_one_is_dropped() {
        let transport = MockTransport::new(
            vec![
                MockRule::new("GET", "https://example.com/.well-known/mcp.json").response(json_ok(
                    r#"{"transport":{"endpoint":"https://other.test/mcp"}}"#,
                )),
                MockRule::new(
                    "GET",
                    "https://example.com/.well-known/mcp/server-card.json",
                )
                .response(json_ok(r#"{"transport":{"endpoint":"/rpc"}}"#)),
            ],
            MockResponse::ok(404, &[], b""),
        );
        let out = discover(transport);
        assert_eq!(out.endpoint.as_deref(), Some("https://example.com/rpc"));
        assert_eq!(
            out.evidence[0].get("blocked").and_then(Value::as_str),
            Some("off-origin endpoint declaration")
        );
        assert_eq!(out.evidence.len(), 2);
    }

    #[test]
    fn initialize_then_modern_fallbacks_and_a_silent_target() {
        let init = MockTransport::new(
            vec![
                MockRule::new("POST", "https://example.com/sse")
                    .body_json_method("initialize")
                    .response(json_ok(
                        r#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"name":"x"}}}"#,
                    )),
            ],
            MockResponse::ok(404, &[], b""),
        );
        let out = discover(init);
        assert_eq!(out.endpoint.as_deref(), Some("https://example.com/sse"));
        assert_eq!(out.evidence.len(), 2, "{:?}", out.evidence);
        assert_eq!(out.evidence[0].get("status"), Some(&Value::from(404)));
        assert_eq!(
            out.evidence[1].get("probed").and_then(Value::as_str),
            Some("initialize")
        );

        let modern = MockTransport::new(
            vec![
                MockRule::new("POST", "https://example.com/mcp")
                    .header("mcp-method", "tools/list")
                    .response(json_ok(r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#)),
            ],
            MockResponse::ok(404, &[], b""),
        );
        let out = discover(modern);
        assert_eq!(out.endpoint.as_deref(), Some("https://example.com/mcp"));
        assert_eq!(
            out.evidence
                .last()
                .unwrap()
                .get("probed")
                .and_then(Value::as_str),
            Some("modern-tools-list")
        );

        let silent =
            MockTransport::new(vec![], MockResponse::error("TypeError: connection refused"));
        let out = discover(silent);
        assert_eq!(out.endpoint, None);
        assert_eq!(out.evidence.len(), 6);
        assert_eq!(out.evidence[2].get("status"), Some(&Value::Null));
    }

    #[test]
    fn a_spent_budget_ends_discovery_with_a_note() {
        let transport = MockTransport::new(vec![], MockResponse::ok(404, &[], b""));
        let out = discover_mcp_endpoint(
            &handle(transport),
            BASE,
            &cfg(),
            Duration::from_secs(2),
            Instant::now(),
        );
        assert_eq!(out.endpoint, None);
        assert_eq!(
            out.evidence[0].get("note").and_then(Value::as_str),
            Some("per-audit deadline exceeded during discovery")
        );
    }

    #[test]
    fn resolve_url_mirrors_the_site() {
        assert_eq!(resolve_url(BASE, "/mcp"), "https://example.com/mcp");
        assert_eq!(
            resolve_url(BASE, "https://other.test/x"),
            "https://other.test/x"
        );
        assert_eq!(resolve_url(BASE, ""), "");
        assert_eq!(resolve_url("not a url", "/x"), "");
    }
}
