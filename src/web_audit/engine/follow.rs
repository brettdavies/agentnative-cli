//! The follow slice, a mirror of the site's `follow.ts`: one hop from the
//! audited site to the off-origin hosts it declares, under its own share of
//! the deadline.
//!
//! A card-document declaration is fetched to learn the endpoint it names.
//! An endpoint declaration gets one GET of its own URL, which may take one
//! redirect hop the slice validates and charges itself, then reciprocity on
//! the final host. Endpoints are tried in declaration order and the first
//! one admitted is the slice's endpoint, so the requests a declaration list
//! can cause stop at the endpoint the audit will use.
//!
//! The card documents declared ahead of every endpoint the slice could
//! admit are read side by side, each GET capped so a host that hangs leaves
//! the slice to the endpoints after it; host slots and the request cap
//! admit those GETs and their redirect hops in declaration order. A card
//! document declared after such an endpoint is read at its place, and only
//! while no endpoint is admitted. Batches settle in order on one clock:
//! what discovery's documents declare, then where its POSTs were redirected
//! off the audited origin, then the API catalog's declarations.
//!
//! Every failure of an endpoint's reciprocity records one outcome, so the
//! trail cannot be read as an oracle for what a third-party host serves. A
//! card document is the site's own declaration, read rather than confirmed,
//! so a card host that gives no response, or one the edge answers for,
//! records unreachable; any answer that yields no endpoint records
//! reciprocity-refused.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use super::api_follow::{ApiFollowResult, settle_api_declarations};
use super::discovery_documents::{
    RetainedDocument, parse_json_object, retained_card, strongest_card,
};
use super::follow_cards::{CARD_DOCUMENT_READ, CardDocumentRead, card_document_read};
use super::follow_refusals::{Upfront, settled_upfront};
use super::follow_requests::{FOLLOW_SLICE, Gate, ReadOptions, SliceClock};
use super::follow_trail::{
    Declaration, Kind, Outcome, Settled, TrailEntry, declaration_key, declares_host,
    normalize_endpoint_url, unique,
};
use super::reciprocity::{MetadataMatch, admitting_artifact};
use super::types::FetchHandle;
use crate::web_audit::fetch::body::STATUS_ONLY_BODY_BYTES;
use crate::web_audit::locality::Locality;
use crate::web_audit::registry::McpDiscovery;

// A streamable-HTTP endpoint may hold a GET open as an event stream; the GET
// only looks for a redirect and a challenge, so a hang must not spend the
// slice reciprocity needs.
const ENDPOINT_GET: ReadOptions = ReadOptions {
    max_body_bytes: STATUS_ONLY_BODY_BYTES,
    accept: None,
    timeout_cap: Some(Duration::from_millis(3_000)),
};

/// The slice's inputs.
#[derive(Clone, Copy, Debug)]
pub struct FollowInput<'a> {
    /// The audited origin.
    pub base: &'a str,
    /// Following is on.
    pub enabled: bool,
    /// A card already named an endpoint on the audited origin, which no
    /// declared host can displace.
    pub entry_endpoint_declared: bool,
    /// Where reciprocity reads a host's card and catalog.
    pub discovery: &'a McpDiscovery,
    /// The run's transport and resolver.
    pub fetch: &'a FetchHandle,
    /// The target's locality; a declaration of another class is blocked.
    pub target_locality: Option<Locality>,
    /// The per-request timeout.
    pub timeout: Duration,
    /// The per-audit deadline.
    pub deadline: Instant,
    /// When the slice opened; its clock runs from here.
    pub opened: Instant,
}

/// What the slice settled.
#[derive(Clone, Debug, Default)]
pub struct FollowResult {
    /// The first declared endpoint reciprocity admitted, pinned to its final URL.
    pub endpoint: Option<String>,
    /// The RFC 9728 metadata that admitted that endpoint, when metadata did.
    pub endpoint_metadata: Option<MetadataMatch>,
    /// The card the slice read for that endpoint: the card document that
    /// named it or the card that admitted it, whichever stands higher.
    pub endpoint_card: Option<RetainedDocument>,
    /// Per declaration key: its entry, then the entries of what a followed
    /// card document declared.
    pub entries: HashMap<String, Vec<TrailEntry>>,
    /// The API catalog's trail entries and the descriptions the slice read.
    pub api: ApiFollowResult,
}

/// The follow slice, opened once per audit.
pub struct FollowSession<'a> {
    input: FollowInput<'a>,
    gate: Gate<'a>,
    walked: HashSet<String>,
    naming_card: Option<RetainedDocument>,
    admitting_card: Option<RetainedDocument>,
    result: FollowResult,
}

impl std::fmt::Debug for FollowSession<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FollowSession")
            .field("gate", &self.gate)
            .field("endpoint", &self.result.endpoint)
            .finish_non_exhaustive()
    }
}

fn upfront_of(input: &FollowInput<'_>, d: &Declaration) -> Option<Settled> {
    settled_upfront(
        d,
        &Upfront {
            enabled: input.enabled,
            entry_endpoint_declared: input.entry_endpoint_declared,
            fetch: input.fetch,
            target_locality: input.target_locality,
        },
    )
}

/// Opens the slice; its clock runs from `input.opened`.
pub fn open_follow(input: FollowInput<'_>) -> FollowSession<'_> {
    let clock = SliceClock {
        timeout: input.timeout,
        deadline_at: input.deadline.min(input.opened + FOLLOW_SLICE),
    };
    FollowSession {
        gate: Gate::new(input.fetch, input.target_locality, clock),
        input,
        walked: HashSet::new(),
        naming_card: None,
        admitting_card: None,
        result: FollowResult::default(),
    }
}

impl FollowSession<'_> {
    fn upfront(&self, d: &Declaration) -> Option<Settled> {
        upfront_of(&self.input, d)
    }

    /// How a declaration settles with no request: before the slice, or once
    /// an endpoint is admitted.
    fn without_request(&self, d: &Declaration) -> Option<Settled> {
        self.upfront(d).or_else(|| {
            self.result
                .endpoint
                .is_some()
                .then(|| Settled::not_followed("beyond-endpoint-of-record"))
        })
    }

    fn read_card_documents(&mut self, declarations: &[Declaration]) -> Vec<CardDocumentRead> {
        let input = self.input;
        let reads =
            self.gate
                .read_declared(declarations, |d| upfront_of(&input, d), CARD_DOCUMENT_READ);
        declarations
            .iter()
            .zip(reads)
            .map(|(d, read)| card_document_read(self.input.base, d, read))
            .collect()
    }

    fn settle_endpoint(&mut self, d: &Declaration) -> Settled {
        if let Err(cause) = self.gate.enter(&d.url) {
            return Settled::budget(cause);
        }
        let fetched = match self.gate.fetch_declared(d, ENDPOINT_GET) {
            Ok(fetched) => fetched,
            Err(settled) => return settled,
        };
        let Some(pinned) = normalize_endpoint_url(&fetched.url) else {
            return Settled::of(Outcome::Blocked);
        };
        let final_url =
            (Some(&pinned) != normalize_endpoint_url(&d.url).as_ref()).then(|| pinned.clone());
        let challenge = fetched.response.headers.get("www-authenticate");
        let admitted =
            match admitting_artifact(&mut self.gate, &pinned, self.input.discovery, challenge) {
                Ok(admitted) => admitted,
                Err(cause) => return Settled::budget(cause),
            };
        let Some(admitted) = admitted else {
            return Settled::landed(final_url, Outcome::ReciprocityRefused);
        };
        self.result.endpoint = Some(pinned);
        self.result.endpoint_metadata = admitted.metadata;
        self.admitting_card = admitted
            .card
            .map(|card| retained_card(&card.url, &card.response));
        Settled {
            admitted_by: Some(admitted.by),
            ..Settled::landed(final_url, Outcome::Followed)
        }
    }

    /// Endpoints in declaration order, stopping at the first one admitted.
    fn settle_endpoints(&mut self, declarations: &[Declaration]) -> Vec<TrailEntry> {
        declarations
            .iter()
            .map(|d| {
                let settled = self
                    .without_request(d)
                    .unwrap_or_else(|| self.settle_endpoint(d));
                TrailEntry::of(d, settled)
            })
            .collect()
    }

    /// A URL declared twice settles once, at its first place.
    fn first_seen(&mut self, d: &Declaration) -> bool {
        self.walked.insert(declaration_key(d.kind, &d.url))
    }

    /// Settles a batch of declarations in order, after every earlier batch.
    /// A URL an earlier batch settled keeps its first entry.
    pub fn settle(&mut self, batch: &[Declaration]) {
        let off_origin: Vec<Declaration> = batch
            .iter()
            .filter(|d| declares_host(d, self.input.base))
            .cloned()
            .collect();
        let declared: Vec<Declaration> = unique(&off_origin)
            .into_iter()
            .filter(|d| !self.walked.contains(&declaration_key(d.kind, &d.url)))
            .collect();
        let first_endpoint = declared
            .iter()
            .position(|d| d.kind == Kind::McpEndpoint && self.upfront(d).is_none())
            .unwrap_or(declared.len());
        let ahead: Vec<Declaration> = declared[..first_endpoint]
            .iter()
            .filter(|d| d.kind == Kind::CardDocument)
            .cloned()
            .collect();
        let mut card_reads: HashMap<String, CardDocumentRead> = ahead
            .iter()
            .map(|d| declaration_key(d.kind, &d.url))
            .zip(self.read_card_documents(&ahead))
            .collect();
        for d in &declared {
            if !self.first_seen(d) {
                continue;
            }
            let key = declaration_key(d.kind, &d.url);
            if d.kind == Kind::McpEndpoint {
                let entries = self.settle_endpoints(std::slice::from_ref(d));
                self.result.entries.insert(key, entries);
                continue;
            }
            let read = match card_reads.remove(&key) {
                Some(read) => read,
                None => match self.without_request(d) {
                    Some(settled) => card_document_read(self.input.base, d, Err(settled)),
                    None => self
                        .read_card_documents(std::slice::from_ref(d))
                        .pop()
                        .expect("one read"),
                },
            };
            let admitted_before = self.result.endpoint.is_some();
            let named: Vec<Declaration> = read
                .named
                .iter()
                .filter(|n| self.first_seen(n))
                .cloned()
                .collect();
            let mut entries = vec![read.entry];
            entries.extend(self.settle_endpoints(&named));
            self.result.entries.insert(key, entries);
            if !admitted_before && self.result.endpoint.is_some() && read.card.is_some() {
                self.naming_card = read.card;
            }
        }
    }

    /// Settles the API catalog's declarations as one batch, after every
    /// earlier batch. The audited site's own MCP endpoint displaces only
    /// other MCP endpoints, never an API host.
    pub fn settle_api(&mut self, declarations: &[Declaration]) {
        let off_origin: Vec<Declaration> = declarations
            .iter()
            .filter(|d| declares_host(d, self.input.base))
            .cloned()
            .collect();
        let declared = unique(&off_origin);
        let upfront = Upfront {
            enabled: self.input.enabled,
            entry_endpoint_declared: false,
            fetch: self.input.fetch,
            target_locality: self.input.target_locality,
        };
        self.result.api =
            settle_api_declarations(&mut self.gate, &declared, |d| settled_upfront(d, &upfront));
    }

    /// What the slice settled.
    pub fn result(self) -> FollowResult {
        let cards: Vec<RetainedDocument> = [self.naming_card, self.admitting_card]
            .into_iter()
            .flatten()
            .collect();
        FollowResult {
            endpoint_card: strongest_card(cards, |card| parse_json_object(&card.response)),
            ..self.result
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;
    use std::sync::Arc;

    use super::*;
    use crate::web_audit::locality::Resolver;
    use crate::web_audit::mock::{MockResponse, MockTransport};
    use crate::web_audit::registry::MCP_DISCOVERY;

    struct PublicResolver;

    impl Resolver for PublicResolver {
        fn resolve(&self, _host: &str) -> Result<Vec<IpAddr>, String> {
            Ok(vec!["93.184.216.34".parse().unwrap()])
        }
    }

    fn declared(kind: Kind, url: &str) -> Declaration {
        Declaration::new(
            kind,
            url.to_string(),
            "/.well-known/api-catalog#/linkset/0".to_string(),
        )
    }

    #[test]
    fn the_clock_runs_from_when_the_slice_opened_not_from_each_batch() {
        let transport = Arc::new(MockTransport::new(
            vec![],
            MockResponse::ok(200, &[], b"{}"),
        ));
        let fetch = FetchHandle::new(transport.clone(), Arc::new(PublicResolver), "anc-test/0");
        let now = Instant::now();
        let mut session = open_follow(FollowInput {
            base: "https://example.com/",
            enabled: true,
            entry_endpoint_declared: false,
            discovery: &MCP_DISCOVERY,
            fetch: &fetch,
            target_locality: None,
            timeout: Duration::from_secs(2),
            deadline: now + Duration::from_secs(20),
            opened: now
                .checked_sub(FOLLOW_SLICE)
                .expect("an instant six seconds back"),
        });
        session.settle(&[declared(Kind::McpEndpoint, "https://mcp.test/mcp")]);
        session.settle_api(&[
            declared(Kind::ApiAnchor, "https://api.test/"),
            declared(Kind::ApiDescription, "https://api.test/openapi.json"),
        ]);
        let result = session.result();
        let mut settled: Vec<(Outcome, Option<&str>)> = result
            .entries
            .values()
            .flatten()
            .chain(&result.api.entries)
            .map(|e| (e.settled.outcome, e.settled.cause))
            .collect();
        settled.dedup();
        assert_eq!(settled, [(Outcome::BudgetExceeded, Some("slice"))]);
        assert!(transport.requests().is_empty());
    }
}
