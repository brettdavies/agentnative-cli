//! The MCP request shapes and evidence readers the engine shares with the
//! MCP handler: the legacy and modern probe headers and bodies, the
//! post-wave `initialized` notification, JSON-RPC extraction from JSON or
//! SSE bodies, and the wave-1 session and lane readers. Bodies are the
//! exact bytes the site sends, key order included.

use std::time::Duration;

use serde_json::{Map, Value, json};

use super::types::{FetchHandle, McpModernLane, ProbeOutcome};
use crate::web_audit::fetch::{FetchInit, FetchOptions, ProbeResponse};
use crate::web_audit::scorecard::EvidenceItem;

/// The modern protocol revision the header-routed probes claim.
pub const MODERN_PROTOCOL_VERSION: &str = "2026-07-28";
/// The client identity every probe body carries.
pub const CLIENT_NAME: &str = "agent-web-audit";
/// The client version every probe body carries.
pub const CLIENT_VERSION: &str = "1.0";

/// Headers for a legacy (`initialize`-era) probe.
pub fn legacy_probe_headers() -> Vec<(String, String)> {
    vec![
        ("Content-Type".to_string(), "application/json".to_string()),
        (
            "Accept".to_string(),
            "application/json, text/event-stream".to_string(),
        ),
    ]
}

/// The legacy `tools/list` request body.
pub fn legacy_tools_list_body() -> String {
    json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {} }).to_string()
}

/// The legacy `initialize` request body, claiming `protocol_version`.
pub fn legacy_initialize_body(protocol_version: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": protocol_version,
            "capabilities": {},
            "clientInfo": { "name": CLIENT_NAME, "version": CLIENT_VERSION },
        },
    })
    .to_string()
}

/// Headers for a modern header-routed probe of `method`.
pub fn modern_probe_headers(method: &str) -> Vec<(String, String)> {
    vec![
        ("Content-Type".to_string(), "application/json".to_string()),
        (
            "Accept".to_string(),
            "application/json, text/event-stream".to_string(),
        ),
        (
            "MCP-Protocol-Version".to_string(),
            MODERN_PROTOCOL_VERSION.to_string(),
        ),
        ("Mcp-Method".to_string(), method.to_string()),
    ]
}

/// The `_meta` block every modern request body carries.
pub fn modern_meta(protocol_version: &str) -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": protocol_version,
        "io.modelcontextprotocol/clientInfo": { "name": CLIENT_NAME, "version": CLIENT_VERSION },
        "io.modelcontextprotocol/clientCapabilities": {},
    })
}

/// A modern request body for `method`.
pub fn modern_probe_body(method: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": { "_meta": modern_meta(MODERN_PROTOCOL_VERSION) },
    })
    .to_string()
}

fn initialized_body() -> String {
    json!({ "jsonrpc": "2.0", "method": "notifications/initialized", "params": {} }).to_string()
}

/// Best-effort session handshake after `initialize`; failures do not fail the audit.
pub fn notify_mcp_initialized(
    fetch: &FetchHandle,
    endpoint: &str,
    session_id: &str,
    timeout: Duration,
) {
    let mut headers = legacy_probe_headers();
    headers.push(("Mcp-Session-Id".to_string(), session_id.to_string()));
    let init = FetchInit {
        method: "POST".to_string(),
        headers,
        body: Some(initialized_body().into_bytes()),
    };
    let opts = FetchOptions {
        timeout,
        ..FetchOptions::default()
    };
    fetch.fetch(endpoint, &init, &opts);
}

/// Extract a JSON-RPC object from a JSON or `text/event-stream` body. An
/// SSE body, by content type or by a leading `event:` / `data:` line,
/// yields the first `data:` line that parses; a plain body parses whole.
pub fn parse_json_rpc(resp: &ProbeResponse) -> Option<Map<String, Value>> {
    let body = resp.body.as_str();
    let content_type = resp.headers.get("content-type").unwrap_or("");
    let looks_sse = body
        .trim_start_matches([' ', '\t', '\n', '\r'])
        .starts_with("event:")
        || body
            .trim_start_matches([' ', '\t', '\n', '\r'])
            .starts_with("data:");
    if content_type.contains("event-stream") || looks_sse {
        return body
            .split('\n')
            .filter_map(|line| line.strip_prefix("data:"))
            .find_map(|data| try_parse_object(data.trim()));
    }
    try_parse_object(body)
}

fn try_parse_object(text: &str) -> Option<Map<String, Value>> {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => Some(map),
        _ => None,
    }
}

/// The JSON-RPC error code in a parsed envelope, if it carries one.
pub fn json_rpc_error_code(rpc: Option<&Map<String, Value>>) -> Option<i64> {
    rpc?.get("error")?.get("code")?.as_i64()
}

/// Session id from the wave-1 `initialize` outcome.
pub fn session_id_from(outcome: Option<&ProbeOutcome>) -> Option<String> {
    let raw = outcome?.evidence.first()?.get("session_id")?.as_str()?;
    (!raw.is_empty()).then(|| raw.to_string())
}

/// Capability groups a handshake evidence row advertised.
pub fn advertised_capabilities(items: &[EvidenceItem]) -> Vec<String> {
    items
        .first()
        .and_then(|item| item.get("capabilities"))
        .and_then(Value::as_array)
        .map(|caps| {
            caps.iter()
                .filter_map(|c| c.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Whether a `capabilities` evidence row advertises the resources group.
pub fn advertises_resources(items: &[EvidenceItem]) -> bool {
    advertised_capabilities(items)
        .iter()
        .any(|group| group == "resources")
}

/// Modern-lane presence from the wave-1 `server/discover` outcome.
pub fn modern_lane_from(outcome: Option<&ProbeOutcome>) -> McpModernLane {
    match outcome {
        None => McpModernLane::Unknown,
        Some(o)
            if o.status == super::types::ProbeStatus::Error
                || o.status == super::types::ProbeStatus::Na =>
        {
            McpModernLane::Unknown
        }
        Some(o) => {
            if o.evidence
                .iter()
                .any(|item| item.contains_key("supported_versions"))
            {
                McpModernLane::Present
            } else {
                McpModernLane::Unevidenced
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web_audit::headers::Headers;

    fn resp(content_type: &str, body: &str) -> ProbeResponse {
        ProbeResponse {
            status: Some(200),
            headers: Headers::from_pairs([("content-type", content_type)]),
            body: body.to_string(),
            error: None,
            elapsed_ms: 0,
            truncated: false,
        }
    }

    #[test]
    fn bodies_are_the_site_bytes_key_order_included() {
        assert_eq!(
            legacy_tools_list_body(),
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#
        );
        assert_eq!(
            legacy_initialize_body("2025-06-18"),
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"agent-web-audit","version":"1.0"}}}"#
        );
        assert_eq!(
            modern_probe_body("tools/list"),
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientInfo":{"name":"agent-web-audit","version":"1.0"},"io.modelcontextprotocol/clientCapabilities":{}}}}"#
        );
        assert_eq!(
            initialized_body(),
            r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#
        );
    }

    #[test]
    fn json_rpc_parses_plain_json_and_the_first_sse_data_line() {
        let plain = resp(
            "application/json",
            r#"{"jsonrpc":"2.0","id":1,"result":{}}"#,
        );
        assert!(parse_json_rpc(&plain).unwrap().contains_key("result"));
        let sse = resp(
            "text/event-stream",
            "event: message\ndata: not json\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{\"code\":-32601}}\n\n",
        );
        let parsed = parse_json_rpc(&sse).unwrap();
        assert_eq!(json_rpc_error_code(Some(&parsed)), Some(-32601));
        let shaped = resp(
            "text/plain",
            "data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"tools\":[]}}",
        );
        assert!(parse_json_rpc(&shaped).is_some());
        assert!(parse_json_rpc(&resp("application/json", "[1,2]")).is_none());
        assert!(parse_json_rpc(&resp("text/html", "<html>")).is_none());
    }

    #[test]
    fn wave_one_readers_read_the_first_evidence_row() {
        let mut item = EvidenceItem::new();
        item.insert("session_id".into(), Value::String("abc".into()));
        item.insert("capabilities".into(), json!(["tools", "resources"]));
        let outcome = ProbeOutcome::new(super::super::types::ProbeStatus::Pass, vec![item]);
        assert_eq!(session_id_from(Some(&outcome)).as_deref(), Some("abc"));
        assert!(advertises_resources(&outcome.evidence));
        assert_eq!(modern_lane_from(Some(&outcome)), McpModernLane::Unevidenced);
        let mut modern = EvidenceItem::new();
        modern.insert("supported_versions".into(), json!(["2026-07-28"]));
        let discover = ProbeOutcome::new(super::super::types::ProbeStatus::Pass, vec![modern]);
        assert_eq!(modern_lane_from(Some(&discover)), McpModernLane::Present);
        assert_eq!(modern_lane_from(None), McpModernLane::Unknown);
        assert_eq!(
            modern_lane_from(Some(&ProbeOutcome::error("x"))),
            McpModernLane::Unknown
        );
        assert_eq!(
            session_id_from(Some(&ProbeOutcome::na("no endpoint"))),
            None
        );
    }
}
