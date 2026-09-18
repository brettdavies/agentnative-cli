//! The Accept-negotiation rows, judged on how the answer was framed
//! (status plus content-type) rather than on a JSON-RPC code, because
//! their question is whether the server honoured the Accept it was sent.

use serde_json::{Map, Value};

use super::ops::McpOp;
use crate::web_audit::engine::ProbeStatus;
use crate::web_audit::fetch::ProbeResponse;

/// The one status whose meaning is exactly "nothing you listed in Accept
/// is available here".
pub const NOT_ACCEPTABLE_STATUS: u16 = 406;
/// An Accept no MCP transport can satisfy.
pub const UNSATISFIABLE_ACCEPT: &str = "application/xml";
/// The stream media type.
pub const SSE_MEDIA_TYPE: &str = "text/event-stream";
/// The JSON media type.
pub const JSON_MEDIA_TYPE: &str = "application/json";

use super::probes::TYPED_REFUSAL_STATUSES;

/// How the answer was framed, which is all a negotiation row reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Framing {
    /// HTTP status, or `None` when the request failed.
    pub status: Option<u16>,
    /// Response media type, parameters stripped and lowercased.
    pub media_type: String,
    /// The body carries a JSON-RPC envelope, not merely some JSON object.
    pub envelope: bool,
}

/// A negotiation row's verdict and its line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NegotiationVerdict {
    /// Pass, noncompliant or broken.
    pub status: ProbeStatus,
    /// The evidence line, when the verdict carries one.
    pub why: Option<String>,
}

fn verdict(status: ProbeStatus, why: Option<String>) -> NegotiationVerdict {
    NegotiationVerdict { status, why }
}

/// Whether the parsed body is a JSON-RPC envelope rather than any JSON
/// object, so a framework's JSON 404 page does not read as the server
/// honouring the Accept.
pub fn is_json_rpc_envelope(rpc: Option<&Map<String, Value>>) -> bool {
    rpc.is_some_and(|r| {
        r.get("jsonrpc").and_then(Value::as_str) == Some("2.0")
            && (r.contains_key("result") || r.contains_key("error"))
    })
}

/// The framing of a response.
pub fn framing_of(resp: &ProbeResponse, envelope: bool) -> Framing {
    let media_type = resp
        .headers
        .get("content-type")
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    Framing {
        status: resp.status,
        media_type,
        envelope,
    }
}

fn classify_accept_json(f: &Framing) -> NegotiationVerdict {
    // Naming only application/json tells the server the caller's parser
    // cannot read a stream. Streaming anyway hands it a success line over
    // a body it declared it cannot consume, and no status says so.
    if f.media_type == SSE_MEDIA_TYPE {
        return verdict(
            ProbeStatus::Broken,
            Some(format!(
                "answered {SSE_MEDIA_TYPE} to a client that accepts only {JSON_MEDIA_TYPE}"
            )),
        );
    }
    // Judged on the framing alone: whether the envelope carries a result
    // or an error is another row's question.
    if f.envelope && f.media_type == JSON_MEDIA_TYPE {
        return verdict(ProbeStatus::Pass, None);
    }
    // A stream-only server that says so serves no JSON-only caller, but
    // it misleads none either: the refusal is immediate and typed.
    if f.status == Some(NOT_ACCEPTABLE_STATUS) {
        return verdict(
            ProbeStatus::Noncompliant,
            Some(format!(
                "refused {JSON_MEDIA_TYPE} with {NOT_ACCEPTABLE_STATUS} instead of serving it"
            )),
        );
    }
    let served = if f.media_type.is_empty() {
        "no content-type"
    } else {
        f.media_type.as_str()
    };
    verdict(
        ProbeStatus::Broken,
        Some(format!(
            "no JSON-RPC envelope in {JSON_MEDIA_TYPE} (served {served})"
        )),
    )
}

fn classify_accept_unsatisfiable(f: &Framing) -> NegotiationVerdict {
    if f.status == Some(NOT_ACCEPTABLE_STATUS) {
        return verdict(ProbeStatus::Pass, None);
    }
    if let Some(status) = f.status.filter(|s| (200..300).contains(s)) {
        // Echoing the unservable type back over a JSON-RPC body is the lie
        // this row exists to catch.
        if f.media_type == UNSATISFIABLE_ACCEPT && f.envelope {
            return verdict(
                ProbeStatus::Broken,
                Some(format!(
                    "claimed {UNSATISFIABLE_ACCEPT} over a JSON-RPC body"
                )),
            );
        }
        // Serving an unasked-for type under an honest label ignores the
        // negotiation without misrepresenting anything.
        let served = if f.media_type.is_empty() {
            "an unlabelled body"
        } else {
            f.media_type.as_str()
        };
        return verdict(
            ProbeStatus::Noncompliant,
            Some(format!(
                "served {served} at {status} where {NOT_ACCEPTABLE_STATUS} was required"
            )),
        );
    }
    // 404 stays outside this arm, as it does everywhere else, so a dead
    // endpoint earns nothing on this row.
    if let Some(status) = f.status.filter(|s| TYPED_REFUSAL_STATUSES.contains(s)) {
        return verdict(
            ProbeStatus::Noncompliant,
            Some(format!(
                "refused with {status} where {NOT_ACCEPTABLE_STATUS} was required"
            )),
        );
    }
    let http = f.status.map_or("none".to_string(), |s| s.to_string());
    verdict(
        ProbeStatus::Broken,
        Some(format!(
            "no deliberate answer to an unsatisfiable Accept (HTTP {http})"
        )),
    )
}

/// A negotiation row: the Accept it sends, and how it reads the framing.
#[derive(Clone, Copy, Debug)]
pub struct NegotiationProbe {
    /// The Accept header value.
    pub accept: &'static str,
    classify: fn(&Framing) -> NegotiationVerdict,
}

impl NegotiationProbe {
    /// Classify a framing.
    pub fn classify(&self, framing: &Framing) -> NegotiationVerdict {
        (self.classify)(framing)
    }
}

/// The negotiation probe for a framed op, or `None` for any other op.
pub fn negotiation_for(op: McpOp) -> Option<NegotiationProbe> {
    match op {
        McpOp::AcceptJson => Some(NegotiationProbe {
            accept: JSON_MEDIA_TYPE,
            classify: classify_accept_json,
        }),
        McpOp::AcceptUnsatisfiable => Some(NegotiationProbe {
            accept: UNSATISFIABLE_ACCEPT,
            classify: classify_accept_unsatisfiable,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn framing(status: Option<u16>, media_type: &str, envelope: bool) -> Framing {
        Framing {
            status,
            media_type: media_type.to_string(),
            envelope,
        }
    }

    #[test]
    fn accept_json_reads_the_framing_alone() {
        let json = negotiation_for(McpOp::AcceptJson).unwrap();
        assert_eq!(
            json.classify(&framing(Some(200), "application/json", true)),
            verdict(ProbeStatus::Pass, None)
        );
        assert_eq!(
            json.classify(&framing(Some(200), "text/event-stream", true))
                .status,
            ProbeStatus::Broken
        );
        assert_eq!(
            json.classify(&framing(Some(406), "text/plain", false)),
            verdict(
                ProbeStatus::Noncompliant,
                Some("refused application/json with 406 instead of serving it".to_string())
            )
        );
        assert_eq!(
            json.classify(&framing(Some(200), "", false)).why.as_deref(),
            Some("no JSON-RPC envelope in application/json (served no content-type)")
        );
    }

    #[test]
    fn accept_unsatisfiable_credits_only_a_406() {
        let xml = negotiation_for(McpOp::AcceptUnsatisfiable).unwrap();
        assert_eq!(
            xml.classify(&framing(Some(406), "", false)).status,
            ProbeStatus::Pass
        );
        assert_eq!(
            xml.classify(&framing(Some(200), "application/xml", true))
                .why
                .as_deref(),
            Some("claimed application/xml over a JSON-RPC body")
        );
        assert_eq!(
            xml.classify(&framing(Some(200), "application/json", true))
                .why
                .as_deref(),
            Some("served application/json at 200 where 406 was required")
        );
        assert_eq!(
            xml.classify(&framing(Some(415), "", false)).why.as_deref(),
            Some("refused with 415 where 406 was required")
        );
        assert_eq!(
            xml.classify(&framing(Some(404), "", false)).why.as_deref(),
            Some("no deliberate answer to an unsatisfiable Accept (HTTP 404)")
        );
        assert_eq!(
            xml.classify(&framing(None, "", false)).why.as_deref(),
            Some("no deliberate answer to an unsatisfiable Accept (HTTP none)")
        );
    }

    #[test]
    fn an_envelope_needs_the_version_and_a_result_or_error() {
        let rpc = |text: &str| serde_json::from_str::<Map<String, Value>>(text).unwrap();
        assert!(is_json_rpc_envelope(Some(&rpc(
            r#"{"jsonrpc":"2.0","result":{}}"#
        ))));
        assert!(is_json_rpc_envelope(Some(&rpc(
            r#"{"jsonrpc":"2.0","error":null}"#
        ))));
        assert!(!is_json_rpc_envelope(Some(&rpc(r#"{"jsonrpc":"2.0"}"#))));
        assert!(!is_json_rpc_envelope(Some(&rpc(r#"{"status":404}"#))));
        assert!(!is_json_rpc_envelope(None));
    }
}
