//! The refusals the follow slice settles before any request, which no
//! response can change, a mirror of `refusal`, `hopRefusal` and
//! `settledUpfront` in the site's `follow-trail.ts`.

use url::Url;

use super::discovery_documents::is_templated_url;
use super::follow_trail::{Declaration, Kind, Outcome, Settled, host_of, normalize_endpoint_url};
use super::types::FetchHandle;
use crate::web_audit::locality::Locality;

// The auditor's own zone, which a declaration reaches only at its
// canonical MCP endpoint.
const SELF_ZONE: &str = "anc.dev";
const CANONICAL_SELF_ENDPOINT: &str = "https://anc.dev/mcp";

fn in_self_zone(host: &str) -> bool {
    // The parser keeps trailing dots as written, and `anc.dev.` and
    // `anc.dev..` name the same zone.
    let host = host.to_ascii_lowercase();
    let host = host.trim_end_matches('.');
    host == SELF_ZONE || host.ends_with(&format!(".{SELF_ZONE}"))
}

/// Where the URL may not be requested at all, or `None` when it may: only
/// https, never an IP literal or a host the guard refuses, and in the
/// auditor's own zone only its canonical MCP endpoint. What a declared host
/// publishes naming its endpoint is its consent to be probed, and consent
/// read over plaintext can be forged by anyone on the network path.
pub(super) fn refusal(
    url: &str,
    kind: Kind,
    fetch: &FetchHandle,
    target_locality: Option<Locality>,
) -> Option<Settled> {
    let parsed = match Url::parse(url) {
        Ok(u) if matches!(u.scheme(), "http" | "https") => u,
        _ => return Some(Settled::of(Outcome::ReciprocityRefused)),
    };
    match fetch.validate(url) {
        Err(reason) if reason.starts_with("blocked") => return Some(Settled::of(Outcome::Blocked)),
        Err(_) => return Some(Settled::of(Outcome::ReciprocityRefused)),
        Ok(class) if target_locality.is_some_and(|t| t != class) => {
            return Some(Settled::of(Outcome::Blocked));
        }
        Ok(_) => {}
    }
    if matches!(parsed.host(), Some(url::Host::Ipv4(_) | url::Host::Ipv6(_))) {
        return Some(Settled::of(Outcome::Blocked));
    }
    if parsed.scheme() != "https" {
        return Some(Settled::not_followed("insecure-scheme"));
    }
    let canonical = kind == Kind::McpEndpoint
        && normalize_endpoint_url(url).as_deref() == Some(CANONICAL_SELF_ENDPOINT);
    if in_self_zone(parsed.host_str().unwrap_or("")) && !canonical {
        return Some(Settled::not_followed("self-path"));
    }
    None
}

/// Where a redirect hop may not be requested, or `None` when it may. A
/// refused hop that names a host records it as the final URL, so the rows
/// read as the host the redirect led to rather than the one that sent it.
pub(super) fn hop_refusal(
    hop: &str,
    kind: Kind,
    fetch: &FetchHandle,
    target_locality: Option<Locality>,
) -> Option<Settled> {
    let refused = if hop.is_empty() {
        Settled::of(Outcome::ReciprocityRefused)
    } else {
        refusal(hop, kind, fetch, target_locality)?
    };
    let final_url = host_of(hop)
        .filter(|h| !h.is_empty())
        .map(|_| hop.to_string());
    Some(Settled {
        final_url,
        ..refused
    })
}

/// What the slice knows before it runs.
#[derive(Clone, Copy, Debug)]
pub struct Upfront<'a> {
    /// Following is on.
    pub enabled: bool,
    /// A card already named an endpoint on the audited origin.
    pub entry_endpoint_declared: bool,
    /// The run's transport and resolver.
    pub fetch: &'a FetchHandle,
    /// The target's locality; a declaration of another class is blocked.
    pub target_locality: Option<Locality>,
}

/// How a declaration settles before any request, or `None` when the slice
/// must request it.
pub(super) fn settled_upfront(d: &Declaration, input: &Upfront<'_>) -> Option<Settled> {
    if let Some(reason) = d.not_followed {
        return Some(Settled::not_followed(reason));
    }
    if is_templated_url(&d.url) {
        return Some(Settled::not_followed("templated-url"));
    }
    if !input.enabled {
        return Some(Settled::not_followed("follow-disabled"));
    }
    if input.entry_endpoint_declared {
        return Some(Settled::not_followed("beyond-endpoint-of-record"));
    }
    refusal(&d.url, d.kind, input.fetch, input.target_locality)
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;
    use std::sync::Arc;

    use super::*;
    use crate::web_audit::locality::Resolver;
    use crate::web_audit::mock::{MockResponse, MockTransport};

    struct PublicResolver;

    impl Resolver for PublicResolver {
        fn resolve(&self, _host: &str) -> Result<Vec<IpAddr>, String> {
            Ok(vec!["93.184.216.34".parse().unwrap()])
        }
    }

    fn refused(url: &str, kind: Kind) -> Option<(Outcome, Option<&'static str>)> {
        let transport = Arc::new(MockTransport::new(vec![], MockResponse::ok(200, &[], b"")));
        let fetch = FetchHandle::new(transport, Arc::new(PublicResolver), "anc-test/0");
        refusal(url, kind, &fetch, None).map(|s| (s.outcome, s.reason))
    }

    #[test]
    fn the_auditors_own_zone_admits_only_its_canonical_mcp_endpoint_in_any_spelling() {
        let self_path = Some((Outcome::NotFollowed, Some("self-path")));
        assert_eq!(
            refused("https://ANC.dev:443/mcp#frag", Kind::McpEndpoint),
            None
        );
        assert_eq!(
            refused("https://anc.dev./mcp", Kind::McpEndpoint),
            self_path
        );
        assert_eq!(
            refused("https://anc.dev../mcp", Kind::McpEndpoint),
            self_path
        );
        assert_eq!(
            refused("https://anc.dev/mcp", Kind::CardDocument),
            self_path
        );
        assert_eq!(
            refused("https://www.anc.dev/other", Kind::McpEndpoint),
            self_path
        );
        assert_eq!(
            refused("http://mcp.example.net/mcp", Kind::McpEndpoint),
            Some((Outcome::NotFollowed, Some("insecure-scheme")))
        );
        assert_eq!(
            refused("https://93.184.216.34/mcp", Kind::McpEndpoint),
            Some((Outcome::Blocked, None))
        );
        assert_eq!(
            refused("ftp://mcp.example.net/mcp", Kind::McpEndpoint),
            Some((Outcome::ReciprocityRefused, None))
        );
        assert_eq!(
            refused("https://mcp.example.net/mcp", Kind::McpEndpoint),
            None
        );
    }
}
