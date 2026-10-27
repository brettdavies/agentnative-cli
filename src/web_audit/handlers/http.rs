//! The `http` handler and the `legacy-alias-redirects` and
//! `retained-document` eval rules, a mirror of the site's
//! `handlers/http.ts`.

use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::assert::{AliasVerdict, ExpectBlock, assert_http, classify_alias_probe};
use super::shared::{
    Miss, is_missing_status, item, redirects_to_http, redirects_to_http_item, resolve_url,
    retry_shaped_why, same_origin_recovery_link, substitute_endpoint, worst,
};
use crate::web_audit::engine::{HandlerContext, ProbeOutcome, ProbeStatus};
use crate::web_audit::fetch::{FetchInit, FetchOptions, ProbeResponse};
use crate::web_audit::registry::WebCheck;
use crate::web_audit::scorecard::EvidenceItem;

/// The `with` block of an `http` check.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct HttpWith {
    /// One path to probe.
    pub path: Option<String>,
    /// Candidate paths; the first that passes wins.
    pub path_any: Option<Vec<String>>,
    /// HTTP method, `GET` by default.
    pub method: Option<String>,
    /// Request headers; their presence also disables root reuse.
    pub headers: Option<Map<String, Value>>,
    /// The expectation block.
    pub expect: Option<ExpectBlock>,
    /// Per-request timeout in seconds; its presence opts into hang detection.
    pub timeout: Option<f64>,
    /// Keep the passing body on the evidence row.
    pub retain_body: Option<bool>,
    /// `retained-document` checks only: the document discovery kept that the
    /// check scores.
    pub retained: Option<String>,
}

/// Header pairs from a `with.headers` object, string values only.
pub fn header_pairs(headers: Option<&Map<String, Value>>) -> Vec<(String, String)> {
    headers
        .into_iter()
        .flat_map(|m| m.iter())
        .filter_map(|(name, value)| value.as_str().map(|v| (name.clone(), v.to_string())))
        .collect()
}

/// Classify a non-passing candidate. A 404/410 is a missing surface. With
/// a status expectation any other miss is a surface that exists and
/// misbehaves; without one the check probes an affordance of an existing
/// document, so a failed assertion is an absence. A timeout is operational
/// unless the check opted into an explicit hang budget. A redirect to http
/// is absent: anc never takes that hop, and a surface served only over
/// plaintext earns no more than a missing one.
fn classify_miss(resp: &ProbeResponse, expect: &ExpectBlock, has_explicit_timeout: bool) -> Miss {
    if redirects_to_http(resp) {
        return Miss::Absent;
    }
    if let Some(error) = &resp.error {
        return if error.starts_with("TimeoutError") && has_explicit_timeout {
            Miss::Broken
        } else {
            Miss::Error
        };
    }
    if is_missing_status(resp.status) {
        return Miss::Absent;
    }
    if expect.has_status_expectation() {
        Miss::Broken
    } else {
        Miss::Absent
    }
}

/// One response asserted against the check's expectations, as its
/// evidence row, and whether it passed. A redirect to http is never
/// asserted: nothing it serves is readable over https.
fn assess_response(
    check: &WebCheck,
    url: &str,
    resp: &ProbeResponse,
    w: &HttpWith,
    expect: &ExpectBlock,
    base: &str,
) -> (bool, EvidenceItem) {
    if redirects_to_http(resp) {
        let mut row = redirects_to_http_item(url, resp.status);
        row.insert("elapsed_ms".to_string(), json!(resp.elapsed_ms));
        row.insert("error".to_string(), Value::Null);
        return (false, row);
    }
    let asserted = assert_http(check, expect, resp);
    let (recovery_ok, recovery_why) =
        if asserted.ok && expect.same_origin_recovery_link == Some(true) {
            same_origin_recovery_link(&resp.body, base)
        } else {
            (true, String::new())
        };
    let ok = asserted.ok && recovery_ok;
    let mut reasons = asserted.reasons;
    if !recovery_why.is_empty() {
        reasons.push(recovery_why);
    }
    let mut row = item([
        ("url", json!(url)),
        ("status", json!(resp.status)),
        ("ok", json!(ok)),
        ("why", json!(reasons)),
        ("elapsed_ms", json!(resp.elapsed_ms)),
        ("error", json!(resp.error)),
    ]);
    if resp.truncated {
        row.insert("truncated".to_string(), json!(true));
    }
    if w.retain_body == Some(true) && ok {
        row.insert("body".to_string(), json!(resp.body));
    }
    (ok, row)
}

/// Resolve `path` or each `path_any` candidate, issue the method with the
/// headers under the check's timeout, and evaluate the expectation.
pub fn run_http(check: &WebCheck, ctx: &HandlerContext) -> ProbeOutcome {
    let w: HttpWith = serde_json::from_str(check.with_json).unwrap_or_default();
    let paths: Vec<String> = w
        .path_any
        .clone()
        .or_else(|| w.path.clone().map(|p| vec![p]))
        .unwrap_or_default();
    let method = w.method.as_deref().unwrap_or("GET");
    let headers = header_pairs(w.headers.as_ref());
    let expect = w.expect.clone().unwrap_or_default();
    let opts = ctx.fetch_options(w.timeout);

    let mut evidence = Vec::new();
    let mut misses: Vec<Miss> = Vec::new();
    for raw_path in &paths {
        let url = resolve_url(
            &ctx.base,
            &substitute_endpoint(raw_path, ctx.mcp_endpoint.as_deref()),
        );
        if url.is_empty() {
            continue;
        }
        let reuse_root =
            ctx.root.is_some() && url == ctx.base && method == "GET" && w.headers.is_none();
        let fetched;
        let resp: &ProbeResponse = match (&ctx.root, reuse_root) {
            (Some(root), true) => root,
            _ => {
                fetched = ctx.fetch.fetch(
                    &url,
                    &FetchInit {
                        method: method.to_string(),
                        headers: headers.clone(),
                        body: None,
                    },
                    &opts,
                );
                &fetched
            }
        };
        // Settled ahead of the assertion, as on every other probe of the MCP
        // endpoint: an expectation like `status_below: 500` would read a busy
        // answer as the fast refusal it asks for.
        let busy = if raw_path.contains("{mcp_endpoint}") {
            retry_shaped_why(resp.status)
        } else {
            None
        };
        if let Some(busy) = busy {
            evidence.push(item([
                ("url", json!(url)),
                ("status", json!(resp.status)),
                ("ok", json!(false)),
                ("why", json!([busy])),
                ("elapsed_ms", json!(resp.elapsed_ms)),
                ("error", json!(resp.error)),
            ]));
            misses.push(Miss::Error);
            continue;
        }
        let (ok, row) = assess_response(check, &url, resp, &w, &expect, &ctx.base);
        evidence.push(row);
        if ok {
            return ProbeOutcome::new(ProbeStatus::Pass, evidence);
        }
        misses.push(classify_miss(resp, &expect, w.timeout.is_some()));
    }
    if evidence.is_empty() {
        return ProbeOutcome::na("no resolvable probe URL");
    }
    ProbeOutcome::new(worst(&misses), evidence)
}

/// Score a document discovery already read against the check's `expect`,
/// exactly as the same response fetched live would score, with no request
/// of its own. Discovery keeps a document only when it read one, so a
/// document it did not keep is absent.
pub fn run_retained_document(check: &WebCheck, ctx: &HandlerContext) -> ProbeOutcome {
    let w: HttpWith = serde_json::from_str(check.with_json).unwrap_or_default();
    let key = w.retained.clone().unwrap_or_default();
    let Some(doc) = ctx.retained_documents.get(key.as_str()) else {
        return ProbeOutcome::new(
            ProbeStatus::Absent,
            vec![item([
                ("retained", json!(key)),
                ("why", json!([format!("discovery kept no {key} document")])),
            ])],
        );
    };
    let expect = w.expect.clone().unwrap_or_default();
    let (ok, mut row) = assess_response(check, &doc.url, &doc.response, &w, &expect, &ctx.base);
    row.insert("retained".to_string(), json!(key));
    if ok {
        return ProbeOutcome::new(ProbeStatus::Pass, vec![row]);
    }
    let status = worst(&[classify_miss(&doc.response, &expect, w.timeout.is_some())]);
    ProbeOutcome::new(status, vec![row])
}

/// One legacy alias: a path, or a path with request headers.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum AliasSpec {
    /// A bare path.
    Path(String),
    /// A path with request headers.
    Full {
        /// The alias path.
        path: String,
        /// Request headers.
        #[serde(default)]
        headers: Option<Map<String, Value>>,
    },
}

/// The `with` block of the `legacy-alias-redirects` rule.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct AliasWith {
    /// The canonical path the aliases should redirect to; never probed.
    pub canonical: String,
    /// The legacy paths to probe without following redirects.
    pub aliases: Option<Vec<AliasSpec>>,
    /// Per-request timeout in seconds.
    pub timeout: Option<f64>,
}

/// Whether the legacy MCP card paths point at the canonical card instead of
/// serving their own copy. One correct redirect passes; otherwise the worst
/// observed defect decides: a redirect elsewhere is broken, an inline copy
/// is noncompliant, nothing published is absent.
pub fn run_legacy_alias_redirects(check: &WebCheck, ctx: &HandlerContext) -> ProbeOutcome {
    let w: AliasWith = serde_json::from_str(check.with_json).unwrap_or_default();
    let opts = FetchOptions {
        follow_redirects: false,
        ..ctx.fetch_options(w.timeout)
    };
    let canonical_url = resolve_url(&ctx.base, &w.canonical);
    if canonical_url.is_empty() {
        return ProbeOutcome::na("no resolvable canonical URL");
    }

    let mut evidence = Vec::new();
    let mut redirected = false;
    let mut misdirected = false;
    let mut inline_copy = false;
    for alias in w.aliases.unwrap_or_default() {
        let (path, headers) = match alias {
            AliasSpec::Path(path) => (path, None),
            AliasSpec::Full { path, headers } => (path, headers),
        };
        let alias_url = resolve_url(
            &ctx.base,
            &substitute_endpoint(&path, ctx.mcp_endpoint.as_deref()),
        );
        if alias_url.is_empty() {
            continue;
        }
        let resp = ctx.fetch.fetch(
            &alias_url,
            &FetchInit {
                headers: header_pairs(headers.as_ref()),
                ..FetchInit::default()
            },
            &opts,
        );
        let (verdict, note) = classify_alias_probe(&resp, &alias_url, &canonical_url);
        evidence.push(item([
            ("url", json!(alias_url)),
            ("role", json!("alias")),
            ("status", json!(resp.status)),
            ("alias_verdict", json!(verdict.as_str())),
            ("why", json!([note])),
        ]));
        match verdict {
            AliasVerdict::Pass => redirected = true,
            AliasVerdict::Broken => {
                // A 2xx alias answered with a body of its own; every other
                // verdict in the broken bucket is a redirect that misses
                // the canonical card.
                if (200..300).contains(&resp.status.unwrap_or(0)) {
                    inline_copy = true;
                } else {
                    misdirected = true;
                }
            }
            AliasVerdict::NotApplicable => {}
        }
    }

    if evidence.is_empty() {
        return ProbeOutcome::na("no resolvable alias URL");
    }
    let status = if redirected {
        ProbeStatus::Pass
    } else if misdirected {
        ProbeStatus::Broken
    } else if inline_copy {
        ProbeStatus::Noncompliant
    } else {
        ProbeStatus::Absent
    };
    ProbeOutcome::new(status, evidence)
}
