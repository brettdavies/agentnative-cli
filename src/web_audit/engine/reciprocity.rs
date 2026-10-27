//! Control-bound reciprocity, a mirror of the site's `reciprocity.ts`:
//! whether the host serving a declared MCP endpoint publishes an artifact
//! naming it, which is the only thing that lets the auditor send that
//! endpoint a wire probe. A site can declare any URL, so a declaration
//! alone admits nothing.
//!
//! Three artifacts count, each read from a fixed location on the endpoint's
//! own host: a SEP-2127 card at `<endpoint>` + card suffix, an entry in that
//! host's AI catalog (never the audited site's), and RFC 9728
//! protected-resource metadata. Each must name the endpoint exactly after
//! syntax normalization, so an artifact naming one path admits no other
//! path, scheme, or port. A 405, an `Allow` header, or a JSON-RPC envelope
//! is what any POST-only route answers, so none of them admits. Every
//! artifact is read over https: one read over plaintext could be forged by
//! anyone on the network path.

use serde_json::Value;
use url::Url;

use super::discovery::resolve_url;
use super::discovery_documents::{
    CardShape, CatalogCardEntry, JsonObject, MCP_SERVER_CARD_TYPE, card_shape, card_suffix_url,
    catalog_card_entries, parse_json_object,
};
use super::follow_requests::{Cause, Gate, ReadOptions};
use super::follow_trail::{host_of, normalize_endpoint_url};
pub use super::reciprocity_metadata::MetadataMatch;
use super::reciprocity_metadata::protected_resource_metadata;
use crate::web_audit::fetch::body::DOCUMENT_MAX_BODY_BYTES;
use crate::web_audit::fetch::{NOT_HTTPS, ProbeResponse, Refusal};
use crate::web_audit::registry::McpDiscovery;

const CARD_READ: ReadOptions = ReadOptions {
    max_body_bytes: DOCUMENT_MAX_BODY_BYTES,
    accept: Some(MCP_SERVER_CARD_TYPE),
    timeout_cap: None,
};
const CATALOG_READ: ReadOptions = ReadOptions {
    max_body_bytes: DOCUMENT_MAX_BODY_BYTES,
    accept: None,
    timeout_cap: None,
};
/// A card read where it was found; an inline catalog card is the catalog
/// URL with a JSON Pointer fragment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdmittingCard {
    /// Where it was read.
    pub url: String,
    /// The response as read.
    pub response: ProbeResponse,
}

/// The artifact that admitted an endpoint.
#[derive(Clone, Debug, PartialEq)]
pub struct Admission {
    /// `card`, `ai-catalog`, or `metadata`.
    pub by: &'static str,
    /// The metadata, when metadata is what admitted it.
    pub metadata: Option<MetadataMatch>,
    /// The card, when a card admitted it.
    pub card: Option<AdmittingCard>,
}

/// Reads through the gate, answering a URL other than https as a read that
/// got no response.
pub(super) fn read(
    gate: &mut Gate<'_>,
    url: &str,
    opts: ReadOptions,
) -> Result<ProbeResponse, Cause> {
    if Url::parse(url).is_ok_and(|u| u.scheme() == "https") {
        return gate.get(url, opts);
    }
    Ok(ProbeResponse {
        refused: Some(Refusal::InsecureScheme),
        ..ProbeResponse::failed(NOT_HTTPS.to_string(), 0)
    })
}

pub(super) fn same_host(a: &str, b: &str) -> bool {
    host_of(a).is_some_and(|host| Some(host) == host_of(b))
}

fn card_names_endpoint(card: Option<&JsonObject>, card_url: &str, endpoint: &str) -> bool {
    let Some(card) = card.filter(|c| card_shape(Some(c)) == CardShape::Sep2127) else {
        return false;
    };
    let Some(remotes) = card.get("remotes").and_then(Value::as_array) else {
        return false;
    };
    remotes.iter().any(|remote| {
        remote
            .get("url")
            .and_then(Value::as_str)
            .and_then(|url| normalize_endpoint_url(&resolve_url(card_url, url)))
            .is_some_and(|url| url == endpoint)
    })
}

/// The card read at `url` when it names `endpoint`.
fn card_naming(
    gate: &mut Gate<'_>,
    url: &str,
    endpoint: &str,
) -> Result<Option<ProbeResponse>, Cause> {
    let response = read(gate, url, CARD_READ)?;
    let names = response.status == Some(200)
        && card_names_endpoint(parse_json_object(&response).as_ref(), url, endpoint);
    Ok(names.then_some(response))
}

/// The first card the endpoint host's own AI catalog lists, inline or by a
/// same-host URL, that names the endpoint.
fn host_catalog_card(
    gate: &mut Gate<'_>,
    endpoint: &str,
    catalog_path: &str,
) -> Result<Option<AdmittingCard>, Cause> {
    let origin = Url::parse(endpoint).map(|u| u.origin().ascii_serialization());
    let catalog_url = resolve_url(&origin.unwrap_or_default(), catalog_path);
    let response = read(gate, &catalog_url, CATALOG_READ)?;
    let catalog = (response.status == Some(200))
        .then(|| parse_json_object(&response))
        .flatten();
    for entry in catalog
        .as_ref()
        .map(catalog_card_entries)
        .unwrap_or_default()
    {
        match entry {
            CatalogCardEntry::Data { index, data } => {
                if card_names_endpoint(Some(&data), &catalog_url, endpoint) {
                    return Ok(Some(AdmittingCard {
                        url: format!("{catalog_url}#/entries/{index}/data"),
                        response: ProbeResponse {
                            body: serde_json::to_string(&data).unwrap_or_default(),
                            ..response
                        },
                    }));
                }
            }
            CatalogCardEntry::Url { url, .. } => {
                let card_url = resolve_url(&catalog_url, &url);
                if !same_host(&card_url, endpoint) {
                    continue;
                }
                if let Some(card) = card_naming(gate, &card_url, endpoint)? {
                    return Ok(Some(AdmittingCard {
                        url: card_url,
                        response: card,
                    }));
                }
            }
            CatalogCardEntry::Neither { .. } => {}
        }
    }
    Ok(None)
}

/// The artifact that admits `endpoint` (already normalized), checked in a
/// fixed order and stopping at the first that names it.
pub fn admitting_artifact(
    gate: &mut Gate<'_>,
    endpoint: &str,
    cfg: &McpDiscovery,
    challenge: Option<&str>,
) -> Result<Option<Admission>, Cause> {
    let card_url = card_suffix_url(endpoint, cfg.card_suffix);
    if let Some(card) = card_naming(gate, &card_url, endpoint)? {
        return Ok(Some(Admission {
            by: "card",
            metadata: None,
            card: Some(AdmittingCard {
                url: card_url,
                response: card,
            }),
        }));
    }
    if let Some(card) = host_catalog_card(gate, endpoint, cfg.ai_catalog)? {
        return Ok(Some(Admission {
            by: "ai-catalog",
            metadata: None,
            card: Some(card),
        }));
    }
    Ok(
        protected_resource_metadata(gate, endpoint, challenge)?.map(|metadata| Admission {
            by: "metadata",
            metadata: Some(metadata),
            card: None,
        }),
    )
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::web_audit::engine::follow_requests::SliceClock;
    use crate::web_audit::engine::types::FetchHandle;
    use crate::web_audit::locality::Resolver;
    use crate::web_audit::mock::{MockResponse, MockRule, MockTransport};
    use crate::web_audit::registry::MCP_DISCOVERY;

    struct PublicResolver;

    impl Resolver for PublicResolver {
        fn resolve(&self, _host: &str) -> Result<Vec<IpAddr>, String> {
            Ok(vec!["93.184.216.34".parse().unwrap()])
        }
    }

    const ENDPOINT: &str = "https://mcp.example.net/mcp";

    fn admitted(rules: Vec<MockRule>) -> Option<&'static str> {
        let transport = Arc::new(MockTransport::new(rules, MockResponse::ok(404, &[], b"")));
        let fetch = FetchHandle::new(transport, Arc::new(PublicResolver), "anc-test/0");
        let clock = SliceClock {
            timeout: Duration::from_secs(2),
            deadline_at: Instant::now() + Duration::from_secs(6),
        };
        let mut gate = Gate::new(&fetch, None, clock);
        admitting_artifact(&mut gate, ENDPOINT, &MCP_DISCOVERY, None)
            .expect("inside the caps")
            .map(|a| a.by)
    }

    fn metadata(resource: &str) -> MockResponse {
        let body = format!(
            "{{\"resource\":\"{resource}\",\"authorization_servers\":[\"https://auth.example.net\"]}}"
        );
        MockResponse::ok(
            200,
            &[("content-type", "application/json")],
            body.as_bytes(),
        )
    }

    #[test]
    fn metadata_admits_only_when_a_nonsense_path_is_not_echoed_back() {
        let published = MockRule::new(
            "GET",
            "https://mcp.example.net/.well-known/oauth-protected-resource/mcp",
        )
        .response(metadata(ENDPOINT));
        let ruled_out = MockRule::new(
            "GET",
            "https://mcp.example.net/.well-known/oauth-protected-resource/anc-web-audit-no-such-resource",
        )
        .response(MockResponse::ok(404, &[], b""));
        assert_eq!(
            admitted(vec![published.clone(), ruled_out]),
            Some("metadata")
        );
        let echoed = MockRule::new(
            "GET",
            "https://mcp.example.net/.well-known/oauth-protected-resource/anc-web-audit-no-such-resource",
        )
        .response(metadata("https://mcp.example.net/anc-web-audit-no-such-resource"));
        assert_eq!(admitted(vec![published.clone(), echoed]), None);
        let unanswered = MockRule::new(
            "GET",
            "https://mcp.example.net/.well-known/oauth-protected-resource/anc-web-audit-no-such-resource",
        )
        .response(MockResponse::ok(503, &[], b""));
        assert_eq!(admitted(vec![published, unanswered]), None);
    }

    #[test]
    fn a_post_only_route_admits_nothing() {
        let allow = MockRule::new("GET", ENDPOINT).response(MockResponse::ok(
            405,
            &[("allow", "POST")],
            b"",
        ));
        assert_eq!(admitted(vec![allow]), None);
    }
}
