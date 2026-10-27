//! The `protected-resource` handler, a mirror of the site's
//! `handlers/protected-resource.ts`: scores what an MCP endpoint that
//! requires sign-in tells a client about signing in, from the facts the
//! audit already settled, with no request of its own. The `challenge` op
//! reads the `WWW-Authenticate` its 401 carried; the `metadata` op reads
//! the authorization servers its RFC 9728 metadata lists.
//!
//! The audited server chooses those values, so none is ever requested (a
//! listed authorization server could be any host) and each reaches the
//! evidence line in a form no reader can take as markup.

use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

use crate::web_audit::engine::reciprocity_metadata::resource_metadata_from_challenge;
use crate::web_audit::engine::{HandlerContext, McpAuthRequired, ProbeOutcome, ProbeStatus};
use crate::web_audit::locality::{Locality, site_block_reason};
use crate::web_audit::pattern::fixed;
use crate::web_audit::registry::WebCheck;
use crate::web_audit::scorecard::EvidenceItem;

const MAX_AUTHORIZATION_SERVERS: usize = 8;
const MAX_URL_LENGTH: usize = 2048;
const MAX_SHOWN_LENGTH: usize = 200;

static BEARER_CHALLENGE: LazyLock<Regex> = LazyLock::new(|| fixed(r"(?i)(?:^|,)\s*bearer(?:\s|$)"));

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ProtectedResourceWith {
    op: Option<String>,
}

/// Every character a URL needs unescaped; anything else is percent-encoded,
/// which leaves no markup, link syntax, or line break for a reader to act on.
fn shown_as_is(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-._~:/?#@!$+,;=%".contains(c)
}

/// A server-chosen value as the evidence line shows it: as written, with
/// every character outside a URL's own set percent-encoded. Long values
/// are cut.
fn shown(value: &Value) -> String {
    let text = match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let mut encoded = String::new();
    for c in text.chars() {
        if shown_as_is(c) {
            encoded.push(c);
        } else {
            let mut buf = [0u8; 4];
            for byte in c.encode_utf8(&mut buf).bytes() {
                encoded.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    if encoded.len() > MAX_SHOWN_LENGTH {
        format!("{}...", &encoded[..MAX_SHOWN_LENGTH])
    } else {
        encoded
    }
}

fn verdict(status: ProbeStatus, item: EvidenceItem, why: &[String]) -> ProbeOutcome {
    let mut item = item;
    item.insert("why".to_string(), json!(why));
    ProbeOutcome::new(status, vec![item])
}

fn item(pairs: &[(&str, Value)]) -> EvidenceItem {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

fn challenge_outcome(auth: &McpAuthRequired) -> ProbeOutcome {
    let item = item(&[("url", json!(auth.endpoint)), ("status", json!(401))]);
    let fail = |why: &str| verdict(ProbeStatus::Noncompliant, item.clone(), &[why.to_string()]);
    let Some(challenge) = auth.challenge.as_deref() else {
        return fail("the 401 carries no WWW-Authenticate challenge");
    };
    if !BEARER_CHALLENGE.is_match(challenge) {
        return fail("the WWW-Authenticate challenge is not a Bearer challenge");
    }
    let Some(named) = resource_metadata_from_challenge(challenge) else {
        return fail("the Bearer challenge names no resource_metadata");
    };
    let why = format!("resource_metadata names {}", shown(&json!(named)));
    verdict(ProbeStatus::Pass, item, &[why])
}

fn host_of(url: &Url) -> &str {
    url.host_str().unwrap_or("")
}

/// Why a listed authorization server is unusable from the audit's vantage,
/// or `None` when an agent there can sign in through it. A private or
/// reserved host is out of reach unless the endpoint itself is: a client
/// that reached a private endpoint shares its network.
fn server_defect(value: &Value, private_reachable: bool) -> Option<String> {
    let Value::String(raw) = value else {
        return Some("is not a URL".to_string());
    };
    if raw.encode_utf16().count() > MAX_URL_LENGTH {
        return Some(format!("is longer than {MAX_URL_LENGTH} characters"));
    }
    let Some(url) = Url::parse(raw)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https"))
    else {
        return Some("is not a URL".to_string());
    };
    if site_block_reason(host_of(&url)).is_some() && !private_reachable {
        return Some("names a private or reserved host".to_string());
    }
    (url.scheme() != "https").then(|| "is not https".to_string())
}

/// A client at the audit's vantage reaches private hosts: the endpoint
/// sits on one, or the audit itself runs against a local target.
fn private_reachable(auth: &McpAuthRequired, ctx: &HandlerContext) -> bool {
    let endpoint_private = Url::parse(&auth.endpoint)
        .ok()
        .is_some_and(|u| site_block_reason(host_of(&u)).is_some());
    endpoint_private || ctx.target_locality == Some(Locality::Local)
}

/// Priced by what an agent at the audit's vantage can do with the list: no
/// usable server leaves it nowhere to sign in, a dead end, while a usable
/// one listed beside bad entries still gets it signed in.
fn metadata_outcome(auth: &McpAuthRequired, ctx: &HandlerContext) -> ProbeOutcome {
    let item = item(&[("url", json!(auth.metadata_url))]);
    let servers = match auth.metadata.get("authorization_servers") {
        Some(Value::Array(servers)) if !servers.is_empty() => servers,
        _ => {
            let why = "the metadata lists no authorization_servers".to_string();
            return verdict(ProbeStatus::Broken, item, &[why]);
        }
    };
    let reachable = private_reachable(auth, ctx);
    let read = &servers[..servers.len().min(MAX_AUTHORIZATION_SERVERS)];
    let defects: Vec<String> = read
        .iter()
        .filter_map(|value| {
            server_defect(value, reachable)
                .map(|defect| format!("authorization server {} {defect}", shown(value)))
        })
        .collect();
    if defects.is_empty() {
        let listed: Vec<String> = read.iter().map(shown).collect();
        let why = format!("authorization_servers: {}", listed.join(", "));
        return verdict(ProbeStatus::Pass, item, &[why]);
    }
    let status = if defects.len() == read.len() {
        ProbeStatus::Broken
    } else {
        ProbeStatus::Noncompliant
    };
    verdict(status, item, &defects)
}

/// Score the sign-in facts the run settled for the check's op.
pub fn run_protected_resource(check: &WebCheck, ctx: &HandlerContext) -> ProbeOutcome {
    let Some(auth) = ctx.mcp_auth.as_deref() else {
        return ProbeOutcome::na("the MCP endpoint does not require sign-in");
    };
    let with: ProtectedResourceWith = serde_json::from_str(check.with_json).unwrap_or_default();
    match with.op.as_deref() {
        Some("challenge") => challenge_outcome(auth),
        Some("metadata") => metadata_outcome(auth, ctx),
        other => ProbeOutcome::error(&format!(
            "protected-resource: check \"{}\" names unknown op {:?}",
            check.id,
            other.unwrap_or("undefined")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shown_percent_encodes_everything_outside_a_url_and_cuts_long_values() {
        assert_eq!(
            shown(&json!("https://a.example/x?y=1")),
            "https://a.example/x?y=1"
        );
        assert_eq!(shown(&json!("<b>é\n")), "%3Cb%3E%C3%A9%0A");
        assert_eq!(shown(&json!(7)), "7");
        let long = "a".repeat(250);
        assert_eq!(shown(&json!(long)), format!("{}...", "a".repeat(200)));
    }

    #[test]
    fn a_server_is_judged_by_what_the_vantage_can_reach() {
        let private = json!("https://10.0.0.1/oauth");
        assert_eq!(
            server_defect(&private, false).as_deref(),
            Some("names a private or reserved host")
        );
        assert_eq!(server_defect(&private, true), None);
        assert_eq!(
            server_defect(&json!("http://auth.example.com"), false).as_deref(),
            Some("is not https")
        );
        assert_eq!(
            server_defect(&json!("ftp://auth.example.com"), false).as_deref(),
            Some("is not a URL")
        );
        assert_eq!(
            server_defect(&json!(3), false).as_deref(),
            Some("is not a URL")
        );
        assert_eq!(
            server_defect(&json!("https://auth.example.com"), false),
            None
        );
    }
}
