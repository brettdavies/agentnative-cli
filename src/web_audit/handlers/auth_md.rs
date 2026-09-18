//! The `auth-md` handler, a mirror of the site's `handlers/auth-md.ts`:
//! the agent auth/registration guide at `/.well-known/auth.md` or
//! `/auth.md`. The `auth-present` antecedent gates it to sites that expose
//! an auth surface, so an open site is n_a, never penalized.

use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;
use serde_json::json;

use super::shared::{Miss, is_missing_status, item, js_trim, resolve_url, worst};
use crate::web_audit::engine::{HandlerContext, ProbeOutcome, ProbeStatus};
use crate::web_audit::fetch::FetchInit;
use crate::web_audit::registry::WebCheck;

/// The `with` block of the `auth-md` check.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct AuthMdWith {
    /// Candidate paths.
    pub path_any: Option<Vec<String>>,
    /// One path.
    pub path: Option<String>,
    /// Per-request timeout in seconds.
    pub timeout: Option<f64>,
}

static MARKDOWNISH_CT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)markdown|text/plain").unwrap());
static HTML_CT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)text/html").unwrap());
static LEADING_HEADING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[\t\n\x0B\x0C\r \u{A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}]*#").unwrap()
});

/// Probe the candidate paths for a non-empty markdown or plain-text document.
pub fn run_auth_md(check: &WebCheck, ctx: &HandlerContext) -> ProbeOutcome {
    let w: AuthMdWith = serde_json::from_str(check.with_json).unwrap_or_default();
    let paths: Vec<String> = w
        .path_any
        .clone()
        .or_else(|| w.path.clone().map(|p| vec![p]))
        .unwrap_or_else(|| vec!["/.well-known/auth.md".to_string(), "/auth.md".to_string()]);
    let opts = ctx.fetch_options(w.timeout);

    let mut evidence = Vec::new();
    let mut misses: Vec<Miss> = Vec::new();
    for raw_path in &paths {
        let url = resolve_url(&ctx.base, raw_path);
        if url.is_empty() {
            continue;
        }
        let resp = ctx.fetch.fetch(&url, &FetchInit::default(), &opts);
        if let Some(error) = &resp.error {
            evidence.push(item([
                ("url", json!(url)),
                ("status", json!(null)),
                ("error", json!(error)),
            ]));
            misses.push(Miss::Error);
            continue;
        }
        let ct = resp.headers.get("content-type").unwrap_or("");
        if resp.status == Some(200) {
            // Valid: a non-empty markdown or plain document, or one that at
            // least opens with a markdown heading. An HTML page or an empty
            // body at the auth.md path is a present-but-broken surface.
            let looks_markdown = MARKDOWNISH_CT.is_match(ct)
                || (!HTML_CT.is_match(ct) && LEADING_HEADING.is_match(&resp.body));
            let valid = !js_trim(&resp.body).is_empty() && looks_markdown;
            evidence.push(item([
                ("url", json!(url)),
                ("status", json!(resp.status)),
                ("ok", json!(valid)),
                ("content_type", json!(ct)),
            ]));
            if valid {
                return ProbeOutcome::new(ProbeStatus::Pass, evidence);
            }
            misses.push(Miss::Broken);
            continue;
        }
        evidence.push(item([
            ("url", json!(url)),
            ("status", json!(resp.status)),
            ("ok", json!(false)),
        ]));
        misses.push(if is_missing_status(resp.status) {
            Miss::Absent
        } else {
            Miss::Broken
        });
    }
    ProbeOutcome::new(worst(&misses), evidence)
}
