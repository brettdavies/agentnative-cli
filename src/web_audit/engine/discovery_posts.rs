//! Discovery's POST probing on the audited origin's common MCP paths, a
//! mirror of the site's `discovery-posts.ts`: the legacy `initialize` and
//! modern `tools/list` POSTs go to every path at once under one timeout,
//! and a legacy answer wins.
//!
//! The site hands a POST redirected off the audited origin back as a
//! declaration its follow slice settles. The local engine has no follow
//! slice, so a redirected POST is followed like any other probe's, which is
//! the only way it finds such an endpoint.

use std::time::Duration;

use serde_json::{Value, json};

use super::discovery::fetch_all;
use super::mcp_wire::{
    legacy_initialize_body, legacy_probe_headers, modern_probe_body, modern_probe_headers,
    parse_json_rpc,
};
use super::types::FetchHandle;
use crate::web_audit::fetch::{FetchInit, FetchOptions, ProbeResponse};
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
}

fn row(value: Value) -> EvidenceItem {
    value.as_object().cloned().unwrap_or_default()
}

fn miss(candidate: &Candidate, resp: &ProbeResponse, probed: &str, why: &str) -> EvidenceItem {
    row(json!({
        "source": candidate.path,
        "status": resp.status,
        "probed": format!("{probed} ({why})"),
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
        ..FetchOptions::default()
    };
    let responses = fetch_all(fetch, requests, &opts);
    let (legacy, modern) = responses.split_at(candidates.len());

    let mut items = Vec::new();
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
            };
        }
        items.push(miss(candidate, resp, "initialize", "no serverInfo"));
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
            };
        }
        items.push(miss(candidate, resp, "modern-tools-list", "no tools"));
    }
    PostProbing {
        endpoint: None,
        items,
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
