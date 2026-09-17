//! Antecedent resolution, a mirror of the site's `antecedents/` modules.
//!
//! A check is scored only when its antecedent holds; otherwise it is `n_a`
//! with the `antecedent-unmet` reason. Antecedents resolve from the declared
//! site type, from MCP discovery, from the single canonical root fetch, or
//! from another check's wave-1 outcome, never from a fresh fetch.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use super::mcp_wire::advertises_resources;
use super::types::{ProbeOutcome, ProbeStatus};
use crate::web_audit::fetch::ProbeResponse;
use crate::web_audit::registry::{AntecedentToken, WebCheckSiteType};
use crate::web_audit::scorecard::{DeclaredSiteType, EvidenceItem};

/// Checks probed in wave 1 because their results feed antecedent tokens or
/// retained bodies consumed by wave-2 checks.
pub const WAVE1_CHECK_IDS: &[&str] = &[
    "robots",
    "llms-txt",
    "llms-full-txt",
    "openapi",
    "oauth-discovery",
    "mcp-initialize",
    "mcp-server-discover",
    "sitemap",
    "accept-markdown",
];

/// Whether a check id is probed in wave 1.
pub fn is_wave1(id: &str) -> bool {
    WAVE1_CHECK_IDS.contains(&id)
}

/// What the wave-1 evidence says about a token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// The check applies and is probed.
    Apply,
    /// The check does not apply; the row is `n_a`.
    NotApplicable,
    /// The antecedent cannot be resolved because the root never answered.
    Error,
}

/// The wave-1 facts every resolver reads.
pub struct AntecedentContext<'a> {
    /// The declared site type; `None` runs everything.
    pub site_type: Option<DeclaredSiteType>,
    /// The discovered MCP endpoint.
    pub mcp_endpoint: Option<&'a str>,
    /// The discovery trail.
    pub discovery_evidence: &'a [EvidenceItem],
    /// The canonical root response; `None` when it failed at the network level.
    pub root: Option<&'a ProbeResponse>,
    /// Wave-1 outcomes keyed by check id.
    pub sources: &'a HashMap<String, ProbeOutcome>,
}

/// The declared-site-type gate, applied before the antecedent token.
pub fn site_type_applies(site_types: &[WebCheckSiteType], ctx: &AntecedentContext<'_>) -> bool {
    if site_types.contains(&WebCheckSiteType::All) {
        return true;
    }
    if site_types.contains(&WebCheckSiteType::Mcp) && ctx.mcp_endpoint.is_some() {
        return true;
    }
    match ctx.site_type {
        None => true,
        Some(DeclaredSiteType::Content) => site_types.contains(&WebCheckSiteType::Content),
        Some(DeclaredSiteType::Api) => site_types.contains(&WebCheckSiteType::Api),
    }
}

/// Resolve one token against the wave-1 context.
pub fn resolve(token: AntecedentToken, ctx: &AntecedentContext<'_>) -> Resolution {
    use AntecedentToken as T;
    match token {
        T::None => Resolution::Apply,
        T::HttpRoot => match ctx.root {
            Some(root) if root.status.is_some() => Resolution::Apply,
            _ => Resolution::Error,
        },
        T::HtmlRoot => html_root_gate(ctx).unwrap_or(Resolution::Apply),
        T::McpPresent => apply_if(ctx.mcp_endpoint.is_some()),
        T::McpAuth => {
            if ctx.mcp_endpoint.is_none() {
                return Resolution::NotApplicable;
            }
            apply_if(
                evidence_shows_auth_challenge(source_evidence(ctx, "mcp-initialize"))
                    || card_declares_auth(ctx),
            )
        }
        T::McpResources => {
            if ctx.mcp_endpoint.is_none() {
                return Resolution::NotApplicable;
            }
            apply_if(
                advertises_resources(source_evidence(ctx, "mcp-initialize"))
                    || advertises_resources(source_evidence(ctx, "mcp-server-discover")),
            )
        }
        T::ApiSurface => apply_if(api_surface_holds(ctx)),
        T::SchemasRef => apply_if(
            source_passed(ctx, "openapi")
                || ctx.root.is_some_and(|root| SCHEMAS_RE.is_match(&root.body)),
        ),
        T::DocsSite => apply_if(
            ctx.site_type == Some(DeclaredSiteType::Content) || source_passed(ctx, "llms-txt"),
        ),
        T::RootLlmsTxt => apply_if(source_passed(ctx, "llms-txt")),
        T::RootLlmsFullTxt => apply_if(source_passed(ctx, "llms-full-txt")),
        T::MarkdownTwin => {
            if let Some(gate) = html_root_gate(ctx) {
                return gate;
            }
            let link = ctx
                .root
                .and_then(|root| root.headers.get("link"))
                .unwrap_or("");
            let advertises_md_alternate =
                REL_ALTERNATE_RE.is_match(link) && TEXT_MARKDOWN_RE.is_match(link);
            apply_if(
                source_passed(ctx, "accept-markdown")
                    || source_passed(ctx, "llms-txt")
                    || advertises_md_alternate,
            )
        }
        T::RobotsPresent => apply_if(source_passed(ctx, "robots")),
        T::AuthPresent => {
            apply_if(source_passed(ctx, "oauth-discovery") || auth_signal_observed(ctx))
        }
    }
}

/// The evidence line for a row gated to `n_a` by its token.
pub fn unmet_evidence(token: AntecedentToken) -> &'static str {
    use AntecedentToken as T;
    match token {
        T::None => "not applicable",
        T::HttpRoot => "root did not answer",
        T::HtmlRoot => "root is not an HTML document",
        T::McpPresent => "no MCP endpoint discovered",
        T::McpAuth => "MCP endpoint does not challenge for auth",
        T::McpResources => {
            "neither initialize nor server/discover advertises capabilities.resources"
        }
        T::ApiSurface => "no API surface detected",
        T::SchemasRef => "no JSON Schema references detected",
        T::DocsSite => "not a docs/content site",
        T::RootLlmsTxt => "root llms.txt not present",
        T::RootLlmsFullTxt => "root llms-full.txt not present",
        T::MarkdownTwin => {
            "site exposes no markdown twin (no text/markdown negotiation, no markdown alternate link, no llms.txt)"
        }
        T::RobotsPresent => "robots.txt not present",
        T::AuthPresent => "no auth surface detected",
    }
}

fn apply_if(holds: bool) -> Resolution {
    if holds {
        Resolution::Apply
    } else {
        Resolution::NotApplicable
    }
}

/// `Error` when the root never answered, `NotApplicable` when it answered as
/// non-HTML, `None` when it is HTML and the caller keeps resolving.
fn html_root_gate(ctx: &AntecedentContext<'_>) -> Option<Resolution> {
    let Some(root) = ctx.root else {
        return Some(Resolution::Error);
    };
    if root.status.is_none() {
        return Some(Resolution::Error);
    }
    let content_type = root.headers.get("content-type").unwrap_or("");
    if !content_type.contains("text/html") {
        return Some(Resolution::NotApplicable);
    }
    None
}

fn source_passed(ctx: &AntecedentContext<'_>, id: &str) -> bool {
    ctx.sources
        .get(id)
        .is_some_and(|o| o.status == ProbeStatus::Pass)
}

fn source_evidence<'a>(ctx: &AntecedentContext<'a>, id: &str) -> &'a [EvidenceItem] {
    ctx.sources.get(id).map_or(&[], |o| o.evidence.as_slice())
}

/// The retained body of a wave-1 check, or empty.
pub fn retained_body<'a>(ctx: &AntecedentContext<'a>, id: &str) -> &'a str {
    source_evidence(ctx, id)
        .iter()
        .find_map(|item| item.get("body").and_then(|b| b.as_str()))
        .unwrap_or("")
}

fn any_evidence_status(items: &[EvidenceItem], status: u64) -> bool {
    items
        .iter()
        .any(|item| item.get("status").and_then(|s| s.as_u64()) == Some(status))
}

fn evidence_shows_auth_challenge(items: &[EvidenceItem]) -> bool {
    any_evidence_status(items, 401)
        || items
            .iter()
            .any(|item| item.get("www_authenticate").is_some_and(|v| v.is_string()))
}

fn card_declares_auth(ctx: &AntecedentContext<'_>) -> bool {
    ctx.discovery_evidence
        .iter()
        .any(|item| item.get("authentication") == Some(&serde_json::Value::Bool(true)))
}

fn auth_signal_observed(ctx: &AntecedentContext<'_>) -> bool {
    if ctx.root.is_some_and(|root| root.status == Some(401)) {
        return true;
    }
    evidence_shows_auth_challenge(source_evidence(ctx, "openapi"))
        || evidence_shows_auth_challenge(source_evidence(ctx, "mcp-initialize"))
        || card_declares_auth(ctx)
}

static SCHEMAS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)application/schema\+json|json-?schema|/schema\.json").unwrap()
});
static API_DOC_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?-u:\b)(?:openapi|swagger)[A-Za-z0-9_./-]*\.(?:json|ya?ml)(?-u:\b)").unwrap()
});
static API_PATH_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)/api/").unwrap());
static SERVICE_DESC_REL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)rel\s*=\s*["']?(?:service-desc|service-doc)(?-u:\b)"#).unwrap()
});
static MCP_TARGET_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\.well-known/mcp|server-card|mcp-skill|/mcp(?-u:\b)").unwrap()
});
static LINK_TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<link(?-u:\b)[^>]*>").unwrap());
static REL_ALTERNATE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)rel=["']?alternate["']?"#).unwrap());
static TEXT_MARKDOWN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)text/markdown").unwrap());

/// A service-desc or service-doc link (Link header or `<link>` tag) to a non-MCP target.
fn rest_service_desc_link(ctx: &AntecedentContext<'_>) -> bool {
    let Some(root) = ctx.root else {
        return false;
    };
    let header_entries = root
        .headers
        .get("link")
        .unwrap_or("")
        .split(',')
        .map(str::to_string);
    let tag_entries = LINK_TAG_RE
        .find_iter(&root.body)
        .map(|m| m.as_str().to_string());
    header_entries
        .chain(tag_entries)
        .any(|entry| SERVICE_DESC_REL_RE.is_match(&entry) && !MCP_TARGET_RE.is_match(&entry))
}

/// Any one signal makes the api-surface antecedent hold.
fn api_surface_holds(ctx: &AntecedentContext<'_>) -> bool {
    if ctx.site_type == Some(DeclaredSiteType::Api) {
        return true;
    }
    if any_evidence_status(source_evidence(ctx, "openapi"), 200) {
        return true;
    }
    if rest_service_desc_link(ctx) {
        return true;
    }
    let llms = retained_body(ctx, "llms-txt");
    if API_DOC_URL_RE.is_match(llms) || API_PATH_RE.is_match(llms) {
        return true;
    }
    API_DOC_URL_RE.is_match(retained_body(ctx, "sitemap"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web_audit::headers::Headers;
    use serde_json::json;

    fn root(
        status: Option<u16>,
        content_type: &str,
        body: &str,
        link: Option<&str>,
    ) -> ProbeResponse {
        let mut pairs = vec![("content-type", content_type)];
        if let Some(link) = link {
            pairs.push(("link", link));
        }
        ProbeResponse {
            status,
            headers: Headers::from_pairs(pairs),
            body: body.to_string(),
            error: None,
            elapsed_ms: 0,
            truncated: false,
        }
    }

    fn outcome(status: ProbeStatus, evidence: serde_json::Value) -> ProbeOutcome {
        let item = evidence.as_object().cloned().unwrap_or_default();
        ProbeOutcome::new(status, vec![item])
    }

    fn ctx<'a>(
        site_type: Option<DeclaredSiteType>,
        endpoint: Option<&'a str>,
        root: Option<&'a ProbeResponse>,
        sources: &'a HashMap<String, ProbeOutcome>,
        discovery: &'a [EvidenceItem],
    ) -> AntecedentContext<'a> {
        AntecedentContext {
            site_type,
            mcp_endpoint: endpoint,
            discovery_evidence: discovery,
            root,
            sources,
        }
    }

    #[test]
    fn root_gates_distinguish_missing_non_html_and_html_roots() {
        let sources = HashMap::new();
        let dead = root(None, "", "", None);
        for dead_root in [Some(&dead), None] {
            let c = ctx(None, None, dead_root, &sources, &[]);
            assert_eq!(resolve(AntecedentToken::HttpRoot, &c), Resolution::Error);
            assert_eq!(resolve(AntecedentToken::HtmlRoot, &c), Resolution::Error);
            assert_eq!(
                resolve(AntecedentToken::MarkdownTwin, &c),
                Resolution::Error
            );
        }
        let json_root = root(Some(200), "application/json", "{}", None);
        let c = ctx(None, None, Some(&json_root), &sources, &[]);
        assert_eq!(resolve(AntecedentToken::HttpRoot, &c), Resolution::Apply);
        assert_eq!(
            resolve(AntecedentToken::HtmlRoot, &c),
            Resolution::NotApplicable
        );
        let html_root = root(Some(200), "text/html; charset=utf-8", "<html></html>", None);
        let c = ctx(None, None, Some(&html_root), &sources, &[]);
        assert_eq!(resolve(AntecedentToken::HtmlRoot, &c), Resolution::Apply);
        assert_eq!(resolve(AntecedentToken::None, &c), Resolution::Apply);
    }

    #[test]
    fn site_type_filter_matches_the_site_rules() {
        let sources = HashMap::new();
        let none = ctx(None, None, None, &sources, &[]);
        let api = ctx(Some(DeclaredSiteType::Api), None, None, &sources, &[]);
        let content_with_mcp = ctx(
            Some(DeclaredSiteType::Content),
            Some("https://x/mcp"),
            None,
            &sources,
            &[],
        );
        assert!(site_type_applies(&[WebCheckSiteType::Content], &none));
        assert!(!site_type_applies(&[WebCheckSiteType::Content], &api));
        assert!(site_type_applies(&[WebCheckSiteType::All], &api));
        assert!(site_type_applies(
            &[WebCheckSiteType::Mcp],
            &content_with_mcp
        ));
        assert!(!site_type_applies(&[WebCheckSiteType::Mcp], &api));
        assert!(site_type_applies(
            &[WebCheckSiteType::Api, WebCheckSiteType::Mcp],
            &api
        ));
    }

    #[test]
    fn api_surface_holds_on_any_one_signal() {
        let mut sources = HashMap::new();
        let html = root(Some(200), "text/html", "<html><head></head></html>", None);
        assert_eq!(
            resolve(
                AntecedentToken::ApiSurface,
                &ctx(None, None, Some(&html), &sources, &[])
            ),
            Resolution::NotApplicable
        );
        assert_eq!(
            resolve(
                AntecedentToken::ApiSurface,
                &ctx(
                    Some(DeclaredSiteType::Api),
                    None,
                    Some(&html),
                    &sources,
                    &[]
                )
            ),
            Resolution::Apply
        );
        sources.insert(
            "openapi".into(),
            outcome(ProbeStatus::Absent, json!({"url": "x", "status": 200})),
        );
        assert_eq!(
            resolve(
                AntecedentToken::ApiSurface,
                &ctx(None, None, Some(&html), &sources, &[])
            ),
            Resolution::Apply
        );
        sources.clear();
        let linked = root(
            Some(200),
            "text/html",
            r#"<link rel="service-desc" href="/openapi.json">"#,
            None,
        );
        assert_eq!(
            resolve(
                AntecedentToken::ApiSurface,
                &ctx(None, None, Some(&linked), &sources, &[])
            ),
            Resolution::Apply
        );
        let mcp_linked = root(
            Some(200),
            "text/html",
            r#"<link rel="service-desc" href="/.well-known/mcp.json">"#,
            None,
        );
        assert_eq!(
            resolve(
                AntecedentToken::ApiSurface,
                &ctx(None, None, Some(&mcp_linked), &sources, &[])
            ),
            Resolution::NotApplicable
        );
        sources.insert(
            "llms-txt".into(),
            outcome(
                ProbeStatus::Pass,
                json!({"body": "# x\n- [API](/api/things)\n"}),
            ),
        );
        assert_eq!(
            resolve(
                AntecedentToken::ApiSurface,
                &ctx(None, None, Some(&html), &sources, &[])
            ),
            Resolution::Apply
        );
        sources.insert(
            "llms-txt".into(),
            outcome(
                ProbeStatus::Pass,
                json!({"body": "# x\n- [docs](/web-audit/skill/openapi)\n"}),
            ),
        );
        assert_eq!(
            resolve(
                AntecedentToken::ApiSurface,
                &ctx(None, None, Some(&html), &sources, &[])
            ),
            Resolution::NotApplicable
        );
        sources.insert(
            "sitemap".into(),
            outcome(
                ProbeStatus::Pass,
                json!({"body": "<loc>https://x/openapi.yaml</loc>"}),
            ),
        );
        assert_eq!(
            resolve(
                AntecedentToken::ApiSurface,
                &ctx(None, None, Some(&html), &sources, &[])
            ),
            Resolution::Apply
        );
    }

    #[test]
    fn mcp_and_auth_tokens_read_wave_one_evidence() {
        let mut sources = HashMap::new();
        let html = root(Some(200), "text/html", "", None);
        let no_endpoint = ctx(None, None, Some(&html), &sources, &[]);
        assert_eq!(
            resolve(AntecedentToken::McpPresent, &no_endpoint),
            Resolution::NotApplicable
        );
        assert_eq!(
            resolve(AntecedentToken::McpAuth, &no_endpoint),
            Resolution::NotApplicable
        );
        assert_eq!(
            resolve(AntecedentToken::McpResources, &no_endpoint),
            Resolution::NotApplicable
        );
        let with = ctx(None, Some("https://x/mcp"), Some(&html), &sources, &[]);
        assert_eq!(
            resolve(AntecedentToken::McpPresent, &with),
            Resolution::Apply
        );
        assert_eq!(
            resolve(AntecedentToken::McpAuth, &with),
            Resolution::NotApplicable
        );
        sources.insert("mcp-initialize".into(), outcome(ProbeStatus::Pass, json!({"status": 200, "capabilities": ["tools", "resources"], "www_authenticate": "Bearer"})));
        let with = ctx(None, Some("https://x/mcp"), Some(&html), &sources, &[]);
        assert_eq!(resolve(AntecedentToken::McpAuth, &with), Resolution::Apply);
        assert_eq!(
            resolve(AntecedentToken::McpResources, &with),
            Resolution::Apply
        );
        assert_eq!(
            resolve(AntecedentToken::AuthPresent, &with),
            Resolution::Apply
        );
        sources.clear();
        let mut card = EvidenceItem::new();
        card.insert("authentication".into(), json!(true));
        let discovery = vec![card];
        let carded = ctx(
            None,
            Some("https://x/mcp"),
            Some(&html),
            &sources,
            &discovery,
        );
        assert_eq!(
            resolve(AntecedentToken::McpAuth, &carded),
            Resolution::Apply
        );
        let challenged = root(Some(401), "text/html", "", None);
        assert_eq!(
            resolve(
                AntecedentToken::AuthPresent,
                &ctx(None, None, Some(&challenged), &sources, &[])
            ),
            Resolution::Apply
        );
        assert_eq!(
            resolve(
                AntecedentToken::AuthPresent,
                &ctx(None, None, Some(&html), &sources, &[])
            ),
            Resolution::NotApplicable
        );
    }

    #[test]
    fn content_tokens_follow_llms_and_negotiation_signals() {
        let mut sources = HashMap::new();
        let html = root(Some(200), "text/html", "", None);
        let plain = ctx(None, None, Some(&html), &sources, &[]);
        for token in [
            AntecedentToken::DocsSite,
            AntecedentToken::RootLlmsTxt,
            AntecedentToken::RootLlmsFullTxt,
            AntecedentToken::MarkdownTwin,
            AntecedentToken::RobotsPresent,
            AntecedentToken::SchemasRef,
        ] {
            assert_eq!(
                resolve(token, &plain),
                Resolution::NotApplicable,
                "{token:?}"
            );
        }
        assert_eq!(
            resolve(
                AntecedentToken::DocsSite,
                &ctx(
                    Some(DeclaredSiteType::Content),
                    None,
                    Some(&html),
                    &sources,
                    &[]
                )
            ),
            Resolution::Apply
        );
        let advertised = root(
            Some(200),
            "text/html",
            "",
            Some(r#"</index.md>; rel="alternate"; type="text/markdown""#),
        );
        assert_eq!(
            resolve(
                AntecedentToken::MarkdownTwin,
                &ctx(None, None, Some(&advertised), &sources, &[])
            ),
            Resolution::Apply
        );
        sources.insert(
            "llms-txt".into(),
            outcome(ProbeStatus::Pass, json!({"body": "# x"})),
        );
        sources.insert("robots".into(), outcome(ProbeStatus::Pass, json!({})));
        let with = ctx(None, None, Some(&html), &sources, &[]);
        for token in [
            AntecedentToken::DocsSite,
            AntecedentToken::RootLlmsTxt,
            AntecedentToken::MarkdownTwin,
            AntecedentToken::RobotsPresent,
        ] {
            assert_eq!(resolve(token, &with), Resolution::Apply, "{token:?}");
        }
        let schema_root = root(
            Some(200),
            "text/html",
            r#"<link rel="describedby" href="/schema.json">"#,
            None,
        );
        assert_eq!(
            resolve(
                AntecedentToken::SchemasRef,
                &ctx(None, None, Some(&schema_root), &sources, &[])
            ),
            Resolution::Apply
        );
        assert_eq!(retained_body(&with, "llms-txt"), "# x");
        assert_eq!(retained_body(&with, "sitemap"), "");
    }

    #[test]
    fn wave_one_membership_and_unmet_lines_match_the_site() {
        assert_eq!(WAVE1_CHECK_IDS.len(), 9);
        assert!(is_wave1("mcp-server-discover"));
        assert!(!is_wave1("llms-txt-format"));
        assert_eq!(
            unmet_evidence(AntecedentToken::McpPresent),
            "no MCP endpoint discovered"
        );
        assert_eq!(unmet_evidence(AntecedentToken::None), "not applicable");
    }
}
