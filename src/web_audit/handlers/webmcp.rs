//! The `webmcp` handler, a mirror of the site's `handlers/webmcp.ts`:
//! scans the target's root HTML for the static markers a page ships when
//! it exposes browser WebMCP tools. Reuses the canonical root fetch.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::json;

use super::shared::{item, why};
use crate::web_audit::engine::{HandlerContext, ProbeOutcome, ProbeStatus};
use crate::web_audit::registry::WebCheck;

// Each marker must be structural: an attribute, a script element, or a
// qualified property access. A bare `webmcp` or `modelcontext` substring
// is not evidence, because naming the Model Context Protocol in nav copy
// is near-universal on the sites this audit targets. `modelContext` stays
// case-sensitive: it is a JS property name.
static WEBMCP_MARKERS: LazyLock<[(&str, Regex); 3]> = LazyLock::new(|| {
    [
        (
            "application/webmcp script block",
            Regex::new(r#"(?i)\btype\s*=\s*["']?application/webmcp(?:\+json)?\b"#).unwrap(),
        ),
        (
            "modelContext API reference",
            Regex::new(r"\b(?:navigator|document|window)\s*\.\s*modelContext\b").unwrap(),
        ),
        (
            "webmcp script asset",
            Regex::new(r#"(?i)<script\b[^>]*\bsrc\s*=\s*["'][^"']*webmcp[^"']*["']"#).unwrap(),
        ),
    ]
});

/// Scan the root HTML for a WebMCP marker.
pub fn run_webmcp(_check: &WebCheck, ctx: &HandlerContext) -> ProbeOutcome {
    let Some(root) = ctx.root.as_deref().filter(|r| r.status.is_some()) else {
        return ProbeOutcome::error("root fetch failed");
    };
    if let Some((name, _)) = WEBMCP_MARKERS
        .iter()
        .find(|(_, re)| re.is_match(&root.body))
    {
        // The marker's own label, never the matched span: the span is the
        // target's markup, unbounded in length and target-controlled.
        return ProbeOutcome::new(
            ProbeStatus::Pass,
            vec![item([
                ("url", json!(ctx.base)),
                ("status", json!(root.status)),
                ("ok", json!(true)),
                ("marker", json!(name)),
            ])],
        );
    }
    ProbeOutcome::new(
        ProbeStatus::Absent,
        vec![item([
            ("url", json!(ctx.base)),
            ("status", json!(root.status)),
            ("ok", json!(false)),
            ("why", why(&["no WebMCP markers in root HTML"])),
        ])],
    )
}
