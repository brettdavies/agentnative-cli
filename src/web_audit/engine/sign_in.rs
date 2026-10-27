//! Whether an MCP endpoint requires sign-in, a mirror of the site's
//! `mcp-auth.ts`. Any route can answer 401, so a 401 alone is refusal
//! evidence. A 401 from the endpoint asks for sign-in when RFC 9728
//! metadata on its own host names the endpoint as the protected resource,
//! with the echo control that keeps a gateway minting metadata for any path
//! from confirming anything. The metadata is read once per audit, for the
//! first 401 that needs it, and metadata the audit already read while
//! finding or admitting the endpoint is not read again. It is read over
//! https only: metadata read over plaintext could be forged to settle
//! sign-in, or to make a common path the endpoint, for a host that never
//! published it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

pub use super::direct_source::DirectSource;
use super::discovery_documents::JsonObject;
use super::follow_trail::normalize_endpoint_url;
use super::reciprocity_metadata::{
    ArtifactSource, MetadataMatch, MetadataQuery, protected_resource_metadata,
    resource_metadata_from_challenge,
};
use super::types::{ProbeOutcome, ProbeStatus};
use crate::web_audit::handlers::mcp::ops::Era;
use crate::web_audit::scorecard::EvidenceItem;

/// The wave-1 wire probes whose answer decides it, in the order a 401's
/// challenge is read, with the lane each asks on.
const WIRE_PROBES: [(&str, Era); 2] = [
    ("mcp-initialize", Era::Legacy),
    ("mcp-server-discover", Era::Modern),
];

/// The endpoint of record requires sign-in: a wire probe drew a 401 and
/// RFC 9728 metadata on the endpoint's own host names it as the protected
/// resource.
#[derive(Clone, Debug, PartialEq)]
pub struct McpAuthRequired {
    /// The endpoint of record.
    pub endpoint: String,
    /// The 401's `WWW-Authenticate` value; `None` when it carried none.
    pub challenge: Option<String>,
    /// The handshake lane whose probe drew the 401, which the token-less
    /// refusal row asks on.
    pub lane: Era,
    /// Where the metadata naming the endpoint was read.
    pub metadata_url: String,
    /// The metadata.
    pub metadata: JsonObject,
}

/// A 401 the endpoint answered, with its challenge and the lane that drew it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignInChallenge {
    /// The `WWW-Authenticate` value, when there was one.
    pub challenge: Option<String>,
    /// The handshake lane that drew it.
    pub lane: Era,
}

/// The wave-1 wire probes' evidence, in the order a 401's challenge is read.
pub fn wire_probe_evidence(sources: &HashMap<String, ProbeOutcome>) -> Vec<&EvidenceItem> {
    WIRE_PROBES
        .iter()
        .flat_map(|(id, _)| sources.get(*id).map_or(&[][..], |o| &o.evidence[..]))
        .collect()
}

/// A handshake probe was served a JSON-RPC result at a 2xx. An error
/// envelope, a body without `result`, and a refusal serve nothing.
pub fn handshake_served(outcome: Option<&ProbeOutcome>) -> bool {
    outcome.is_some_and(|o| {
        let status = o.evidence.first().and_then(|e| e.get("status"));
        o.json_rpc_result
            && status
                .and_then(|s| s.as_u64())
                .is_some_and(|s| (200..300).contains(&s))
    })
}

/// A wave-1 wire probe was served a result: the endpoint answered a request
/// that carried no token.
pub fn served_without_sign_in(sources: &HashMap<String, ProbeOutcome>) -> bool {
    WIRE_PROBES
        .iter()
        .any(|(id, _)| handshake_served(sources.get(*id)))
}

/// The lane's own handshake read absent: the server refused the lane as one
/// it does not offer, without the 401 that asks for sign-in, so a token
/// would not open it.
pub fn lane_refused(sources: &HashMap<String, ProbeOutcome>, lane: Era) -> bool {
    WIRE_PROBES.iter().any(|(id, probe_lane)| {
        *probe_lane == lane
            && sources
                .get(*id)
                .is_some_and(|o| o.status == ProbeStatus::Absent)
    })
}

/// The 401 a wave-1 wire probe drew from the endpoint.
fn wire_challenge(sources: &HashMap<String, ProbeOutcome>) -> Option<SignInChallenge> {
    WIRE_PROBES.iter().find_map(|(id, lane)| {
        let outcome = sources.get(*id)?;
        let first = outcome.evidence.first()?;
        (outcome.status != ProbeStatus::Error
            && first.get("status").and_then(|s| s.as_u64()) == Some(401))
        .then(|| SignInChallenge {
            challenge: first
                .get("www_authenticate")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            lane: *lane,
        })
    })
}

/// A common path whose discovery POST drew a 401, with the challenge it carried.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChallengedPath {
    /// The configured path.
    pub path: &'static str,
    /// The resolved URL.
    pub url: String,
    /// Which discovery POST drew the 401: `initialize` or `modern-tools-list`.
    pub probed: &'static str,
    /// The `WWW-Authenticate` value, when there was one.
    pub challenge: Option<String>,
}

/// A challenged path whose own metadata names it, and the sign-in it shows.
#[derive(Clone, Debug, PartialEq)]
pub struct SignInEndpoint {
    /// The path.
    pub path: ChallengedPath,
    /// The metadata naming it.
    pub metadata: MetadataMatch,
    /// The 401 it answered.
    pub challenge: SignInChallenge,
}

/// The first challenged path, in probe order, whose own metadata names it.
pub fn sign_in_endpoint<S: ArtifactSource + ?Sized>(
    challenged: &[ChallengedPath],
    source: &mut S,
) -> Option<SignInEndpoint> {
    challenged.iter().find_map(|path| {
        let query = MetadataQuery {
            challenge: path.challenge.as_deref(),
            of_record: false,
        };
        let metadata = protected_resource_metadata(source, &path.url, query).ok()??;
        let lane = if path.probed == "initialize" {
            Era::Legacy
        } else {
            Era::Modern
        };
        Some(SignInEndpoint {
            challenge: SignInChallenge {
                challenge: path.challenge.clone(),
                lane,
            },
            metadata,
            path: path.clone(),
        })
    })
}

/// Reads the endpoint's metadata at most once per audit: the first 401 it
/// is asked about decides, and every later ask gets that answer. Known
/// metadata stands unless the 401's challenge names another location, which
/// then takes precedence the way RFC 9728 orders them.
#[derive(Debug)]
pub struct SignInResolver {
    endpoint: Option<String>,
    known: Option<MetadataMatch>,
    source: Mutex<DirectSource>,
    settled: OnceLock<Option<Arc<McpAuthRequired>>>,
}

impl SignInResolver {
    /// A resolver for the endpoint of record and the metadata finding or
    /// admitting it already read.
    pub fn new(
        endpoint: Option<String>,
        known: Option<MetadataMatch>,
        source: DirectSource,
    ) -> Self {
        SignInResolver {
            endpoint,
            known,
            source: Mutex::new(source),
            settled: OnceLock::new(),
        }
    }

    /// Whether a 401 the endpoint answered is backed by metadata naming it.
    pub fn resolve(&self, answer: &SignInChallenge) -> Option<Arc<McpAuthRequired>> {
        self.settled.get_or_init(|| self.backed(answer)).clone()
    }

    fn backed(&self, answer: &SignInChallenge) -> Option<Arc<McpAuthRequired>> {
        let raw = self.endpoint.as_deref()?;
        let endpoint = normalize_endpoint_url(raw)?;
        let named = answer
            .challenge
            .as_deref()
            .and_then(resource_metadata_from_challenge);
        let found = match &self.known {
            Some(known) if named.as_ref().is_none_or(|n| *n == known.url) => known.clone(),
            _ => {
                let query = MetadataQuery {
                    challenge: answer.challenge.as_deref(),
                    of_record: true,
                };
                let mut source = self.source.lock().unwrap_or_else(|e| e.into_inner());
                protected_resource_metadata(&mut *source, &endpoint, query).ok()??
            }
        };
        Some(Arc::new(McpAuthRequired {
            endpoint: raw.to_string(),
            challenge: answer.challenge.clone(),
            lane: answer.lane,
            metadata_url: found.url,
            metadata: found.metadata,
        }))
    }
}

/// Whether the endpoint of record requires sign-in, once wave 1 has probed
/// it. Wave 1's 401 is read first. When neither wire probe drew one, nor was
/// served a result, the 401 discovery drew while finding the endpoint still
/// stands: a timeout, a rate limit, or a server error on the handshake says
/// nothing about whether the endpoint requires sign-in.
pub fn settle_mcp_auth(
    observed: Option<&SignInChallenge>,
    sources: &HashMap<String, ProbeOutcome>,
    resolver: &SignInResolver,
) -> Option<Arc<McpAuthRequired>> {
    let answer = wire_challenge(sources).or_else(|| {
        (!served_without_sign_in(sources))
            .then(|| observed.cloned())
            .flatten()
    })?;
    resolver.resolve(&answer)
}
