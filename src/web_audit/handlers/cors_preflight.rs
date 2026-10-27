//! The CORS posture pair (`mcp-cors-preflight` / `mcp-cors-actual`), a
//! mirror of the site's `handlers/cors-preflight.ts`. Each check issues
//! both probes, the OPTIONS preflight and an Origin-bearing JSON-RPC POST,
//! and classifies its own surface from the pair, so the two checks share
//! no engine state. A consistent no-CORS posture is n_a with the
//! `posture-consistent` reason; only partial or misconfigured CORS scores
//! broken.

use std::thread;

use serde::Deserialize;
use serde_json::json;

use super::shared::{
    endpoint_redirects, item, resolve_url, retry_shaped_answer, retry_shaped_why,
    substitute_endpoint,
};
use crate::web_audit::engine::mcp_wire::{legacy_probe_headers, legacy_tools_list_body};
use crate::web_audit::engine::{HandlerContext, ProbeOutcome, ProbeStatus};
use crate::web_audit::fetch::body::STATUS_ONLY_BODY_BYTES;
use crate::web_audit::fetch::{FetchInit, FetchOptions, ProbeResponse};
use crate::web_audit::registry::WebCheck;
use crate::web_audit::scorecard::NaReason;

/// The `with` block of a CORS check.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct CorsWith {
    /// The path to probe, usually `{mcp_endpoint}`.
    pub path: String,
    /// Which of the pair this check classifies: `preflight` or `actual`.
    pub surface: String,
    /// The Origin to send.
    pub origin: Option<String>,
    /// The preflight's requested method.
    pub request_method: Option<String>,
    /// The preflight's requested headers.
    pub request_headers: Option<String>,
    /// Per-request timeout in seconds.
    pub timeout: Option<f64>,
}

const POSTURE_WHY: &str =
    "no Allow-Origin on the preflight or the POST: consistent no-CORS posture";

fn is_2xx(status: Option<u16>) -> bool {
    status.is_some_and(|s| (200..300).contains(&s))
}

fn header(resp: &ProbeResponse, name: &str) -> Option<String> {
    resp.headers.get(name).map(str::to_string)
}

/// Probe the preflight and the POST, then classify this check's surface.
pub fn run_cors_preflight(check: &WebCheck, ctx: &HandlerContext) -> ProbeOutcome {
    let w: CorsWith = serde_json::from_str(check.with_json).unwrap_or_default();
    let is_preflight = match w.surface.as_str() {
        "preflight" => true,
        "actual" => false,
        _ => {
            return ProbeOutcome::error(&format!(
                "cors-preflight: check \"{}\" needs with.surface \"preflight\" or \"actual\"",
                check.id
            ));
        }
    };
    let url = resolve_url(
        &ctx.base,
        &substitute_endpoint(&w.path, ctx.mcp_endpoint.as_deref()),
    );
    if url.is_empty() {
        return ProbeOutcome::na("no endpoint to preflight");
    }
    let origin = w
        .origin
        .clone()
        .unwrap_or_else(|| "https://example.com".to_string());
    let opts = endpoint_redirects(
        FetchOptions {
            max_body_bytes: Some(STATUS_ONLY_BODY_BYTES),
            ..ctx.fetch_options(w.timeout)
        },
        &w.path,
        ctx.mcp_endpoint_followed,
        "POST",
    );
    let mut post_headers = legacy_probe_headers();
    post_headers.push(("Origin".to_string(), origin.clone()));
    if let Some(session) = ctx.mcp_session_id.as_deref().filter(|s| !s.is_empty()) {
        post_headers.push(("Mcp-Session-Id".to_string(), session.to_string()));
    }
    let preflight_init = FetchInit {
        method: "OPTIONS".to_string(),
        headers: vec![
            ("Origin".to_string(), origin.clone()),
            (
                "Access-Control-Request-Method".to_string(),
                w.request_method
                    .clone()
                    .unwrap_or_else(|| "POST".to_string()),
            ),
            (
                "Access-Control-Request-Headers".to_string(),
                w.request_headers
                    .clone()
                    .unwrap_or_else(|| "content-type".to_string()),
            ),
        ],
        body: None,
    };
    let post_init = FetchInit {
        method: "POST".to_string(),
        headers: post_headers,
        body: Some(legacy_tools_list_body().into_bytes()),
    };

    // Both verdicts read response headers only, and the pair goes out
    // together as it does on the site.
    let (pre, post) = thread::scope(|s| {
        let pre = s.spawn(|| ctx.fetch.fetch(&url, &preflight_init, &opts));
        let post = ctx.fetch.fetch(&url, &post_init, &opts);
        let pre = pre.join().unwrap_or_else(|_| {
            ProbeResponse::failed("Error: preflight probe panicked".to_string(), 0)
        });
        (pre, post)
    });

    let pre_acao = header(&pre, "access-control-allow-origin");
    let post_acao = header(&post, "access-control-allow-origin");
    let pre_row = item([
        ("probe", json!("preflight")),
        ("url", json!(url)),
        ("status", json!(pre.status)),
        ("allow_origin", json!(pre_acao)),
        (
            "allow_methods",
            json!(header(&pre, "access-control-allow-methods")),
        ),
        (
            "allow_headers",
            json!(header(&pre, "access-control-allow-headers")),
        ),
        ("error", json!(pre.error)),
    ]);
    let post_row = item([
        ("probe", json!("post")),
        ("url", json!(url)),
        ("status", json!(post.status)),
        ("allow_origin", json!(post_acao)),
        ("error", json!(post.error)),
    ]);
    // The classified surface's own probe row leads, so the generic n_a
    // evidence line (the first row's `why`) always describes this check.
    let mut evidence = if is_preflight {
        vec![pre_row, post_row]
    } else {
        vec![post_row, pre_row]
    };

    // A sibling probe that failed or was asked to retry cannot suppress a
    // check whose own probe answered. The sibling is still needed to read
    // the pair as a posture, so when its answer is unusable the surface's
    // own Allow-Origin classifies and a pair without one is an operational
    // unknown, not a declared opt-out. A busy answer's headers describe the
    // layer reporting the load, so they are never read as either surface's
    // posture.
    let (own, sibling) = if is_preflight {
        (&pre, &post)
    } else {
        (&post, &pre)
    };
    if own.error.is_some() {
        return ProbeOutcome::new(ProbeStatus::Error, evidence);
    }
    if let Some(busy) = retry_shaped_why(own.status) {
        evidence[0].insert("why".to_string(), json!([busy]));
        return ProbeOutcome::new(ProbeStatus::Error, evidence);
    }
    let sibling_unusable = match (&sibling.error, retry_shaped_answer(sibling.status)) {
        (Some(error), _) => Some(format!("failed ({error})")),
        (None, Some(busy)) => Some(format!("answered {busy}")),
        (None, None) => None,
    };
    let sibling_unknown = |label: &str, unusable: &str| {
        (
            ProbeStatus::Error,
            format!("the {label} probe {unusable}, so the no-CORS posture cannot be confirmed"),
        )
    };

    let (status, why) = if is_preflight {
        if pre_acao.is_some() {
            if is_2xx(pre.status) {
                (
                    ProbeStatus::Pass,
                    "preflight declares CORS with a 2xx".to_string(),
                )
            } else {
                (
                    ProbeStatus::Broken,
                    format!(
                        "Allow-Origin on a non-2xx preflight ({}): misconfigured",
                        pre.status.map_or("null".to_string(), |s| s.to_string())
                    ),
                )
            }
        } else if let Some(unusable) = &sibling_unusable {
            sibling_unknown("POST", unusable)
        } else if post_acao.is_some() {
            (
                ProbeStatus::Broken,
                "the POST carries Allow-Origin but the preflight does not: inconsistent posture"
                    .to_string(),
            )
        } else {
            (ProbeStatus::Na, POSTURE_WHY.to_string())
        }
    } else if post_acao.is_some() {
        (
            ProbeStatus::Pass,
            "the POST response carries Allow-Origin".to_string(),
        )
    } else if let Some(unusable) = &sibling_unusable {
        sibling_unknown("preflight", unusable)
    } else if pre_acao.is_some() {
        (
            ProbeStatus::Broken,
            "the preflight declares CORS but the POST omits Allow-Origin".to_string(),
        )
    } else {
        (ProbeStatus::Na, POSTURE_WHY.to_string())
    };

    evidence[0].insert("why".to_string(), json!([why]));
    ProbeOutcome {
        na_reason: (status == ProbeStatus::Na).then_some(NaReason::PostureConsistent),
        ..ProbeOutcome::new(status, evidence)
    }
}
