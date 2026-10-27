//! The endpoint the MCP rows are scored at, a mirror of the site's
//! `endpoint-of-record.ts`, chosen once discovery's POSTs and the follow
//! slice have both finished: the audited site's own endpoint, else the
//! first declared endpoint the slice admitted. Beside it, the
//! declared-hosts trail in declaration order, the API catalog's share of
//! what the slice settled, and the reason the rows that need an endpoint
//! were not evaluated when a declared host is why there is none. For a
//! followed endpoint, the card the slice read for it becomes the card of
//! record when it stands higher than discovery's.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use super::api_catalog::{CatalogAnchor, api_declarations};
use super::api_follow::ApiFollowResult;
use super::discovery::{DiscoveryDocuments, DiscoveryResult};
use super::discovery_documents::{RetainedDocument, SERVER_CARD_DOCUMENT, outranks};
use super::follow::{FollowInput, FollowResult, open_follow};
use super::follow_trail::{
    Declaration, DeclaredHostReason, Kind, Outcome, Settled, TrailEntry, declaration_key,
    declared_host_reason, declares_host,
};
use super::reciprocity_metadata::MetadataMatch;
use super::sign_in::SignInChallenge;
use super::types::FetchHandle;
use crate::web_audit::locality::Locality;
use crate::web_audit::registry::McpDiscovery;

/// Where the MCP rows are scored, and what the follow left for the rest.
#[derive(Clone, Debug, Default)]
pub struct EndpointOfRecord {
    /// The endpoint the MCP rows probe.
    pub endpoint: Option<String>,
    /// The endpoint came from a declared host, pinned when reciprocity admitted it.
    pub followed: bool,
    /// RFC 9728 metadata naming the endpoint that finding or admitting it
    /// already read.
    pub metadata: Option<MetadataMatch>,
    /// The 401 discovery drew from the audited site's own endpoint while
    /// finding it; `None` when it found it another way.
    pub challenge: Option<SignInChallenge>,
    /// The declared-hosts trail, in declaration order.
    pub trail: Vec<TrailEntry>,
    /// Why the rows that need an endpoint were not evaluated, when a declared host is why.
    pub unmet: Option<DeclaredHostReason>,
    /// What the slice settled for the API catalog.
    pub api: ApiFollowResult,
}

/// An endpoint admitted while the audited site's own endpoint was found: never probed.
fn superseded(entry: &TrailEntry) -> TrailEntry {
    TrailEntry {
        settled: Settled {
            final_url: entry.settled.final_url.clone(),
            cause: entry.settled.cause,
            ..Settled::not_followed("beyond-endpoint-of-record")
        },
        ..entry.clone()
    }
}

fn unmet_reason(trail: &[TrailEntry]) -> Option<DeclaredHostReason> {
    trail
        .iter()
        .filter(|e| e.kind.is_mcp())
        .find_map(declared_host_reason)
}

/// The endpoint of record from discovery's own endpoint and the slice.
pub fn endpoint_of_record(
    base: &str,
    discovery: &DiscoveryResult,
    follow: &FollowResult,
) -> EndpointOfRecord {
    let own = discovery.endpoint.as_ref();
    let mut trail: Vec<TrailEntry> = Vec::new();
    let mut keys: HashSet<String> = HashSet::new();
    let mut record = |entry: &TrailEntry| {
        if !keys.insert(declaration_key(entry.kind, &entry.url)) {
            return;
        }
        let supersede = own.is_some()
            && entry.settled.outcome == Outcome::Followed
            && entry.kind == Kind::McpEndpoint;
        trail.push(if supersede {
            superseded(entry)
        } else {
            entry.clone()
        });
    };
    let record_mcp = |declarations: &[Declaration], record: &mut dyn FnMut(&TrailEntry)| {
        for d in declarations.iter().filter(|d| declares_host(d, base)) {
            // Only a card read after the POSTs (one at the audited site's own
            // endpoint) declares what the slice never saw.
            match follow.entries.get(&declaration_key(d.kind, &d.url)) {
                Some(group) => group.iter().for_each(&mut *record),
                None => record(&TrailEntry::of(
                    d,
                    Settled::not_followed("beyond-endpoint-of-record"),
                )),
            }
        }
    };
    record_mcp(&discovery.declarations, &mut record);
    follow.api.entries.iter().for_each(&mut record);
    record_mcp(&discovery.redirected, &mut record);
    let endpoint = own.cloned().or_else(|| follow.endpoint.clone());
    EndpointOfRecord {
        followed: own.is_none() && follow.endpoint.is_some(),
        metadata: if own.is_some() {
            discovery.endpoint_metadata.clone()
        } else {
            follow.endpoint_metadata.clone()
        },
        challenge: own.and(discovery.endpoint_challenge.clone()),
        unmet: endpoint.is_none().then(|| unmet_reason(&trail)).flatten(),
        endpoint,
        trail,
        api: follow.api.clone(),
    }
}

/// Discovery's result with `card` as its card of record when it outranks
/// discovery's own, whose read then drops the document mark.
fn with_endpoint_card(
    mut discovery: DiscoveryResult,
    card: Option<RetainedDocument>,
) -> DiscoveryResult {
    let Some(card) = card.filter(|c| outranks(c, discovery.documents.get(SERVER_CARD_DOCUMENT)))
    else {
        return discovery;
    };
    for item in &mut discovery.evidence {
        if item.get("document").and_then(|d| d.as_str()) == Some(SERVER_CARD_DOCUMENT) {
            item.shift_remove("document");
        }
    }
    discovery.documents.insert(SERVER_CARD_DOCUMENT, card);
    discovery
}

/// The run's inputs to the follow.
#[derive(Clone, Copy, Debug)]
pub struct Settle<'a> {
    /// The audited origin.
    pub base: &'a str,
    /// Following is on.
    pub enabled: bool,
    /// The site answered the root or a document read; a redirected POST is
    /// an answer of its own.
    pub site_answered: bool,
    /// An API row applies to the declared site type.
    pub api_rows_apply: bool,
    /// Where reciprocity reads a host's card and catalog.
    pub discovery: &'a McpDiscovery,
    /// The run's transport and resolver.
    pub fetch: &'a FetchHandle,
    /// The target's locality.
    pub target_locality: Option<Locality>,
    /// The per-request timeout.
    pub timeout: Duration,
    /// The per-audit deadline.
    pub deadline: Instant,
}

/// Discovery's POSTs and the follow slice side by side, then where the
/// POSTs were redirected off the audited origin, on what is left of the
/// slice, then the API catalog's declarations, then the endpoint of record.
pub fn settle_endpoint_of_record(
    documents: DiscoveryDocuments<'_>,
    anchors: &[CatalogAnchor],
    opts: &Settle<'_>,
) -> (DiscoveryResult, EndpointOfRecord) {
    let opened = Instant::now();
    let declared = if opts.site_answered {
        documents.declarations()
    } else {
        Vec::new()
    };
    let mut session = open_follow(FollowInput {
        base: opts.base,
        enabled: opts.enabled,
        entry_endpoint_declared: documents.card_endpoint().is_some(),
        discovery: opts.discovery,
        fetch: opts.fetch,
        target_locality: opts.target_locality,
        timeout: opts.timeout,
        deadline: opts.deadline,
        opened,
    });
    let discovery = std::thread::scope(|scope| {
        let posts = scope.spawn(move || documents.probe_endpoint());
        session.settle(&declared);
        posts.join().unwrap_or_default()
    });
    session.settle(&discovery.redirected);
    if opts.site_answered && opts.api_rows_apply {
        session.settle_api(&api_declarations(anchors));
    }
    let followed = session.result();
    let record = endpoint_of_record(opts.base, &discovery, &followed);
    let discovery = if record.followed {
        with_endpoint_card(discovery, followed.endpoint_card)
    } else {
        discovery
    };
    (discovery, record)
}
