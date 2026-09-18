//! The per-era conformance probes: one request each, era-shaped headers,
//! and a raw body where the probe is the body itself (malformed, batch).
//! Codes -32603 and -32099 are deliberately not probed: an internal error
//! cannot be forced from outside, and triggering rate limits against
//! third-party servers is abusive.

use serde_json::{Value, json};

use super::ops::McpOp;
use crate::web_audit::engine::mcp_wire::{
    MODERN_PROTOCOL_VERSION, legacy_probe_headers, modern_meta, modern_probe_body,
    modern_probe_headers,
};

/// A refusal delivered as an HTTP status with no JSON-RPC envelope: the
/// request was rejected, not mishandled. 404 is deliberately outside the
/// set so a dead endpoint earns nothing on any row that consults it.
pub const TYPED_REFUSAL_STATUSES: [u16; 2] = [400, 415];

/// A protocol revision that predates the modern lane, so every conforming
/// server must refuse it with -32022 and `data.supported`.
pub const UNSUPPORTED_VERSION_CLAIM: &str = "2025-03-26";
/// A tool name outside any real catalog.
pub const UNKNOWN_TOOL_NAME: &str = "not_a_real_tool";
/// A method no server implements.
pub const UNKNOWN_METHOD: &str = "nonexistent/method";
/// A resource URI no server holds.
pub const RESOURCE_MISS_URI: &str = "resource://agent-web-audit/nonexistent";

/// One conformance probe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConformanceProbe {
    /// Request headers.
    pub headers: Vec<(String, String)>,
    /// Request body, verbatim.
    pub body: String,
    /// JSON-RPC error codes that satisfy the row.
    pub accept: &'static [i64],
    /// Bare HTTP statuses that satisfy the row when no envelope came back.
    pub http_accept: &'static [u16],
    /// The accepted envelope must carry `error.data.supported`.
    pub require_supported: bool,
}

fn envelope(method: &str, params: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }).to_string()
}

/// The probe for a code-judged conformance op, or `None` for any other op.
pub fn conformance_for(op: McpOp) -> Option<ConformanceProbe> {
    let probe = match op {
        McpOp::MalformedBody => ConformanceProbe {
            headers: legacy_probe_headers(),
            body: "not-json{{".to_string(),
            accept: &[-32700],
            http_accept: &TYPED_REFUSAL_STATUSES,
            require_supported: false,
        },
        // A valid all-legacy batch is served on the pinned SDK line; the
        // reliably rejected shape is a batch carrying a modern-envelope
        // element.
        McpOp::BatchReject => ConformanceProbe {
            headers: legacy_probe_headers(),
            body: format!("[{}]", modern_probe_body("tools/list")),
            accept: &[-32600],
            http_accept: &[],
            require_supported: false,
        },
        McpOp::UnknownTool => ConformanceProbe {
            headers: legacy_probe_headers(),
            body: envelope(
                "tools/call",
                json!({ "name": UNKNOWN_TOOL_NAME, "arguments": {} }),
            ),
            accept: &[-32602],
            http_accept: &[],
            require_supported: false,
        },
        McpOp::ModernUnknownMethod => ConformanceProbe {
            headers: modern_probe_headers(UNKNOWN_METHOD),
            body: modern_probe_body(UNKNOWN_METHOD),
            accept: &[-32601],
            http_accept: &[],
            require_supported: false,
        },
        McpOp::ModernClientcaps => {
            let mut meta = modern_meta(MODERN_PROTOCOL_VERSION);
            if let Some(map) = meta.as_object_mut() {
                map.remove("io.modelcontextprotocol/clientCapabilities");
            }
            ConformanceProbe {
                headers: modern_probe_headers("tools/list"),
                body: envelope("tools/list", json!({ "_meta": meta })),
                accept: &[-32602, -32600],
                http_accept: &[],
                require_supported: false,
            }
        }
        McpOp::ModernHeaderMismatch => ConformanceProbe {
            headers: modern_probe_headers("resources/list"),
            body: modern_probe_body("tools/list"),
            accept: &[-32020],
            http_accept: &[],
            require_supported: false,
        },
        McpOp::ModernVersionReject => {
            let mut headers = modern_probe_headers("tools/list");
            for (name, value) in &mut headers {
                if name == "MCP-Protocol-Version" {
                    *value = UNSUPPORTED_VERSION_CLAIM.to_string();
                }
            }
            ConformanceProbe {
                headers,
                body: envelope(
                    "tools/list",
                    json!({ "_meta": modern_meta(UNSUPPORTED_VERSION_CLAIM) }),
                ),
                accept: &[-32022],
                http_accept: &[],
                require_supported: true,
            }
        }
        // -32602 is the miss code at the SDK encode seam; -32002 is
        // receive-tolerated legacy compat from non-SDK servers.
        McpOp::ModernResourcesMiss => {
            let mut headers = modern_probe_headers("resources/read");
            headers.push(("Mcp-Name".to_string(), RESOURCE_MISS_URI.to_string()));
            ConformanceProbe {
                headers,
                body: envelope(
                    "resources/read",
                    json!({ "uri": RESOURCE_MISS_URI, "_meta": modern_meta(MODERN_PROTOCOL_VERSION) }),
                ),
                accept: &[-32602, -32002],
                http_accept: &[],
                require_supported: false,
            }
        }
        _ => return None,
    };
    Some(probe)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
        headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn probe_bodies_are_the_site_bytes() {
        let unknown_tool = conformance_for(McpOp::UnknownTool).unwrap();
        assert_eq!(
            unknown_tool.body,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"not_a_real_tool","arguments":{}}}"#
        );
        let batch = conformance_for(McpOp::BatchReject).unwrap();
        assert!(
            batch.body.starts_with(
                r#"[{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"#
            )
        );
        assert!(batch.body.ends_with("}}}]"));
        let clientcaps = conformance_for(McpOp::ModernClientcaps).unwrap();
        assert!(!clientcaps.body.contains("clientCapabilities"));
        assert!(
            clientcaps
                .body
                .contains("io.modelcontextprotocol/protocolVersion")
        );
        assert!(
            clientcaps
                .body
                .contains("io.modelcontextprotocol/clientInfo")
        );
        let mismatch = conformance_for(McpOp::ModernHeaderMismatch).unwrap();
        assert_eq!(
            header(&mismatch.headers, "mcp-method"),
            Some("resources/list")
        );
        assert!(mismatch.body.contains(r#""method":"tools/list""#));
        let version = conformance_for(McpOp::ModernVersionReject).unwrap();
        assert_eq!(
            header(&version.headers, "mcp-protocol-version"),
            Some(UNSUPPORTED_VERSION_CLAIM)
        );
        assert!(version.body.contains(&format!(
            r#""io.modelcontextprotocol/protocolVersion":"{UNSUPPORTED_VERSION_CLAIM}""#
        )));
        assert!(version.require_supported);
        let miss = conformance_for(McpOp::ModernResourcesMiss).unwrap();
        assert_eq!(header(&miss.headers, "mcp-name"), Some(RESOURCE_MISS_URI));
        assert!(miss.body.starts_with(&format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{{"uri":"{RESOURCE_MISS_URI}","_meta":{{"#
        )));
        assert_eq!(
            conformance_for(McpOp::MalformedBody).unwrap().body,
            "not-json{{"
        );
        assert_eq!(conformance_for(McpOp::AcceptJson), None);
        assert_eq!(conformance_for(McpOp::Initialize), None);
    }
}
