//! Pure logic for the documents MCP discovery reads, a mirror of the site's
//! `discovery-documents.ts`: parsing and shape classification for server
//! cards of both generations, the AI catalog entries that name cards, and
//! the API catalog; the endpoint a card declares; and the evidence row each
//! read leaves. No I/O here.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};
use url::Url;

use super::discovery::resolve_url;
use super::follow_trail::{Declaration, Kind};
use crate::web_audit::fetch::ProbeResponse;
use crate::web_audit::pattern::fixed;
use crate::web_audit::scorecard::EvidenceItem;

/// The media type an AI catalog entry names a server card by, and the
/// `Accept` a card read sends (the site's `MCP_SERVER_CARD_TYPE`).
pub const MCP_SERVER_CARD_TYPE: &str = "application/mcp-server-card+json";

/// Retained-document key of the AI catalog.
pub const AI_CATALOG_DOCUMENT: &str = "ai-catalog";
/// Retained-document key of the RFC 9727 API catalog.
pub const API_CATALOG_DOCUMENT: &str = "api-catalog";
/// Retained-document key of the card of record.
pub const SERVER_CARD_DOCUMENT: &str = "server-card";

// Bounds the card reads one catalog can cause, whatever it lists.
const MAX_CATALOG_CARDS: usize = 4;

// The extension schema's remote URL template variable: `{name}`.
static URL_TEMPLATE_VARIABLE: LazyLock<Regex> =
    LazyLock::new(|| fixed(r"\{[A-Za-z_][A-Za-z0-9_]*\}"));

/// A JSON object body.
pub type JsonObject = Map<String, Value>;

/// The documents discovery keeps for later checks, by retained-document key.
pub type RetainedDocuments = HashMap<&'static str, RetainedDocument>;

/// A document discovery read and keeps for later checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetainedDocument {
    /// Where it was read; an inline catalog card is the catalog URL with a
    /// JSON Pointer fragment.
    pub url: String,
    /// The response as read.
    pub response: ProbeResponse,
    /// Server cards only.
    pub shape: Option<CardShape>,
}

/// A card with `remotes[]` is SEP-2127; one with `transport`,
/// `mcp_endpoint`, or `url` is SEP-1649.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardShape {
    /// Carries `remotes[]`.
    Sep2127,
    /// Carries a SEP-1649 endpoint field.
    Sep1649,
    /// Some other JSON object.
    Unrecognized,
    /// Not a JSON object, or cut at its cap.
    Unparseable,
}

impl CardShape {
    /// The evidence spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            CardShape::Sep2127 => "sep-2127",
            CardShape::Sep1649 => "sep-1649",
            CardShape::Unrecognized => "unrecognized",
            CardShape::Unparseable => "unparseable",
        }
    }
}

/// The body as a JSON object; a body cut at its byte cap never parses.
pub fn parse_json_object(resp: &ProbeResponse) -> Option<JsonObject> {
    if resp.truncated {
        return None;
    }
    match serde_json::from_str::<Value>(&resp.body) {
        Ok(Value::Object(map)) => Some(map),
        _ => None,
    }
}

/// The shape a card body classifies as.
pub fn card_shape(card: Option<&JsonObject>) -> CardShape {
    let Some(card) = card else {
        return CardShape::Unparseable;
    };
    if card.get("remotes").is_some_and(Value::is_array) {
        return CardShape::Sep2127;
    }
    if ["transport", "mcp_endpoint", "url"]
        .iter()
        .any(|k| card.contains_key(*k))
    {
        return CardShape::Sep1649;
    }
    CardShape::Unrecognized
}

/// Where a card stands as the card of record, highest first: a SEP-2127
/// card, a SEP-1649 card, any other JSON object, a body that does not
/// parse. A card without `remotes` that carries the `$schema` marker every
/// SEP-2127 card has stands as SEP-2127, since the extension schema does
/// not require `remotes`.
fn card_standing(card: Option<&JsonObject>) -> u8 {
    match card_shape(card) {
        CardShape::Sep2127 => 3,
        CardShape::Sep1649 => 2,
        CardShape::Unrecognized => {
            if card
                .and_then(|c| c.get("$schema"))
                .is_some_and(Value::is_string)
            {
                3
            } else {
                1
            }
        }
        CardShape::Unparseable => 0,
    }
}

/// The first of `items` whose card stands highest.
pub fn strongest_card<T>(items: Vec<T>, card_of: impl Fn(&T) -> Option<JsonObject>) -> Option<T> {
    let mut best: Option<(T, u8)> = None;
    for item in items {
        let standing = card_standing(card_of(&item).as_ref());
        if best.as_ref().is_none_or(|(_, b)| standing > *b) {
            best = Some((item, standing));
        }
    }
    best.map(|(item, _)| item)
}

/// Whether `candidate` stands higher as the card of record than `record`;
/// any card outranks none.
pub fn outranks(candidate: &RetainedDocument, record: Option<&RetainedDocument>) -> bool {
    record.is_none_or(|record| {
        card_standing(parse_json_object(&candidate.response).as_ref())
            > card_standing(parse_json_object(&record.response).as_ref())
    })
}

/// A card of either generation, which an arbitrary JSON object, a JSON-RPC
/// error included, is not.
pub fn is_card_of_either_generation(card: Option<&JsonObject>) -> bool {
    card_standing(card) >= 2
}

/// The shape an AI catalog body classifies as.
pub fn ai_catalog_shape(catalog: Option<&JsonObject>) -> &'static str {
    match catalog {
        None => "unparseable",
        Some(c) if c.get("entries").is_some_and(Value::is_array) => "ai-catalog",
        Some(_) => "unrecognized",
    }
}

/// The shape an API catalog body classifies as.
pub fn api_catalog_shape(catalog: Option<&JsonObject>) -> &'static str {
    match catalog {
        None => "unparseable",
        Some(c) if c.get("linkset").is_some_and(Value::is_array) => "linkset",
        Some(_) => "unrecognized",
    }
}

/// The URL of a SEP-2127 remote the endpoint can come from: streamable
/// HTTP with a string URL.
fn streamable_remote_url(remote: &Value) -> Option<&str> {
    let remote = remote.as_object()?;
    if remote.get("type").and_then(Value::as_str) != Some("streamable-http") {
        return None;
    }
    remote.get("url").and_then(Value::as_str)
}

/// The endpoint a card declares, as written: a SEP-2127 card's first
/// `streamable-http` remote, else the first SEP-1649 endpoint field present.
pub fn card_endpoint(card: &JsonObject) -> Option<String> {
    if let Some(remotes) = card.get("remotes").and_then(Value::as_array) {
        return remotes
            .iter()
            .find_map(streamable_remote_url)
            .map(str::to_string);
    }
    let transport = card.get("transport").and_then(Value::as_object);
    [
        card.get("mcp_endpoint"),
        card.get("url"),
        transport.and_then(|t| t.get("url")),
        transport.and_then(|t| t.get("endpoint")),
    ]
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
    .find(|s| !s.is_empty())
    .map(str::to_string)
}

/// A SEP-2127 card's remotes other than the one [`card_endpoint`] takes:
/// one card names one endpoint of record, so these are never followed.
pub fn other_remotes(card: &JsonObject, source: &str, base: &str) -> Vec<Declaration> {
    let Some(remotes) = card.get("remotes").and_then(Value::as_array) else {
        return Vec::new();
    };
    let primary = remotes
        .iter()
        .position(|r| streamable_remote_url(r).is_some());
    remotes
        .iter()
        .enumerate()
        .filter(|(index, _)| Some(*index) != primary)
        .filter_map(|(_, remote)| remote.as_object()?.get("url")?.as_str())
        .filter_map(|written| {
            let url = if is_templated_url(written) {
                written.to_string()
            } else {
                resolve_url(base, written)
            };
            (!url.is_empty()).then(|| Declaration {
                not_followed: Some("beyond-endpoint-of-record"),
                ..Declaration::new(Kind::McpEndpoint, url, source.to_string())
            })
        })
        .collect()
}

/// A card declares authentication the way SEP-1649 cards spell it.
pub fn card_has_auth_field(card: &JsonObject) -> bool {
    card.contains_key("authentication") || card.contains_key("auth")
}

/// A URL carrying a `{variable}` needs values the auditor does not have,
/// so it is never requested.
pub fn is_templated_url(url: &str) -> bool {
    URL_TEMPLATE_VARIABLE.is_match(url)
}

/// The card location the extension defines for an endpoint: its URL plus
/// the suffix, query and fragment dropped.
pub fn card_suffix_url(endpoint: &str, suffix: &str) -> String {
    let Ok(mut url) = Url::parse(endpoint) else {
        return String::new();
    };
    let path = format!("{}{suffix}", url.path().trim_end_matches('/'));
    url.set_path(&path);
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

/// One MCP server-card entry of an AI catalog.
#[derive(Clone, Debug, PartialEq)]
pub enum CatalogCardEntry {
    /// An inline card, read in place.
    Data {
        /// Position in `entries`.
        index: usize,
        /// The card.
        data: JsonObject,
    },
    /// A card URL, as written.
    Url {
        /// Position in `entries`.
        index: usize,
        /// The URL.
        url: String,
    },
    /// An entry naming neither.
    Neither {
        /// Position in `entries`.
        index: usize,
    },
}

/// The first MCP server-card entries of an AI catalog, inline `data`
/// preferred over `url`.
pub fn catalog_card_entries(catalog: &JsonObject) -> Vec<CatalogCardEntry> {
    let Some(entries) = catalog.get("entries").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        if out.len() == MAX_CATALOG_CARDS {
            break;
        }
        let Some(entry) = entry.as_object() else {
            continue;
        };
        if entry.get("type").and_then(Value::as_str) != Some(MCP_SERVER_CARD_TYPE) {
            continue;
        }
        if let Some(data) = entry.get("data").and_then(Value::as_object) {
            out.push(CatalogCardEntry::Data {
                index,
                data: data.clone(),
            });
        } else if let Some(url) = entry.get("url").and_then(Value::as_str) {
            out.push(CatalogCardEntry::Url {
                index,
                url: url.to_string(),
            });
        } else {
            out.push(CatalogCardEntry::Neither { index });
        }
    }
    out
}

/// Whether two URLs share an origin; false when either does not parse.
pub fn same_origin(candidate: &str, base: &str) -> bool {
    match (Url::parse(candidate), Url::parse(base)) {
        (Ok(c), Ok(b)) => c.origin() == b.origin(),
        _ => false,
    }
}

/// The endpoint a card declares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclaredEndpoint {
    /// Resolved against the audited origin, or as written when templated.
    pub url: String,
    /// Written as a URL template, so never requested.
    pub templated: bool,
}

/// A card discovery read, with the endpoint it declares.
#[derive(Clone, Debug, PartialEq)]
pub struct CardRead {
    /// Where it was declared: a path on the audited origin, or a catalog
    /// entry as a JSON Pointer.
    pub source: String,
    /// Where it was read.
    pub url: String,
    /// The response as read.
    pub response: ProbeResponse,
    /// An AI catalog's inline card: read in place, with no request.
    pub inline: bool,
    /// The parsed body, when it answered and parsed.
    pub card: Option<JsonObject>,
    /// The shape, when it answered.
    pub shape: Option<CardShape>,
    /// The endpoint the card declares, wherever it is.
    pub declaration: Option<DeclaredEndpoint>,
    /// The declared endpoint when it is on the audited origin and not a template.
    pub endpoint: Option<String>,
}

/// Read one card response.
pub fn read_card(
    source: &str,
    url: &str,
    response: ProbeResponse,
    inline: bool,
    base: &str,
) -> CardRead {
    let answered = inline || response.status == Some(200);
    let card = if answered {
        parse_json_object(&response)
    } else {
        None
    };
    let shape = answered.then(|| card_shape(card.as_ref()));
    let declared = card.as_ref().and_then(card_endpoint);
    let (declaration, endpoint) = match declared {
        None => (None, None),
        Some(written) if is_templated_url(&written) => (
            Some(DeclaredEndpoint {
                url: written,
                templated: true,
            }),
            None,
        ),
        Some(written) => {
            let resolved = resolve_url(base, &written);
            let endpoint = same_origin(&resolved, base).then(|| resolved.clone());
            (
                Some(DeclaredEndpoint {
                    url: resolved,
                    templated: false,
                }),
                endpoint,
            )
        }
    };
    CardRead {
        source: source.to_string(),
        url: url.to_string(),
        response,
        inline,
        card,
        shape,
        declaration,
        endpoint,
    }
}

/// What a card read declares: the endpoint it names, then its other remotes.
pub fn card_declarations(read: &CardRead, base: &str) -> Vec<Declaration> {
    let mut out: Vec<Declaration> = read
        .declaration
        .iter()
        .map(|d| Declaration {
            not_followed: d.templated.then_some("templated-url"),
            ..Declaration::new(Kind::McpEndpoint, d.url.clone(), read.source.clone())
        })
        .collect();
    if let Some(card) = &read.card {
        out.extend(other_remotes(card, &read.source, base));
    }
    out
}

/// The card that names an audited-origin endpoint, else the strongest that
/// parses, the first of equal standing.
pub fn preferred_card<'a>(reads: &[&'a CardRead]) -> Option<&'a CardRead> {
    if let Some(named) = reads.iter().find(|r| r.endpoint.is_some()) {
        return Some(named);
    }
    let mut best: Option<&CardRead> = None;
    for read in reads.iter().filter(|r| r.card.is_some()) {
        if best.is_none_or(|b| card_standing(read.card.as_ref()) > card_standing(b.card.as_ref())) {
            best = Some(read);
        }
    }
    best
}

fn claims_json(response: &ProbeResponse) -> bool {
    let content_type = response
        .headers
        .get("content-type")
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    content_type == "application/json" || content_type.ends_with("+json")
}

/// The first card a site published that answered 200 with a JSON content
/// type and does not parse, which the card check reads as broken. An HTML
/// page answering 200 at a well-known path is a soft 404, not a card.
pub fn unparseable_published_card<'a>(reads: &[&'a CardRead]) -> Option<&'a CardRead> {
    reads
        .iter()
        .find(|r| r.shape == Some(CardShape::Unparseable) && claims_json(&r.response))
        .copied()
}

/// A card read as the document a later check scores.
pub fn retained_card(url: &str, response: &ProbeResponse) -> RetainedDocument {
    RetainedDocument {
        url: url.to_string(),
        shape: Some(card_shape(parse_json_object(response).as_ref())),
        response: response.clone(),
    }
}

fn insert_response_fields(item: &mut EvidenceItem, resp: &ProbeResponse) {
    item.insert("status".to_string(), json!(resp.status));
    if let Some(error) = &resp.error {
        item.insert("error".to_string(), json!(error));
    }
    if resp.truncated {
        item.insert("truncated".to_string(), json!(true));
    }
}

/// The evidence row of a catalog read.
pub fn document_item(
    source: &str,
    key: &str,
    resp: &ProbeResponse,
    shape: Option<&str>,
) -> EvidenceItem {
    let mut item = EvidenceItem::new();
    item.insert("source".to_string(), json!(source));
    item.insert("document".to_string(), json!(key));
    insert_response_fields(&mut item, resp);
    if let Some(shape) = shape {
        item.insert("shape".to_string(), json!(shape));
    }
    item
}

/// The evidence row of a card read.
pub fn card_item(read: &CardRead, is_record: bool, authentication: bool) -> EvidenceItem {
    let mut item = EvidenceItem::new();
    item.insert("source".to_string(), json!(read.source));
    if is_record {
        item.insert("document".to_string(), json!(SERVER_CARD_DOCUMENT));
    }
    if !read.inline {
        insert_response_fields(&mut item, &read.response);
    }
    if let Some(shape) = read.shape {
        item.insert("shape".to_string(), json!(shape.as_str()));
    }
    if let Some(declaration) = &read.declaration {
        item.insert("endpoint".to_string(), json!(declaration.url));
        if authentication {
            item.insert("authentication".to_string(), json!(true));
        }
        if declaration.templated {
            item.insert("not_followed".to_string(), json!("templated url"));
        } else if read.endpoint.is_none() {
            item.insert(
                "blocked".to_string(),
                json!("off-origin endpoint declaration"),
            );
        }
    } else if read.card.is_some() {
        item.insert("note".to_string(), json!("card present, no endpoint field"));
    }
    item
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web_audit::headers::Headers;

    const BASE: &str = "https://example.com/";

    fn response(status: u16, content_type: &str, body: &str) -> ProbeResponse {
        let mut headers = Headers::new();
        headers.append("content-type", content_type);
        ProbeResponse {
            status: Some(status),
            headers,
            body: body.to_string(),
            error: None,
            elapsed_ms: 0,
            truncated: false,
            refused: None,
        }
    }

    fn object(text: &str) -> JsonObject {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn shapes_and_standing_follow_the_site() {
        assert_eq!(card_shape(None), CardShape::Unparseable);
        assert_eq!(
            card_shape(Some(&object(r#"{"remotes":[]}"#))),
            CardShape::Sep2127
        );
        assert_eq!(
            card_shape(Some(&object(r#"{"url":"/mcp"}"#))),
            CardShape::Sep1649
        );
        assert_eq!(
            card_shape(Some(&object(r#"{"name":"x"}"#))),
            CardShape::Unrecognized
        );
        assert!(is_card_of_either_generation(Some(&object(
            r#"{"$schema":"https://x"}"#
        ))));
        assert!(!is_card_of_either_generation(Some(&object(
            r#"{"jsonrpc":"2.0","error":{}}"#
        ))));
        let mut cut = response(200, "application/json", "{}");
        cut.truncated = true;
        assert_eq!(parse_json_object(&cut), None);
        assert_eq!(
            ai_catalog_shape(Some(&object(r#"{"entries":[]}"#))),
            "ai-catalog"
        );
        assert_eq!(
            api_catalog_shape(Some(&object(r#"{"linkset":{}}"#))),
            "unrecognized"
        );
    }

    #[test]
    fn a_card_declares_its_first_streamable_remote_or_a_sep_1649_field() {
        let card = object(
            r#"{"remotes":[{"type":"sse","url":"/sse"},{"type":"streamable-http","url":"/rpc"}]}"#,
        );
        assert_eq!(card_endpoint(&card).as_deref(), Some("/rpc"));
        assert_eq!(
            card_endpoint(&object(r#"{"remotes":[{"type":"sse"}]}"#)),
            None
        );
        let card = object(r#"{"mcp_endpoint":"","transport":{"url":"/t"}}"#);
        assert_eq!(card_endpoint(&card).as_deref(), Some("/t"));
    }

    #[test]
    fn a_read_resolves_same_origin_endpoints_and_declares_the_rest() {
        let read = read_card(
            "/x",
            "https://example.com/x",
            response(
                200,
                "application/json",
                r#"{"url":"https://victim.test/mcp"}"#,
            ),
            false,
            BASE,
        );
        assert_eq!(read.endpoint, None);
        assert_eq!(
            serde_json::to_string(&card_item(&read, false, false)).unwrap(),
            r#"{"source":"/x","status":200,"shape":"sep-1649","endpoint":"https://victim.test/mcp","blocked":"off-origin endpoint declaration"}"#
        );
        let read = read_card(
            "/x",
            "https://example.com/x",
            response(
                200,
                "application/json",
                r#"{"remotes":[{"type":"streamable-http","url":"https://{tenant}.example.com/mcp"}]}"#,
            ),
            false,
            BASE,
        );
        assert_eq!(read.endpoint, None);
        assert_eq!(
            card_item(&read, false, false).get("not_followed"),
            Some(&json!("templated url"))
        );
        let read = read_card(
            "/x",
            "https://example.com/x",
            response(404, "text/html", "{}"),
            false,
            BASE,
        );
        assert_eq!((read.card.is_none(), read.shape), (true, None));
    }

    #[test]
    fn the_suffix_url_drops_trailing_slashes_query_and_fragment() {
        assert_eq!(
            card_suffix_url("https://example.com/mcp/?a=1#f", "/server-card"),
            "https://example.com/mcp/server-card"
        );
        assert_eq!(
            card_suffix_url("https://example.com/", "/server-card"),
            "https://example.com/server-card"
        );
    }

    #[test]
    fn catalog_entries_stop_at_four_and_prefer_inline_data() {
        let catalog = object(&format!(
            r#"{{"entries":[{{"type":"other","url":"/a"}},{e},{e},{{"type":"{t}","data":{{"url":"/d"}},"url":"/u"}},{{"type":"{t}"}},{e}]}}"#,
            e = format!(r#"{{"type":"{MCP_SERVER_CARD_TYPE}","url":"/c"}}"#),
            t = MCP_SERVER_CARD_TYPE
        ));
        let entries = catalog_card_entries(&catalog);
        assert_eq!(entries.len(), 4);
        assert!(matches!(
            entries[2],
            CatalogCardEntry::Data { index: 3, .. }
        ));
        assert_eq!(entries[3], CatalogCardEntry::Neither { index: 4 });
    }

    #[test]
    fn the_preferred_card_names_an_endpoint_else_stands_highest() {
        let unrecognized = read_card(
            "/a",
            "https://example.com/a",
            response(200, "application/json", r#"{"name":"x"}"#),
            false,
            BASE,
        );
        let legacy = read_card(
            "/b",
            "https://example.com/b",
            response(200, "application/json", r#"{"url":"https://other.test/"}"#),
            false,
            BASE,
        );
        assert_eq!(
            preferred_card(&[&unrecognized, &legacy]).map(|r| r.source.as_str()),
            Some("/b")
        );
        let broken = read_card(
            "/c",
            "https://example.com/c",
            response(200, "application/json; charset=utf-8", "{"),
            false,
            BASE,
        );
        assert_eq!(preferred_card(&[&broken]), None);
        assert_eq!(
            unparseable_published_card(&[&unrecognized, &broken]).map(|r| r.source.as_str()),
            Some("/c")
        );
    }
}
