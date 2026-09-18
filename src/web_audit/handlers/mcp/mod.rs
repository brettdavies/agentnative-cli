//! The `mcp` handler, a mirror of the site's `handlers/mcp.ts`. Builds
//! the JSON-RPC payload per op (legacy initialize / tools-list /
//! resources-list / error under the registry's pinned protocol version;
//! the modern header-routed rows; the per-era error-code conformance
//! family; the Accept-negotiation family), POSTs it through the guard,
//! parses JSON or SSE, and evaluates the answer. Returns n_a when no
//! endpoint was discovered, and an unprobed `absent` when wave 1
//! evidenced no modern lane.
//!
//! Responses are read as the site reads them: through the capped body
//! reader to the shared probe cap, the server closing, or the per-check
//! timeout, and only then parsed. There is no early return on an id
//! match, so a server that keeps its stream open times out here exactly
//! as it does on anc.dev.

pub mod negotiation;
pub mod ops;
pub mod probes;

use std::time::Instant;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use self::negotiation::{framing_of, is_json_rpc_envelope, negotiation_for};
use self::ops::{Era, Family, McpOp};
use self::probes::{TYPED_REFUSAL_STATUSES, conformance_for};
use super::shared::{item, why};
use crate::web_audit::engine::mcp_wire::{
    json_rpc_error_code, legacy_initialize_body, legacy_probe_headers, legacy_tools_list_body,
    modern_probe_body, modern_probe_headers, parse_json_rpc,
};
use crate::web_audit::engine::{HandlerContext, McpModernLane, ProbeOutcome, ProbeStatus};
use crate::web_audit::fetch::body::AUDIT_PROBE_MAX_BODY_BYTES;
use crate::web_audit::fetch::{FetchInit, FetchOptions};
use crate::web_audit::registry::WebCheck;
use crate::web_audit::scorecard::EvidenceItem;

/// The `with` block of an `mcp` check.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct McpWith {
    /// The op, by registry spelling.
    pub op: String,
    /// `capabilities` to judge initialize on its capabilities object.
    pub assert: Option<String>,
    /// Wire method for the `error` op.
    pub method: Option<String>,
    /// The refusal code the `error` op expects.
    pub expect_code: Option<i64>,
    /// Per-request timeout in seconds.
    pub timeout: Option<f64>,
}

const SERVER_INFO_META_KEY: &str = "io.modelcontextprotocol/serverInfo";

/// Codes that signal the probed era lane is not offered: method not
/// found, and unsupported protocol version. They read as unavailability
/// only on an op that names a method the lane could be missing.
const LANE_UNAVAILABLE_CODES: [i64; 2] = [-32601, -32022];
/// A rate-limit refusal measures the auditor's own request volume, so it
/// is an operational condition rather than a penalty.
const RATE_LIMITED_CODE: i64 = -32099;
/// JSON-RPC's reserved generic server error, whose session-required
/// meaning is one reading among many.
const SESSION_REQUIRED_CODE: i64 = -32000;
/// Statuses whose shape is "not now" rather than "not here".
const RETRY_SHAPED_STATUSES: [u16; 2] = [408, 429];

fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => true,
    }
}

/// `value ?? null`.
fn or_null(value: Option<&Value>) -> Value {
    value.cloned().unwrap_or(Value::Null)
}

fn object_keys(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_object)
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default()
}

fn build_body(op: McpOp, method: &str, protocol_version: &str) -> String {
    match op {
        McpOp::Initialize => legacy_initialize_body(protocol_version),
        McpOp::ToolsList => legacy_tools_list_body(),
        McpOp::ResourcesList => {
            json!({ "jsonrpc": "2.0", "id": 1, "method": "resources/list", "params": {} })
                .to_string()
        }
        _ => match op.spec().method {
            Some(modern_method) => modern_probe_body(modern_method),
            None => {
                json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": {} }).to_string()
            }
        },
    }
}

/// Whether the row reads a capability its own lane advertised one request
/// earlier. Refusing a method named in your own handshake contradicts that
/// handshake, so the era softening must not reach such a row.
fn refuses_own_advertisement(op: McpOp, ctx: &HandlerContext) -> bool {
    let spec = op.spec();
    let Some(group) = spec.advertises else {
        return false;
    };
    let advertised = if spec.era == Era::Modern {
        &ctx.mcp_lanes.modern_advertised
    } else {
        &ctx.mcp_lanes.legacy_advertised
    };
    advertised.iter().any(|g| g == group)
}

/// A lane's own era probe answered with an unavailability code is
/// reporting an era it does not serve, unless the lane advertised the very
/// capability it is refusing.
fn era_lane_unavailable(op: McpOp, code: Option<i64>, ctx: &HandlerContext) -> bool {
    match code {
        Some(code) if LANE_UNAVAILABLE_CODES.contains(&code) => !refuses_own_advertisement(op, ctx),
        _ => false,
    }
}

/// Whether a status is compatible with reading an answer as an era signal
/// at all: a 5xx and a retry-shaped status both describe the target's
/// condition rather than its protocol surface.
fn era_readable_status(status: Option<u16>) -> bool {
    match status {
        None => true,
        Some(s) => s < 500 && !RETRY_SHAPED_STATUSES.contains(&s),
    }
}

/// Whether a `server/discover` answer reports the modern lane's absence.
/// An error envelope is judged on its code; only an envelope-free answer
/// is judged on its status. -32000 counts only where its session-required
/// reading is coherent.
fn modern_lane_refused(status: Option<u16>, code: Option<i64>) -> bool {
    match code {
        Some(SESSION_REQUIRED_CODE) => era_readable_status(status),
        Some(code) => LANE_UNAVAILABLE_CODES.contains(&code),
        None => status.is_some_and(|s| TYPED_REFUSAL_STATUSES.contains(&s)),
    }
}

fn code_text(code: Option<i64>) -> String {
    code.map_or("null".to_string(), |c| c.to_string())
}

fn outcome(status: ProbeStatus, ev: EvidenceItem) -> ProbeOutcome {
    ProbeOutcome::new(status, vec![ev])
}

/// Probe the discovered endpoint with the op's request and classify the answer.
pub fn run_mcp(check: &WebCheck, ctx: &HandlerContext) -> ProbeOutcome {
    let Some(endpoint) = ctx.mcp_endpoint.as_deref().filter(|e| !e.is_empty()) else {
        return ProbeOutcome::na("no MCP endpoint discovered");
    };
    let w: McpWith = serde_json::from_str(check.with_json).unwrap_or_default();
    let Some(op) = McpOp::parse(&w.op) else {
        return ProbeOutcome::error(&format!(
            "mcp: check \"{}\" names unknown op {:?}",
            check.id, w.op
        ));
    };
    // The lane is not there, and an agent reaching for it finds nothing,
    // so the row occupies its slot at zero credit like any other absence.
    // It is marked unprobed because no request was sent.
    if op.modern_lane_dependent() && ctx.mcp_lanes.modern == McpModernLane::Unevidenced {
        return ProbeOutcome {
            unprobed: true,
            ..ProbeOutcome::new(
                ProbeStatus::Absent,
                vec![item([
                    ("url", json!(endpoint)),
                    (
                        "why",
                        why(&["no modern lane: server/discover returned no result"]),
                    ),
                ])],
            )
        };
    }
    let spec = op.spec();
    let conformance = conformance_for(op);
    let negotiation = negotiation_for(op);
    // Modern probes stay sessionless and carry no Mcp-Name. Conformance
    // probes open fully table-shaped, and only the legacy three re-ask
    // with a session. A negotiation probe sends the lane's proven
    // tools/list under a different Accept.
    let mut headers: Vec<(String, String)> = if let Some(n) = &negotiation {
        vec![
            (
                "Content-Type".to_string(),
                negotiation::JSON_MEDIA_TYPE.to_string(),
            ),
            ("Accept".to_string(), n.accept.to_string()),
        ]
    } else if let Some(c) = &conformance {
        c.headers.clone()
    } else if let Some(method) = spec.method {
        modern_probe_headers(method)
    } else {
        legacy_probe_headers()
    };
    // The session rides the legacy era rows, which are the ones that ask
    // a lane for a result it may hold behind a session.
    let session = ctx.mcp_session_id.as_deref().filter(|s| !s.is_empty());
    if spec.family == Family::Era
        && spec.era == Era::Legacy
        && op != McpOp::Initialize
        && let Some(session) = session
    {
        headers.push(("Mcp-Session-Id".to_string(), session.to_string()));
    }
    let body = if negotiation.is_some() {
        legacy_tools_list_body()
    } else if let Some(c) = &conformance {
        c.body.clone()
    } else {
        build_body(op, w.method.as_deref().unwrap_or(""), ctx.protocol_version)
    };
    // Every MCP probe reads at most a small JSON-RPC envelope, so the
    // shared probe cap bounds what a hostile endpoint can make the auditor
    // buffer.
    let timeout = ctx.timeout_for(w.timeout);
    let opts = FetchOptions {
        timeout,
        max_body_bytes: Some(AUDIT_PROBE_MAX_BODY_BYTES),
        ..FetchOptions::default()
    };
    // The re-ask is a second hop on one row's budget.
    let deadline_at = Instant::now() + timeout;
    let post = |headers: Vec<(String, String)>, opts: &FetchOptions| {
        ctx.fetch.fetch(
            endpoint,
            &FetchInit {
                method: "POST".to_string(),
                headers,
                body: Some(body.clone().into_bytes()),
            },
            opts,
        )
    };

    let mut resp = post(headers.clone(), &opts);
    let mut rpc = parse_json_rpc(&resp);
    if op.legacy_conformance()
        && let Some(session) = session
        && json_rpc_error_code(rpc.as_ref()) == Some(SESSION_REQUIRED_CODE)
    {
        let slice = deadline_at.saturating_duration_since(Instant::now());
        if !slice.is_zero() {
            let mut with_session = headers.clone();
            with_session.push(("Mcp-Session-Id".to_string(), session.to_string()));
            resp = post(
                with_session,
                &FetchOptions {
                    timeout: slice,
                    ..opts.clone()
                },
            );
            rpc = parse_json_rpc(&resp);
        }
    }

    let mut ev = item([
        ("url", json!(endpoint)),
        ("status", json!(resp.status)),
        ("error", json!(resp.error)),
    ]);
    if let Some(challenge) = resp.headers.get("www-authenticate") {
        ev.insert("www_authenticate".to_string(), json!(challenge));
    }
    let set_why = |ev: &mut EvidenceItem, line: &str| {
        ev.insert("why".to_string(), why(&[line]));
    };

    if resp.error.as_deref().is_some_and(|e| !e.is_empty()) {
        set_why(&mut ev, "request failed");
        return outcome(ProbeStatus::Error, ev);
    }

    let code = json_rpc_error_code(rpc.as_ref());
    if code == Some(RATE_LIMITED_CODE) {
        ev.insert("error_code".to_string(), json!(RATE_LIMITED_CODE));
        set_why(&mut ev, "rate limited by the target");
        return outcome(ProbeStatus::Error, ev);
    }

    // Settled ahead of the arms below because a legacy server declines
    // this method both with an envelope and without one.
    if spec.discriminates && modern_lane_refused(resp.status, code) {
        let reason = match code {
            Some(code) => format!("code {code}"),
            None => format!(
                "HTTP {}",
                resp.status.map_or("null".to_string(), |s| s.to_string())
            ),
        };
        if let Some(code) = code {
            ev.insert("error_code".to_string(), json!(code));
        }
        set_why(
            &mut ev,
            &format!("no modern lane: server/discover refused with {reason}"),
        );
        return outcome(ProbeStatus::Absent, ev);
    }

    // A negotiation row reads the framing of whatever came back.
    if let Some(n) = &negotiation {
        let framing = framing_of(&resp, is_json_rpc_envelope(rpc.as_ref()));
        ev.insert("accept".to_string(), json!(n.accept));
        ev.insert("content_type".to_string(), json!(framing.media_type));
        let verdict = n.classify(&framing);
        if let Some(line) = &verdict.why {
            set_why(&mut ev, line);
        }
        return outcome(verdict.status, ev);
    }

    // A typed refusal is the status code plus the absence of a JSON-RPC
    // error envelope, not a parse failure. 404 stays out of every arm so
    // a dead endpoint earns nothing.
    let typed_http_refusal = conformance
        .as_ref()
        .is_some_and(|c| resp.status.is_some_and(|s| c.http_accept.contains(&s)));

    // The endpoint exists, so a response that carries no parseable
    // JSON-RPC is a broken surface, not an absent one.
    let Some(rpc) = rpc else {
        if typed_http_refusal {
            set_why(&mut ev, "typed HTTP refusal with no JSON-RPC envelope");
            return outcome(ProbeStatus::Pass, ev);
        }
        set_why(&mut ev, "no parseable JSON-RPC response");
        return outcome(ProbeStatus::Broken, ev);
    };

    // Conformance classification: the expected refusal code passes. An
    // envelope-shaped refusal carrying the wrong code still tells the
    // agent the call failed, so it is a taxonomy defect rather than a
    // trap; a result where a refusal was required, or an envelope with no
    // numeric code, leaves the agent believing something untrue.
    if let Some(c) = &conformance {
        ev.insert("error_code".to_string(), json!(code));
        let Some(code) = code else {
            if typed_http_refusal {
                set_why(&mut ev, "typed HTTP refusal with no JSON-RPC envelope");
                return outcome(ProbeStatus::Pass, ev);
            }
            set_why(
                &mut ev,
                if rpc.contains_key("error") {
                    "the JSON-RPC error envelope carries no numeric error.code"
                } else {
                    "expected a JSON-RPC error envelope, got a result"
                },
            );
            return outcome(ProbeStatus::Broken, ev);
        };
        if c.accept.contains(&code) {
            if c.require_supported {
                let supported = rpc
                    .get("error")
                    .and_then(|e| e.get("data"))
                    .and_then(|d| d.get("supported"))
                    .filter(|s| s.is_array());
                let Some(supported) = supported else {
                    // The refusal itself is correct and well-formed; the
                    // client simply cannot read what to renegotiate to.
                    set_why(
                        &mut ev,
                        "error.data.supported missing from the version-reject envelope",
                    );
                    return outcome(ProbeStatus::Noncompliant, ev);
                };
                ev.insert("supported_versions".to_string(), supported.clone());
            }
            return outcome(ProbeStatus::Pass, ev);
        }
        let accepted = c
            .accept
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(" or ");
        set_why(
            &mut ev,
            &format!("expected error code {accepted}, got {code}"),
        );
        return outcome(ProbeStatus::Noncompliant, ev);
    }

    // Era-lane classification for the ops that ask a lane about itself.
    // Each names a method the lane could be missing, so an
    // unavailability-coded refusal answers the probe truthfully and reads
    // absent.
    if rpc.contains_key("error") {
        ev.insert("error_code".to_string(), json!(code));
        let expected = w.expect_code.unwrap_or(-32601);
        if spec.expects_refusal {
            if code == Some(expected) {
                return outcome(ProbeStatus::Pass, ev);
            }
            if era_lane_unavailable(op, code, ctx) {
                return outcome(ProbeStatus::Absent, ev);
            }
            set_why(
                &mut ev,
                &format!("expected error code {expected}, got {}", code_text(code)),
            );
            return outcome(ProbeStatus::Noncompliant, ev);
        }
        let status = if era_lane_unavailable(op, code, ctx) {
            ProbeStatus::Absent
        } else {
            ProbeStatus::Broken
        };
        return outcome(status, ev);
    }

    let empty = Map::new();
    let result = rpc
        .get("result")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let ok = match op {
        McpOp::Initialize => {
            let server_info = result.get("serverInfo");
            let capabilities = result.get("capabilities").filter(|c| !c.is_null());
            ev.insert("serverInfo".to_string(), or_null(server_info));
            ev.insert(
                "protocolVersion".to_string(),
                or_null(result.get("protocolVersion")),
            );
            ev.insert("capabilities".to_string(), json!(object_keys(capabilities)));
            ev.insert(
                "session_id".to_string(),
                json!(resp.headers.get("mcp-session-id")),
            );
            if w.assert.as_deref() == Some("capabilities") {
                truthy(capabilities) && !object_keys(capabilities).is_empty()
            } else {
                truthy(server_info.and_then(|s| s.get("name")))
            }
        }
        McpOp::ToolsList | McpOp::ModernToolsList => {
            let tools = result.get("tools").and_then(Value::as_array);
            ev.insert(
                "tools".to_string(),
                tools.map_or(Value::Null, |t| {
                    Value::Array(t.iter().map(|tool| or_null(tool.get("name"))).collect())
                }),
            );
            ev.insert(
                "with_input_schema".to_string(),
                json!(tools.map_or(0, |t| {
                    t.iter()
                        .filter(|tool| truthy(tool.get("inputSchema")))
                        .count()
                })),
            );
            tools.is_some()
        }
        McpOp::ServerDiscover => {
            let supported = result.get("supportedVersions").filter(|s| s.is_array());
            let server_info = result
                .get("_meta")
                .and_then(|m| m.get(SERVER_INFO_META_KEY));
            let capabilities = result.get("capabilities").filter(|c| !c.is_null());
            ev.insert("supported_versions".to_string(), or_null(supported));
            ev.insert("serverInfo".to_string(), or_null(server_info));
            // The modern lane's capability advertisement; the mcp-resources
            // antecedent reads it alongside the legacy initialize evidence.
            ev.insert("capabilities".to_string(), json!(object_keys(capabilities)));
            supported.is_some() && truthy(server_info.and_then(|s| s.get("name")))
        }
        McpOp::ResourcesList => {
            let resources = result.get("resources").and_then(Value::as_array);
            ev.insert(
                "resources".to_string(),
                resources.map_or(Value::Null, |r| {
                    Value::Array(
                        r.iter()
                            .map(|res| {
                                res.get("uri")
                                    .filter(|v| !v.is_null())
                                    .or_else(|| res.get("name"))
                                    .cloned()
                                    .unwrap_or(Value::Null)
                            })
                            .collect(),
                    )
                }),
            );
            resources.is_some_and(|r| !r.is_empty())
        }
        _ => {
            ev.insert("error_code".to_string(), json!(code));
            code == Some(w.expect_code.unwrap_or(-32601))
        }
    };
    outcome(
        if ok {
            ProbeStatus::Pass
        } else {
            ProbeStatus::Broken
        },
        ev,
    )
}
