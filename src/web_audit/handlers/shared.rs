//! Helpers every stateless handler shares: token substitution, evidence
//! rows, the statuses that ask for a retry, a URL that answered only with
//! a redirect to http, the miss ranking across candidates, and the
//! same-origin recovery-link scan. A mirror of the site's
//! `handlers/shared.ts`.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Value, json};
use url::Url;

use crate::web_audit::engine::ProbeStatus;
use crate::web_audit::fetch::{
    CrossOriginRedirects, FetchOptions, NO_PLAINTEXT_REQUEST, ProbeResponse, Refusal,
};
use crate::web_audit::pattern::fixed;
use crate::web_audit::scorecard::EvidenceItem;

pub use crate::web_audit::engine::discovery::resolve_url;

/// JavaScript `\s` (WhiteSpace and LineTerminator) as a Rust class body,
/// for the handler patterns the site writes with `\s` and `\S`.
pub const JS_SPACE: &str = r"\t\n\x0B\x0C\r \u{A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}";

/// Replace the `{mcp_endpoint}` token and the `{mcp_origin}` token (the
/// endpoint's scheme, host, and port); each yields `""` when the endpoint
/// is unknown.
pub fn substitute_endpoint(value: &str, mcp_endpoint: Option<&str>) -> String {
    if !value.contains("{mcp_endpoint}") && !value.contains("{mcp_origin}") {
        return value.to_string();
    }
    let origin = mcp_endpoint
        .and_then(|e| Url::parse(e).ok())
        .map(|u| u.origin().ascii_serialization())
        .unwrap_or_default();
    value
        .replace("{mcp_endpoint}", mcp_endpoint.unwrap_or(""))
        .replace("{mcp_origin}", &origin)
}

/// Redirect handling for a request to the MCP endpoint. No probe of it
/// reaches a host nothing confirmed: on a declared host no redirect is
/// taken, and on the audited origin only hops that stay on it are. There a
/// GET gets the hop it may not take back as its answer, since a redirect is
/// a valid answer to a GET; any other method fails on it.
pub fn mcp_endpoint_redirects(opts: FetchOptions, followed: bool, method: &str) -> FetchOptions {
    if followed {
        return FetchOptions {
            refuse_redirects: true,
            ..opts
        };
    }
    let cross_origin = if matches!(method, "GET" | "HEAD") {
        CrossOriginRedirects::Return
    } else {
        CrossOriginRedirects::Refuse
    };
    FetchOptions {
        cross_origin_redirects: Some(cross_origin),
        ..opts
    }
}

/// Redirect handling for a probe of `raw_path`: the MCP endpoint's when the
/// path targets it. A document on a declared endpoint's host takes no
/// redirect either; on the audited origin it keeps the default.
pub fn endpoint_redirects(
    opts: FetchOptions,
    raw_path: &str,
    followed: bool,
    method: &str,
) -> FetchOptions {
    if raw_path.contains("{mcp_endpoint}") {
        return mcp_endpoint_redirects(opts, followed, method);
    }
    FetchOptions {
        refuse_redirects: raw_path.contains("{mcp_origin}") && followed,
        ..opts
    }
}

/// Replace the `{host}` token used by DoH record names.
pub fn substitute_host(value: &str, host: &str) -> String {
    value.replace("{host}", host)
}

/// An evidence row from ordered key-value pairs.
pub fn item<const N: usize>(pairs: [(&str, Value); N]) -> EvidenceItem {
    let mut map = EvidenceItem::new();
    for (key, value) in pairs {
        map.insert(key.to_string(), value);
    }
    map
}

/// A `why` value from one or more lines.
pub fn why(lines: &[&str]) -> Value {
    Value::Array(
        lines
            .iter()
            .map(|l| Value::String((*l).to_string()))
            .collect(),
    )
}

/// A missing surface: 404 or 410.
pub fn is_missing_status(status: Option<u16>) -> bool {
    matches!(status, Some(404) | Some(410))
}

/// Statuses whose shape is "not now" rather than "not here": a target
/// asking to be retried is reporting its own load, not answering what the
/// request asked. Each carries its RFC 9110 reason phrase, which a row's
/// evidence names.
pub const RETRY_SHAPED_STATUSES: [(u16, &str); 2] =
    [(408, "Request Timeout"), (429, "Too Many Requests")];

/// `HTTP <status> <reason>` for a retry-shaped status.
pub fn retry_shaped_answer(status: Option<u16>) -> Option<String> {
    let status = status?;
    RETRY_SHAPED_STATUSES
        .iter()
        .find(|(s, _)| *s == status)
        .map(|(s, reason)| format!("HTTP {s} {reason}"))
}

/// The `why` of a row its own retry-shaped answer settles as `error`.
/// Such an answer reports the target's load at that moment, whichever
/// layer sent it, so neither its body nor its headers describe the surface
/// the row asks about.
pub fn retry_shaped_why(status: Option<u16>) -> Option<String> {
    retry_shaped_answer(status).map(|answer| format!("the target answered {answer}; not scored"))
}

/// The URL answered, but only with a redirect to http, whose hop the guard
/// refused: nothing it serves is readable over https.
pub fn redirects_to_http(resp: &ProbeResponse) -> bool {
    resp.refused == Some(Refusal::InsecureScheme) && resp.status.is_some()
}

/// Why a declared URL was not requested over plaintext.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaintextReason {
    /// The URL as written is not https.
    NotHttps,
    /// The URL answered with a redirect to http.
    RedirectsToHttp,
}

impl PlaintextReason {
    /// The site's wording.
    pub fn as_str(self) -> &'static str {
        match self {
            PlaintextReason::NotHttps => "not https",
            PlaintextReason::RedirectsToHttp => "redirects to http",
        }
    }

    fn why(self) -> String {
        format!("{}; {NO_PLAINTEXT_REQUEST}", self.as_str())
    }
}

/// The evidence fields for a declared URL anc did not request over
/// plaintext, which the caller adds after its own fields.
pub fn plaintext_item(reason: PlaintextReason) -> EvidenceItem {
    item([
        ("blocked", json!(reason.as_str())),
        ("ok", json!(false)),
        ("why", json!([reason.why()])),
    ])
}

/// The evidence item for a URL that answered only with a redirect to http.
pub fn redirects_to_http_item(url: &str, status: Option<u16>) -> EvidenceItem {
    item([
        ("url", json!(url)),
        ("status", json!(status)),
        ("ok", json!(false)),
        ("why", json!([PlaintextReason::RedirectsToHttp.why()])),
    ])
}

/// How a non-passing candidate missed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Miss {
    /// The surface is there and wrong.
    Broken,
    /// The surface is not there.
    Absent,
    /// The surface is listed but not usable over https.
    Noncompliant,
    /// The probe did not complete.
    Error,
}

impl Miss {
    /// The verdict this miss gives a row it decides.
    pub fn status(self) -> ProbeStatus {
        match self {
            Miss::Broken => ProbeStatus::Broken,
            Miss::Absent => ProbeStatus::Absent,
            Miss::Noncompliant => ProbeStatus::Noncompliant,
            Miss::Error => ProbeStatus::Error,
        }
    }
}

/// Across candidates, any broken outranks absent (something is there and
/// wrong) and a definitive absence outranks an operational error. A
/// noncompliant miss earns credit and an absent one does not, so it ranks
/// below absent: a link listed only over http never lifts a row a dead link
/// decides.
pub fn worst(misses: &[Miss]) -> ProbeStatus {
    [Miss::Broken, Miss::Absent, Miss::Noncompliant]
        .into_iter()
        .find(|miss| misses.contains(miss))
        .map_or(ProbeStatus::Error, Miss::status)
}

/// The number of UTF-16 code units, which is what a JavaScript `length`
/// counts.
pub fn js_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// JavaScript's `String.prototype.trim`, which also strips U+FEFF.
pub fn js_trim(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || c == '\u{FEFF}')
}

// CommonMark lets a destination sit in angle brackets (`[x](<a b.md>)`) so it
// can hold spaces or parentheses; the brackets are syntax, not part of the URL.
static MARKDOWN_LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    fixed(&format!(
        r#"\]\((?:<([^<>\n]+)>|([^){JS_SPACE}]+))(?:[{JS_SPACE}]+"[^"]*")?\)"#
    ))
});

/// Every inline-link destination in a markdown body, in document order, as
/// written: an angle-bracketed destination without its brackets, and an
/// optional `"title"` dropped.
pub fn markdown_hrefs(body: &str) -> impl Iterator<Item = &str> {
    MARKDOWN_LINK_RE
        .captures_iter(body)
        .filter_map(|c| c.get(1).or_else(|| c.get(2)))
        .map(|m| m.as_str())
}

fn is_recovery_path(pathname: &str) -> bool {
    pathname.ends_with("/sitemap.xml")
        || pathname.ends_with("/llms.txt")
        || pathname == "/docs"
        || pathname.starts_with("/docs/")
}

/// Whether markdown contains at least one href that resolves to a
/// same-origin sitemap.xml, llms.txt, or /docs recovery surface.
pub fn same_origin_recovery_link(body: &str, base: &str) -> (bool, String) {
    let Ok(base_url) = Url::parse(base) else {
        return (false, "unparseable audit origin".to_string());
    };
    for href in markdown_hrefs(body) {
        let Ok(url) = base_url.join(href) else {
            continue;
        };
        if url.origin() != base_url.origin() {
            continue;
        }
        if is_recovery_path(url.path()) {
            return (true, format!("same-origin recovery {}", url.path()));
        }
    }
    (
        false,
        "no same-origin sitemap.xml, llms.txt, or /docs link".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_substitute_and_an_unknown_endpoint_yields_empty() {
        assert_eq!(
            substitute_endpoint("{mcp_endpoint}", Some("https://x/mcp")),
            "https://x/mcp"
        );
        assert_eq!(substitute_endpoint("{mcp_endpoint}", None), "");
        assert_eq!(substitute_endpoint("/plain", None), "/plain");
        assert_eq!(
            substitute_host("_agents.{host}", "example.com"),
            "_agents.example.com"
        );
    }

    fn hrefs(body: &str) -> Vec<&str> {
        markdown_hrefs(body).collect()
    }

    #[test]
    fn angle_bracket_destinations_are_read_without_their_brackets() {
        assert_eq!(
            hrefs("- [Guide](<https://example.com/docs/getting started (v2).md>): how\n"),
            ["https://example.com/docs/getting started (v2).md"]
        );
        assert_eq!(hrefs("[x](<a(b)c>)"), ["a(b)c"]);
        assert!(hrefs("[x](<a\nb>)").is_empty());
        assert_eq!(hrefs("[x](<a>b)"), ["<a>b"]);
    }

    #[test]
    fn a_title_after_either_destination_form_is_dropped() {
        assert_eq!(
            hrefs(r#"[a](/a.md "A title") [b](</b c.md> "B")"#),
            ["/a.md", "/b c.md"]
        );
    }

    #[test]
    fn an_unbracketed_destination_ends_at_its_first_close_paren() {
        assert_eq!(
            hrefs("[w](https://en.wikipedia.org/wiki/Rust_(language))"),
            ["https://en.wikipedia.org/wiki/Rust_(language"]
        );
    }

    #[test]
    fn recovery_links_are_same_origin_only() {
        let base = "https://example.com/";
        assert_eq!(
            same_origin_recovery_link("see [map](/sitemap.xml)", base),
            (true, "same-origin recovery /sitemap.xml".to_string())
        );
        assert!(same_origin_recovery_link("[docs](/docs/start \"Start\")", base).0);
        assert_eq!(
            same_origin_recovery_link("[start](</docs/getting started.md>)", base),
            (
                true,
                "same-origin recovery /docs/getting%20started.md".to_string()
            )
        );
        assert_eq!(
            same_origin_recovery_link("[x](https://evil.test/llms.txt)", base),
            (
                false,
                "no same-origin sitemap.xml, llms.txt, or /docs link".to_string()
            )
        );
        assert_eq!(
            same_origin_recovery_link("[x](/llms.txt)", "nope").1,
            "unparseable audit origin"
        );
    }

    #[test]
    fn retry_shaped_statuses_name_their_reason_phrase() {
        assert_eq!(
            retry_shaped_why(Some(408)).as_deref(),
            Some("the target answered HTTP 408 Request Timeout; not scored")
        );
        assert_eq!(
            retry_shaped_answer(Some(429)).as_deref(),
            Some("HTTP 429 Too Many Requests")
        );
        for status in [None, Some(200), Some(404), Some(500), Some(503)] {
            assert_eq!(retry_shaped_why(status), None, "{status:?}");
        }
    }

    #[test]
    fn misses_rank_broken_over_absent_over_noncompliant_over_error() {
        assert_eq!(worst(&[Miss::Error, Miss::Absent]), ProbeStatus::Absent);
        assert_eq!(worst(&[Miss::Absent, Miss::Broken]), ProbeStatus::Broken);
        assert_eq!(
            worst(&[Miss::Noncompliant, Miss::Absent]),
            ProbeStatus::Absent
        );
        assert_eq!(
            worst(&[Miss::Error, Miss::Noncompliant]),
            ProbeStatus::Noncompliant
        );
        assert_eq!(worst(&[Miss::Error]), ProbeStatus::Error);
        assert_eq!(worst(&[]), ProbeStatus::Error);
    }

    fn response(status: Option<u16>, refused: Option<Refusal>) -> ProbeResponse {
        ProbeResponse {
            status,
            refused,
            ..ProbeResponse::failed(String::new(), 0)
        }
    }

    #[test]
    fn only_an_answered_refusal_redirects_to_http() {
        let refused = Some(Refusal::InsecureScheme);
        assert!(redirects_to_http(&response(Some(301), refused)));
        assert!(!redirects_to_http(&response(None, refused)));
        assert!(!redirects_to_http(&response(Some(301), None)));
    }

    #[test]
    fn plaintext_items_carry_the_site_wording() {
        assert_eq!(
            Value::Object(plaintext_item(PlaintextReason::NotHttps)),
            json!({
                "blocked": "not https",
                "ok": false,
                "why": ["not https; anc sends no plaintext request"],
            })
        );
        assert_eq!(
            Value::Object(redirects_to_http_item("https://x/a", Some(302))),
            json!({
                "url": "https://x/a",
                "status": 302,
                "ok": false,
                "why": ["redirects to http; anc sends no plaintext request"],
            })
        );
    }

    #[test]
    fn javascript_length_and_trim_semantics() {
        assert_eq!(js_len("héllo"), 5);
        assert_eq!(js_len("😀"), 2);
        assert_eq!(js_trim("\u{FEFF} x \u{00A0}"), "x");
    }
}
