//! The `api-description` eval rule, the OpenAPI row, a mirror of the site's
//! `handlers/api-description.ts`. With no API anchor in the API catalog,
//! the row probes the audited origin's well-known paths as an `http` check
//! does. With API anchors, it scores the description each anchor declares
//! instead, wherever it is hosted: one the follow read off the audited
//! origin, or one on the audited origin, read here. Presence is the check's
//! own `expect`, so a YAML description passes as a JSON one does.
//!
//! A description cut at `OPENAPI_MAX_BODY_BYTES` can hold its marker past
//! the bytes read (Stripe's `spec3.json` puts `openapi` after its
//! components), so a cut body shaped like a description that meets every
//! expectation but the marker still counts as present, its evidence marked
//! truncated. Evidence from a host other than the audited origin is marked
//! `off_origin`, so the antecedents that read this row as the site's own
//! OpenAPI leave it out.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::json;

use super::http::{HttpWith, assess_response, classify_miss, run_http};
use super::shared::{JS_SPACE, item, plaintext_item, worst};
use super::targets::{TargetOutcome, aggregate_targets};
use crate::web_audit::engine::api_targets::{ApiDescriptionTarget, DescriptionReach};
use crate::web_audit::engine::discovery_documents::same_origin;
use crate::web_audit::engine::{HandlerContext, ProbeOutcome, ProbeStatus};
use crate::web_audit::fetch::body::OPENAPI_MAX_BODY_BYTES;
use crate::web_audit::fetch::{FetchInit, FetchOptions, ProbeResponse};
use crate::web_audit::pattern::fixed;
use crate::web_audit::registry::WebCheck;
use crate::web_audit::scorecard::EvidenceItem;

static DESCRIPTION_TYPE_RE: LazyLock<Regex> =
    LazyLock::new(|| fixed(r"(?i)json|yaml|vnd\.oai\.openapi"));
static HTML_TYPE_RE: LazyLock<Regex> = LazyLock::new(|| fixed(r"(?i)html"));
static MARKUP_START_RE: LazyLock<Regex> = LazyLock::new(|| fixed(&format!(r"^[{JS_SPACE}]*<")));
static JSON_OBJECT_START_RE: LazyLock<Regex> =
    LazyLock::new(|| fixed(&format!(r"^[{JS_SPACE}]*\{{")));
// Service descriptions that are not OpenAPI, named in their opening bytes.
static OTHER_FORMAT_RE: LazyLock<Regex> = LazyLock::new(|| {
    fixed(&format!(
        r#"(?m)^[{JS_SPACE}]*asyncapi[{JS_SPACE}]*:|"asyncapi"[{JS_SPACE}]*:|"__schema"[{JS_SPACE}]*:|"_postman_id"[{JS_SPACE}]*:|^#%RAML"#
    ))
});

fn read_in_part_why() -> String {
    format!(
        "description larger than {} KiB; read in part, presence counted",
        OPENAPI_MAX_BODY_BYTES / 1024
    )
}

/// A JSON or YAML document by its content type, or a body that opens a
/// JSON object; never an HTML page or a description in another format.
fn description_shaped(resp: &ProbeResponse) -> bool {
    let content_type = resp.headers.get("content-type").unwrap_or("");
    if HTML_TYPE_RE.is_match(content_type) || MARKUP_START_RE.is_match(&resp.body) {
        return false;
    }
    if OTHER_FORMAT_RE.is_match(&resp.body) {
        return false;
    }
    DESCRIPTION_TYPE_RE.is_match(content_type) || JSON_OBJECT_START_RE.is_match(&resp.body)
}

fn assess_description(
    check: &WebCheck,
    url: &str,
    resp: &ProbeResponse,
    w: &HttpWith,
    base: &str,
) -> (bool, EvidenceItem) {
    let expect = w.expect.clone().unwrap_or_default();
    let whole = assess_response(check, url, resp, w, &expect, base);
    if whole.0 || !resp.truncated || !description_shaped(resp) {
        return whole;
    }
    let unmarked = super::assert::ExpectBlock {
        body_regex: None,
        ..expect
    };
    let (ok, mut row) = assess_response(check, url, resp, w, &unmarked, base);
    if !ok {
        return whole;
    }
    if let Some(why) = row.get_mut("why").and_then(|v| v.as_array_mut()) {
        why.push(json!(read_in_part_why()));
    }
    (true, row)
}

fn describe(
    check: &WebCheck,
    target: &ApiDescriptionTarget,
    w: &HttpWith,
    ctx: &HandlerContext,
) -> TargetOutcome {
    let fetched;
    let (url, resp) = match &target.reach {
        DescriptionReach::Unmet(unmet) => {
            return TargetOutcome {
                status: ProbeStatus::Na,
                na_reason: Some(unmet.reason),
                evidence: vec![item([
                    ("host", json!(unmet.host)),
                    ("description", json!(target.url)),
                    ("why", json!([unmet.url])),
                    ("off_origin", json!(true)),
                ])],
            };
        }
        DescriptionReach::Plaintext(reason) => {
            let mut row = item([
                ("url", json!(target.url)),
                ("description", json!(target.url)),
                ("off_origin", json!(true)),
            ]);
            row.extend(plaintext_item(*reason));
            return TargetOutcome {
                status: ProbeStatus::Absent,
                na_reason: None,
                evidence: vec![row],
            };
        }
        DescriptionReach::Fetched(read) => (read.url.as_str(), &read.response),
        DescriptionReach::OnOrigin => {
            let opts = FetchOptions {
                max_body_bytes: Some(OPENAPI_MAX_BODY_BYTES),
                ..ctx.fetch_options(w.timeout)
            };
            fetched = ctx.fetch.fetch(&target.url, &FetchInit::default(), &opts);
            (target.url.as_str(), &fetched)
        }
    };
    let (ok, mut row) = assess_description(check, url, resp, w, &ctx.base);
    row.insert("description".into(), json!(target.url));
    if !same_origin(url, &ctx.base) {
        row.insert("off_origin".into(), json!(true));
    }
    let expect = w.expect.clone().unwrap_or_default();
    let status = if ok {
        ProbeStatus::Pass
    } else {
        worst(&[classify_miss(resp, &expect, w.timeout.is_some())])
    };
    TargetOutcome {
        status,
        na_reason: None,
        evidence: vec![row],
    }
}

/// Score the descriptions the catalog declares, or the audited origin's
/// well-known paths when it declares none.
pub fn run_api_description(check: &WebCheck, ctx: &HandlerContext) -> ProbeOutcome {
    let Some(targets) = ctx.api_targets.as_deref() else {
        return run_http(check, ctx);
    };
    if targets.descriptions.is_empty() {
        return ProbeOutcome::na("no OpenAPI description the audit can request");
    }
    let w: HttpWith = serde_json::from_str(check.with_json).unwrap_or_default();
    aggregate_targets(
        targets
            .descriptions
            .iter()
            .map(|target| describe(check, target, &w, ctx))
            .collect(),
    )
}

/// The descriptions the OpenAPI row retained, by the URL the catalog
/// declared each at.
pub fn api_description_bodies(evidence: &[EvidenceItem]) -> HashMap<String, String> {
    evidence
        .iter()
        .filter_map(|item| {
            let description = item.get("description")?.as_str()?;
            let body = item.get("body")?.as_str()?;
            Some((description.to_string(), body.to_string()))
        })
        .collect()
}
