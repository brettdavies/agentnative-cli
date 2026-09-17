//! The `content-without-js` handler, a mirror of the site's
//! `handlers/content-without-js.ts`: the root HTML must carry an H1 and a
//! minimum of visible text. When that floor fails but a passing
//! `llms.txt` links resolvable same-origin content, the row softens to
//! `na` after bounded, validated probes of those links.

use std::collections::HashSet;
use std::sync::LazyLock;
use std::time::Instant;

use regex::Regex;
use serde::Deserialize;
use serde_json::json;
use url::Url;

use super::shared::{JS_SPACE, item, js_len, js_trim, markdown_hrefs, why};
use crate::web_audit::engine::{HandlerContext, ProbeOutcome, ProbeStatus, why_item};
use crate::web_audit::fetch::body::AUDIT_PROBE_MAX_BODY_BYTES;
use crate::web_audit::fetch::{FetchInit, FetchOptions};
use crate::web_audit::registry::WebCheck;
use crate::web_audit::scorecard::EvidenceItem;

const MIN_VISIBLE_CHARS: usize = 200;
const MAX_TWIN_PROBES: usize = 3;
const MIN_TWIN_CHARS: usize = 40;
const INDEX_PATHS: [&str; 5] = [
    "/llms.txt",
    "/llms-full.txt",
    "/robots.txt",
    "/sitemap.xml",
    "/favicon.ico",
];
const BUDGET_EXHAUSTED: &str = "nested-probe budget exhausted";

/// The `with` block of the `content-without-js` check.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct ContentWithoutJsWith {
    /// The check's whole budget in seconds.
    pub timeout: Option<f64>,
}

static SCRIPT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<script.*?</script>").unwrap());
static STYLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<style.*?</style>").unwrap());
static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());
static SPACE_RUN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"[{JS_SPACE}]+")).unwrap());
static H1: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?i)<h1[{JS_SPACE}>]")).unwrap());

/// The page's text with scripts, styles and tags removed and whitespace
/// collapsed.
pub fn visible_text(html: &str) -> String {
    let stripped = SCRIPT.replace_all(html, " ");
    let stripped = STYLE.replace_all(&stripped, " ");
    let stripped = TAG.replace_all(&stripped, " ");
    let collapsed = SPACE_RUN.replace_all(&stripped, " ");
    js_trim(&collapsed).to_string()
}

fn floor_met(html: &str) -> (bool, Vec<String>) {
    let has_h1 = H1.is_match(html);
    let text = visible_text(html);
    let length = js_len(&text);
    let why = vec![
        if has_h1 {
            "h1 present".to_string()
        } else {
            "no h1 in raw HTML".to_string()
        },
        format!("visible text {length} chars (min {MIN_VISIBLE_CHARS})"),
    ];
    (has_h1 && length >= MIN_VISIBLE_CHARS, why)
}

/// Same-origin content links from the llms.txt body, index files and the
/// root excluded, deduplicated.
pub fn content_hrefs(llms_body: &str, base: &str) -> Vec<String> {
    let Ok(base_url) = Url::parse(base) else {
        return Vec::new();
    };
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for raw in markdown_hrefs(llms_body) {
        let Ok(url) = base_url.join(raw) else {
            continue;
        };
        if url.origin() != base_url.origin() {
            continue;
        }
        let pathname = url.path();
        let trimmed = pathname.strip_suffix('/').unwrap_or(pathname);
        let path = if trimmed.is_empty() { "/" } else { trimmed };
        if INDEX_PATHS.contains(&pathname) || INDEX_PATHS.contains(&path) {
            continue;
        }
        if pathname == "/" || trimmed.is_empty() {
            continue;
        }
        let href = url.to_string();
        if seen.insert(href.clone()) {
            out.push(href);
        }
    }
    out
}

/// Check the root's content floor, softening on a resolvable twin.
pub fn run_content_without_js(check: &WebCheck, ctx: &HandlerContext) -> ProbeOutcome {
    let w: ContentWithoutJsWith = serde_json::from_str(check.with_json).unwrap_or_default();
    let deadline_at = Instant::now() + ctx.timeout_for(w.timeout);
    let Some(root) = ctx.root.as_deref() else {
        return ProbeOutcome::error("root fetch unavailable");
    };
    let Some(status) = root.status else {
        return ProbeOutcome::error("root fetch unavailable");
    };
    let root_row = |ok: bool, lines: &[String]| {
        item([
            ("url", json!(ctx.base)),
            ("status", json!(status)),
            ("ok", json!(ok)),
            ("why", json!(lines)),
        ])
    };

    if !(200..300).contains(&status) {
        return ProbeOutcome::new(
            ProbeStatus::Broken,
            vec![root_row(
                false,
                &[format!("root status {status} is not 2xx")],
            )],
        );
    }

    let (floor_ok, floor_why) = floor_met(&root.body);
    if floor_ok {
        return ProbeOutcome::new(ProbeStatus::Pass, vec![root_row(true, &floor_why)]);
    }

    let llms_body = ctx
        .retained_bodies
        .get("llms-txt")
        .map(String::as_str)
        .unwrap_or("");
    if llms_body.is_empty() {
        let mut lines = floor_why.clone();
        lines.push("no retained llms.txt body".to_string());
        return ProbeOutcome::new(ProbeStatus::Absent, vec![root_row(false, &lines)]);
    }

    let hrefs: Vec<String> = content_hrefs(llms_body, &ctx.base)
        .into_iter()
        .take(MAX_TWIN_PROBES)
        .collect();
    let mut evidence: Vec<EvidenceItem> = vec![root_row(false, &floor_why)];
    let mut exhausted = false;
    for href in &hrefs {
        let slice = deadline_at.saturating_duration_since(Instant::now());
        if slice.is_zero() {
            evidence.push(why_item(BUDGET_EXHAUSTED));
            exhausted = true;
            break;
        }
        if let Err(reason) = ctx.validate_nested(href) {
            evidence.push(item([("url", json!(href)), ("blocked", json!(reason))]));
            continue;
        }
        let resp = ctx.fetch.fetch(
            href,
            &FetchInit::default(),
            &FetchOptions {
                timeout: slice,
                max_body_bytes: Some(AUDIT_PROBE_MAX_BODY_BYTES),
                ..FetchOptions::default()
            },
        );
        let Some(twin_status) = resp.status.filter(|_| resp.error.is_none()) else {
            evidence.push(item([
                ("url", json!(href)),
                ("status", json!(resp.status)),
                ("error", json!(resp.error)),
            ]));
            continue;
        };
        let length = js_len(js_trim(&resp.body));
        let substantial = (200..300).contains(&twin_status) && length >= MIN_TWIN_CHARS;
        evidence.push(item([
            ("url", json!(href)),
            ("status", json!(twin_status)),
            ("ok", json!(substantial)),
            ("why", why(&[&format!("twin body {length} chars")])),
        ]));
        if substantial {
            evidence.push(why_item(
                "digital twin discoverable via resolvable llms.txt content link",
            ));
            return ProbeOutcome::new(ProbeStatus::Na, evidence);
        }
    }

    evidence.push(why_item(if hrefs.is_empty() {
        "llms.txt has no same-origin content links"
    } else {
        "llms.txt content links did not resolve"
    }));
    ProbeOutcome {
        incomplete: exhausted,
        ..ProbeOutcome::new(ProbeStatus::Absent, evidence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_text_strips_scripts_styles_and_tags() {
        let html = "<html><head><style>b{}</style><script>var x = '<b>';</script></head><body><h1>Hi</h1>\n<p>there  \t you</p></body></html>";
        assert_eq!(visible_text(html), "Hi there you");
        assert!(H1.is_match("<H1 class=\"x\">"));
        assert!(!H1.is_match("<h10>"));
    }

    #[test]
    fn content_hrefs_keep_same_origin_content_pages_only() {
        let body = "[a](/docs/a) [b](/llms.txt) [c](/) [d](/docs/) [e](https://o.test/x) [f](/docs/a) [g](/sitemap.xml/)";
        assert_eq!(
            content_hrefs(body, "https://example.com/"),
            ["https://example.com/docs/a", "https://example.com/docs/"]
        );
    }
}
