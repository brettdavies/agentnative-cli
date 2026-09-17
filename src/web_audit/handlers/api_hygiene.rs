//! The `api-hygiene` handler, a mirror of the site's
//! `handlers/api-hygiene.ts`: JSON error bodies and rate-limit headers.
//! Derives one non-mutating GET URL from the retained wave-1 OpenAPI body
//! (a documented 4xx example, else the first safe GET) and falls back to a
//! well-known nonsense path when the body is missing or unusable.

use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use url::Url;

use super::shared::{JS_SPACE, item, why};
use crate::web_audit::engine::{HandlerContext, ProbeOutcome, ProbeStatus};
use crate::web_audit::fetch::body::{AUDIT_PROBE_MAX_BODY_BYTES, STATUS_ONLY_BODY_BYTES};
use crate::web_audit::fetch::{FetchInit, FetchOptions, ProbeResponse};
use crate::web_audit::registry::WebCheck;

/// The path probed when no OpenAPI operation can be derived.
pub const API_HYGIENE_FALLBACK_PATH: &str = "/anc-web-audit-no-such-api";

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

static PATH_PARAM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{[^}]+\}").unwrap());
static CLIENT_ERROR_CODE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(4\d\d|4XX)$").unwrap());
static HTML_CT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)html").unwrap());
static HTML_BODY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"(?i)^[{JS_SPACE}]*<(!doctype|html|head|body)\b")).unwrap()
});

fn is_client_error(status: u16) -> bool {
    (400..500).contains(&status)
}

fn fill_path(path: &str) -> String {
    PATH_PARAM
        .replace_all(path, "anc-web-audit-no-such")
        .into_owned()
}

struct OpenApiOp<'a> {
    path: &'a str,
    responses: Vec<&'a str>,
}

fn operations_from(spec: &Map<String, Value>) -> Vec<OpenApiOp<'_>> {
    let Some(paths) = spec.get("paths").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (path, entry) in paths {
        let Some(methods) = entry.as_object().filter(|_| path.starts_with('/')) else {
            continue;
        };
        for (method, op) in methods {
            if !matches!(method.to_lowercase().as_str(), "get" | "head") {
                continue;
            }
            let responses = op
                .as_object()
                .and_then(|o| o.get("responses"))
                .and_then(Value::as_object)
                .map(|r| r.keys().map(String::as_str).collect())
                .unwrap_or_default();
            out.push(OpenApiOp { path, responses });
        }
    }
    out
}

fn resolve_same_origin(base: &str, path: &str) -> Option<String> {
    Url::parse(base)
        .ok()?
        .join(&fill_path(path))
        .ok()
        .map(|u| u.to_string())
}

/// The single GET URL both hygiene checks share, and where it came from.
pub fn derive_api_probe_url(openapi_body: &str, ctx: &HandlerContext) -> (String, &'static str) {
    let fallback = || {
        (
            resolve_same_origin(&ctx.base, API_HYGIENE_FALLBACK_PATH).unwrap_or_default(),
            "fallback",
        )
    };
    if openapi_body.is_empty() {
        return fallback();
    }
    let Ok(Value::Object(spec)) = serde_json::from_str::<Value>(openapi_body) else {
        return fallback();
    };
    let ops = operations_from(&spec);
    let documented_4xx = ops.iter().find(|op| {
        op.responses
            .iter()
            .any(|code| CLIENT_ERROR_CODE.is_match(code))
    });
    let Some(chosen) = documented_4xx.or(ops.first()) else {
        return fallback();
    };
    let Some(url) = resolve_same_origin(&ctx.base, chosen.path) else {
        return fallback();
    };
    if ctx.validate_nested(&url).is_err() {
        return fallback();
    }
    let same_origin = match (Url::parse(&url), Url::parse(&ctx.base)) {
        (Ok(u), Ok(b)) => u.origin() == b.origin(),
        _ => false,
    };
    if !same_origin {
        return fallback();
    }
    (
        url,
        if documented_4xx.is_some() {
            "openapi-4xx"
        } else {
            "openapi-get"
        },
    )
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

/// Probe the derived API URL for the op's hygiene signal.
pub fn run_api_hygiene(check: &WebCheck, ctx: &HandlerContext) -> ProbeOutcome {
    let w: ApiHygieneWith = serde_json::from_str(check.with_json).unwrap_or_default();
    let op = w.op.as_deref().unwrap_or("json-errors");
    let rate_limit = op == "rate-limit";
    let openapi = ctx
        .retained_bodies
        .get("openapi")
        .map(String::as_str)
        .unwrap_or("");
    let (url, source) = derive_api_probe_url(openapi, ctx);
    if let Err(reason) = ctx.validate_nested(&url) {
        return ProbeOutcome::new(
            ProbeStatus::Error,
            vec![item([
                ("url", json!(url)),
                ("blocked", json!(reason)),
                ("source", json!(source)),
            ])],
        );
    }

    let resp = ctx.fetch.fetch(
        &url,
        &FetchInit::default(),
        &FetchOptions {
            max_body_bytes: Some(if rate_limit {
                STATUS_ONLY_BODY_BYTES
            } else {
                AUDIT_PROBE_MAX_BODY_BYTES
            }),
            ..ctx.fetch_options(w.timeout)
        },
    );
    let Some(status) = resp.status.filter(|_| resp.error.is_none()) else {
        return ProbeOutcome::new(
            ProbeStatus::Error,
            vec![item([
                ("url", json!(url)),
                ("status", json!(resp.status)),
                ("error", json!(resp.error)),
                ("source", json!(source)),
            ])],
        );
    };

    if rate_limit {
        let header = rate_limit_header(&resp);
        let line = match header {
            Some(name) => format!("rate-limit header {name}"),
            None => "no rate-limit header".to_string(),
        };
        return ProbeOutcome::new(
            if header.is_some() {
                ProbeStatus::Pass
            } else {
                ProbeStatus::Absent
            },
            vec![item([
                ("url", json!(url)),
                ("status", json!(status)),
                ("ok", json!(header.is_some())),
                ("source", json!(source)),
                ("why", why(&[&line])),
            ])],
        );
    }

    if is_client_error(status) && is_json_body(&resp) {
        return ProbeOutcome::new(
            ProbeStatus::Pass,
            vec![item([
                ("url", json!(url)),
                ("status", json!(status)),
                ("ok", json!(true)),
                ("source", json!(source)),
                ("why", why(&["client-error JSON body"])),
            ])],
        );
    }

    let html = is_html(&resp);
    let shape = if html {
        "HTML error body"
    } else if is_json_body(&resp) {
        "JSON body but not a client error"
    } else {
        "non-JSON error body"
    };
    let status_line = format!("status {status}");
    let verdict = if status >= 500 || html || is_client_error(status) {
        ProbeStatus::Broken
    } else {
        ProbeStatus::Absent
    };
    ProbeOutcome::new(
        verdict,
        vec![item([
            ("url", json!(url)),
            ("status", json!(status)),
            ("ok", json!(false)),
            ("source", json!(source)),
            ("why", why(&[&status_line, shape])),
        ])],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_parameters_are_filled_and_client_error_codes_recognized() {
        assert_eq!(
            fill_path("/users/{id}/posts/{postId}"),
            "/users/anc-web-audit-no-such/posts/anc-web-audit-no-such"
        );
        assert!(CLIENT_ERROR_CODE.is_match("404"));
        assert!(CLIENT_ERROR_CODE.is_match("4xx"));
        assert!(!CLIENT_ERROR_CODE.is_match("500"));
    }

    #[test]
    fn operations_keep_spec_order_and_safe_methods_only() {
        let spec: Value = serde_json::json!({
            "paths": {
                "/b": { "post": { "responses": { "404": {} } }, "get": { "responses": { "200": {} } } },
                "/a": { "head": { "responses": { "4XX": {} } } },
                "relative": { "get": {} }
            }
        });
        let ops = operations_from(spec.as_object().unwrap());
        let seen: Vec<(&str, Vec<&str>)> =
            ops.iter().map(|o| (o.path, o.responses.clone())).collect();
        assert_eq!(seen, [("/b", vec!["200"]), ("/a", vec!["4XX"])]);
    }
}
