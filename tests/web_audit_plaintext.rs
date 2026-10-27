//! The plaintext evidence each handler records: an `http` llms.txt link is
//! never requested and reads noncompliant, a link that redirects to http
//! reads the same, and a document that redirects to http reads absent, each
//! with the site's evidence item. The corpus pins the rows these produce;
//! these pin which requests went out and the items themselves.

#![cfg(feature = "web")]

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use agentnative::web_audit::engine::discovery_documents::{API_CATALOG_DOCUMENT, RetainedDocument};
use agentnative::web_audit::engine::{HandlerContext, ProbeOutcome, ProbeStatus};
use agentnative::web_audit::fetch::{ProbeResponse, Refusal};
use agentnative::web_audit::handlers::auth_md::run_auth_md;
use agentnative::web_audit::handlers::http::{run_http, run_retained_document};
use agentnative::web_audit::handlers::llms_txt_quality::run_llms_txt_quality;
use agentnative::web_audit::handlers::markdown_frontmatter::run_markdown_frontmatter;
use agentnative::web_audit::handlers::scoped_llms::run_scoped_llms;
use agentnative::web_audit::mock::{MockResponse, MockRule, MockTransport};
use common::handler_context::{check, context};
use serde_json::{Value, json};

const REDIRECTS_TO_HTTP: &str = "redirects to http; anc sends no plaintext request";

fn ok() -> MockResponse {
    MockResponse::ok(200, &[("content-type", "text/markdown")], b"# Doc\n")
}

fn missing() -> MockResponse {
    MockResponse::ok(404, &[], b"")
}

fn to_http(url: &str) -> MockResponse {
    let location = url.replacen("https://", "http://", 1);
    MockResponse::ok(301, &[("location", location.as_str())], b"moved")
}

fn get(url: &str, response: MockResponse) -> MockRule {
    MockRule::new("GET", url).response(response)
}

fn with_llms(transport: &Arc<MockTransport>, body: &str) -> HandlerContext {
    let mut ctx = context(transport.clone());
    ctx.retained_bodies = Arc::new(HashMap::from([("llms-txt".to_string(), body.to_string())]));
    ctx
}

fn requested(transport: &MockTransport) -> Vec<String> {
    transport.requests().into_iter().map(|r| r.url).collect()
}

fn item_for<'a>(outcome: &'a ProbeOutcome, url: &str) -> &'a serde_json::Map<String, Value> {
    outcome
        .evidence
        .iter()
        .find(|e| e.get("url") == Some(&json!(url)))
        .unwrap_or_else(|| panic!("no evidence item for {url}: {:?}", outcome.evidence))
}

#[test]
fn an_http_llms_link_is_never_requested_and_reads_noncompliant() {
    let transport = Arc::new(MockTransport::new(
        vec![get("https://example.com/docs/guide.md", ok())],
        ok(),
    ));
    let ctx = with_llms(
        &transport,
        "# X\n- [Guide](/docs/guide.md)\n- [Plain](http://example.com/docs/plain.md)\n",
    );
    let outcome = run_llms_txt_quality(check("llms-txt-links"), &ctx);
    assert_eq!(outcome.status, ProbeStatus::Noncompliant);
    assert_eq!(requested(&transport), ["https://example.com/docs/guide.md"]);
    let plain = item_for(&outcome, "http://example.com/docs/plain.md");
    assert_eq!(
        Value::Object(plain.clone()),
        json!({
            "url": "http://example.com/docs/plain.md",
            "blocked": "not https",
            "ok": false,
            "why": ["not https; anc sends no plaintext request"],
            "link_verdict": "noncompliant",
        })
    );
    let guide = item_for(&outcome, "https://example.com/docs/guide.md");
    assert_eq!(guide.get("link_verdict"), Some(&json!("pass")));
}

#[test]
fn an_llms_link_that_redirects_to_http_reads_noncompliant() {
    let url = "https://example.com/docs/moved.md";
    let transport = Arc::new(MockTransport::new(vec![get(url, to_http(url))], ok()));
    let ctx = with_llms(&transport, "# X\n- [Moved](/docs/moved.md)\n");
    let outcome = run_llms_txt_quality(check("llms-txt-links"), &ctx);
    assert_eq!(outcome.status, ProbeStatus::Noncompliant);
    assert_eq!(requested(&transport), [url]);
    assert_eq!(
        Value::Object(item_for(&outcome, url).clone()),
        json!({
            "url": url,
            "status": 301,
            "ok": false,
            "why": [REDIRECTS_TO_HTTP],
            "link_verdict": "noncompliant",
        })
    );
}

#[test]
fn a_dead_llms_link_outranks_an_http_one_and_an_http_one_outranks_an_error() {
    let transport = Arc::new(MockTransport::new(
        vec![
            get("https://example.com/docs/api.md", missing()),
            get(
                "https://example.com/docs/down.md",
                MockResponse::error("connection refused"),
            ),
        ],
        ok(),
    ));
    let dead = with_llms(
        &transport,
        "# X\n- [Plain](http://example.com/p.md)\n- [API](/docs/api.md)\n",
    );
    assert_eq!(
        run_llms_txt_quality(check("llms-txt-links"), &dead).status,
        ProbeStatus::Absent
    );
    let down = with_llms(
        &transport,
        "# X\n- [Down](/docs/down.md)\n- [Plain](http://example.com/p.md)\n",
    );
    assert_eq!(
        run_llms_txt_quality(check("llms-txt-links"), &down).status,
        ProbeStatus::Noncompliant
    );
}

#[test]
fn a_document_that_redirects_to_http_reads_absent_on_an_http_row() {
    let url = "https://example.com/llms.txt";
    let transport = Arc::new(MockTransport::new(vec![get(url, to_http(url))], ok()));
    let outcome = run_http(check("llms-txt"), &context(transport.clone()));
    assert_eq!(outcome.status, ProbeStatus::Absent);
    assert_eq!(requested(&transport), [url]);
    let row = item_for(&outcome, url);
    assert_eq!(row.get("status"), Some(&json!(301)));
    assert_eq!(row.get("ok"), Some(&json!(false)));
    assert_eq!(row.get("why"), Some(&json!([REDIRECTS_TO_HTTP])));
    assert_eq!(row.get("error"), Some(&Value::Null));
}

#[test]
fn an_auth_md_that_redirects_to_http_reads_absent() {
    let url = "https://example.com/.well-known/auth.md";
    let transport = Arc::new(MockTransport::new(vec![get(url, to_http(url))], missing()));
    let outcome = run_auth_md(check("auth-md"), &context(transport));
    assert_eq!(outcome.status, ProbeStatus::Absent);
    assert_eq!(
        Value::Object(item_for(&outcome, url).clone()),
        json!({"url": url, "status": 301, "ok": false, "why": [REDIRECTS_TO_HTTP]})
    );
}

#[test]
fn a_scoped_llms_that_redirects_to_http_reads_absent() {
    let url = "https://example.com/docs/llms.txt";
    let transport = Arc::new(MockTransport::new(vec![get(url, to_http(url))], missing()));
    let mut ctx = context(transport);
    ctx.scoped_dirs = Arc::new(vec!["/docs".to_string()]);
    let outcome = run_scoped_llms(check("llms-txt-scoped"), &ctx);
    assert_eq!(outcome.status, ProbeStatus::Absent);
    assert_eq!(
        Value::Object(item_for(&outcome, url).clone()),
        json!({"url": url, "status": 301, "ok": false, "why": [REDIRECTS_TO_HTTP]})
    );
}

#[test]
fn a_kept_document_that_redirected_to_http_reads_absent() {
    let url = "https://example.com/.well-known/api-catalog";
    let mut ctx = context(Arc::new(MockTransport::new(vec![], missing())));
    let response = ProbeResponse {
        status: Some(301),
        error: Some(
            "redirect refused: 301 to http://example.com/.well-known/api-catalog: not https"
                .to_string(),
        ),
        refused: Some(Refusal::InsecureScheme),
        ..ProbeResponse::failed(String::new(), 0)
    };
    let kept = RetainedDocument {
        url: url.to_string(),
        response,
        shape: None,
    };
    ctx.retained_documents = Arc::new(HashMap::from([(API_CATALOG_DOCUMENT, kept)]));
    let outcome = run_retained_document(check("api-catalog"), &ctx);
    assert_eq!(outcome.status, ProbeStatus::Absent);
    let row = item_for(&outcome, url);
    assert_eq!(row.get("status"), Some(&json!(301)));
    assert_eq!(row.get("why"), Some(&json!([REDIRECTS_TO_HTTP])));
    assert_eq!(row.get("error"), Some(&Value::Null));
    assert_eq!(row.get("retained"), Some(&json!("api-catalog")));
}

#[test]
fn a_markdown_twin_that_redirects_to_http_reads_absent() {
    let url = "https://example.com/";
    let transport = Arc::new(MockTransport::new(vec![get(url, to_http(url))], missing()));
    let outcome = run_markdown_frontmatter(check("markdown-frontmatter"), &context(transport));
    assert_eq!(outcome.status, ProbeStatus::Absent);
    assert_eq!(
        Value::Object(item_for(&outcome, url).clone()),
        json!({"url": url, "status": 301, "ok": false, "why": [REDIRECTS_TO_HTTP]})
    );
}
