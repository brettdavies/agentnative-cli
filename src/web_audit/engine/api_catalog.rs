//! The RFC 9727 API catalog as the API category reads it, a mirror of the
//! site's `api-catalog.ts`: which linkset anchors are API hosts, which
//! description each declares, and what the catalog names for the API
//! follow to settle.
//!
//! An anchor is an API host when one of its `service-desc` targets is a
//! description other than an MCP surface: an MCP-first site points
//! service-desc at its server card, which describes no REST API. Every
//! other anchor is still declared, and recorded not followed, so the trail
//! shows what the catalog named and why the audit left it alone.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::discovery::{API_CATALOG_PATH, resolve_url};
use super::discovery_documents::{RetainedDocument, is_templated_url, parse_json_object};
use super::follow_trail::{Declaration, Kind};
use crate::web_audit::pattern::fixed;

/// A link target that is an MCP surface (a card, the endpoint, its usage
/// doc) rather than an API description.
pub static MCP_TARGET_RE: LazyLock<Regex> =
    LazyLock::new(|| fixed(r"(?i)\.well-known/mcp|server-card|mcp-skill|/mcp(?-u:\b)"));

// Bounds the declarations one catalog can add to the trail, whatever it lists.
const MAX_CATALOG_ANCHORS: usize = 8;

/// A URL the catalog declares, with the JSON Pointer into the catalog where it does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogLink {
    /// The URL, resolved against the catalog; a templated URL as written.
    pub url: String,
    /// `/.well-known/api-catalog#<pointer>`.
    pub source: String,
}

/// One linkset context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogAnchor {
    /// The anchor itself.
    pub link: CatalogLink,
    /// The anchor's first `service-desc` target that is not an MCP surface.
    pub description: Option<CatalogLink>,
}

impl CatalogAnchor {
    /// The API category evaluates this anchor.
    pub fn is_api(&self) -> bool {
        self.description.is_some()
    }
}

fn catalog_url(raw: Option<&Value>, base: &str) -> String {
    match raw.and_then(Value::as_str) {
        Some(raw) if !raw.is_empty() && is_templated_url(raw) => raw.to_string(),
        Some(raw) if !raw.is_empty() => resolve_url(base, raw),
        _ => String::new(),
    }
}

fn api_description(links: Option<&Value>, base: &str, pointer: &str) -> Option<CatalogLink> {
    for (index, link) in links?.as_array()?.iter().enumerate() {
        let Some(href) = link.get("href").and_then(Value::as_str) else {
            continue;
        };
        if MCP_TARGET_RE.is_match(href) {
            continue;
        }
        let url = catalog_url(link.get("href"), base);
        if !url.is_empty() {
            return Some(CatalogLink {
                url,
                source: format!("{pointer}/service-desc/{index}"),
            });
        }
    }
    None
}

/// The anchors a retained API catalog lists, in linkset order; none unless
/// it answered 200 with a linkset.
pub fn catalog_anchors(catalog: Option<&RetainedDocument>) -> Vec<CatalogAnchor> {
    let Some(catalog) = catalog.filter(|c| c.response.status == Some(200)) else {
        return Vec::new();
    };
    let Some(parsed) = parse_json_object(&catalog.response) else {
        return Vec::new();
    };
    let Some(linkset) = parsed.get("linkset").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut anchors = Vec::new();
    for (index, context) in linkset.iter().take(MAX_CATALOG_ANCHORS).enumerate() {
        if !context.is_object() {
            continue;
        }
        let url = catalog_url(context.get("anchor"), &catalog.url);
        if url.is_empty() {
            continue;
        }
        let pointer = format!("{API_CATALOG_PATH}#/linkset/{index}");
        let description = api_description(context.get("service-desc"), &catalog.url, &pointer);
        anchors.push(CatalogAnchor {
            link: CatalogLink {
                url,
                source: pointer,
            },
            description,
        });
    }
    anchors
}

/// What the catalog declares, in the order the follow admits it: every
/// anchor, then each API description.
pub fn api_declarations(anchors: &[CatalogAnchor]) -> Vec<Declaration> {
    let declared = |kind, link: &CatalogLink, no_service_desc: bool| Declaration {
        not_followed: no_service_desc.then_some("no-service-desc"),
        ..Declaration::new(kind, link.url.clone(), link.source.clone())
    };
    let mut out: Vec<Declaration> = anchors
        .iter()
        .map(|a| declared(Kind::ApiAnchor, &a.link, !a.is_api()))
        .collect();
    out.extend(anchors.iter().filter_map(|a| {
        a.description
            .as_ref()
            .map(|d| declared(Kind::ApiDescription, d, false))
    }));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web_audit::fetch::ProbeResponse;
    use crate::web_audit::headers::Headers;

    fn catalog(status: u16, body: &str) -> RetainedDocument {
        RetainedDocument {
            url: "https://example.com/.well-known/api-catalog".to_string(),
            response: ProbeResponse {
                status: Some(status),
                headers: Headers::new(),
                body: body.to_string(),
                error: None,
                elapsed_ms: 0,
                truncated: false,
                refused: None,
            },
            shape: None,
        }
    }

    #[test]
    fn anchors_keep_linkset_order_and_skip_mcp_service_descs() {
        let body = r#"{"linkset":[
            {"anchor":"https://api.example.net/","service-desc":[{"href":"/.well-known/mcp/server-card.json"},{"href":"https://api.example.net/openapi.json"}]},
            {"anchor":"/mcp","service-desc":[{"href":"https://example.com/mcp"}]},
            "not an object",
            {"service-desc":[{"href":"/openapi.json"}]},
            {"anchor":"https://{tenant}.example.org/","service-desc":[{"href":"/spec.yaml"}]}
        ]}"#;
        let anchors = catalog_anchors(Some(&catalog(200, body)));
        type Seen<'a> = (&'a str, &'a str, Option<(&'a str, &'a str)>);
        let seen: Vec<Seen<'_>> = anchors
            .iter()
            .map(|a| {
                (
                    a.link.url.as_str(),
                    a.link.source.as_str(),
                    a.description
                        .as_ref()
                        .map(|d| (d.url.as_str(), d.source.as_str())),
                )
            })
            .collect();
        assert_eq!(
            seen,
            [
                (
                    "https://api.example.net/",
                    "/.well-known/api-catalog#/linkset/0",
                    Some((
                        "https://api.example.net/openapi.json",
                        "/.well-known/api-catalog#/linkset/0/service-desc/1"
                    ))
                ),
                (
                    "https://example.com/mcp",
                    "/.well-known/api-catalog#/linkset/1",
                    None
                ),
                (
                    "https://{tenant}.example.org/",
                    "/.well-known/api-catalog#/linkset/4",
                    Some((
                        "https://example.com/spec.yaml",
                        "/.well-known/api-catalog#/linkset/4/service-desc/0"
                    ))
                ),
            ]
        );
        let kinds: Vec<(&str, bool)> = api_declarations(&anchors)
            .iter()
            .map(|d| (d.kind.as_str(), d.not_followed.is_some()))
            .collect();
        assert_eq!(
            kinds,
            [
                ("api-anchor", false),
                ("api-anchor", true),
                ("api-anchor", false),
                ("api-description", false),
                ("api-description", false),
            ]
        );
    }

    #[test]
    fn only_a_200_linkset_lists_anchors_and_only_the_first_eight_contexts_count() {
        let one = r#"{"anchor":"https://a.example.net/","service-desc":[{"href":"/o.json"}]}"#;
        let many = format!("{{\"linkset\":[{}]}}", [one; 10].join(","));
        assert_eq!(catalog_anchors(Some(&catalog(200, &many))).len(), 8);
        assert!(catalog_anchors(Some(&catalog(404, &many))).is_empty());
        assert!(catalog_anchors(Some(&catalog(200, r#"{"linkset":{}}"#))).is_empty());
        assert!(catalog_anchors(None).is_empty());
    }
}
