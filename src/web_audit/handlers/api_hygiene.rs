//! The `api-hygiene` handler, a mirror of the site's
//! `handlers/api-hygiene.ts`: JSON error bodies and rate-limit headers, at
//! the URL `api_probe_url` derives on a host. With no API anchor in the API
//! catalog, the one host is the audited origin and each row sends its own
//! GET. With anchors, each anchor host receives one GET, which both rows
//! read, and each row aggregates its hosts, so when every anchor is off the
//! audited origin no probe reaches it. A GET to an anchor host off the
//! audited origin takes no redirect to another origin.

use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

use super::api_probe_url::{ProbeUrl, derive_api_probe_url, derive_host_probe_url};
use super::shared::{JS_SPACE, item, plaintext_item, resolve_url, why};
use super::targets::{TargetOutcome, aggregate_targets};
use crate::web_audit::engine::api_targets::ApiHostTarget;
use crate::web_audit::engine::{HandlerContext, ProbeOutcome, ProbeStatus};
use crate::web_audit::fetch::body::{AUDIT_PROBE_MAX_BODY_BYTES, STATUS_ONLY_BODY_BYTES};
use crate::web_audit::fetch::redirect::{DEFAULT_MAX_REDIRECTS, is_redirect};
use crate::web_audit::fetch::{FetchInit, FetchOptions, ProbeResponse};
use crate::web_audit::registry::WebCheck;

const RATE_LIMIT_HEADERS: [&str; 8] = [
    "ratelimit-limit",
    "ratelimit-remaining",
    "ratelimit-reset",
    "ratelimit-policy",
    "x-ratelimit-limit",
    "x-ratelimit-remaining",
    "x-ratelimit-reset",
    "retry-after",
];

/// The `with` block of an `api-hygiene` check.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct ApiHygieneWith {
    /// `json-errors` or `rate-limit`; `json-errors` by default.
    pub op: Option<String>,
    /// Per-request timeout in seconds.
    pub timeout: Option<f64>,
}

/// The anchor-host GETs a run has sent, one per URL, shared by both rows.
#[derive(Debug, Default)]
pub struct HostProbes(Mutex<Vec<(String, Arc<OnceLock<ProbeResponse>>)>>);

impl HostProbes {
    /// The response for `url`, sending it only when no row has.
    fn get(&self, url: &str, send: impl FnOnce() -> ProbeResponse) -> ProbeResponse {
        let cell = {
            let mut sent = self.0.lock().unwrap_or_else(|e| e.into_inner());
            match sent.iter().find(|(sent_url, _)| sent_url == url) {
                Some((_, cell)) => Arc::clone(cell),
                None => {
                    let cell = Arc::new(OnceLock::new());
                    sent.push((url.to_string(), Arc::clone(&cell)));
                    cell
                }
            }
        };
        cell.get_or_init(send).clone()
    }
}

static HTML_CT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)html").unwrap());
static HTML_BODY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"(?i)^[{JS_SPACE}]*<(!doctype|html|head|body)\b")).unwrap()
});

fn is_client_error(status: u16) -> bool {
    (400..500).contains(&status)
}

fn is_html(resp: &ProbeResponse) -> bool {
    let ct = resp.headers.get("content-type").unwrap_or("");
    HTML_CT.is_match(ct) || HTML_BODY.is_match(&resp.body)
}

fn is_json_body(resp: &ProbeResponse) -> bool {
    if is_html(resp) {
        return false;
    }
    matches!(
        serde_json::from_str::<Value>(&resp.body),
        Ok(Value::Object(_)) | Ok(Value::Array(_))
    )
}

fn rate_limit_header(resp: &ProbeResponse) -> Option<&'static str> {
    RATE_LIMIT_HEADERS
        .into_iter()
        .find(|name| resp.headers.contains(name))
}

fn probe_once(
    rate_limit: bool,
    derived: &ProbeUrl,
    ctx: &HandlerContext,
    get: impl FnOnce(&str) -> ProbeResponse,
) -> TargetOutcome {
    let (url, source) = (&derived.url, derived.source);
    let outcome = |status, item| TargetOutcome {
        status,
        na_reason: None,
        evidence: vec![item],
    };
    if let Err(reason) = ctx.validate_nested(url) {
        let row = item([
            ("url", json!(url)),
            ("blocked", json!(reason)),
            ("source", json!(source)),
        ]);
        return outcome(ProbeStatus::Error, row);
    }
    let resp = get(url);
    let Some(status) = resp.status.filter(|_| resp.error.is_none()) else {
        let row = item([
            ("url", json!(url)),
            ("status", json!(resp.status)),
            ("error", json!(resp.error)),
            ("source", json!(source)),
        ]);
        return outcome(ProbeStatus::Error, row);
    };
    let verdict = |ok: bool, verdict, lines: &[&str]| {
        let row = item([
            ("url", json!(url)),
            ("status", json!(status)),
            ("ok", json!(ok)),
            ("source", json!(source)),
            ("why", why(lines)),
        ]);
        outcome(verdict, row)
    };

    if rate_limit {
        return match rate_limit_header(&resp) {
            Some(name) => verdict(
                true,
                ProbeStatus::Pass,
                &[&format!("rate-limit header {name}")],
            ),
            None => verdict(false, ProbeStatus::Absent, &["no rate-limit header"]),
        };
    }
    if is_client_error(status) && is_json_body(&resp) {
        return verdict(true, ProbeStatus::Pass, &["client-error JSON body"]);
    }
    let html = is_html(&resp);
    let shape = if html {
        "HTML error body"
    } else if is_json_body(&resp) {
        "JSON body but not a client error"
    } else {
        "non-JSON error body"
    };
    let miss = if status >= 500 || html || is_client_error(status) {
        ProbeStatus::Broken
    } else {
        ProbeStatus::Absent
    };
    verdict(false, miss, &[&format!("status {status}"), shape])
}

fn origin_of(url: &str) -> Option<String> {
    Url::parse(url)
        .ok()
        .map(|u| u.origin().ascii_serialization())
}

/// A GET whose redirect to another origin comes back as its answer.
fn same_origin_get(ctx: &HandlerContext, url: &str, opts: &FetchOptions) -> ProbeResponse {
    let origin = origin_of(url);
    let opts = FetchOptions {
        follow_redirects: false,
        ..opts.clone()
    };
    let mut current = url.to_string();
    for hop in 0..=DEFAULT_MAX_REDIRECTS {
        let resp = ctx.fetch.fetch(&current, &FetchInit::default(), &opts);
        let location = resp.headers.get("location").map(str::to_string);
        let (Some(status), Some(location)) = (resp.status, location) else {
            return resp;
        };
        let next = resolve_url(&current, &location);
        if !is_redirect(status) || origin_of(&next) != origin {
            return resp;
        }
        if hop == DEFAULT_MAX_REDIRECTS {
            break;
        }
        current = next;
    }
    ProbeResponse::failed(
        format!("redirect limit exceeded ({DEFAULT_MAX_REDIRECTS} hops)"),
        0,
    )
}

fn probe_host(
    rate_limit: bool,
    target: &ApiHostTarget,
    opts: &FetchOptions,
    ctx: &HandlerContext,
) -> TargetOutcome {
    if let Some(unmet) = &target.unmet {
        return TargetOutcome {
            status: ProbeStatus::Na,
            na_reason: Some(unmet.reason),
            evidence: vec![item([
                ("host", json!(unmet.host)),
                ("why", json!([unmet.url])),
            ])],
        };
    }
    if let Some(reason) = target.plaintext {
        let mut row = item([("url", json!(target.anchors.first().map(|a| &a.url)))]);
        row.extend(plaintext_item(reason));
        return TargetOutcome {
            status: ProbeStatus::Absent,
            na_reason: None,
            evidence: vec![row],
        };
    }
    let allowed = |url: &str| ctx.validate_nested(url).is_ok();
    let derived = derive_host_probe_url(target, &ctx.api_description_bodies, &allowed);
    probe_once(rate_limit, &derived, ctx, |url| {
        ctx.api_host_probes.get(url, || {
            if target.declared {
                same_origin_get(ctx, url, opts)
            } else {
                ctx.fetch.fetch(url, &FetchInit::default(), opts)
            }
        })
    })
}

/// Probe the derived API URL, or each API anchor host, for the op's signal.
pub fn run_api_hygiene(check: &WebCheck, ctx: &HandlerContext) -> ProbeOutcome {
    let w: ApiHygieneWith = serde_json::from_str(check.with_json).unwrap_or_default();
    let rate_limit = w.op.as_deref() == Some("rate-limit");
    let Some(targets) = ctx.api_targets.as_deref() else {
        let openapi = ctx
            .retained_bodies
            .get("openapi")
            .map_or("", String::as_str);
        let allowed = |url: &str| ctx.validate_nested(url).is_ok();
        let derived = derive_api_probe_url(openapi, &ctx.base, &allowed);
        let opts = FetchOptions {
            max_body_bytes: Some(if rate_limit {
                STATUS_ONLY_BODY_BYTES
            } else {
                AUDIT_PROBE_MAX_BODY_BYTES
            }),
            ..ctx.fetch_options(w.timeout)
        };
        let get = |url: &str| ctx.fetch.fetch(url, &FetchInit::default(), &opts);
        let one = probe_once(rate_limit, &derived, ctx, get);
        return ProbeOutcome::new(one.status, one.evidence);
    };
    if targets.hosts.is_empty() {
        return ProbeOutcome::na("no API anchor host the audit can probe");
    }
    let opts = FetchOptions {
        max_body_bytes: Some(AUDIT_PROBE_MAX_BODY_BYTES),
        ..ctx.fetch_options(w.timeout)
    };
    let outcomes: Vec<TargetOutcome> = std::thread::scope(|scope| {
        let handles: Vec<_> = targets
            .hosts
            .iter()
            .map(|t| scope.spawn(|| probe_host(rate_limit, t, &opts, ctx)))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("a host probe does not panic"))
            .collect()
    });
    aggregate_targets(outcomes)
}
