//! The API catalog's share of the follow slice, a mirror of the site's
//! `follow-api.ts` over the request gate (`follow_requests`).
//!
//! An anchor host off the audited origin is admitted (one of the slice's
//! host slots), which is what lets wave 2 send it the hygiene probes; it
//! gets no request here. A `service-desc` target is read once, under the
//! slice's clock, request cap and host slots, and kept for the OpenAPI row.
//! A description host that gives no response, or whose answer is a
//! redirect the slice does not take (its one redirect hop redirecting
//! again), records unreachable; any other answer records followed. The API
//! batch settles after the MCP batches, on what is left of the slice.

use std::time::Duration;

use super::follow_requests::{Fetched, Gate, ReadOptions};
use super::follow_trail::{Declaration, Kind, Outcome, Settled, TrailEntry, declaration_key};
use super::waves::is_edge_error_status;
use crate::web_audit::fetch::body::OPENAPI_MAX_BODY_BYTES;
use crate::web_audit::fetch::redirect::is_redirect;

const DESCRIPTION_READ: ReadOptions = ReadOptions {
    max_body_bytes: OPENAPI_MAX_BODY_BYTES,
    accept: None,
    timeout_cap: Some(Duration::from_millis(3_000)),
};

/// What the slice settled for the catalog.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ApiFollowResult {
    /// Trail entries in declaration order: the anchors, then the descriptions.
    pub entries: Vec<TrailEntry>,
    /// The descriptions read, with their declaration keys, in declaration order.
    pub descriptions: Vec<(String, Fetched)>,
}

/// A description's trail outcome from where its read landed.
fn described(read: &Result<Fetched, Settled>, declared: &str) -> Settled {
    let fetched = match read {
        Ok(fetched) => fetched,
        Err(settled) => return settled.clone(),
    };
    let status = fetched.response.status;
    let unread =
        status.is_none() || is_edge_error_status(status) || status.is_some_and(is_redirect);
    Settled::landed(
        (fetched.url != declared).then(|| fetched.url.clone()),
        if unread {
            Outcome::Unreachable
        } else {
            Outcome::Followed
        },
    )
}

/// Admits the anchor hosts in declaration order, then reads the
/// descriptions side by side.
pub fn settle_api_declarations(
    gate: &mut Gate<'_>,
    declarations: &[Declaration],
    upfront: impl Fn(&Declaration) -> Option<Settled>,
) -> ApiFollowResult {
    let mut result = ApiFollowResult::default();
    for anchor in declarations.iter().filter(|d| d.kind == Kind::ApiAnchor) {
        let settled = upfront(anchor).unwrap_or_else(|| match gate.enter(&anchor.url) {
            Ok(()) => Settled::of(Outcome::Followed),
            Err(cause) => Settled::budget(cause),
        });
        result.entries.push(TrailEntry::of(anchor, settled));
    }
    let wanted: Vec<Declaration> = declarations
        .iter()
        .filter(|d| d.kind == Kind::ApiDescription)
        .cloned()
        .collect();
    let reads = gate.read_declared(&wanted, &upfront, DESCRIPTION_READ);
    for (d, read) in wanted.iter().zip(reads) {
        let settled = described(&read, &d.url);
        if let Ok(fetched) = read
            && settled.outcome == Outcome::Followed
        {
            result
                .descriptions
                .push((declaration_key(d.kind, &d.url), fetched));
        }
        result.entries.push(TrailEntry::of(d, settled));
    }
    result
}
