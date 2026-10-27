//! The `llms-txt-quality` trio (format / links / when-to-use), a mirror
//! of the site's `handlers/llms-txt-quality.ts`. Reads the retained
//! wave-1 `/llms.txt` body, so format and when-to-use issue no fetch;
//! link probes are validated and budgeted like scoped-llms.

use std::collections::HashSet;
use std::sync::LazyLock;
use std::time::Instant;

use regex::Regex;
use serde::Deserialize;
use serde_json::json;
use url::Url;

use super::shared::{JS_SPACE, Miss, is_missing_status, item, markdown_hrefs, why, worst};
use crate::web_audit::engine::{HandlerContext, ProbeOutcome, ProbeStatus, why_item};
use crate::web_audit::fetch::body::STATUS_ONLY_BODY_BYTES;
use crate::web_audit::fetch::{FetchInit, FetchOptions};
use crate::web_audit::registry::WebCheck;
use crate::web_audit::scorecard::EvidenceItem;

/// Links probed when the check names no cap.
pub const DEFAULT_MAX_LINKS: usize = 8;

const BUDGET_EXHAUSTED: &str = "nested-probe budget exhausted";

/// The `with` block of an `llms-txt-quality` check.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct LlmsTxtQualityWith {
    /// `format`, `links` or `when-to-use`; `format` by default.
    pub op: Option<String>,
    /// Link cap for the `links` op.
    pub max_candidates: Option<usize>,
    /// The op's whole budget in seconds.
    pub timeout: Option<f64>,
}

static H1: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?mR)^#[{JS_SPACE}]+[^{JS_SPACE}]")).unwrap());
static SUMMARY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?mR)^>[{JS_SPACE}]+[^{JS_SPACE}]")).unwrap());
static LINKS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"\]\([^)^{JS_SPACE}]+\)")).unwrap());
static WHEN_TO_USE_HEADING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?imR)^#{{1,3}}[{JS_SPACE}]+[^\n\r\u{{2028}}\u{{2029}}]*(when[{JS_SPACE}]+to[{JS_SPACE}]+use|programmatic access|when to (?:connect|call) (?:the )?mcp)"
    ))
    .unwrap()
});

fn format_why(body: &str) -> (bool, Vec<&'static str>) {
    let has_h1 = H1.is_match(body);
    let has_summary = SUMMARY.is_match(body);
    let has_links = LINKS.is_match(body);
    let why = vec![
        if has_h1 { "h1 present" } else { "no h1" },
        if has_summary {
            "summary blockquote present"
        } else {
            "no summary blockquote"
        },
        if has_links {
            "link index present"
        } else {
            "no markdown link index"
        },
    ];
    (has_h1 && has_summary && has_links, why)
}

/// Followable, deduplicated hrefs from the llms.txt body, resolved against
/// the base.
pub fn hrefs_from(body: &str, base: &str) -> Vec<String> {
    let Ok(base) = Url::parse(base) else {
        return Vec::new();
    };
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for raw in markdown_hrefs(body) {
        if raw.starts_with('#') || raw.starts_with("mailto:") || raw.starts_with("javascript:") {
            continue;
        }
        let Ok(href) = base.join(raw) else {
            continue;
        };
        let href = href.to_string();
        if seen.insert(href.clone()) {
            out.push(href);
        }
    }
    out
}

/// Evaluate the op against the retained llms.txt body.
pub fn run_llms_txt_quality(check: &WebCheck, ctx: &HandlerContext) -> ProbeOutcome {
    let w: LlmsTxtQualityWith = serde_json::from_str(check.with_json).unwrap_or_default();
    let op = w.op.as_deref().unwrap_or("format");
    let body = ctx
        .retained_bodies
        .get("llms-txt")
        .map(String::as_str)
        .unwrap_or("");
    if body.is_empty() {
        return ProbeOutcome::new(
            ProbeStatus::Absent,
            vec![why_item("no retained llms.txt body")],
        );
    }
    let llms_url = format!("{}llms.txt", ctx.base);

    if op == "format" {
        let (ok, lines) = format_why(body);
        return ProbeOutcome::new(
            if ok {
                ProbeStatus::Pass
            } else {
                ProbeStatus::Absent
            },
            vec![item([
                ("url", json!(llms_url)),
                ("ok", json!(ok)),
                ("why", why(&lines)),
            ])],
        );
    }

    if op == "when-to-use" {
        let ok = WHEN_TO_USE_HEADING.is_match(body);
        return ProbeOutcome::new(
            if ok {
                ProbeStatus::Pass
            } else {
                ProbeStatus::Absent
            },
            vec![item([
                ("url", json!(llms_url)),
                ("ok", json!(ok)),
                (
                    "why",
                    why(&[if ok {
                        "when-to-use or programmatic-access heading present"
                    } else {
                        "no when-to-use heading"
                    }]),
                ),
            ])],
        );
    }

    let deadline_at = Instant::now() + ctx.timeout_for(w.timeout);
    let cap = w.max_candidates.unwrap_or(DEFAULT_MAX_LINKS);
    let hrefs: Vec<String> = hrefs_from(body, &ctx.base).into_iter().take(cap).collect();
    if hrefs.is_empty() {
        return ProbeOutcome::new(
            ProbeStatus::Absent,
            vec![why_item("llms.txt has no followable links")],
        );
    }

    let mut evidence: Vec<EvidenceItem> = Vec::new();
    let mut misses: Vec<Miss> = Vec::new();
    let mut exhausted = false;
    for href in &hrefs {
        let slice = deadline_at.saturating_duration_since(Instant::now());
        if slice.is_zero() {
            evidence.push(why_item(BUDGET_EXHAUSTED));
            misses.push(Miss::Error);
            exhausted = true;
            break;
        }
        if let Err(reason) = ctx.validate_nested(href) {
            evidence.push(item([
                ("url", json!(href)),
                ("blocked", json!(reason)),
                ("ok", json!(false)),
            ]));
            misses.push(Miss::Absent);
            continue;
        }
        let resp = ctx.fetch.fetch(
            href,
            &FetchInit::default(),
            &FetchOptions {
                timeout: slice,
                max_body_bytes: Some(STATUS_ONLY_BODY_BYTES),
                ..FetchOptions::default()
            },
        );
        let Some(status) = resp.status.filter(|_| resp.error.is_none()) else {
            evidence.push(item([
                ("url", json!(href)),
                ("status", json!(resp.status)),
                ("error", json!(resp.error)),
                ("ok", json!(false)),
            ]));
            misses.push(Miss::Error);
            continue;
        };
        let ok = (200..400).contains(&status);
        evidence.push(item([
            ("url", json!(href)),
            ("status", json!(status)),
            ("ok", json!(ok)),
        ]));
        if !ok {
            misses.push(if is_missing_status(Some(status)) {
                Miss::Absent
            } else {
                Miss::Broken
            });
        }
    }

    if misses.is_empty() {
        return ProbeOutcome::new(ProbeStatus::Pass, evidence);
    }
    ProbeOutcome {
        incomplete: exhausted,
        ..ProbeOutcome::new(worst(&misses), evidence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_reads_the_three_structural_signals() {
        let good = "# Site\r\n\r\n> A summary\r\n\r\n- [Docs](/docs)\r\n";
        assert_eq!(
            format_why(good),
            (
                true,
                vec![
                    "h1 present",
                    "summary blockquote present",
                    "link index present"
                ]
            )
        );
        assert_eq!(
            format_why("plain text").1,
            vec!["no h1", "no summary blockquote", "no markdown link index"]
        );
    }

    #[test]
    fn when_to_use_heading_matches_the_site_forms() {
        assert!(WHEN_TO_USE_HEADING.is_match("## When To Use\n"));
        assert!(WHEN_TO_USE_HEADING.is_match("# Programmatic access\n"));
        assert!(WHEN_TO_USE_HEADING.is_match("### Guide: when to call the MCP\n"));
        assert!(!WHEN_TO_USE_HEADING.is_match("#### when to use\n"));
        assert!(!WHEN_TO_USE_HEADING.is_match("when to use\n"));
    }

    #[test]
    fn hrefs_skip_fragments_and_schemes_and_deduplicate() {
        let body = "[a](/x) [b](#top) [c](mailto:x@y) [d](javascript:void) [e](/x) [f](https://o.test/y \"T\")";
        assert_eq!(
            hrefs_from(body, "https://example.com/"),
            ["https://example.com/x", "https://o.test/y"]
        );
    }
}
