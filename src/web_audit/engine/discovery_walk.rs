//! What MCP discovery has read so far, and the result it finishes with:
//! the evidence trail, the card of record, and the declarations its
//! documents and POSTs leave for the follow slice.

use std::time::Instant;

use serde_json::json;

use super::discovery::DiscoveryResult;
use super::discovery_documents::{
    CardRead, RetainedDocuments, SERVER_CARD_DOCUMENT, card_declarations, card_has_auth_field,
    card_item, is_card_of_either_generation, preferred_card, retained_card,
    unparseable_published_card,
};
use super::follow_trail::{Declaration, Kind};
use super::sign_in::SignInEndpoint;
use crate::web_audit::registry::McpDiscovery;
use crate::web_audit::scorecard::EvidenceItem;

/// One server-card entry of the AI catalog, as discovery settled it.
pub(super) enum CatalogSlot {
    /// Read inline or from the audited origin.
    Read(Box<CardRead>),
    /// A card URL on another origin, never requested.
    Declaration { index: usize, url: String },
    /// An entry naming no card URL or data.
    Neither { index: usize },
}

impl CatalogSlot {
    fn read(&self) -> Option<&CardRead> {
        match self {
            CatalogSlot::Read(read) => Some(read),
            _ => None,
        }
    }
}

/// The endpoint a card names on the audited origin, catalog cards first.
fn card_endpoint_of(catalog: &[&CardRead], well_known: &[CardRead]) -> Option<String> {
    catalog
        .iter()
        .copied()
        .chain(well_known)
        .find_map(|r| r.endpoint.clone())
}

/// What discovery has read so far.
pub(super) struct Walk<'a> {
    pub(super) cfg: &'a McpDiscovery,
    pub(super) base: &'a str,
    pub(super) well_known: Vec<CardRead>,
    pub(super) document_items: Vec<EvidenceItem>,
    pub(super) document_statuses: Vec<Option<u16>>,
    pub(super) catalog_slots: Vec<CatalogSlot>,
    pub(super) post_items: Vec<EvidenceItem>,
    pub(super) post_endpoint: Option<String>,
    pub(super) post_redirected: Vec<Declaration>,
    pub(super) post_sign_in: Option<SignInEndpoint>,
    pub(super) suffix: Option<CardRead>,
    pub(super) deadline_hit: bool,
    pub(super) documents: RetainedDocuments,
    pub(super) documents_read: Option<Instant>,
}

impl<'a> Walk<'a> {
    pub(super) fn new(cfg: &'a McpDiscovery, base: &'a str) -> Self {
        Walk {
            cfg,
            base,
            well_known: Vec::new(),
            document_items: Vec::new(),
            document_statuses: Vec::new(),
            catalog_slots: Vec::new(),
            post_items: Vec::new(),
            post_endpoint: None,
            post_redirected: Vec::new(),
            post_sign_in: None,
            suffix: None,
            deadline_hit: false,
            documents: RetainedDocuments::new(),
            documents_read: None,
        }
    }

    pub(super) fn catalog_reads(&self) -> Vec<&CardRead> {
        self.catalog_slots
            .iter()
            .filter_map(CatalogSlot::read)
            .collect()
    }

    pub(super) fn card_endpoint(&self) -> Option<String> {
        card_endpoint_of(&self.catalog_reads(), &self.well_known)
    }

    /// What the documents declare, in SEP-2127 order: the catalog's
    /// entries, the suffix card, then the well-known cards.
    pub(super) fn declarations(&self) -> Vec<Declaration> {
        let mut out = Vec::new();
        for slot in &self.catalog_slots {
            match slot {
                CatalogSlot::Read(read) => out.extend(card_declarations(read, self.base)),
                CatalogSlot::Declaration { index, url } => out.push(Declaration::new(
                    Kind::CardDocument,
                    url.clone(),
                    format!("{}#/entries/{index}", self.cfg.ai_catalog),
                )),
                CatalogSlot::Neither { .. } => {}
            }
        }
        for read in self.suffix.iter().chain(&self.well_known) {
            out.extend(card_declarations(read, self.base));
        }
        out
    }

    /// The status of every document read; `None` where it got no answer.
    pub(super) fn statuses(&self) -> Vec<Option<u16>> {
        let catalog = self.catalog_reads();
        let fetched = catalog.iter().filter(|r| !r.inline);
        self.well_known
            .iter()
            .map(|r| r.response.status)
            .chain(self.document_statuses.iter().copied())
            .chain(fetched.map(|r| r.response.status))
            .collect()
    }

    pub(super) fn exhausted(mut self) -> DiscoveryResult {
        self.deadline_hit = true;
        self.finish()
    }

    pub(super) fn finish(self) -> DiscoveryResult {
        let declarations = self.declarations();
        let Walk {
            cfg,
            base: _,
            well_known,
            document_items,
            document_statuses: _,
            catalog_slots,
            post_items,
            post_endpoint,
            post_redirected,
            post_sign_in,
            suffix,
            deadline_hit,
            mut documents,
            documents_read,
        } = self;
        let catalog: Vec<&CardRead> = catalog_slots.iter().filter_map(CatalogSlot::read).collect();
        let known: Vec<&CardRead> = well_known.iter().collect();
        let endpoint = card_endpoint_of(&catalog, &well_known).or(post_endpoint.clone());
        let found_by_post = endpoint.is_some() && endpoint == post_endpoint;
        let sign_in = post_sign_in.filter(|_| found_by_post);
        // The suffix URL is a guess the site never published, and an MCP
        // handler can answer it with any JSON object, a JSON-RPC error
        // included.
        let suffix_card = suffix
            .as_ref()
            .filter(|s| is_card_of_either_generation(s.card.as_ref()));
        let published: Vec<&CardRead> = catalog.iter().chain(&known).copied().collect();
        let record = preferred_card(&catalog)
            .or(suffix_card)
            .or_else(|| preferred_card(&known))
            .or_else(|| unparseable_published_card(&published));
        if let Some(record) = record {
            documents.insert(
                SERVER_CARD_DOCUMENT,
                retained_card(&record.url, &record.response),
            );
        }
        // SEP-2127 cards carry no auth field, so the declaration comes from
        // whichever card of either generation names the discovered endpoint.
        let auth_card = endpoint.as_ref().and_then(|endpoint| {
            catalog
                .iter()
                .copied()
                .chain(suffix.as_ref())
                .chain(&well_known)
                .find(|r| {
                    r.endpoint.as_ref() == Some(endpoint)
                        && r.card.as_ref().is_some_and(card_has_auth_field)
                })
        });
        let is = |a: Option<&CardRead>, b: &CardRead| a.is_some_and(|a| std::ptr::eq(a, b));
        let item = |read: &CardRead| card_item(read, is(record, read), is(auth_card, read));
        let catalog_item = |slot: &CatalogSlot| -> EvidenceItem {
            let value = match slot {
                CatalogSlot::Read(read) => return item(read),
                CatalogSlot::Declaration { index, url } => json!({
                    "source": format!("{}#/entries/{index}", cfg.ai_catalog),
                    "card_url": url,
                    "blocked": "off-origin card declaration",
                }),
                CatalogSlot::Neither { index } => json!({
                    "source": format!("{}#/entries/{index}", cfg.ai_catalog),
                    "note": "catalog entry names no card url or data",
                }),
            };
            value.as_object().cloned().unwrap_or_default()
        };
        let mut evidence: Vec<EvidenceItem> = well_known.iter().map(item).collect();
        evidence.extend(document_items);
        evidence.extend(catalog_slots.iter().map(catalog_item));
        evidence.extend(post_items);
        evidence.extend(suffix.as_ref().map(item));
        if deadline_hit {
            evidence.push(
                json!({ "note": "per-audit deadline exceeded during discovery" })
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            );
        }
        // The audited site serves its own endpoint, so where its other paths
        // redirect can never become the endpoint of record.
        let redirected = post_redirected
            .into_iter()
            .map(|d| Declaration {
                not_followed: endpoint.is_some().then_some("beyond-endpoint-of-record"),
                ..d
            })
            .collect();
        DiscoveryResult {
            endpoint,
            endpoint_metadata: sign_in.as_ref().map(|s| s.metadata.clone()),
            endpoint_challenge: sign_in.map(|s| s.challenge),
            evidence,
            declarations,
            redirected,
            documents,
            follow_opened: Some(documents_read.unwrap_or_else(Instant::now)),
        }
    }
}
