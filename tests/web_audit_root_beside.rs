//! The root fetch and discovery's first document reads run side by side, and
//! what the root turns out to be decides how long the reads keep: a root that
//! fails at the network level cuts them to the degraded budget, and one that
//! redirects to http ends the run. A stalling host costs no more than it would
//! with the reads waiting their turn.

#![cfg(feature = "web")]

use std::io::Cursor;
use std::io::{self, Read};
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use agentnative::web_audit::engine::discovery::API_CATALOG_PATH;
use agentnative::web_audit::engine::{
    DEGRADED_PER_CHECK_TIMEOUT, FetchHandle, HandlerSet, ProbeOutcome, ProbeStatus, RunInput,
    RunOutcome, run_web_audit,
};
use agentnative::web_audit::fetch::launch::Launch;
use agentnative::web_audit::fetch::memo::RequestMemo;
use agentnative::web_audit::fetch::{FetchInit, FetchOptions};
use agentnative::web_audit::headers::Headers;
use agentnative::web_audit::locality::Resolver;
use agentnative::web_audit::registry::MCP_DISCOVERY;
use agentnative::web_audit::transport::{Request, Response, Transport, TransportError};

const BASE: &str = "https://example.com/";
const DOCUMENT: &str = "/.well-known/mcp.json";

/// How long a held answer waits for what it is watching before giving up, so
/// an engine that never starts it fails the assertion instead of hanging.
const GIVE_UP: Duration = Duration::from_secs(1);

struct PublicResolver;

impl Resolver for PublicResolver {
    fn resolve(&self, _host: &str) -> Result<Vec<IpAddr>, String> {
        Ok(vec!["93.184.216.34".parse().unwrap()])
    }
}

#[derive(Default)]
struct Latch {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Latch {
    /// Whether the latch opened in time.
    fn wait(&self, at_most: Duration) -> bool {
        let until = Instant::now() + at_most;
        let mut open = self.open.lock().unwrap();
        while !*open {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            open = self.changed.wait_timeout(open, left).unwrap().0;
        }
        true
    }

    fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }
}

type Answer = dyn Fn(&str) -> Result<Response, TransportError> + Send + Sync;

struct Site {
    answer: Box<Answer>,
    seen: Mutex<Vec<String>>,
}

impl Transport for Site {
    fn send(&self, request: &Request) -> Result<Response, TransportError> {
        self.seen.lock().unwrap().push(request.url.clone());
        let path = url::Url::parse(&request.url).unwrap().path().to_string();
        (self.answer)(&path)
    }
}

fn site(
    answer: impl Fn(&str) -> Result<Response, TransportError> + Send + Sync + 'static,
) -> Arc<Site> {
    Arc::new(Site {
        answer: Box::new(answer),
        seen: Mutex::new(Vec::new()),
    })
}

fn answer(status: u16, headers: &[(&str, &str)], body: &str) -> Result<Response, TransportError> {
    Ok(Response {
        status,
        headers: Headers::from_pairs(headers.iter().copied()),
        body: Box::new(Cursor::new(body.as_bytes().to_vec())),
    })
}

fn html() -> Result<Response, TransportError> {
    answer(200, &[("content-type", "text/html")], "<html></html>")
}

fn refused() -> Result<Response, TransportError> {
    Err(TransportError::Verbatim("connection refused".to_string()))
}

fn input<'a>(transport: Arc<Site>) -> RunInput<'a> {
    let fetch = FetchHandle::new(transport, Arc::new(PublicResolver), "test-ua");
    let mut input = RunInput::new(BASE, fetch, Arc::new(HandlerSet::new()));
    input.per_check_timeout = Duration::from_secs(8);
    input.per_audit_deadline = Duration::from_secs(25);
    input
}

#[test]
fn a_document_read_goes_out_while_the_root_is_unanswered_and_the_root_reads_while_it_is() {
    // Each holds its answer until the other arrives, which only happens if both
    // are out at once.
    let root_out = Arc::new(Latch::default());
    let document_out = Arc::new(Latch::default());
    let root_saw_document = Arc::new(AtomicBool::new(false));
    let document_saw_root = Arc::new(AtomicBool::new(false));
    let (ro, dout, rs, ds) = (
        root_out.clone(),
        document_out.clone(),
        root_saw_document.clone(),
        document_saw_root.clone(),
    );
    let transport = site(move |path| match path {
        "/" => {
            ro.release();
            rs.store(dout.wait(GIVE_UP), Ordering::SeqCst);
            html()
        }
        DOCUMENT => {
            dout.release();
            ds.store(ro.wait(GIVE_UP), Ordering::SeqCst);
            answer(404, &[], "")
        }
        _ => answer(404, &[], ""),
    });
    let outcome = run_web_audit(input(transport));
    assert!(matches!(outcome, RunOutcome::Complete(_)));
    assert!(root_saw_document.load(Ordering::SeqCst));
    assert!(document_saw_root.load(Ordering::SeqCst));
}

#[test]
fn a_redirect_hop_of_a_first_read_waits_for_the_root_and_the_wait_is_not_its_budget() {
    // The hop's answer arrives after the read's own timeout counted from its
    // start, and after any grace on it: only a budget moved by the wait reaches it.
    let timeout = Duration::from_secs(2);
    let root_answered = Arc::new(AtomicBool::new(false));
    let hop_before_root = Arc::new(AtomicBool::new(false));
    let (answered, before) = (root_answered.clone(), hop_before_root.clone());
    let transport = site(move |path| match path {
        "/" => {
            thread::sleep(Duration::from_millis(1500));
            answered.store(true, Ordering::SeqCst);
            html()
        }
        DOCUMENT => answer(302, &[("location", "/moved")], ""),
        "/moved" => {
            before.store(!answered.load(Ordering::SeqCst), Ordering::SeqCst);
            thread::sleep(Duration::from_millis(900));
            answer(404, &[], "")
        }
        _ => answer(404, &[], ""),
    });
    let mut run = input(transport.clone());
    run.per_check_timeout = timeout;
    let RunOutcome::Complete(report) = run_web_audit(run) else {
        panic!("the root answered");
    };
    assert!(!hop_before_root.load(Ordering::SeqCst));
    assert!(
        transport
            .seen
            .lock()
            .unwrap()
            .iter()
            .any(|url| url.ends_with("/moved")),
        "the hop never went out"
    );
    let trail: Vec<_> = report
        .discovery
        .evidence
        .iter()
        .filter(|item| item.get("source").and_then(|s| s.as_str()) == Some(DOCUMENT))
        .collect();
    assert_eq!(
        trail.first().and_then(|item| item.get("status")),
        Some(&serde_json::json!(404)),
        "the read lost its answer to the wait for the root: {trail:?}"
    );
}

type AnswerUntilStopped =
    dyn Fn(&str, &Arc<Latch>) -> Result<Response, TransportError> + Send + Sync;

/// A target whose held requests and bodies end only when the run stops it.
struct UntilStopped {
    stopped: Arc<Latch>,
    open: AtomicUsize,
    answer: Box<AnswerUntilStopped>,
}

impl Transport for UntilStopped {
    fn send(&self, request: &Request) -> Result<Response, TransportError> {
        self.open.fetch_add(1, Ordering::SeqCst);
        let path = url::Url::parse(&request.url).unwrap().path().to_string();
        let answer = (self.answer)(&path, &self.stopped);
        self.open.fetch_sub(1, Ordering::SeqCst);
        answer
    }

    fn stop(&self) {
        self.stopped.release();
    }
}

struct BodyUntilStopped {
    stopped: Arc<Latch>,
    dropped: Arc<AtomicBool>,
}

impl Read for BodyUntilStopped {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        self.stopped.wait(Duration::from_secs(30));
        Err(io::Error::new(io::ErrorKind::ConnectionAborted, "stopped"))
    }
}

impl Drop for BodyUntilStopped {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

#[test]
fn a_run_ending_on_its_root_returns_only_once_every_request_it_started_has_ended() {
    let transport = Arc::new(UntilStopped {
        stopped: Arc::new(Latch::default()),
        open: AtomicUsize::new(0),
        answer: Box::new(|path, stopped| match path {
            "/" => answer(301, &[("location", "http://example.com/")], ""),
            DOCUMENT => {
                stopped.wait(Duration::from_secs(30));
                Err(TransportError::Network("stopped".to_string()))
            }
            _ => answer(404, &[], ""),
        }),
    });
    let fetch = FetchHandle::new(transport.clone(), Arc::new(PublicResolver), "test-ua");
    let started = Instant::now();
    let outcome = run_web_audit(RunInput::new(BASE, fetch, Arc::new(HandlerSet::new())));
    assert!(matches!(outcome, RunOutcome::Unreachable { .. }));
    assert_eq!(
        transport.open.load(Ordering::SeqCst),
        0,
        "a request outlived the run"
    );
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_scored_run_stops_the_body_reads_the_memo_runs_before_its_report_exists() {
    let dropped = Arc::new(AtomicBool::new(false));
    let body_dropped = dropped.clone();
    let transport = Arc::new(UntilStopped {
        stopped: Arc::new(Latch::default()),
        open: AtomicUsize::new(0),
        answer: Box::new(move |path, stopped| match path {
            "/" => html(),
            "/slow.md" => Ok(Response {
                status: 200,
                headers: Headers::new(),
                body: Box::new(BodyUntilStopped {
                    stopped: stopped.clone(),
                    dropped: body_dropped.clone(),
                }),
            }),
            _ => answer(404, &[], ""),
        }),
    });
    let mut set = HandlerSet::new();
    set.register("http", |check, ctx| {
        if check.id == "robots" {
            // A status-only read: its body is left to the memo.
            ctx.fetch.fetch(
                &format!("{}slow.md", ctx.base),
                &FetchInit::default(),
                &FetchOptions {
                    max_body_bytes: Some(0),
                    ..ctx.fetch_options(None)
                },
            );
        }
        ProbeOutcome::new(ProbeStatus::Absent, Vec::new())
    });
    let fetch = FetchHandle::new(transport, Arc::new(PublicResolver), "test-ua");
    let outcome = run_web_audit(RunInput::new(BASE, fetch, Arc::new(set)));
    assert!(matches!(outcome, RunOutcome::Complete(_)));
    assert!(
        dropped.load(Ordering::SeqCst),
        "the memo's body read outlived the run"
    );
}

#[test]
fn a_run_on_a_transport_that_ignores_the_stop_still_returns_soon_after() {
    let release = Arc::new(Latch::default());
    let held = release.clone();
    let transport = site(move |path| match path {
        "/" => answer(301, &[("location", "http://example.com/")], ""),
        DOCUMENT => {
            held.wait(Duration::from_secs(30));
            answer(404, &[], "")
        }
        _ => answer(404, &[], ""),
    });
    let started = Instant::now();
    let outcome = run_web_audit(input(transport));
    let elapsed = started.elapsed();
    release.release();
    assert!(matches!(outcome, RunOutcome::Unreachable { .. }));
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");
}

#[test]
fn a_root_redirecting_to_http_means_a_held_redirect_hop_is_never_sent() {
    let transport = site(move |path| match path {
        "/" => {
            thread::sleep(Duration::from_millis(40));
            answer(301, &[("location", "http://example.com/")], "")
        }
        DOCUMENT => answer(302, &[("location", "/moved")], ""),
        _ => answer(404, &[], ""),
    });
    let outcome = run_web_audit(input(transport.clone()));
    assert!(matches!(outcome, RunOutcome::Unreachable { .. }));
    let asked = transport.seen.lock().unwrap();
    assert!(
        !asked.iter().any(|url| url.ends_with("/moved")),
        "{asked:?}"
    );
}

#[test]
fn a_root_failing_at_the_network_level_cuts_a_read_in_flight_to_the_degraded_budget() {
    let release = Arc::new(Latch::default());
    let held = release.clone();
    let transport = site(move |path| match path {
        "/" => refused(),
        DOCUMENT => {
            held.wait(Duration::from_secs(30));
            answer(404, &[], "")
        }
        _ => answer(404, &[], ""),
    });
    let started = Instant::now();
    let outcome = run_web_audit(input(transport));
    let elapsed = started.elapsed();
    release.release();
    assert!(matches!(outcome, RunOutcome::Complete(_)));
    assert!(
        elapsed >= DEGRADED_PER_CHECK_TIMEOUT - Duration::from_millis(50),
        "{elapsed:?}"
    );
    assert!(
        elapsed < DEGRADED_PER_CHECK_TIMEOUT + Duration::from_millis(1_500),
        "the read kept its configured budget: {elapsed:?}"
    );
}

#[test]
fn a_target_that_answers_nothing_costs_its_root_timeout_not_that_plus_a_turn_for_the_reads() {
    let timeout = Duration::from_secs(1);
    let release = Arc::new(Latch::default());
    let held = release.clone();
    let transport = site(move |_| {
        held.wait(Duration::from_secs(30));
        answer(404, &[], "")
    });
    let mut run = input(transport);
    run.per_check_timeout = timeout;
    let started = Instant::now();
    let outcome = run_web_audit(run);
    let elapsed = started.elapsed();
    release.release();
    assert!(matches!(outcome, RunOutcome::Unreachable { .. }));
    // The root and the first reads together, then the common-path POSTs, then the
    // moment the run's end gives a transport that ignores its stop. A turn of its
    // own for the reads would add a whole timeout.
    assert!(
        elapsed < timeout * 2 + Duration::from_millis(250) + timeout / 2,
        "{elapsed:?}"
    );
}

#[test]
fn a_root_redirecting_to_http_ends_the_run_on_its_answer_and_stops_reads_in_flight() {
    let release = Arc::new(Latch::default());
    let held = release.clone();
    let transport = site(move |path| match path {
        "/" => answer(301, &[("location", "http://example.com/")], ""),
        DOCUMENT => {
            held.wait(Duration::from_secs(30));
            answer(404, &[], "")
        }
        _ => answer(404, &[], ""),
    });
    let started = Instant::now();
    let outcome = run_web_audit(input(transport.clone()));
    let elapsed = started.elapsed();
    release.release();
    let RunOutcome::Unreachable { reason, .. } = outcome else {
        panic!("a root redirecting to http scored");
    };
    assert!(reason.contains("redirects to http"), "{reason}");
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");
    let asked = transport.seen.lock().unwrap();
    assert!(
        asked.iter().all(|url| !url.starts_with("http:")),
        "{asked:?}"
    );
}

/// Blocks on one host's name, where no request timeout reaches.
struct StallsOn {
    host: &'static str,
    release: Arc<Latch>,
}

impl Resolver for StallsOn {
    fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, String> {
        if host == self.host {
            self.release.wait(Duration::from_secs(30));
        }
        Ok(vec!["93.184.216.34".parse().unwrap()])
    }
}

#[test]
fn a_first_read_stuck_on_a_name_lookup_does_not_hold_the_run_and_sends_nothing_when_it_ends() {
    let transport = site(|path| match path {
        "/" => html(),
        DOCUMENT => answer(302, &[("location", "https://stall.example/card")], ""),
        _ => answer(404, &[], ""),
    });
    let release = Arc::new(Latch::default());
    let resolver = StallsOn {
        host: "stall.example",
        release: release.clone(),
    };
    let fetch = FetchHandle::new(transport.clone(), Arc::new(resolver), "test-ua");
    let mut run = RunInput::new(BASE, fetch, Arc::new(HandlerSet::new()));
    run.per_check_timeout = Duration::from_millis(300);
    run.per_audit_deadline = Duration::from_secs(20);
    let started = Instant::now();
    let outcome = run_web_audit(run);
    let elapsed = started.elapsed();
    assert!(matches!(outcome, RunOutcome::Complete(_)));
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
    // The abandoned read goes on once its lookup ends. Time for it to send, if it
    // were going to.
    release.release();
    thread::sleep(Duration::from_millis(100));
    let asked = transport.seen.lock().unwrap();
    assert!(
        !asked.iter().any(|url| url.contains("stall.example")),
        "{asked:?}"
    );
}

#[test]
fn a_redirect_hop_a_cut_has_already_spent_is_not_sent_and_reads_as_the_deadline() {
    // The first answer lands past the cut's budget, before the root has said
    // anything, so what the root's failure leaves the hop is nothing.
    let answered = Arc::new(Latch::default());
    let opened = answered.clone();
    let transport = site(move |path| match path {
        DOCUMENT => {
            thread::sleep(Duration::from_millis(200));
            opened.release();
            answer(302, &[("location", "/moved")], "")
        }
        _ => answer(404, &[], ""),
    });
    let memo = RequestMemo::new();
    let launch = Launch::new();
    let read = {
        let fetch = FetchHandle::new(transport.clone(), Arc::new(PublicResolver), "test-ua")
            .with_memo(memo.clone());
        let launch = launch.clone();
        thread::spawn(move || {
            fetch.fetch(
                &format!("https://example.com{DOCUMENT}"),
                &FetchInit::default(),
                &FetchOptions {
                    timeout: Duration::from_secs(5),
                    cut_to: Some(Duration::from_millis(100)),
                    launch: Some(launch),
                    ..FetchOptions::default()
                },
            )
        })
    };
    assert!(answered.wait(GIVE_UP * 5));
    // Time for the answer to reach the hop that waits on the root.
    thread::sleep(Duration::from_millis(50));
    memo.cut();
    launch.root_settled();
    let response = read.join().expect("the read ends");
    memo.close();
    assert_eq!(
        response.error.as_deref(),
        Some("TimeoutError: deadline exceeded"),
        "{response:?}"
    );
    let asked = transport.seen.lock().unwrap();
    assert!(
        !asked.iter().any(|url| url.ends_with("/moved")),
        "{asked:?}"
    );
}

/// Every name lookup after the run's first two, its classification and the root's
/// own, takes a moment, so the root's answer comes back ahead of the first reads.
#[derive(Default)]
struct SlowAfterTheRoot(AtomicUsize);

impl Resolver for SlowAfterTheRoot {
    fn resolve(&self, _host: &str) -> Result<Vec<IpAddr>, String> {
        if self.0.fetch_add(1, Ordering::SeqCst) >= 2 {
            thread::sleep(Duration::from_millis(100));
        }
        Ok(vec!["93.184.216.34".parse().unwrap()])
    }
}

#[test]
fn a_root_refusing_ahead_of_the_first_reads_still_lets_every_one_go_out() {
    let transport = site(|path| match path {
        "/" => answer(301, &[("location", "http://example.com/")], ""),
        _ => answer(404, &[], ""),
    });
    let fetch = FetchHandle::new(
        transport.clone(),
        Arc::new(SlowAfterTheRoot::default()),
        "test-ua",
    );
    let outcome = run_web_audit(RunInput::new(BASE, fetch, Arc::new(HandlerSet::new())));
    assert!(matches!(outcome, RunOutcome::Unreachable { .. }));
    let asked = transport.seen.lock().unwrap();
    let wanted = MCP_DISCOVERY
        .well_known
        .iter()
        .chain([&MCP_DISCOVERY.ai_catalog, &API_CATALOG_PATH]);
    for path in wanted {
        assert!(
            asked.iter().any(|url| url.ends_with(path)),
            "{path} never went out: {asked:?}"
        );
    }
}
