//! The `dns-doh` handler, a mirror of the site's `handlers/dns-doh.ts`.
//! Queries DNS-over-HTTPS (JSON API) for agent-discovery records. Passes
//! when any queried name returns `Status: 0` with a non-empty `Answer`;
//! NXDOMAIN is definitive for that name; the fallback resolver is tried
//! only on a resolver-level failure. The engine's locality gate decides
//! whether this handler runs at all for a local target.

use serde::Deserialize;
use serde_json::{Value, json};

use super::shared::{item, substitute_host};
use crate::web_audit::engine::{HandlerContext, ProbeOutcome, ProbeStatus};
use crate::web_audit::fetch::FetchInit;
use crate::web_audit::registry::WebCheck;
use crate::web_audit::scorecard::EvidenceItem;

/// The resolvers queried when the check names none.
pub const DEFAULT_RESOLVERS: [&str; 2] = [
    "https://cloudflare-dns.com/dns-query",
    "https://dns.google/resolve",
];

/// The `with` block of a `dns-doh` check.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct DnsDohWith {
    /// Record names to query, with `{host}` substituted.
    pub names: Vec<String>,
    /// Record type, `SVCB` by default.
    #[serde(rename = "type")]
    pub record_type: Option<String>,
    /// DoH JSON endpoints, in fallback order.
    pub resolvers: Option<Vec<String>>,
    /// Per-request timeout in seconds.
    pub timeout: Option<f64>,
}

/// `encodeURIComponent`: every byte percent-encoded except the unreserved
/// set JavaScript leaves alone.
pub fn encode_uri_component(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(byte as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// Query each name against the resolvers in order.
pub fn run_dns_doh(check: &WebCheck, ctx: &HandlerContext) -> ProbeOutcome {
    if ctx.host.is_empty() {
        return ProbeOutcome::na("no host in URL");
    }
    let w: DnsDohWith = serde_json::from_str(check.with_json).unwrap_or_default();
    let names: Vec<String> = w
        .names
        .iter()
        .map(|n| substitute_host(n, &ctx.host))
        .collect();
    let record_type = w.record_type.as_deref().unwrap_or("SVCB");
    let resolvers: Vec<String> = w
        .resolvers
        .clone()
        .unwrap_or_else(|| DEFAULT_RESOLVERS.iter().map(|r| (*r).to_string()).collect());
    let opts = ctx.fetch_options(w.timeout);
    let init = FetchInit {
        headers: vec![("Accept".to_string(), "application/dns-json".to_string())],
        ..FetchInit::default()
    };

    let mut evidence: Vec<EvidenceItem> = Vec::new();
    for name in &names {
        for resolver in &resolvers {
            let url = format!(
                "{resolver}?name={}&type={}",
                encode_uri_component(name),
                encode_uri_component(record_type)
            );
            let resp = ctx.fetch.fetch(&url, &init, &opts);
            let data: Option<Value> = serde_json::from_str(&resp.body).ok();
            let failed = resp.error.as_deref().is_some_and(|e| !e.is_empty());
            // A resolver-level failure tries the fallback resolver.
            let Some(data) = data.filter(|_| !failed) else {
                continue;
            };
            let answers = data
                .get("Answer")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            let status = data.get("Status").cloned().unwrap_or(Value::Null);
            let ok = status.as_f64() == Some(0.0) && answers > 0;
            evidence.push(item([
                ("name", json!(name)),
                ("resolver", json!(resolver)),
                ("dns_status", status),
                ("answers", json!(answers)),
            ]));
            if ok {
                return ProbeOutcome::new(ProbeStatus::Pass, evidence);
            }
            // A definitive DNS answer moves to the next name.
            break;
        }
    }
    // Any definitive empty answer means the records are absent; nothing
    // definitive at all is an operational error.
    if !evidence.is_empty() {
        return ProbeOutcome::new(ProbeStatus::Absent, evidence);
    }
    ProbeOutcome::error("all DoH resolvers failed")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_component_encoding_matches_javascript() {
        assert_eq!(
            encode_uri_component("_agents.example.com"),
            "_agents.example.com"
        );
        assert_eq!(encode_uri_component("a b/c?d=é"), "a%20b%2Fc%3Fd%3D%C3%A9");
        assert_eq!(encode_uri_component("-_.!~*'()"), "-_.!~*'()");
    }
}
