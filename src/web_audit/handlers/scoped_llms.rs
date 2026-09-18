//! The `scoped-llms` handler, a mirror of the site's
//! `handlers/scoped-llms.ts`: per-section `llms.txt` / `llms-full.txt`
//! candidates under the section directories the engine enumerated from
//! the root llms.txt link index and the sitemap. Every candidate came
//! from the target's own documents, so each is validated before it is
//! fetched.

use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;
use serde_json::json;
use url::Url;

use super::shared::{Miss, is_missing_status, item, js_trim, worst};
use crate::web_audit::engine::{HandlerContext, ProbeOutcome, ProbeStatus, why_item};
use crate::web_audit::fetch::FetchInit;
use crate::web_audit::registry::WebCheck;

/// Candidates probed when the check names no cap.
pub const DEFAULT_MAX_CANDIDATES: usize = 8;

/// The `with` block of a `scoped-llms` check.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct ScopedLlmsWith {
    /// The file under each section directory, `llms.txt` by default.
    pub file: Option<String>,
    /// Candidate cap.
    pub max_candidates: Option<usize>,
    /// Per-request timeout in seconds.
    pub timeout: Option<f64>,
}

static LOOKS_LIKE_LLMS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mR)^#|\]\(").unwrap());

/// Probe each section directory for the scoped file.
pub fn run_scoped_llms(check: &WebCheck, ctx: &HandlerContext) -> ProbeOutcome {
    let w: ScopedLlmsWith = serde_json::from_str(check.with_json).unwrap_or_default();
    let file = w.file.as_deref().unwrap_or("llms.txt");
    let cap = w.max_candidates.unwrap_or(DEFAULT_MAX_CANDIDATES);
    let opts = ctx.fetch_options(w.timeout);
    let dirs: Vec<&String> = ctx.scoped_dirs.iter().take(cap).collect();

    if dirs.is_empty() {
        return ProbeOutcome::new(
            ProbeStatus::Absent,
            vec![why_item(
                "no section directories in the root llms.txt or sitemap",
            )],
        );
    }

    let mut evidence = Vec::new();
    let mut misses: Vec<Miss> = Vec::new();
    for dir in dirs {
        let Some(url) = Url::parse(&ctx.base)
            .ok()
            .and_then(|b| b.join(&format!("{dir}/{file}")).ok())
            .map(|u| u.to_string())
        else {
            continue;
        };
        if let Err(reason) = ctx.validate_nested(&url) {
            evidence.push(item([("url", json!(url)), ("blocked", json!(reason))]));
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
        if resp.status == Some(200) {
            let valid = !js_trim(&resp.body).is_empty() && LOOKS_LIKE_LLMS.is_match(&resp.body);
            evidence.push(item([
                ("url", json!(url)),
                ("status", json!(resp.status)),
                ("ok", json!(valid)),
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
    if misses.is_empty() {
        return ProbeOutcome::new(ProbeStatus::Absent, evidence);
    }
    ProbeOutcome::new(worst(&misses), evidence)
}
