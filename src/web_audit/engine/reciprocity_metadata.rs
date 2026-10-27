//! RFC 9728 protected-resource metadata as reciprocity reads it, a mirror
//! of `resolveProtectedResourceMetadata` in the site's `reciprocity.ts`:
//! the metadata must name the endpoint as its `resource`, and below the
//! root, metadata at a nonsense path must not echo that path back.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;
use url::Url;

use super::discovery_documents::{JsonObject, parse_json_object};
use super::follow_requests::{Cause, Gate, ReadOptions};
use super::follow_trail::normalize_endpoint_url;
use super::reciprocity::{read, same_host};
use crate::web_audit::fetch::body::METADATA_MAX_BODY_BYTES;
use crate::web_audit::handlers::shared::RETRY_SHAPED_STATUSES;

const PROTECTED_RESOURCE_PATH: &str = "/.well-known/oauth-protected-resource";
// A path no real resource lives at: metadata naming it as its resource was
// generated from the request, not published for an endpoint.
const ECHO_PROBE_PATH: &str = "/anc-web-audit-no-such-resource";

const METADATA_READ: ReadOptions = ReadOptions {
    max_body_bytes: METADATA_MAX_BODY_BYTES,
    accept: None,
    timeout_cap: None,
};

static RESOURCE_METADATA_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?:^|[\s,])resource_metadata\s*=\s*(?:"([^"]*)"|([^\s,]+))"#).unwrap()
});

/// RFC 9728 metadata naming an endpoint, and where it was read.
#[derive(Clone, Debug, PartialEq)]
pub struct MetadataMatch {
    /// Where it was read.
    pub url: String,
    /// The parsed document.
    pub metadata: JsonObject,
}

/// RFC 9728 §3.1: the path-suffixed well-known location, then the root one.
pub fn protected_resource_metadata_urls(endpoint: &str) -> Vec<String> {
    let Ok(u) = Url::parse(endpoint) else {
        return Vec::new();
    };
    let origin = u.origin().ascii_serialization();
    let path = if u.path() == "/" { "" } else { u.path() };
    let search = u.query().map(|q| format!("?{q}")).unwrap_or_default();
    let suffixed = format!("{origin}{PROTECTED_RESOURCE_PATH}{path}{search}");
    let root = format!("{origin}{PROTECTED_RESOURCE_PATH}");
    if suffixed == root {
        vec![root]
    } else {
        vec![suffixed, root]
    }
}

/// The `resource_metadata` URL a `WWW-Authenticate` challenge names, as written.
pub fn resource_metadata_from_challenge(challenge: &str) -> Option<String> {
    let captures = RESOURCE_METADATA_RE.captures(challenge)?;
    captures
        .get(1)
        .or_else(|| captures.get(2))
        .map(|m| m.as_str().to_string())
}

fn resource_of(metadata: Option<&JsonObject>) -> Option<String> {
    metadata?
        .get("resource")
        .and_then(Value::as_str)
        .and_then(normalize_endpoint_url)
}

fn read_metadata(gate: &mut Gate<'_>, url: &str) -> Result<Option<JsonObject>, Cause> {
    let response = read(gate, url, METADATA_READ)?;
    Ok((response.status == Some(200))
        .then(|| parse_json_object(&response))
        .flatten())
}

/// Whether metadata at a nonsense path rules out a gateway that echoes any
/// requested path: `Some(true)` when it answered below 500 without naming
/// that path, `Some(false)` when it named it, `None` when the read says
/// nothing about the path (no answer, a server error, a retry status).
fn echo_ruled_out(gate: &mut Gate<'_>, origin: &str) -> Result<Option<bool>, Cause> {
    let url = format!("{origin}{PROTECTED_RESOURCE_PATH}{ECHO_PROBE_PATH}");
    let response = read(gate, &url, METADATA_READ)?;
    let Some(status) = response.status else {
        return Ok(None);
    };
    if status >= 500 || RETRY_SHAPED_STATUSES.iter().any(|(s, _)| *s == status) {
        return Ok(None);
    }
    let echoed = (status == 200)
        .then(|| resource_of(parse_json_object(&response).as_ref()))
        .flatten();
    Ok(Some(
        echoed != normalize_endpoint_url(&format!("{origin}{ECHO_PROBE_PATH}")),
    ))
}

/// The metadata URL a challenge names: `Ok(None)` when it names none,
/// `Err(())` when it names one the auditor will not read (anything but
/// https on the endpoint's own public host).
fn challenge_metadata_url(
    gate: &Gate<'_>,
    endpoint: &str,
    challenge: &str,
) -> Result<Option<String>, ()> {
    let Some(raw) = resource_metadata_from_challenge(challenge) else {
        return Ok(None);
    };
    if gate.fetch.validate(&raw).is_err() {
        return Err(());
    }
    let https = Url::parse(&raw).is_ok_and(|u| u.scheme() == "https");
    if !https || !same_host(&raw, endpoint) {
        return Err(());
    }
    Ok(Some(raw))
}

/// RFC 9728 metadata for `endpoint`, read from the challenge's
/// `resource_metadata` URL when there is one, else the well-known
/// locations, matching only when its `resource` normalizes to the
/// endpoint. Below the root, metadata at a nonsense path must not echo
/// that path back: a gateway that generates metadata for any requested
/// path confirms nothing.
pub(super) fn protected_resource_metadata(
    gate: &mut Gate<'_>,
    endpoint: &str,
    challenge: Option<&str>,
) -> Result<Option<MetadataMatch>, Cause> {
    let urls = match challenge.map(|c| challenge_metadata_url(gate, endpoint, c)) {
        Some(Err(())) => return Ok(None),
        Some(Ok(Some(url))) => vec![url],
        Some(Ok(None)) | None => protected_resource_metadata_urls(endpoint),
    };
    let mut found: Option<MetadataMatch> = None;
    for url in urls {
        if let Some(metadata) = read_metadata(gate, &url)? {
            found = Some(MetadataMatch { url, metadata });
            break;
        }
    }
    let Some(found) = found.filter(|f| resource_of(Some(&f.metadata)).as_deref() == Some(endpoint))
    else {
        return Ok(None);
    };
    let Ok(parsed) = Url::parse(endpoint) else {
        return Ok(None);
    };
    if parsed.path() == "/" {
        return Ok(Some(found));
    }
    let origin = parsed.origin().ascii_serialization();
    Ok((echo_ruled_out(gate, &origin)? == Some(true)).then_some(found))
}
