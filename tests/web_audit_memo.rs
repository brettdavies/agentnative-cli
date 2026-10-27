//! An audit asks the target for each distinct request once. A row that needs
//! an answer another row already received reads that answer instead of
//! reaching the target again, so a site is not charged twice for one
//! question and a second fetch cannot disagree with the first. A request goes
//! out again only when the answer on record cannot serve the caller: it holds
//! less of the body than the caller reads, or it arrived later than the
//! caller's deadline allows. A caller that skips the body still leaves the
//! body in the memo for one that reads it.
//!
//! The cases port `agentnative-site:tests/web-audit-request-memo.test.ts`
//! against the mock transport and a scripted transport whose bodies stall or
//! arrive late. A read blocked in the transport cannot be cancelled from
//! outside, so where the site asserts its stub saw a cancel, these cases
//! assert the memo let the read go at its deadline and dropped the body once
//! the blocked read returned.

#![cfg(feature = "web")]

mod common;

use std::collections::{BTreeMap, HashMap};
use std::io::{self, Cursor, Read};
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use agentnative::web_audit::fetch::memo::{MemoStats, RequestMemo};
use agentnative::web_audit::fetch::{
    CrossOriginRedirects, FetchInit, FetchOptions, Fetcher, ProbeResponse, Refusal,
};
use agentnative::web_audit::handlers::default_handlers;
use agentnative::web_audit::headers::Headers;
use agentnative::web_audit::locality::Resolver;
use agentnative::web_audit::mock::{MockResponse, MockRule, MockTransport};
use agentnative::web_audit::transport::{Request, Response, Transport, TransportError};
use common::corpus::{load_cases, replay_on, transport_for};

const URL_A: &str = "https://example.com/robots.txt";
const DEADLINE: &str = "TimeoutError: deadline exceeded";

struct PublicResolver;

impl Resolver for PublicResolver {
    fn resolve(&self, _host: &str) -> Result<Vec<IpAddr>, String> {
        Ok(vec!["93.184.216.34".parse().unwrap()])
    }
}

type Shared = Arc<dyn Transport + Send + Sync>;
type Answer = dyn Fn(&Request) -> Result<Response, TransportError> + Send + Sync;

/// A scripted target that records every request it receives.
struct Target {
    received: Mutex<Vec<String>>,
    answer: Box<Answer>,
}

impl Transport for Target {
    fn send(&self, request: &Request) -> Result<Response, TransportError> {
        self.received
            .lock()
            .unwrap()
            .push(format!("{} {}", request.method, request.url));
        (self.answer)(request)
    }
}

fn target(
    answer: impl Fn(&Request) -> Result<Response, TransportError> + Send + Sync + 'static,
) -> (Arc<Target>, Shared) {
    let target = Arc::new(Target {
        received: Mutex::new(Vec::new()),
        answer: Box::new(answer),
    });
    let shared: Shared = target.clone();
    (target, shared)
}

impl Target {
    fn received(&self) -> Vec<String> {
        self.received.lock().unwrap().clone()
    }
}

/// A target answering every request with the same rule.
fn mock(response: MockResponse) -> (Arc<MockTransport>, Shared) {
    let transport = Arc::new(MockTransport::new(Vec::new(), response));
    let shared: Shared = transport.clone();
    (transport, shared)
}

fn received(transport: &MockTransport) -> Vec<String> {
    transport
        .requests()
        .iter()
        .map(|r| format!("{} {}", r.method, r.url))
        .collect()
}

fn text(status: u16, headers: &[(&str, &str)], body: &str) -> Response {
    Response {
        status,
        headers: Headers::from_pairs(headers.iter().copied()),
        body: Box::new(Cursor::new(body.as_bytes().to_vec())),
    }
}

#[derive(Default)]
struct Opts {
    timeout_ms: Option<u64>,
    max_body_bytes: Option<usize>,
    follow_redirects: Option<bool>,
    cross_origin: Option<CrossOriginRedirects>,
}

fn fetch(transport: &Shared, memo: Option<&RequestMemo>, url: &str, opts: Opts) -> ProbeResponse {
    fetch_with(transport, memo, url, &FetchInit::default(), opts)
}

fn fetch_with(
    transport: &Shared,
    memo: Option<&RequestMemo>,
    url: &str,
    init: &FetchInit,
    opts: Opts,
) -> ProbeResponse {
    let options = FetchOptions {
        timeout: Duration::from_millis(opts.timeout_ms.unwrap_or(8_000)),
        max_body_bytes: opts.max_body_bytes,
        follow_redirects: opts.follow_redirects.unwrap_or(true),
        cross_origin_redirects: opts.cross_origin,
        ..FetchOptions::default()
    };
    let fetcher = match memo {
        Some(memo) => Fetcher::memoized(transport, &PublicResolver, "test-ua", memo),
        None => Fetcher::new(&**transport, &PublicResolver, "test-ua"),
    };
    fetcher.fetch(url, init, &options)
}

fn ms(timeout_ms: u64) -> Opts {
    Opts {
        timeout_ms: Some(timeout_ms),
        ..Opts::default()
    }
}

fn status_only(timeout_ms: u64) -> Opts {
    Opts {
        timeout_ms: Some(timeout_ms),
        max_body_bytes: Some(0),
        ..Opts::default()
    }
}

fn capped(max_body_bytes: usize) -> Opts {
    Opts {
        max_body_bytes: Some(max_body_bytes),
        ..Opts::default()
    }
}

/// A latch the stalling doubles block on; dropping the guard opens it, so
/// abandoned reads end with the test.
#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    fn wait(&self) {
        let mut open = self.open.lock().unwrap();
        while !*open {
            open = self.changed.wait(open).unwrap();
        }
    }

    fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }
}

struct Release(Arc<Gate>);

impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}

/// A body that sends one chunk, then blocks until the gate opens; `dropped`
/// reports whether a reader let go of it.
struct StalledBody {
    first: Option<Vec<u8>>,
    gate: Arc<Gate>,
    dropped: Arc<AtomicBool>,
}

impl Read for StalledBody {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(first) = self.first.take() {
            buf[..first.len()].copy_from_slice(&first);
            return Ok(first.len());
        }
        self.gate.wait();
        Err(io::Error::new(io::ErrorKind::TimedOut, "stalled"))
    }
}

impl Drop for StalledBody {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

/// A target whose status and headers arrive at once and whose body stalls
/// after one chunk.
fn stalls_after_headers(
    status: u16,
    headers: &'static [(&'static str, &'static str)],
) -> (Arc<Target>, Shared, Arc<AtomicBool>, Release) {
    let gate = Arc::new(Gate::default());
    let dropped = Arc::new(AtomicBool::new(false));
    let (body_gate, body_dropped) = (gate.clone(), dropped.clone());
    let (target, shared) = target(move |_| {
        Ok(Response {
            status,
            headers: Headers::from_pairs(headers.iter().copied()),
            body: Box::new(StalledBody {
                first: Some(b"{\"error\":".to_vec()),
                gate: body_gate.clone(),
                dropped: body_dropped.clone(),
            }),
        })
    });
    (target, shared, dropped, Release(gate))
}

/// A body that arrives whole at `at`.
struct LateBody {
    at: Instant,
    text: Option<Vec<u8>>,
}

impl Read for LateBody {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let Some(text) = self.text.take() else {
            return Ok(0);
        };
        thread::sleep(self.at.saturating_duration_since(Instant::now()));
        buf[..text.len()].copy_from_slice(&text);
        Ok(text.len())
    }
}

/// A target whose status and headers arrive at once and whose body arrives
/// whole after `delay`.
fn body_after(delay: Duration, body: &'static str) -> (Arc<Target>, Shared) {
    target(move |_| {
        Ok(Response {
            status: 200,
            headers: Headers::new(),
            body: Box::new(LateBody {
                at: Instant::now() + delay,
                text: Some(body.as_bytes().to_vec()),
            }),
        })
    })
}

/// A target that never answers until the gate opens.
fn hang() -> (Arc<Target>, Shared, Release) {
    let gate = Arc::new(Gate::default());
    let waits = gate.clone();
    let (target, shared) = target(move |_| {
        waits.wait();
        Err(TransportError::Timeout)
    });
    (target, shared, Release(gate))
}

fn sleep_ms(ms: u64) {
    thread::sleep(Duration::from_millis(ms));
}

/// Block until `reached` holds, checking every millisecond. A case that
/// needs another thread to have got somewhere waits on that state rather
/// than on the clock, so a loaded machine slows it down instead of
/// reordering it.
fn wait_for(what: &str, reached: impl Fn() -> bool) {
    let cap = Instant::now() + Duration::from_secs(5);
    while !reached() {
        assert!(Instant::now() < cap, "waited 5 s for {what}");
        sleep_ms(1);
    }
}

#[test]
fn an_identical_request_reads_the_answer_already_received_as_a_copy_of_its_own() {
    let (transport, shared) = mock(MockResponse::ok(200, &[("x-a", "1")], b"User-agent: *"));
    let memo = RequestMemo::new();
    let mut first = fetch(&shared, Some(&memo), URL_A, Opts::default());
    let second = fetch(&shared, Some(&memo), URL_A, Opts::default());
    first.headers = Headers::from_pairs([("x-a", "changed")]);
    assert_eq!(received(&transport), [format!("GET {URL_A}")]);
    assert_eq!(
        (
            second.status,
            second.body.as_str(),
            second.headers.get("x-a")
        ),
        (Some(200), "User-agent: *", Some("1"))
    );
}

#[test]
fn the_memo_counts_the_hops_it_sent_and_answered_from_the_record_and_the_body_bytes_it_kept() {
    let (_, shared) = mock(MockResponse::ok(200, &[], b"User-agent: *"));
    let memo = RequestMemo::new();
    let llms = "https://example.com/llms.txt";
    fetch(&shared, Some(&memo), URL_A, Opts::default());
    fetch(&shared, Some(&memo), URL_A, Opts::default());
    fetch(&shared, Some(&memo), llms, capped(0));
    fetch(&shared, Some(&memo), llms, Opts::default());
    assert_eq!(
        memo.stats(),
        MemoStats {
            sent: 2,
            reused: 2,
            retained_bytes: 26,
            bodies_retained: 2,
            largest_body_bytes: 13,
            over_body_cap: 0,
            over_total_cap: 0,
            reads_open: 0,
            waiting: 0,
        }
    );
}

#[test]
fn requests_that_differ_in_method_header_or_body_each_reach_the_target() {
    let (transport, shared) = mock(MockResponse::ok(200, &[], b"ok"));
    let memo = RequestMemo::new();
    let accept = FetchInit {
        headers: vec![("accept".to_string(), "text/markdown".to_string())],
        ..FetchInit::default()
    };
    let post = |body: &str| FetchInit {
        method: "POST".to_string(),
        body: Some(body.as_bytes().to_vec()),
        ..FetchInit::default()
    };
    fetch(&shared, Some(&memo), URL_A, Opts::default());
    fetch_with(&shared, Some(&memo), URL_A, &accept, Opts::default());
    fetch_with(&shared, Some(&memo), URL_A, &post("a"), Opts::default());
    fetch_with(&shared, Some(&memo), URL_A, &post("b"), Opts::default());
    assert_eq!(transport.requests().len(), 4);
}

#[test]
fn concurrent_identical_requests_share_one_request() {
    let (target, shared) = target(|_| {
        sleep_ms(50);
        Ok(text(200, &[], "ok"))
    });
    let memo = RequestMemo::new();
    let bodies: Vec<String> = thread::scope(|s| {
        let calls: Vec<_> = (0..3)
            .map(|_| s.spawn(|| fetch(&shared, Some(&memo), URL_A, Opts::default()).body))
            .collect();
        calls.into_iter().map(|c| c.join().unwrap()).collect()
    });
    assert_eq!(target.received().len(), 1);
    assert_eq!(bodies, ["ok", "ok", "ok"]);
}

#[test]
fn a_status_only_probe_and_a_body_reader_share_one_request_in_either_order() {
    let (reader_first, shared) = mock(MockResponse::ok(200, &[], b"# Guide\n"));
    let memo = RequestMemo::new();
    let reader = fetch(&shared, Some(&memo), URL_A, Opts::default());
    let status_only = fetch(&shared, Some(&memo), URL_A, capped(0));
    assert_eq!(
        (
            reader.body.as_str(),
            status_only.status,
            status_only.body.as_str(),
            reader_first.requests().len()
        ),
        ("# Guide\n", Some(200), "", 1)
    );

    let (status_first, shared) = mock(MockResponse::ok(200, &[], b"# Guide\n"));
    let other = RequestMemo::new();
    fetch(&shared, Some(&other), URL_A, capped(0));
    let later = fetch(&shared, Some(&other), URL_A, Opts::default());
    assert_eq!(
        (later.body.as_str(), status_first.requests().len()),
        ("# Guide\n", 1)
    );
}

#[test]
fn a_retry_that_serves_less_than_the_answer_on_record_does_not_replace_it() {
    let calls = AtomicUsize::new(0);
    let (target, shared) = target(move |_| {
        if calls.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(text(200, &[], "0123456789"))
        } else {
            Err(TransportError::Verbatim(
                "TypeError: connection reset".into(),
            ))
        }
    });
    let memo = RequestMemo::new();
    fetch(&shared, Some(&memo), URL_A, capped(4));
    let failed = fetch(&shared, Some(&memo), URL_A, Opts::default());
    let capped = fetch(&shared, Some(&memo), URL_A, capped(4));
    assert_eq!(
        (
            failed.error.as_deref(),
            capped.body.as_str(),
            target.received().len()
        ),
        (Some("TypeError: connection reset"), "0123", 2)
    );
}

#[test]
fn a_body_cut_at_a_smaller_cap_than_the_next_caller_reads_is_asked_for_again() {
    let (transport, shared) = mock(MockResponse::ok(200, &[], b"0123456789"));
    let memo = RequestMemo::new();
    let cut = fetch(&shared, Some(&memo), URL_A, capped(4));
    let full = fetch(&shared, Some(&memo), URL_A, Opts::default());
    let smaller = fetch(&shared, Some(&memo), URL_A, capped(2));
    assert_eq!(
        (
            cut.body.as_str(),
            cut.truncated,
            full.body.as_str(),
            smaller.body.as_str(),
            smaller.truncated
        ),
        ("0123", true, "0123456789", "01", true)
    );
    assert_eq!(transport.requests().len(), 2);
}

#[test]
fn a_timeout_answers_only_a_caller_whose_deadline_is_no_longer() {
    let (target, shared, _release) = hang();
    let memo = RequestMemo::new();
    let timed_out = fetch(&shared, Some(&memo), URL_A, ms(40));
    let shorter = fetch(&shared, Some(&memo), URL_A, ms(20));
    wait_for("the first request to reach the target", || {
        !target.received().is_empty()
    });
    assert_eq!(
        (
            timed_out.error.as_deref(),
            shorter.error.as_deref(),
            target.received().len()
        ),
        (Some(DEADLINE), Some(DEADLINE), 1)
    );
    fetch(&shared, Some(&memo), URL_A, ms(60));
    wait_for(
        "the longer caller's own request to reach the target",
        || target.received().len() >= 2,
    );
    assert_eq!(target.received().len(), 2);
}

#[test]
fn an_answer_that_took_longer_than_the_next_caller_allows_is_asked_for_again() {
    let (target, shared) = target(|_| {
        sleep_ms(60);
        Ok(text(200, &[], "slow"))
    });
    let memo = RequestMemo::new();
    fetch(&shared, Some(&memo), URL_A, ms(1_000));
    fetch(&shared, Some(&memo), URL_A, ms(20));
    wait_for(
        "the hurried caller's own request to reach the target",
        || target.received().len() >= 2,
    );
    assert_eq!(target.received().len(), 2);
}

#[test]
fn a_redirect_one_caller_followed_answers_callers_that_keep_it_with_or_without_its_body() {
    let card = "https://example.com/.well-known/mcp.json";
    let canonical = "https://example.com/mcp/server-card";
    let transport = Arc::new(MockTransport::new(
        vec![
            MockRule::new("GET", card).response(MockResponse::ok(
                301,
                &[("location", canonical)],
                b"Moved",
            )),
            MockRule::new("GET", canonical).response(MockResponse::ok(200, &[], b"{}")),
        ],
        MockResponse::error("unmatched"),
    ));
    let shared: Shared = transport.clone();
    let memo = RequestMemo::new();
    let followed = fetch(&shared, Some(&memo), card, Opts::default());
    let kept = fetch(
        &shared,
        Some(&memo),
        card,
        Opts {
            follow_redirects: Some(false),
            max_body_bytes: Some(0),
            ..Opts::default()
        },
    );
    assert_eq!(
        (followed.status, kept.status, kept.headers.get("location")),
        (Some(200), Some(301), Some(canonical))
    );
    assert_eq!(
        received(&transport),
        [format!("GET {card}"), format!("GET {canonical}")]
    );
    let with_body = fetch(
        &shared,
        Some(&memo),
        card,
        Opts {
            follow_redirects: Some(false),
            ..Opts::default()
        },
    );
    assert_eq!(
        (with_body.body.as_str(), transport.requests().len()),
        ("Moved", 2)
    );
}

#[test]
fn a_redirect_one_caller_followed_answers_a_caller_that_may_keep_cross_origin_redirects() {
    let endpoint = "https://example.com/mcp";
    let slash = format!("{endpoint}/");
    let transport = Arc::new(MockTransport::new(
        vec![
            MockRule::new("GET", endpoint).response(MockResponse::ok(
                308,
                &[("location", slash.as_str())],
                b"Moved",
            )),
            MockRule::new("GET", &slash).response(MockResponse::ok(200, &[], b"{\"ok\":true}")),
        ],
        MockResponse::error("unmatched"),
    ));
    let shared: Shared = transport.clone();
    let memo = RequestMemo::new();
    fetch(&shared, Some(&memo), endpoint, Opts::default());
    let returning = fetch(
        &shared,
        Some(&memo),
        endpoint,
        Opts {
            cross_origin: Some(CrossOriginRedirects::Return),
            ..Opts::default()
        },
    );
    assert_eq!(
        (returning.status, returning.body.as_str()),
        (Some(200), "{\"ok\":true}")
    );
    assert_eq!(
        received(&transport),
        [format!("GET {endpoint}"), format!("GET {slash}")]
    );
}

#[test]
fn a_status_only_probe_answers_at_the_headers_while_the_memo_reads_a_stalling_body_within_its_deadline()
 {
    let (target, shared, _, _release) = stalls_after_headers(200, &[]);
    let memo = RequestMemo::new();
    let started = Instant::now();
    let status_only = fetch(&shared, Some(&memo), URL_A, status_only(1_000));
    let status_ms = started.elapsed();
    let reader = fetch(&shared, Some(&memo), URL_A, ms(1_000));
    assert_eq!(
        (
            status_only.status,
            status_only.error.as_deref(),
            status_ms < Duration::from_millis(500)
        ),
        (Some(200), None, true)
    );
    assert_eq!(
        (reader.error.as_deref(), target.received().len()),
        (Some(DEADLINE), 1)
    );
}

#[test]
fn the_memos_body_read_stops_at_the_status_only_callers_deadline_when_nobody_waits_on_it() {
    let (_, shared, dropped, release) = stalls_after_headers(200, &[]);
    let memo = RequestMemo::new();
    let started = Instant::now();
    fetch(&shared, Some(&memo), URL_A, status_only(500));
    assert_eq!(
        (dropped.load(Ordering::SeqCst), memo.stats().reads_open),
        (false, 1)
    );
    thread::sleep((started + Duration::from_millis(600)).saturating_duration_since(Instant::now()));
    let stats = memo.stats();
    assert_eq!((stats.reads_open, stats.retained_bytes), (0, 0));
    drop(release);
    wait_for("the abandoned read to drop the body", || {
        dropped.load(Ordering::SeqCst)
    });
    assert_eq!(memo.stats().retained_bytes, 0);
}

#[test]
fn a_body_reader_that_joins_the_memos_body_read_late_in_its_budget_still_reads_the_body() {
    let (target, shared) = body_after(Duration::from_millis(300), "BODY");
    let memo = RequestMemo::new();
    thread::scope(|s| {
        let status_only = s.spawn(|| fetch(&shared, Some(&memo), URL_A, status_only(1_000)));
        wait_for("the status-only request to reach the target", || {
            target.received().len() == 1
        });
        sleep_ms(200);
        let reader = fetch(&shared, Some(&memo), URL_A, ms(350));
        status_only.join().unwrap();
        assert_eq!(
            (
                reader.body.as_str(),
                reader.error.as_deref(),
                target.received().len()
            ),
            ("BODY", None, 1)
        );
    });
}

#[test]
fn a_body_reader_with_a_later_deadline_pushes_the_memos_body_read_back_to_it() {
    let (target, shared) = body_after(Duration::from_millis(800), "BODY");
    let memo = RequestMemo::new();
    fetch(&shared, Some(&memo), URL_A, status_only(400));
    let reader = fetch(&shared, Some(&memo), URL_A, ms(2_000));
    assert_eq!(
        (
            reader.body.as_str(),
            reader.error.as_deref(),
            target.received().len()
        ),
        ("BODY", None, 1)
    );
}

#[test]
fn a_status_only_caller_does_not_wait_for_a_concurrent_body_readers_stalled_body() {
    let (target, shared, _, _release) = stalls_after_headers(404, &[("ratelimit-limit", "100")]);
    let memo = RequestMemo::new();
    thread::scope(|s| {
        let reader = s.spawn(|| {
            fetch(
                &shared,
                Some(&memo),
                URL_A,
                Opts {
                    timeout_ms: Some(1_000),
                    max_body_bytes: Some(65_536),
                    ..Opts::default()
                },
            )
        });
        wait_for("the body reader's request to reach the target", || {
            target.received().len() == 1
        });
        let started = Instant::now();
        let status_only = fetch(&shared, Some(&memo), URL_A, status_only(1_000));
        let status_ms = started.elapsed();
        assert_eq!(
            (
                status_only.status,
                status_only.headers.get("ratelimit-limit"),
                status_ms < Duration::from_millis(500),
                reader.is_finished()
            ),
            (Some(404), Some("100"), true, false)
        );
        let reader = reader.join().unwrap();
        assert_eq!(
            (reader.error.as_deref(), target.received().len()),
            (Some(DEADLINE), 1)
        );
    });
}

#[test]
fn a_body_read_that_fails_after_the_headers_still_answers_a_status_only_caller() {
    let (target, shared, _, _release) = stalls_after_headers(404, &[("ratelimit-limit", "100")]);
    let memo = RequestMemo::new();
    let body_reader = || Opts {
        timeout_ms: Some(500),
        max_body_bytes: Some(65_536),
        ..Opts::default()
    };
    let reader = fetch(&shared, Some(&memo), URL_A, body_reader());
    let status_only = fetch(&shared, Some(&memo), URL_A, status_only(500));
    let same_reader = fetch(&shared, Some(&memo), URL_A, body_reader());
    assert_eq!(
        (reader.status, reader.error.as_deref()),
        (None, Some(DEADLINE))
    );
    assert_eq!(
        (
            status_only.status,
            status_only.error.as_deref(),
            status_only.headers.get("ratelimit-limit")
        ),
        (Some(404), None, Some("100"))
    );
    assert_eq!(
        (same_reader.error.as_deref(), target.received().len()),
        (Some(DEADLINE), 1)
    );
}

#[test]
fn closing_the_memo_stops_a_body_read_it_runs_on_its_own_and_lets_go_of_every_answer() {
    let (_, shared, dropped, release) = stalls_after_headers(200, &[]);
    let memo = RequestMemo::new();
    fetch(&shared, Some(&memo), URL_A, status_only(5_000));
    assert_eq!(
        (
            dropped.load(Ordering::SeqCst),
            memo.stats().reads_open,
            memo.len()
        ),
        (false, 1, 1)
    );
    memo.close();
    memo.close();
    assert_eq!((memo.stats().reads_open, memo.len()), (0, 0));
    drop(release);
    wait_for("the abandoned read to drop the body", || {
        dropped.load(Ordering::SeqCst)
    });
}

#[test]
fn closing_the_memo_aborts_a_request_still_waiting_on_the_target() {
    let (target, shared, _release) = hang();
    let memo = RequestMemo::new();
    let (answer, after_close) = thread::scope(|s| {
        let pending = s.spawn(|| fetch(&shared, Some(&memo), URL_A, ms(5_000)));
        wait_for("the request to reach the target", || {
            target.received().len() == 1
        });
        let closed = Instant::now();
        memo.close();
        let answer = pending.join().unwrap();
        (answer, closed.elapsed())
    });
    assert_eq!(
        (
            answer.status,
            target.received().len(),
            after_close < Duration::from_secs(2)
        ),
        (None, 1, true)
    );
}

#[test]
fn a_closed_memo_sends_nothing_and_keeps_nothing() {
    let (transport, shared) = mock(MockResponse::ok(200, &[], b"ok"));
    let memo = RequestMemo::new();
    memo.close();
    let answer = fetch(&shared, Some(&memo), URL_A, Opts::default());
    assert_eq!(
        (answer.status, transport.requests().len(), memo.len()),
        (None, 0, 0)
    );
}

#[test]
fn a_caller_still_waiting_on_the_memo_when_it_closes_cannot_keep_its_body_read_running() {
    let (_, shared, dropped, release) = stalls_after_headers(200, &[]);
    let memo = RequestMemo::new();
    fetch(&shared, Some(&memo), URL_A, status_only(5_000));
    thread::scope(|s| {
        let waiting = s.spawn(|| fetch(&shared, Some(&memo), URL_A, ms(5_000)));
        wait_for("the body reader to wait on the memo's read", || {
            memo.stats().waiting == 1
        });
        memo.close();
        waiting.join().unwrap();
    });
    assert_eq!(memo.stats().reads_open, 0);
    drop(release);
    wait_for("the abandoned read to drop the body", || {
        dropped.load(Ordering::SeqCst)
    });
}

#[test]
fn a_caller_still_waiting_on_the_memo_when_it_closes_keeps_nothing_in_it() {
    let (target, shared, _release) = hang();
    let memo = RequestMemo::new();
    fetch(&shared, Some(&memo), URL_A, ms(50));
    thread::scope(|s| {
        let waiting = s.spawn(|| fetch(&shared, Some(&memo), URL_A, ms(5_000)));
        wait_for(
            "the longer caller's own request to reach the target",
            || target.received().len() == 2 && memo.stats().waiting == 1,
        );
        memo.close();
        waiting.join().unwrap();
    });
    assert_eq!(memo.len(), 0);
}

#[test]
fn a_caller_waiting_on_an_identical_request_in_flight_gives_up_at_its_own_deadline() {
    let (target, shared) = target(|_| {
        sleep_ms(1_000);
        Ok(text(200, &[], "late"))
    });
    let memo = RequestMemo::new();
    let started = Instant::now();
    let (slow, hurried) = thread::scope(|s| {
        let slow = s.spawn(|| fetch(&shared, Some(&memo), URL_A, ms(3_000)));
        wait_for("the slow caller's request to reach the target", || {
            target.received().len() == 1
        });
        let hurried_started = Instant::now();
        let hurried = fetch(&shared, Some(&memo), URL_A, ms(50));
        assert!(hurried_started.elapsed() < Duration::from_millis(500));
        assert!(
            !slow.is_finished(),
            "the hurried caller waited for the answer"
        );
        (slow.join().unwrap(), hurried)
    });
    assert_eq!(
        (
            slow.body.as_str(),
            hurried.error.as_deref(),
            target.received().len()
        ),
        ("late", Some(DEADLINE), 1)
    );
    assert!(started.elapsed() >= Duration::from_millis(1_000));
}

#[test]
fn a_connection_error_answers_a_later_caller_whose_deadline_it_arrived_within() {
    let (transport, shared) = mock(MockResponse::error("TypeError: connection reset"));
    let memo = RequestMemo::new();
    let first = fetch(&shared, Some(&memo), URL_A, Opts::default());
    let second = fetch(&shared, Some(&memo), URL_A, Opts::default());
    assert_eq!(
        (
            first.error.as_deref(),
            second.error.as_deref(),
            transport.requests().len()
        ),
        (
            Some("TypeError: connection reset"),
            Some("TypeError: connection reset"),
            1
        )
    );
}

#[test]
fn a_body_past_the_memo_cap_reaches_its_reader_but_is_asked_for_again_by_the_next_reader() {
    let big = vec![b'x'; 1024 * 1024 + 1];
    let (transport, shared) = mock(MockResponse::ok(200, &[], &big));
    let memo = RequestMemo::new();
    let first = fetch(&shared, Some(&memo), URL_A, Opts::default());
    let status_only = fetch(&shared, Some(&memo), URL_A, capped(0));
    assert_eq!(
        (
            first.body.len(),
            status_only.status,
            transport.requests().len()
        ),
        (big.len(), Some(200), 1)
    );
    let second = fetch(&shared, Some(&memo), URL_A, Opts::default());
    assert_eq!(
        (second.body.len(), transport.requests().len()),
        (big.len(), 2)
    );
    let stats = memo.stats();
    assert_eq!(
        (
            stats.sent,
            stats.reused,
            stats.retained_bytes,
            stats.over_body_cap,
            stats.over_total_cap
        ),
        (2, 1, 0, 2, 0)
    );
}

#[test]
fn the_memo_stops_holding_bodies_once_an_audit_holds_8_mib_of_them() {
    let megabyte = vec![b'y'; 1024 * 1024];
    let (transport, shared) = mock(MockResponse::ok(200, &[], &megabyte));
    let memo = RequestMemo::new();
    let urls: Vec<String> = (0..9)
        .map(|i| format!("https://example.com/page-{i}"))
        .collect();
    for url in &urls {
        fetch(&shared, Some(&memo), url, Opts::default());
    }
    fetch(&shared, Some(&memo), &urls[0], Opts::default());
    fetch(&shared, Some(&memo), &urls[8], Opts::default());
    assert_eq!(transport.requests().len(), 10);
    let stats = memo.stats();
    assert_eq!(
        (
            stats.sent,
            stats.reused,
            stats.retained_bytes,
            stats.bodies_retained,
            stats.largest_body_bytes,
            stats.over_body_cap,
            stats.over_total_cap
        ),
        (10, 1, 8 * megabyte.len(), 8, megabyte.len(), 0, 2)
    );
}

#[test]
fn a_memo_held_redirect_is_still_checked_by_the_guard_before_a_caller_follows_it() {
    let card = "https://example.com/.well-known/mcp.json";
    let metadata = "https://169.254.169.254/latest/meta-data/";
    let (transport, shared) = mock(MockResponse::ok(302, &[("location", metadata)], b""));
    let memo = RequestMemo::new();
    let kept = fetch(
        &shared,
        Some(&memo),
        card,
        Opts {
            follow_redirects: Some(false),
            max_body_bytes: Some(0),
            ..Opts::default()
        },
    );
    let followed = fetch(&shared, Some(&memo), card, Opts::default());
    assert_eq!(kept.status, Some(302));
    let error = followed.error.unwrap_or_default();
    assert!(error.contains("(redirect hop 1)"), "{error}");
    assert_eq!(received(&transport), [format!("GET {card}")]);
}

#[test]
fn a_memo_held_redirect_off_the_targets_network_is_refused_for_a_caller_that_follows_it() {
    let card = "https://example.com/.well-known/mcp.json";
    let private = "http://10.0.0.5/admin";
    let (transport, shared) = mock(MockResponse::ok(307, &[("location", private)], b""));
    let memo = RequestMemo::new();
    let kept = fetch(
        &shared,
        Some(&memo),
        card,
        Opts {
            follow_redirects: Some(false),
            ..Opts::default()
        },
    );
    let followed = fetch(&shared, Some(&memo), card, Opts::default());
    assert_eq!(kept.status, Some(307));
    assert_eq!(
        followed.error.as_deref(),
        Some("blocked: ipv4 10.0.0.5 is in blocked range 10.0.0.0/8 (redirect hop 1)")
    );
    assert_eq!(received(&transport), [format!("GET {card}")]);
}

#[test]
fn a_memo_held_redirect_to_http_is_refused_for_a_follower_and_kept_as_sent_for_a_keeper() {
    let card = "https://example.com/.well-known/mcp.json";
    let plain = "http://example.com/.well-known/mcp.json";
    let (transport, shared) = mock(MockResponse::ok(301, &[("location", plain)], b"Moved"));
    let memo = RequestMemo::new();
    let followed = fetch(&shared, Some(&memo), card, Opts::default());
    let kept = fetch(
        &shared,
        Some(&memo),
        card,
        Opts {
            follow_redirects: Some(false),
            ..Opts::default()
        },
    );
    assert_eq!(
        (followed.status, followed.error.as_deref(), followed.refused),
        (
            Some(301),
            Some("redirect refused: 301 to http://example.com/.well-known/mcp.json: not https"),
            Some(Refusal::InsecureScheme)
        )
    );
    assert_eq!(
        (kept.status, kept.body.as_str(), kept.error, kept.refused),
        (Some(301), "Moved", None, None)
    );
    assert_eq!(received(&transport), [format!("GET {card}")]);
}

#[test]
fn a_cross_origin_redirect_one_caller_kept_is_refused_for_a_caller_that_refuses_cross_origin_hops()
{
    let endpoint = "https://example.com/mcp";
    let elsewhere = "https://other.example.net/mcp";
    let (transport, shared) = mock(MockResponse::ok(307, &[("location", elsewhere)], b""));
    let memo = RequestMemo::new();
    let returned = fetch(
        &shared,
        Some(&memo),
        endpoint,
        Opts {
            cross_origin: Some(CrossOriginRedirects::Return),
            ..Opts::default()
        },
    );
    let refused = fetch(
        &shared,
        Some(&memo),
        endpoint,
        Opts {
            cross_origin: Some(CrossOriginRedirects::Refuse),
            ..Opts::default()
        },
    );
    assert_eq!(
        (returned.status, refused.error),
        (
            Some(307),
            Some(format!("redirect refused: 307 to {elsewhere}"))
        )
    );
    assert_eq!(received(&transport), [format!("GET {endpoint}")]);
}

#[test]
fn without_a_memo_every_call_reaches_the_target() {
    let (transport, shared) = mock(MockResponse::ok(200, &[], b"ok"));
    fetch(&shared, None, URL_A, Opts::default());
    fetch(&shared, None, URL_A, Opts::default());
    assert_eq!(transport.requests().len(), 2);
}

/// Method, URL, sorted headers and body of one outgoing request: what the
/// target sees.
fn request_of(request: &Request) -> String {
    let mut headers: Vec<String> = request
        .headers
        .iter()
        .map(|(name, value)| format!("{}: {value}", name.to_ascii_lowercase()))
        .collect();
    headers.sort();
    let body = String::from_utf8_lossy(request.body.as_deref().unwrap_or_default());
    serde_json::json!([
        request.method.to_ascii_uppercase(),
        request.url,
        headers,
        body
    ])
    .to_string()
}

#[test]
fn no_conformance_scenario_sends_a_request_it_already_sent() {
    let mut repeated: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for case in load_cases() {
        let transport = transport_for(&case.scenario);
        replay_on(
            &case.scenario,
            Arc::new(default_handlers()),
            transport.clone(),
        );
        let mut sent: HashMap<String, usize> = HashMap::new();
        for request in transport.requests() {
            *sent.entry(request_of(&request)).or_default() += 1;
        }
        let mut again: Vec<String> = sent
            .into_iter()
            .filter(|(_, count)| *count > 1)
            .map(|(request, count)| format!("x{count} {request}"))
            .collect();
        again.sort();
        if !again.is_empty() {
            repeated.insert(case.name, again);
        }
    }
    assert!(repeated.is_empty(), "{repeated:#?}");
}

#[test]
fn without_a_memo_a_status_only_read_lets_go_of_the_body_unread() {
    let (_, shared, dropped, _release) = stalls_after_headers(200, &[]);
    let started = Instant::now();
    let answer = fetch(&shared, None, URL_A, status_only(5_000));
    assert_eq!(
        (
            answer.status,
            answer.body.as_str(),
            dropped.load(Ordering::SeqCst)
        ),
        (Some(200), "", true)
    );
    assert!(started.elapsed() < Duration::from_secs(1));
}
