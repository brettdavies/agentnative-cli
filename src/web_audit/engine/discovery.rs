//! MCP endpoint discovery on the audited origin, SEP-2127 first, a mirror
//! of the site's `discovery.ts`.
//!
//! The document reads come first: one concurrent round of the SEP-1649
//! well-known cards, the AI catalog, and the API catalog, then the
//! catalog's first MCP server-card entries: an inline card is read in
//! place, a card URL on the audited origin is fetched, and a card URL
//! anywhere else is recorded as a declaration and never requested. A
//! card's endpoint on the audited origin, catalog cards first, is the
//! endpoint. Without one, the legacy `initialize` and modern `tools/list`
//! POSTs go to the common paths together under one timeout, and a legacy
//! answer wins (`discovery_posts`). With an endpoint and no catalog card,
//! `<endpoint>` + `card_suffix` is read last.
//!
//! A card's endpoint is attacker-controlled, so one off the audited origin,
//! or one written as a URL template, is recorded and never probed.
//!
//! The card of record is the first card in SEP-2127 order: a catalog card,
//! then the suffix card when it is a card of either generation, then a
//! SEP-1649 well-known card; with none that parses, a published card that
//! claims JSON and does not parse is kept so the card check reads it
//! broken. The card of record, the AI catalog, and the API catalog are
//! kept for later checks, each read under [`DOCUMENT_MAX_BODY_BYTES`].
//!
//! Each pass probes its requests concurrently under one slice bounded by
//! the per-audit deadline and the discovery budget, so a target that never
//! answers cannot outrun the audit budget.

use std::time::{Duration, Instant};

use url::Url;

use super::discovery_documents::{
    AI_CATALOG_DOCUMENT, API_CATALOG_DOCUMENT, CatalogCardEntry, MCP_SERVER_CARD_TYPE,
    RetainedDocument, RetainedDocuments, ai_catalog_shape, api_catalog_shape, card_suffix_url,
    catalog_card_entries, document_item, parse_json_object, preferred_card, read_card, same_origin,
};
use super::discovery_posts::{Candidate, probe_common_paths};
use super::discovery_walk::{CatalogSlot, Walk};
use super::follow_trail::Declaration;
use super::pool::run_bounded;
use super::types::FetchHandle;
use crate::web_audit::fetch::body::DOCUMENT_MAX_BODY_BYTES;
use crate::web_audit::fetch::{FetchInit, FetchOptions, ProbeResponse};
use crate::web_audit::registry::McpDiscovery;
use crate::web_audit::scorecard::EvidenceItem;

/// Hard wall-clock cap on the whole discovery phase, so it never starves
/// the check waves.
pub const DISCOVERY_BUDGET: Duration = Duration::from_millis(12_000);

/// The RFC 9727 API catalog location (the site's `API_CATALOG_PATH`).
pub const API_CATALOG_PATH: &str = "/.well-known/api-catalog";

/// Slack past a pass's slice before an unreturned probe counts as failed.
const PASS_GRACE: Duration = Duration::from_millis(250);

/// Where discovery landed, how it got there, and what it kept.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiscoveryResult {
    /// The endpoint, when a card or a POST found one.
    pub endpoint: Option<String>,
    /// The `mcp_discovery` trail: every document read, then the POSTs.
    pub evidence: Vec<EvidenceItem>,
    /// What the documents declare, in SEP-2127 order.
    pub declarations: Vec<Declaration>,
    /// Where common paths redirected their POSTs off the audited origin, in
    /// probe order.
    pub redirected: Vec<Declaration>,
    /// The documents later checks score, by retained-document key.
    pub documents: RetainedDocuments,
    /// When the document reads ended, before any POST: where the site's
    /// engine opens its follow slice, whose clock runs from here.
    pub follow_opened: Option<Instant>,
}

/// Join a path to the base, or pass an absolute URL through unchanged; empty
/// when the path cannot resolve. Mirrors the site's `resolveUrl`.
pub fn resolve_url(base: &str, path_or_url: &str) -> String {
    if path_or_url.is_empty() {
        return String::new();
    }
    if path_or_url.starts_with("http://") || path_or_url.starts_with("https://") {
        return path_or_url.to_string();
    }
    Url::parse(base)
        .and_then(|b| b.join(path_or_url))
        .map(|u| u.to_string())
        .unwrap_or_default()
}

/// The path and query of a URL, as the site's `pathOf` writes a source.
fn path_of(url: &str) -> String {
    Url::parse(url)
        .map(|u| match u.query().filter(|q| !q.is_empty()) {
            Some(query) => format!("{}?{query}", u.path()),
            None => u.path().to_string(),
        })
        .unwrap_or_default()
}

fn unanswered() -> ProbeResponse {
    ProbeResponse::failed("TimeoutError: deadline exceeded".to_string(), 0)
}

/// Send every request concurrently under `opts`; responses keep request
/// order, and one still unanswered past the slice reads as a timeout.
pub(super) fn fetch_all(
    fetch: &FetchHandle,
    requests: Vec<(String, FetchInit)>,
    opts: &FetchOptions,
) -> Vec<ProbeResponse> {
    let concurrency = requests.len();
    let deadline = Instant::now() + opts.timeout + PASS_GRACE;
    let jobs: Vec<(FetchHandle, String, FetchInit, FetchOptions)> = requests
        .into_iter()
        .map(|(url, init)| (fetch.clone(), url, init, opts.clone()))
        .collect();
    run_bounded(jobs, concurrency, deadline, |(fetch, url, init, opts)| {
        fetch.fetch(&url, &init, &opts)
    })
    .into_iter()
    .map(|r| r.unwrap_or_else(unanswered))
    .collect()
}

fn document_options(timeout: Duration) -> FetchOptions {
    FetchOptions {
        timeout,
        max_body_bytes: Some(DOCUMENT_MAX_BODY_BYTES),
        ..FetchOptions::default()
    }
}

// The extension's discovery flow asks for a card by its media type.
fn card_request() -> FetchInit {
    FetchInit {
        headers: vec![("accept".to_string(), MCP_SERVER_CARD_TYPE.to_string())],
        ..FetchInit::default()
    }
}

/// Discovery after its document reads, before any POST.
pub struct DiscoveryDocuments<'a> {
    walk: Walk<'a>,
    fetch: &'a FetchHandle,
    timeout: Duration,
    deadline_at: Instant,
    spent: bool,
}

impl std::fmt::Debug for DiscoveryDocuments<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiscoveryDocuments")
            .field("spent", &self.spent)
            .finish_non_exhaustive()
    }
}

/// Discover the MCP endpoint for `base` under the registry's configuration.
pub fn discover_mcp_endpoint(
    fetch: &FetchHandle,
    base: &str,
    cfg: &McpDiscovery,
    timeout: Duration,
    deadline: Instant,
) -> DiscoveryResult {
    read_discovery_documents(fetch, base, cfg, timeout, deadline).probe_endpoint()
}

fn slice_left(timeout: Duration, deadline_at: Instant) -> Option<Duration> {
    let slice = timeout.min(deadline_at.saturating_duration_since(Instant::now()));
    (!slice.is_zero()).then_some(slice)
}

/// The document reads that open discovery: the SEP-1649 cards, the AI
/// catalog and the API catalog, then the catalog's card entries.
pub fn read_discovery_documents<'a>(
    fetch: &'a FetchHandle,
    base: &'a str,
    cfg: &'a McpDiscovery,
    timeout: Duration,
    deadline: Instant,
) -> DiscoveryDocuments<'a> {
    let deadline_at = deadline.min(Instant::now() + DISCOVERY_BUDGET);
    let pass_budget = || slice_left(timeout, deadline_at);
    let done = |walk: Walk<'a>, spent: bool| DiscoveryDocuments {
        walk,
        fetch,
        timeout,
        deadline_at,
        spent,
    };
    let mut walk = Walk::new(cfg, base);

    // Document reads: the SEP-1649 cards, the AI catalog, the API catalog.
    let well_known: Vec<(&str, String)> = cfg
        .well_known
        .iter()
        .map(|path| (*path, resolve_url(base, path)))
        .filter(|(_, url)| !url.is_empty())
        .collect();
    let catalog_url = resolve_url(base, cfg.ai_catalog);
    let api_catalog_url = resolve_url(base, API_CATALOG_PATH);
    let Some(document_pass) = pass_budget() else {
        return done(walk, true);
    };
    let mut requests: Vec<(String, FetchInit)> = well_known
        .iter()
        .map(|(_, url)| (url.clone(), FetchInit::default()))
        .collect();
    requests.push((catalog_url.clone(), FetchInit::default()));
    requests.push((api_catalog_url.clone(), FetchInit::default()));
    let mut responses = fetch_all(fetch, requests, &document_options(document_pass));
    let api_catalog_resp = responses.pop().unwrap_or_else(unanswered);
    let catalog_resp = responses.pop().unwrap_or_else(unanswered);
    walk.well_known = well_known
        .iter()
        .zip(responses)
        .map(|((path, url), resp)| read_card(path, url, resp, false, base))
        .collect();
    let catalog = if catalog_resp.status == Some(200) {
        parse_json_object(&catalog_resp)
    } else {
        None
    };
    let api_catalog_shape = (api_catalog_resp.status == Some(200))
        .then(|| api_catalog_shape(parse_json_object(&api_catalog_resp).as_ref()));
    walk.document_statuses = vec![catalog_resp.status, api_catalog_resp.status];
    walk.document_items.push(document_item(
        cfg.ai_catalog,
        AI_CATALOG_DOCUMENT,
        &catalog_resp,
        (catalog_resp.status == Some(200)).then(|| ai_catalog_shape(catalog.as_ref())),
    ));
    walk.document_items.push(document_item(
        API_CATALOG_PATH,
        API_CATALOG_DOCUMENT,
        &api_catalog_resp,
        api_catalog_shape,
    ));
    walk.documents.insert(
        AI_CATALOG_DOCUMENT,
        RetainedDocument {
            url: catalog_url.clone(),
            response: catalog_resp.clone(),
            shape: None,
        },
    );
    walk.documents.insert(
        API_CATALOG_DOCUMENT,
        RetainedDocument {
            url: api_catalog_url,
            response: api_catalog_resp,
            shape: None,
        },
    );

    // The catalog's card entries: inline in place, same-origin URLs fetched.
    let entries: Vec<(CatalogCardEntry, Option<String>)> = catalog
        .as_ref()
        .map(catalog_card_entries)
        .unwrap_or_default()
        .into_iter()
        .map(|entry| {
            let url = match &entry {
                CatalogCardEntry::Url { url, .. } => resolve_url(&catalog_url, url),
                _ => String::new(),
            };
            (entry, (!url.is_empty()).then_some(url))
        })
        .collect();
    let fetched: Vec<&String> = entries
        .iter()
        .filter_map(|(_, url)| url.as_ref().filter(|u| same_origin(u, base)))
        .collect();
    let mut card_responses = if fetched.is_empty() {
        Vec::new()
    } else {
        let Some(card_pass) = pass_budget() else {
            return done(walk, true);
        };
        let requests = fetched
            .iter()
            .map(|url| ((*url).clone(), card_request()))
            .collect();
        fetch_all(fetch, requests, &document_options(card_pass))
    }
    .into_iter();
    for (entry, url) in entries {
        let slot = match (entry, url) {
            (CatalogCardEntry::Data { index, data }, _) => {
                let pointer = format!("#/entries/{index}/data");
                let response = ProbeResponse {
                    body: serde_json::to_string(&data).unwrap_or_default(),
                    ..catalog_resp.clone()
                };
                CatalogSlot::Read(Box::new(read_card(
                    &format!("{}{pointer}", cfg.ai_catalog),
                    &format!("{catalog_url}{pointer}"),
                    response,
                    true,
                    base,
                )))
            }
            (CatalogCardEntry::Url { index, .. }, Some(url)) if !same_origin(&url, base) => {
                CatalogSlot::Declaration { index, url }
            }
            (CatalogCardEntry::Url { .. }, Some(url)) => {
                let response = card_responses.next().unwrap_or_else(unanswered);
                CatalogSlot::Read(Box::new(read_card(
                    &path_of(&url),
                    &url,
                    response,
                    false,
                    base,
                )))
            }
            (CatalogCardEntry::Url { index, .. }, None)
            | (CatalogCardEntry::Neither { index }, _) => CatalogSlot::Neither { index },
        };
        walk.catalog_slots.push(slot);
    }

    walk.documents_read = Some(Instant::now());
    done(walk, false)
}

impl DiscoveryDocuments<'_> {
    /// What the documents declare so far, in SEP-2127 order; a suffix card
    /// read later can add more.
    pub fn declarations(&self) -> Vec<Declaration> {
        self.walk.declarations()
    }

    /// An endpoint a card names on the audited origin, which no POST can displace.
    pub fn card_endpoint(&self) -> Option<String> {
        self.walk.card_endpoint()
    }

    /// The status of every document read; `None` where it got no answer.
    pub fn statuses(&self) -> Vec<Option<u16>> {
        self.walk.statuses()
    }

    /// A document read so far, by retained-document key.
    pub fn document(&self, key: &str) -> Option<&RetainedDocument> {
        self.walk.documents.get(key)
    }

    /// The common-path POSTs and the suffix-card read that finish discovery.
    pub fn probe_endpoint(self) -> DiscoveryResult {
        let DiscoveryDocuments {
            mut walk,
            fetch,
            timeout,
            deadline_at,
            spent,
        } = self;
        if spent {
            return walk.exhausted();
        }
        let pass_budget = || slice_left(timeout, deadline_at);
        let (cfg, base) = (walk.cfg, walk.base);

        // POST probing, only when no card named an endpoint on the audited origin.
        let candidates: Vec<Candidate> = cfg
            .common_paths
            .iter()
            .map(|path| Candidate {
                path,
                url: resolve_url(base, path),
            })
            .filter(|c| !c.url.is_empty())
            .collect();
        if walk.card_endpoint().is_none() && !candidates.is_empty() {
            let Some(post_pass) = pass_budget() else {
                return walk.exhausted();
            };
            let probed = probe_common_paths(fetch, &candidates, cfg.protocol_version, post_pass);
            walk.post_items = probed.items;
            walk.post_endpoint = probed.endpoint;
            walk.post_redirected = probed.redirected;
        }

        // With no catalog card, the endpoint's own card outranks a SEP-1649 card.
        let endpoint = walk.card_endpoint().or_else(|| walk.post_endpoint.clone());
        if let Some(endpoint) = endpoint
            && preferred_card(&walk.catalog_reads()).is_none()
        {
            let Some(suffix_pass) = pass_budget() else {
                return walk.exhausted();
            };
            let url = card_suffix_url(&endpoint, cfg.card_suffix);
            let response = fetch_all(
                fetch,
                vec![(url.clone(), card_request())],
                &document_options(suffix_pass),
            )
            .pop()
            .unwrap_or_else(unanswered);
            walk.suffix = Some(read_card(&path_of(&url), &url, response, false, base));
        }
        walk.finish()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::Value;

    use super::*;
    use crate::web_audit::engine::discovery_documents::SERVER_CARD_DOCUMENT;
    use crate::web_audit::locality::Resolver;
    use crate::web_audit::mock::{MockResponse, MockRule, MockTransport};
    use crate::web_audit::transport::Request;

    struct PublicResolver;
    impl Resolver for PublicResolver {
        fn resolve(&self, _: &str) -> Result<Vec<std::net::IpAddr>, String> {
            Ok(vec!["93.184.216.34".parse().unwrap()])
        }
    }

    const BASE: &str = "https://example.com/";

    fn cfg() -> McpDiscovery {
        McpDiscovery {
            ai_catalog: "/.well-known/ai-catalog.json",
            card_suffix: "/server-card",
            well_known: &["/.well-known/mcp.json", "/.well-known/mcp/server-card.json"],
            common_paths: &["/mcp", "/sse", "/message"],
            protocol_version: "2025-06-18",
        }
    }

    fn json_ok(body: &str) -> MockResponse {
        MockResponse::ok(
            200,
            &[("content-type", "application/json")],
            body.as_bytes(),
        )
    }

    fn get(url: &str, body: &str) -> MockRule {
        MockRule::new("GET", url).response(json_ok(body))
    }

    fn not_found() -> MockResponse {
        MockResponse::ok(404, &[], b"")
    }

    struct Run {
        out: DiscoveryResult,
        requests: Vec<Request>,
    }

    fn run_with(rules: Vec<MockRule>, deadline: Instant) -> Run {
        let transport = Arc::new(MockTransport::new(rules, not_found()));
        let fetch = FetchHandle::new(transport.clone(), Arc::new(PublicResolver), "test-ua");
        let out = discover_mcp_endpoint(&fetch, BASE, &cfg(), Duration::from_secs(2), deadline);
        Run {
            out,
            requests: transport.requests(),
        }
    }

    fn run(rules: Vec<MockRule>) -> Run {
        run_with(rules, Instant::now() + Duration::from_secs(10))
    }

    fn sent(run: &Run) -> Vec<String> {
        run.requests
            .iter()
            .map(|r| format!("{} {}", r.method, r.url))
            .collect()
    }

    fn sorted(mut items: Vec<String>) -> Vec<String> {
        items.sort();
        items
    }

    fn accept_of(request: &Request) -> Option<&str> {
        request
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("accept"))
            .map(|(_, value)| value.as_str())
    }

    fn evidence(run: &Run) -> String {
        serde_json::to_string(&run.out.evidence).unwrap()
    }

    const DOCUMENT_ROUND: [&str; 4] = [
        "GET https://example.com/.well-known/ai-catalog.json",
        "GET https://example.com/.well-known/api-catalog",
        "GET https://example.com/.well-known/mcp.json",
        "GET https://example.com/.well-known/mcp/server-card.json",
    ];

    #[test]
    fn a_silent_site_reads_every_document_then_posts_both_lanes_together() {
        let run = run(vec![]);
        let sent = sent(&run);
        assert_eq!(sent.len(), 10, "{sent:?}");
        assert_eq!(sorted(sent[..4].to_vec()), DOCUMENT_ROUND);
        assert!(sent[4..].iter().all(|r| r.starts_with("POST ")), "{sent:?}");
        let methods: Vec<String> = run.requests[4..]
            .iter()
            .map(|r| {
                serde_json::from_slice::<Value>(r.body.as_deref().unwrap()).unwrap()["method"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(methods.iter().filter(|m| *m == "initialize").count(), 3);
        assert_eq!(methods.iter().filter(|m| *m == "tools/list").count(), 3);
        assert_eq!(run.out.endpoint, None);
        assert_eq!(
            evidence(&run),
            concat!(
                r#"[{"source":"/.well-known/mcp.json","status":404},"#,
                r#"{"source":"/.well-known/mcp/server-card.json","status":404},"#,
                r#"{"source":"/.well-known/ai-catalog.json","document":"ai-catalog","status":404},"#,
                r#"{"source":"/.well-known/api-catalog","document":"api-catalog","status":404},"#,
                r#"{"source":"/mcp","status":404,"probed":"initialize (no serverInfo)"},"#,
                r#"{"source":"/sse","status":404,"probed":"initialize (no serverInfo)"},"#,
                r#"{"source":"/message","status":404,"probed":"initialize (no serverInfo)"},"#,
                r#"{"source":"/mcp","status":404,"probed":"modern-tools-list (no tools)"},"#,
                r#"{"source":"/sse","status":404,"probed":"modern-tools-list (no tools)"},"#,
                r#"{"source":"/message","status":404,"probed":"modern-tools-list (no tools)"}]"#,
            )
        );
        let mut kept: Vec<&str> = run.out.documents.keys().copied().collect();
        kept.sort();
        assert_eq!(kept, ["ai-catalog", "api-catalog"]);
    }

    #[test]
    fn a_catalog_card_wins_and_off_origin_cards_are_never_requested() {
        let catalog = format!(
            r#"{{"entries":[
                {{"type":"{t}","url":"https://cards.test/card.json"}},
                {{"type":"{t}"}},
                {{"type":"{t}","url":"/cards/one.json"}},
                {{"type":"{t}","data":{{"remotes":[{{"type":"streamable-http","url":"/inline"}}]}}}}
            ]}}"#,
            t = MCP_SERVER_CARD_TYPE
        );
        let run = run(vec![
            get("https://example.com/.well-known/ai-catalog.json", &catalog),
            get(
                "https://example.com/cards/one.json",
                r#"{"remotes":[{"type":"streamable-http","url":"https://example.com/rpc"}]}"#,
            ),
            get(
                "https://example.com/.well-known/mcp.json",
                r#"{"mcp_endpoint":"/mcp","authentication":{"type":"oauth2"}}"#,
            ),
        ]);
        let sent = sent(&run);
        assert_eq!(sorted(sent[..4].to_vec()), DOCUMENT_ROUND);
        assert_eq!(sent[4..], ["GET https://example.com/cards/one.json"]);
        assert_eq!(
            accept_of(&run.requests[4]),
            Some("application/mcp-server-card+json")
        );
        assert_eq!(run.out.endpoint.as_deref(), Some("https://example.com/rpc"));
        let card = &run.out.documents[SERVER_CARD_DOCUMENT];
        assert_eq!(card.url, "https://example.com/cards/one.json");
        assert_eq!(
            card.shape,
            Some(super::super::discovery_documents::CardShape::Sep2127)
        );
        let rows: Vec<&EvidenceItem> = run.out.evidence.iter().collect();
        assert_eq!(
            serde_json::to_string(&rows[4..]).unwrap(),
            concat!(
                r#"[{"source":"/.well-known/ai-catalog.json#/entries/0","card_url":"https://cards.test/card.json","blocked":"off-origin card declaration"},"#,
                r#"{"source":"/.well-known/ai-catalog.json#/entries/1","note":"catalog entry names no card url or data"},"#,
                r#"{"source":"/cards/one.json","document":"server-card","status":200,"shape":"sep-2127","endpoint":"https://example.com/rpc"},"#,
                r#"{"source":"/.well-known/ai-catalog.json#/entries/3/data","shape":"sep-2127","endpoint":"https://example.com/inline"}]"#,
            )
        );
    }

    #[test]
    fn a_well_known_endpoint_skips_the_posts_and_reads_the_suffix_card() {
        let run = run(vec![
            get(
                "https://example.com/.well-known/mcp/server-card.json",
                r#"{"transport":{"endpoint":"/rpc/"},"auth":{}}"#,
            ),
            get(
                "https://example.com/rpc/server-card",
                r#"{"remotes":[{"type":"streamable-http","url":"/rpc/"}]}"#,
            ),
        ]);
        let sent = sent(&run);
        assert_eq!(sent.len(), 5, "{sent:?}");
        assert_eq!(sent[4], "GET https://example.com/rpc/server-card");
        assert_eq!(
            accept_of(&run.requests[4]),
            Some("application/mcp-server-card+json")
        );
        assert_eq!(
            run.out.endpoint.as_deref(),
            Some("https://example.com/rpc/")
        );
        assert_eq!(
            run.out.documents[SERVER_CARD_DOCUMENT].url,
            "https://example.com/rpc/server-card"
        );
        assert_eq!(
            serde_json::to_string(&run.out.evidence[1]).unwrap(),
            r#"{"source":"/.well-known/mcp/server-card.json","status":200,"shape":"sep-1649","endpoint":"https://example.com/rpc/","authentication":true}"#
        );
        assert_eq!(
            serde_json::to_string(run.out.evidence.last().unwrap()).unwrap(),
            r#"{"source":"/rpc/server-card","document":"server-card","status":200,"shape":"sep-2127","endpoint":"https://example.com/rpc/"}"#
        );
    }

    #[test]
    fn a_suffix_answer_that_is_no_card_leaves_the_well_known_card_of_record() {
        let run = run(vec![
            get(
                "https://example.com/.well-known/mcp.json",
                r#"{"url":"/mcp"}"#,
            ),
            get(
                "https://example.com/mcp/server-card",
                r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601}}"#,
            ),
        ]);
        assert_eq!(
            run.out.documents[SERVER_CARD_DOCUMENT].url,
            "https://example.com/.well-known/mcp.json"
        );
        assert_eq!(
            run.out.evidence.last().unwrap().get("shape"),
            Some(&Value::from("unrecognized"))
        );
    }

    #[test]
    fn a_legacy_answer_outranks_a_modern_one_and_the_endpoint_card_is_read_last() {
        let run = run(vec![
            MockRule::new("POST", "https://example.com/message")
                .body_json_method("initialize")
                .response(json_ok(
                    r#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"name":"x"}}}"#,
                )),
            MockRule::new("POST", "https://example.com/mcp")
                .header("mcp-method", "tools/list")
                .response(json_ok(r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#)),
        ]);
        assert_eq!(
            run.out.endpoint.as_deref(),
            Some("https://example.com/message")
        );
        let sent = sent(&run);
        assert_eq!(sent.len(), 11, "{sent:?}");
        assert_eq!(sent[10], "GET https://example.com/message/server-card");
        let probed: Vec<&str> = run
            .out
            .evidence
            .iter()
            .filter_map(|e| e.get("probed").and_then(Value::as_str))
            .collect();
        assert_eq!(
            probed,
            [
                "initialize (no serverInfo)",
                "initialize (no serverInfo)",
                "initialize"
            ]
        );
    }

    #[test]
    fn off_origin_and_templated_endpoints_are_recorded_and_never_probed() {
        let run = run(vec![
            get(
                "https://example.com/.well-known/mcp.json",
                r#"{"url":"https://victim.test/mcp"}"#,
            ),
            get(
                "https://example.com/.well-known/mcp/server-card.json",
                r#"{"remotes":[{"type":"streamable-http","url":"https://{tenant}.example.com/mcp"}]}"#,
            ),
        ]);
        assert!(
            run.requests
                .iter()
                .all(|r| r.url.starts_with("https://example.com/")),
            "{:?}",
            sent(&run)
        );
        assert_eq!(run.out.endpoint, None);
        assert_eq!(
            run.out.evidence[0].get("blocked"),
            Some(&Value::from("off-origin endpoint declaration"))
        );
        assert_eq!(
            run.out.evidence[1].get("not_followed"),
            Some(&Value::from("templated url"))
        );
        assert_eq!(
            run.out.documents[SERVER_CARD_DOCUMENT].url,
            "https://example.com/.well-known/mcp/server-card.json"
        );
    }

    #[test]
    fn a_document_over_its_cap_is_truncated_and_unparseable() {
        let big = format!(
            r#"{{"linkset":[],"pad":"{}"}}"#,
            "x".repeat(DOCUMENT_MAX_BODY_BYTES)
        );
        let run = run(vec![get(
            "https://example.com/.well-known/api-catalog",
            &big,
        )]);
        assert_eq!(
            serde_json::to_string(&run.out.evidence[3]).unwrap(),
            r#"{"source":"/.well-known/api-catalog","document":"api-catalog","status":200,"truncated":true,"shape":"unparseable"}"#
        );
        let kept = &run.out.documents[API_CATALOG_DOCUMENT].response;
        assert_eq!(kept.body.len(), DOCUMENT_MAX_BODY_BYTES);
    }

    #[test]
    fn a_spent_budget_sends_nothing_and_ends_with_a_note() {
        let run = run_with(vec![], Instant::now());
        assert!(run.requests.is_empty());
        assert_eq!(run.out.endpoint, None);
        assert_eq!(
            evidence(&run),
            r#"[{"note":"per-audit deadline exceeded during discovery"}]"#
        );
    }

    #[test]
    fn resolve_url_and_path_of_mirror_the_site() {
        assert_eq!(resolve_url(BASE, "/mcp"), "https://example.com/mcp");
        assert_eq!(
            resolve_url(BASE, "https://other.test/x"),
            "https://other.test/x"
        );
        assert_eq!(resolve_url(BASE, ""), "");
        assert_eq!(resolve_url("not a url", "/x"), "");
        assert_eq!(path_of("https://example.com/a/b?x=1#f"), "/a/b?x=1");
        assert_eq!(path_of("https://example.com/a?"), "/a");
    }
}
