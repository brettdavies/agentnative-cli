//! The card documents the follow slice reads, a mirror of
//! `cardDocumentRead` in the site's `follow.ts`: a card a site's catalog
//! names by URL is read to learn the endpoint it names, which the slice
//! then confirms like any declared endpoint.

use std::time::Duration;

use super::discovery::resolve_url;
use super::discovery_documents::{
    MCP_SERVER_CARD_TYPE, RetainedDocument, card_endpoint, is_templated_url, other_remotes,
    parse_json_object, retained_card,
};
use super::follow_requests::{Fetched, ReadOptions};
use super::follow_trail::{Declaration, Kind, Outcome, Settled, TrailEntry, declares_host};
use super::waves::is_edge_error_status;
use crate::web_audit::fetch::body::DOCUMENT_MAX_BODY_BYTES;

pub(super) const CARD_DOCUMENT_READ: ReadOptions = ReadOptions {
    max_body_bytes: DOCUMENT_MAX_BODY_BYTES,
    accept: Some(MCP_SERVER_CARD_TYPE),
    timeout_cap: Some(Duration::from_millis(3_000)),
};

pub(super) struct CardDocumentRead {
    pub(super) entry: TrailEntry,
    pub(super) named: Vec<Declaration>,
    pub(super) card: Option<RetainedDocument>,
}

/// A card document's trail entry, and the endpoints it names, which settle
/// later in declaration order. A card host that gives no response, or one
/// the edge answers for, records unreachable; any answer that yields no
/// endpoint records reciprocity-refused.
pub(super) fn card_document_read(
    base: &str,
    d: &Declaration,
    read: Result<Fetched, Settled>,
) -> CardDocumentRead {
    let unnamed = |settled: Settled| CardDocumentRead {
        entry: TrailEntry::of(d, settled),
        named: Vec::new(),
        card: None,
    };
    let fetched = match read {
        Ok(fetched) => fetched,
        Err(settled) => return unnamed(settled),
    };
    let final_url = (fetched.url != d.url).then(|| fetched.url.clone());
    let status = fetched.response.status;
    if status.is_none() || is_edge_error_status(status) {
        return unnamed(Settled::landed(final_url, Outcome::Unreachable));
    }
    let card = (status == Some(200))
        .then(|| parse_json_object(&fetched.response))
        .flatten();
    let Some((card, named)) = card.and_then(|c| card_endpoint(&c).map(|n| (c, n))) else {
        return unnamed(Settled::landed(final_url, Outcome::ReciprocityRefused));
    };
    let primary = if is_templated_url(&named) {
        Declaration {
            not_followed: Some("templated-url"),
            ..Declaration::new(Kind::McpEndpoint, named, d.url.clone())
        }
    } else {
        Declaration::new(
            Kind::McpEndpoint,
            resolve_url(&fetched.url, &named),
            d.url.clone(),
        )
    };
    let mut named = vec![primary];
    named.extend(other_remotes(&card, &d.url, &fetched.url));
    CardDocumentRead {
        entry: TrailEntry::of(d, Settled::landed(final_url, Outcome::Followed)),
        named: named
            .into_iter()
            .filter(|n| declares_host(n, base))
            .collect(),
        card: Some(retained_card(&fetched.url, &fetched.response)),
    }
}
