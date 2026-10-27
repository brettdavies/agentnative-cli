//! The URL the API hygiene probes request on one host, a mirror of the
//! site's `handlers/api-probe-url.ts`: derived from an OpenAPI description
//! (a documented 4xx GET, else the first safe GET) when one parses as JSON,
//! else a well-known nonsense path. Either path is appended to a base path,
//! as OpenAPI appends a path to its server URL: on an API anchor host, the
//! description's first server when that is on the anchor's origin, else the
//! anchor's own path. Pure; the caller supplies the guard's verdict.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};
use url::Url;

use crate::web_audit::engine::api_targets::ApiHostTarget;
use crate::web_audit::engine::discovery_documents::is_templated_url;

/// The path probed when no OpenAPI operation can be derived.
pub const API_HYGIENE_FALLBACK_PATH: &str = "/anc-web-audit-no-such-api";

static PATH_PARAM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{[^}]+\}").unwrap());
static CLIENT_ERROR_CODE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(4\d\d|4XX)$").unwrap());

/// The derived GET and where it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeUrl {
    /// The URL.
    pub url: String,
    /// `openapi-4xx`, `openapi-get` or `fallback`.
    pub source: &'static str,
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

/// `path` under `base`'s path; a path naming another authority leaves the
/// origin, which the caller refuses.
fn under_base(base: &str, path: &str) -> String {
    let Ok(parsed) = Url::parse(base) else {
        return String::new();
    };
    let prefix = parsed.path().trim_end_matches('/');
    parsed
        .join(&format!("{prefix}{}", fill_path(path)))
        .map(|u| u.to_string())
        .unwrap_or_default()
}

fn parse_spec(body: &str) -> Option<Map<String, Value>> {
    match serde_json::from_str::<Value>(body) {
        Ok(Value::Object(map)) => Some(map),
        _ => None,
    }
}

fn probe_under(
    spec: Option<&Map<String, Value>>,
    base: &str,
    allowed: &dyn Fn(&str) -> bool,
) -> ProbeUrl {
    let fallback = || ProbeUrl {
        url: under_base(base, API_HYGIENE_FALLBACK_PATH),
        source: "fallback",
    };
    let Some(spec) = spec else {
        return fallback();
    };
    let ops = operations_from(spec);
    let documented_4xx = ops.iter().find(|op| {
        op.responses
            .iter()
            .any(|code| CLIENT_ERROR_CODE.is_match(code))
    });
    let Some(chosen) = documented_4xx.or(ops.first()) else {
        return fallback();
    };
    let url = under_base(base, chosen.path);
    let same_origin = match (Url::parse(&url), Url::parse(base)) {
        (Ok(u), Ok(b)) => u.origin() == b.origin(),
        _ => false,
    };
    if !allowed(&url) || !same_origin {
        return fallback();
    }
    ProbeUrl {
        url,
        source: if documented_4xx.is_some() {
            "openapi-4xx"
        } else {
            "openapi-get"
        },
    }
}

/// The single GET URL both hygiene checks share on the audited origin.
pub fn derive_api_probe_url(
    openapi_body: &str,
    base: &str,
    allowed: &dyn Fn(&str) -> bool,
) -> ProbeUrl {
    probe_under(parse_spec(openapi_body).as_ref(), base, allowed)
}

/// The description's first server URL, resolved against where it is
/// served, when it is a plain URL on `origin`.
fn server_base(
    spec: Option<&Map<String, Value>>,
    description: &str,
    origin: &str,
) -> Option<String> {
    let url = spec?
        .get("servers")?
        .as_array()?
        .first()?
        .get("url")?
        .as_str()?;
    if is_templated_url(url) {
        return None;
    }
    let resolved = Url::parse(description).ok()?.join(url).ok()?;
    (resolved.origin().ascii_serialization() == origin).then(|| resolved.to_string())
}

/// The first probe URL an anchor's description on the host yields, else
/// the nonsense path under the first anchor's base.
pub fn derive_host_probe_url(
    target: &ApiHostTarget,
    bodies: &HashMap<String, String>,
    allowed: &dyn Fn(&str) -> bool,
) -> ProbeUrl {
    let mut fallback: Option<ProbeUrl> = None;
    for anchor in &target.anchors {
        let spec = bodies.get(&anchor.description).and_then(|b| parse_spec(b));
        let anchor_base = if is_templated_url(&anchor.url) {
            format!("{}/", target.origin)
        } else {
            anchor.url.clone()
        };
        let base =
            server_base(spec.as_ref(), &anchor.description, &target.origin).unwrap_or(anchor_base);
        let derived = probe_under(spec.as_ref(), &base, allowed);
        if derived.source != "fallback" {
            return derived;
        }
        fallback.get_or_insert(derived);
    }
    fallback.unwrap_or_else(|| derive_api_probe_url("", &format!("{}/", target.origin), allowed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web_audit::engine::api_targets::HostAnchor;

    fn allow(_: &str) -> bool {
        true
    }

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

    #[test]
    fn a_host_probe_goes_under_the_first_server_on_the_anchor_origin_else_the_anchor_path() {
        let spec = r#"{"servers":[{"url":"/v2"}],"paths":{"/items/{id}":{"get":{"responses":{"404":{}}}}}}"#;
        let off = r#"{"servers":[{"url":"https://elsewhere.test/v9"}],"paths":{"/x":{"get":{}}}}"#;
        let bodies = HashMap::from([
            (
                "https://api.test/openapi.json".to_string(),
                spec.to_string(),
            ),
            ("https://api.test/off.json".to_string(), off.to_string()),
        ]);
        let host = |anchors: &[(&str, &str)]| ApiHostTarget {
            origin: "https://api.test".to_string(),
            anchors: anchors
                .iter()
                .map(|(url, description)| HostAnchor {
                    url: url.to_string(),
                    description: description.to_string(),
                })
                .collect(),
            declared: true,
            unmet: None,
            plaintext: None,
        };
        assert_eq!(
            derive_host_probe_url(
                &host(&[("https://api.test/", "https://api.test/openapi.json")]),
                &bodies,
                &allow
            ),
            ProbeUrl {
                url: "https://api.test/v2/items/anc-web-audit-no-such".into(),
                source: "openapi-4xx"
            }
        );
        assert_eq!(
            derive_host_probe_url(
                &host(&[("https://api.test/base/", "https://api.test/off.json")]),
                &bodies,
                &allow
            )
            .url,
            "https://api.test/base/x"
        );
        assert_eq!(
            derive_host_probe_url(
                &host(&[("https://api.test/v1", "https://api.test/none.yaml")]),
                &bodies,
                &allow
            ),
            ProbeUrl {
                url: "https://api.test/v1/anc-web-audit-no-such-api".into(),
                source: "fallback"
            }
        );
    }

    #[test]
    fn a_path_naming_another_authority_or_a_refused_url_falls_back() {
        let spec = r#"{"paths":{"//evil.test/x":{"get":{}}}}"#;
        assert_eq!(
            derive_api_probe_url(spec, "https://example.com/", &allow).source,
            "fallback"
        );
        let ok = r#"{"paths":{"/x":{"get":{}}}}"#;
        assert_eq!(
            derive_api_probe_url(ok, "https://example.com/", &|_| false),
            ProbeUrl {
                url: "https://example.com/anc-web-audit-no-such-api".into(),
                source: "fallback"
            }
        );
    }
}
