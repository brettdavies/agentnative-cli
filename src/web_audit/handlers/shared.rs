//! Helpers every stateless handler shares: token substitution, evidence
//! rows, the miss ranking across candidates, and the same-origin
//! recovery-link scan. A mirror of the site's `handlers/shared.ts`.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;
use url::Url;

use crate::web_audit::engine::ProbeStatus;
use crate::web_audit::scorecard::EvidenceItem;

pub use crate::web_audit::engine::discovery::resolve_url;

/// JavaScript `\s` (WhiteSpace and LineTerminator) as a Rust class body,
/// for the handler patterns the site writes with `\s` and `\S`.
pub const JS_SPACE: &str = r"\t\n\x0B\x0C\r \u{A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}";

/// Replace the `{mcp_endpoint}` token; empty when the endpoint is unknown.
pub fn substitute_endpoint(value: &str, mcp_endpoint: Option<&str>) -> String {
    if !value.contains("{mcp_endpoint}") {
        return value.to_string();
    }
    value.replace("{mcp_endpoint}", mcp_endpoint.unwrap_or(""))
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

/// How a non-passing candidate missed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Miss {
    /// The surface is there and wrong.
    Broken,
    /// The surface is not there.
    Absent,
    /// The probe did not complete.
    Error,
}

/// Across candidates, any broken outranks absent (something is there and
/// wrong) and a definitive absence outranks an operational error.
pub fn worst(misses: &[Miss]) -> ProbeStatus {
    if misses.contains(&Miss::Broken) {
        ProbeStatus::Broken
    } else if misses.contains(&Miss::Absent) {
        ProbeStatus::Absent
    } else {
        ProbeStatus::Error
    }
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

static MARKDOWN_HREF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\]\(([^)\s]+)(?:\s+"[^"]*")?\)"#).unwrap());

/// Every markdown link target in `body`, in order, with an optional title
/// stripped.
pub fn markdown_hrefs(body: &str) -> impl Iterator<Item = &str> {
    MARKDOWN_HREF_RE
        .captures_iter(body)
        .map(|c| c.get(1).unwrap().as_str())
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

    #[test]
    fn recovery_links_are_same_origin_only() {
        let base = "https://example.com/";
        assert_eq!(
            same_origin_recovery_link("see [map](/sitemap.xml)", base),
            (true, "same-origin recovery /sitemap.xml".to_string())
        );
        assert!(same_origin_recovery_link("[docs](/docs/start \"Start\")", base).0);
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
    fn misses_rank_broken_over_absent_over_error() {
        assert_eq!(worst(&[Miss::Error, Miss::Absent]), ProbeStatus::Absent);
        assert_eq!(worst(&[Miss::Absent, Miss::Broken]), ProbeStatus::Broken);
        assert_eq!(worst(&[Miss::Error]), ProbeStatus::Error);
        assert_eq!(worst(&[]), ProbeStatus::Error);
    }

    #[test]
    fn javascript_length_and_trim_semantics() {
        assert_eq!(js_len("héllo"), 5);
        assert_eq!(js_len("😀"), 2);
        assert_eq!(js_trim("\u{FEFF} x \u{00A0}"), "x");
    }
}
