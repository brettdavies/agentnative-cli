//! A row that asserts nothing about the body and keeps none reads the status
//! and headers, so a 70 MB llms-full.txt costs its headers. The memo keeps
//! reading the start of the body for a later row, and a row whose failure is
//! the body itself reads through.

#![cfg(feature = "web")]

mod common;

use std::io::{self, Cursor, Read};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use agentnative::web_audit::engine::{HandlerContext, ProbeStatus};
use agentnative::web_audit::fetch::memo::{MEMO_BODY_MAX_BYTES, RequestMemo};
use agentnative::web_audit::handlers::http::{run_http, run_legacy_alias_redirects};
use agentnative::web_audit::headers::Headers;
use agentnative::web_audit::registry::WebCheck;
use agentnative::web_audit::transport::{Request, Response, Transport, TransportError};
use common::handler_context::{check, context};
use serde_json::{Value, json};

const CHUNK: usize = 64 * 1024;

/// A body that never ends, noting how much of it was read and when it was let go.
struct EndlessBody {
    read: Arc<AtomicUsize>,
    dropped: Arc<AtomicBool>,
}

impl Read for EndlessBody {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        thread::sleep(Duration::from_millis(1));
        let n = buf.len().min(CHUNK);
        self.read.fetch_add(n, Ordering::SeqCst);
        Ok(n)
    }
}

impl Drop for EndlessBody {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

/// A body that sends its first bytes and then holds open, as a stuck SSE stream does.
struct HeldOpenBody {
    first: Option<Vec<u8>>,
    until: Instant,
}

impl Read for HeldOpenBody {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(first) = self.first.take() {
            buf[..first.len()].copy_from_slice(&first);
            return Ok(first.len());
        }
        thread::sleep(self.until.saturating_duration_since(Instant::now()));
        Err(io::Error::new(io::ErrorKind::TimedOut, "held open"))
    }
}

type Answer = dyn Fn(&Request) -> Response + Send + Sync;

struct Target {
    answer: Box<Answer>,
    requests: Mutex<Vec<String>>,
}

impl Transport for Target {
    fn send(&self, request: &Request) -> Result<Response, TransportError> {
        self.requests.lock().unwrap().push(request.url.clone());
        Ok((self.answer)(request))
    }
}

fn target(answer: impl Fn(&Request) -> Response + Send + Sync + 'static) -> Arc<Target> {
    Arc::new(Target {
        answer: Box::new(answer),
        requests: Mutex::new(Vec::new()),
    })
}

struct Endless {
    read: Arc<AtomicUsize>,
    dropped: Arc<AtomicBool>,
    target: Arc<Target>,
}

fn endless(headers: &'static [(&'static str, &'static str)]) -> Endless {
    let read = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicBool::new(false));
    let (r, d) = (read.clone(), dropped.clone());
    let target = target(move |_| Response {
        status: 200,
        headers: Headers::from_pairs(headers.iter().copied()),
        body: Box::new(EndlessBody {
            read: r.clone(),
            dropped: d.clone(),
        }),
    });
    Endless {
        read,
        dropped,
        target,
    }
}

fn row(with: Value) -> WebCheck {
    let mut row = *check("security-txt");
    row.with_json = Box::leak(with.to_string().into_boxed_str());
    row
}

fn memoized(transport: Arc<Target>, memo: &RequestMemo, timeout: Duration) -> HandlerContext {
    let mut ctx = context(transport);
    ctx.fetch = ctx.fetch.with_memo(memo.clone());
    ctx.default_timeout = timeout;
    ctx.mcp_endpoint = Some("https://example.com/mcp".to_string());
    ctx
}

#[test]
fn a_row_needing_no_body_passes_on_each_header_expectation_while_the_body_never_ends() {
    // A regex expectation compiles from the registry, so those two are registry rows.
    let cases: [(WebCheck, &'static [(&'static str, &'static str)]); 5] = [
        (*check("llms-full-txt"), &[]),
        (
            row(json!({ "path": "/large", "expect": { "status_below": 500 } })),
            &[],
        ),
        (
            *check("accept-markdown"),
            &[("content-type", "text/markdown")],
        ),
        (
            row(json!({ "path": "/large", "expect": { "header_present": "vary" } })),
            &[("vary", "accept")],
        ),
        (*check("markdown-vary"), &[("vary", "accept, user-agent")]),
    ];
    for (case, headers) in cases {
        // A row that read this body would run out of time on it, and say so.
        let transport = target(move |_| Response {
            status: 200,
            headers: Headers::from_pairs(headers.iter().copied()),
            body: Box::new(HeldOpenBody {
                first: Some(b"x".to_vec()),
                until: Instant::now() + Duration::from_secs(3),
            }),
        });
        let memo = RequestMemo::new();
        let outcome = run_http(
            &case,
            &memoized(transport, &memo, Duration::from_millis(300)),
        );
        memo.close();
        assert_eq!(
            outcome.status,
            ProbeStatus::Pass,
            "{}: {outcome:?}",
            case.with_json
        );
        assert!(
            outcome.evidence[0].get("truncated").is_none(),
            "{}: {outcome:?}",
            case.with_json
        );
    }
}

#[test]
fn a_legacy_alias_reads_its_status_while_its_body_never_ends() {
    // A row that read this body would run out of time on it, and read that failure
    // as the alias not being there.
    let transport = target(|_| Response {
        status: 200,
        headers: Headers::new(),
        body: Box::new(HeldOpenBody {
            first: Some(b"x".to_vec()),
            until: Instant::now() + Duration::from_secs(3),
        }),
    });
    let memo = RequestMemo::new();
    let outcome = run_legacy_alias_redirects(
        &row(json!({
            "canonical": "/.well-known/mcp/server-card.json",
            "aliases": ["/.well-known/mcp.json"],
        })),
        &memoized(transport, &memo, Duration::from_millis(300)),
    );
    memo.close();
    assert_eq!(outcome.status, ProbeStatus::Noncompliant, "{outcome:?}");
}

#[test]
fn the_memo_reads_the_first_mib_in_the_background_and_no_further() {
    let body = endless(&[]);
    let memo = RequestMemo::new();
    run_http(
        &row(json!({ "path": "/llms-full.txt", "expect": { "status": [200] } })),
        &memoized(body.target.clone(), &memo, Duration::from_secs(10)),
    );
    let until = Instant::now() + Duration::from_secs(5);
    while !body.dropped.load(Ordering::SeqCst) && Instant::now() < until {
        thread::sleep(Duration::from_millis(5));
    }
    memo.close();
    assert!(body.dropped.load(Ordering::SeqCst));
    assert!(
        body.read.load(Ordering::SeqCst) <= MEMO_BODY_MAX_BYTES + 3 * CHUNK,
        "{}",
        body.read.load(Ordering::SeqCst)
    );
}

#[test]
fn a_later_row_that_needs_the_body_reads_it_from_the_same_request() {
    let transport = target(|_| Response {
        status: 200,
        headers: Headers::new(),
        body: Box::new(Cursor::new(b"User-agent: GPTBot\nDisallow: /\n".to_vec())),
    });
    let memo = RequestMemo::new();
    let ctx = memoized(transport.clone(), &memo, Duration::from_secs(2));
    let status = run_http(check("robots"), &ctx);
    let body = run_http(check("robots-ai-rules"), &ctx);
    memo.close();
    assert_eq!(status.status, ProbeStatus::Pass, "{status:?}");
    assert_eq!(body.status, ProbeStatus::Pass, "{body:?}");
    assert_eq!(transport.requests.lock().unwrap().len(), 1);
}

#[test]
fn a_row_asserting_on_the_body_reads_it() {
    let transport = target(|_| Response {
        status: 200,
        headers: Headers::new(),
        body: Box::new(Cursor::new(
            b"Contact: mailto:security@example.com\n".to_vec(),
        )),
    });
    let outcome = run_http(check("security-txt"), &context(transport));
    assert_eq!(outcome.status, ProbeStatus::Pass, "{outcome:?}");
}

#[test]
fn a_row_that_keeps_the_body_reads_it() {
    let transport = target(|_| Response {
        status: 200,
        headers: Headers::new(),
        body: Box::new(Cursor::new(b"# Site\n".to_vec())),
    });
    let outcome = run_http(
        &row(json!({
            "path": "/llms.txt",
            "retain_body": true,
            "expect": { "status": [200] }
        })),
        &context(transport),
    );
    assert_eq!(outcome.evidence[0].get("body"), Some(&json!("# Site\n")));
}

#[test]
fn a_hang_detection_row_reads_through_so_a_body_held_open_after_a_clean_status_is_broken() {
    let transport = target(|_| Response {
        status: 200,
        headers: Headers::from_pairs([("content-type", "text/event-stream")]),
        body: Box::new(HeldOpenBody {
            first: Some(b": open\n\n".to_vec()),
            until: Instant::now() + Duration::from_secs(2),
        }),
    });
    let memo = RequestMemo::new();
    let outcome = run_http(
        &row(json!({
            "path": "{mcp_endpoint}",
            "timeout": 0.1,
            "expect": { "status_below": 500, "sse_when_success": true }
        })),
        &memoized(transport, &memo, Duration::from_secs(2)),
    );
    memo.close();
    assert_eq!(outcome.status, ProbeStatus::Broken, "{outcome:?}");
    let error = outcome.evidence[0].get("error").and_then(Value::as_str);
    assert!(
        error.is_some_and(|e| e.starts_with("TimeoutError")),
        "{outcome:?}"
    );
}
