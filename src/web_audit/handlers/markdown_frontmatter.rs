//! The `markdown-frontmatter` handler, a mirror of the site's
//! `handlers/markdown-frontmatter.ts`: a YAML frontmatter block at the
//! head of the root markdown twin. Detection is structural (fence pair
//! plus at least one key line); the values are never parsed.

use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::http::header_pairs;
use super::shared::{JS_SPACE, item, resolve_url, why};
use crate::web_audit::engine::{HandlerContext, ProbeOutcome, ProbeStatus};
use crate::web_audit::fetch::FetchInit;
use crate::web_audit::registry::WebCheck;

/// The `with` block of the `markdown-frontmatter` check.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct MarkdownFrontmatterWith {
    /// The path to probe, `/` by default.
    pub path: Option<String>,
    /// Request headers, `Accept: text/markdown` by default.
    pub headers: Option<Map<String, Value>>,
    /// Per-request timeout in seconds.
    pub timeout: Option<f64>,
}

static HTML_CT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)text/html").unwrap());
static FENCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:---|\.\.\.)$").unwrap());
static KEY_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^[^{JS_SPACE}][^:]*:([{JS_SPACE}]|$)")).unwrap());

/// Fetch the markdown twin and look for a well-formed frontmatter block.
pub fn run_markdown_frontmatter(check: &WebCheck, ctx: &HandlerContext) -> ProbeOutcome {
    let w: MarkdownFrontmatterWith = serde_json::from_str(check.with_json).unwrap_or_default();
    let path = w.path.as_deref().unwrap_or("/");
    let headers = match &w.headers {
        Some(headers) => header_pairs(Some(headers)),
        None => vec![("Accept".to_string(), "text/markdown".to_string())],
    };
    let opts = ctx.fetch_options(w.timeout);

    let url = resolve_url(&ctx.base, path);
    if url.is_empty() {
        return ProbeOutcome::error("no resolvable probe URL");
    }
    let resp = ctx.fetch.fetch(
        &url,
        &FetchInit {
            headers,
            ..FetchInit::default()
        },
        &opts,
    );
    if resp.error.is_some() || resp.status.is_none() {
        return ProbeOutcome::new(
            ProbeStatus::Error,
            vec![item([
                ("url", json!(url)),
                ("status", json!(resp.status)),
                ("error", json!(resp.error)),
            ])],
        );
    }
    let row = |ok: bool, line: &str| {
        vec![item([
            ("url", json!(url)),
            ("status", json!(resp.status)),
            ("ok", json!(ok)),
            ("why", why(&[line])),
        ])]
    };

    // The antecedent should preclude an HTML root here; guard anyway so a
    // stray `---` in HTML never reads as a frontmatter fence.
    let content_type = resp.headers.get("content-type").unwrap_or("");
    if HTML_CT.is_match(content_type) {
        return ProbeOutcome::new(
            ProbeStatus::Absent,
            row(false, "root served HTML, not a markdown twin"),
        );
    }

    let body = resp.body.strip_prefix('\u{FEFF}').unwrap_or(&resp.body);
    let lines: Vec<&str> = body
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    if lines.first() != Some(&"---") {
        return ProbeOutcome::new(
            ProbeStatus::Absent,
            row(false, "no leading frontmatter fence"),
        );
    }
    let Some(terminator) = (1..lines.len()).find(|&i| FENCE.is_match(lines[i])) else {
        return ProbeOutcome::new(
            ProbeStatus::Broken,
            row(false, "unterminated frontmatter fence"),
        );
    };
    let key_lines = (1..terminator)
        .filter(|&i| KEY_LINE.is_match(lines[i]))
        .count();
    if key_lines == 0 {
        return ProbeOutcome::new(
            ProbeStatus::Broken,
            row(false, "frontmatter fence encloses no key line"),
        );
    }
    ProbeOutcome::new(
        ProbeStatus::Pass,
        row(
            true,
            &format!("frontmatter present ({key_lines} key lines)"),
        ),
    )
}
