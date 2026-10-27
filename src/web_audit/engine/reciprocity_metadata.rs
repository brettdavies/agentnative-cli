//! RFC 9728 protected-resource metadata as reciprocity and sign-in read it,
//! a mirror of `resolveProtectedResourceMetadata` in the site's
//! `reciprocity.ts`: the metadata must name the endpoint as its `resource`,
//! and below the root, metadata at a nonsense path must not echo that path
//! back. Reads go through an [`ArtifactSource`]: the follow slice's gate,
//! or a direct source outside it.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;
use url::Url;

use super::discovery_documents::{JsonObject, parse_json_object};
use super::follow_requests::{Cause, Gate, ReadOptions};
use super::follow_trail::normalize_endpoint_url;
use super::reciprocity::{read, same_host};
use crate::web_audit::fetch::ProbeResponse;
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

/// Where artifact reads go: one GET per URL, redirects disabled.
pub trait ArtifactSource {
    /// Read `url`, or say what stopped the read before it started.
    fn get(&mut self, url: &str, opts: ReadOptions) -> Result<ProbeResponse, Cause>;
    /// Whether the guard would let a request reach `url`.
    fn admits(&self, url: &str) -> bool;
}

impl ArtifactSource for Gate<'_> {
    fn get(&mut self, url: &str, opts: ReadOptions) -> Result<ProbeResponse, Cause> {
        Gate::get(self, url, opts)
    }

    fn admits(&self, url: &str) -> bool {
        self.fetch.validate(url).is_ok()
    }
}

/// RFC 9728 metadata naming an endpoint, and where it was read.
#[derive(Clone, Debug, PartialEq, Eq)]
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

fn read_metadata<S: ArtifactSource + ?Sized>(
    source: &mut S,
    url: &str,
) -> Result<Option<JsonObject>, Cause> {
    let response = read(source, url, METADATA_READ)?;
    Ok((response.status == Some(200))
        .then(|| parse_json_object(&response))
        .flatten())
}

/// What the nonsense-path read says about a gateway that echoes any
/// requested path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EchoControl {
    /// It answered below 500 without naming that path.
    RuledOut,
    /// It named it.
    Echoed,
    /// No answer, a server error, or a retry status: nothing about the path.
    Unanswered,
}

fn echo_control<S: ArtifactSource + ?Sized>(
    source: &mut S,
    origin: &str,
) -> Result<EchoControl, Cause> {
    let url = format!("{origin}{PROTECTED_RESOURCE_PATH}{ECHO_PROBE_PATH}");
    let response = read(source, &url, METADATA_READ)?;
    let Some(status) = response.status else {
        return Ok(EchoControl::Unanswered);
    };
    if status >= 500 || RETRY_SHAPED_STATUSES.iter().any(|(s, _)| *s == status) {
        return Ok(EchoControl::Unanswered);
    }
    let echoed = (status == 200)
        .then(|| resource_of(parse_json_object(&response).as_ref()))
        .flatten();
    Ok(
        if echoed == normalize_endpoint_url(&format!("{origin}{ECHO_PROBE_PATH}")) {
            EchoControl::Echoed
        } else {
            EchoControl::RuledOut
        },
    )
}

/// The metadata URL a challenge names: `Ok(None)` when it names none,
/// `Err(())` when it names one the auditor will not read (anything but
/// https on the endpoint's own host, or a URL the guard refuses).
fn challenge_metadata_url<S: ArtifactSource + ?Sized>(
    source: &S,
    endpoint: &str,
    challenge: &str,
) -> Result<Option<String>, ()> {
    let Some(raw) = resource_metadata_from_challenge(challenge) else {
        return Ok(None);
    };
    if !source.admits(&raw) {
        return Err(());
    }
    let https = Url::parse(&raw).is_ok_and(|u| u.scheme() == "https");
    if !https || !same_host(&raw, endpoint) {
        return Err(());
    }
    Ok(Some(raw))
}

/// How a metadata read is asked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MetadataQuery<'a> {
    /// The `WWW-Authenticate` value of a 401 the endpoint answered.
    pub challenge: Option<&'a str>,
    /// The endpoint is already of record, so the metadata decides only
    /// whether it requires sign-in, never whether it exists or may be
    /// probed: an unanswered echo read leaves the match standing, and only
    /// one that echoed refuses it. Where the metadata finds or admits an
    /// endpoint, an unanswered read confirms nothing.
    pub of_record: bool,
}

/// RFC 9728 metadata for `endpoint`, read from the challenge's
/// `resource_metadata` URL when there is one, else the well-known
/// locations, matching only when its `resource` normalizes to the
/// endpoint. Below the root, metadata at a nonsense path must not echo
/// that path back: a gateway that generates metadata for any requested
/// path confirms nothing.
pub fn protected_resource_metadata<S: ArtifactSource + ?Sized>(
    source: &mut S,
    endpoint: &str,
    query: MetadataQuery<'_>,
) -> Result<Option<MetadataMatch>, Cause> {
    let urls = match query
        .challenge
        .map(|c| challenge_metadata_url(source, endpoint, c))
    {
        Some(Err(())) => return Ok(None),
        Some(Ok(Some(url))) => vec![url],
        Some(Ok(None)) | None => protected_resource_metadata_urls(endpoint),
    };
    let mut found: Option<MetadataMatch> = None;
    for url in urls {
        if let Some(metadata) = read_metadata(source, &url)? {
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
    Ok(match echo_control(source, &origin)? {
        EchoControl::RuledOut => Some(found),
        EchoControl::Unanswered if query.of_record => Some(found),
        EchoControl::Unanswered | EchoControl::Echoed => None,
    })
}
