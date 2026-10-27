//! Discovery's POST probing on the audited origin's common MCP paths, a
//! mirror of the site's `discovery-posts.ts`: the legacy `initialize` and
//! modern `tools/list` POSTs go to every path at once under one timeout,
//! and a legacy answer wins.
//!
//! A POST is never re-sent off the audited origin. A common path that
//! answers with a redirect to another origin is recorded with its target,
//! and the target is declared like a card's off-origin endpoint, so it is
//! probed only once its own host confirms it.

use std::time::Duration;

use serde_json::{Value, json};
use url::Url;

use super::discovery::fetch_all;
use super::discovery_documents::same_origin;
use super::follow_trail::{Declaration, Kind};
use super::mcp_wire::{
    legacy_initialize_body, legacy_probe_headers, modern_probe_body, modern_probe_headers,
    parse_json_rpc,
};
use super::types::FetchHandle;
use crate::web_audit::fetch::redirect::is_redirect;
use crate::web_audit::fetch::{CrossOriginRedirects, FetchInit, FetchOptions, ProbeResponse};
use crate::web_audit::scorecard::EvidenceItem;

/// A common path and the URL it resolved to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// The configured path.
    pub path: &'static str,
    /// The resolved URL.
    pub url: String,
}

/// What the POST pass found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PostProbing {
    /// The endpoint, when a lane answered.
    pub endpoint: Option<String>,
    /// One row per response read, legacy lane first.
    pub items: Vec<EvidenceItem>,
    /// Where POSTs were redirected off the audited origin, in probe order.
    pub redirected: Vec<Declaration>,
}

fn row(value: Value) -> EvidenceItem {
    value.as_object().cloned().unwrap_or_default()
}

/// Where a POST answer redirected off the audited origin, as the URL a hop
/// would request.
fn off_origin_redirect(url: &str, resp: &ProbeResponse) -> Option<String> {
    let location = resp.headers.get("location")?;
    if !resp.status.is_some_and(is_redirect) {
        return None;
    }
    let target = Url::parse(url).ok()?.join(location).ok()?.to_string();
    (!same_origin(&target, url)).then_some(target)
}

fn miss(
    candidate: &Candidate,
    resp: &ProbeResponse,
    probed: &str,
    why: &str,
    redirected: &mut Vec<Declaration>,
) -> EvidenceItem {
    let Some(target) = off_origin_redirect(&candidate.url, resp) else {
        return row(json!({
            "source": candidate.path,
            "status": resp.status,
            "probed": format!("{probed} ({why})"),
        }));
    };
    if !redirected.iter().any(|d| d.url == target) {
        redirected.push(Declaration::new(
            Kind::McpEndpoint,
            target.clone(),
            candidate.path.to_string(),
        ));
    }
    row(json!({
        "source": candidate.path,
        "status": resp.status,
        "probed": format!("{probed} (off-origin redirect)"),
        "redirect": target,
    }))
}

fn found(candidate: &Candidate, probed: &str) -> EvidenceItem {
    row(json!({ "source": candidate.path, "endpoint": candidate.url, "probed": probed }))
}

/// Legacy `initialize` and modern `tools/list` on every common path at
/// once; a legacy answer wins.
pub fn probe_common_paths(
    fetch: &FetchHandle,
    candidates: &[Candidate],
    protocol_version: &str,
    timeout: Duration,
) -> PostProbing {
    let legacy = FetchInit {
        method: "POST".to_string(),
        headers: legacy_probe_headers(),
        body: Some(legacy_initialize_body(protocol_version).into_bytes()),
    };
    let modern = FetchInit {
        method: "POST".to_string(),
        headers: modern_probe_headers("tools/list"),
        body: Some(modern_probe_body("tools/list").into_bytes()),
    };
    let requests: Vec<(String, FetchInit)> = [&legacy, &modern]
        .into_iter()
        .flat_map(|init| candidates.iter().map(|c| (c.url.clone(), init.clone())))
        .collect();
    let opts = FetchOptions {
        timeout,
        cross_origin_redirects: Some(CrossOriginRedirects::Return),
        ..FetchOptions::default()
    };
    let responses = fetch_all(fetch, requests, &opts);
    let (legacy, modern) = responses.split_at(candidates.len());

    let mut items = Vec::new();
    let mut redirected = Vec::new();
    for (candidate, resp) in candidates.iter().zip(legacy) {
        let has_server_info = parse_json_rpc(resp)
            .as_ref()
            .and_then(|rpc| rpc.get("result"))
            .and_then(Value::as_object)
            .and_then(|result| result.get("serverInfo"))
            .is_some_and(truthy);
        if has_server_info {
            items.push(found(candidate, "initialize"));
            return PostProbing {
                endpoint: Some(candidate.url.clone()),
                items,
                redirected,
            };
        }
        items.push(miss(
            candidate,
            resp,
            "initialize",
            "no serverInfo",
            &mut redirected,
        ));
    }
    for (candidate, resp) in candidates.iter().zip(modern) {
        let has_tools = parse_json_rpc(resp)
            .as_ref()
            .and_then(|rpc| rpc.get("result"))
            .and_then(|result| result.get("tools"))
            .is_some_and(Value::is_array);
        if has_tools {
            items.push(found(candidate, "modern-tools-list"));
            return PostProbing {
                endpoint: Some(candidate.url.clone()),
                items,
                redirected,
            };
        }
        items.push(miss(
            candidate,
            resp,
            "modern-tools-list",
            "no tools",
            &mut redirected,
        ));
    }
    PostProbing {
        endpoint: None,
        items,
        redirected,
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
